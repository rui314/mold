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

use std::num::NonZeroU32;

use bstr::BStr;

use crate::arch::Arch;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{FileId, SymtabBlock};
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

// Chunk represents a contiguous region in an output file.
#[derive(Debug)]
pub struct ChunkHeader<E: Layout> {
    pub name: &'static BStr,
    pub shdr: ElfShdr<E>,

    /// Index in the output section header table; 0 for headers.
    pub shndx: u32,

    pub num_dynrels: u64,
    pub num_relrs: u64,
    pub relr: Vec<u64>,
    pub is_relro: bool,

    /// For --gdb-index
    pub is_compressed: bool,

    // Some synethetic sections add local symbols to the output.
    // For example, range extension thunks adds function_name@thunk
    // symbol for each thunk entry. The following members are used
    // for such synthesizing symbols.
    pub local_symtab_idx: u32,
    pub num_local_symtab: u32,
    pub strtab_size: u64,
    pub strtab_offset: u64,

    /// For --section-order
    pub sect_order: i64,
}

impl<E: Layout> ChunkHeader<E> {
    pub fn new(name: &'static str, sh_type: u32, sh_flags: u64) -> ChunkHeader<E> {
        ChunkHeader {
            name: BStr::new(name.as_bytes()),
            shdr: {
                let mut shdr = ElfShdr::<E>::default();
                shdr.sh_type.set(sh_type);
                shdr.sh_flags.set(sh_flags);
                shdr.sh_addralign.set(1);
                shdr
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

    pub fn with_name(name: &'static BStr, sh_type: u32, sh_flags: u64) -> ChunkHeader<E> {
        ChunkHeader {
            name,
            ..ChunkHeader::<E>::new("", sh_type, sh_flags)
        }
    }

    pub fn is_alloc(&self) -> bool {
        self.shdr.sh_flags.get() & SHF_ALLOC as u64 != 0
    }
}

// ELF header which is at the beginning of each ELF file.
#[derive(Debug)]
pub struct OutputEhdr<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> OutputEhdr<E> {
    pub fn new(sh_flags: u64) -> OutputEhdr<E> {
        let mut hdr = ChunkHeader::<E>::new("EHDR", 0, sh_flags);
        hdr.shdr.sh_size.set(ElfEhdr::<E>::size() as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        OutputEhdr { hdr }
    }
}

// OutputShdr represents the section header. The section header is usually
// located at the end of an ELF file and is optional for executables.
// Executables work without it because the runtime only reads the program
// header. Section header is significant only in object files and not
// needed at runtime
#[derive(Debug)]
pub struct OutputShdr<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> OutputShdr<E> {
    pub fn new() -> OutputShdr<E> {
        let mut hdr = ChunkHeader::<E>::new("SHDR", 0, 0);
        hdr.shdr.sh_size.set(1);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        OutputShdr { hdr }
    }
}

impl<E: Arch> Default for OutputShdr<E> {
    fn default() -> Self {
        Self::new()
    }
}

// Program header, a.k.a. segment header. Each entry in the program header
// represents a contiguous region of memory and has attributes such as
// page protection bits. On program startup, the kernel mmap's the file
// contents to memory based on the program header.
#[derive(Debug)]
pub struct OutputPhdr<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub phdrs: Vec<ElfPhdr<E>>,
}

impl<E: Arch> OutputPhdr<E> {
    pub fn new(sh_flags: u64) -> OutputPhdr<E> {
        let mut hdr = ChunkHeader::<E>::new("PHDR", 0, sh_flags);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        OutputPhdr {
            hdr,
            phdrs: Vec::new(),
        }
    }
}

// .gdb_index contains several tables to speed up gdb start-up.
#[derive(Debug)]
pub struct GdbIndexSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Layout> GdbIndexSection<E> {
    pub fn new() -> GdbIndexSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".gdb_index", SHT_PROGBITS, 0);
        hdr.shdr.sh_addralign.set(4);
        GdbIndexSection { hdr }
    }
}

impl<E: Layout> Default for GdbIndexSection<E> {
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
        warn!("entry symbol is not defined: {sym}");
    }
    0
}

