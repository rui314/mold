//! Output chunks: the contiguous regions that make up the output file.
//!
//! Besides the output sections built from input sections, the linker
//! synthesizes many sections of its own (`.got`, `.plt`, `.dynamic`,
//! `.symtab` and so on). Every such section has a [`ChunkHeader`] holding
//! its section header and bookkeeping, and is addressed by a [`ChunkId`].
//! Operations common to all chunks dispatch on the id.

pub mod arm_exidx;
pub mod dynamic;
pub mod eh_frame;
pub mod got;
pub mod merged;
pub mod misc;
pub mod opd;
pub mod output_section;
pub mod sframe;
pub mod symtab;
pub mod version;

use std::marker::PhantomData;
use std::num::NonZeroU32;

use bstr::BStr;

use crate::arch::Arch;
use crate::args::SectionOrderKind;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{FileId, SymtabBlock};
use crate::symbol::AddrFlags;
use crate::tls;
use crate::{error, warn};

pub use merged::MergedSectionId;

/// Index of an output section in `Context::output_sections`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutputSectionId(NonZeroU32);

impl OutputSectionId {
    #[inline]
    pub fn new(index: u32) -> OutputSectionId {
        let encoded = index.checked_add(1).expect("too many output sections");
        OutputSectionId(NonZeroU32::new(encoded).unwrap())
    }

    #[inline]
    pub fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

/// Identifies a chunk of the output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChunkId {
    Ehdr,
    Phdr,
    Shdr,
    Interp,
    Got,
    GotPlt,
    RelPlt,
    RelDyn,
    RelrDyn,
    Dynamic,
    Strtab,
    Dynstr,
    Hash,
    GnuHash,
    GnuDebuglink,
    Shstrtab,
    Plt,
    PltGot,
    Symtab,
    SymtabShndx,
    Dynsym,
    EhFrame,
    EhFrameHdr,
    EhFrameReloc,
    SFrame,
    SFrameReloc,
    Copyrel,
    CopyrelRelro,
    Versym,
    Verneed,
    Verdef,
    BuildId,
    NotePackage,
    NoteProperty,
    RiscvAttributes,
    ArmExidx,
    Ppc64SaveRestore,
    Ppc64Opd,
    GdbIndex,
    RelroPadding,
    Output(OutputSectionId),
    Merged(MergedSectionId),
    Reloc(u32),
    ComdatGroup(u32),
    Compressed(u32),
    /// A section of the main output that a separate debug file lists
    /// without contents.
    Placeholder(u32),
}

impl ChunkId {
    /// Whether the chunk is one of the ELF headers rather than a section.
    pub fn is_header(self) -> bool {
        matches!(self, ChunkId::Ehdr | ChunkId::Phdr | ChunkId::Shdr)
    }

    pub fn as_output_section(self) -> Option<OutputSectionId> {
        match self {
            ChunkId::Output(id) => Some(id),
            _ => None,
        }
    }
}

/// State shared by all chunks.
#[derive(Debug)]
pub struct ChunkHeader {
    pub name: &'static BStr,
    pub shdr: ElfShdr,

    /// Index in the output section header table; 0 for headers.
    pub shndx: u32,

    pub num_dynrels: u64,
    pub num_relrs: u64,
    pub relr: Vec<u64>,
    pub is_relro: bool,

    /// For --gdb-index
    pub is_compressed: bool,

    // Synthesized local symbols, e.g. `foo$got` or thunk labels.
    pub local_symtab_idx: u32,
    pub num_local_symtab: u32,
    pub strtab_size: u64,
    pub strtab_offset: u64,

    /// For --section-order
    pub sect_order: i64,
}

impl ChunkHeader {
    pub fn new(name: &'static str, sh_type: u32, sh_flags: u64) -> ChunkHeader {
        ChunkHeader {
            name: BStr::new(name.as_bytes()),
            shdr: ElfShdr {
                sh_type,
                sh_flags,
                sh_addralign: 1,
                ..ElfShdr::default()
            },
            shndx: 0,
            num_dynrels: 0,
            num_relrs: 0,
            relr: Vec::new(),
            is_relro: false,
            is_compressed: false,
            local_symtab_idx: 0,
            num_local_symtab: 0,
            strtab_size: 0,
            strtab_offset: 0,
            sect_order: 0,
        }
    }

