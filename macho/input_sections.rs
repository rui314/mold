//! Input sections.

use mold_common::bytes::display;
use mold_common::error;

use crate::arch::Target;
use crate::chunks::ChunkId;
use crate::context::Context;
use crate::input_files::{DW_EH_PE_SDATA4, ObjectFile};
use crate::macho::{
    MachSection, MachSym, N_PEXT, N_WEAK_DEF, S_THREAD_LOCAL_REGULAR, S_THREAD_LOCAL_ZEROFILL,
};
use crate::symbol::SymbolId;

/// A subsection index, u32 as in mold.
pub type InputSectionId = u32;

/// The subsection arena. Indexable by a u32 id or a usize (through
/// Deref to the Vec), so id vectors can be u32 - half the size - while
/// every arena access stays `isecs[id]`.
#[derive(Debug, Default)]
pub struct InputSections(pub Vec<InputSection>);

impl std::ops::Deref for InputSections {
    type Target = Vec<InputSection>;
    #[inline]
    fn deref(&self) -> &Vec<InputSection> {
        &self.0
    }
}
impl std::ops::DerefMut for InputSections {
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<InputSection> {
        &mut self.0
    }
}
impl std::ops::Index<u32> for InputSections {
    type Output = InputSection;
    #[inline]
    fn index(&self, id: u32) -> &InputSection {
        &self.0[id as usize]
    }
}
impl std::ops::IndexMut<u32> for InputSections {
    #[inline]
    fn index_mut(&mut self, id: u32) -> &mut InputSection {
        &mut self.0[id as usize]
    }
}
impl std::ops::Index<usize> for InputSections {
    type Output = InputSection;
    #[inline]
    fn index(&self, id: usize) -> &InputSection {
        &self.0[id]
    }
}
impl std::ops::IndexMut<usize> for InputSections {
    #[inline]
    fn index_mut(&mut self, id: usize) -> &mut InputSection {
        &mut self.0[id]
    }
}

impl InputSections {
    /// Follows literal-merge redirects to the surviving subsection.
    pub fn resolve(&self, mut id: usize) -> usize {
        while self[id].replacement != NO_REPLACEMENT {
            id = self[id].replacement as usize;
        }
        id
    }
}

/// What a relocation refers to.
#[derive(Clone, Copy, Debug)]
pub enum RelocTarget {
    /// An index into the owning object's symbol list.
    Sym(u32),
    /// A subsection. During target-specific relocation reading this is
    /// an index into the object's section header list; input file
    /// parsing rewrites it to an index into the global subsection
    /// arena.
    Section(u32),
}

/// A relocation in a form independent of the raw Mach-O records: the
/// addend is explicit, and the target is a symbol or a section.
#[derive(Clone, Copy, Debug)]
pub struct Reloc {
    /// Offset within the containing section.
    pub offset: u32,
    pub ty: u8,
    /// Size in bytes of the relocated field.
    pub size: u8,
    pub is_pcrel: bool,
    /// True if the previous record is a SUBTRACTOR paired with this one.
    pub is_subtracted: bool,
    /// The target, packed into one word: a symbol or subsection index
    /// in the low 31 bits, with `TARGET_SECTION` set for a subsection.
    /// Read through `target()`, write through `set_target()` or
    /// `RelocTarget::pack()`; as a two-word enum it padded the struct
    /// from 24 to 32 bytes, and a debug link holds ~12M of these.
    pub target: u32,
    pub addend: i64,
}

// A Reloc is the size of an ELF RELA entry, which mold reads from
// the mapping without materializing anything; Mach-O needs the record
// processed (ADDEND fusion, SUBTRACTOR pairing, subsection rebasing),
// so this is the form that processing produces. It can be smaller on
// 32-bit hosts, as i386 aligns i64 to 4 bytes.
const _: () = assert!(std::mem::size_of::<Reloc>() <= 24);

const TARGET_SECTION: u32 = 1 << 31;

impl RelocTarget {
    #[inline]
    pub fn pack(self) -> u32 {
        match self {
            Self::Sym(i) => {
                debug_assert!(i & TARGET_SECTION == 0);
                i
            }
            Self::Section(i) => {
                debug_assert!(i & TARGET_SECTION == 0);
                i | TARGET_SECTION
            }
        }
    }
}

