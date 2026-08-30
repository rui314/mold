//! Input sections and the records the linker parses out of them.

use std::cell::Cell;
use std::fmt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use bstr::BStr;

use crate::arch::{Arch, Family};
use crate::args::UnresolvedKind;
use crate::chunks::merged::{MergedSection, MergedSectionId};
use crate::chunks::OutputSectionId;
use crate::context::Context;
use crate::diagnostics::Diagnostics;
use crate::elf::*;
use crate::input_files::{ObjId, ObjectFile};
use crate::symbol::{Symbol, SymbolId, NEEDS_CANONICAL, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSDESC};
use crate::util::compress::{zlib_decompress, zstd_decompress};
use crate::util::concurrent_map::EntryId;
use crate::util::hyperloglog::HyperLogLog;
use crate::util::{self, cstr_at, leak_bytes};
use crate::{error, fatal};

/// Identifies an input section by its file and section index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SectionRef {
    pub file: ObjId,
    pub shndx: u32,
}

impl SectionRef {
    #[inline]
    pub(crate) fn encode(self) -> u64 {
        (u64::from(self.file.0) << 32) | u64::from(self.shndx)
    }

    #[inline]
    pub(crate) fn decode(value: u64) -> SectionRef {
        SectionRef {
            file: ObjId((value >> 32) as u32),
            shndx: value as u32,
        }
    }
}

/// A compact arena pointer to an input section. C++ mold stores output-section
/// members as `ArenaPtr<InputSection>` rather than looking them up by file and
/// ELF section index.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InputSectionId(u32);

const _: () = assert!(std::mem::size_of::<InputSectionId>() == 4);

impl InputSectionId {
    /// A placeholder used only while a member array is being filled.
    pub(crate) const NONE: InputSectionId = InputSectionId(0);
}

/// Identifies a section fragment in a merged output section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FragmentRef {
    pub section: MergedSectionId,
    pub entry: EntryId,
}

/// A byte range that RISC-V or LoongArch relaxation removed from a
/// section: the contents at and after `offset` shift towards the
/// beginning of the section by `delta` bytes in total.
#[derive(Clone, Copy, Debug)]
pub struct RelocDelta {
    pub offset: u64,
    pub delta: i64,
}

const IS_ALIVE: u8 = 1 << 0;
const IS_VISITED: u8 = 1 << 1;
const IS_ADDRESS_TAKEN: u8 = 1 << 2;
const IS_UNCOMPRESSED: u8 = 1 << 3;
const IS_ICF_REMOVED: u8 = 1 << 4;
const HAS_RELOCS: u8 = 1 << 5;
const IS_NOBITS: u8 = 1 << 6;
const NO_RELSEC: u32 = u32::MAX;
const NO_FDE: u32 = u32::MAX;

/// Rarely used input-section state, allocated only when needed.
#[derive(Debug, Default)]
struct InputSectionExtras {
    /// The `.ARM.exidx` section describing this section (ARM32).
    exidx: Option<u32>,

    /// Bytes removed by linker relaxation (RISC-V, LoongArch).
    r_deltas: Box<[RelocDelta]>,
}

/// A section of an input object file.
#[derive(Debug)]
pub struct InputSection {
    pub file: ObjId,
    pub shndx: u32,

    /// Offset in the owner file's section-name string table.
    name_offset: u32,

    // UINT16_MAX means that name() must scan the remaining suffix.
    namelen: u16,

    /// The section header's flags; the rest of the header is read from the
    /// file when needed.
    pub sh_flags: u64,

    /// The section contents as they appear in the file, or the
    /// decompressed contents once [`Self::uncompress`] has run.
    contents: usize,

    /// The size after decompression; may shrink during relaxation.
    pub sh_size: u64,
    p2align: AtomicU8,

    pub output_section: Option<OutputSectionId>,

    /// Offset within the output section. During ICF it temporarily holds the
    /// dense section index and then the encoded leader, as in C++ mold's
    /// union. Sections are processed in parallel, so it is atomic.
    offset: AtomicU64,

    /// Index of the relocation section applying to this section, or
    /// `NO_RELSEC`. Rewritten relocations live at the same index in the
    /// owner file's side table.
    relsec_idx: u32,

    /// First FDE in the owner file's contiguous run for this section.
    pub(crate) fde_begin: u32,

    /// A self-relative pointer to rarely used state, in four-byte units.
    extra: i32,

    flags: AtomicU8,
}

const _: () = assert!(std::mem::size_of::<InputSection>() == 64);

#[inline]
fn to_p2align(alignment: u64) -> u8 {
    if alignment == 0 {
        0
    } else {
        alignment.trailing_zeros() as u8
    }
}

impl InputSection {
    #[inline]
    pub fn new<E: Arch>(
        diag: &Diagnostics,
        file: &ObjectFile,
        file_id: ObjId,
        shndx: u32,
        shdr: &ElfShdr,
        name: &'static BStr,
    ) -> InputSection {
        let contents: &'static [u8] =
            if shdr.sh_type == SHT_NOBITS || (shndx as usize) >= file.num_elf_sections {
                &[]
            } else {
                file.base.section_contents_from_shdr(diag, shdr)
            };

        let (sh_size, p2align) = if shdr.sh_flags & SHF_COMPRESSED as u64 != 0 {
            let chdr = ElfChdr::parse::<E>(contents);
            (chdr.ch_size, to_p2align(chdr.ch_addralign))
        } else {
            (shdr.sh_size, to_p2align(shdr.sh_addralign))
        };

        let mut isec = InputSection {
            file: file_id,
            shndx,
            name_offset: if (shndx as usize) < file.num_elf_sections {
                shdr.sh_name
            } else {
                0
            },
            namelen: name.len().min(u16::MAX as usize) as u16,
            sh_flags: shdr.sh_flags,
            contents: contents.as_ptr() as usize,
            sh_size,
            p2align: AtomicU8::new(p2align),
            output_section: None,
            offset: AtomicU64::new(u64::MAX),
            relsec_idx: NO_RELSEC,
            fde_begin: NO_FDE,
            extra: 0,
            flags: AtomicU8::new(IS_ALIVE),
        };

        // Compressed sections are usually decompressed straight into the
        // output file, but REL-type targets read relocation addends from
        // section contents, so those need the contents early. SH4 stores
        // addends in sections despite being RELA.
        if !E::IS_RELA || E::FAMILY == Family::Sh4 {
            isec.uncompress::<E>(diag, file, name, shdr.sh_size as usize);
        }
        isec
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & IS_ALIVE != 0
    }

    #[inline]
    pub fn section_ref(&self) -> SectionRef {
        SectionRef {
            file: self.file,
            shndx: self.shndx,
        }
    }

    #[inline]
    pub fn is_visited(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & IS_VISITED != 0
    }

