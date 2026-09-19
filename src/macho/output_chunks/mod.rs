//! Output chunks: the pieces an output file is assembled from.
//!
//! A chunk is a contiguous byte range of the output: the mach header with
//! its load commands, an output section collecting input sections, or a
//! table in __LINKEDIT. Each kind is a struct of its own holding a
//! ChunkHeader and the data it is written from, reached through the
//! typed fields of Context; a ChunkId names one, and `ctx.chunks` lists
//! the chunks of the output in file order. Segments group chunks for
//! the LC_SEGMENT_64 load commands. mold-rust's output_chunks has the
//! same shape.

pub mod chained_fixups;
pub mod dyld_info;
pub mod eh_frame;
pub mod export_trie;
pub mod got;
pub mod misc;
pub mod objc;
pub mod output_section;
pub mod symtab;
pub mod unwind_info;

use std::num::NonZeroU32;

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;

pub use output_section::{OutputSection, Tail, Thunk};

#[derive(Debug)]
pub struct ChunkHeader {
    pub segname: &'static str,
    pub sectname: String,
    pub addr: u64,
    pub fileoff: u64,
    pub size: u64,
    pub p2align: u32,
    pub flags: u32,
    pub reserved1: u32,
    pub reserved2: u32,
    /// Whether the chunk is described by a section header in its
    /// segment's load command. Linkedit tables and the mach header are
    /// not.
    pub is_sect: bool,
    /// The 1-based ordinal of the section among the output's sections
    /// (what an nlist's n_sect holds), 0 for a chunk that is not a
    /// section; mold-rust's shndx.
    pub n_sect: u8,
}

impl ChunkHeader {
    /// The header of a section of the image.
    pub fn new(segname: &'static str, sectname: &str) -> ChunkHeader {
        ChunkHeader {
            segname,
            sectname: sectname.to_string(),
            addr: 0,
            fileoff: 0,
            size: 0,
            p2align: 0,
            flags: 0,
            reserved1: 0,
            reserved2: 0,
            is_sect: true,
            n_sect: 0,
        }
    }

    /// The header of a __LINKEDIT table, which no section header
    /// describes.
    pub fn linkedit() -> ChunkHeader {
        let mut hdr = ChunkHeader::new("__LINKEDIT", "");
        hdr.is_sect = false;
        hdr
    }

    pub fn is_zerofill(&self) -> bool {
        matches!(self.flags & SECTION_TYPE, S_ZEROFILL | S_THREAD_LOCAL_ZEROFILL)
    }
}

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

/// Names a chunk of the output. Every kind but the output sections and
/// the -sectcreate sections exists at most once, so the kind alone
/// names it; `Context::chunk_header` reaches any chunk's header, and
/// the typed Context field its data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChunkId {
    /// The mach header, load commands and header padding.
    MachHeader,
    /// A section of the output image, concatenating input sections.
    Output(OutputSectionId),
    Stubs,
    StubHelper,
    LazyPtrs,
    Got,
    ThreadPtrs,
    ObjcStubs,
    ObjcMethlist,
    ObjcImageInfo,
    /// A section created from a file by -sectcreate (or empty, for
    /// -add_empty_section): an index into `Context::sectcreate_sections`.
    SectCreate(u32),
    InitOffsets,
    UnwindInfo,
    EhFrame,
    RebaseInfo,
    BindInfo,
    WeakBindInfo,
    LazyBindInfo,
    ChainedFixups,
    ExportTrie,
    FunctionStarts,
    DataInCode,
    IndirectSymtab,
    Symtab,
    Strtab,
    /// Must be the last chunk in the file.
    CodeSignature,
}

impl ChunkId {
    /// The chunks that exist at most once, in the order `pack` numbers
    /// them.
    const UNITS: [ChunkId; 24] = [
        ChunkId::MachHeader,
        ChunkId::Stubs,
        ChunkId::StubHelper,
        ChunkId::LazyPtrs,
        ChunkId::Got,
        ChunkId::ThreadPtrs,
        ChunkId::ObjcStubs,
        ChunkId::ObjcMethlist,
        ChunkId::ObjcImageInfo,
        ChunkId::InitOffsets,
        ChunkId::UnwindInfo,
        ChunkId::EhFrame,
        ChunkId::RebaseInfo,
        ChunkId::BindInfo,
        ChunkId::WeakBindInfo,
        ChunkId::LazyBindInfo,
        ChunkId::ChainedFixups,
        ChunkId::ExportTrie,
        ChunkId::FunctionStarts,
        ChunkId::DataInCode,
        ChunkId::IndirectSymtab,
        ChunkId::Symtab,
        ChunkId::Strtab,
        ChunkId::CodeSignature,
    ];