    pub fn with_name(name: &'static BStr, sh_type: u32, sh_flags: u64) -> ChunkHeader {
        ChunkHeader {
            name,
            ..ChunkHeader::new("", sh_type, sh_flags)
        }
    }

    pub fn is_alloc(&self) -> bool {
        self.shdr.sh_flags & SHF_ALLOC as u64 != 0
    }

    pub fn is_nobits(&self) -> bool {
        self.shdr.sh_type == SHT_NOBITS
    }
}

/// The ELF file header.
#[derive(Debug)]
pub struct OutputEhdr {
    pub hdr: ChunkHeader,
}

impl OutputEhdr {
    pub fn new<E: Arch>(sh_flags: u64) -> OutputEhdr {
        let mut hdr = ChunkHeader::new("EHDR", 0, sh_flags);
        hdr.shdr.sh_size = ElfEhdr::size::<E>() as u64;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        OutputEhdr { hdr }
    }
}

/// The section header table, usually at the end of the file. It is not
/// needed at runtime; only the program header is.
#[derive(Debug)]
pub struct OutputShdr {
    pub hdr: ChunkHeader,
}

impl OutputShdr {
    pub fn new<E: Arch>() -> OutputShdr {
        let mut hdr = ChunkHeader::new("SHDR", 0, 0);
        hdr.shdr.sh_size = 1;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        OutputShdr { hdr }
    }
}

/// The program header, describing the segments the kernel maps.
#[derive(Debug)]
pub struct OutputPhdr {
    pub hdr: ChunkHeader,
    pub phdrs: Vec<ElfPhdr>,
}

impl OutputPhdr {
    pub fn new<E: Arch>(sh_flags: u64) -> OutputPhdr {
        let mut hdr = ChunkHeader::new("PHDR", 0, sh_flags);
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        OutputPhdr {
            hdr,
            phdrs: Vec::new(),
        }
    }
}

/// `.gdb_index`, built after everything else has been written.
#[derive(Debug)]
pub struct GdbIndexSection {
    pub hdr: ChunkHeader,
}

impl GdbIndexSection {
    pub fn new() -> GdbIndexSection {
        let mut hdr = ChunkHeader::new(".gdb_index", SHT_PROGBITS, 0);
        hdr.shdr.sh_addralign = 4;
        GdbIndexSection { hdr }
    }
}

impl Default for GdbIndexSection {
    fn default() -> Self {
        Self::new()
    }
}

fn entry_addr<E: Arch>(ctx: &Context<E>) -> u64 {
    if ctx.args.relocatable {
        return 0;
    }
    let sym = &ctx.symbols[ctx.syms.entry];
    if let Some(FileId::Obj(_)) = sym.file() {
        return sym.addr(ctx);
    }
    if !ctx.args.shared {
        warn!(ctx, "entry symbol is not defined: {sym}");
    }
    0
}

fn write_ehdr<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut ehdr = ElfEhdr::default();
    ehdr.e_ident[..4].copy_from_slice(b"\x7fELF");
    ehdr.e_ident[EI_CLASS as usize] = if E::IS_64 { ELFCLASS64 } else { ELFCLASS32 } as u8;
    ehdr.e_ident[EI_DATA as usize] = if E::IS_LITTLE_ENDIAN {
        ELFDATA2LSB
    } else {
        ELFDATA2MSB
    } as u8;
    ehdr.e_ident[EI_VERSION as usize] = EV_CURRENT as u8;
    ehdr.e_machine = E::E_MACHINE as u16;
    ehdr.e_version = EV_CURRENT;
    ehdr.e_entry = entry_addr(ctx);
    ehdr.e_flags = E::eflags(ctx);
    ehdr.e_ehsize = ElfEhdr::size::<E>() as u16;

    // If e_shstrndx is too large, the real value goes to the zeroth
    // section's sh_link.
    if let Some(shstrtab) = &ctx.shstrtab {
        ehdr.e_shstrndx = if shstrtab.hdr.shndx < SHN_LORESERVE {
            shstrtab.hdr.shndx as u16
        } else {
            SHN_XINDEX as u16
        };
    }

    ehdr.e_type = if ctx.args.relocatable {
        ET_REL
    } else if ctx.args.pie && ctx.args.ttext_segment.is_some() {
        ET_EXEC
    } else if ctx.args.pic {
        ET_DYN
    } else {
        ET_EXEC
    } as u16;

    if let Some(phdr) = &ctx.phdr {
        ehdr.e_phoff = phdr.hdr.shdr.sh_offset;
        ehdr.e_phentsize = ElfPhdr::size::<E>() as u16;
        ehdr.e_phnum = (phdr.hdr.shdr.sh_size / ElfPhdr::size::<E>() as u64) as u16;
    }

    if let Some(shdr) = &ctx.shdr {
        ehdr.e_shoff = shdr.hdr.shdr.sh_offset;
        ehdr.e_shentsize = ElfShdr::size::<E>() as u16;
        // e_shnum is 16 bits; a larger count is stored in the zeroth
        // section's sh_size.
        let shnum = shdr.hdr.shdr.sh_size / ElfShdr::size::<E>() as u64;
        ehdr.e_shnum = if shnum <= u16::MAX as u64 {
            shnum as u16
        } else {
            0
        };
    }

    ehdr.write::<E>(buf);
}