    #[inline]
    pub fn p2align(&self) -> u8 {
        self.p2align.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn update_p2align(&self, p2align: u8) {
        self.p2align.fetch_max(p2align, Ordering::Relaxed);
    }

    #[inline]
    pub fn is_address_taken(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & IS_ADDRESS_TAKEN != 0
    }

    #[inline]
    pub fn is_uncompressed(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & IS_UNCOMPRESSED != 0
    }

    #[inline]
    pub fn is_icf_removed(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & IS_ICF_REMOVED != 0
    }

    #[inline]
    pub fn sh_type(&self, file: &ObjectFile) -> u32 {
        self.sh_type_from(&file.base.shdrs)
    }

    #[inline]
    pub(crate) fn sh_type_from(&self, shdrs: &ShdrTable<'_>) -> u32 {
        if self.flags.load(Ordering::Relaxed) & IS_NOBITS != 0 {
            SHT_NOBITS
        } else {
            shdrs.sh_type(self.shndx as usize)
        }
    }

    #[inline]
    pub fn set_nobits(&self) {
        self.flags.fetch_or(IS_NOBITS, Ordering::Relaxed);
    }

    #[inline]
    pub fn is_compressed(&self) -> bool {
        self.sh_flags & SHF_COMPRESSED as u64 != 0 && !self.is_uncompressed()
    }

    /// Marks the section visited, returning true if it was not already.
    #[inline]
    pub fn visit(&self) -> bool {
        // Most sections are referenced many times, so check with a plain
        // load before the atomic read-modify-write.
        if self.is_visited() {
            return false;
        }
        self.flags.fetch_or(IS_VISITED, Ordering::Relaxed) & IS_VISITED == 0
    }

    #[inline]
    pub fn set_visited(&self) {
        self.flags.fetch_or(IS_VISITED, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_address_taken(&self) {
        self.flags.fetch_or(IS_ADDRESS_TAKEN, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_icf_removed(&self) {
        self.flags.fetch_or(IS_ICF_REMOVED, Ordering::Relaxed);
    }

    /// Marks the section dead. Returns true if it was alive; the caller is
    /// then responsible for killing the section's FDEs.
    #[inline]
    pub fn kill(&self) -> bool {
        self.flags.fetch_and(!IS_ALIVE, Ordering::Relaxed) & IS_ALIVE != 0
    }

    #[inline]
    pub fn revive(&self) {
        self.flags.fetch_or(IS_ALIVE, Ordering::Relaxed);
    }

    /// The section contents, decompressed if [`Self::uncompress`] has run.
    #[inline]
    pub fn contents(&self) -> &'static [u8] {
        if self.contents == 0 {
            return &[];
        }
        debug_assert!(!self.is_compressed());
        // SAFETY: contents points into an input mapping or a leaked
        // decompression buffer, both live for the complete link. sh_size is
        // their current logical size except during relaxation, whose callers
        // use original_contents below.
        unsafe { std::slice::from_raw_parts(self.contents as *const u8, self.sh_size as usize) }
    }

    /// The complete input contents, including bytes removed by relaxation.
    #[inline]
    pub fn original_contents(&self, file: &ObjectFile) -> &'static [u8] {
        if self.contents == 0 {
            return &[];
        }
        debug_assert!(!self.is_compressed());
        let size = if self.is_uncompressed() {
            self.sh_size
        } else {
            file.base.shdrs.sh_offset_and_size(self.shndx as usize).1
        };
        // SAFETY: as in contents; an uncompressed buffer has sh_size bytes,
        // while an ordinary input view has its ELF section header's size.
        unsafe { std::slice::from_raw_parts(self.contents as *const u8, size as usize) }
    }

    /// The section name, read from the owner file's string table.
    #[inline]
    pub fn name(&self, file: &ObjectFile) -> &'static BStr {
        self.name_in(file.base.shstrtab, file.num_elf_sections)
    }

    #[inline]
    pub(crate) fn name_in(
        &self,
        shstrtab: &'static [u8],
        num_elf_sections: usize,
    ) -> &'static BStr {
        if self.shndx as usize >= num_elf_sections {
            let name: &[u8] = if self.sh_flags & SHF_TLS as u64 != 0 {
                b".tls_common"
            } else {
                b".common"
            };
            return BStr::new(name);
        }

        let start = self.name_offset as usize;
        if self.namelen == u16::MAX {
            BStr::new(cstr_at(shstrtab, start))
        } else {
            BStr::new(&shstrtab[start..start + self.namelen as usize])
        }
    }

    /// The offset within the output section.
    #[inline]
    pub fn offset(&self) -> u64 {
        self.offset.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_offset(&self, offset: u64) {
        self.offset.store(offset, Ordering::Relaxed);
    }

    /// The dense section index used while ICF constructs its graph.
    #[inline]
    pub(crate) fn icf_index(&self) -> Option<u32> {
        let index = self.offset();
        (index != u64::MAX).then(|| u32::try_from(index).expect("invalid ICF section index"))
    }

    #[inline]
    pub(crate) fn set_icf_index(&self, index: u32) {
        self.set_offset(index as u64);
    }

    #[inline]
    pub fn addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        let osec = self.output_section.expect("section has no output section");
        ctx.output_sections[osec.index()].hdr.shdr.sh_addr + self.offset()
    }

    #[inline]
    pub fn is_alloc(&self) -> bool {
        self.sh_flags & SHF_ALLOC as u64 != 0
    }

    /// Sort key for deterministic ordering: files are processed in
    /// command line order, sections in file order.
    pub fn priority(&self, file: &ObjectFile) -> u64 {
        ((file.base.priority as u64) << 32) | self.shndx as u64
    }

    /// Replaces compressed contents with a decompressed copy. `file` is
    /// the owning file's name, for diagnostics.
    pub fn uncompress<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        file: &dyn fmt::Display,
        name: &BStr,
        input_size: usize,
    ) {
        if !self.is_compressed() {
            return;
        }
        let mut buf = vec![0u8; self.sh_size as usize];
        self.copy_contents_to::<E>(diag, file, name, input_size, &mut buf);
        self.contents = leak_bytes(buf).as_ptr() as usize;
        self.flags.fetch_or(IS_UNCOMPRESSED, Ordering::Relaxed);
    }

    /// Copies the (decompressed) contents into `buf`, which must be
    /// `sh_size` bytes long.
    pub fn copy_contents_to<E: Arch>(
        &self,
        diag: &Diagnostics,
        file: &dyn fmt::Display,
        name: &BStr,
        input_size: usize,
        buf: &mut [u8],
    ) {
        if !self.is_compressed() {
            // SAFETY: an ordinary input section has at least sh_size bytes,
            // and replacement buffers are created with that same size.
            let contents = unsafe {
                std::slice::from_raw_parts(self.contents as *const u8, self.sh_size as usize)
            };
            buf.copy_from_slice(&contents[..buf.len()]);
            return;
        }

        let hdr_size = ElfChdr::size::<E>();
        if input_size < hdr_size {
            fatal!(diag, "{file}:({name}): corrupted compressed section");
        }
        // SAFETY: input_size comes from this section's validated ELF header.
        let contents =
            unsafe { std::slice::from_raw_parts(self.contents as *const u8, input_size) };
        let chdr = ElfChdr::parse::<E>(contents);
        let data = &contents[hdr_size..];

        let result = match chdr.ch_type {
            ELFCOMPRESS_ZLIB => zlib_decompress(data, buf),
            ELFCOMPRESS_ZSTD => zstd_decompress(data, buf),
            ty => fatal!(
                diag,
                "{file}:({name}): unsupported compression type: 0x{ty:x}"
            ),
        };
        if let Err(msg) = result {
            fatal!(diag, "{file}:({name}): uncompress failed: {msg}");
        }
    }

    /// Replaces the contents, e.g. after reversing a `.ctors` section.
    pub fn set_contents(&mut self, contents: &'static [u8]) {
        debug_assert_eq!(contents.len(), self.sh_size as usize);
        self.contents = contents.as_ptr() as usize;
    }

    /// Drops the contents, for a section converted to BSS.
    pub fn clear_contents(&mut self) {
        self.contents = 0;
    }