    /// The id as one u32 (never u32::MAX), for InputSection, whose
    /// size counts: the top two bits say which of the three shapes it
    /// is, the rest holds the index. mold-rust's InputSection stores an
    /// Option<OutputSectionId>, a word too, since an ELF subsection
    /// only ever lands in an output section; a Mach-O subsection may
    /// also stand for a GOT slot or a rewritten method list.
    pub fn pack(self) -> u32 {
        match self {
            ChunkId::Output(id) => {
                let i = id.index() as u32;
                assert!(i < 1 << 30, "too many output sections");
                i
            }
            ChunkId::SectCreate(i) => (1 << 30) | i,
            _ => (2 << 30) | ChunkId::UNITS.iter().position(|&c| c == self).unwrap() as u32,
        }
    }

    #[inline]
    pub fn unpack(v: u32) -> ChunkId {
        let i = v & ((1 << 30) - 1);
        match v >> 30 {
            0 => ChunkId::Output(OutputSectionId::new(i)),
            1 => ChunkId::SectCreate(i),
            _ => ChunkId::UNITS[i as usize],
        }
    }
}

/// The mach header, load commands and header padding: the first
/// chunk of __TEXT.
#[derive(Debug)]
pub struct OutputMachHeader {
    pub hdr: ChunkHeader,
}

impl OutputMachHeader {
    pub fn new() -> OutputMachHeader {
        let mut hdr = ChunkHeader::new("__TEXT", "");
        hdr.is_sect = false;
        OutputMachHeader { hdr }
    }
}

/// A segment of the output file, grouping chunks.
#[derive(Debug, Default)]
pub struct OutputSegment {
    pub name: &'static str,
    pub chunks: Vec<ChunkId>,
    pub cmd: SegmentCommand,
}

impl OutputSegment {
    pub fn new(name: &'static str) -> OutputSegment {
        OutputSegment { name, chunks: Vec::new(), cmd: SegmentCommand::default() }
    }
}

/// Returns the maxprot/initprot for a well-known segment name.
pub fn segment_prot(name: &str) -> u32 {
    match name {
        "__PAGEZERO" => 0,
        "__TEXT" => VM_PROT_READ | VM_PROT_EXECUTE,
        "__LINKEDIT" => VM_PROT_READ,
        _ => VM_PROT_READ | VM_PROT_WRITE,
    }
}

/// Writes a chunk's bytes into its own slice of the output. The mach
/// header, the symbol and string tables and the code signature are
/// written serially after the parallel copy (see copy_chunks), so they
/// have nothing to do here.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, id: ChunkId, buf: &mut [u8]) {
    match id {
        ChunkId::MachHeader | ChunkId::Symtab | ChunkId::Strtab | ChunkId::CodeSignature => {}
        ChunkId::Output(id) => output_section::copy_buf(ctx, id, buf),
        ChunkId::Stubs => got::stubs::copy_buf(ctx, buf),
        ChunkId::StubHelper => got::stub_helper::copy_buf(ctx, buf),
        ChunkId::LazyPtrs => got::lazy_ptrs::copy_buf(ctx, buf),
        ChunkId::Got => got::got::copy_buf(ctx, buf),
        ChunkId::ThreadPtrs => got::thread_ptrs::copy_buf(ctx, buf),
        ChunkId::ObjcStubs => objc::objc_stubs::copy_buf(ctx, buf),
        ChunkId::ObjcMethlist => objc::objc_methlist::copy_buf(ctx, buf),
        ChunkId::ObjcImageInfo => objc::objc_imageinfo::copy_buf(ctx, buf),
        ChunkId::SectCreate(i) => misc::sectcreate::copy_buf(ctx, i, buf),
        ChunkId::InitOffsets => misc::init_offsets::copy_buf(ctx, buf),
        ChunkId::UnwindInfo => unwind_info::copy_buf(ctx, buf),
        ChunkId::EhFrame => eh_frame::copy_buf(ctx, buf),
        ChunkId::RebaseInfo => dyld_info::rebase_info::copy_buf(ctx, buf),
        ChunkId::BindInfo => dyld_info::bind_info::copy_buf(ctx, buf),
        ChunkId::WeakBindInfo => dyld_info::weak_bind_info::copy_buf(ctx, buf),
        ChunkId::LazyBindInfo => dyld_info::lazy_bind_info::copy_buf(ctx, buf),
        ChunkId::ChainedFixups => chained_fixups::copy_buf(ctx, buf),
        ChunkId::ExportTrie => export_trie::copy_buf(ctx, buf),
        ChunkId::FunctionStarts => misc::function_starts::copy_buf(ctx, buf),
        ChunkId::DataInCode => misc::data_in_code::copy_buf(ctx, buf),
        ChunkId::IndirectSymtab => symtab::indirect_symtab::copy_buf(ctx, buf),
    }
}

