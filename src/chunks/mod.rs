//! Output chunks: the pieces an output file is assembled from.
//!
//! A chunk is a contiguous byte range of the output: the mach header with
//! its load commands, an output section collecting input sections, or a
//! table in __LINKEDIT. Each kind is a struct of its own holding a
//! ChunkHeader and the data it is written from, reached through the
//! typed fields of Context; a ChunkId names one, and `ctx.chunks` lists
//! the chunks of the output in file order. Segments group chunks for
//! the LC_SEGMENT_64 load commands. mold's chunks module has the
//! same shape.

pub mod bind_info;
pub mod chained_fixups;
pub mod code_signature;
pub mod data_in_code;
pub mod eh_frame;
pub mod export_trie;
pub mod extern_relocs;
pub mod function_starts;
pub mod got;
pub mod indirect_symtab;
pub mod init_offsets;
pub mod lazy_bind_info;
pub mod lazy_ptrs;
pub mod local_relocs;
pub mod objc_imageinfo;
pub mod objc_methlist;
pub mod objc_stubs;
pub mod output_section;
pub mod rebase_info;
pub mod sectcreate;
pub mod split_info;
pub mod strtab;
pub mod stub_helper;
pub mod stubs;
pub mod symtab;
pub mod unwind_info;
pub mod weak_bind_info;

use rayon::prelude::*;
use std::num::NonZeroU32;

use crate::context::Context;
use crate::input_files::FileId;
use crate::macho::*;
use crate::target::Target;

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
    /// section; mold's shndx.
    pub n_sect: u8,
}