    /// FDE records describing this section. FDEs for one input section are
    /// contiguous, and the last record carries the end marker, as in C++.
    #[inline]
    pub fn fdes<'a>(&self, file: &'a ObjectFile) -> &'a [FdeRecord] {
        if self.fde_begin == NO_FDE {
            return &[];
        }
        let begin = self.fde_begin as usize;
        let end = begin
            + file.fdes[begin..]
                .iter()
                .position(|fde| fde.is_last)
                .expect("unterminated FDE run")
            + 1;
        &file.fdes[begin..end]
    }

    /// The addend of a relocation against this section.
    #[inline]
    pub fn rel_addend<E: Arch>(&self, rel: &ElfRel) -> i64 {
        if E::IS_RELA && E::FAMILY != Family::Sh4 {
            rel.r_addend
        } else {
            E::get_addend(&self.contents()[rel.r_offset as usize..], rel)
        }
    }

    /// Formats the section as `file:(name)` for diagnostics.
    pub fn display<'a>(&'a self, file: &'a ObjectFile) -> impl fmt::Display + 'a {
        struct Display<'a>(&'a InputSection, &'a ObjectFile);
        impl fmt::Display for Display<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}:({})", self.1, self.0.name(self.1))
            }
        }
        Display(self, file)
    }

    /// Whether a relocation can be encoded in the RELR format.
    #[inline]
    pub fn is_relr_reloc<E: Arch>(&self, ctx: &Context<E>, rel: &ElfRel) -> bool {
        ctx.args.pack_dyn_relocs_relr
            && (1u64 << self.p2align()).is_multiple_of(E::WORD_SIZE as u64)
            && rel.r_offset.is_multiple_of(E::WORD_SIZE as u64)
    }

    /// Returns the name of the function containing `offset`.
    pub fn func_name<E: Arch>(&self, ctx: &Context<E>, offset: u64) -> Option<String> {
        let file = &ctx.objs[self.file.index()];
        for &id in &file.base.symbols {
            let sym = &ctx.symbols[id];
            if sym.file() != Some(self.file.into()) {
                continue;
            }
            let esym = &sym.esym(ctx);
            if esym.st_shndx as u32 == self.shndx
                && esym.st_type() == STT_FUNC
                && esym.st_value <= offset
                && offset < esym.st_value + esym.st_size
            {
                return Some(sym.to_string());
            }
        }
        None
    }

    /// Whether the file has a relocation section for this section.
    #[inline]
    pub fn has_relsec(&self) -> bool {
        self.relsec_idx != NO_RELSEC
    }

    /// Attaches the section's relocation section.
    pub fn set_relsec(&mut self, relsec_idx: u32, has_relocs: bool) {
        debug_assert!(!self.has_relsec());
        debug_assert_ne!(relsec_idx, NO_RELSEC);
        self.relsec_idx = relsec_idx;
        if has_relocs {
            self.flags.fetch_or(HAS_RELOCS, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn relsec_idx(&self) -> Option<u32> {
        (self.relsec_idx != NO_RELSEC).then_some(self.relsec_idx)
    }

    #[inline]
    pub fn has_relocations(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & HAS_RELOCS != 0
    }

    #[inline]
    pub fn rels<'a, E: Layout>(&self, file: &'a ObjectFile) -> Rels<'a, E> {
        file.relocations::<E>(self.relsec_idx())
    }

    /// Visits relocations without materializing a deferred CREL table.
    #[inline]
    pub fn for_each_reloc<E: Arch>(&self, ctx: &Context<E>, f: impl FnMut(ElfRel, usize)) {
        let file = &ctx.objs[self.file.index()];
        file.for_each_relocation::<E>(&ctx.diag, self.relsec_idx(), f);
    }

    #[inline]
    fn extra_ptr(&self) -> Option<*mut InputSectionExtras> {
        if self.extra == 0 {
            return None;
        }
        let field = std::ptr::addr_of!(self.extra).cast::<u8>();
        // SAFETY: a nonzero offset was created from an extras allocation in
        // the same arena and InputSection has not moved since then.
        Some(unsafe {
            field
                .offset(self.extra as isize * 4)
                .cast::<InputSectionExtras>()
                .cast_mut()
        })
    }

    fn extra_mut(&mut self, arena: &SectionArena) -> &mut InputSectionExtras {
        if self.extra == 0 {
            let ptr = arena.insert_extra(InputSectionExtras::default());
            let field = std::ptr::addr_of!(self.extra).cast::<u8>();
            let bytes = ptr as isize - field as isize;
            debug_assert_eq!(bytes % 4, 0);
            self.extra = i32::try_from(bytes / 4).expect("input-section extras are too far away");
            debug_assert_ne!(self.extra, 0);
        }
        // SAFETY: extra_mut has exclusive access to the section and hence to
        // its uniquely owned extras record.
        unsafe { &mut *self.extra_ptr().unwrap() }
    }

    #[inline]
    pub fn icf_leader(&self) -> Option<SectionRef> {
        self.is_icf_removed().then(|| self.icf_leader_in_round())
    }

    #[inline]
    pub(crate) fn icf_leader_in_round(&self) -> SectionRef {
        SectionRef::decode(self.offset())
    }

    #[inline]
    pub fn set_icf_leader(&self, leader: SectionRef) {
        self.set_offset(leader.encode());
    }

    #[inline]
    pub fn exidx(&self) -> Option<u32> {
        self.extra_ptr()
            // SAFETY: a published extras pointer remains valid with its arena.
            .and_then(|extra| unsafe { (*extra).exidx })
    }

    pub fn set_exidx(&mut self, exidx: u32, arena: &SectionArena) {
        self.extra_mut(arena).exidx = Some(exidx);
    }

    #[inline]
    pub fn r_deltas(&self) -> &[RelocDelta] {
        self.extra_ptr()
            // SAFETY: a published extras pointer remains valid with its arena.
            .map_or(&[], |extra| unsafe { &(*extra).r_deltas })
    }

    pub fn set_r_deltas(&mut self, deltas: Box<[RelocDelta]>, arena: &SectionArena) {
        debug_assert!(!deltas.is_empty());
        self.extra_mut(arena).r_deltas = deltas;
    }

    /// Reports a relocation whose value doesn't fit in the field.
    #[inline(always)]
    pub fn check_range<E: Arch>(
        &self,
        ctx: &Context<E>,
        rel_idx: usize,
        val: i64,
        lo: i64,
        hi: i64,
    ) {
        if val < lo || hi <= val {
            self.report_out_of_range::<E>(ctx, rel_idx, val, lo, hi);
        }
    }

    #[cold]
    fn report_out_of_range<E: Arch>(
        &self,
        ctx: &Context<E>,
        rel_idx: usize,
        val: i64,
        lo: i64,
        hi: i64,
    ) {
        let file = &ctx.objs[self.file.index()];
        let rel = self.rels::<E>(file).at(rel_idx);
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
        error!(
            ctx,
            "{}: relocation {} against {} out of range: {val} is not in [{lo}, {hi})",
            self.display(file),
            rel.type_name::<E>(),
            sym
        );
    }

    /// For a relocation in a non-allocated section, finds the section
    /// fragment it refers to, if it refers to a mergeable section.
    #[inline]
    pub fn fragment<E: Arch>(&self, ctx: &Context<E>, rel: &ElfRel) -> Option<(FragmentRef, i64)> {
        debug_assert!(!self.is_alloc());
        let file = &ctx.objs[self.file.index()];
        self.fragment_with_file::<E>(file, rel)
    }

    /// Like [`Self::fragment`], using an owner the caller already loaded.
    #[inline]
    pub fn fragment_with_file<E: Arch>(
        &self,
        file: &ObjectFile,
        rel: &ElfRel,
    ) -> Option<(FragmentRef, i64)> {
        debug_assert!(!self.is_alloc());
        let sym_idx = rel.r_sym as usize;
        if sym_idx >= file.base.elf_syms.len() {
            return None;
        }
        let st_shndx = file.base.elf_syms.st_shndx_in::<E>(sym_idx);
        if matches!(st_shndx as u32, SHN_UNDEF | SHN_ABS | SHN_COMMON) {
            return None;
        }
        let shndx = file.shndx_from(sym_idx, st_shndx);
        let m = file.mergeable_section(shndx)?;
        let esym = file.base.elf_syms.at_in::<E>(sym_idx);
        let addend = self.rel_addend::<E>(rel);
        if esym.st_type() == STT_SECTION {
            let (frag, offset) = m.fragment(esym.st_value.wrapping_add(addend as u64))?;
            Some((
                FragmentRef {
                    section: m.parent,
                    entry: frag,
                },
                offset,
            ))
        } else {
            let (frag, offset) = m.fragment(esym.st_value)?;
            Some((
                FragmentRef {
                    section: m.parent,
                    entry: frag,
                },
                offset + addend,
            ))
        }
    }

    /// Returns a tombstone value if `sym` refers to a dead debug section.
    ///
    /// Linkers don't remove debug info for functions eliminated by COMDAT
    /// deduplication or ICF, since that would require parsing the whole
    /// debug section. Instead, dead debug records get a tombstone value
    /// that debuggers know to skip.
    #[inline(always)]
    pub fn tombstone<E: Arch>(
        &self,
        ctx: &Context<E>,
        sym: &Symbol,
        frag: Option<FragmentRef>,
    ) -> Option<u64> {
        let file = &ctx.objs[self.file.index()];
        self.tombstone_with_file(ctx, file, sym, frag)
    }

    /// Like [`Self::tombstone`], using an owner the caller already loaded.
    #[inline(always)]
    pub fn tombstone_with_file<E: Arch>(
        &self,
        ctx: &Context<E>,
        file: &ObjectFile,
        sym: &Symbol,
        frag: Option<FragmentRef>,
    ) -> Option<u64> {
        if frag.is_some() {
            return None;
        }

        let isec = sym.input_section_ref();
        let discarded = sym.file().is_none() && sym.name().is_empty() && !sym.is_fragment_dummy();
        let discarded = discarded && std::ptr::eq(sym, &ctx.symbols[SymbolId::DISCARDED_COMDAT]);

        match isec {
            None if !discarded => return None,
            Some(isec) if isec.is_alive() => return None,
            _ => {}
        }

        let name: &[u8] = self.name(file);
        if !name.starts_with(b".debug_") {
            return None;
        }

        // A section folded by ICF keeps real values in .debug_line so that
        // users can set breakpoints inside the merged section.
        if let Some(isec) = isec {
            if isec.is_icf_removed() && name == b".debug_line" {
                return None;
            }
        }

        // 0 is an invalid value in most debug info sections, so we use it
        // as a tombstone. .debug_loc and .debug_ranges reserve 0 as the
        // terminator marker, so those use 1.
        Some(if name == b".debug_loc" || name == b".debug_ranges" {
            1
        } else {
            0
        })
    }

    /// Checks whether the symbol a relocation refers to is unresolved and
    /// records the error if so. Returns true if the relocation must be
    /// skipped.
    #[inline(always)]
    pub fn record_undef_error<E: Arch>(&self, ctx: &Context<E>, rel: &ElfRel) -> bool {
        let file = &ctx.objs[self.file.index()];
        self.record_undef_error_with_file(ctx, file, rel)
    }

    /// Like [`Self::record_undef_error`], using an owner the caller already
    /// loaded.
    #[inline(always)]
    pub fn record_undef_error_with_file<E: Arch>(
        &self,
        ctx: &Context<E>,
        file: &ObjectFile,
        rel: &ElfRel,
    ) -> bool {
        // A relocation may refer to a linker-synthesized symbol for a
        // section fragment, which is always resolved.
        let sym_idx = rel.r_sym as usize;
        if sym_idx >= file.base.elf_syms.len() {
            return false;
        }
        let sym_id = file.base.symbols[sym_idx];
        let sym = &ctx.symbols[sym_id];

        // A global symbol in a discarded COMDAT group should resolve to the
        // corresponding symbol in the prevailing group. If it doesn't, the
        // object files violate the One Definition Rule.
        if sym.file().is_none() && sym_id != SymbolId::DISCARDED_COMDAT {
            self.report_discarded_comdat(ctx, file, rel, sym);
            return true;
        }

        // A non-weak undefined symbol must be promoted to an imported
        // symbol or resolved to a definition. Otherwise, we need to report
        // an error or a warning.
        //
        // Every ELF file has an absolute local symbol as its first symbol.
        // Referring to that symbol is always valid.
        let st_info = file.base.elf_syms.st_info_in::<E>(sym_idx);
        let is_undef = file.base.elf_syms.st_shndx_in::<E>(sym_idx) as u32 == SHN_UNDEF
            && (st_info >> 4) as u32 != STB_WEAK
            && sym.sym_idx != 0;

        if is_undef && sym.is_undef() {
            match ctx.args.unresolved_symbols {
                UnresolvedKind::Error if !sym.is_imported() => {
                    self.record_undefined_reference(ctx, file, rel, sym_id);
                    return true;
                }
                UnresolvedKind::Warn => self.record_undefined_reference(ctx, file, rel, sym_id),
                _ => {}
            }
        }
        false
    }

    #[cold]
    fn report_discarded_comdat<E: Arch>(
        &self,
        ctx: &Context<E>,
        file: &ObjectFile,
        rel: &ElfRel,
        sym: &Symbol,
    ) {
        let mut msg = format!(
            "{}: {} refers to a discarded COMDAT section probably due to an ODR violation",
            self.display(file),
            sym
        );
        if let Some(owner) = find_comdat_owner(ctx, file, rel.r_sym as usize) {
            msg += &format!(
                "\n>>> prevailing definition is in {}",
                ctx.objs[owner.index()]
            );
        }
        error!(ctx, "{msg}");
    }

    #[cold]
    fn record_undefined_reference<E: Arch>(
        &self,
        ctx: &Context<E>,
        file: &ObjectFile,
        rel: &ElfRel,
        sym_id: SymbolId,
    ) {
        let mut msg = String::new();
        match file.base.source_name(&ctx.symbols) {
            Some(source) => msg += &format!(">>> referenced by {}\n", util::display(source)),
            None => msg += &format!(">>> referenced by {}\n", self.display(file)),
        }
        msg += &format!(">>>               {file}");
        if let Some(func) = self.func_name(ctx, rel.r_offset) {
            msg += &format!(":({func})");
        }
        msg.push('\n');
        ctx.undef_errors
            .lock()
            .unwrap()
            .entry(sym_id)
            .or_default()
            .push(msg);
    }

    /// Copies the section to `buf` and applies relocations.
    pub fn write_to<E: Arch>(&self, ctx: &Context<E>, buf: &mut [u8]) {
        let file = &ctx.objs[self.file.index()];
        if self.sh_type(file) == SHT_NOBITS || self.sh_size == 0 {
            return;
        }
        let buf = &mut buf[..self.sh_size as usize];
        let input_size = file.base.shdrs.sh_offset_and_size(self.shndx as usize).1 as usize;

        // On RISC-V and LoongArch, relaxation may have removed bytes from
        // the middle of the section, so a relaxed section is copied piecewise.
        if self.r_deltas().is_empty() {
            self.copy_contents_to::<E>(&ctx.diag, file, self.name(file), input_size, buf);
        } else {
            let contents = self.original_contents(file);
            let deltas = self.r_deltas();
            buf[..deltas[0].offset as usize]
                .copy_from_slice(&contents[..deltas[0].offset as usize]);

            for (i, d) in deltas.iter().enumerate() {
                let end = deltas
                    .get(i + 1)
                    .map_or(contents.len(), |n| n.offset as usize);
                let removed = removed_bytes(deltas, i) as usize;
                let src_start = d.offset as usize + removed;
                let dst_start = (d.offset as i64 + removed as i64 - d.delta) as usize;
                let len = end - src_start;
                buf[dst_start..dst_start + len].copy_from_slice(&contents[src_start..end]);
            }
        }

        if !ctx.args.relocatable {
            if self.is_alloc() {
                E::apply_reloc_alloc(ctx, self, buf);
            } else {
                E::apply_reloc_nonalloc(ctx, self, buf);
            }
        }
    }
}