fn to_vec(record: &impl FileRecord) -> Vec<u8> {
    record.as_bytes().to_vec()
}

/// Appends a NUL-terminated string, padding the command to 8 bytes.
fn append_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
}

fn create_segment_cmd<E: Arch>(ctx: &Context<E>, seg: &OutputSegment) -> Vec<u8> {
    let mut cmd = seg.cmd;
    cmd.cmd = LC_SEGMENT_64;
    cmd.segname = str_to_name(seg.name);

    let sects: Vec<&ChunkHeader> =
        seg.chunks.iter().map(|&id| ctx.chunk_header(id)).filter(|hdr| hdr.is_sect).collect();

    cmd.nsects = sects.len() as u32;
    cmd.cmdsize = (size_of::<SegmentCommand>() + sects.len() * size_of::<MachSection>()) as u32;
    cmd.maxprot = segment_prot(seg.name);
    cmd.initprot = segment_prot(seg.name);
    // dyld makes __DATA_CONST read-only once binds are applied.
    if seg.name == "__DATA_CONST" {
        cmd.flags = SG_READ_ONLY;
    }

    let mut buf = to_vec(&cmd);
    for hdr in sects {
        let mut sect = MachSection {
            sectname: str_to_name(&hdr.sectname),
            segname: str_to_name(seg.name),
            addr: hdr.addr,
            size: hdr.size,
            offset: hdr.fileoff as u32,
            p2align: hdr.p2align,
            flags: hdr.flags,
            reserved1: hdr.reserved1,
            reserved2: hdr.reserved2,
            ..Default::default()
        };
        if hdr.is_zerofill() {
            sect.offset = 0;
        }
        buf.extend_from_slice(sect.as_bytes());
    }
    buf
}

fn create_dyld_info_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let mut cmd = DyldInfoCommand {
        cmd: LC_DYLD_INFO_ONLY,
        cmdsize: size_of::<DyldInfoCommand>() as u32,
        ..Default::default()
    };
    let hdr = &ctx.rebase_info.hdr;
    if hdr.size > 0 {
        cmd.rebase_off = hdr.fileoff as u32;
        cmd.rebase_size = hdr.size as u32;
    }
    let hdr = &ctx.bind_info.hdr;
    if hdr.size > 0 {
        cmd.bind_off = hdr.fileoff as u32;
        cmd.bind_size = hdr.size as u32;
    }
    let hdr = &ctx.weak_bind_info.hdr;
    if hdr.size > 0 {
        cmd.weak_bind_off = hdr.fileoff as u32;
        cmd.weak_bind_size = hdr.size as u32;
    }
    let hdr = &ctx.lazy_bind_info.hdr;
    if hdr.size > 0 {
        cmd.lazy_bind_off = hdr.fileoff as u32;
        cmd.lazy_bind_size = hdr.size as u32;
    }
    let hdr = &ctx.export_trie.hdr;
    if hdr.size > 0 {
        cmd.export_off = hdr.fileoff as u32;
        cmd.export_size = hdr.size as u32;
    }
    to_vec(&cmd)
}

fn create_symtab_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let cmd = SymtabCommand {
        cmd: LC_SYMTAB,
        cmdsize: size_of::<SymtabCommand>() as u32,
        symoff: ctx.symtab.hdr.fileoff as u32,
        nsyms: ctx.symtab.entries.len() as u32,
        stroff: ctx.strtab.hdr.fileoff as u32,
        strsize: ctx.strtab.hdr.size as u32,
    };
    to_vec(&cmd)
}