impl ChunkHeader {
    /// The header of a section of the image.
    pub fn new(segname: &'static str, sectname: &str) -> Self {
        Self {
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
    pub fn linkedit() -> Self {
        let mut hdr = Self::new("__LINKEDIT", "");
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
    pub fn new(index: u32) -> Self {
        let encoded = index.checked_add(1).expect("too many output sections");
        Self(NonZeroU32::new(encoded).unwrap())
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
    WeakGot,
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
    LocalRelocs,
    ExternRelocs,
    FunctionStarts,
    DataInCode,
    SplitInfo,
    IndirectSymtab,
    Symtab,
    Strtab,
    /// Must be the last chunk in the file.
    CodeSignature,
}

impl ChunkId {
    /// The chunks that exist at most once, in the order `pack` numbers
    /// them.
    const UNITS: [Self; 27] = [
        Self::MachHeader,
        Self::Stubs,
        Self::StubHelper,
        Self::LazyPtrs,
        Self::Got,
        Self::WeakGot,
        Self::ObjcStubs,
        Self::ObjcMethlist,
        Self::ObjcImageInfo,
        Self::InitOffsets,
        Self::UnwindInfo,
        Self::EhFrame,
        Self::RebaseInfo,
        Self::BindInfo,
        Self::WeakBindInfo,
        Self::LazyBindInfo,
        Self::ChainedFixups,
        Self::ExportTrie,
        Self::LocalRelocs,
        Self::ExternRelocs,
        Self::FunctionStarts,
        Self::DataInCode,
        Self::SplitInfo,
        Self::IndirectSymtab,
        Self::Symtab,
        Self::Strtab,
        Self::CodeSignature,
    ];

    /// The id as one u32 (never u32::MAX), for InputSection, whose
    /// size counts: the top two bits say which of the three shapes it
    /// is, the rest holds the index. mold's InputSection stores an
    /// Option<OutputSectionId>, a word too, since an ELF subsection
    /// only ever lands in an output section; a Mach-O subsection may
    /// also stand for a GOT slot or a rewritten method list.
    pub fn pack(self) -> u32 {
        match self {
            Self::Output(id) => {
                let i = id.index() as u32;
                assert!(i < 1 << 30, "too many output sections");
                i
            }
            Self::SectCreate(i) => (1 << 30) | i,
            _ => (2 << 30) | Self::UNITS.iter().position(|&c| c == self).unwrap() as u32,
        }
    }

    #[inline]
    pub fn unpack(v: u32) -> Self {
        let i = v & ((1 << 30) - 1);
        match v >> 30 {
            0 => Self::Output(OutputSectionId::new(i)),
            1 => Self::SectCreate(i),
            _ => Self::UNITS[i as usize],
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
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__TEXT", "");
        hdr.is_sect = false;
        Self { hdr }
    }
}

impl Default for OutputMachHeader {
    fn default() -> Self {
        Self::new()
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
    pub fn new(name: &'static str) -> Self {
        Self { name, chunks: Vec::new(), cmd: SegmentCommand::default() }
    }
}

/// Returns the maxprot/initprot for a well-known segment name.
pub fn segment_prot(name: &str) -> u32 {
    match name {
        "__PAGEZERO" => 0,
        "__TEXT" | "__TEXT_EXEC" => VM_PROT_READ | VM_PROT_EXECUTE,
        "__LINKEDIT" => VM_PROT_READ,
        _ => VM_PROT_READ | VM_PROT_WRITE,
    }
}

/// Returns the load-command index of the segment containing `addr`, and
/// the offset within it.
pub fn segment_and_offset<E: Target>(ctx: &Context<E>, addr: u64) -> (usize, u64) {
    for (i, seg) in ctx.segments.iter().enumerate() {
        if seg.cmd.vmaddr <= addr
            && addr < seg.cmd.vmaddr + seg.cmd.vmsize
            && seg.name != "__PAGEZERO"
        {
            return (i, addr - seg.cmd.vmaddr);
        }
    }
    unreachable!("no segment contains address {addr:#x}");
}

/// Writes a chunk's bytes into its own slice of the output. The mach
/// header, the symbol and string tables, the relocations (the local
/// ones read the pointers they describe, the external ones the symbol
/// indices) and the code signature are written
/// serially after the parallel copy (see copy_chunks), so they have
/// nothing to do here.
pub fn copy_buf<E: Target>(ctx: &Context<E>, id: ChunkId, buf: &mut [u8]) {
    match id {
        ChunkId::MachHeader
        | ChunkId::Symtab
        | ChunkId::Strtab
        | ChunkId::LocalRelocs
        | ChunkId::ExternRelocs
        | ChunkId::CodeSignature => {}
        ChunkId::Output(id) => output_section::copy_buf(ctx, id, buf),
        ChunkId::Stubs => stubs::copy_buf(ctx, buf),
        ChunkId::StubHelper => stub_helper::copy_buf(ctx, buf),
        ChunkId::LazyPtrs => lazy_ptrs::copy_buf(ctx, buf),
        ChunkId::Got => got::copy_buf(ctx, false, buf),
        ChunkId::WeakGot => got::copy_buf(ctx, true, buf),
        ChunkId::ObjcStubs => objc_stubs::copy_buf(ctx, buf),
        ChunkId::ObjcMethlist => objc_methlist::copy_buf(ctx, buf),
        ChunkId::ObjcImageInfo => objc_imageinfo::copy_buf(ctx, buf),
        ChunkId::SectCreate(i) => sectcreate::copy_buf(ctx, i, buf),
        ChunkId::InitOffsets => init_offsets::copy_buf(ctx, buf),
        ChunkId::UnwindInfo => unwind_info::copy_buf(ctx, buf),
        ChunkId::EhFrame => eh_frame::copy_buf(ctx, buf),
        ChunkId::RebaseInfo => rebase_info::copy_buf(ctx, buf),
        ChunkId::BindInfo => bind_info::copy_buf(ctx, buf),
        ChunkId::WeakBindInfo => weak_bind_info::copy_buf(ctx, buf),
        ChunkId::LazyBindInfo => lazy_bind_info::copy_buf(ctx, buf),
        ChunkId::ChainedFixups => chained_fixups::copy_buf(ctx, buf),
        ChunkId::ExportTrie => export_trie::copy_buf(ctx, buf),
        ChunkId::FunctionStarts => function_starts::copy_buf(ctx, buf),
        ChunkId::DataInCode => data_in_code::copy_buf(ctx, buf),
        ChunkId::SplitInfo => split_info::copy_buf(ctx, buf),
        ChunkId::IndirectSymtab => indirect_symtab::copy_buf(ctx, buf),
    }
}

fn to_vec(record: &impl FileRecord) -> Vec<u8> {
    record.as_bytes().to_vec()
}

/// Appends a NUL-terminated string, padding the command to 8 bytes.
fn append_string(buf: &mut Vec<u8>, s: &[u8]) {
    buf.extend_from_slice(s);
    buf.push(0);
    while !buf.len().is_multiple_of(8) {
        buf.push(0);
    }
}

/// A segment's maxprot and initprot in the output.
pub fn segment_prots<E: Target>(ctx: &Context<E>, name: &str) -> (u32, u32) {
    // -segprot overrides the defaults.
    if let Some(&(_, max, init)) = ctx.args.segprots.iter().find(|(seg, _, _)| seg == name) {
        return (u32::from(max), u32::from(init));
    }
    // With __TEXT_EXEC, __TEXT holds no code.
    if name == "__TEXT" && ctx.args.text_exec {
        return (VM_PROT_READ, VM_PROT_READ);
    }
    let prot = segment_prot(name);
    (prot, prot)
}

fn create_segment_cmd<E: Target>(ctx: &Context<E>, seg: &OutputSegment) -> Vec<u8> {
    let mut cmd = seg.cmd;
    cmd.cmd = LC_SEGMENT_64;
    cmd.segname = str_to_name(seg.name);

    let sects: Vec<&ChunkHeader> =
        seg.chunks.iter().map(|&id| ctx.chunk_header(id)).filter(|hdr| hdr.is_sect).collect();

    cmd.nsects = sects.len() as u32;
    cmd.cmdsize = (size_of::<SegmentCommand>() + sects.len() * size_of::<MachSection>()) as u32;
    (cmd.maxprot, cmd.initprot) = segment_prots(ctx, seg.name);
    // dyld makes __DATA_CONST read-only once binds are applied; not in
    // an image bound for the shared region, which ld-prime leaves to
    // the cache (or kernel collection) builder.
    if seg.name == "__DATA_CONST" && !ctx.args.shared_region {
        cmd.flags = SG_READ_ONLY;
    }
    // A segment of nothing but sections the command line made
    // (-sectcreate contents or -add_empty_section anchors) has nothing
    // to relocate; ld-prime flags it SG_NORELOC. Not so an empty
    // section a `section$start$` boundary symbol conjured, any input
    // section, or a segment of no sections at all such as __PAGEZERO.
    if !seg.chunks.is_empty()
        && seg.chunks.iter().all(|&id| match id {
            ChunkId::SectCreate(i) => ctx.sectcreate_sections[i as usize].from_option,
            _ => false,
        })
    {
        cmd.flags |= SG_NORELOC;
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

fn create_dyld_info_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
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

fn create_symtab_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let cmd = SymtabCommand {
        cmd: LC_SYMTAB,
        cmdsize: size_of::<SymtabCommand>() as u32,
        symoff: ctx.symtab.hdr.fileoff as u32,
        nsyms: ctx.symtab.len() as u32,
        stroff: ctx.strtab.hdr.fileoff as u32,
        strsize: ctx.strtab.hdr.size as u32,
    };
    to_vec(&cmd)
}

fn create_dysymtab_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
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
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        cmd.locreloff = ctx.local_relocs.hdr.fileoff as u32;
        cmd.nlocrel = ctx.local_relocs.locs.len() as u32;
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) {
        cmd.extreloff = ctx.extern_relocs.hdr.fileoff as u32;
        cmd.nextrel = ctx.extern_relocs.relocs.len() as u32;
    }
    to_vec(&cmd)
}

fn create_function_starts_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    create_linkedit_data_cmd(LC_FUNCTION_STARTS, &ctx.function_starts.hdr)
}

fn create_uuid_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let cmd = UuidCommand {
        cmd: LC_UUID,
        cmdsize: size_of::<UuidCommand>() as u32,
        uuid: *ctx.uuid.lock().unwrap(),
    };
    to_vec(&cmd)
}

/// Whether the output records its platform in a load command. ld-prime
/// leaves the command out of an image no dyld loads and out of
/// firmware of any kind, -r output included, unless
/// -version_load_command asks for it, and out of a -r output for no
/// platform.
pub fn has_version_cmd(args: &crate::cmdline::Args) -> bool {
    let without_dyld = args.without_dyld() && !args.relocatable;
    args.platform != 0
        && (args.version_load_command || (!without_dyld && args.platform != PLATFORM_FIRMWARE))
}

/// The load command naming the deployment target, for a final image
/// and for -r alike. LC_BUILD_VERSION came with macOS 10.14; for an
/// x86-64 target older than that, ld-prime writes the legacy
/// LC_VERSION_MIN_MACOSX {version, sdk} that its loader reads (arm64
/// macOS gets LC_BUILD_VERSION at any version).
pub fn create_version_cmd<E: Target>(platform: u32, minos: u32, sdk: u32) -> Vec<u8> {
    if E::CPUTYPE != CPU_TYPE_ARM64
        && platform == PLATFORM_MACOS
        && minos != 0
        && minos < encode_version(10, 14, 0)
    {
        return to_vec(&VersionMinCommand {
            cmd: LC_VERSION_MIN_MACOSX,
            cmdsize: size_of::<VersionMinCommand>() as u32,
            version: minos,
            sdk,
        });
    }
    let cmd = BuildVersionCommand {
        cmd: LC_BUILD_VERSION,
        cmdsize: (size_of::<BuildVersionCommand>() + 8) as u32,
        platform,
        minos,
        sdk,
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

fn create_source_version_cmd<E: Target>(_ctx: &Context<E>) -> Vec<u8> {
    let cmd = SourceVersionCommand {
        cmd: LC_SOURCE_VERSION,
        cmdsize: size_of::<SourceVersionCommand>() as u32,
        version: 0,
    };
    to_vec(&cmd)
}

fn create_load_dylib_cmd(dylib: &crate::input_files::DylibFile) -> Vec<u8> {
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
    append_string(&mut buf, b"/usr/lib/dyld");
    let size = buf.len() as u32;
    buf[4..8].copy_from_slice(&size.to_le_bytes());
    buf
}

/// The install name a dylib output records in LC_ID_DYLIB: -install_name,
/// else -final_output, else the output path.
pub fn output_install_name<E: Target>(ctx: &Context<E>) -> &[u8] {
    ctx.args.output_install_name()
}

fn create_id_dylib_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let name = output_install_name(ctx);
    let cmd = DylibCommand {
        cmd: LC_ID_DYLIB,
        cmdsize: 0,
        nameoff: size_of::<DylibCommand>() as u32,
        // The build time once, which prebinding compared with the
        // one a client recorded; nothing reads it now, and ld-prime
        // writes 1 here and 2 in the clients' load commands.
        timestamp: 1,
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
fn create_string_cmd(kind: u32, path: &[u8]) -> Vec<u8> {
    let cmd =
        DylinkerCommand { cmd: kind, cmdsize: 0, nameoff: size_of::<DylinkerCommand>() as u32 };
    let mut buf = to_vec(&cmd);
    append_string(&mut buf, path);
    let size = buf.len() as u32;
    buf[4..8].copy_from_slice(&size.to_le_bytes());
    buf
}

fn create_main_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
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

/// A -static image has no dyld to read LC_MAIN; the kernel (or a boot
/// loader) starts its thread from LC_UNIXTHREAD's register state, all
/// zero but the program counter at the entry point.
fn create_unixthread_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let size = 16 + E::THREAD_STATE_COUNT as usize * 4;
    let mut buf = Vec::with_capacity(size);
    for word in [LC_UNIXTHREAD, size as u32, E::THREAD_STATE_FLAVOR, E::THREAD_STATE_COUNT] {
        buf.extend_from_slice(&word.to_le_bytes());
    }
    buf.resize(size, 0);
    let pc = 16 + E::THREAD_STATE_PC_OFFSET;
    buf[pc..pc + 8].copy_from_slice(&ctx.entry_addr.to_le_bytes());
    buf
}

fn create_code_signature_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
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

pub fn create_load_commands<E: Target>(ctx: &Context<E>) -> Vec<Vec<u8>> {
    // In ld64's order: the segments; a dylib's identity; the dyld
    // tables; the symbol tables; the dynamic linker; identification
    // (UUID, build and source versions); the entry point; the split
    // info; the libraries; the run-path list; the code tables
    // (function starts, data-in-code); the signature last.
    let mut vec = Vec::new();

    // A -preload image's __LINKEDIT is no segment: its tables follow
    // the segments in the file, and nothing maps them.
    for seg in &ctx.segments {
        if !(ctx.args.preload && seg.name == "__LINKEDIT") {
            vec.push(create_segment_cmd(ctx, seg));
        }
    }

    if ctx.args.output_type == MH_DYLIB {
        vec.push(create_id_dylib_cmd(ctx));
    }

    // Chained fixups replace the classic dyld info; the export trie
    // then gets a load command of its own. A -static image has no dyld
    // to read either: it exports nothing, has only the fixups
    // -fixup_chains or -no_fixup_chains asks for, and has a dynamic
    // symbol table only under -pie, for the local relocations that
    // slide it without them (with them, it lists none). A kext has
    // only its relocations.
    // (Decided by the options, not by the tables' sizes: the layout
    // sizes the header before the tables exist.)
    // (A -preload image under -fixup_chains gets its chains, and their
    // table after the segments, but ld-prime writes no command naming
    // the table: its loader must know where to find it.)
    if ctx.use_chained_fixups() {
        if !ctx.args.preload {
            vec.push(create_linkedit_data_cmd(LC_DYLD_CHAINED_FIXUPS, &ctx.chained_fixups.hdr));
        }
        // Present even with nothing exported (an 8-byte empty trie),
        // as ld-prime writes it.
        if !ctx.args.without_dyld() {
            vec.push(create_linkedit_data_cmd(LC_DYLD_EXPORTS_TRIE, &ctx.export_trie.hdr));
        }
    } else if !ctx.args.without_dyld() || ctx.args.fixup_chains == Some(false) {
        vec.push(create_dyld_info_cmd(ctx));
    }
    vec.push(create_symtab_cmd(ctx));
    if !ctx.args.static_link || ctx.args.pie {
        vec.push(create_dysymtab_cmd(ctx));
    }
    if ctx.args.output_type == MH_EXECUTE && !ctx.args.static_link {
        vec.push(create_dylinker_cmd());
    }
    // -no_uuid leaves the command out, as ld-prime does, though dyld
    // then refuses to load the image ("missing LC_UUID load command").
    if ctx.args.uuid {
        vec.push(create_uuid_cmd(ctx));
    }
    if has_version_cmd(&ctx.args) {
        vec.push(create_version_cmd::<E>(
            ctx.args.platform,
            ctx.args.platform_minos,
            ctx.args.platform_sdk,
        ));
    }
    if !ctx.args.preload {
        vec.push(create_source_version_cmd(ctx));
    }
    if ctx.args.output_type == MH_EXECUTE {
        vec.push(if ctx.args.static_link {
            create_unixthread_cmd(ctx)
        } else {
            create_main_cmd(ctx)
        });
    }
    if ctx.chunks.contains(&ChunkId::SplitInfo) {
        vec.push(create_linkedit_data_cmd(LC_SEGMENT_SPLIT_INFO, &ctx.split_info.hdr));
    }

    // Libraries in ordinal order (command-line order, then the
    // auto-linked ones).
    let mut dylibs: Vec<&crate::input_files::DylibFile> =
        ctx.dylibs.iter().filter(|d| !d.is_bundle_loader).collect();
    dylibs.sort_by_key(|d| d.dylib_idx);
    for dylib in dylibs {
        vec.push(create_load_dylib_cmd(dylib));
    }

    for rpath in &ctx.args.rpaths {
        vec.push(create_string_cmd(LC_RPATH, rpath));
    }

    // The umbrella commands follow the libraries and the rpaths,
    // clients first, as ld-prime orders them.
    if ctx.args.output_type == MH_DYLIB {
        for client in &ctx.args.allowable_clients {
            vec.push(create_string_cmd(LC_SUB_CLIENT, client));
        }
        if let Some(name) = &ctx.args.umbrella {
            vec.push(create_string_cmd(LC_SUB_FRAMEWORK, name));
        }
    }

    // Also present with no functions at all (an 8-byte empty table),
    // as ld-prime writes it; -no_function_starts drops it.
    if ctx.args.function_starts {
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
pub fn mach_header_size<E: Target>(ctx: &Context<E>) -> u64 {
    let cmds = create_load_commands(ctx);
    let size: usize = cmds.iter().map(Vec::len).sum();
    size_of::<MachHeader>() as u64 + size as u64 + header_pad(ctx, &cmds)
}

/// The free space ld-prime leaves after a final image's load commands:
/// -headerpad, at least 32 bytes, or with -headerpad_max_install_names
/// room for each dylib command to grow by MAXPATHLEN. ld-prime places
/// the sections after an estimate of the load commands, not their
/// final size, so the space grows by the estimate's excess too: it
/// counts dylib_use_command's 28-byte header for each dependency, and
/// LC_DYLD_INFO_ONLY plus LC_DYLD_EXPORTS_TRIE unless the image is an
/// arm64 one with chained fixups (a -static image: 32 bytes).
fn header_pad<E: Target>(ctx: &Context<E>, cmds: &[Vec<u8>]) -> u64 {
    // A -preload image's header has pages of its own, ahead of the
    // segments, and ld-prime leaves nothing free after its commands.
    if ctx.args.preload {
        return 0;
    }
    let dylib_cmds: Vec<(DylibCommand, &[u8])> = cmds
        .iter()
        .filter(|c| {
            matches!(
                LoadCommand::read_from(c).cmd,
                LC_ID_DYLIB | LC_LOAD_DYLIB | LC_LOAD_WEAK_DYLIB | LC_REEXPORT_DYLIB
            )
        })
        .map(|c| (DylibCommand::read_from(c), c.as_slice()))
        .collect();

    let mut pad = ctx.args.headerpad.max(32);
    if ctx.args.headerpad_max_install_names {
        pad = pad.max(dylib_cmds.len() as u64 * 1024);
    }

    let mut excess = if ctx.args.static_link {
        32
    } else if ctx.args.is_kext() {
        0
    } else if !ctx.use_chained_fixups() {
        16
    } else if E::CPUTYPE == CPU_TYPE_ARM64 {
        0
    } else {
        32
    };
    for (cmd, bytes) in dylib_cmds {
        if cmd.cmd != LC_ID_DYLIB {
            let name = &bytes[cmd.nameoff as usize..];
            let len = name.iter().position(|&b| b == 0).unwrap_or(name.len()) as u64;
            excess += crate::util::align_to(len + 29, 8) - crate::util::align_to(len + 25, 8);
        }
    }
    pad + excess
}

pub fn copy_mach_header<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let cmds = create_load_commands(ctx);

    let is_static_executable = ctx.args.output_type == MH_EXECUTE && ctx.args.static_link;
    let hdr = MachHeader {
        magic: MH_MAGIC_64,
        cputype: E::CPUTYPE,
        cpusubtype: E::CPUSUBTYPE,
        filetype: if ctx.args.preload { MH_PRELOAD } else { ctx.args.output_type },
        ncmds: cmds.len() as u32,
        sizeofcmds: cmds.iter().map(Vec::len).sum::<usize>() as u32,
        // Under -flat_namespace every import is a flat lookup that
        // dyld resolves at load, so ld64 does not claim MH_NOUNDEFS.
        flags: if is_static_executable {
            MH_NOUNDEFS
        } else if ctx.args.flat_namespace {
            MH_DYLDLINK
        } else {
            MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL
        },
        reserved: 0,
    };

    let mut hdr = hdr;
    match ctx.args.output_type {
        MH_EXECUTE => {
            if ctx.args.pie {
                hdr.flags |= MH_PIE;
            }
        }
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
    if ctx.symbols.syms.par_iter().any(|sym| match sym.file() {
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
    // override a dylib's weak export, which dyld must let win. An
    // exported weak definition also makes the image bind to weak in
    // ld-prime's eyes, referenced from within the image or not (a
    // dylib whose only weak definition nothing calls still gets
    // 0x118085), since another image's copy may replace it.
    if ctx.symbols.syms.par_iter().any(|sym| {
        sym.is_weak_def()
            && sym.is_extern()
            && !sym.is_private_extern()
            && sym
                .input_section()
                .map(|i| i as usize)
                .is_some_and(|isec| ctx.isecs[isec].is_alive())
    }) {
        hdr.flags |= MH_WEAK_DEFINES | MH_BINDS_TO_WEAK;
    }
    if (0..ctx.symbols.syms.len()).into_par_iter().any(|i| ctx.overrides_weak_export(i as u32)) {
        hdr.flags |= MH_WEAK_DEFINES;
    }
    // -bind_at_load makes the stubs bind through the GOT instead of
    // lazily; ld-prime does not set MH_BINDATLOAD for it (dyld binds
    // everything at load anyway). MH_APP_EXTENSION_SAFE is for dyld
    // too, and ld-prime leaves it out of an image no dyld loads.
    if ctx.args.application_extension && !ctx.args.static_link {
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

/// Writes the UUID into the LC_UUID command of a header that
/// `copy_mach_header` already wrote, leaving everything else as it is.
pub fn write_uuid<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let hdr = MachHeader::read_from(buf);
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&buf[off..]);
        if lc.cmd == LC_UUID {
            let mut cmd = UuidCommand::read_from(&buf[off..]);
            cmd.uuid = *ctx.uuid.lock().unwrap();
            cmd.write_to(&mut buf[off..]);
            return;
        }
        off += lc.cmdsize as usize;
    }
}