fn write_shdr<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let size = ElfShdr::size::<E>();
    buf.fill(0);

    let mut first = ElfShdr::default();
    if let Some(shstrtab) = &ctx.shstrtab {
        if shstrtab.hdr.shndx >= SHN_LORESERVE {
            first.sh_link = shstrtab.hdr.shndx;
        }
    }
    let shnum = buf.len() / size;
    if shnum > u16::MAX as usize {
        first.sh_size = shnum as u64;
    }
    first.write::<E>(buf);

    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        if hdr.shndx != 0 {
            hdr.shdr.write::<E>(&mut buf[hdr.shndx as usize * size..]);
        }
    }
}

/// The segment flags a chunk requires.
pub fn to_phdr_flags<E: Arch>(ctx: &Context<E>, id: ChunkId) -> u32 {
    // All sections are put into a single RWX segment if --omagic
    if ctx.args.omagic {
        return PF_R | PF_W | PF_X;
    }

    let hdr = ctx.chunk_header(id);
    let write = hdr.shdr.sh_flags & SHF_WRITE as u64 != 0;
    let mut exec = hdr.shdr.sh_flags & SHF_EXECINSTR as u64 != 0;

    // .text is not readable if --execute-only
    if exec && ctx.args.execute_only {
        if write {
            error!(
                ctx,
                "--execute-only is not compatible with writable section: {}", hdr.name
            );
        }
        return PF_X;
    }

    // .rodata is merged with .text if --no-rosegment
    if !write && !ctx.args.rosegment {
        exec = true;
    }

    PF_R | if write { PF_W } else { 0 } | if exec { PF_X } else { 0 }
}

