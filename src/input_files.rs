//! Input object files and shared libraries.

// DWARF constants keep the spelling of the specification.
#![allow(non_upper_case_globals)]

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use crate::arch::{Arch, Family};
use crate::args::Args;
use crate::chunks::merged::MergedSection;
use crate::context::Context;
use crate::diagnostics::Diagnostics;
use crate::elf::*;
use crate::input_sections::{
    CieRecord, FdeRecord, FragmentRef, InputSection, MergeableSection, RelocationSpan, SFrameFde,
    SectionList,
};
use crate::mapped_file::MappedFile;
use crate::symbol::{
    hash_key, Bins, ParallelSymbolAllocator, Symbol, SymbolId, SymbolSlot, SymbolTable, NEEDS_PLT,
};
use crate::util::{
    self, align_to, bits, cstr_at, leak_bytes, path_clean, path_filename, read_uleb,
};
use crate::{error, fatal, out, warn};
use bstr::BStr;

#[derive(Clone, Copy, Debug, Default)]
struct NameLen(u8);

impl NameLen {
    const LONG_NAME: usize = 240;

    #[inline]
    fn new(len: usize) -> NameLen {
        if len < Self::LONG_NAME {
            return NameLen(len as u8);
        }
        let log2 = (len - Self::LONG_NAME + 1).ilog2() as usize;
        NameLen((Self::LONG_NAME + log2.min(u8::MAX as usize - Self::LONG_NAME)) as u8)
    }

    #[inline]
    fn is_long(self) -> bool {
        self.0 as usize >= Self::LONG_NAME
    }

    #[inline]
    fn lower_bound(self) -> usize {
        if !self.is_long() {
            return self.0 as usize;
        }
        Self::LONG_NAME - 1 + (1 << (self.0 as usize - Self::LONG_NAME))
    }

    #[inline]
    fn get(self, strtab: &'static [u8], offset: usize) -> &'static [u8] {
        let rest = strtab.get(offset..).unwrap_or(&[]);
        let lower = self.lower_bound().min(rest.len());
        let len = if self.is_long() {
            lower + cstr_at(rest, lower).len()
        } else {
            lower
        };
        &rest[..len]
    }
}

/// Index of an object file in `Context::objs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjId(pub u32);

/// Index of a shared library in `Context::dsos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DsoId(pub u32);

impl ObjId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl DsoId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileId {
    Obj(ObjId),
    Dso(DsoId),
}

impl FileId {
    pub fn is_dso(self) -> bool {
        matches!(self, FileId::Dso(_))
    }

    pub fn as_obj(self) -> Option<ObjId> {
        match self {
            FileId::Obj(id) => Some(id),
            FileId::Dso(_) => None,
        }
    }

    pub fn as_dso(self) -> Option<DsoId> {
        match self {
            FileId::Dso(id) => Some(id),
            FileId::Obj(_) => None,
        }
    }
}

impl From<ObjId> for FileId {
    fn from(id: ObjId) -> FileId {
        FileId::Obj(id)
    }
}

impl From<DsoId> for FileId {
    fn from(id: DsoId) -> FileId {
        FileId::Dso(id)
    }
}

/// State shared by object files and shared libraries.
#[derive(Debug)]
pub struct InputFile {
    pub mf: Option<&'static MappedFile>,
    pub filename: String,

    /// Position in the command line; lower is earlier. Symbol resolution
    /// breaks ties in favor of earlier files.
    pub priority: u32,
    pub is_reachable: AtomicBool,
    pub is_little_endian: bool,
    pub e_flags: u32,

    pub shdrs: ShdrTable<'static>,
    pub shstrtab: &'static [u8],
    pub elf_syms: SymTable,
    pub symbol_strtab: &'static [u8],

    // Parallel to elf_syms; avoids rescanning complete symbol names.
    symbol_name_lengths: Vec<NameLen>,

    /// The symbol for each entry of `elf_syms`, plus any fragment dummies
    /// appended after them.
    pub symbols: Vec<SymbolId>,
    pub first_global: usize,

    pub as_needed: bool,

    // Output symbol table layout
    pub local_symtab_idx: u32,
    pub global_symtab_idx: u32,
    pub num_local_symtab: u32,
    pub num_global_symtab: u32,
    pub strtab_offset: u64,
    pub strtab_size: u64,

    /// Output symbol table index (relative to the file's block) of each
    /// symbol, or -1 if not written. For `--emit-relocs`.
    pub output_sym_indices: Vec<i32>,
}

impl InputFile {
    fn empty(filename: &str) -> InputFile {
        InputFile {
            mf: None,
            filename: filename.to_string(),
            priority: 0,
            is_reachable: AtomicBool::new(false),
            is_little_endian: true,
            e_flags: 0,
            shdrs: ShdrTable::default(),
            shstrtab: &[],
            elf_syms: SymTable::default(),
            symbol_strtab: &[],
            symbol_name_lengths: Vec::new(),
            symbols: Vec::new(),
            first_global: 0,
            as_needed: false,
            local_symtab_idx: 0,
            global_symtab_idx: 0,
            num_local_symtab: 0,
            num_global_symtab: 0,
            strtab_offset: 0,
            strtab_size: 0,
            output_sym_indices: Vec::new(),
        }
    }

    /// Reads the ELF and section headers.
    fn parse<E: Arch>(
        diag: &Diagnostics,
        mf: &'static MappedFile,
        display: &dyn fmt::Display,
    ) -> InputFile {
        let data = mf.data();
        if data.len() < ElfEhdr::size::<E>() {
            fatal!(diag, "{display}: file too small");
        }
        if !data.starts_with(b"\x7fELF") {
            fatal!(diag, "{display}: not an ELF file");
        }

        let ehdr = ElfEhdr::parse::<E>(data);
        let shoff = ehdr.e_shoff as usize;
        let shdr_size = ElfShdr::size::<E>();

        // e_shnum is a 16-bit field. A file with more than 65535 sections
        // stores the real number in the first section header's sh_size.
        let first = data.get(shoff..shoff + shdr_size).map(ElfShdr::parse::<E>);
        let num_sections = match (ehdr.e_shnum, &first) {
            (0, Some(first)) => first.sh_size as usize,
            (n, _) => n as usize,
        };

        let Some(shdr_bytes) = data.get(shoff..shoff + num_sections * shdr_size) else {
            fatal!(
                diag,
                "{}: e_shoff or e_shnum corrupted: {} {num_sections}",
                mf.name,
                data.len()
            );
        };
        let shdrs = ShdrTable::in_file(shdr_bytes, RecordLayout::of::<E>());

        let mut file = InputFile {
            mf: Some(mf),
            is_little_endian: E::IS_LITTLE_ENDIAN,
            e_flags: ehdr.e_flags,
            shdrs,
            ..InputFile::empty(&mf.name)
        };

        // e_shstrndx is likewise a 16-bit field; a large index is stored
        // in the first section header's sh_link.
        let shstrtab_idx = if ehdr.e_shstrndx as u32 == SHN_XINDEX {
            file.shdrs.get(0).map_or(0, |s| s.sh_link as usize)
        } else {
            ehdr.e_shstrndx as usize
        };
        file.shstrtab = file.section_contents_checked(diag, shstrtab_idx, display);
        file
    }

    #[inline]
    pub fn is_reachable(&self) -> bool {
        self.is_reachable.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_reachable(&self, value: bool) {
        self.is_reachable.store(value, Ordering::Relaxed);
    }

    /// Marks the file reachable, returning true if it wasn't already.
    #[inline]
    pub fn mark_reachable(&self) -> bool {
        // A relaxed load + branch (assuming miss) takes only around 20 cycles,
        // while an atomic RMW can easily take hundreds on x86. It is common
        // that another thread beat us in marking, so test optimistically first.
        if self.is_reachable.load(Ordering::Relaxed) {
            return false;
        }
        !self.is_reachable.swap(true, Ordering::Relaxed)
    }

    #[inline]
    pub fn data(&self) -> &'static [u8] {
        self.mf.map_or(&[], MappedFile::data)
    }

    /// The contents of a section, checked against the file size.
    #[inline]
    pub fn section_contents(&self, diag: &Diagnostics, idx: usize) -> &'static [u8] {
        self.section_contents_checked(diag, idx, &self.filename)
    }

    fn section_contents_checked(
        &self,
        diag: &Diagnostics,
        idx: usize,
        display: &dyn fmt::Display,
    ) -> &'static [u8] {
        if idx >= self.shdrs.len() {
            fatal!(diag, "{display}: invalid section index: {idx}");
        }
        let (sh_offset, sh_size) = self.shdrs.sh_offset_and_size(idx);
        self.section_contents_range_checked(diag, sh_offset, sh_size, display)
    }

    /// The contents described by a section header already decoded by the
    /// caller. This avoids decoding its offset and size a second time.
    #[inline]
    pub(crate) fn section_contents_from_shdr(
        &self,
        diag: &Diagnostics,
        shdr: &ElfShdr,
    ) -> &'static [u8] {
        self.section_contents_range_checked(diag, shdr.sh_offset, shdr.sh_size, &self.filename)
    }

    fn section_contents_range_checked(
        &self,
        diag: &Diagnostics,
        sh_offset: u64,
        sh_size: u64,
        display: &dyn fmt::Display,
    ) -> &'static [u8] {
        let data = self.data();
        let start = sh_offset as usize;
        let end = start.saturating_add(sh_size as usize);
        if end > data.len() {
            fatal!(
                diag,
                "{display}: section header is out of range: {sh_offset}"
            );
        }
        &data[start..end]
    }

    #[inline]
    pub fn find_section(&self, sh_type: u32) -> Option<usize> {
        (0..self.shdrs.len()).find(|&i| self.shdrs.sh_type(i) == sh_type)
    }

    #[inline]
    pub fn section_name(&self, shndx: usize) -> &'static [u8] {
        cstr_at(self.shstrtab, self.shdrs.sh_name(shndx) as usize)
    }

    #[inline]
    pub fn symbol_name(&self, i: usize) -> &'static [u8] {
        let offset = self.elf_syms.st_name(i) as usize;
        self.symbol_name_lengths.get(i).map_or_else(
            || cstr_at(self.symbol_strtab, offset),
            |len| len.get(self.symbol_strtab, offset),
        )
    }

    /// Like [`Self::symbol_name`] for code specialized for the target.
    #[inline(always)]
    pub fn symbol_name_in<E: Layout>(&self, i: usize) -> &'static [u8] {
        let offset = self.elf_syms.st_name_in::<E>(i) as usize;
        if let Some(len) = self.symbol_name_lengths.get(i) {
            len.get(self.symbol_strtab, offset)
        } else {
            cstr_at(self.symbol_strtab, offset)
        }
    }

    fn populate_symbol_name_lengths<E: Arch>(&mut self) {
        self.symbol_name_lengths = self
            .elf_syms
            .name_offsets_in::<E>()
            .map(|offset| {
                let offset = offset as usize;
                NameLen::new(cstr_at(self.symbol_strtab, offset).len())
            })
            .collect();
    }

    pub fn global_symbols(&self) -> &[SymbolId] {
        &self.symbols[self.first_global.min(self.symbols.len())..]
    }

    pub fn local_symbols(&self) -> &[SymbolId] {
        &self.symbols[..self.first_global.min(self.symbols.len())]
    }

    /// The source file name recorded as an STT_FILE symbol.
    pub fn source_name(&self, symbols: &SymbolTable) -> Option<&'static [u8]> {
        self.local_symbols()
            .iter()
            .map(|&id| &symbols[id])
            .find(|sym| sym.ty() == STT_FILE)
            .map(|sym| {
                let name: &'static BStr = sym.name();
                &**name
            })
    }
}

// A COMDAT group usually contains an inline function's code and related data,
// such as its string literals. Groups are identified by a signature. If two
// groups have the same signature, the linker keeps one and discards the
// sections in the other.
#[derive(Debug)]
pub struct ComdatGroupRef {
    pub sect_idx: u32,

    // The vector backing comdat_groups is outside the arena. The symbol arena
    // has fewer than 2^31 slots, leaving the high bit for is_owner.
    signature_and_owner: u32,
}

impl ComdatGroupRef {
    const IS_OWNER: u32 = 1 << 31;

    fn new(sect_idx: u32, signature: SymbolId) -> ComdatGroupRef {
        assert!(signature.0 < Self::IS_OWNER);
        ComdatGroupRef {
            sect_idx,
            signature_and_owner: signature.0,
        }
    }

    #[inline]
    pub fn signature(&self) -> SymbolId {
        SymbolId(self.signature_and_owner & !Self::IS_OWNER)
    }

    #[inline]
    pub fn is_owner(&self) -> bool {
        self.signature_and_owner & Self::IS_OWNER != 0
    }

    #[inline]
    pub fn set_owner(&mut self, is_owner: bool) {
        if is_owner {
            self.signature_and_owner |= Self::IS_OWNER;
        } else {
            self.signature_and_owner &= !Self::IS_OWNER;
        }
    }

    /// Returns the word that receives an exceptional signature while COMDAT
    /// metadata is gathered. Ownership has not been selected at that point.
    #[inline]
    pub(crate) fn signature_word_mut(&mut self) -> &mut u32 {
        debug_assert!(!self.is_owner());
        &mut self.signature_and_owner
    }
}

const _: () = assert!(std::mem::size_of::<ComdatGroupRef>() == 8);

/// An exceptional COMDAT signature that must be interned by name. Most groups
/// use their file's existing global symbol and need only `ComdatGroupRef`.
#[derive(Debug)]
pub(crate) struct PendingComdatSignature {
    pub key: &'static [u8],
    pub group_idx: u32,
    pub name_len: u32,
}

const _: () = assert!(std::mem::size_of::<PendingComdatSignature>() == 24);

/// RISC-V attributes read from `.riscv.attributes`.
#[derive(Debug, Default)]
pub struct RiscvAttributes {
    pub stack_align: Option<u64>,
    pub arch: Option<&'static [u8]>,
    pub unaligned_access: bool,
}