impl Reloc {
    #[inline]
    pub fn target(&self) -> RelocTarget {
        if self.target & TARGET_SECTION != 0 {
            RelocTarget::Section(self.target & !TARGET_SECTION)
        } else {
            RelocTarget::Sym(self.target)
        }
    }
    #[inline]
    pub fn set_target(&mut self, t: RelocTarget) {
        self.target = t.pack();
    }

    /// Whether the relocation is a branch: a direct call or jump.
    #[inline]
    pub fn is_func_call<E: crate::arch::Target>(&self) -> bool {
        self.ty == E::RELOC_BRANCH
    }

    /// The output address of the relocation's target, a symbol or a
    /// subsection, as sold's Relocation::get_addr. `file` is the object
    /// the relocation is of.
    pub fn addr<E: Target>(&self, ctx: &Context<E>, file: &ObjectFile) -> u64 {
        match self.target() {
            RelocTarget::Sym(idx) => ctx.symbols[file.symbols[idx as usize]].addr(ctx),
            RelocTarget::Section(idx) => ctx.isecs[idx as usize].addr(ctx),
        }
    }

    /// The symbol the relocation refers to, if it refers to one.
    #[inline]
    pub fn sym(&self, file: &ObjectFile) -> Option<SymbolId> {
        match self.target() {
            RelocTarget::Sym(idx) => Some(file.symbols[idx as usize]),
            RelocTarget::Section(_) => None,
        }
    }

    /// The subsection the relocation's target lives in, if any: the
    /// one it refers to, or its symbol's.
    pub fn subsec<E: Target>(&self, ctx: &Context<E>, file: &ObjectFile) -> Option<usize> {
        match self.target() {
            RelocTarget::Sym(idx) => {
                ctx.symbols[file.symbols[idx as usize]].input_section().map(|i| i as usize)
            }
            RelocTarget::Section(idx) => Some(idx as usize),
        }
    }

    /// Whether the relocation's target is thread-local data.
    pub fn refers_to_tls<E: Target>(&self, ctx: &Context<E>, file: &ObjectFile) -> bool {
        self.subsec(ctx, file).is_some_and(|isec| {
            let isec = &ctx.isecs[isec];
            matches!(
                isec.hdr(&ctx.objs[isec.file as usize]).section_type(),
                S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL
            )
        })
    }

    /// How a diagnostic names the relocation's target: its symbol, or
    /// the subsection it points to.
    pub fn target_name<E: Target>(
        &self,
        ctx: &Context<E>,
        file: &ObjectFile,
    ) -> std::borrow::Cow<'static, [u8]> {
        match self.target() {
            RelocTarget::Sym(idx) => ctx.symbols[file.symbols[idx as usize]].name().into(),
            RelocTarget::Section(idx) => ctx.isecs[idx as usize].name(ctx),
        }
    }
}

/// Sentinel for `InputSection::replacement`: no surviving copy.
pub const NO_REPLACEMENT: u32 = u32::MAX;