fn create_phdr<E: Arch>(ctx: &Context<E>) -> Vec<ElfPhdr> {
    let mut vec: Vec<ElfPhdr> = Vec::new();

    let define = |vec: &mut Vec<ElfPhdr>, p_type: u32, flags: u32, id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        let mut phdr = ElfPhdr {
            p_type,
            p_flags: flags,
            p_align: shdr.sh_addralign,
            ..ElfPhdr::default()
        };
        if shdr.sh_type == SHT_NOBITS {
            // p_offset is not significant for a segment with no file
            // contents, but some loaders want it congruent with the
            // virtual address modulo the page size.
            phdr.p_offset = shdr.sh_addr % ctx.page_size;
        } else {
            phdr.p_offset = shdr.sh_offset;
            phdr.p_filesz = shdr.sh_size;
        }
        phdr.p_vaddr = shdr.sh_addr;
        phdr.p_paddr = shdr.sh_addr;
        if shdr.sh_flags & SHF_ALLOC as u64 != 0 {
            phdr.p_memsz = shdr.sh_size;
        }
        vec.push(phdr);
    };

    let append = |vec: &mut Vec<ElfPhdr>, id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        let phdr = vec.last_mut().unwrap();
        phdr.p_align = phdr.p_align.max(shdr.sh_addralign);
        phdr.p_memsz = shdr.sh_addr + shdr.sh_size - phdr.p_vaddr;
        if shdr.sh_type != SHT_NOBITS {
            phdr.p_filesz = phdr.p_memsz;
        }
    };

    let is_bss = |id: ChunkId| ctx.chunk_header(id).shdr.sh_type == SHT_NOBITS;
    let is_tbss = |id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        shdr.sh_type == SHT_NOBITS && shdr.sh_flags & SHF_TLS as u64 != 0
    };
    let is_note = |id: ChunkId| ctx.chunk_header(id).shdr.sh_type == SHT_NOTE;

    // Only these chunks are considered when creating PT_LOAD segments.
    let mut chunks: Vec<ChunkId> = ctx
        .chunks
        .iter()
        .copied()
        .filter(|&id| ctx.chunk_header(id).is_alloc() && !is_tbss(id))
        .collect();

    // The ELF spec requires PT_LOAD entries to be sorted by p_vaddr.
    chunks.sort_by_key(|&id| ctx.chunk_header(id).shdr.sh_addr);

    // PT_PHDR for the program header itself.
    if let Some(phdr) = &ctx.phdr {
        if phdr.hdr.is_alloc() {
            define(&mut vec, PT_PHDR, PF_R, ChunkId::Phdr);
        }
    }

    if ctx.interp.is_some() {
        define(&mut vec, PT_INTERP, PF_R, ChunkId::Interp);
    }

    // PT_NOTE for note sections.
    let mut i = 0;
    while i < chunks.len() {
        let first = chunks[i];
        i += 1;
        if is_note(first) {
            let flags = to_phdr_flags(ctx, first);
            define(&mut vec, PT_NOTE, flags, first);
            while i < chunks.len() && is_note(chunks[i]) && to_phdr_flags(ctx, chunks[i]) == flags {
                append(&mut vec, chunks[i]);
                i += 1;
            }
        }
    }

    // PT_LOAD segments.
    let mut i = 0;
    while i < chunks.len() {
        let first = chunks[i];
        i += 1;
        let flags = to_phdr_flags(ctx, first);
        define(&mut vec, PT_LOAD, flags, first);
        if !ctx.args.nmagic && !ctx.args.omagic {
            let last = vec.last_mut().unwrap();
            last.p_align = last.p_align.max(ctx.page_size);
        }

        // Add contiguous ALLOC sections as long as they have the same
        // section flags and there's no on-disk gap in between.
        if !is_bss(first) {
            let first_shdr = ctx.chunk_header(first).shdr;
            while i < chunks.len()
                && !is_bss(chunks[i])
                && to_phdr_flags(ctx, chunks[i]) == flags
                && {
                    let shdr = &ctx.chunk_header(chunks[i]).shdr;
                    shdr.sh_offset.wrapping_sub(first_shdr.sh_offset)
                        == shdr.sh_addr.wrapping_sub(first_shdr.sh_addr)
                }
            {
                append(&mut vec, chunks[i]);
                i += 1;
            }
        }
        while i < chunks.len() && is_bss(chunks[i]) && to_phdr_flags(ctx, chunks[i]) == flags {
            append(&mut vec, chunks[i]);
            i += 1;
        }
    }

    // PT_TLS
    let is_tls = |id: ChunkId| ctx.chunk_header(id).shdr.sh_flags & SHF_TLS as u64 != 0;
    let mut i = 0;
    while i < ctx.chunks.len() {
        let first = ctx.chunks[i];
        i += 1;
        if is_tls(first) {
            define(&mut vec, PT_TLS, PF_R, first);
            while i < ctx.chunks.len() && is_tls(ctx.chunks[i]) {
                append(&mut vec, ctx.chunks[i]);
                i += 1;
            }
        }
    }

    if let Some(dynamic) = &ctx.dynamic {
        if dynamic.hdr.shdr.sh_size != 0 {
            let flags = to_phdr_flags(ctx, ChunkId::Dynamic);
            define(&mut vec, PT_DYNAMIC, flags, ChunkId::Dynamic);
        }
    }

    if ctx.eh_frame_hdr.is_some() {
        define(&mut vec, PT_GNU_EH_FRAME, PF_R, ChunkId::EhFrameHdr);
    }

    if ctx.sframe.hdr.shdr.sh_size != 0 && ctx.chunks.contains(&ChunkId::SFrame) {
        define(&mut vec, PT_GNU_SFRAME, PF_R, ChunkId::SFrame);
    }

    if let Some(id) = ctx.find_chunk_by_name(b".note.gnu.property") {
        define(&mut vec, PT_GNU_PROPERTY, PF_R, id);
    }

    if ctx
        .riscv_attributes
        .as_ref()
        .is_some_and(|sec| sec.hdr.shdr.sh_size != 0)
    {
        define(
            &mut vec,
            PT_RISCV_ATTRIBUTES,
            PF_R,
            ChunkId::RiscvAttributes,
        );
    }

    if ctx.arm_exidx.is_some() {
        define(&mut vec, PT_ARM_EXIDX, PF_R, ChunkId::ArmExidx);
    }

    // PT_GNU_STACK is a marker segment controlling the executable bit of
    // the stack area.
    vec.push(ElfPhdr {
        p_type: PT_GNU_STACK,
        p_flags: if ctx.args.z_execstack {
            PF_R | PF_W | PF_X
        } else {
            PF_R | PF_W
        },
        p_memsz: ctx.args.z_stack_size,
        p_align: 1,
        ..ElfPhdr::default()
    });

    // PT_GNU_RELRO
    if ctx.args.z_relro {
        let mut i = 0;
        while i < chunks.len() {
            let first = chunks[i];
            i += 1;
            if ctx.chunk_header(first).is_relro {
                define(&mut vec, PT_GNU_RELRO, PF_R, first);
                while i < chunks.len() && ctx.chunk_header(chunks[i]).is_relro {
                    append(&mut vec, chunks[i]);
                    i += 1;
                }
                vec.last_mut().unwrap().p_align = 1;
            }
        }
    }

    for &id in &ctx.chunks {
        if ctx.chunk_header(id).name == b".openbsd.randomdata" {
            define(&mut vec, PT_OPENBSD_RANDOMIZE, PF_R | PF_W, id);
        }
    }

    // --physical-image-base sets p_paddr for embedded programs whose
    // segments start out in ROM. We keep vaddr == paddr for as many
    // segments as possible so that they can be used in place, but give
    // up once a gap between segments is two pages or larger.
    if let Some(base) = ctx.args.physical_image_base {
        if let Some(first) = vec.iter().position(|p| p.p_type == PT_LOAD) {
            let mut addr = base;
            let mut in_sync = vec[first].p_vaddr == addr;
            vec[first].p_paddr = addr;
            addr += vec[first].p_memsz;

            for p in vec[first + 1..]
                .iter_mut()
                .take_while(|p| p.p_type == PT_LOAD)
            {
                if in_sync && addr <= p.p_vaddr && p.p_vaddr < addr + ctx.page_size * 2 {
                    p.p_paddr = p.p_vaddr;
                    addr = p.p_vaddr + p.p_memsz;
                } else {
                    in_sync = false;
                    p.p_paddr = addr;
                    addr += p.p_memsz;
                }
            }
        }
    }

    vec.resize(
        vec.len() + ctx.args.spare_program_headers.max(0) as usize,
        ElfPhdr::default(),
    );
    vec
}