impl Drop for InputSection {
    fn drop(&mut self) {
        if let Some(extra) = self.extra_ptr() {
            // SAFETY: this section uniquely owns its initialized extras
            // record. Its arena storage is released after all sections drop.
            unsafe { std::ptr::drop_in_place(extra) };
        }
    }
}

impl InputSection {
    /// The number of bytes relaxation removed at the location of
    /// relocation `rel`, and the number removed before it.
    #[inline]
    pub fn removed_at(&self, rel: &ElfRel) -> (i64, i64) {
        let deltas = self.r_deltas();
        let k = deltas.partition_point(|d| d.offset < rel.r_offset);
        let removed = if deltas.get(k).is_some_and(|d| d.offset == rel.r_offset) {
            removed_bytes(deltas, k)
        } else {
            0
        };
        let before = if k > 0 { deltas[k - 1].delta } else { 0 };
        (removed, before)
    }
}

/// The number of bytes removed at delta `i`.
pub fn removed_bytes(deltas: &[RelocDelta], i: usize) -> i64 {
    if i == 0 {
        deltas[0].delta
    } else {
        deltas[i].delta - deltas[i - 1].delta
    }
}

/// The total number of bytes removed before `offset`.
pub fn r_delta(isec: &InputSection, offset: u64) -> i64 {
    let deltas = isec.r_deltas();
    let i = deltas.partition_point(|d| d.offset < offset);
    if i == 0 {
        0
    } else {
        deltas[i - 1].delta
    }
}