/// A subsection of an input object file's section.
///
/// Mach-O linking granularity is the subsection: objects are built with
/// MH_SUBSECTIONS_VIA_SYMBOLS, and each section is split at its symbols,
/// so that unreferenced pieces can be dead-stripped. `shndx` names the
/// containing section; `input_addr` and `size` delimit this piece of it.
#[derive(Debug)]
pub struct InputSection {
    /// Index of the object file this section came from (u32 to keep the
    /// struct small; `usize::MAX` becomes `u32::MAX` for a synthetic
    /// section with no object).
    pub file: u32,
    /// Index of the parent section's header in the owning object's
    /// section list (the internal object's, for a synthesized one):
    /// mold's shndx. Resolved through `hdr`; a u32 index
    /// instead of an 8-byte header pointer. `p2align` is held inline
    /// because it is the one header field the linker raises per
    /// subsection.
    pub shndx: u32,
    pub p2align: u8,
    /// This subsection's address in the object's address space. Object
    /// files stay well under 4 GiB, so a u32 holds it.
    pub input_addr: u32,
    /// The subsection's size. One subsection is far under 4 GiB, so a
    /// u32 holds it; arithmetic with 64-bit addresses casts up.
    pub size: u32,
    /// The subsection contents, as a bare pointer - the length is
    /// `size` - or 0 when there are none (a zero-fill or empty
    /// section). Stored as an integer, not a slice, to save 8 bytes and
    /// keep the struct trivially Send/Sync; read through `contents()`.
    /// mold likewise keeps `contents` as a bare address.
    pub contents: usize,
    /// This subsection's relocations: a range in the owning object's
    /// `relocs` arena, offsets relative to the subsection. sold keeps
    /// rel_offset/nrels per subsection the same way, rather than a Vec
    /// per subsection - a debug link has millions of relocations.
    pub rel_offset: u32,
    pub nrels: u32,
    /// The chunk this subsection is laid out in, as `ChunkId::pack`
    /// encodes it, or `u32::MAX` until assigned: read through
    /// `output_section()`. mold's field of this name holds an
    /// Option<OutputSectionId> (a word, thanks to the id's niche); a
    /// Mach-O subsection may also be placed in the GOT (a folded
    /// __objc_classrefs entry) or in __objc_methlist (a rewritten
    /// method list), so the packed ChunkId keeps the struct at 56 bytes.
    pub output_section: u32,
    /// Offset from the start of the output section (u32::MAX marks a
    /// subsection not yet placed, during thunk layout). An output
    /// section stays well under 4 GiB, so a u32 suffices.
    pub offset: u32,
    /// IS_ALIVE and the transient IS_VISITED bit, in one atomic byte so
    /// the parallel dead-strip walk marks sections in place (mold's
    /// InputSection flags). Read through is_alive(); the &mut setters
    /// write without an atomic operation.
    pub flags: std::sync::atomic::AtomicU8,
    /// For a literal merged with an identical one, the surviving copy's
    /// subsection index, or `NO_REPLACEMENT`. A u32 sentinel rather than
    /// an `Option<usize>` (16 bytes) keeps the struct small.
    pub replacement: u32,
    /// This subsection's compact-unwind records: a range in
    /// ctx.unwind_records, as sold keeps unwind_offset/nunwind on each
    /// subsection (records arrive grouped by function).
    pub unwind_offset: u32,
    pub nunwind: u32,
}

// InputSection is the highest-count struct in a link (millions on a
// debug build), so it is kept compact: 56 bytes, under mold's 64
// (ours carries the unwind range but no section-name/flags word).
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<InputSection>() == 56);

const IS_ALIVE: u8 = 1 << 0;
const IS_VISITED: u8 = 1 << 1;
/// Placed by the pass that synthesized it, or that moved it into a
/// chunk of the linker's (its osec and output offset are set by hand),
/// so create_output_sections must not assign it to an output section by
/// name.
const IS_PLACED: u8 = 1 << 2;
/// The subsection starts at a multiple of its alignment regardless of
/// its input offset: a fixed-size record (a literal, an initializer
/// pointer, a CFString), which ld64 aligns with no modulus.
const NO_MODULUS: u8 = 1 << 3;
/// A literal record a symbol names: ld-prime keeps it a subsection of its
/// own, merged with no identical copy, and a -r output keeps its label.
const IS_LABELED: u8 = 1 << 4;
/// The image may observe the subsection's address (see
/// compute_address_significance), so ICF must keep it apart.
const IS_ADDRESS_TAKEN: u8 = 1 << 5;

impl InputSection {
    /// A live subsection of section `shndx` of object `file`: `size`
    /// bytes, `data` (empty for zero-fill), aligned to 2^`p2align`, at
    /// input address 0, with no relocations and no place in the output
    /// yet. The callers set what differs.
    pub fn new(file: u32, shndx: u32, p2align: u8, size: u32, data: &'static [u8]) -> Self {
        Self {
            file,
            shndx,
            p2align,
            input_addr: 0,
            size,
            contents: if data.is_empty() { 0 } else { data.as_ptr() as usize },
            rel_offset: 0,
            nrels: 0,
            output_section: u32::MAX,
            offset: 0,
            flags: Self::flags_alive(),
            replacement: NO_REPLACEMENT,
            unwind_offset: 0,
            nunwind: 0,
        }
    }