fn create_dysymtab_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let data = &ctx.symtab;
    let mut cmd = DysymtabCommand {
        cmd: LC_DYSYMTAB,
        cmdsize: size_of::<DysymtabCommand>() as u32,
        ilocalsym: 0,
        nlocalsym: data.nlocal,
        iextdefsym: data.nlocal,
        nextdefsym: data.nextdef,
        iundefsym: data.nlocal + data.nextdef,
        nundefsym: data.nundef,
        ..Default::default()
    };
    if ctx.chunks.contains(&ChunkId::IndirectSymtab) {
        cmd.indirectsymoff = ctx.indirect_symtab.hdr.fileoff as u32;
        cmd.nindirectsyms = (ctx.indirect_symtab.hdr.size / 4) as u32;
    }
    to_vec(&cmd)
}

fn create_function_starts_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    create_linkedit_data_cmd(LC_FUNCTION_STARTS, &ctx.function_starts.hdr)
}

fn create_uuid_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let cmd = UuidCommand {
        cmd: LC_UUID,
        cmdsize: size_of::<UuidCommand>() as u32,
        uuid: *ctx.uuid.lock().unwrap(),
    };
    to_vec(&cmd)
}

fn create_build_version_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let cmd = BuildVersionCommand {
        cmd: LC_BUILD_VERSION,
        cmdsize: (size_of::<BuildVersionCommand>() + 8) as u32,
        platform: ctx.args.platform,
        minos: ctx.args.platform_minos,
        sdk: ctx.args.platform_sdk,
        ntools: 1,
    };
    let mut buf = to_vec(&cmd);
    // A build_tool_version entry stamping which linker made the
    // image: {u32 tool, u32 version}. Apple's tools are 1..3
    // (clang/swift/ld); this linker identifies itself with sold's
    // number, 54321, so "otool -l | grep 'tool 54321'" spots our
    // output.
    buf.extend_from_slice(&54321u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf
}

fn create_source_version_cmd<E: Arch>(_ctx: &Context<E>) -> Vec<u8> {
    let cmd = SourceVersionCommand {
        cmd: LC_SOURCE_VERSION,
        cmdsize: size_of::<SourceVersionCommand>() as u32,
        version: 0,
    };
    to_vec(&cmd)
}

fn create_load_dylib_cmd(dylib: &crate::macho::input_files::DylibFile) -> Vec<u8> {
    let cmd = DylibCommand {
        cmd: if dylib.is_reexported {
            LC_REEXPORT_DYLIB
        } else if dylib.is_weak {
            LC_LOAD_WEAK_DYLIB
        } else {
            LC_LOAD_DYLIB
        },
        cmdsize: 0,
        nameoff: size_of::<DylibCommand>() as u32,
        timestamp: 2,
        current_version: dylib.current_version,
        compatibility_version: dylib.compatibility_version,
    };
    let mut buf = to_vec(&cmd);
    append_string(&mut buf, &dylib.install_name);
    let size = buf.len() as u32;
    buf[4..8].copy_from_slice(&size.to_le_bytes());
    buf
}

fn create_dylinker_cmd() -> Vec<u8> {
    let cmd = DylinkerCommand {
        cmd: LC_LOAD_DYLINKER,
        cmdsize: 0,
        nameoff: size_of::<DylinkerCommand>() as u32,
    };
    let mut buf = to_vec(&cmd);
    append_string(&mut buf, "/usr/lib/dyld");
    let size = buf.len() as u32;
    buf[4..8].copy_from_slice(&size.to_le_bytes());
    buf
}

fn create_id_dylib_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let name = ctx
        .args
        .install_name
        .as_deref()
        .or(ctx.args.final_output.as_deref())
        .unwrap_or(&ctx.args.output);
    let cmd = DylibCommand {
        cmd: LC_ID_DYLIB,
        cmdsize: 0,
        nameoff: size_of::<DylibCommand>() as u32,
        timestamp: 0,
        current_version: ctx.args.current_version,
        compatibility_version: ctx.args.compatibility_version,
    };
    let mut buf = to_vec(&cmd);
    append_string(&mut buf, name);
    let size = buf.len() as u32;
    buf[4..8].copy_from_slice(&size.to_le_bytes());
    buf
}