fn write_ehdr<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut ehdr = ElfEhdr::<E>::default();
    ehdr.e_ident[..4].copy_from_slice(b"\x7fELF");
    ehdr.e_ident[EI_CLASS as usize] = if E::IS_64 { ELFCLASS64 } else { ELFCLASS32 } as u8;
    ehdr.e_ident[EI_DATA as usize] = if E::IS_LITTLE_ENDIAN {
        ELFDATA2LSB
    } else {
        ELFDATA2MSB
    } as u8;
    ehdr.e_ident[EI_VERSION as usize] = EV_CURRENT as u8;
    ehdr.e_machine.set(E::E_MACHINE as u16);
    ehdr.e_version.set(EV_CURRENT);
    ehdr.e_entry.set(entry_addr(ctx));
    ehdr.e_flags.set(E::eflags(ctx));
    ehdr.e_ehsize.set(ElfEhdr::<E>::size() as u16);

    // If e_shstrndx is too large, a dummy value is set to e_shstrndx.
    // The real value is stored to the zero'th section's sh_link field.
    if let Some(shstrtab) = &ctx.shstrtab {
        ehdr.e_shstrndx.set(if shstrtab.hdr.shndx < SHN_LORESERVE {
            shstrtab.hdr.shndx as u16
        } else {
            SHN_XINDEX as u16
        });
    }

    ehdr.e_type.set(if ctx.args.relocatable {
        ET_REL
    } else if ctx.args.pie && ctx.args.ttext_segment.is_some() {
        ET_EXEC
    } else if ctx.args.pic {
        ET_DYN
    } else {
        ET_EXEC
    } as u16);

    if let Some(phdr) = &ctx.phdr {
        ehdr.e_phoff.set(phdr.hdr.shdr.sh_offset.get());
        ehdr.e_phentsize
            .set(std::mem::size_of::<ElfPhdr<E>>() as u16);
        ehdr.e_phnum
            .set((phdr.hdr.shdr.sh_size.get() / std::mem::size_of::<ElfPhdr<E>>() as u64) as u16);
    }

    if let Some(shdr) = &ctx.shdr {
        ehdr.e_shoff.set(shdr.hdr.shdr.sh_offset.get());
        ehdr.e_shentsize.set(ElfShdr::<E>::size() as u16);
        // Since e_shnum is a 16-bit integer field, we can't store a very
        // large value there. If it is >65535, the real value is stored to
        // the zero'th section's sh_size field.
        let shnum = shdr.hdr.shdr.sh_size.get() / ElfShdr::<E>::size() as u64;
        ehdr.e_shnum.set(if shnum <= u16::MAX as u64 {
            shnum as u16
        } else {
            0
        });
    }

    ehdr.write(buf);
}