/// An input relocatable object file.
#[derive(Debug)]
pub struct ObjectFile {
    pub base: InputFile,
    pub archive_name: String,

    /// The sections by section header index, plus sections synthesized
    /// for common symbols.
    pub sections: SectionList,
    pub sections_parsed: bool,

    /// CREL relocation tables decoded into ordinary records, indexed by
    /// relocation section.
    decoded_crel: Vec<Option<Box<[ElfRel]>>>,

    /// The number of section headers in the file; `base.shdrs` may have
    /// synthesized headers appended after them.
    pub num_elf_sections: usize,

    pub cies: Vec<CieRecord>,
    pub fdes: Vec<FdeRecord>,
    pub has_symver: Vec<bool>,
    pub comdat_groups: Vec<ComdatGroupRef>,
    pub(crate) pending_comdat_signatures: Vec<PendingComdatSignature>,
    pub comdat_discarded: Vec<bool>,

    pub eh_frame_sections: Vec<u32>,
    pub sframe_sections: Vec<u32>,
    pub sframe_fdes: Vec<SFrameFde>,

    pub exclude_libs: bool,
    pub gnu_properties: BTreeMap<u32, u32>,
    pub needs_executable_stack: bool,
    pub is_lto_input: bool,
    pub is_lto_output: bool,
    pub is_gcc_offload_obj: bool,
    pub is_rust_obj: bool,
    pub is_dwarf32: bool,
    pub has_init_array: bool,
    pub has_ctors: bool,

    // Output .eh_frame layout
    pub fde_idx: u64,
    pub fde_offset: u64,
    pub fde_size: u64,

    /// `.llvm_addrsig`, for `--icf=safe`.
    pub llvm_addrsig: Option<InputSection>,

    /// Non-allocated `.debug_info` sections.
    pub debug_info_sections: Vec<u32>,
    pub debug_pubnames: Option<u32>,
    pub debug_pubtypes: Option<u32>,

    // For LTO
    /// COMDAT group signatures of the symbols of an IR object, by symbol
    /// index. IR objects have no sections, so their COMDAT groups are
    /// tracked per symbol.
    pub lto_comdat_keys: Vec<Option<&'static [u8]>>,
    pub lto_comdat_signatures: Vec<Option<SymbolId>>,
    pub lto_comdat_discarded: Vec<bool>,

    pub riscv_attributes: RiscvAttributes,
    /// `.got2` for PPC32.
    pub got2: Option<u32>,

    symtab_shndx: Vec<u32>,
    num_frag_syms: usize,
    num_common_symbols: u32,
}

impl fmt::Display for ObjectFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.archive_name.is_empty() {
            write!(f, "{}", path_clean(&self.base.filename))
        } else {
            write!(
                f,
                "{}({})",
                path_clean(&self.archive_name),
                self.base.filename
            )
        }
    }
}

#[cold]
#[inline(never)]
fn invalid_relocation_symbol(file: &ObjectFile, r_sym: usize) -> ! {
    panic!("{file}: invalid relocation symbol index {r_sym}")
}

#[cold]
#[inline(never)]
fn invalid_relocation_section(file: &ObjectFile, r_sym: usize, shndx: usize) -> ! {
    panic!("{file}: relocation symbol {r_sym} has invalid section index {shndx}")
}

/// Formats a file the way it appears in diagnostics before the file
/// object exists.
struct FileName<'a>(&'a str, &'a str);

impl fmt::Display for FileName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.1.is_empty() {
            write!(f, "{}", path_clean(self.0))
        } else {
            write!(f, "{}({})", path_clean(self.1), self.0)
        }
    }
}

fn is_debug_section(shdr: &ElfShdr, name: &[u8]) -> bool {
    shdr.sh_flags & SHF_ALLOC as u64 == 0 && name.starts_with(b".debug_")
}

fn is_known_section_type<E: Arch>(shdr: &ElfShdr) -> bool {
    let ty = shdr.sh_type;
    let flags = shdr.sh_flags as u32;
    if matches!(
        ty,
        SHT_PROGBITS | SHT_NOTE | SHT_NOBITS | SHT_INIT_ARRAY | SHT_FINI_ARRAY | SHT_PREINIT_ARRAY
    ) {
        return true;
    }
    if (SHT_LOUSER..=SHT_HIUSER).contains(&ty) && flags & SHF_ALLOC == 0 {
        return true;
    }
    if (SHT_LOOS..=SHT_HIOS).contains(&ty) && flags & SHF_OS_NONCONFORMING == 0 {
        return true;
    }
    match E::FAMILY {
        Family::X86_64 if ty == SHT_X86_64_UNWIND => true,
        Family::Arm32 if ty == SHT_ARM_EXIDX || ty == SHT_ARM_ATTRIBUTES => true,
        Family::RiscV if ty == SHT_RISCV_ATTRIBUTES => true,
        _ => false,
    }
}

/// A streaming reader for CREL, an experimental compact relocation encoding
/// that only LLVM produces at the moment.
struct CrelReader<'a> {
    data: &'a [u8],
    nrels: usize,
    scale: u32,
    is_rela: bool,
}

impl<'a> CrelReader<'a> {
    fn new<E: Arch>(diag: &Diagnostics, file: &dyn fmt::Display, mut data: &'a [u8]) -> Self {
        let hdr = read_uleb(&mut data);
        let is_rela = hdr & 0b100 != 0;
        if is_rela && !E::IS_RELA {
            fatal!(
                diag,
                "{file}: CREL with addends is not supported for {}",
                E::NAME
            );
        }
        CrelReader {
            data,
            nrels: (hdr >> 3) as usize,
            scale: (hdr & 0b11) as u32,
            is_rela,
        }
    }

    fn len(&self) -> usize {
        self.nrels
    }

    fn for_each(&self, mut f: impl FnMut(ElfRel, usize)) {
        let mut data = self.data;
        let nflags = if self.is_rela { 3 } else { 2 };
        let (mut offset, mut r_type, mut r_sym, mut addend) = (0u64, 0i64, 0i64, 0i64);

        for i in 0..self.nrels {
            let flags = data[0];
            data = &data[1..];

            // The first byte combines flags with the low bits of an offset
            // delta. A large delta continues as ULEB128 and can wrap the
            // current offset.
            let delta = if flags & 0x80 != 0 {
                (read_uleb(&mut data) << (7 - nflags)) | ((flags & 0x7f) as u64 >> nflags)
            } else {
                (flags >> nflags) as u64
            };
            offset = offset.wrapping_add(delta << self.scale);

            if flags & 1 != 0 {
                r_sym += util::read_sleb(&mut data);
            }
            if flags & 2 != 0 {
                r_type += util::read_sleb(&mut data);
            }
            if self.is_rela && flags & 4 != 0 {
                addend += util::read_sleb(&mut data);
            }

            f(ElfRel::new(offset, r_type as u32, r_sym as u32, addend), i);
        }
    }
}

// SHT_CREL is an experimental alternative relocation table format
// designed to reduce the size of the table. Only LLVM supports it
// at the moment.
//
// This function converts a CREL relocation table to a regular one.
fn decode_crel<E: Arch>(diag: &Diagnostics, file: &dyn fmt::Display, data: &[u8]) -> Box<[ElfRel]> {
    let reader = CrelReader::new::<E>(diag, file, data);
    // Own a fixed-size array without value-initializing elements that the
    // caller is about to overwrite.
    let mut rels = Box::<[ElfRel]>::new_uninit_slice(reader.len());
    reader.for_each(|rel, i| {
        rels[i].write(rel);
    });
    // SAFETY: CrelReader::for_each visits every index from zero to len once.
    unsafe { rels.assume_init() }
}

impl ObjectFile {
    /// Creates the internal object file that holds linker-synthesized
    /// symbols.
    pub fn internal() -> ObjectFile {
        let mut file = ObjectFile::with_base(InputFile::empty("<internal>"), String::new());
        file.sections_parsed = true;
        file.base.set_reachable(true);
        file
    }

    fn with_base(base: InputFile, archive_name: String) -> ObjectFile {
        ObjectFile {
            num_elf_sections: base.shdrs.len(),
            base,
            archive_name,
            sections: SectionList::default(),
            sections_parsed: false,
            decoded_crel: Vec::new(),
            cies: Vec::new(),
            fdes: Vec::new(),
            has_symver: Vec::new(),
            comdat_groups: Vec::new(),
            pending_comdat_signatures: Vec::new(),
            comdat_discarded: Vec::new(),
            eh_frame_sections: Vec::new(),
            sframe_sections: Vec::new(),
            sframe_fdes: Vec::new(),
            exclude_libs: false,
            gnu_properties: BTreeMap::new(),
            needs_executable_stack: false,
            is_lto_input: false,
            is_lto_output: false,
            is_gcc_offload_obj: false,
            is_rust_obj: false,
            is_dwarf32: false,
            has_init_array: false,
            has_ctors: false,
            fde_idx: 0,
            fde_offset: 0,
            fde_size: 0,
            llvm_addrsig: None,
            debug_info_sections: Vec::new(),
            debug_pubnames: None,
            debug_pubtypes: None,
            lto_comdat_keys: Vec::new(),
            lto_comdat_signatures: Vec::new(),
            lto_comdat_discarded: Vec::new(),
            riscv_attributes: RiscvAttributes::default(),
            got2: None,
            symtab_shndx: Vec::new(),
            num_frag_syms: 0,
            num_common_symbols: 0,
        }
    }

    /// Opens an object file and reads its symbol table. Sections are read
    /// later, once COMDAT group selection is done.
    pub fn new<E: Arch>(
        diag: &Diagnostics,
        mf: &'static MappedFile,
        archive_name: String,
    ) -> ObjectFile {
        let display = FileName(&mf.name, &archive_name);
        let base = InputFile::parse::<E>(diag, mf, &display);
        let mut file = ObjectFile::with_base(base, archive_name);
        file.parse_symbols::<E>(diag);
        file
    }

    /// Creates the object for an IR file claimed by the LTO plugin. Its
    /// symbols come from the plugin rather than from an ELF symbol table,
    /// and it has no sections.
    pub fn lto_input<E: Arch>(
        mf: &'static MappedFile,
        archive_name: String,
        elf_syms: SymTable,
        strtab: &'static [u8],
        comdat_keys: Vec<Option<&'static [u8]>>,
    ) -> ObjectFile {
        let mut base = InputFile::empty(&mf.name);
        base.mf = Some(mf);
        base.elf_syms = elf_syms;
        base.symbol_strtab = strtab;
        base.populate_symbol_name_lengths::<E>();
        base.first_global = 1;
        let mut file = ObjectFile::with_base(base, archive_name);
        file.is_lto_input = true;
        file.lto_comdat_signatures = vec![None; comdat_keys.len()];
        file.lto_comdat_discarded = vec![false; comdat_keys.len()];
        file.lto_comdat_keys = comdat_keys;
        file
    }

    pub fn id_display(&self) -> &dyn fmt::Display {
        self
    }

    /// The section index of the symbol at `idx`. Indices too large for
    /// the 16-bit `st_shndx` field are stored in `.symtab_shndx`.
    #[inline]
    pub fn shndx_at(&self, idx: usize) -> usize {
        let st_shndx = self.base.elf_syms.st_shndx(idx);
        self.shndx_from(idx, st_shndx)
    }

    /// Like [`Self::shndx_at`] for code specialized for the target.
    #[inline]
    pub fn shndx_at_in<E: Layout>(&self, idx: usize) -> usize {
        let st_shndx = self.base.elf_syms.st_shndx_in::<E>(idx);
        self.shndx_from(idx, st_shndx)
    }

    /// Resolves an already-read symbol's section index.
    #[inline]
    pub(crate) fn shndx_from(&self, idx: usize, st_shndx: u16) -> usize {
        if st_shndx as u32 == SHN_XINDEX {
            self.symtab_shndx.get(idx).copied().unwrap_or(0) as usize
        } else if st_shndx as u32 >= SHN_LORESERVE {
            0
        } else {
            st_shndx as usize
        }
    }

    /// The section at `shndx`. A section that has been converted to a
    /// mergeable one is returned as well, as a dead section: symbols
    /// defined in it keep referring to it until they're attached to
    /// fragments.
    #[inline]
    pub fn section(&self, shndx: usize) -> Option<&InputSection> {
        self.sections.section(shndx)
    }

    #[inline]
    pub fn section_mut(&mut self, shndx: usize) -> Option<&mut InputSection> {
        self.sections.section_mut(shndx)
    }

    /// The regular section at `shndx`, which must exist.
    #[inline]
    pub fn section_at(&self, shndx: u32) -> &InputSection {
        self.section(shndx as usize).expect("no such input section")
    }

    /// Returns a relocation table from the file unless a compressed table
    /// was decoded into the side table.
    #[inline(always)]
    pub(crate) fn relocations<E: Layout>(&self, relsec_idx: Option<u32>) -> Rels<'_, E> {
        let Some(relsec_idx) = relsec_idx else {
            return Rels::new(&[]);
        };
        if let Some(Some(rels)) = self.decoded_crel.get(relsec_idx as usize) {
            return Rels::decoded(rels);
        }