    /// The initial flag word of a live section.
    pub fn flags_alive() -> std::sync::atomic::AtomicU8 {
        std::sync::atomic::AtomicU8::new(IS_ALIVE)
    }
    pub fn flags_alive_no_modulus() -> std::sync::atomic::AtomicU8 {
        std::sync::atomic::AtomicU8::new(IS_ALIVE | NO_MODULUS)
    }
    /// The initial flag word of a section that never joins the link.
    pub fn flags_dead() -> std::sync::atomic::AtomicU8 {
        std::sync::atomic::AtomicU8::new(0)
    }
    /// The initial flag word of a live synthetic section placed by the
    /// pass that made it.
    pub fn flags_placed() -> std::sync::atomic::AtomicU8 {
        std::sync::atomic::AtomicU8::new(IS_ALIVE | IS_PLACED)
    }
    /// The chunk this subsection is laid out in, once assigned.
    #[inline]
    pub fn output_section(&self) -> Option<ChunkId> {
        if self.output_section == u32::MAX {
            None
        } else {
            Some(ChunkId::unpack(self.output_section))
        }
    }

    pub fn set_output_section(&mut self, id: ChunkId) {
        self.output_section = id.pack();
    }

    #[inline]
    pub fn is_placed(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & IS_PLACED != 0
    }
    /// The next output offset at or after `off` where this subsection
    /// may start. ld64 keeps each subsection at the offset it had within
    /// its input section modulo the section's alignment (an 8-byte
    /// subsection at offset 8 of a 16-aligned section stays 8 mod 16),
    /// rather than rounding every subsection up to the section's
    /// alignment; the latter pads the output by an average of half the
    /// alignment per subsection (NetNewsWire's __TEXT,__const was 11KB
    /// larger than ld-prime's).
    ///
    /// ld64 models this as a subsection alignment of (power of two,
    /// modulus). A fixed-size literal is the exception: its alignment
    /// is the literal size with modulus 0, so a 16-byte literal from a
    /// p2align-3 __literal16 section, or one that survived merging
    /// with a copy from a better-aligned section, still lands on a
    /// 16-byte boundary. Its input offset is not a constraint the
    /// compiler meant, and a 16-byte load through a scaled PAGEOFF12
    /// immediate can only address a 16-aligned slot.
    #[inline]
    pub fn align_offset(&self, off: u64) -> u64 {
        let align = 1u64 << self.p2align;
        if self.flags.load(std::sync::atomic::Ordering::Relaxed) & NO_MODULUS != 0 {
            return mold_common::bits::align_to(off, align);
        }
        mold_common::bits::align_to_mod(off, align, self.input_addr as u64 & (align - 1))
    }

    /// The alignment, as a power of two, that ld64 gives the part of
    /// this subsection starting at object address `addr`: the
    /// section's, unless that part sits at a nonzero offset modulo it,
    /// whose trailing zeros count instead (a subsection at 8 mod 16 is
    /// 8-aligned). ld64 keeps the most aligned of two copies of a weak
    /// definition or a literal.
    pub fn p2align_at(&self, addr: u64) -> u8 {
        let modulus = addr & ((1 << self.p2align) - 1);
        if self.is_record() || modulus == 0 { self.p2align } else { modulus.trailing_zeros() as u8 }
    }

    /// Whether this subsection is made of fixed-size records.
    pub fn is_record(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & NO_MODULUS != 0
    }