/// Finds the object owning the prevailing COMDAT group for a symbol in a
/// discarded group. Used only on an error path.
fn find_comdat_owner<E: Arch>(
    ctx: &Context<E>,
    file: &ObjectFile,
    sym_idx: usize,
) -> Option<ObjId> {
    let esym = &file.base.elf_syms.at(sym_idx);
    if esym.is_undef() || esym.is_abs() || esym.is_common() {
        return None;
    }
    let shndx = file.shndx_at(sym_idx);
    for group in &file.comdat_groups {
        if group.is_owner() || !file.comdat_members(group).any(|m| m == shndx as u32) {
            continue;
        }
        for other in &ctx.objs {
            for other_group in &other.comdat_groups {
                if other_group.is_owner() && other_group.signature() == group.signature() {
                    return Some(other.id());
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    None,
    Error,
    Canonical,
    Plt,
}

fn do_action<E: Arch>(
    ctx: &Context<E>,
    action: Action,
    isec: &InputSection,
    sym: &Symbol,
    rel: &ElfRel,
) {
    match action {
        Action::None => {}
        Action::Error => error!(
            ctx,
            "{}: {} relocation at offset 0x{:x} against symbol `{}' can not be used; recompile with -fPIC",
            isec.display(&ctx.objs[isec.file.index()]),
            rel.type_name::<E>(),
            rel.r_offset,
            sym
        ),
        Action::Canonical => sym.add_flags(NEEDS_CANONICAL),
        Action::Plt => sym.add_flags(NEEDS_PLT),
    }
}

fn output_type<E: Arch>(ctx: &Context<E>) -> usize {
    if ctx.args.shared {
        0
    } else if ctx.args.pie {
        1
    } else {
        2
    }
}

fn sym_type(sym: &Symbol) -> usize {
    if sym.is_absolute() {
        0
    } else if !sym.is_imported() {
        1
    } else if sym.ty() != STT_FUNC {
        2
    } else {
        3
    }
}

/// Handles a PC-relative relocation such as R_X86_64_PC32. These can't be
/// promoted to dynamic relocations because dynamic linkers generally don't
/// support PC-relative ones.
pub fn scan_pcrel<E: Arch>(ctx: &Context<E>, isec: &InputSection, sym: &Symbol, rel: &ElfRel) {
    use Action::*;
    const TABLE: [[Action; 4]; 3] = [
        // Absolute  Local  Imported data  Imported code
        [Error, None, Error, Plt],           // Shared object
        [Error, None, Canonical, Canonical], // Position-independent exec
        [None, None, Canonical, Canonical],  // Position-dependent exec
    ];
    do_action(ctx, TABLE[output_type(ctx)][sym_type(sym)], isec, sym, rel);
}

/// Handles an absolute relocation smaller than the pointer size, such as
/// R_X86_64_32. Dynamic linkers don't support dynamic relocations of that
/// size, so it's an error if the value isn't known at link time.
pub fn scan_absrel<E: Arch>(ctx: &Context<E>, isec: &InputSection, sym: &Symbol, rel: &ElfRel) {
    use Action::*;
    const TABLE: [[Action; 4]; 3] = [
        // Absolute  Local  Imported data  Imported code
        [None, Error, Error, Error],        // Shared object
        [None, Error, Error, Error],        // Position-independent exec
        [None, None, Canonical, Canonical], // Position-dependent exec
    ];
    do_action(ctx, TABLE[output_type(ctx)][sym_type(sym)], isec, sym, rel);
}

pub fn scan_tlsdesc<E: Arch>(ctx: &Context<E>, sym: &Symbol) {
    if ctx.args.is_static || (ctx.args.relax && sym.is_tprel_linktime_const(ctx)) {
        // Relax TLSDESC to Local Exec: the TP-relative offset is
        // materialized directly, so no dynamic relocation is needed.
        //
        // TLSDESC relocations must always be relaxed for statically-linked
        // executables even with -no-relax, because a static executable
        // doesn't contain the trampoline function TLSDESC needs.
    } else if ctx.args.relax && sym.is_tprel_runtime_const(ctx) {
        // The TP-relative offset is known at process startup, so relax to
        // code that reads it from the GOT and adds TP.
        sym.add_flags(NEEDS_GOTTP);
    } else {
        sym.add_flags(NEEDS_TLSDESC);
    }
}

pub fn check_tlsle<E: Arch>(ctx: &Context<E>, isec: &InputSection, sym: &Symbol, rel: &ElfRel) {
    if ctx.args.shared {
        error!(
            ctx,
            "{}: relocation {} against `{}` can not be used when making a shared object; recompile with -fPIC",
            isec.display(&ctx.objs[isec.file.index()]),
            rel.type_name::<E>(),
            sym
        );
    }
}

/// A CIE record in an input `.eh_frame` section.
///
/// Compilers emit nearly identical CIEs in every object file, so the
/// linker deduplicates them; and since FDEs of dead functions must go, the
/// linker has to understand the record structure rather than copying
/// `.eh_frame` as opaque bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RelocationSpan {
    Input(&'static [u8]),
    SideTable(u32),
}

impl RelocationSpan {
    #[inline]
    fn rels<'a, E: Layout>(self, file: &'a ObjectFile) -> Rels<'a, E> {
        match self {
            RelocationSpan::Input(data) => Rels::new(data),
            RelocationSpan::SideTable(relsec_idx) => file.relocations::<E>(Some(relsec_idx)),
        }
    }
}

#[derive(Debug)]
pub struct CieRecord {
    /// The `.eh_frame` input section containing the record.
    pub section: u32,
    /// That section's contents, which FDEs consult through their CIE.
    pub contents: &'static [u8],
    pub input_offset: u32,
    pub output_offset: u32,
    /// Index of the first relocation applying to the record.
    pub rel_idx: u32,
    /// The relocation table shared by this CIE and its FDEs.
    pub(crate) relocations: RelocationSpan,
    pub icf_idx: u32,
    /// The size of the initial_location and address_range fields of FDEs
    /// associated with this CIE: 4 or 8.
    pub fde_ptr_size: u8,
    /// Whether this is the representative of its equivalence class.
    pub is_leader: bool,
}

impl CieRecord {
    #[inline]
    pub fn size<E: Layout>(&self) -> usize {
        record_size::<E>(self.contents, self.input_offset)
    }

    #[inline]
    pub fn contents<E: Layout>(&self) -> &'static [u8] {
        let start = self.input_offset as usize;
        &self.contents[start..start + self.size::<E>()]
    }

    #[inline]
    pub fn rels<'a, E: Layout>(&self, file: &'a ObjectFile) -> Rels<'a, E> {
        rels_in(
            self.relocations.rels::<E>(file),
            self.rel_idx,
            self.input_offset as usize + self.size::<E>(),
        )
    }
}

/// The size of the `.eh_frame` record at `offset`: its length field
/// plus the field itself.
#[inline]
fn record_size<E: Layout>(contents: &[u8], offset: u32) -> usize {
    E::Endian::read_u32(&contents[offset as usize..]) as usize + 4
}