        Rels::new(self.input_relocation_data::<E>(relsec_idx))
    }

    /// Visits relocations without materializing a deferred CREL table.
    #[inline]
    pub(crate) fn for_each_relocation<E: Arch>(
        &self,
        diag: &Diagnostics,
        relsec_idx: Option<u32>,
        mut f: impl FnMut(ElfRel, usize),
    ) {
        let Some(relsec_idx) = relsec_idx else {
            return;
        };
        let index = relsec_idx as usize;
        if self.base.shdrs.at_in::<E>(index).sh_type == SHT_CREL
            && !self.decoded_crel.get(index).is_some_and(Option::is_some)
        {
            let data = self.input_relocation_data::<E>(relsec_idx);
            CrelReader::new::<E>(diag, self, data).for_each(f);
            return;
        }

        for (i, rel) in self.relocations::<E>(Some(relsec_idx)).iter().enumerate() {
            f(rel, i);
        }
    }

    #[inline(always)]
    fn input_relocation_data<E: Layout>(&self, relsec_idx: u32) -> &'static [u8] {
        // Relocation sections are range-checked when sections are parsed.
        let (offset, size) = self
            .base
            .shdrs
            .sh_offset_and_size_in::<E>(relsec_idx as usize);
        &self.base.data()[offset as usize..(offset + size) as usize]
    }

    #[inline]
    fn relocation_span<E: Layout>(&self, relsec_idx: Option<u32>) -> RelocationSpan {
        let Some(relsec_idx) = relsec_idx else {
            return RelocationSpan::Input(&[]);
        };
        if self
            .decoded_crel
            .get(relsec_idx as usize)
            .is_some_and(Option::is_some)
        {
            RelocationSpan::SideTable(relsec_idx)
        } else {
            RelocationSpan::Input(self.input_relocation_data::<E>(relsec_idx))
        }
    }

    /// Returns relocations for rewriting. Ordinary records live in the
    /// input's private writable mapping; decoded CREL records use the side
    /// table because they have no ordinary in-file representation.
    pub fn rels_mut<E: Layout>(&mut self, shndx: u32) -> RelsMut<'_, E> {
        let Some(relsec_idx) = self.section_at(shndx).relsec_idx() else {
            return RelsMut::new(&mut []);
        };
        let index = relsec_idx as usize;
        if self.decoded_crel.get(index).is_some_and(Option::is_some) {
            return RelsMut::decoded(self.decoded_crel[index].as_deref_mut().unwrap());
        }

        let (offset, size) = self.base.shdrs.sh_offset_and_size_in::<E>(index);
        let mf = self
            .base
            .mf
            .expect("input relocations without a mapped file");
        // SAFETY: a mutable ObjectFile owns this relocation section for the
        // duration of the pass, and section parsing checked its range.
        let data = unsafe { mf.data_mut_ptr(offset as usize..(offset + size) as usize) };
        // SAFETY: the same exclusive ownership applies while the returned
        // relocation view is alive.
        RelsMut::new(unsafe { &mut *data })
    }

    fn set_decoded_crel(&mut self, index: usize, rels: Box<[ElfRel]>) {
        if self.decoded_crel.len() <= index {
            self.decoded_crel.resize_with(index + 1, || None);
        }
        debug_assert!(self.decoded_crel[index].is_none());
        self.decoded_crel[index] = Some(rels);
    }

    #[inline]
    pub fn mergeable_section(&self, shndx: usize) -> Option<&MergeableSection> {
        self.sections.mergeable(shndx)
    }

    #[inline]
    pub fn mergeable_section_mut(&mut self, shndx: usize) -> Option<&mut MergeableSection> {
        self.sections.mergeable_mut(shndx)
    }

    /// The section the symbol at `idx` is defined in.
    #[inline]
    pub fn symbol_section(&self, idx: usize) -> Option<&InputSection> {
        self.section(self.shndx_at(idx))
    }

    /// Whether the symbol at `idx` is defined in a discarded COMDAT group.
    #[inline]
    pub fn is_discarded_comdat(&self, idx: usize) -> bool {
        if self.comdat_discarded.is_empty() {
            return false;
        }
        let st_shndx = self.base.elf_syms.st_shndx(idx) as u32;
        if st_shndx == SHN_ABS || st_shndx == SHN_COMMON {
            return false;
        }
        self.comdat_discarded[self.shndx_from(idx, st_shndx as u16)]
    }

    #[inline]
    fn is_discarded_comdat_sym(&self, idx: usize, esym: &ElfSym) -> bool {
        if self.comdat_discarded.is_empty() || esym.is_abs() || esym.is_common() {
            return false;
        }
        self.comdat_discarded[self.shndx_from(idx, esym.st_shndx)]
    }

    /// Iterates over the live regular sections.
    #[inline]
    pub fn input_sections(&self) -> impl Iterator<Item = &InputSection> {
        self.sections.regular()
    }

    #[inline]
    pub fn input_sections_mut(&mut self) -> impl Iterator<Item = &mut InputSection> {
        self.sections.regular_mut()
    }

    #[inline]
    pub fn mergeable_sections(&self) -> impl Iterator<Item = &MergeableSection> {
        self.sections.mergeable_sections()
    }

    #[inline]
    pub fn mergeable_sections_mut(&mut self) -> impl Iterator<Item = &mut MergeableSection> {
        self.sections.mergeable_sections_mut()
    }

    /// The section indices of a COMDAT group's members, read from the
    /// group section as they are needed.
    #[inline]
    pub fn comdat_members(&self, group: &ComdatGroupRef) -> impl Iterator<Item = u32> + '_ {
        let data = self.base.data();
        let (sh_offset, sh_size) = self.base.shdrs.sh_offset_and_size(group.sect_idx as usize);
        let start = sh_offset as usize;
        let bytes = &data[start..start + sh_size as usize];
        let is_little_endian = self.base.is_little_endian;
        bytes.chunks_exact(4).skip(1).map(move |b| {
            let b = [b[0], b[1], b[2], b[3]];
            if is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        })
    }

    /// Marks a section dead along with its FDEs.
    pub fn kill_section(&self, shndx: usize) {
        if let Some(isec) = self.section(shndx) {
            if isec.kill() {
                for fde in isec.fdes(self) {
                    fde.kill();
                }
            }
        }
    }

    fn parse_symbols<E: Arch>(&mut self, diag: &Diagnostics) {
        if let Some(idx) = self.base.find_section(SHT_SYMTAB) {
            let shdr = self.base.shdrs.at(idx);
            // In ELF, all local symbols precede global symbols in the
            // symbol table; sh_info is the index of the first global.
            self.base.first_global = shdr.sh_info as usize;
            let contents = self.base.section_contents(diag, idx);
            if !contents.len().is_multiple_of(ElfSym::size::<E>()) {
                fatal!(diag, "{self}: corrupted section");
            }
            self.base.elf_syms = SymTable::in_file(contents, RecordLayout::of::<E>());
            self.base.symbol_strtab = self.base.section_contents(diag, shdr.sh_link as usize);
            self.base.populate_symbol_name_lengths::<E>();

            if let Some(idx) = self.base.find_section(SHT_SYMTAB_SHNDX) {
                let bytes = self.base.section_contents(diag, idx);
                self.symtab_shndx = bytes.chunks_exact(4).map(E::Endian::read_u32).collect();
            }
        }
    }

    /// Registers the global symbols in this worker's symbol-table bin. The
    /// complete slot array is allocated first and does not move before gather.
    pub(crate) fn register_global_symbols<E: Arch>(
        &mut self,
        args: &Args,
        bins: &mut Bins<SymbolSlot>,
    ) {
        let n = self.base.elf_syms.len();
        if n == 0 {
            return;
        }
        self.base.symbols = vec![SymbolId::DISCARDED_COMDAT; n];
        let num_globals = n.saturating_sub(self.base.first_global);
        self.has_symver = vec![false; num_globals];

        for i in self.base.first_global..n {
            let esym = self.base.elf_syms.at_in::<E>(i);
            if esym.is_common() {
                self.num_common_symbols += 1;
            }

            let mut key = self.base.symbol_name_in::<E>(i);
            let mut name = key;

            // Parse a symbol version after an atsign. A default version
            // (`@@`) is dropped from the key; any other stays part of it,
            // and its length is remembered so that the key's name needn't
            // be scanned for the atsign again.
            let mut ver_len = 0;
            if let Some(pos) = util::find_byte(b'@', name) {
                let ver = &name[pos..];
                name = &name[..pos];
                if ver.starts_with(b"@@") {
                    key = name;
                } else {
                    ver_len = ver.len();
                }
                if ver != b"@" {
                    self.has_symver[i - self.base.first_global] = true;
                }
            }

            // Handle --wrap
            if esym.is_undef() && !args.wrap.is_empty() {
                let as_str = |s: &[u8]| String::from_utf8_lossy(s).into_owned();
                if let Some(real) = name.strip_prefix(b"__real_") {
                    if args.wrap.contains(&as_str(real)) {
                        key = &key[7..];
                    }
                } else if args.wrap.contains(&as_str(key)) {
                    key = leak_bytes([b"__wrap_", key].concat());
                }
            }
            bins.record_hashed(
                key,
                hash_key(key),
                key.len() - ver_len,
                SymbolSlot::new(&mut self.base.symbols[i]),
            );
        }
    }

    /// Reads COMDAT groups and detects GCC offload objects. Both affect
    /// which input sections can be discarded before LTO.
    pub fn read_section_metadata<E: Arch>(&mut self, diag: &Diagnostics) {
        debug_assert!(!self.sections_parsed);

        for i in 0..self.num_elf_sections {
            // SAFETY: `i` comes from the section-header table's range.
            let (sh_type, sh_flags) =
                unsafe { self.base.shdrs.file_type_and_flags_in_unchecked::<E>(i) };

            if sh_flags & SHF_EXCLUDE as u64 != 0
                && self
                    .base
                    .section_name(i)
                    .starts_with(b".gnu.offload_lto_.symtab.")
            {
                self.is_gcc_offload_obj = true;
            }

            if sh_type != SHT_GROUP {
                continue;
            }
            // SAFETY: `i` comes from the section-header table's range.
            let shdr = unsafe { self.base.shdrs.file_at_in_unchecked::<E>(i) };
            if shdr.sh_info as usize >= self.base.elf_syms.len() {
                fatal!(diag, "{self}: invalid symbol index");
            }

            let esym = self.base.elf_syms.at(shdr.sh_info as usize);
            let name = if esym.st_type() == STT_SECTION {
                self.base
                    .section_name(self.shndx_from(shdr.sh_info as usize, esym.st_shndx))
            } else {
                self.base.symbol_name_in::<E>(shdr.sh_info as usize)
            };

            // Ignore a broken comdat group GCC emits for .debug_macros.
            // https://github.com/rui314/mold/issues/438
            if name.starts_with(b"wm4.") {
                continue;
            }

            let contents = self.base.section_contents_from_shdr(diag, &shdr);
            if contents.len() < 4 {
                fatal!(diag, "{self}: empty SHT_GROUP");
            }
            let kind = E::Endian::read_u32(contents);
            if kind == 0 {
                continue;
            }
            if kind != GRP_COMDAT {
                fatal!(diag, "{self}: unsupported SHT_GROUP format");
            }

            // Ordinary global signatures already have a Symbol. Local, section
            // and versioned signatures use the same symbol table through their
            // full name.
            let version = util::find_byte(b'@', name);
            let is_own_global = esym.st_type() != STT_SECTION
                && esym.st_bind() != STB_LOCAL
                && !esym.is_undef()
                && version.is_none();

            let signature = if is_own_global {
                self.base.symbols[shdr.sh_info as usize]
            } else {
                SymbolId::DISCARDED_COMDAT
            };
            let group_idx = self.comdat_groups.len() as u32;
            self.comdat_groups
                .push(ComdatGroupRef::new(i as u32, signature));
            if !is_own_global {
                self.pending_comdat_signatures.push(PendingComdatSignature {
                    key: name,
                    group_idx,
                    name_len: version.unwrap_or(name.len()) as u32,
                });
            }
        }
    }

    /// Counts relocations referring to the section symbol of a mergeable
    /// section; each gets a dummy symbol in `reattach_section_pieces`.
    fn count_frag_syms<E: Layout>(&self, rels: Rels<'_, E>) -> usize {
        let mut count = 0;
        for r_sym in rels.sym_indices().map(|i| i as usize) {
            let Some(st_info) = self.base.elf_syms.st_info_in_checked::<E>(r_sym) else {
                invalid_relocation_symbol(self, r_sym);
            };
            if (st_info & 0xf) as u32 != STT_SECTION {
                continue;
            }

            // SAFETY: `st_info_in_checked` just proved the symbol index.
            let st_shndx = unsafe { self.base.elf_syms.st_shndx_in_unchecked::<E>(r_sym) };
            let shndx = self.shndx_from(r_sym, st_shndx);
            let Some(flags) = self.base.shdrs.sh_flags_in_checked::<E>(shndx) else {
                invalid_relocation_section(self, r_sym, shndx);
            };
            count += usize::from(flags & SHF_MERGE as u64 != 0);
        }
        count
    }

    fn parse_note_gnu_property<E: Arch>(&mut self, mut data: &'static [u8]) {
        while data.len() >= ElfNhdr::size::<E>() {
            let hdr = ElfNhdr::parse::<E>(data);
            data = &data[ElfNhdr::size::<E>()..];

            let name_len = hdr.n_namesz as usize;
            let name = &data[..name_len.saturating_sub(1).min(data.len())];
            data = &data[(align_to(name_len as u64, 4) as usize).min(data.len())..];

            let desc_len = hdr.n_descsz as usize;
            let mut desc = &data[..desc_len.min(data.len())];
            data =
                &data[(align_to(desc_len as u64, E::WORD_SIZE as u64) as usize).min(data.len())..];

            if hdr.n_type != NT_GNU_PROPERTY_TYPE_0 || name != b"GNU" {
                continue;
            }

            while desc.len() >= 8 {
                let ty = E::Endian::read_u32(desc);
                let size = E::Endian::read_u32(&desc[4..]) as usize;
                desc = &desc[8..];

                // Most properties are 32-bit values; skip anything else
                // (e.g. GNU_PROPERTY_STACK_SIZE).
                if size == 4 && desc.len() >= 4 {
                    *self.gnu_properties.entry(ty).or_insert(0) |= E::Endian::read_u32(desc);
                }
                desc =
                    &desc[(align_to(size as u64, E::WORD_SIZE as u64) as usize).min(desc.len())..];
            }
        }
    }

    fn read_riscv_attributes<E: Arch>(&mut self, diag: &Diagnostics, data: &'static [u8]) {
        if data.is_empty() {
            fatal!(diag, "{self}: corrupted .riscv.attributes section");
        }
        if data[0] != b'A' {
            return;
        }
        let mut data = &data[1..];

        while !data.is_empty() {
            let sz = E::Endian::read_u32(data) as usize;
            if data.len() < sz || sz < 4 {
                fatal!(diag, "{self}: corrupted .riscv.attributes section");
            }
            let mut p = &data[4..sz];
            data = &data[sz..];

            let Some(rest) = p.strip_prefix(b"riscv\0") else {
                continue;
            };
            p = rest;
            if p.first() != Some(&(ELF_TAG_FILE as u8)) {
                fatal!(diag, "{self}: corrupted .riscv.attributes section");
            }
            p = &p[5..]; // skip the tag and the sub-sub-section size

            while !p.is_empty() {
                let tag = read_uleb(&mut p) as u32;
                match tag {
                    ELF_TAG_RISCV_STACK_ALIGN => {
                        self.riscv_attributes.stack_align = Some(read_uleb(&mut p))
                    }
                    ELF_TAG_RISCV_ARCH => {
                        let end = p.iter().position(|&b| b == 0).unwrap_or(p.len());
                        self.riscv_attributes.arch = Some(&p[..end]);
                        p = &p[(end + 1).min(p.len())..];
                    }
                    ELF_TAG_RISCV_UNALIGNED_ACCESS => {
                        self.riscv_attributes.unaligned_access = read_uleb(&mut p) != 0
                    }
                    _ => {}
                }
            }
        }
    }

    fn initialize_sections<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        args: &Args,
        id: ObjId,
        section_arena: &crate::input_sections::SectionArena,
    ) {
        let nsections = self.num_elf_sections;
        let expected_reloc_type = if E::IS_RELA { SHT_RELA } else { SHT_REL };
        debug_assert!(self.comdat_discarded.is_empty() || self.comdat_discarded.len() == nsections);
        for i in 0..nsections {
            if !self.comdat_discarded.is_empty()
                // SAFETY: a nonempty COMDAT bitmap has one entry per input
                // section, and `i` is in that range.
                && unsafe { *self.comdat_discarded.get_unchecked(i) }
            {
                continue;
            }

            // SAFETY: `i` comes from the file's section-header range.
            let (sh_type, flags) =
                unsafe { self.base.shdrs.file_type_and_flags_in_unchecked::<E>(i) };
            if flags & SHF_EXCLUDE as u64 != 0
                && flags & SHF_ALLOC as u64 == 0
                && sh_type != SHT_LLVM_ADDRSIG
                && !args.relocatable
            {
                continue;
            }

            if E::IS_ARM && sh_type == SHT_ARM_ATTRIBUTES {
                continue;
            }
            if E::IS_RISCV && sh_type == SHT_RISCV_ATTRIBUTES {
                // SAFETY: `i` comes from the file's section-header range.
                let shdr = unsafe { self.base.shdrs.file_at_in_unchecked::<E>(i) };
                let contents = self.base.section_contents_from_shdr(diag, &shdr);
                self.read_riscv_attributes::<E>(diag, contents);
                continue;
            }

            match sh_type {
                SHT_GROUP | SHT_SYMTAB | SHT_SYMTAB_SHNDX | SHT_STRTAB | SHT_NULL => {}
                SHT_REL | SHT_RELA => {
                    if sh_type != expected_reloc_type {
                        continue;
                    }
                    // SAFETY: `i` comes from the file's section-header range.
                    let shdr = unsafe { self.base.shdrs.file_at_in_unchecked::<E>(i) };
                    let target = shdr.sh_info as usize;
                    let Some(target_flags) = self.base.shdrs.sh_flags_in_checked::<E>(target)
                    else {
                        continue;
                    };
                    if target_flags & SHF_ALLOC as u64 != 0 {
                        let contents = self.base.section_contents_from_shdr(diag, &shdr);
                        if !contents.len().is_multiple_of(ElfRel::size::<E>()) {
                            fatal!(diag, "{self}: corrupted section");
                        }
                        self.num_frag_syms += self.count_frag_syms(Rels::<E>::new(contents));
                    }
                    // Relocations are attached to their sections below.
                }
                SHT_CREL => {
                    // SAFETY: `i` comes from the file's section-header range.
                    let shdr = unsafe { self.base.shdrs.file_at_in_unchecked::<E>(i) };
                    let target = shdr.sh_info as usize;
                    let Some(target_flags) = self.base.shdrs.sh_flags_in_checked::<E>(target)
                    else {
                        continue;
                    };
                    let target_is_alloc = target_flags & SHF_ALLOC as u64 != 0;
                    if target_is_alloc || args.relocatable || args.emit_relocs {
                        let contents = self.base.section_contents_from_shdr(diag, &shdr);
                        let decoded = decode_crel::<E>(diag, self, contents);

                        // Count the relocations just decoded while they are in cache.
                        if target_is_alloc {
                            self.num_frag_syms +=
                                self.count_frag_syms(Rels::<E>::decoded(&decoded));
                        }
                        self.set_decoded_crel(i, decoded);
                    }
                    // Relocations are attached to their sections below.
                }
                _ => {
                    // SAFETY: `i` comes from the file's section-header range.
                    let shdr = unsafe { self.base.shdrs.file_at_in_unchecked::<E>(i) };
                    let name = cstr_at(self.base.shstrtab, shdr.sh_name as usize);
                    if !is_known_section_type::<E>(&shdr) {
                        fatal!(
                            diag,
                            "{self}: {}: unsupported section type: 0x{:x}",
                            util::display(name),
                            shdr.sh_type
                        );
                    }

                    // .note.GNU-stack controls executable-ness of the stack
                    // in GNU linkers. We ignore it because silently making
                    // the stack executable is too dangerous, but tell the
                    // user if that matters.
                    if name == b".note.GNU-stack" && !args.relocatable {
                        if flags & SHF_EXECINSTR as u64 != 0 {
                            if !args.z_execstack && !args.z_execstack_if_needed {
                                warn!(
                                    diag,
                                    "{self}: this file may cause a segmentation fault because it requires an executable stack. See https://github.com/rui314/mold/tree/main/docs/execstack.md for more info."
                                );
                            }
                            self.needs_executable_stack = true;
                        }
                        continue;
                    }

                    if name == b".note.gnu.property" {
                        let contents = self.base.section_contents_from_shdr(diag, &shdr);
                        self.parse_note_gnu_property::<E>(contents);
                        continue;
                    }

                    // A build-id section in an input file is unusual but
                    // possible (`ld.bfd -r --build-id`).
                    if name == b".note.gnu.build-id" {
                        continue;
                    }

                    // Old glibc i386 CRT files and ICC emit these.
                    if name == b".gnu.linkonce.t.__x86.get_pc_thunk.bx"
                        || name == b".gnu.linkonce.t.__i686.get_pc_thunk.bx"
                        || name == b".gnu.linkonce.d.DW.ref.__gxx_personality_v0"
                    {
                        continue;
                    }

                    if (args.strip_all || args.strip_debug) && is_debug_section(&shdr, name) {
                        continue;
                    }

                    if !args.discard_section.is_empty()
                        && args
                            .discard_section
                            .contains(&*String::from_utf8_lossy(name))
                    {
                        continue;
                    }

                    if name == b".comment"
                        && self
                            .base
                            .section_contents_from_shdr(diag, &shdr)
                            .starts_with(b"rustc ")
                    {
                        self.is_rust_obj = true;
                    }

                    // Without a section header (--oformat=binary), non-alloc
                    // sections have no place in the output.
                    if args.oformat_binary && flags & SHF_ALLOC as u64 == 0 {
                        continue;
                    }

                    let isec =
                        InputSection::new::<E>(diag, self, id, i as u32, &shdr, BStr::new(name));

                    // Save .llvm_addrsig for --icf=safe. Tools that mutate the
                    // symbol table tend not to preserve sh_link, so a section
                    // with sh_link == 0 is ignored.
                    if shdr.sh_type == SHT_LLVM_ADDRSIG && !args.relocatable {
                        if shdr.sh_link != 0 {
                            self.llvm_addrsig = Some(isec);
                        }
                        continue;
                    }

                    if matches!(
                        shdr.sh_type,
                        SHT_INIT_ARRAY | SHT_FINI_ARRAY | SHT_PREINIT_ARRAY
                    ) {
                        self.has_init_array = true;
                    }
                    if name == b".ctors"
                        || name.starts_with(b".ctors.")
                        || name == b".dtors"
                        || name.starts_with(b".dtors.")
                    {
                        self.has_ctors = true;
                    }
                    if name == b".eh_frame" {
                        self.eh_frame_sections.push(i as u32);
                    }
                    if E::SUPPORTS_SFRAME && name == b".sframe" {
                        self.sframe_sections.push(i as u32);
                    }
                    if name == b".debug_info" && flags & SHF_ALLOC as u64 == 0 {
                        self.debug_info_sections.push(i as u32);
                    }
                    if E::FAMILY == Family::Ppc32 && name == b".got2" {
                        self.got2 = Some(i as u32);
                    }

                    // With --gdb-index, .debug_gnu_pubnames and
                    // .debug_gnu_pubtypes are consumed by .gdb_index.
                    if args.gdb_index {
                        if name == b".debug_gnu_pubnames" {
                            self.debug_pubnames = Some(i as u32);
                            isec.kill();
                        }
                        if name == b".debug_gnu_pubtypes" {
                            self.debug_pubtypes = Some(i as u32);
                            isec.kill();
                        }
                        if name == b".debug_types" {
                            fatal!(
                                diag,
                                "{self}: mold's --gdb-index is not compatible with .debug_types; to fix this error, remove -fdebug-types-section and recompile"
                            );
                        }
                    }

                    self.sections.insert(i, isec, section_arena);
                }
            }
        }

        // Attach relocation tables to their sections.
        for i in 0..nsections {
            // SAFETY: `i` comes from the file's section-header range.
            let sh_type = unsafe { self.base.shdrs.file_type_in_unchecked::<E>(i) };
            if sh_type != expected_reloc_type && sh_type != SHT_CREL {
                continue;
            }
            // SAFETY: `i` comes from the file's section-header range.
            let shdr = unsafe { self.base.shdrs.file_at_in_unchecked::<E>(i) };
            let target = shdr.sh_info as usize;
            if self.section(target).is_none() {
                continue;
            }

            let has_relocs = if sh_type == SHT_CREL {
                if let Some(Some(decoded)) = self.decoded_crel.get(i) {
                    !decoded.is_empty()
                } else {
                    let contents = self.base.section_contents_from_shdr(diag, &shdr);
                    CrelReader::new::<E>(diag, self, contents).len() != 0
                }
            } else {
                shdr.sh_size != 0
            };

            let isec = self.section_mut(target).unwrap();
            debug_assert!(!isec.has_relsec());
            isec.set_relsec(i as u32, has_relocs);
        }

        // Attach .ARM.exidx sections to the sections they describe.
        if E::FAMILY == Family::Arm32 {
            let pairs: Vec<(usize, usize)> = self
                .input_sections()
                .filter(|isec| isec.sh_type(self) == SHT_ARM_EXIDX)
                .map(|isec| {
                    (
                        self.base.shdrs.at(isec.shndx as usize).sh_link as usize,
                        isec.shndx as usize,
                    )
                })
                .collect();
            for (target, exidx) in pairs {
                if let Some(isec) = self.section_mut(target) {
                    isec.set_exidx(exidx as u32, section_arena);
                }
            }
        }
    }

    /// Relocations are usually sorted by offset, but RISC-V and LoongArch
    /// object files don't always follow that convention.
    fn sort_relocations<E: Arch>(&mut self) {
        if !E::IS_RISCV && !E::IS_LOONGARCH {
            return;
        }
        let sections: Vec<u32> = self.input_sections().map(|isec| isec.shndx).collect();
        for shndx in sections {
            let isec = self.section_at(shndx);
            if !isec.is_alive() || !isec.is_alloc() {
                continue;
            }
            let rels = isec.rels::<E>(self);
            if !rels.iter().map(|r| r.r_offset).is_sorted() {
                let mut sorted: Vec<ElfRel> = rels.iter().collect();
                sorted.sort_by_key(|r| r.r_offset);
                self.rels_mut::<E>(shndx).set_all(&sorted);
            }
        }
    }

    /// Constructs sections after COMDAT ownership is known. Members of losing
    /// groups are normally skipped. If another selection will run after LTO,
    /// construct them too so a different copy can become live.
    pub(crate) fn parse_sections<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        args: &Args,
        id: ObjId,
        section_arena: &crate::input_sections::SectionArena,
        allocator: &ParallelSymbolAllocator<'_>,
        keep_discarded_comdat: bool,
    ) {
        debug_assert!(!self.sections_parsed);
        let n = self.base.shdrs.len();
        self.sections = SectionList::new(n, self.num_common_symbols as usize, section_arena);

        if !keep_discarded_comdat && !self.comdat_groups.is_empty() {
            let mut discarded = vec![false; n];
            for group in self.comdat_groups.iter().filter(|group| !group.is_owner()) {
                for member in self.comdat_members(group) {
                    if let Some(slot) = discarded.get_mut(member as usize) {
                        *slot = true;
                    }
                }
            }
            self.comdat_discarded = discarded;
        }

        let count = self.num_local_symbols();
        // SAFETY: initialize_local_symbols fills all `count` slots, including
        // any unused tail, before the parallel allocator publishes the final
        // symbol-table length.
        unsafe {
            allocator.allocate(count, |base_id, slots| {
                self.initialize_sections::<E>(diag, args, id, section_arena);
                self.initialize_local_symbols::<E>(diag, id, base_id, slots);
                self.sort_relocations::<E>();
                self.sections_parsed = true;
            });
        }
    }

    /// The number of local symbols this file contributes to the symbol
    /// table: the null symbol plus every local not in a discarded COMDAT.
    #[inline]
    pub fn num_local_symbols(&self) -> usize {
        if self.base.elf_syms.is_empty() {
            return 0;
        }
        1 + (1..self.base.first_global)
            .filter(|&i| !self.is_discarded_comdat(i))
            .count()
    }

    /// Fills in the local symbols. `slots` has `num_local_symbols()`
    /// entries whose ids start at `base_id`; every one of them is
    /// initialized, those left over with blank symbols.
    pub fn initialize_local_symbols<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        id: ObjId,
        base_id: SymbolId,
        slots: &mut [MaybeUninit<Symbol>],
    ) {
        let mut next = 0;
        if !slots.is_empty() && !self.base.elf_syms.is_empty() {
            next = self.initialize_local_symbols_into::<E>(diag, id, base_id, slots);
        }
        for slot in &mut slots[next..] {
            slot.write(Symbol::new(BStr::new(b"")));
        }
    }

    /// Returns the number of slots written.
    fn initialize_local_symbols_into<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        id: ObjId,
        base_id: SymbolId,
        slots: &mut [MaybeUninit<Symbol>],
    ) -> usize {
        let file_id = FileId::Obj(id);

        let mut first = Symbol::new(BStr::new(b""));
        first.set_file(file_id);
        first.sym_idx = 0;
        slots[0].write(first);
        self.base.symbols[0] = base_id;

        let mut next = 1;
        for i in 1..self.base.first_global {
            let esym = self.base.elf_syms.at_in::<E>(i);
            if esym.is_common() {
                fatal!(diag, "{self}: common local symbol?");
            }
            if self.is_discarded_comdat_sym(i, &esym) {
                self.base.symbols[i] = SymbolId::DISCARDED_COMDAT;
                continue;
            }

            let shndx = (!esym.is_abs()).then(|| self.shndx_from(i, esym.st_shndx));

            let name: &'static [u8] = if esym.st_type() == STT_SECTION {
                let shndx = shndx.unwrap();
                match self.section(shndx) {
                    Some(isec) => isec.name(self),
                    None => self.base.section_name(shndx),
                }
            } else {
                self.base.symbol_name_in::<E>(i)
            };

            let mut sym = Symbol::new(BStr::new(name));
            sym.set_file(file_id);
            sym.value = esym.st_value;
            sym.sym_idx = i as u32;
            sym.set_esym(&esym);
            sym.set_rust(self.is_rust_obj);
            if let Some(shndx) = shndx {
                if let Some(section) = self.section(shndx) {
                    sym.set_input_section(section);
                }
            }
            slots[next].write(sym);
            self.base.symbols[i] = SymbolId(base_id.0 + next as u32);
            next += 1;
        }
        debug_assert_eq!(next, slots.len());
        next
    }

    /// Parses `.eh_frame` sections into CIE and FDE records.
    ///
    /// Unlike most sections, `.eh_frame` is not copied opaquely: FDEs of
    /// dead functions are dropped, identical CIEs are merged, and the
    /// records are indexed for `.eh_frame_hdr`.
    pub fn parse_ehframe<E: Arch>(&mut self, diag: &Diagnostics) {
        let eh_frame_sections = std::mem::take(&mut self.eh_frame_sections);
        for &shndx in &eh_frame_sections {
            let isec = self.section_at(shndx);
            let contents = isec.contents();
            let relocations = self.relocation_span::<E>(isec.relsec_idx());
            let rels = isec.rels::<E>(self);
            let cies_begin = self.cies.len();
            let fdes_begin = self.fdes.len();
            let mut new_cies: Vec<CieRecord> = Vec::new();
            let mut new_fdes: Vec<FdeRecord> = Vec::new();

            let mut rel_idx = 0;
            let mut pos = 0;
            while pos + 4 <= contents.len() {
                let size = E::Endian::read_u32(&contents[pos..]) as usize;
                if size == 0 {
                    break;
                }
                let begin_offset = pos;
                let end_offset = pos + size + 4;
                let id = E::Endian::read_u32(&contents[pos + 4..]);
                pos = end_offset;

                let rel_begin = rel_idx;
                while rel_idx < rels.len() && (rels.at(rel_idx).r_offset as usize) < end_offset {
                    rel_idx += 1;
                }

                if id == 0 {
                    let mut cie = CieRecord {
                        section: shndx,
                        contents,
                        input_offset: begin_offset as u32,
                        output_offset: 0,
                        rel_idx: rel_begin as u32,
                        relocations,
                        icf_idx: 0,
                        fde_ptr_size: 0,
                        is_leader: false,
                    };
                    cie.fde_ptr_size = parse_fde_encoding::<E>(
                        diag,
                        self,
                        isec,
                        &contents[begin_offset..end_offset],
                    );
                    new_cies.push(cie);
                } else {
                    // An FDE without a valid relocation is dead from the
                    // start; `ld -r` tends to produce such FDEs.
                    if rel_begin == rel_idx || rels.at(rel_begin).r_sym == 0 {
                        continue;
                    }
                    if rels.at(rel_begin).r_offset as usize - begin_offset != 8 {
                        fatal!(
                            diag,
                            "{}: FDE's first relocation should have offset 8",
                            isec.display(self)
                        );
                    }
                    // The function may belong to a discarded COMDAT group.
                    if self
                        .symbol_section(rels.at(rel_begin).r_sym as usize)
                        .is_none()
                    {
                        continue;
                    }
                    new_fdes.push(FdeRecord::new(begin_offset as u32, rel_begin as u32));
                }
            }

            // Associate CIEs with FDEs.
            for fde in &mut new_fdes {
                let off = fde.input_offset as usize + 4;
                let cie_offset = E::Endian::read_i32(&contents[off..]) as i64;
                let target = off as i64 - cie_offset;
                let Some(ci) = new_cies
                    .iter()
                    .position(|c| c.input_offset as i64 == target)
                else {
                    fatal!(diag, "{}: bad FDE pointer", isec.display(self));
                };
                fde.cie_idx = (cies_begin + ci) as u16;
            }

            self.cies.extend(new_cies);
            self.fdes.extend(new_fdes);
            let _ = fdes_begin;
            self.kill_section(shndx as usize);
        }
        self.eh_frame_sections = eh_frame_sections;

        // Group FDEs by the section they describe, keeping FDEs of the
        // same section contiguous.
        let section_of = |file: &ObjectFile, fde: &FdeRecord| -> usize {
            let rel = fde.rels::<E>(file).at(0);
            file.shndx_at_in::<E>(rel.r_sym as usize)
        };
        let mut order: Vec<(u64, usize, usize)> = self
            .fdes
            .iter()
            .enumerate()
            .map(|(i, fde)| {
                let shndx = section_of(self, fde);
                (self.section_at(shndx as u32).priority(self), i, shndx)
            })
            .collect();
        order.sort();
        let fdes = std::mem::take(&mut self.fdes);
        let mut sorted: Vec<Option<FdeRecord>> = fdes.into_iter().map(Some).collect();
        self.fdes = order
            .iter()
            .map(|&(_, i, _)| sorted[i].take().unwrap())
            .collect();
        let shndxs: Vec<usize> = order.iter().map(|&(_, _, s)| s).collect();

        // Associate FDEs with input sections.
        let mut i = 0;
        while i < shndxs.len() {
            let begin = i;
            let shndx = shndxs[i];
            while i < shndxs.len() && shndxs[i] == shndx {
                i += 1;
            }
            self.fdes[i - 1].is_last = true;
            if self.section_at(shndx as u32).is_alive() {
                self.section_mut(shndx).unwrap().fde_begin = begin as u32;
            } else {
                for fde in &self.fdes[begin..i] {
                    fde.kill();
                }
            }
        }
    }

    /// Parses `.sframe` sections into function descriptor entries.
    pub fn parse_sframe<E: Arch>(&mut self, diag: &Diagnostics) {
        let Some(abi) = E::SFRAME_ABI else {
            return;
        };
        let sections = std::mem::take(&mut self.sframe_sections);
        for &shndx in &sections {
            // The section is reconstructed into the output .sframe.
            self.kill_section(shndx as usize);
            let isec = self.section_at(shndx);
            let data = isec.contents();

            if data.len() < SFrameHeader::size::<E>() {
                fatal!(diag, "{}: corrupted .sframe section", isec.display(self));
            }
            let hdr = SFrameHeader::parse::<E>(data);
            if hdr.magic != SFRAME_MAGIC {
                fatal!(diag, "{}: corrupted .sframe section", isec.display(self));
            }
            if hdr.abi_arch != abi {
                fatal!(
                    diag,
                    "{}: .sframe is incompatible with {}",
                    isec.display(self),
                    E::NAME
                );
            }
            // Only SFrame version 3 is supported; older sections are
            // ignored rather than rejected.
            if hdr.version != 3 {
                continue;
            }

            let hdr_len = SFrameHeader::size::<E>() + hdr.auxhdr_len as usize;
            let fde_off = hdr_len + hdr.fdeoff as usize;
            let fre_off = hdr_len + hdr.freoff as usize;
            let rels = isec.rels::<E>(self);
            let mut rel_idx = 0;
            let mut new_fdes = Vec::new();

            for i in 0..hdr.num_fdes as usize {
                let idx_off = fde_off + i * SFrameFdeIdx::size::<E>();
                let ent = SFrameFdeIdx::parse::<E>(&data[idx_off..]);

                // An FDE without a func_start relocation isn't tied to any
                // function, so drop it.
                while rel_idx < rels.len() && (rels.at(rel_idx).r_offset as usize) < idx_off {
                    rel_idx += 1;
                }
                if rel_idx == rels.len() || rels.at(rel_idx).r_offset as usize != idx_off {
                    continue;
                }
                let rel = &rels.at(rel_idx);
                let off = fre_off + ent.func_start_fre_off as usize;

                let Some(func) = self.symbol_section(rel.r_sym as usize) else {
                    continue;
                };
                let fre = &data[off..off + sframe_fre_block_size::<E>(data, off)];
                new_fdes.push(SFrameFde {
                    section: func.shndx,
                    sym: self.base.symbols[rel.r_sym as usize],
                    addend: rel.r_addend,
                    fre,
                    func_size: ent.func_size,
                    num_fres: E::Endian::read_u16(&data[off..]) as u32,
                });
            }
            self.sframe_fdes.extend(new_fdes);
        }
        self.sframe_sections = sections;
    }

    /// Converts sections with SHF_MERGE into mergeable sections. Files
    /// are converted in parallel, so the merged sections are behind a
    /// lock while they are being looked up and created.
    pub fn convert_mergeable_sections<E: Arch>(
        &mut self,
        ctx_args: &Args,
        merged: &RwLock<Vec<MergedSection>>,
        diag: &Diagnostics,
    ) {
        let mut sections = std::mem::take(&mut self.sections);
        for i in 0..sections.len() {
            let Some(isec) = sections.section(i) else {
                continue;
            };
            if isec.sh_size == 0 || isec.has_relsec() || isec.sh_flags & SHF_MERGE as u64 == 0 {
                continue;
            }
            let name = isec.name(self);
            let Some(parent) =
                MergedSection::get_instance(ctx_args, merged, name, &self.base.shdrs.at_in::<E>(i))
            else {
                continue;
            };
            sections
                .regular_section_mut(i)
                .expect("a regular section")
                .uncompress::<E>(
                    diag,
                    self,
                    name,
                    self.base.shdrs.sh_offset_and_size(i).1 as usize,
                );
            sections.set_mergeable(i, parent);
        }
        self.sections = sections;
    }

    /// Attaches symbols in mergeable sections to the section fragments they
    /// refer to.
    ///
    /// Section GC and ICF work on a graph of sections and fragments, so
    /// every non-absolute symbol must refer to a non-mergeable section or
    /// a fragment. This is done only for SHF_ALLOC sections, since GC and
    /// ICF work only on them.
    pub(crate) fn reattach_section_symbols<E: Arch>(
        &self,
        diag: &Diagnostics,
        id: ObjId,
        symbols: &SymbolEditor<'_>,
        merged: &[MergedSection],
    ) {
        for i in 1..self.base.elf_syms.len() {
            let esym = self.base.elf_syms.at_in::<E>(i);
            if esym.is_abs() || esym.is_common() || esym.is_undef() {
                continue;
            }
            let sym_id = self.base.symbols[i];
            let shndx = self.shndx_from(i, esym.st_shndx);
            let Some(m) = self.mergeable_section(shndx) else {
                continue;
            };
            if !merged[m.parent.index()].resolved {
                continue;
            }
            let Some((frag, offset)) = m.fragment(esym.st_value) else {
                fatal!(diag, "{self}: bad symbol value: {}", esym.st_value);
            };
            let frag = FragmentRef {
                section: m.parent,
                entry: frag,
            };
            symbols.with_symbol(sym_id, |sym| {
                if sym.file() != Some(FileId::Obj(id)) {
                    return;
                }
                sym.set_fragment(frag);
                sym.value = offset as u64;
            });
        }
    }

    pub(crate) fn num_fragment_dummies(&self) -> usize {
        self.num_frag_syms
    }

    /// For each relocation referring to a mergeable section symbol, creates
    /// a dummy non-section symbol and redirects the relocation to it.
    pub(crate) fn reattach_fragment_relocations<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        id: ObjId,
        merged: &[MergedSection],
        base_id: SymbolId,
        slots: &mut [MaybeUninit<Symbol>],
    ) {
        debug_assert_eq!(slots.len(), self.num_frag_syms);
        self.base.symbols.reserve(slots.len());
        let mut next = 0;

        for shndx in 0..self.sections.len() {
            let (relsec_idx, contents) = {
                let Some(isec) = self.section(shndx) else {
                    continue;
                };
                if !isec.is_alloc() {
                    continue;
                }
                let Some(relsec_idx) = isec.relsec_idx() else {
                    continue;
                };
                (relsec_idx, isec.contents())
            };

            // Rewrite a relocation table in one visit, as the in-memory
            // ElfRel array is updated in place in C++. Ordinary records live
            // in the private input mapping; decoded CREL records remain in
            // the side table.
            let relsec_idx = relsec_idx as usize;
            let mut decoded = self.decoded_crel.get_mut(relsec_idx).and_then(Option::take);
            let mut rels = match decoded.as_deref_mut() {
                Some(data) => RelsMut::<E>::decoded(data),
                None => {
                    let (offset, size) = self.base.shdrs.sh_offset_and_size_in::<E>(relsec_idx);
                    let mf = self
                        .base
                        .mf
                        .expect("input relocations without a mapped file");
                    // SAFETY: this file-parallel pass exclusively owns the
                    // relocation section, whose range was checked at parse
                    // time. No overlapping shared slice is used here.
                    let data =
                        unsafe { mf.data_mut_ptr(offset as usize..(offset + size) as usize) };
                    // SAFETY: the exclusive access described above lasts
                    // until this relocation view is dropped.
                    RelsMut::<E>::new(unsafe { &mut *data })
                }
            };

            for ri in 0..rels.len() {
                let r_sym = rels.r_sym(ri) as usize;
                if (self.base.elf_syms.st_info_in::<E>(r_sym) & 0xf) as u32 != STT_SECTION {
                    continue;
                }
                let found = {
                    let esym = self.base.elf_syms.at_in::<E>(r_sym);
                    let sym_shndx = self.shndx_from(r_sym, esym.st_shndx);
                    self.mergeable_section(sym_shndx).map(|m| {
                        debug_assert!(merged[m.parent.index()].resolved);
                        let rel = rels.at(ri);
                        let addend = if E::IS_RELA && E::FAMILY != Family::Sh4 {
                            rel.r_addend
                        } else {
                            E::get_addend(&contents[rel.r_offset as usize..], &rel)
                        };
                        let Some((frag, in_frag_offset)) =
                            m.fragment(esym.st_value.wrapping_add(addend as u64))
                        else {
                            fatal!(diag, "{self}: bad relocation at {}", rel.r_sym);
                        };
                        (
                            rel,
                            FragmentRef {
                                section: m.parent,
                                entry: frag,
                            },
                            (in_frag_offset - addend) as u64,
                        )
                    })
                };
                let Some((rel, frag, value)) = found else {
                    continue;
                };

                let dummy_idx = self.base.symbols.len() as u32;
                self.base.symbols.push(SymbolId(base_id.0 + next as u32));

                let mut sym = Symbol::new(BStr::new(b"<fragment>"));
                sym.set_file(FileId::Obj(id));
                sym.set_fragment_dummy(true);
                sym.sym_idx = rel.r_sym;
                sym.set_esym(&self.base.elf_syms.at(rel.r_sym as usize));
                sym.set_visibility(STV_HIDDEN);
                sym.set_fragment(frag);
                sym.value = value;
                slots[next].write(sym);
                next += 1;

                rels.set_r_sym(ri, dummy_idx);
            }

            if let Some(data) = decoded {
                debug_assert!(self.decoded_crel[relsec_idx].is_none());
                self.decoded_crel[relsec_idx] = Some(data);
            }
        }

        // num_frag_syms may include references to mergeable sections that
        // were not converted; the extra symbols stay unused.
        for slot in &mut slots[next..] {
            self.base.symbols.push(SymbolId(base_id.0 + next as u32));
            slot.write(Symbol::new(BStr::new(b"")));
            next += 1;
        }
    }

    /// Scans relocations of live sections and of CIEs.
    pub fn scan_relocations<E: Arch>(&self, ctx: &Context<E>) {
        for isec in self.input_sections() {
            if isec.is_alive() && isec.is_alloc() {
                E::scan_relocations(ctx, isec);
            }
        }

        for cie in &self.cies {
            for rel in cie.rels::<E>(self) {
                let sym = &ctx.symbols[self.base.symbols[rel.r_sym as usize]];
                if ctx.args.pic && rel.r_type == E::R_ABS {
                    error!(
                        ctx,
                        "{self}: relocation {} in .eh_frame can not be used when making a position-independent output; recompile with -fPIE or -fPIC",
                        rel.type_name::<E>()
                    );
                }
                if sym.is_imported() {
                    if sym.ty() != STT_FUNC {
                        fatal!(
                            ctx,
                            "{self}: {sym}: .eh_frame CIE record with an external data reference is not supported"
                        );
                    }
                    sym.add_flags(NEEDS_PLT);
                }
            }
        }
    }

    /// Allocates space in `.common`/`.tls_common` for common symbols
    /// that weren't resolved to real definitions.
    ///
    /// Common symbols come from C tentative definitions: `int foo;` in a
    /// header included from several translation units yields a common
    /// symbol in each object, which the linker merges into one.
    pub fn convert_common_symbols<E: Arch>(
        &mut self,
        diag: &Diagnostics,
        args: &Args,
        id: ObjId,
        symbols: &mut SymbolTable,
        default_version: u16,
        section_arena: &crate::input_sections::SectionArena,
    ) {
        if self.num_common_symbols == 0 {
            return;
        }
        for i in self.base.first_global..self.base.elf_syms.len() {
            let esym = self.base.elf_syms.at_in::<E>(i);
            if !esym.is_common() {
                continue;
            }
            let sym_id = self.base.symbols[i];
            let sym = &symbols[sym_id];
            if sym.file() != Some(FileId::Obj(id)) {
                if args.warn_common {
                    warn!(diag, "{self}: multiple common symbols: {sym}");
                }
                continue;
            }

            let mut shdr = ElfShdr {
                sh_type: SHT_NOBITS,
                sh_size: esym.st_size,
                sh_addralign: esym.st_value,
                ..ElfShdr::default()
            };
            shdr.sh_flags = if sym.ty() == STT_TLS {
                (SHF_ALLOC | SHF_WRITE | SHF_TLS) as u64
            } else {
                (SHF_ALLOC | SHF_WRITE) as u64
            };
            let name: &'static [u8] = if sym.ty() == STT_TLS {
                b".tls_common"
            } else {
                b".common"
            };

            self.base.shdrs.push(shdr);
            let shndx = self.base.shdrs.len() - 1;
            let isec = InputSection::new::<E>(diag, self, id, shndx as u32, &shdr, BStr::new(name));
            self.sections.push(isec, section_arena);

            let sym = &mut symbols[sym_id];
            sym.set_input_section(self.section_at(shndx as u32));
            sym.value = 0;
            sym.sym_idx = i as u32;
            sym.ver_idx = default_version;
            sym.set_weak(false);
        }
    }

    /// Decides which symbols go to the output symbol table and sizes the
    /// file's block of `.symtab` and `.strtab`.
    pub fn plan_symtab<E: Arch>(&self, ctx: &Context<E>, id: ObjId) -> SymtabPlan {
        let mut plan = SymtabPlan {
            output_sym_indices: vec![-1; self.base.elf_syms.len()],
            ..SymtabPlan::default()
        };
        let file_id = FileId::Obj(id);

        // Symbols in garbage-collected sections and fragments are dropped
        // along with them.
        let is_alive = |sym: &Symbol| -> bool {
            if !ctx.args.gc_sections {
                return true;
            }
            if let Some(frag) = sym.fragment() {
                return ctx.fragment(frag).is_alive();
            }
            if let Some(isec) = sym.input_section_ref() {
                return isec.is_alive();
            }
            true
        };

        if !ctx.args.discard_all && !ctx.args.strip_all && ctx.args.retain_symbols_file.is_none() {
            for i in 1..self.base.first_global.min(self.base.elf_syms.len()) {
                if self.is_discarded_comdat(i) {
                    continue;
                }
                let sym_id = self.base.symbols[i];
                let sym = &ctx.symbols[sym_id];
                if is_alive(sym) && should_write_to_local_symtab(ctx, sym) {
                    plan.strtab_size += sym.name().len() as u64 + 1;
                    plan.output_sym_indices[i] = plan.num_local_symtab as i32;
                    plan.num_local_symtab += 1;
                    sym.set_write_to_symtab();
                }
            }
        }

        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym_id = self.base.symbols[i];
            let sym = &ctx.symbols[sym_id];
            if sym.file() == Some(file_id)
                && is_alive(sym)
                && (ctx.args.retain_symbols_file.is_none() || sym.write_to_symtab())
            {
                plan.strtab_size += sym.name().len() as u64 + 1;
                // Global symbols can be demoted to local by visibility or
                // version scripts.
                if sym.is_local(ctx) {
                    plan.output_sym_indices[i] = plan.num_local_symtab as i32;
                    plan.num_local_symtab += 1;
                } else {
                    plan.output_sym_indices[i] = plan.num_global_symtab as i32;
                    plan.num_global_symtab += 1;
                }
                sym.set_write_to_symtab();
            }
        }
        plan
    }

    /// Whether the file's `.debug_info` contains only DWARF32 units.
    pub fn is_dwarf32<E: Arch>(&mut self, diag: &Diagnostics) -> bool {
        let name = self.to_string();
        for shndx in self.debug_info_sections.clone() {
            let isec = self.section_at(shndx);
            let section_name = isec.name(self);
            if isec.sh_size < 12 {
                // Too short to be valid; garbage in, garbage out.
                return true;
            }
            let mut buf = [0u8; 12];
            let input_size = self.base.shdrs.sh_offset_and_size(shndx as usize).1 as usize;
            isec.copy_contents_to::<E>(diag, &name, section_name, input_size, &mut buf);
            // A 32-bit CU starts with a 32-bit size; a 64-bit CU starts
            // with 0xffffffff followed by a 64-bit size.
            if E::Endian::read_u32(&buf) != 0xffff_ffff {
                return true;
            }
            let first_size = E::Endian::read_u64(&buf[4..]) as usize + 12;
            if first_size as u64 == isec.sh_size {
                continue;
            }
            // A section combined by `ld -r` may contain several CUs.
            let isec = self.section_mut(shndx as usize).unwrap();
            isec.uncompress::<E>(diag, &name, section_name, input_size);
            let contents = isec.contents();
            let mut p = first_size;
            while contents.len() - p >= 12 {
                if E::Endian::read_u32(&contents[p..]) != 0xffff_ffff {
                    return true;
                }
                p += E::Endian::read_u64(&contents[p + 4..]) as usize + 12;
            }
        }
        false
    }

    /// Produces this file's block of the output symbol table.
    pub fn populate_symtab<E: Arch>(
        &self,
        ctx: &Context<E>,
        id: ObjId,
        block: &mut SymtabBlock<'_>,
    ) {
        let id = FileId::Obj(id);

        for i in 1..self.base.first_global.min(self.base.symbols.len()) {
            let sym = &ctx.symbols[self.base.symbols[i]];
            if sym.write_to_symtab() && self.base.output_sym_indices[i] >= 0 {
                block.push_local::<E>(ctx, sym);
            }
        }
        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym = &ctx.symbols[self.base.symbols[i]];
            if sym.file() == Some(id) && sym.write_to_symtab() {
                if sym.is_local(ctx) {
                    block.push_local::<E>(ctx, sym);
                } else {
                    block.push_global::<E>(ctx, sym);
                }
            }
        }
    }
}