    pub fn is_alive(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & IS_ALIVE != 0
    }
    /// Whether the subsection is in the output as itself: live, and not
    /// replaced by an identical copy (see `replacement`).
    #[inline]
    pub fn is_emitted(&self) -> bool {
        self.is_alive() && self.replacement == NO_REPLACEMENT
    }
    #[inline]
    pub fn set_alive(&mut self, v: bool) {
        let f = self.flags.get_mut();
        if v {
            *f |= IS_ALIVE;
        } else {
            *f &= !IS_ALIVE;
        }
    }
    /// Marks the subsection dead.
    #[inline]
    pub fn kill(&mut self) {
        *self.flags.get_mut() &= !IS_ALIVE;
    }
    /// Atomically sets the visited bit; true if this call set it.
    #[inline]
    pub fn visit(&self) -> bool {
        self.flags.fetch_or(IS_VISITED, std::sync::atomic::Ordering::Relaxed) & IS_VISITED == 0
    }
    /// Clears the visited bit, through a shared reference.
    #[inline]
    pub fn unmark_visited(&self) {
        self.flags.fetch_and(!IS_VISITED, std::sync::atomic::Ordering::Relaxed);
    }
    /// Whether the visited bit is set.
    #[inline]
    pub fn is_visited(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & IS_VISITED != 0
    }
    /// Marks this literal record as named by a symbol.
    #[inline]
    pub fn mark_labeled(&self) {
        self.flags.fetch_or(IS_LABELED, std::sync::atomic::Ordering::Relaxed);
    }
    /// Whether a symbol names this literal record.
    #[inline]
    pub fn is_labeled(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & IS_LABELED != 0
    }
    #[inline]
    pub fn is_address_taken(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & IS_ADDRESS_TAKEN != 0
    }
    /// Sets the address-taken bit, writing only if it is clear: many
    /// references reach the same few subsections from every thread.
    #[inline]
    pub fn set_address_taken(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        if self.flags.load(Relaxed) & IS_ADDRESS_TAKEN == 0 {
            self.flags.fetch_or(IS_ADDRESS_TAKEN, Relaxed);
        }
    }
    /// Reads and clears the visited bit.
    #[inline]
    pub fn take_visited(&mut self) -> bool {
        let f = self.flags.get_mut();
        let v = *f & IS_VISITED != 0;
        *f &= !IS_VISITED;
        v
    }