/// Recomputes the program header and the TLS layout constants.
pub fn update_phdr<E: Arch>(ctx: &mut Context<E>) {
    if ctx.phdr.is_none() {
        return;
    }
    let phdrs = create_phdr(ctx);
    for phdr in &phdrs {
        if phdr.p_type == PT_TLS {
            ctx.tls_begin = phdr.p_vaddr;
            ctx.tp_addr = tls::tp_addr::<E>(phdr);
            ctx.dtp_addr = tls::dtp_addr::<E>(phdr);
            break;
        }
    }
    let phdr = ctx.phdr.as_mut().unwrap();
    phdr.hdr.shdr.sh_size = (phdrs.len() * ElfPhdr::size::<E>()) as u64;
    phdr.phdrs = phdrs;
}

/// Updates a chunk's section header for the current layout. Called at
/// least twice: once to size the section, and again after section
/// indices are known.
pub fn update_shdr<E: Arch>(ctx: &mut Context<E>, id: ChunkId) {
    match id {
        ChunkId::Phdr => update_phdr(ctx),
        ChunkId::Interp => misc::interp::update_shdr(ctx),
        ChunkId::GotPlt => got::gotplt::update_shdr(ctx),
        ChunkId::RelPlt => got::relplt::update_shdr(ctx),
        ChunkId::RelDyn => dynamic::reldyn::update_shdr(ctx),
        ChunkId::Dynamic => dynamic::dynamic::update_shdr(ctx),
        ChunkId::Strtab => symtab::strtab::update_shdr(ctx),
        ChunkId::Shstrtab => symtab::shstrtab::update_shdr(ctx),
        ChunkId::Plt => got::plt::update_shdr(ctx),
        ChunkId::Symtab => symtab::symtab::update_shdr(ctx),
        ChunkId::Dynsym => symtab::dynsym::update_shdr(ctx),
        ChunkId::Hash => symtab::hash::update_shdr(ctx),
        ChunkId::GnuHash => symtab::gnu_hash::update_shdr(ctx),
        ChunkId::EhFrameHdr => eh_frame::eh_frame_hdr::update_shdr(ctx),
        ChunkId::EhFrameReloc => eh_frame::eh_frame_reloc::update_shdr(ctx),
        ChunkId::SFrameReloc => sframe::sframe_reloc::update_shdr(ctx),
        ChunkId::Versym => version::versym::update_shdr(ctx),
        ChunkId::Verneed => version::verneed::update_shdr(ctx),
        ChunkId::Verdef => version::verdef::update_shdr(ctx),
        ChunkId::BuildId => misc::build_id::update_shdr(ctx),
        ChunkId::NotePackage => misc::note_package::update_shdr(ctx),
        ChunkId::NoteProperty => misc::note_property::update_shdr(ctx),
        ChunkId::RiscvAttributes => misc::riscv_attributes::update_shdr(ctx),
        ChunkId::ArmExidx => arm_exidx::update_shdr(ctx),
        ChunkId::GnuDebuglink => misc::gnu_debuglink::update_shdr(ctx),
        ChunkId::Reloc(i) => misc::reloc::update_shdr(ctx, i),
        ChunkId::ComdatGroup(i) => misc::comdat_group::update_shdr(ctx, i),
        _ => {}
    }
}