/// The layout of a file's block of the output symbol table.
#[derive(Debug, Default)]
pub struct SymtabPlan {
    pub output_sym_indices: Vec<i32>,
    pub num_local_symtab: u32,
    pub num_global_symtab: u32,
    pub strtab_size: u64,
}

impl InputFile {
    pub fn apply_symtab_plan(&mut self, plan: SymtabPlan) {
        self.output_sym_indices = plan.output_sym_indices;
        self.num_local_symtab = plan.num_local_symtab;
        self.num_global_symtab = plan.num_global_symtab;
        self.strtab_size = plan.strtab_size;
    }
}

/// A file's or chunk's part of `.symtab`, `.strtab` and `.symtab_shndx`:
/// its local symbols, its global symbols and their names, written in
/// place. The parts of different files don't overlap, so they are
/// written in parallel.
pub struct SymtabBlock<'a> {
    locals: SymtabEntries<'a>,
    globals: SymtabEntries<'a>,
    strtab: &'a mut [u8],
    /// The offset of `strtab` within `.strtab`.
    strtab_base: u64,
    strtab_len: usize,
}

/// A run of `.symtab` entries and, if the output has the section, the
/// matching `.symtab_shndx` entries.
pub struct SymtabEntries<'a> {
    syms: &'a mut [u8],
    xindex: Option<&'a mut [u8]>,
    len: usize,
}