    /// This subsection's bytes. Empty for a zero-fill or empty section;
    /// otherwise the `size` bytes at `contents` (which point into the
    /// mmap'd input, so they live for the whole link).
    #[inline]
    pub fn contents(&self) -> &'static [u8] {
        if self.contents == 0 {
            &[]
        } else {
            // SAFETY: for a non-empty section `contents` is the start of
            // `size` valid bytes in the leaked/mmap'd input.
            unsafe { std::slice::from_raw_parts(self.contents as *const u8, self.size as usize) }
        }
    }

    /// The parent section header, through the owning object's section
    /// list - mold resolves a section's shdr through its file the same
    /// way.
    #[inline]
    pub fn hdr<'a>(&self, file: &'a ObjectFile) -> &'a MachSection {
        &file.sect_hdrs[self.shndx as usize]
    }

    /// This subsection's output address: its output section's address
    /// plus its offset there, as mold's isec.addr(ctx) derives it. A
    /// literal-merge loser reports its surviving copy's address; an
    /// unplaced subsection reports 0.
    #[inline]
    pub fn addr<E: Target>(&self, ctx: &Context<E>) -> u64 {
        let mut isec = self;
        if isec.replacement != NO_REPLACEMENT {
            isec = &ctx.isecs[ctx.isecs.resolve(isec.replacement as usize)];
        }
        let Some(chunk) = isec.output_section() else {
            return 0;
        };
        if isec.offset == u32::MAX {
            return 0;
        }
        ctx.chunk_header(chunk).addr + isec.offset as u64
    }

    /// The section ordinal (a MachSym's sect) of the chunk this
    /// subsection is laid out in; 0 when it has none.
    pub fn sect_idx<E: Target>(&self, ctx: &Context<E>) -> u8 {
        self.output_section().map_or(0, |id| ctx.chunk_header(id).sect_idx)
    }

    /// This subsection's relocations, sliced from its object's reloc
    /// arena (subsections keep only a rel_offset/nrels range,
    /// sold-style).
    #[inline]
    pub fn rels<'a>(&self, file: &'a ObjectFile) -> &'a [Reloc] {
        let off = self.rel_offset as usize;
        &file.relocs[off..off + self.nrels as usize]
    }

    /// The symbol that names this subsection: of those at its start, the
    /// one subsec_name_rank ranks first. A literal merged by its content
    /// (see input_files::has_merged_subsecs) is named by none of the
    /// labels a compiler or assembler makes for itself (see
    /// is_private_label), and by nothing at all if one but an ltmpN is
    /// among them, as ld-prime has it (mergeable records name their
    /// entries so).
    pub fn label<E: Target>(&self, ctx: &Context<E>) -> Option<&'static [u8]> {
        let obj = &ctx.objs[self.file as usize];
        self.label_index(ctx).map(|i| ctx.symbols[obj.symbols[i]].name())
    }

    /// The index in its object's symbol table of the symbol that names
    /// this subsection (see label).
    pub fn label_index<E: Target>(&self, ctx: &Context<E>) -> Option<usize> {
        use crate::input_files::has_merged_subsecs;
        let obj = &ctx.objs[self.file as usize];
        let key = Some(self.label_key());
        let labels = (0..obj.mach_syms.len())
            .filter(|&i| msym_label_key(&obj.mach_syms[i]) == key)
            .map(|i| (i, &obj.mach_syms[i], ctx.symbols[obj.symbols[i]].name()));
        let merged = has_merged_subsecs(self.hdr(obj));
        if merged
            && labels
                .clone()
                .any(|(_, _, name)| is_private_label(name) && !name.starts_with(b"ltmp"))
        {
            return None;
        }
        labels
            .filter(|(_, _, name)| !(merged && is_private_label(name)))
            .max_by_key(|&(i, n, name)| (subsec_name_rank(n, name), name, i))
            .map(|(i, _, _)| i)
    }

    /// Where the labels at the start of this subsection sit: its
    /// section, counted from 1 as MachSyms count them, and its address.
    fn label_key(&self) -> (u32, u64) {
        (self.shndx + 1, self.input_addr as u64)
    }

    /// The name of this subsection in a diagnostic: its label (see
    /// label), or else its section and its offset there,
    /// "__TEXT,__cstring+0x10".
    pub fn name<E: Target>(&self, ctx: &Context<E>) -> std::borrow::Cow<'static, [u8]> {
        if let Some(name) = self.label(ctx) {
            return name.into();
        }
        let hdr = self.hdr(&ctx.objs[self.file as usize]);
        let (seg, sect) = (display(hdr.segname()), display(hdr.sectname()));
        let off = self.input_addr as u64 - hdr.addr.get();
        format!("{seg},{sect}+0x{off:x}").into_bytes().into()
    }

    /// Names the place `offset` bytes into this subsection:
    /// "'NAME'+0xOFF (path)".
    pub fn location<E: Target>(&self, ctx: &Context<E>, offset: u32) -> String {
        let path = ctx.objs[self.file as usize].mf.name.display();
        let name = self.name(ctx);
        let name = display(&name);
        if offset == 0 {
            format!("'{name}' ({path})")
        } else {
            format!("'{name}'+0x{offset:X} ({path})")
        }
    }

    /// Reports a relocation that can't be applied where it is, `offset`
    /// bytes into this subsection.
    pub fn fixup_error<E: Target>(&self, ctx: &Context<E>, offset: u32, msg: std::fmt::Arguments) {
        let file = ctx.objs[self.file as usize].mf.name.display();
        let name = self.name(ctx);
        let name = display(&name);
        error!("{file}: {name}+0x{offset:x}: {msg}");
    }

    /// Whether the target of relocation `r` of this subsection has an
    /// address in the image, as a PC-relative reference that goes
    /// through no stub or GOT slot needs (an x86-64 RIP-relative one, an
    /// arm64 adrp or the offset into its page): an import has none,
    /// which is an error.
    pub fn target_has_address<E: Target>(&self, ctx: &Context<E>, r: &Reloc) -> bool {
        let file = &ctx.objs[self.file as usize];
        let Some(id) = r.sym(file).filter(|&id| ctx.symbols[id].is_imported()) else {
            return true;
        };
        let msg = format_args!("target '{}' does not have address", ctx.symbols[id]);
        self.fixup_error(ctx, r.offset, msg);
        false
    }

    /// Notes relocation `i` of `rels`, this subsection's (subsection
    /// `isec_id`), whose pointer is at `addr`, if it is a text
    /// relocation: in a range of text_reloc_ranges, and needing a fixup.
    #[inline]
    pub fn check_text_reloc<E: Target>(
        &self,
        ctx: &Context<E>,
        isec_id: usize,
        rels: &[Reloc],
        i: usize,
        addr: u64,
    ) {
        if ctx.text_reloc_ranges.iter().any(|range| range.contains(&addr)) {
            self.note_text_reloc(ctx, isec_id, rels, i);
        }
    }

    /// Records a pointer in a read-only segment if dyld (or whatever
    /// loads the image) has to bind or slide it, as the fixup builders
    /// decide.
    #[cold]
    fn note_text_reloc<E: Target>(
        &self,
        ctx: &Context<E>,
        isec_id: usize,
        rels: &[Reloc],
        i: usize,
    ) {
        let file = &ctx.objs[self.file as usize];
        let rel = &rels[i];
        let slides = ctx.args.pie || ctx.args.output_type != crate::macho::MH_EXECUTE;
        let needs_fixup = match rel.sym(file) {
            Some(id)
                if ctx.symbols[id].binds_at_runtime(ctx) || ctx.symbols[id].binds_to_self(ctx) =>
            {
                true
            }
            Some(id) if ctx.symbols[id].is_absolute(ctx) => false,
            _ => slides && !rel.refers_to_tls(ctx, file),
        };
        if needs_fixup {
            ctx.text_relocs.lock().unwrap().push((isec_id as u32, i as u32));
        }
    }
}