/// The relocations of a `.eh_frame` record: those from index `begin`
/// that apply before `end`.
#[inline]
fn rels_in<E: Layout>(rels: Rels<'_, E>, begin: u32, end: usize) -> Rels<'_, E> {
    let begin = begin as usize;
    let rest = rels.slice(begin..rels.len());
    let count = rest
        .iter()
        .take_while(|r| (r.r_offset as usize) < end)
        .count();
    rels.slice(begin..begin + count)
}

/// An FDE record in an input `.eh_frame` section.
#[derive(Debug)]
pub struct FdeRecord {
    pub input_offset: u32,
    pub output_offset: u32,
    pub rel_idx: u32,
    pub cie_idx: u16,
    pub is_alive: AtomicBool,
    pub(crate) is_last: bool,
}

impl FdeRecord {
    #[inline]
    pub fn new(input_offset: u32, rel_idx: u32) -> FdeRecord {
        FdeRecord {
            input_offset,
            output_offset: 0,
            rel_idx,
            cie_idx: 0,
            is_alive: AtomicBool::new(true),
            is_last: false,
        }
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn kill(&self) {
        self.is_alive.store(false, Ordering::Relaxed);
    }

    #[inline]
    fn cie<'a>(&self, file: &'a ObjectFile) -> &'a CieRecord {
        &file.cies[self.cie_idx as usize]
    }

    #[inline]
    pub fn size<E: Layout>(&self, file: &ObjectFile) -> usize {
        self.size_with::<E>(&file.cies)
    }

    #[inline]
    pub(crate) fn size_with<E: Layout>(&self, cies: &[CieRecord]) -> usize {
        record_size::<E>(cies[self.cie_idx as usize].contents, self.input_offset)
    }

    #[inline]
    pub fn contents<E: Layout>(&self, file: &ObjectFile) -> &'static [u8] {
        let start = self.input_offset as usize;
        &self.cie(file).contents[start..start + self.size::<E>(file)]
    }

    #[inline]
    pub fn rels<'a, E: Layout>(&self, file: &'a ObjectFile) -> Rels<'a, E> {
        let cie = self.cie(file);
        let end = self.input_offset as usize + record_size::<E>(cie.contents, self.input_offset);
        rels_in(cie.relocations.rels::<E>(file), self.rel_idx, end)
    }
}

/// A function descriptor entry read from an input `.sframe` section. The
/// FRE block carries no relocations and is copied verbatim; only the
/// function start is relocated.
#[derive(Debug)]
pub struct SFrameFde {
    /// The function's input section, for liveness.
    pub section: u32,
    /// The symbol the func_start relocation refers to.
    pub sym: SymbolId,
    pub addend: i64,
    pub fre: &'static [u8],
    pub func_size: u32,
    pub num_fres: u32,
}

/// A section fragment: the unit of deduplication in a mergeable section.
#[derive(Debug)]
pub struct SectionFragment {
    pub p2align: AtomicU8,
    /// The offset in the output section. Fragments are laid out in runs
    /// that are processed in parallel; each fragment belongs to exactly
    /// one run.
    offset: AtomicU64,
    pub is_alive: AtomicBool,
    /// Whether the fragment must be placed within 4 GiB of the start of
    /// the output section.
    pub is_32bit: AtomicBool,
}