impl<'a> SymtabEntries<'a> {
    pub fn new(syms: &'a mut [u8], xindex: Option<&'a mut [u8]>) -> SymtabEntries<'a> {
        SymtabEntries {
            syms,
            xindex,
            len: 0,
        }
    }

    fn push<E: Arch>(&mut self, esym: ElfSym, xindex: u32) {
        let size = ElfSym::size::<E>();
        esym.write::<E>(&mut self.syms[self.len * size..(self.len + 1) * size]);
        if let Some(entries) = &mut self.xindex {
            E::Endian::write_u32(&mut entries[self.len * 4..], xindex);
        }
        self.len += 1;
    }
}

impl<'a> SymtabBlock<'a> {
    pub fn new(
        locals: SymtabEntries<'a>,
        globals: SymtabEntries<'a>,
        strtab: &'a mut [u8],
        strtab_base: u64,
    ) -> SymtabBlock<'a> {
        SymtabBlock {
            locals,
            globals,
            strtab,
            strtab_base,
            strtab_len: 0,
        }
    }

    /// Adds a name made of `parts`, returning its `.strtab` offset.
    fn add_string(&mut self, parts: &[&[u8]]) -> u32 {
        let offset = self.strtab_base + self.strtab_len as u64;
        for part in parts {
            self.strtab[self.strtab_len..self.strtab_len + part.len()].copy_from_slice(part);
            self.strtab_len += part.len();
        }
        self.strtab[self.strtab_len] = 0;
        self.strtab_len += 1;
        offset as u32
    }

    pub fn push_local<E: Arch>(&mut self, ctx: &Context<E>, sym: &Symbol) {
        let st_name = self.add_string(&[sym.name().as_ref()]);
        let (esym, xindex) = crate::chunks::symtab::to_output_esym(ctx, sym, st_name);
        self.locals.push::<E>(esym, xindex);
    }

    pub fn push_global<E: Arch>(&mut self, ctx: &Context<E>, sym: &Symbol) {
        let st_name = self.add_string(&[sym.name().as_ref()]);
        let (esym, xindex) = crate::chunks::symtab::to_output_esym(ctx, sym, st_name);
        self.globals.push::<E>(esym, xindex);
    }

    /// Adds a synthesized local symbol with a name built from `name` and
    /// `suffix`.
    pub fn push_synthetic<E: Arch>(&mut self, name: &[u8], suffix: &[u8], esym: ElfSym) {
        let st_name = self.add_string(&[name, suffix]);
        self.locals.push::<E>(ElfSym { st_name, ..esym }, 0);
    }

    /// Adds a local symbol whose name is a fixed `.strtab` entry, such as
    /// an ARM32 mapping symbol.
    pub fn push_mapping_symbol<E: Arch>(&mut self, st_name: u32, esym: ElfSym) {
        self.locals.push::<E>(ElfSym { st_name, ..esym }, 0);
    }
}