/// Where a symbol labels a subsection's start, if it is a label at all.
fn msym_label_key(msym: &crate::macho::MachSym) -> Option<(u32, u64)> {
    (!msym.is_stab() && msym.ty() == crate::macho::N_SECT)
        .then_some((msym.sect as u32, msym.value.get()))
}

/// Whether a label is one a compiler or assembler makes for itself: an
/// assembler temporary (L...) or a linker-private label (l...) - the
/// compiler's lCPI0_0 constant-pool and l_.str string labels, the arm64
/// assembler's ltmpN.
pub fn is_private_label(name: &[u8]) -> bool {
    name.starts_with(b"L") || name.starts_with(b"l")
}

/// How ld-prime prefers a symbol at a subsection's start to name the
/// subsection in a diagnostic: an exported one before a private extern,
/// a local, a weak definition and an ltmpN label; among equals, the
/// greatest name.
pub fn subsec_name_rank(msym: &MachSym, name: &[u8]) -> u8 {
    if name.starts_with(b"ltmp") {
        0
    } else if msym.desc.get() & N_WEAK_DEF != 0 {
        1
    } else if !msym.is_extern() {
        2
    } else if msym.n_type & N_PEXT != 0 {
        3
    } else {
        4
    }
}

/// Sentinel for an absent index in `UnwindRecord` (no personality, no
/// LSDA, no FDE).
pub const UNWIND_NONE: u32 = u32::MAX;

/// A record from a __compact_unwind section, describing how to unwind
/// the stack through one function.
///
/// One record per function, walked by unwind-info encoding, dead-strip
/// and ICF, so it is kept to eight u32s (32 bytes): every index is a
/// u32 with `UNWIND_NONE` for "absent" rather than an `Option<usize>`,
/// which is 16 bytes each - as mold's FdeRecord/CieRecord hold u32
/// indices. Read the optional fields through `personality()`, `lsda()`,
/// `fde()`.
#[derive(Clone, Debug)]
pub struct UnwindRecord {
    /// The input section holding the function.
    pub isec: u32,
    /// The function's offset within `isec`.
    pub input_offset: u32,
    pub code_len: u32,
    pub encoding: u32,
    /// The personality symbol, or `UNWIND_NONE`.
    pub personality_sym: u32,
    /// The language-specific data area: an input section (or
    /// `UNWIND_NONE`) and an offset within it.
    pub lsda_isec: u32,
    pub lsda_off: u32,
    /// For a record synthesized from DWARF unwind info, the FDE it
    /// points to (an index into `ctx.fdes`), or `UNWIND_NONE`.
    pub fde_idx: u32,
}

const _: () = assert!(std::mem::size_of::<UnwindRecord>() == 32);