// LC_RPATH and LC_SUB_FRAMEWORK share the layout of every
// single-string load command: a cmd/cmdsize header plus the offset of
// an inline NUL-terminated string, padded to an 8-byte multiple.
fn create_string_cmd(kind: u32, path: &str) -> Vec<u8> {
    let cmd =
        DylinkerCommand { cmd: kind, cmdsize: 0, nameoff: size_of::<DylinkerCommand>() as u32 };
    let mut buf = to_vec(&cmd);
    append_string(&mut buf, path);
    let size = buf.len() as u32;
    buf[4..8].copy_from_slice(&size.to_le_bytes());
    buf
}

fn create_main_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    // The entry point is a file offset into __TEXT, whose file offset
    // is zero.
    let text = ctx.segments.iter().find(|s| s.name == "__TEXT").unwrap();
    let cmd = EntryPointCommand {
        cmd: LC_MAIN,
        cmdsize: size_of::<EntryPointCommand>() as u32,
        entryoff: ctx.entry_addr - text.cmd.vmaddr,
        stacksize: ctx.args.stack_size,
    };
    to_vec(&cmd)
}

fn create_code_signature_cmd<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    create_linkedit_data_cmd(LC_CODE_SIGNATURE, &ctx.code_signature.hdr)
}

fn create_linkedit_data_cmd(cmd: u32, hdr: &ChunkHeader) -> Vec<u8> {
    let cmd = LinkEditDataCommand {
        cmd,
        cmdsize: size_of::<LinkEditDataCommand>() as u32,
        dataoff: hdr.fileoff as u32,
        datasize: hdr.size as u32,
    };
    to_vec(&cmd)
}

pub fn create_load_commands<E: Arch>(ctx: &Context<E>) -> Vec<Vec<u8>> {
    // In ld64's order: the segments; a dylib's identity; the dyld
    // tables; the symbol tables; the dynamic linker; identification
    // (UUID, build and source versions); the entry point; the
    // libraries; the run-path list; the code tables (function starts,
    // data-in-code); the signature last.
    let mut vec = Vec::new();

    for seg in &ctx.segments {
        vec.push(create_segment_cmd(ctx, seg));
    }

    if ctx.args.output_type == MH_DYLIB {
        vec.push(create_id_dylib_cmd(ctx));
        if let Some(name) = &ctx.args.umbrella {
            vec.push(create_string_cmd(LC_SUB_FRAMEWORK, name));
        }
        for client in &ctx.args.allowable_clients {
            vec.push(create_string_cmd(LC_SUB_CLIENT, client));
        }
    }

    // Chained fixups replace the classic dyld info; the export trie
    // then gets a load command of its own.
    if ctx.chained_fixups.hdr.size > 0 {
        vec.push(create_linkedit_data_cmd(LC_DYLD_CHAINED_FIXUPS, &ctx.chained_fixups.hdr));
        if ctx.export_trie.hdr.size > 0 {
            vec.push(create_linkedit_data_cmd(LC_DYLD_EXPORTS_TRIE, &ctx.export_trie.hdr));
        }
    } else {
        vec.push(create_dyld_info_cmd(ctx));
    }
    vec.push(create_symtab_cmd(ctx));
    vec.push(create_dysymtab_cmd(ctx));
    if ctx.args.output_type == MH_EXECUTE {
        vec.push(create_dylinker_cmd());
    }
    vec.push(create_uuid_cmd(ctx));
    vec.push(create_build_version_cmd(ctx));
    vec.push(create_source_version_cmd(ctx));
    if ctx.args.output_type == MH_EXECUTE {
        vec.push(create_main_cmd(ctx));
    }

    // Libraries in ordinal order (command-line order, then the
    // auto-linked ones).
    let mut dylibs: Vec<&crate::macho::input_files::DylibFile> =
        ctx.dylibs.iter().filter(|d| !d.is_bundle_loader).collect();
    dylibs.sort_by_key(|d| d.dylib_idx);
    for dylib in dylibs {
        vec.push(create_load_dylib_cmd(dylib));
    }

    for rpath in &ctx.args.rpaths {
        vec.push(create_string_cmd(LC_RPATH, rpath));
    }

    if ctx.function_starts.hdr.size > 0 {
        vec.push(create_function_starts_cmd(ctx));
    }

    // ld64 always writes LC_DATA_IN_CODE, even with no entries;
    // tooling takes its absence as "old linker".
    if ctx.chunks.contains(&ChunkId::DataInCode) {
        vec.push(create_linkedit_data_cmd(LC_DATA_IN_CODE, &ctx.data_in_code.hdr));
    }

    if ctx.chunks.contains(&ChunkId::CodeSignature) {
        vec.push(create_code_signature_cmd(ctx));
    }
    vec
}