fn should_write_to_local_symtab<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> bool {
    if sym.ty() == STT_SECTION {
        return false;
    }

    // Temporary local symbols such as .L.str.42 are compiler-internal and
    // numerous, and not referenced by DWARF, so they are discarded by
    // default, as lld does. Even with --discard-none, temporary symbols
    // in mergeable sections are discarded, as traditional linkers do.
    let name: &[u8] = sym.name();
    if name.starts_with(b".L") || name == b"L0\x01" {
        if ctx.args.discard_locals {
            return false;
        }
        if let Some(isec) = sym.input_section_ref() {
            if isec.sh_flags & SHF_MERGE as u64 != 0 {
                return false;
            }
        }
    }
    true
}

/// Determines how FDE pointers are encoded by parsing a CIE's
/// augmentation string, so that FDEs covering an empty range can be
/// dropped from `.eh_frame_hdr`.
fn parse_fde_encoding<E: Arch>(
    diag: &Diagnostics,
    file: &ObjectFile,
    isec: &InputSection,
    data: &[u8],
) -> u8 {
    let ptr_size = |enc: u8| -> u8 {
        match enc as u32 & 0xf {
            DW_EH_PE_absptr => E::WORD_SIZE as u8,
            DW_EH_PE_udata4 | DW_EH_PE_sdata4 => 4,
            DW_EH_PE_udata8 | DW_EH_PE_sdata8 => 8,
            _ => fatal!(
                diag,
                "{}: unsupported FDE pointer encoding: {enc}",
                isec.display(file)
            ),
        }
    };

    // Skip the length, CIE ID and version fields.
    let version = data[8];
    if version != 1 && version != 3 {
        fatal!(
            diag,
            "{}: unsupported CIE version: {version}",
            isec.display(file)
        );
    }
    let mut rest = &data[9..];
    let aug_len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    let aug = &rest[..aug_len];
    rest = &rest[(aug_len + 1).min(rest.len())..];

    let enc = 'enc: {
        // An empty augmentation string means raw absolute pointers. A
        // string not starting with 'z' is a legacy augmentation whose
        // layout we don't know.
        if aug.is_empty() {
            break 'enc DW_EH_PE_absptr as u8;
        }
        if aug[0] != b'z' {
            fatal!(
                diag,
                "{}: unsupported CIE augmentation string: {}",
                isec.display(file),
                util::display(aug)
            );
        }

        read_uleb(&mut rest); // code alignment factor
        read_uleb(&mut rest); // data alignment factor
        if version == 1 {
            rest = &rest[1..]; // return address register
        } else {
            read_uleb(&mut rest);
        }
        read_uleb(&mut rest); // augmentation data length

        for &c in &aug[1..] {
            match c {
                b'R' => break 'enc rest[0],
                b'L' => rest = &rest[1..],
                b'P' => rest = &rest[ptr_size(rest[0]) as usize + 1..],
                b'S' | b'B' | b'G' => {}
                _ => fatal!(
                    diag,
                    "{}: unsupported CIE augmentation string: {}",
                    isec.display(file),
                    util::display(aug)
                ),
            }
        }
        DW_EH_PE_absptr as u8
    };

    if enc & 0xf0 != 0 && enc as u32 & 0xf0 != DW_EH_PE_pcrel {
        fatal!(
            diag,
            "{}: unsupported FDE pointer encoding: {enc}",
            isec.display(file)
        );
    }
    ptr_size(enc)
}