fn write_shdr<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let size = ElfShdr::<E>::size();
    buf.fill(0);

    let mut first = ElfShdr::<E>::default();
    if let Some(shstrtab) = &ctx.shstrtab {
        if shstrtab.hdr.shndx >= SHN_LORESERVE {
            first.sh_link.set(shstrtab.hdr.shndx);
        }
    }
    let shnum = buf.len() / size;
    if shnum > u16::MAX as usize {
        first.sh_size.set(shnum as u64);
    }
    first.write(buf);

    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        if hdr.shndx != 0 {
            hdr.shdr.write(&mut buf[hdr.shndx as usize * size..]);
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
    let write = hdr.shdr.sh_flags.get() & SHF_WRITE as u64 != 0;
    let mut exec = hdr.shdr.sh_flags.get() & SHF_EXECINSTR as u64 != 0;

    // .text is not readable if --execute-only
    if exec && ctx.args.execute_only {
        if write {
            error!(
                "--execute-only is not compatible with writable section: {}",
                hdr.name
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

fn create_phdr<E: Arch>(ctx: &Context<E>) -> Vec<ElfPhdr<E>> {
    let mut vec: Vec<ElfPhdr<E>> = Vec::new();

    let define = |vec: &mut Vec<ElfPhdr<E>>, p_type: u32, flags: u32, id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        let mut phdr = ElfPhdr::<E>::default();
        phdr.p_type_mut().set(p_type);
        phdr.p_flags_mut().set(flags);
        phdr.p_align_mut().set(shdr.sh_addralign.get());
        if shdr.sh_type.get() == SHT_NOBITS {
            // p_offset indicates the in-file start offset and is not
            // significant for segments with zero on-file size. We still want to
            // keep it congruent with the virtual address modulo page size
            // because some loaders (at least FreeBSD's) are picky about it.
            phdr.p_offset_mut().set(shdr.sh_addr.get() % ctx.page_size);
        } else {
            phdr.p_offset_mut().set(shdr.sh_offset.get());
            phdr.p_filesz_mut().set(shdr.sh_size.get());
        }
        phdr.p_vaddr_mut().set(shdr.sh_addr.get());
        phdr.p_paddr_mut().set(shdr.sh_addr.get());
        if shdr.sh_flags.get() & SHF_ALLOC as u64 != 0 {
            phdr.p_memsz_mut().set(shdr.sh_size.get());
        }
        vec.push(phdr);
    };

    let append = |vec: &mut Vec<ElfPhdr<E>>, id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        let phdr = vec.last_mut().unwrap();
        let align = phdr.p_align().get().max(shdr.sh_addralign.get());
        phdr.p_align_mut().set(align);
        let memsz = shdr.sh_addr.get() + shdr.sh_size.get() - phdr.p_vaddr().get();
        phdr.p_memsz_mut().set(memsz);
        if shdr.sh_type.get() != SHT_NOBITS {
            phdr.p_filesz_mut().set(memsz);
        }
    };

    let is_bss = |id: ChunkId| ctx.chunk_header(id).shdr.sh_type.get() == SHT_NOBITS;
    let is_tbss = |id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        shdr.sh_type.get() == SHT_NOBITS && shdr.sh_flags.get() & SHF_TLS as u64 != 0
    };
    let is_note = |id: ChunkId| ctx.chunk_header(id).shdr.sh_type.get() == SHT_NOTE;

    // When we are creating PT_LOAD segments, we consider only
    // the following chunks.
    let mut chunks: Vec<ChunkId> = ctx
        .chunks
        .iter()
        .copied()
        .filter(|&id| ctx.chunk_header(id).is_alloc() && !is_tbss(id))
        .collect();

    // The ELF spec says that "loadable segment entries in the program
    // header table appear in ascending order, sorted on the p_vaddr
    // member".
    chunks.sort_by_key(|&id| ctx.chunk_header(id).shdr.sh_addr.get());

    // Create a PT_PHDR for the program header itself.
    if let Some(phdr) = &ctx.phdr {
        if phdr.hdr.is_alloc() {
            define(&mut vec, PT_PHDR, PF_R, ChunkId::Phdr);
        }
    }

    // Create a PT_INTERP.
    if ctx.interp.is_some() {
        define(&mut vec, PT_INTERP, PF_R, ChunkId::Interp);
    }

    // Create a PT_NOTE for SHF_NOTE sections.
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

    // Create PT_LOAD segments.
    let mut i = 0;
    while i < chunks.len() {
        let first = chunks[i];
        i += 1;
        let flags = to_phdr_flags(ctx, first);
        define(&mut vec, PT_LOAD, flags, first);
        if !ctx.args.nmagic && !ctx.args.omagic {
            let last = vec.last_mut().unwrap();
            let align = last.p_align().get().max(ctx.page_size);
            last.p_align_mut().set(align);
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
                    shdr.sh_offset
                        .get()
                        .wrapping_sub(first_shdr.sh_offset.get())
                        == shdr.sh_addr.get().wrapping_sub(first_shdr.sh_addr.get())
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

    // Create a PT_TLS.
    let is_tls = |id: ChunkId| ctx.chunk_header(id).shdr.sh_flags.get() & SHF_TLS as u64 != 0;
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

    // Add PT_DYNAMIC
    if let Some(dynamic) = &ctx.dynamic {
        if dynamic.hdr.shdr.sh_size.get() != 0 {
            let flags = to_phdr_flags(ctx, ChunkId::Dynamic);
            define(&mut vec, PT_DYNAMIC, flags, ChunkId::Dynamic);
        }
    }

    // Add PT_GNU_EH_FRAME
    if ctx.eh_frame_hdr.is_some() {
        define(&mut vec, PT_GNU_EH_FRAME, PF_R, ChunkId::EhFrameHdr);
    }

    // Add PT_GNU_SFRAME
    if ctx.sframe.hdr.shdr.sh_size.get() != 0 && ctx.chunks.contains(&ChunkId::SFrame) {
        define(&mut vec, PT_GNU_SFRAME, PF_R, ChunkId::SFrame);
    }

    // Add PT_GNU_PROPERTY
    if let Some(id) = ctx.find_chunk_by_name(b".note.gnu.property") {
        define(&mut vec, PT_GNU_PROPERTY, PF_R, id);
    }

    // Create a PT_RISCV_ATTRIBUTES
    if ctx
        .riscv_attributes
        .as_ref()
        .is_some_and(|sec| sec.hdr.shdr.sh_size.get() != 0)
    {
        define(
            &mut vec,
            PT_RISCV_ATTRIBUTES,
            PF_R,
            ChunkId::RiscvAttributes,
        );
    }

    // Create a PT_ARM_EDXIDX
    if ctx.arm_exidx.is_some() {
        define(&mut vec, PT_ARM_EXIDX, PF_R, ChunkId::ArmExidx);
    }

    // Add PT_GNU_STACK, which is a marker segment that doesn't really
    // contain any segments. It controls executable bit of stack area.
    let mut stack = ElfPhdr::<E>::default();
    stack.p_type_mut().set(PT_GNU_STACK);
    stack.p_flags_mut().set(if ctx.args.z_execstack {
        PF_R | PF_W | PF_X
    } else {
        PF_R | PF_W
    });
    stack.p_memsz_mut().set(ctx.args.z_stack_size);
    stack.p_align_mut().set(1);
    vec.push(stack);

    // Create a PT_GNU_RELRO.
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
                vec.last_mut().unwrap().p_align_mut().set(1);
            }
        }
    }

    // Create a PT_OPENBSD_RANDOMIZE
    for &id in &ctx.chunks {
        if ctx.chunk_header(id).name == b".openbsd.randomdata" {
            define(&mut vec, PT_OPENBSD_RANDOMIZE, PF_R | PF_W, id);
        }
    }

    // Set p_paddr if --physical-image-base was given. --physical-image-base
    // is typically used in embedded programming to specify the base address
    // of a memory-mapped ROM area. In that environment, paddr refers to a
    // segment's initial location in ROM and vaddr refers the its run-time
    // address.
    //
    // When a device is turned on, it start executing code at a fixed
    // location in the ROM area. At that location is a startup routine that
    // copies data or code from ROM to RAM before using them.
    //
    // .data must have different paddr and vaddr because ROM is not writable.
    // paddr of .rodata and .text may or may be equal to vaddr. They can be
    // directly read or executed from ROM, but oftentimes they are copied
    // from ROM to RAM because Flash or EEPROM are usually much slower than
    // DRAM.
    //
    // We want to keep vaddr == pvaddr for as many segments as possible so
    // that they can be directly read/executed from ROM. If a gap between
    // two segments is two page size or larger, we give up and pack segments
    // tightly so that we don't waste too much ROM area.
    if let Some(base) = ctx.args.physical_image_base {
        if let Some(first) = vec.iter().position(|p| p.p_type().get() == PT_LOAD) {
            let mut addr = base;
            let mut in_sync = vec[first].p_vaddr().get() == addr;
            vec[first].p_paddr_mut().set(addr);
            addr += vec[first].p_memsz().get();

            for p in vec[first + 1..]
                .iter_mut()
                .take_while(|p| p.p_type().get() == PT_LOAD)
            {
                if in_sync
                    && addr <= p.p_vaddr().get()
                    && p.p_vaddr().get() < addr + ctx.page_size * 2
                {
                    let vaddr = p.p_vaddr().get();
                    p.p_paddr_mut().set(vaddr);
                    addr = vaddr + p.p_memsz().get();
                } else {
                    in_sync = false;
                    p.p_paddr_mut().set(addr);
                    addr += p.p_memsz().get();
                }
            }
        }
    }

    vec.resize(
        vec.len() + ctx.args.spare_program_headers.max(0) as usize,
        ElfPhdr::<E>::default(),
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
        if phdr.p_type().get() == PT_TLS {
            ctx.tls_begin = phdr.p_vaddr().get();
            ctx.tp_addr = tls::tp_addr::<E>(phdr);
            ctx.dtp_addr = tls::dtp_addr::<E>(phdr);
            break;
        }
    }
    let phdr = ctx.phdr.as_mut().unwrap();
    phdr.hdr
        .shdr
        .sh_size
        .set((phdrs.len() * std::mem::size_of::<ElfPhdr<E>>()) as u64);
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

/// Writes a chunk's dynamic relocations to its assigned output slots.
pub fn write_dynrels<E: Arch>(ctx: &Context<E>, id: ChunkId, out: &mut [E::Rel]) {
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
            ElfPhdr::<E>::write_all(phdrs, buf);
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