/// Computes a chunk's size from its contents. For output sections this
/// also assigns offsets to the members.
pub fn compute_section_size<E: Arch>(ctx: &mut Context<E>, id: ChunkId) {
    match id {
        ChunkId::Output(id) => output_section::compute_section_size(ctx, id),
        ChunkId::Merged(id) => merged::compute_section_size(ctx, id),
        ChunkId::ArmExidx => arm_exidx::compute_section_size(ctx),
        _ => {}
    }
}

/// The number of dynamic relocations a chunk emits.
pub fn num_dynrels<E: Arch>(ctx: &Context<E>, id: ChunkId) -> u64 {
    match id {
        ChunkId::Output(id) => output_section::num_dynrels(ctx, id),
        ChunkId::Got => got::got::num_dynrels(ctx),
        ChunkId::Copyrel => ctx.copyrel.symbols.len() as u64,
        ChunkId::CopyrelRelro => ctx.copyrel_relro.symbols.len() as u64,
        ChunkId::Ppc64Opd => opd::num_dynrels(ctx),
        _ => 0,
    }
}

/// The offsets (relative to the chunk) of base relocations that can be
/// encoded in RELR form, marking them as such.
pub fn relr_offsets<E: Arch>(ctx: &mut Context<E>, id: ChunkId) -> Vec<u64> {
    match id {
        ChunkId::Output(id) => output_section::relr_offsets(ctx, id),
        ChunkId::Got => got::got::relr_offsets(ctx),
        ChunkId::Ppc64Opd => opd::relr_offsets(ctx),
        _ => Vec::new(),
    }
}