/// The byte length of an SFrame FRE block: a 5-byte attribute header
/// followed by frame row entries of variable width.
fn sframe_fre_block_size<E: Arch>(data: &[u8], offset: usize) -> usize {
    let num_fres = E::Endian::read_u16(&data[offset..]) as usize;
    let addr_size = 1usize << bits(data[offset + 2] as u64, 3, 0);
    let mut p = offset + 5;
    for _ in 0..num_fres {
        let info = data[p + addr_size] as u64;
        let num_words = bits(info, 4, 1) as usize;
        let word_size = 1usize << bits(info, 6, 5);
        p += addr_size + 1 + num_words * word_size;
    }
    p - offset
}

/// A shared library given as an input.
#[derive(Debug)]
pub struct SharedFile {
    pub base: InputFile,
    pub soname: String,
    pub version_strings: Vec<&'static [u8]>,

    /// For each symbol, the `foo@VERSION` alias of a default-versioned
    /// definition `foo@@VERSION`.
    pub symbols2: Vec<SymbolId>,
    pub versyms: Vec<u16>,

    /// Symbol table keys for `base.symbols` and `symbols2`, interned by
    /// `gather_symbols`.
    pub symbol_keys: Vec<(&'static [u8], &'static [u8])>,
    pub symbol2_keys: Vec<Option<(&'static [u8], &'static [u8])>>,

    sorted_syms: OnceLock<Vec<SymbolId>>,
}

impl fmt::Display for SharedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", path_clean(&self.base.filename))
    }
}

impl SharedFile {
    pub fn new<E: Arch>(diag: &Diagnostics, mf: &'static MappedFile) -> SharedFile {
        let base = InputFile::parse::<E>(diag, mf, &FileName(&mf.name, ""));
        let mut file = SharedFile {
            base,
            soname: String::new(),
            version_strings: Vec::new(),
            symbols2: Vec::new(),
            versyms: Vec::new(),
            symbol_keys: Vec::new(),
            symbol2_keys: Vec::new(),
            sorted_syms: OnceLock::new(),
        };
        file.parse::<E>(diag);
        file
    }

    /// The strings of the dynamic entries with the given tag, such as the
    /// DT_NEEDED libraries.
    fn dynamic_strings<E: Arch>(&self, diag: &Diagnostics, tag: u64) -> Vec<&'static [u8]> {
        let Some(idx) = self.base.find_section(SHT_DYNAMIC) else {
            return Vec::new();
        };
        let shdr = self.base.shdrs.at(idx);
        let strtab = self.base.section_contents(diag, shdr.sh_link as usize);
        ElfDyn::parse_all::<E>(self.base.section_contents(diag, idx))
            .into_iter()
            .filter(|entry| entry.d_tag == tag)
            .map(|entry| cstr_at(strtab, entry.d_val as usize))
            .collect()
    }

    fn get_soname<E: Arch>(&self, diag: &Diagnostics) -> String {
        if let Some(soname) = self.dynamic_strings::<E>(diag, DT_SONAME as u64).first() {
            return String::from_utf8_lossy(soname).into_owned();
        }
        if self.base.mf.is_none_or(|mf| mf.given_fullpath) {
            return self.base.filename.clone();
        }
        path_filename(&self.base.filename)
    }

    fn parse<E: Arch>(&mut self, diag: &Diagnostics) {
        let Some(symtab_idx) = self.base.find_section(SHT_DYNSYM) else {
            return;
        };
        let symtab_shdr = self.base.shdrs.at(symtab_idx);
        self.base.symbol_strtab = self
            .base
            .section_contents(diag, symtab_shdr.sh_link as usize);
        self.soname = self.get_soname::<E>(diag);
        self.version_strings = self.read_version_strings::<E>(diag);

        let esyms = ElfSym::parse_all::<E>(self.base.section_contents(diag, symtab_idx));
        let first = symtab_shdr.sh_info as usize;
        if esyms.len() < first {
            fatal!(diag, "{self}: invalid symbol table");
        }
        // Only the symbols this file exports are kept, so the table is
        // rebuilt rather than read in place.
        self.base.elf_syms = SymTable::new(RecordLayout::of::<E>());

        let vers: Vec<u16> = match self.base.find_section(SHT_GNU_VERSYM) {
            Some(idx) => self
                .base
                .section_contents(diag, idx)
                .chunks_exact(2)
                .map(E::Endian::read_u16)
                .collect(),
            None => Vec::new(),
        };

        for i in first..esyms.len() {
            let esym = esyms[i];
            let mut ver = if vers.is_empty() {
                VER_NDX_GLOBAL as u16
            } else {
                vers[i] & !(VERSYM_HIDDEN as u16)
            };

            // Version index 0 is valid only for unversioned undefined
            // symbols; a defined symbol with index 0 is ill-formed.
            if ver as u32 == VER_NDX_LOCAL {
                if !esym.is_undef() {
                    fatal!(
                        diag,
                        "{self}: invalid version index 0 for defined symbol {}",
                        util::display(cstr_at(self.base.symbol_strtab, esym.st_name as usize))
                    );
                }
                ver = VER_NDX_GLOBAL as u16;
            }

            // A definition whose versym is exactly VERSYM_HIDDEN |
            // VER_NDX_GLOBAL is a compatibility alias for binaries linked
            // before the library adopted versioning. New references must
            // not bind to it.
            if !vers.is_empty()
                && vers[i] == (VERSYM_HIDDEN | VER_NDX_GLOBAL) as u16
                && !esym.is_undef()
            {
                continue;
            }

            self.base.elf_syms.push(esym);
            self.versyms.push(if esym.is_undef() {
                VER_NDX_GLOBAL as u16
            } else {
                ver
            });

            let name = cstr_at(self.base.symbol_strtab, esym.st_name as usize);
            let has_version = ver as u32 != VER_NDX_GLOBAL
                && (ver as usize) < self.version_strings.len()
                && !self.version_strings[ver as usize].is_empty();

            let versioned_key = || -> (&'static [u8], &'static [u8]) {
                let key = leak_bytes([name, b"@", self.version_strings[ver as usize]].concat());
                (key, name)
            };

            // A default-versioned symbol `foo@@VER` can be referred to as
            // either `foo` or `foo@VER`. We resolve `foo` normally and make
            // `foo@VER` a forwarding alias.
            if !has_version {
                self.symbol_keys.push((name, name));
                self.symbol2_keys.push(None);
            } else if esym.is_undef() || vers[i] & VERSYM_HIDDEN as u16 != 0 {
                self.symbol_keys.push(versioned_key());
                self.symbol2_keys.push(None);
            } else {
                self.symbol_keys.push((name, name));
                self.symbol2_keys.push(Some(versioned_key()));
            }
        }

        self.base.first_global = 0;
        self.base.symbols = vec![SymbolId::DISCARDED_COMDAT; self.base.elf_syms.len()];
        self.symbols2 = vec![SymbolId::NONE; self.base.elf_syms.len()];
    }

    /// Records symbols and default-version aliases in this worker's bin.
    /// Both slot arrays are complete and stable until gather.
    pub(crate) fn record_global_symbols(&mut self, bins: &mut Bins<SymbolSlot>) {
        for (i, (key, name)) in std::mem::take(&mut self.symbol_keys)
            .into_iter()
            .enumerate()
        {
            bins.record(key, name.len(), SymbolSlot::new(&mut self.base.symbols[i]));
        }
        for (i, alias) in std::mem::take(&mut self.symbol2_keys)
            .into_iter()
            .enumerate()
        {
            if let Some((key, name)) = alias {
                bins.record(key, name.len(), SymbolSlot::new(&mut self.symbols2[i]));
            }
        }
    }

    pub fn dt_needed<E: Arch>(&self, diag: &Diagnostics) -> Vec<&'static [u8]> {
        self.dynamic_strings::<E>(diag, DT_NEEDED as u64)
    }

    pub fn dt_audit<E: Arch>(&self, diag: &Diagnostics) -> &'static [u8] {
        self.dynamic_strings::<E>(diag, DT_AUDIT as u64)
            .first()
            .copied()
            .unwrap_or(b"")
    }

    /// Reads `.gnu.version_d` and `.gnu.version_r` into a table of version
    /// names indexed by version index. Both sections share the index space.
    fn read_version_strings<E: Arch>(&self, diag: &Diagnostics) -> Vec<&'static [u8]> {
        let mut vec: Vec<&'static [u8]> = Vec::new();
        let mut set = |idx: usize, name: &'static [u8]| {
            if vec.len() <= idx {
                vec.resize(idx + 1, b"");
            }
            vec[idx] = name;
        };

        if let Some(idx) = self.base.find_section(SHT_GNU_VERDEF) {
            let verdef = self.base.section_contents(diag, idx);
            let strtab = self
                .base
                .section_contents(diag, self.base.shdrs.at(idx).sh_link as usize);
            let mut pos = 0;
            loop {
                let ver = ElfVerdef::parse::<E>(&verdef[pos..]);
                if ver.vd_ndx as u32 == VER_NDX_UNSPECIFIED {
                    fatal!(diag, "{self}: symbol version too large");
                }
                let aux = ElfVerdaux::parse::<E>(&verdef[pos + ver.vd_aux as usize..]);
                set(ver.vd_ndx as usize, cstr_at(strtab, aux.vda_name as usize));
                if ver.vd_next == 0 {
                    break;
                }
                pos += ver.vd_next as usize;
            }
        }

        if let Some(idx) = self.base.find_section(SHT_GNU_VERNEED) {
            let verneed = self.base.section_contents(diag, idx);
            let strtab = self
                .base
                .section_contents(diag, self.base.shdrs.at(idx).sh_link as usize);
            let mut pos = 0;
            loop {
                let vn = ElfVerneed::parse::<E>(&verneed[pos..]);
                let mut aux_pos = pos + vn.vn_aux as usize;
                for _ in 0..vn.vn_cnt {
                    let aux = ElfVernaux::parse::<E>(&verneed[aux_pos..]);
                    let idx = (aux.vna_other & !(VERSYM_HIDDEN as u16)) as usize;
                    set(idx, cstr_at(strtab, aux.vna_name as usize));
                    if aux.vna_next == 0 {
                        break;
                    }
                    aux_pos += aux.vna_next as usize;
                }
                if vn.vn_next == 0 {
                    break;
                }
                pos += vn.vn_next as usize;
            }
        }
        vec
    }

    /// The symbols this file defines at the same address as `sym`.
    pub fn symbols_at<E: Arch>(&self, ctx: &Context<E>, sym: &Symbol, id: DsoId) -> &[SymbolId] {
        let sorted = self.sorted_syms.get_or_init(|| {
            let mut syms: Vec<SymbolId> = self
                .base
                .symbols
                .iter()
                .copied()
                .filter(|&s| ctx.symbols[s].file() == Some(FileId::Dso(id)))
                .collect();
            syms.sort_by_key(|&s| (ctx.symbols[s].esym(ctx).st_value, s));
            syms
        });
        let value = sym.esym(ctx).st_value;
        let begin = sorted.partition_point(|&s| ctx.symbols[s].esym(ctx).st_value < value);
        let end = sorted.partition_point(|&s| ctx.symbols[s].esym(ctx).st_value <= value);
        &sorted[begin..end]
    }

    /// Infers the alignment of a symbol from its address and its section's
    /// alignment, for copy relocations.
    pub fn alignment(&self, sym: &Symbol) -> u64 {
        let shdr = &self
            .base
            .shdrs
            .at(self.base.elf_syms.at(sym.sym_idx as usize).st_shndx as usize);
        let mut align = shdr.sh_addralign.max(1);
        if sym.value != 0 {
            align = align.min(1 << sym.value.trailing_zeros());
        }
        align
    }

    /// Whether a symbol lives in a read-only segment.
    pub fn is_readonly<E: Arch>(&self, sym: &Symbol) -> bool {
        let data = self.base.data();
        let ehdr = ElfEhdr::parse::<E>(data);
        let val = self.base.elf_syms.at(sym.sym_idx as usize).st_value;
        let phoff = ehdr.e_phoff as usize;
        let size = ElfPhdr::size::<E>();
        (0..ehdr.e_phnum as usize)
            .map(|i| ElfPhdr::parse::<E>(&data[phoff + i * size..]))
            .any(|phdr| {
                (phdr.p_type == PT_LOAD || phdr.p_type == PT_GNU_RELRO)
                    && phdr.p_flags & PF_W == 0
                    && phdr.p_vaddr <= val
                    && val < phdr.p_vaddr + phdr.p_memsz
            })
    }

    pub fn plan_symtab<E: Arch>(&self, ctx: &Context<E>, id: DsoId) -> SymtabPlan {
        let mut plan = SymtabPlan {
            output_sym_indices: vec![-1; self.base.elf_syms.len()],
            ..SymtabPlan::default()
        };
        for i in 0..self.base.symbols.len() {
            let sym_id = self.base.symbols[i];
            let sym = &ctx.symbols[sym_id];
            if sym.file() == Some(FileId::Dso(id))
                && (sym.is_imported() || sym.is_exported())
                && (ctx.args.retain_symbols_file.is_none() || sym.write_to_symtab())
            {
                plan.strtab_size += sym.name().len() as u64 + 1;
                plan.output_sym_indices[i] = plan.num_global_symtab as i32;
                plan.num_global_symtab += 1;
                sym.set_write_to_symtab();
            }
        }
        plan
    }

    pub fn populate_symtab<E: Arch>(
        &self,
        ctx: &Context<E>,
        id: DsoId,
        block: &mut SymtabBlock<'_>,
    ) {
        for &sym_id in &self.base.symbols {
            let sym = &ctx.symbols[sym_id];
            if sym.file() == Some(FileId::Dso(id)) && sym.write_to_symtab() {
                block.push_global::<E>(ctx, sym);
            }
        }
    }
}