/// Returns the size of the mach header chunk: the header, the load
/// commands and the header padding.
pub fn mach_header_size<E: Arch>(ctx: &Context<E>) -> u64 {
    let cmds: usize = create_load_commands(ctx).iter().map(Vec::len).sum();
    size_of::<MachHeader>() as u64 + cmds as u64 + ctx.args.headerpad
}

pub fn copy_mach_header<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let cmds = create_load_commands(ctx);

    let hdr = MachHeader {
        magic: MH_MAGIC_64,
        cputype: E::CPUTYPE,
        cpusubtype: E::CPUSUBTYPE,
        filetype: ctx.args.output_type,
        ncmds: cmds.len() as u32,
        sizeofcmds: cmds.iter().map(Vec::len).sum::<usize>() as u32,
        flags: if ctx.args.flat_namespace {
            MH_NOUNDEFS | MH_DYLDLINK
        } else {
            MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL
        },
        reserved: 0,
    };

    let mut hdr = hdr;
    match ctx.args.output_type {
        MH_EXECUTE => hdr.flags |= MH_PIE,
        MH_DYLIB => {
            if !ctx.dylibs.iter().any(|d| d.is_reexported) {
                hdr.flags |= MH_NO_REEXPORTED_DYLIBS;
            }
            if ctx.args.mark_dead_strippable_dylib {
                hdr.flags |= MH_DEAD_STRIPPABLE_DYLIB;
            }
        }
        _ => {}
    }
    // MH_BINDS_TO_WEAK: the image binds to a symbol some dylib
    // defines weakly, or to one of its own coalescable weak
    // definitions (dyld must then consider weak coalescing when it
    // binds). ld-prime sets it on an executable calling a dylib's
    // weak definition, and on any image with weak-lookup binds.
    if ctx.symbols.syms.iter().any(|sym| match sym.file() {
        Some(FileId::Dylib(idx)) => {
            idx != u32::MAX
                && sym.is_used()
                && ctx.dylibs[idx as usize].weak_exports.contains(sym.name())
        }
        _ => false,
    }) || ctx.chained_fixups.imports.iter().any(|&(id, _)| ctx.binds_weak_lookup(id))
        || !ctx.weak_bind_info.contents.is_empty()
    {
        hdr.flags |= MH_BINDS_TO_WEAK;
    }
    // MH_WEAK_DEFINES advertises exported weak symbols (auto-hidden and
    // private-extern weak definitions don't count, since no other
    // image can coalesce against them) and strong definitions that
    // override a dylib's weak export, which dyld must let win.
    if ctx.symbols.syms.iter().any(|sym| {
        sym.is_weak_def()
            && sym.is_extern()
            && !sym.is_private_extern()
            && sym
                .input_section()
                .map(|i| i as usize)
                .is_some_and(|isec| ctx.isecs[isec].is_alive())
    }) || (0..ctx.symbols.syms.len()).any(|i| ctx.overrides_weak_export(i as u32))
    {
        hdr.flags |= MH_WEAK_DEFINES;
    }
    // -bind_at_load makes the stubs bind through the GOT instead of
    // lazily; ld-prime does not set MH_BINDATLOAD for it (dyld binds
    // everything at load anyway).
    if ctx.args.application_extension {
        hdr.flags |= MH_APP_EXTENSION_SAFE;
    }
    if ctx
        .chunks
        .iter()
        .any(|&id| ctx.chunk_header(id).flags & SECTION_TYPE == S_THREAD_LOCAL_VARIABLES)
    {
        hdr.flags |= MH_HAS_TLV_DESCRIPTORS;
    }
    hdr.write_to(buf);

    let mut off = size_of::<MachHeader>();
    for cmd in &cmds {
        buf[off..off + cmd.len()].copy_from_slice(cmd);
        off += cmd.len();
    }
}