impl SectionFragment {
    #[inline]
    pub fn new(is_alive: bool) -> SectionFragment {
        SectionFragment {
            p2align: AtomicU8::new(0),
            offset: AtomicU64::new(0),
            is_alive: AtomicBool::new(is_alive),
            is_32bit: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn offset(&self) -> u64 {
        self.offset.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_offset(&self, offset: u64) {
        self.offset.store(offset, Ordering::Relaxed);
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_alive(&self) {
        self.is_alive.store(true, Ordering::Relaxed);
    }

    pub fn is_32bit(&self) -> bool {
        self.is_32bit.load(Ordering::Relaxed)
    }

    pub fn set_32bit(&self) {
        self.is_32bit.store(true, Ordering::Relaxed);
    }
}

/// An input section with the SHF_MERGE flag, split into fragments.
///
/// Mergeable sections typically contain string literals or fixed-size
/// constants. The linker splits them into pieces, deduplicates the pieces
/// across all input files, and emits each unique piece once.
#[derive(Debug)]
pub struct MergeableSection {
    pub parent: MergedSectionId,
    pub p2align: u8,
    pub shndx: u32,
    input_offset: u32,
    /// The fragment of each piece, parallel to `frag_offsets`.
    pub fragments: Vec<EntryId>,
    frag_offsets: Vec<u32>,
    hashes: Vec<u64>,
}

impl MergeableSection {
    /// Refers to an input section in its stable dense slot. The section
    /// itself is dead from now on; its contents live on as fragments.
    #[inline]
    fn new(parent: MergedSectionId, input_offset: u32, section: &InputSection) -> MergeableSection {
        section.kill();
        MergeableSection {
            parent,
            p2align: section.p2align(),
            shndx: section.shndx,
            input_offset,
            fragments: Vec::new(),
            frag_offsets: Vec::new(),
            hashes: Vec::new(),
        }
    }

    /// Splits the section contents into pieces and computes their hashes.
    pub fn split_contents<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        file: &dyn fmt::Display,
        section: &InputSection,
        name: &BStr,
        parent: &MergedSection,
        sketch: &mut HyperLogLog,
    ) {
        let data = section.contents();
        if data.len() > u32::MAX as usize {
            fatal!(
                diag,
                "{}: mergeable section too large",
                format_args!("{file}:({name})")
            );
        }
        let entsize = parent.hdr.shdr.sh_entsize as usize;

        if parent.hdr.shdr.sh_flags & SHF_STRINGS as u64 != 0 {
            let mut pos = 0;
            while pos < data.len() {
                self.frag_offsets.push(pos as u32);
                let Some(end) = find_null(data, pos, entsize) else {
                    fatal!(
                        diag,
                        "{}: string is not null terminated",
                        format_args!("{file}:({name})")
                    );
                };
                pos = end + entsize;
            }
        } else {
            if !data.len().is_multiple_of(entsize) {
                fatal!(
                    diag,
                    "{}: section size is not multiple of sh_entsize",
                    format_args!("{file}:({name})")
                );
            }
            self.frag_offsets = (0..data.len()).step_by(entsize).map(|p| p as u32).collect();
        }

        // Compute hashes for section pieces
        self.hashes.reserve(self.frag_offsets.len());
        for i in 0..self.frag_offsets.len() {
            let hash = xxhash_rust::xxh3::xxh3_64(self.contents(section, i));
            self.hashes.push(hash);
            sketch.insert(hash);
        }
    }

    /// Inserts the pieces into the parent section's fragment map.
    pub fn resolve_contents(
        &mut self,
        section: &InputSection,
        parent: &crate::chunks::merged::MergedSection,
        gc_sections: bool,
    ) {
        let n = self.frag_offsets.len();
        self.fragments.reserve(n);

        // The hash table is typically much larger than the cache, so each
        // insertion stalls on a cache miss for its first probe. We know all
        // hashes upfront, so prefetch the bucket a few insertions ahead.
        const LOOKAHEAD: usize = 8;

        for i in 0..n {
            if let Some(&hash) = self.hashes.get(i + LOOKAHEAD) {
                parent.map.prefetch(hash);
            }
            let id = parent.insert(
                self.contents(section, i),
                self.hashes[i],
                self.p2align,
                gc_sections,
            );
            self.fragments.push(id);
        }

        // Reclaim memory as we'll never use this vector again.
        self.hashes = Vec::new();
    }

    /// Finds the fragment containing `offset` and the offset within it.
    #[inline]
    pub fn fragment(&self, offset: u64) -> Option<(EntryId, i64)> {
        let idx = self
            .frag_offsets
            .partition_point(|&o| o as u64 <= offset)
            .checked_sub(1)?;
        Some((
            self.fragments[idx],
            offset as i64 - self.frag_offsets[idx] as i64,
        ))
    }

    #[inline]
    fn contents(&self, section: &InputSection, i: usize) -> &'static [u8] {
        let contents = section.contents();
        let start = self.frag_offsets[i] as usize;
        match self.frag_offsets.get(i + 1) {
            Some(&end) => &contents[start..end as usize],
            None => &contents[start..],
        }
    }
}

#[inline]
fn find_null(data: &[u8], pos: usize, entsize: usize) -> Option<usize> {
    if entsize == 1 {
        return memchr::memchr(0, &data[pos..]).map(|i| pos + i);
    }
    let mut p = pos;
    while p + entsize <= data.len() {
        if data[p..p + entsize].iter().all(|&b| b == 0) {
            return Some(p);
        }
        p += entsize;
    }
    None
}

/// ObjectFile needs a lookup table indexed by ELF section number. The table
/// lives outside the arena, so a regular section is stored as its 31-bit arena
/// index. The high bit distinguishes mergeable sections, which are stored as
/// indices into `mergeable`.
#[derive(Debug)]
pub struct SectionList {
    indices: Vec<u32>,
    mergeable: Vec<MergeableSection>,
    arena_base: NonNull<u8>,
}

// SAFETY: a SectionList owns the distinct arena objects named by its table.
// Shared access yields only shared references, and mutation requires an
// exclusive borrow of the list.
unsafe impl Send for SectionList {}
unsafe impl Sync for SectionList {}

/// A sparsely-backed address range for input sections. Allocation is
/// thread-safe and monotonic; individual allocations are not freed. Large
/// links fill the beginning densely, so transparent huge pages reduce
/// address-translation overhead without populating unused pages.
pub struct SectionArena {
    data: NonNull<u8>,
    offset: std::sync::atomic::AtomicUsize,
    size: usize,
    id: u64,
}

const SECTION_ARENA_BLOCK_SIZE: usize = 64 * 1024;
const SECTION_ARENA_BLOCK_ALIGNMENT: usize = 64;
const MAX_LOCAL_SECTION_ALLOC: usize = SECTION_ARENA_BLOCK_SIZE / 4;

#[derive(Clone, Copy)]
struct LocalSectionBlock {
    arena_id: u64,
    position: usize,
    end: usize,
}

thread_local! {
    static LOCAL_SECTION_BLOCK: Cell<LocalSectionBlock> = const {
        Cell::new(LocalSectionBlock {
            arena_id: 0,
            position: 0,
            end: 0,
        })
    };
}

static NEXT_SECTION_ARENA_ID: AtomicU64 = AtomicU64::new(1);

// SAFETY: allocation returns disjoint ranges, and the arena never accesses
// their contents itself.
unsafe impl Send for SectionArena {}
unsafe impl Sync for SectionArena {}

impl SectionArena {
    // The mapping is much larger than ordinary links need. A smaller
    // reservation is used on 32-bit hosts, where address space is limited.
    const SIZE: usize = if usize::BITS == 64 {
        1usize << 33
    } else {
        1usize << 28
    };

    pub fn new() -> SectionArena {
        let flags = libc::MAP_ANONYMOUS | libc::MAP_PRIVATE;
        #[cfg(any(target_os = "android", target_os = "linux"))]
        let flags = flags | libc::MAP_NORESERVE;

        // SAFETY: this creates private anonymous storage. SectionList makes
        // typed references only to elements it has initialized.
        let data = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                Self::SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                -1,
                0,
            )
        };
        if data == libc::MAP_FAILED {
            panic!(
                "mmap of {} bytes for input sections failed: {}",
                Self::SIZE,
                std::io::Error::last_os_error()
            );
        }

        #[cfg(any(target_os = "android", target_os = "linux"))]
        // SAFETY: the range is the fresh mapping; the advice is only a hint.
        unsafe {
            libc::madvise(data, Self::SIZE, libc::MADV_HUGEPAGE);
        }

        SectionArena {
            data: NonNull::new(data.cast()).expect("mmap returned a null address"),
            // Leave the first slots unused so that a base-relative index is
            // never zero.
            offset: std::sync::atomic::AtomicUsize::new(8),
            size: Self::SIZE,
            id: NEXT_SECTION_ARENA_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn allocate_global(&self, size: usize, alignment: usize) -> usize {
        debug_assert!(alignment.is_power_of_two());
        let mut old = self.offset.load(Ordering::Relaxed);
        loop {
            let begin = old
                .checked_add(alignment - 1)
                .map(|value| value & !(alignment - 1))
                .expect("input-section arena is full");
            let end = begin
                .checked_add(size)
                .expect("input-section arena is full");
            assert!(end <= self.size, "input-section arena is full");
            match self
                .offset
                .compare_exchange_weak(old, end, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return begin,
                Err(value) => old = value,
            }
        }
    }

    fn allocate_offset(&self, size: usize, alignment: usize) -> usize {
        if size <= MAX_LOCAL_SECTION_ALLOC && alignment <= SECTION_ARENA_BLOCK_ALIGNMENT {
            // Most files need a small section block. Reserve such allocations
            // from a thread-local block to avoid contending on the global bump
            // pointer.
            return LOCAL_SECTION_BLOCK.with(|slot| {
                let mut local = slot.get();
                if local.arena_id == self.id {
                    let begin = (local.position + alignment - 1) & !(alignment - 1);
                    if begin <= local.end && size <= local.end - begin {
                        local.position = begin + size;
                        slot.set(local);
                        return begin;
                    }
                }

                let begin =
                    self.allocate_global(SECTION_ARENA_BLOCK_SIZE, SECTION_ARENA_BLOCK_ALIGNMENT);
                slot.set(LocalSectionBlock {
                    arena_id: self.id,
                    position: begin + size,
                    end: begin + SECTION_ARENA_BLOCK_SIZE,
                });
                begin
            });
        }

        self.allocate_global(size, alignment)
    }

    fn insert(&self, section: InputSection) -> u32 {
        let size = std::mem::size_of::<InputSection>();
        let begin = self.allocate_offset(size, std::mem::align_of::<InputSection>());
        debug_assert!(begin > 0 && begin < Self::SIZE);
        debug_assert_eq!(begin % 4, 0);
        debug_assert_eq!(begin % std::mem::align_of::<InputSection>(), 0);

        // SAFETY: the atomic bump pointer assigned this object a disjoint,
        // properly aligned range within the mapping.
        unsafe {
            self.data
                .as_ptr()
                .add(begin)
                .cast::<InputSection>()
                .write(section)
        };
        u32::try_from(begin / 4).expect("input-section arena is too large")
    }

    #[inline]
    fn input_ptr(&self, id: InputSectionId) -> *mut InputSection {
        debug_assert_ne!(id, InputSectionId::NONE);
        // SAFETY: every nonzero InputSectionId was created from an initialized
        // allocation within this arena.
        unsafe {
            self.data
                .as_ptr()
                .add(id.0 as usize * 4)
                .cast::<InputSection>()
        }
    }

    #[inline]
    pub(crate) fn section(&self, id: InputSectionId) -> &InputSection {
        // SAFETY: InputSectionId values remain live until their SectionList is
        // dropped, after all output-section users.
        unsafe { &*self.input_ptr(id) }
    }

    #[inline]
    pub(crate) fn section_mut(&mut self, id: InputSectionId) -> &mut InputSection {
        // SAFETY: an exclusive arena borrow gives exclusive access to the
        // section named by this id.
        unsafe { &mut *self.input_ptr(id) }
    }

    fn insert_extra(&self, extra: InputSectionExtras) -> *mut InputSectionExtras {
        let begin = self.allocate_offset(
            std::mem::size_of::<InputSectionExtras>(),
            std::mem::align_of::<InputSectionExtras>(),
        );
        // SAFETY: the arena assigned this record a disjoint, properly aligned
        // range which remains live until after all InputSections are dropped.
        let ptr = unsafe { self.data.as_ptr().add(begin).cast::<InputSectionExtras>() };
        unsafe { ptr.write(extra) };
        ptr
    }
}

impl Default for SectionArena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SectionArena {
    fn drop(&mut self) {
        // SAFETY: SectionLists have already dropped their initialized
        // elements; this releases the mapping that supplied their storage.
        unsafe { libc::munmap(self.data.as_ptr().cast(), self.size) };
    }
}

const MERGEABLE_SECTION: u32 = 1 << 31;
const SECTION_INDEX_MASK: u32 = !MERGEABLE_SECTION;

impl Default for SectionList {
    fn default() -> Self {
        SectionList {
            indices: Vec::new(),
            mergeable: Vec::new(),
            arena_base: NonNull::dangling(),
        }
    }
}

impl SectionList {
    /// A list for `nsections` section indices, none with a section yet.
    #[inline]
    pub fn new(nsections: usize, additional: usize, arena: &SectionArena) -> SectionList {
        let mut indices = vec![0; nsections];
        indices.reserve(additional);
        SectionList {
            indices,
            mergeable: Vec::new(),
            arena_base: arena.data,
        }
    }

    #[inline]
    fn input_ptr(&self, index: u32) -> *mut InputSection {
        debug_assert!(index != 0 && index & MERGEABLE_SECTION == 0);
        // SAFETY: regular indices are offsets in four-byte units into this
        // SectionList's arena mapping.
        unsafe { self.arena_base.as_ptr().add(index as usize * 4).cast() }
    }

    /// The number of section indices.
    #[inline]
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Adds the section for `shndx`, which has none yet.
    #[inline]
    pub fn insert(&mut self, shndx: usize, section: InputSection, arena: &SectionArena) {
        debug_assert_eq!(self.indices[shndx], 0);
        debug_assert_eq!(self.arena_base, arena.data);
        self.indices[shndx] = arena.insert(section);
    }

    /// Adds a section the linker made up, under a new section index.
    pub fn push(&mut self, section: InputSection, arena: &SectionArena) {
        self.indices.push(0);
        self.insert(self.indices.len() - 1, section, arena);
    }

    #[inline]
    pub fn section(&self, shndx: usize) -> Option<&InputSection> {
        let value = *self.indices.get(shndx)?;
        if value == 0 {
            None
        } else if value & MERGEABLE_SECTION != 0 {
            let m = &self.mergeable[((value & SECTION_INDEX_MASK) - 1) as usize];
            // SAFETY: the mergeable section retains the arena index of its
            // stable input section.
            Some(unsafe { &*self.input_ptr(m.input_offset) })
        } else {
            // SAFETY: a nonzero regular table entry is a live arena index.
            Some(unsafe { &*self.input_ptr(value) })
        }
    }

    #[inline]
    pub fn section_mut(&mut self, shndx: usize) -> Option<&mut InputSection> {
        let value = *self.indices.get(shndx)?;
        if value == 0 {
            None
        } else if value & MERGEABLE_SECTION != 0 {
            let input_offset =
                self.mergeable[((value & SECTION_INDEX_MASK) - 1) as usize].input_offset;
            // SAFETY: an exclusive SectionList borrow gives exclusive access
            // to each input section it owns.
            Some(unsafe { &mut *self.input_ptr(input_offset) })
        } else {
            // SAFETY: an exclusive SectionList borrow gives exclusive access
            // to each input section it owns.
            Some(unsafe { &mut *self.input_ptr(value) })
        }
    }

    #[inline]
    pub fn mergeable(&self, shndx: usize) -> Option<&MergeableSection> {
        let value = *self.indices.get(shndx)?;
        (value & MERGEABLE_SECTION != 0)
            .then(|| &self.mergeable[((value & SECTION_INDEX_MASK) - 1) as usize])
    }

    #[inline]
    pub fn mergeable_mut(&mut self, shndx: usize) -> Option<&mut MergeableSection> {
        let value = *self.indices.get(shndx)?;
        (value & MERGEABLE_SECTION != 0)
            .then(|| &mut self.mergeable[((value & SECTION_INDEX_MASK) - 1) as usize])
    }

    /// Returns a regular section before it is converted to a mergeable one.
    pub fn regular_section_mut(&mut self, shndx: usize) -> Option<&mut InputSection> {
        let value = self.indices[shndx];
        (value != 0 && value & MERGEABLE_SECTION == 0)
            // SAFETY: an exclusive SectionList borrow gives exclusive access
            // to the regular section named by this table entry.
            .then(|| unsafe { &mut *self.input_ptr(value) })
    }

    /// Installs mergeable metadata while leaving the input section in its
    /// stable arena slot.
    pub fn set_mergeable(&mut self, shndx: usize, parent: MergedSectionId) {
        let value = self.indices[shndx];
        debug_assert!(value != 0 && value & MERGEABLE_SECTION == 0);
        debug_assert!(self.mergeable.len() < SECTION_INDEX_MASK as usize);
        // SAFETY: `value` is the regular arena index currently in this slot.
        let input = unsafe { &*self.input_ptr(value) };
        self.mergeable
            .push(MergeableSection::new(parent, value, input));
        self.indices[shndx] = MERGEABLE_SECTION | self.mergeable.len() as u32;
    }

    /// Returns mergeable metadata together with its stable input section.
    pub fn mergeable_with_section_mut(
        &mut self,
        shndx: usize,
    ) -> Option<(&mut MergeableSection, &InputSection)> {
        let value = *self.indices.get(shndx)?;
        if value & MERGEABLE_SECTION == 0 {
            return None;
        }
        let mergeable_idx = ((value & SECTION_INDEX_MASK) - 1) as usize;
        let input_offset = self.mergeable[mergeable_idx].input_offset;
        let input = self.input_ptr(input_offset);
        // SAFETY: mergeable metadata and its arena-allocated input section
        // occupy disjoint storage and both belong to this SectionList.
        Some((&mut self.mergeable[mergeable_idx], unsafe { &*input }))
    }

    /// The regular sections in section order.
    pub fn regular(&self) -> impl Iterator<Item = &InputSection> {
        self.indices
            .iter()
            .copied()
            .filter(|&index| index != 0 && index & MERGEABLE_SECTION == 0)
            .map(|index| {
                // SAFETY: every yielded regular index names a distinct,
                // initialized section owned by this SectionList.
                unsafe { &*self.input_ptr(index) }
            })
    }

    pub fn regular_mut(&mut self) -> impl Iterator<Item = &mut InputSection> {
        let base = self.arena_base;
        self.indices
            .iter()
            .copied()
            .filter(|&index| index != 0 && index & MERGEABLE_SECTION == 0)
            .map(move |index| {
                // SAFETY: regular table entries are distinct, and the
                // iterator holds an exclusive borrow of this SectionList.
                unsafe { &mut *base.as_ptr().add(index as usize * 4).cast() }
            })
    }

    /// The regular sections and their compact arena pointers, in section order.
    pub fn regular_ids_mut(&mut self) -> impl Iterator<Item = (InputSectionId, &mut InputSection)> {
        let base = self.arena_base;
        self.indices
            .iter()
            .copied()
            .filter(|&index| index != 0 && index & MERGEABLE_SECTION == 0)
            .map(move |index| {
                // SAFETY: regular table entries are distinct, and the
                // iterator holds an exclusive borrow of this SectionList.
                let section = unsafe { &mut *base.as_ptr().add(index as usize * 4).cast() };
                (InputSectionId(index), section)
            })
    }

    pub fn mergeable_sections(&self) -> impl Iterator<Item = &MergeableSection> {
        self.mergeable.iter()
    }

    pub fn mergeable_sections_mut(&mut self) -> impl Iterator<Item = &mut MergeableSection> {
        self.mergeable.iter_mut()
    }

    /// The mergeable sections together with their stable input sections.
    pub fn mergeable_sections_with_inputs_mut(
        &mut self,
    ) -> impl Iterator<Item = (&mut MergeableSection, &InputSection)> {
        let base = self.arena_base;
        self.mergeable.iter_mut().map(move |m| {
            // SAFETY: each mergeable record retains the arena index of its
            // initialized input section, which is disjoint from this vector.
            let input = unsafe {
                &*base
                    .as_ptr()
                    .add(m.input_offset as usize * 4)
                    .cast::<InputSection>()
            };
            (m, input)
        })
    }
}

impl Drop for SectionList {
    fn drop(&mut self) {
        for &value in &self.indices {
            let index = if value == 0 {
                continue;
            } else if value & MERGEABLE_SECTION != 0 {
                self.mergeable[((value & SECTION_INDEX_MASK) - 1) as usize].input_offset
            } else {
                value
            };
            // SAFETY: each initialized input section appears exactly once in
            // the table, either directly or through its mergeable metadata.
            unsafe { std::ptr::drop_in_place(self.input_ptr(index)) };
        }
    }
}