/// Symbol definition strength, from strongest to weakest:
///
/// 1. Strong defined symbol
/// 2. Weak defined symbol
/// 3. Strong defined symbol in a DSO or archive
/// 4. Weak defined symbol in a DSO or archive
/// 5. Common symbol
/// 6. Common symbol in an archive
/// 7. Undefined
///
/// These are heuristics rather than exact science; they avoid link errors
/// in all programs we've tested. Ties are broken by file priority.
#[inline]
fn symbol_rank_from_fields(
    is_common: bool,
    st_bind: u32,
    is_dso: bool,
    is_in_archive: bool,
) -> u64 {
    if is_common {
        debug_assert!(!is_dso);
        return if is_in_archive { 6 } else { 5 };
    }
    if is_dso || is_in_archive {
        return if st_bind == STB_WEAK { 4 } else { 3 };
    }
    if st_bind == STB_WEAK {
        2
    } else {
        1
    }
}

#[inline]
pub fn symbol_rank(esym: &ElfSym, is_dso: bool, is_in_archive: bool) -> u64 {
    symbol_rank_from_fields(esym.is_common(), esym.st_bind(), is_dso, is_in_archive)
}

/// The rank used to choose a symbol definition. Ties in definition
/// strength are broken in favor of the file occurring first on the command
/// line.
#[inline]
pub fn symbol_resolution_rank(
    esym: &ElfSym,
    is_dso: bool,
    is_in_archive: bool,
    priority: u32,
) -> u64 {
    (symbol_rank(esym, is_dso, is_in_archive) << 32) | priority as u64
}

/// Returns the resolution rank of a symbol that already owns a definition.
/// `Symbol` caches the defining ELF symbol's fields, so we don't need to
/// decode that record again for every competing definition.
#[inline]
pub fn resolved_symbol_rank(sym: &Symbol, is_dso: bool, is_in_archive: bool, priority: u32) -> u64 {
    (symbol_rank_from_fields(sym.is_common(), sym.st_bind(), is_dso, is_in_archive) << 32)
        | priority as u64
}

/// An exclusive view of the symbol table shared by file-parallel passes.
/// Individual symbols are serialized by their byte-sized spin locks.
pub(crate) struct SymbolEditor<'a> {
    symbols: *mut Symbol,
    len: usize,
    _symbols: PhantomData<&'a mut [Symbol]>,
}

// The exclusive slice represented by `symbols` cannot be accessed except
// through `with_symbol`, which serializes accesses to each element.
unsafe impl Sync for SymbolEditor<'_> {}

impl<'a> SymbolEditor<'a> {
    pub(crate) fn new(symbols: &'a mut [Symbol]) -> SymbolEditor<'a> {
        SymbolEditor {
            symbols: symbols.as_mut_ptr(),
            len: symbols.len(),
            _symbols: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn with_symbol<R>(&self, id: SymbolId, f: impl FnOnce(&mut Symbol) -> R) -> R {
        debug_assert!(id.index() < self.len);
        // SAFETY: the editor owns an exclusive borrow of the whole table,
        // and Symbol::with_resolution_lock serializes accesses to this slot.
        unsafe { Symbol::with_resolution_lock(self.symbols.add(id.index()), f) }
    }

    #[inline]
    pub(crate) fn skip_dso(&self, id: SymbolId) -> bool {
        debug_assert!(id.index() < self.len);
        // SAFETY: the editor owns a live symbol array; skip_dso_at reads only
        // the atomic state byte shared with the resolution lock.
        unsafe { Symbol::skip_dso_at(self.symbols.add(id.index())) }
    }
}

/// The files and defaults needed while editing symbols during resolution.
pub struct SymbolResolver<'a> {
    editor: SymbolEditor<'a>,
    objs: &'a [Box<ObjectFile>],
    dsos: &'a [Box<SharedFile>],
    default_version: u16,
}

// SymbolResolver's mutable symbol-table access is serialized by its editor.
unsafe impl Sync for SymbolResolver<'_> {}

impl<'a> SymbolResolver<'a> {
    pub fn new(
        symbols: &'a mut [Symbol],
        objs: &'a [Box<ObjectFile>],
        dsos: &'a [Box<SharedFile>],
        default_version: u16,
    ) -> SymbolResolver<'a> {
        SymbolResolver {
            editor: SymbolEditor::new(symbols),
            objs,
            dsos,
            default_version,
        }
    }

    #[inline]
    fn with_symbol<R>(&self, id: SymbolId, f: impl FnOnce(&mut Symbol) -> R) -> R {
        self.editor.with_symbol(id, f)
    }

    #[inline]
    fn skip_dso(&self, id: SymbolId) -> bool {
        self.editor.skip_dso(id)
    }

    #[inline]
    fn current_rank(&self, sym: &Symbol) -> u64 {
        let Some(file) = sym.file() else {
            return 7 << 32;
        };
        let base = match file {
            FileId::Obj(id) => &self.objs[id.index()].base,
            FileId::Dso(id) => &self.dsos[id.index()].base,
        };
        resolved_symbol_rank(sym, file.is_dso(), !base.is_reachable(), base.priority)
    }
}

impl ObjectFile {
    /// Resolves this file's global definitions in place. Before sections
    /// are parsed, all definitions are treated as live; the final round uses
    /// the actual section state.
    pub fn resolve_symbols<E: Arch>(&self, resolver: &SymbolResolver<'_>, id: ObjId) {
        let in_archive = !self.base.is_reachable();
        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym_id = self.base.symbols[i];
            self.resolve_symbol::<E>(resolver, id, i, sym_id, in_archive);
        }
    }

    /// Resolves only symbols marked to ignore DSO definitions. This is the
    /// rare hidden-symbol retry; filtering before `resolve_symbol` avoids
    /// decoding and locking every other definition, as in C++.
    pub fn resolve_skip_dso_symbols<E: Arch>(&self, resolver: &SymbolResolver<'_>, id: ObjId) {
        let in_archive = !self.base.is_reachable();
        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym_id = self.base.symbols[i];
            if resolver.skip_dso(sym_id) {
                self.resolve_symbol::<E>(resolver, id, i, sym_id, in_archive);
            }
        }
    }

    // Makes this file's i'th symbol the definition of the global symbol it
    // refers to if it is the best one seen so far.
    fn resolve_symbol<E: Arch>(
        &self,
        resolver: &SymbolResolver<'_>,
        id: ObjId,
        i: usize,
        sym_id: SymbolId,
        in_archive: bool,
    ) {
        let esym = self.base.elf_syms.at_in::<E>(i);
        if esym.is_undef() {
            return;
        }

        let mut origin = None;
        if !esym.is_abs() && !esym.is_common() && self.sections_parsed {
            let shndx = self.shndx_from(i, esym.st_shndx);
            let Some(isec) = self.section(shndx) else {
                return;
            };
            if !isec.is_alive() {
                return;
            }
            origin = Some(isec);
        }

        let rank = symbol_resolution_rank(&esym, false, in_archive, self.base.priority);
        resolver.with_symbol(sym_id, |sym| {
            if rank < resolver.current_rank(sym) {
                sym.set_file(FileId::Obj(id));
                match origin {
                    Some(section) => sym.set_input_section(section),
                    None => sym.clear_origin(),
                }
                sym.value = esym.st_value;
                sym.sym_idx = i as u32;
                sym.set_esym(&esym);
                sym.ver_idx = resolver.default_version;
                sym.set_weak(esym.is_weak());
                sym.set_versioned_default(false);
                sym.set_rust(self.is_rust_obj);
            }
        });
    }
}

impl SharedFile {
    /// Resolves this shared library's definitions in place, including the
    /// forwarding aliases for default symbol versions.
    pub fn resolve_symbols<E: Arch>(&self, resolver: &SymbolResolver<'_>, id: DsoId) {
        for i in 0..self.base.elf_syms.len() {
            let esym = self.base.elf_syms.at_in::<E>(i);
            let sym_id = self.base.symbols[i];
            if esym.is_undef() || resolver.skip_dso(sym_id) {
                continue;
            }

            let rank = symbol_resolution_rank(&esym, true, false, self.base.priority);
            resolver.with_symbol(sym_id, |sym| {
                if rank < resolver.current_rank(sym) {
                    sym.set_file(FileId::Dso(id));
                    sym.clear_origin();
                    sym.value = esym.st_value;
                    sym.sym_idx = i as u32;
                    sym.set_esym(&esym);
                    sym.ver_idx = self.versyms[i];
                    sym.set_weak(true);
                    sym.set_versioned_default(false);
                    sym.set_rust(false);
                }
            });

            // A symbol with the default version is a special case because,
            // unlike other symbols, the symbol can be referred by two names,
            // `foo` and `foo@VERSION`. Resolve the latter as a proxy of the
            // former.
            let alias_id = self.symbols2[i];
            if alias_id != SymbolId::NONE && alias_id != sym_id {
                resolver.with_symbol(alias_id, |sym| {
                    if rank < resolver.current_rank(sym) {
                        sym.set_file(FileId::Dso(id));
                        sym.set_symbol_origin(sym_id);
                        sym.sym_idx = i as u32;
                        sym.set_esym(&esym);
                        sym.set_rust(false);
                        sym.set_versioned_default(true);
                    }
                });
            }
        }
    }
}

/// Prints a `--trace-symbol` line for a reference or definition.
pub fn print_trace_symbol(
    diag: &Diagnostics,
    file: &dyn fmt::Display,
    esym: &ElfSym,
    sym: &Symbol,
) {
    if !esym.is_undef() {
        out!(diag, "trace-symbol: {file}: definition of {sym}");
    } else if esym.is_weak() {
        out!(diag, "trace-symbol: {file}: weak reference to {sym}");
    } else {
        out!(diag, "trace-symbol: {file}: reference to {sym}");
    }
}