/// Storage for a chunk's dynamic relocations. Native records are used by
/// Android packing and when the target's records have the host layout;
/// other targets are encoded directly into their output bytes.
pub enum DynRelBuffer<'a, E: Arch> {
    Native(&'a mut [ElfRel]),
    Encoded(&'a mut [u8], PhantomData<E>),
}

impl<'a, E: Arch> DynRelBuffer<'a, E> {
    pub fn native(rels: &'a mut [ElfRel]) -> Self {
        DynRelBuffer::Native(rels)
    }

    pub fn output(buf: &'a mut [u8]) -> Self {
        debug_assert!(buf.len().is_multiple_of(ElfRel::size::<E>()));

        if E::IS_64
            && E::IS_RELA
            && E::Endian::IS_NATIVE
            && buf.as_ptr().align_offset(std::mem::align_of::<ElfRel>()) == 0
        {
            let len = buf.len() / std::mem::size_of::<ElfRel>();
            // SAFETY: ElfRel has the native ELF64 Rela field layout when the
            // target and host have the same byte order, the output section is
            // suitably aligned, every bit pattern is valid for its integer
            // fields, and `buf` is borrowed exclusively for the returned
            // slice's lifetime.
            let rels =
                unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<ElfRel>(), len) };
            return DynRelBuffer::Native(rels);
        }

        DynRelBuffer::Encoded(buf, PhantomData)
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            DynRelBuffer::Native(rels) => rels.len(),
            DynRelBuffer::Encoded(buf, _) => buf.len() / ElfRel::size::<E>(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn write(&mut self, i: usize, rel: ElfRel) {
        match self {
            DynRelBuffer::Native(rels) => rels[i] = rel,
            DynRelBuffer::Encoded(buf, _) => {
                let size = ElfRel::size::<E>();
                rel.write::<E>(&mut buf[i * size..(i + 1) * size]);
            }
        }
    }

    pub fn split_at_offsets(self, offsets: &[u64]) -> Vec<Self> {
        match self {
            DynRelBuffer::Native(rels) => crate::output_file::split_at_offsets(rels, offsets)
                .into_iter()
                .map(DynRelBuffer::Native)
                .collect(),
            DynRelBuffer::Encoded(buf, _) => {
                let size = ElfRel::size::<E>() as u64;
                let offsets: Vec<u64> = offsets.iter().map(|&offset| offset * size).collect();
                crate::output_file::split_at_offsets(buf, &offsets)
                    .into_iter()
                    .map(|buf| DynRelBuffer::Encoded(buf, PhantomData))
                    .collect()
            }
        }
    }
}

/// Writes a chunk's dynamic relocations to its assigned output slots.
pub fn write_dynrels<E: Arch>(ctx: &Context<E>, id: ChunkId, out: DynRelBuffer<'_, E>) {
    match id {
        ChunkId::Output(id) => output_section::write_dynrels(ctx, id, out),
        ChunkId::Got => got::got::write_dynrels(ctx, out),
        ChunkId::Copyrel => misc::copyrel::write_dynrels(ctx, &ctx.copyrel, out),
        ChunkId::CopyrelRelro => misc::copyrel::write_dynrels(ctx, &ctx.copyrel_relro, out),
        ChunkId::Ppc64Opd => opd::write_dynrels(ctx, out),
        _ => {}
    }
}

/// Sizes the local symbols a chunk synthesizes.
pub fn compute_symtab_size<E: Arch>(ctx: &mut Context<E>, id: ChunkId) {
    match id {
        ChunkId::Output(id) => output_section::compute_symtab_size(ctx, id),
        ChunkId::Got => got::got::compute_symtab_size(ctx),
        ChunkId::Plt => got::plt::compute_symtab_size(ctx),
        ChunkId::PltGot => got::pltgot::compute_symtab_size(ctx),
        _ => {}
    }
}

/// Produces the local symbols a chunk synthesizes.
pub fn populate_symtab<E: Arch>(ctx: &Context<E>, id: ChunkId, block: &mut SymtabBlock<'_>) {
    match id {
        ChunkId::Output(id) => output_section::populate_symtab(ctx, id, block),
        ChunkId::Got => got::got::populate_symtab(ctx, block),
        ChunkId::Plt => got::plt::populate_symtab(ctx, block),
        ChunkId::PltGot => got::pltgot::populate_symtab(ctx, block),
        _ => {}
    }
}

/// Writes a chunk's contents into its region of the output file.
///
/// Chunks whose contents spill into other chunks (`.eh_frame` writes the
/// `.eh_frame_hdr` table, `.symtab` writes `.strtab`) are handled by the
/// output file writer, which hands them the extra buffers.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, id: ChunkId, buf: &mut [u8]) {
    match id {
        ChunkId::Ehdr => write_ehdr(ctx, buf),
        ChunkId::Shdr => write_shdr(ctx, buf),
        ChunkId::Phdr => {
            let phdrs = &ctx.phdr.as_ref().unwrap().phdrs;
            ElfPhdr::write_all::<E>(phdrs, buf);
        }
        ChunkId::Interp => misc::interp::copy_buf(ctx, buf),
        ChunkId::Got => got::got::copy_buf(ctx, buf),
        ChunkId::GotPlt => got::gotplt::copy_buf(ctx, buf),
        ChunkId::RelPlt => got::relplt::copy_buf(ctx, buf),
        ChunkId::RelDyn => dynamic::reldyn::copy_buf(ctx, buf),
        ChunkId::RelrDyn => dynamic::relrdyn::copy_buf(ctx, buf),
        ChunkId::Dynamic => dynamic::dynamic::copy_buf(ctx, buf),
        ChunkId::Strtab => symtab::strtab::copy_buf(ctx, buf),
        ChunkId::Dynstr => symtab::dynstr::copy_buf(ctx, buf),
        ChunkId::Hash => symtab::hash::copy_buf(ctx, buf),
        ChunkId::GnuHash => symtab::gnu_hash::copy_buf(ctx, buf),
        ChunkId::GnuDebuglink => misc::gnu_debuglink::copy_buf(ctx, buf),
        ChunkId::Shstrtab => symtab::shstrtab::copy_buf(ctx, buf),
        ChunkId::Plt => got::plt::copy_buf(ctx, buf),
        ChunkId::PltGot => got::pltgot::copy_buf(ctx, buf),
        ChunkId::Symtab | ChunkId::SymtabShndx => {}
        ChunkId::Dynsym => symtab::dynsym::copy_buf(ctx, buf),
        ChunkId::EhFrame | ChunkId::EhFrameHdr => {}
        ChunkId::EhFrameReloc => {}
        ChunkId::SFrame => sframe::copy_buf(ctx, buf),
        ChunkId::SFrameReloc => sframe::sframe_reloc::copy_buf(ctx, buf),
        ChunkId::Copyrel | ChunkId::CopyrelRelro => {}
        ChunkId::Versym => version::versym::copy_buf(ctx, buf),
        ChunkId::Verneed => version::verneed::copy_buf(ctx, buf),
        ChunkId::Verdef => version::verdef::copy_buf(ctx, buf),
        ChunkId::BuildId => misc::build_id::copy_buf(ctx, buf),
        ChunkId::NotePackage => misc::note_package::copy_buf(ctx, buf),
        ChunkId::NoteProperty => misc::note_property::copy_buf(ctx, buf),
        ChunkId::RiscvAttributes => misc::riscv_attributes::copy_buf(ctx, buf),
        ChunkId::ArmExidx => arm_exidx::copy_buf(ctx, buf),
        ChunkId::Ppc64SaveRestore => misc::ppc64_save_restore::copy_buf(ctx, buf),
        ChunkId::Ppc64Opd => opd::copy_buf(ctx, buf),
        ChunkId::GdbIndex | ChunkId::RelroPadding | ChunkId::Placeholder(_) => {}
        ChunkId::Output(id) => output_section::copy_buf(ctx, id, buf),
        ChunkId::Merged(id) => merged::copy_buf(ctx, id, buf),
        ChunkId::Reloc(_) => {}
        ChunkId::ComdatGroup(i) => misc::comdat_group::copy_buf(ctx, i, buf),
        ChunkId::Compressed(i) => misc::compressed::copy_buf(ctx, i, buf),
    }
}

/// Writes a chunk's contents to a scratch buffer, for compression.
pub fn write_to<E: Arch>(ctx: &Context<E>, id: ChunkId, buf: &mut [u8]) {
    match id {
        ChunkId::Output(id) => output_section::write_to(ctx, id, buf),
        ChunkId::Merged(id) => merged::write_to(ctx, id, buf),
        _ => unreachable!("write_to is only for output and merged sections"),
    }
}

/// The address of a synthesized symbol placed at the start of a chunk.
pub fn chunk_start<E: Arch>(ctx: &Context<E>, id: ChunkId) -> u64 {
    ctx.chunk_header(id).shdr.sh_addr
}

/// Whether a chunk should be kept after `--section-order` filtering.
pub fn section_order_index<E: Arch>(
    ctx: &Context<E>,
    name: &[u8],
    kind: SectionOrderKind,
) -> Option<usize> {
    ctx.args
        .section_order
        .iter()
        .position(|o| o.kind == kind && o.name.as_bytes() == name)
}

/// Resolves a symbol's address without going through its PLT entry.
pub fn addr_no_plt<E: Arch>(ctx: &Context<E>, sym: &crate::symbol::Symbol) -> u64 {
    sym.addr_with(ctx, AddrFlags::NO_PLT)
}