impl UnwindRecord {
    #[inline]
    pub fn personality(&self) -> Option<SymbolId> {
        (self.personality_sym != UNWIND_NONE).then_some(self.personality_sym)
    }
    #[inline]
    pub fn lsda(&self) -> Option<(usize, u32)> {
        (self.lsda_isec != UNWIND_NONE).then_some((self.lsda_isec as usize, self.lsda_off))
    }
    #[inline]
    pub fn fde(&self) -> Option<usize> {
        (self.fde_idx != UNWIND_NONE).then_some(self.fde_idx as usize)
    }

    /// The personality routine of the record's function: the record's
    /// own, or for one in DWARF mode, its FDE's CIE's.
    pub fn function_personality<E: Target>(&self, ctx: &Context<E>) -> Option<SymbolId> {
        self.personality().or_else(|| ctx.cies[ctx.fdes[self.fde()?].cie as usize].personality)
    }

    /// The LSDA of the record's function: the record's own, or for one
    /// in DWARF mode, its FDE's.
    pub fn function_lsda<E: Target>(&self, ctx: &Context<E>) -> Option<(usize, u32)> {
        self.lsda().or_else(|| {
            let (isec, off) = ctx.fdes[self.fde()?].lsda?;
            Some((isec as usize, off))
        })
    }
}

/// A DWARF Common Information Entry from an object's __eh_frame.
#[derive(Debug)]
pub struct CieRecord {
    /// The owning object (u32 index).
    pub obj: u32,
    pub input_addr: u32,
    /// The CIE bytes: a slice of the object's __eh_frame (with its
    /// relocations pre-applied), not a per-record copy - mold's
    /// CieRecord borrows its contents the same way.
    pub data: &'static [u8],
    pub personality: Option<SymbolId>,
    pub personality_offset: u32,
    /// How the CIE's FDEs encode their function's address and size:
    /// its 'R' augmentation, or DW_EH_PE_absptr without one. Each FDE
    /// checks it as it is read.
    pub fde_enc: u8,
    /// How they encode their LSDA pointer, if the CIE has an 'L'
    /// augmentation; checked the same way.
    pub lsda_enc: Option<u8>,
    pub output_offset: u32,
    pub is_alive: bool,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<CieRecord>() == 48);

impl CieRecord {
    /// The size of the function address and size that start its FDEs'
    /// fields: 4 bytes in DW_EH_PE_sdata4 (GCC's 0x1b), 8 in
    /// DW_EH_PE_absptr (0x10, what clang writes).
    pub fn pc_size(&self) -> usize {
        if self.fde_enc & 0xf == DW_EH_PE_SDATA4 { 4 } else { 8 }
    }

    /// The size of an LSDA pointer of its FDEs, in the same encodings.
    pub fn lsda_size(&self) -> usize {
        if self.lsda_enc.is_some_and(|enc| enc & 0xf == DW_EH_PE_SDATA4) { 4 } else { 8 }
    }
}

/// A DWARF Frame Description Entry from an object's __eh_frame.
#[derive(Debug)]
pub struct FdeRecord {
    /// The owning object (u32 index).
    pub obj: u32,
    pub input_addr: u32,
    /// The FDE bytes: a slice of the object's processed __eh_frame.
    pub data: &'static [u8],
    /// Index of the CIE this FDE points at (ctx.cies).
    pub cie: u32,
    /// The subsection holding the function.
    pub isec: u32,
    pub func_offset: u32,
    pub code_len: u32,
    /// The language-specific data area: a subsection and an offset.
    pub lsda: Option<(u32, u32)>,
    pub output_offset: u32,
}

// Every index a u32 and the record bytes borrowed, as in mold
// (whose FdeRecord derives even more and is 16 bytes).
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<FdeRecord>() == 56);

impl FdeRecord {
    /// The offset of the LSDA pointer: the augmentation data, past its
    /// ULEB128 length, after the length, CIE pointer, pc_begin and
    /// pc_range (`pc_size` bytes each, see CieRecord::pc_size).
    pub fn lsda_pos(&self, pc_size: usize) -> usize {
        let mut pos = 8 + 2 * pc_size;
        while self.data[pos] & 0x80 != 0 {
            pos += 1;
        }
        pos + 1
    }
}
