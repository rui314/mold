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
pub mod chain_starts;
pub mod chained_fixups;
pub mod code_signature;
pub mod data_in_code;
pub mod delay_init;
pub mod eh_frame;
pub mod export_trie;
pub mod extern_relocs;
pub mod function_starts;
pub mod got;
pub mod indirect_symtab;
pub mod init_offsets;
pub mod lazy_bind_info;
pub mod lazy_helpers;
pub mod lazy_load_got;
pub mod lazy_load_info;
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
use crate::symbol::SymbolId;
use crate::symbol_moves::MoveOption;
use crate::target::Target;

pub use output_section::{OutputSection, Tail, Thunk};

/// A chunk's place in the output: its section header's fields, the
/// names among them bytes, as Mach-O names are (see
/// macho::name_to_bytes).
#[derive(Debug)]
pub struct ChunkHeader {
    pub segname: &'static [u8],
    pub sectname: &'static [u8],
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
    pub fn new(segname: &'static [u8], sectname: &'static [u8]) -> Self {
        Self {
            segname,
            sectname,
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
        let mut hdr = Self::new(b"__LINKEDIT", b"");
        hdr.is_sect = false;
        hdr
    }

    pub fn is_zerofill(&self) -> bool {
        matches!(self.flags & SECTION_TYPE, S_ZEROFILL | S_THREAD_LOCAL_ZEROFILL)
    }

    /// Whether the section is typed as part of the thread-local
    /// template: its initial values or its zero fill.
    pub fn is_thread_local(&self) -> bool {
        matches!(self.flags & SECTION_TYPE, S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL)
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
    DelayStubs,
    DelayHelper,
    LazyHelpers,
    LazyLoadGot,
    ObjcStubs,
    ObjcMethlist,
    ObjcImageInfo,
    /// A section created from a file by -sectcreate (or empty, for
    /// -add_empty_section): an index into `Context::sectcreate_sections`.
    SectCreate(u32),
    InitOffsets,
    ChainStarts,
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
    /// The mergeable record (LC_ATOM_INFO) of a -make_mergeable dylib.
    MergeableRecord,
    SplitInfo,
    LazyLoadInfo,
    IndirectSymtab,
    Symtab,
    Strtab,
    /// Must be the last chunk in the file.
    CodeSignature,
}

impl ChunkId {
    /// The chunks that exist at most once, in the order `pack` numbers
    /// them.
    const UNITS: [Self; 33] = [
        Self::MachHeader,
        Self::Stubs,
        Self::StubHelper,
        Self::LazyPtrs,
        Self::Got,
        Self::DelayStubs,
        Self::DelayHelper,
        Self::LazyHelpers,
        Self::LazyLoadGot,
        Self::ObjcStubs,
        Self::ObjcMethlist,
        Self::ObjcImageInfo,
        Self::InitOffsets,
        Self::ChainStarts,
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
        Self::MergeableRecord,
        Self::SplitInfo,
        Self::LazyLoadInfo,
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
        let mut hdr = ChunkHeader::new(b"__TEXT", b"");
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
    pub name: &'static [u8],
    pub chunks: Vec<ChunkId>,
    pub cmd: SegmentCommand,
}

impl OutputSegment {
    pub fn new(name: &'static [u8]) -> Self {
        Self { name, chunks: Vec::new(), cmd: SegmentCommand::default() }
    }
}

/// Returns the maxprot/initprot for a well-known segment name.
pub fn segment_prot(name: &[u8]) -> u32 {
    match name {
        b"__PAGEZERO" => 0,
        b"__TEXT" | b"__TEXT_EXEC" => VM_PROT_READ | VM_PROT_EXECUTE,
        b"__LINKEDIT" => VM_PROT_READ,
        _ => VM_PROT_READ | VM_PROT_WRITE,
    }
}

/// Returns the load-command index of the segment containing `addr`, and
/// the offset within it.
pub fn segment_and_offset<E: Target>(ctx: &Context<E>, addr: u64) -> (usize, u64) {
    for (i, seg) in ctx.segments.iter().enumerate() {
        if seg.cmd.vmaddr <= addr
            && addr < seg.cmd.vmaddr + seg.cmd.vmsize
            && seg.name != b"__PAGEZERO"
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
        ChunkId::Got => got::copy_buf(ctx, buf),
        ChunkId::DelayStubs => delay_init::copy_stubs(ctx, buf),
        ChunkId::DelayHelper => delay_init::copy_helper(ctx, buf),
        ChunkId::LazyHelpers => lazy_helpers::copy_buf(ctx, buf),
        ChunkId::LazyLoadGot => lazy_load_got::copy_buf(ctx, buf),
        ChunkId::ObjcStubs => objc_stubs::copy_buf(ctx, buf),
        ChunkId::ObjcMethlist => objc_methlist::copy_buf(ctx, buf),
        ChunkId::ObjcImageInfo => objc_imageinfo::copy_buf(ctx, buf),
        ChunkId::SectCreate(i) => copy_contents(ctx.sectcreate_sections[i as usize].contents, buf),
        ChunkId::InitOffsets => init_offsets::copy_buf(ctx, buf),
        ChunkId::ChainStarts => chain_starts::copy_buf(ctx, buf),
        ChunkId::UnwindInfo => unwind_info::copy_buf(ctx, buf),
        ChunkId::EhFrame => eh_frame::copy_buf(ctx, buf),
        ChunkId::RebaseInfo => copy_contents(&ctx.rebase_info.contents, buf),
        ChunkId::BindInfo => copy_contents(&ctx.bind_info.contents, buf),
        ChunkId::WeakBindInfo => copy_contents(&ctx.weak_bind_info.contents, buf),
        ChunkId::LazyBindInfo => copy_contents(&ctx.lazy_bind_info.contents, buf),
        ChunkId::ChainedFixups => copy_contents(&ctx.chained_fixups.contents, buf),
        ChunkId::ExportTrie => copy_contents(&ctx.export_trie.contents, buf),
        ChunkId::FunctionStarts => copy_contents(&ctx.function_starts.contents, buf),
        ChunkId::DataInCode => data_in_code::copy_buf(ctx, buf),
        ChunkId::MergeableRecord => crate::make_mergeable::copy_buf(ctx, buf),
        ChunkId::SplitInfo => copy_contents(&ctx.split_info.contents, buf),
        ChunkId::LazyLoadInfo => lazy_load_info::copy_buf(ctx, buf),
        ChunkId::IndirectSymtab => indirect_symtab::copy_buf(ctx, buf),
    }
}

/// Copies a chunk's contents, built during layout, into its slice of
/// the output.
fn copy_contents(contents: &[u8], buf: &mut [u8]) {
    buf[..contents.len()].copy_from_slice(contents);
}

fn to_vec(record: &impl FileRecord) -> Vec<u8> {
    record.as_bytes().to_vec()
}

/// Ends a load command with its NUL-terminated string, padded to 8
/// bytes, and sets its cmdsize.
fn append_string(mut buf: Vec<u8>, s: &[u8]) -> Vec<u8> {
    buf.extend_from_slice(s);
    buf.push(0);
    buf.resize(buf.len().next_multiple_of(8), 0);
    let size = buf.len() as u32;
    buf[4..8].copy_from_slice(&size.to_le_bytes());
    buf
}

/// A segment's maxprot and initprot in the output.
pub fn segment_prots<E: Target>(ctx: &Context<E>, seg: &OutputSegment) -> (u32, u32) {
    let name = seg.name;
    // -segprot overrides the defaults.
    if let Some(&(_, max, init)) = ctx.args.segprots.iter().find(|(seg, _, _)| seg == name) {
        return (u32::from(max), u32::from(init));
    }
    // With __TEXT_EXEC, __TEXT holds no code.
    if name == b"__TEXT" && ctx.args.text_exec {
        return (VM_PROT_READ, VM_PROT_READ);
    }
    if holds_moved_code(ctx, seg) {
        return (VM_PROT_READ | VM_PROT_EXECUTE, VM_PROT_READ | VM_PROT_EXECUTE);
    }
    let prot = segment_prot(name);
    (prot, prot)
}

/// Whether a segment is one -move_to_ro_segment made for code: all its
/// sections hold what the option moved there, code among it. ld-prime
/// gives such a segment the read-write protection of any other one it
/// doesn't know, where the code it moved can't run (a Bus error); mold
/// deliberately makes it executable - and read-only, as the option's
/// name has it.
fn holds_moved_code<E: Target>(ctx: &Context<E>, seg: &OutputSegment) -> bool {
    let moved_ro = |id: &ChunkId| match id {
        ChunkId::Output(osec) => ctx.output_section(*osec).moved == Some(MoveOption::Ro),
        _ => false,
    };
    let is_code = |id: &ChunkId| ctx.chunk_header(*id).flags & S_ATTR_PURE_INSTRUCTIONS != 0;
    seg.chunks.iter().all(moved_ro) && seg.chunks.iter().any(is_code)
}

fn create_segment_cmd<E: Target>(ctx: &Context<E>, seg: &OutputSegment) -> Vec<u8> {
    let mut cmd = seg.cmd;
    cmd.cmd = LC_SEGMENT_64;
    cmd.segname = bytes_to_name(seg.name);

    let sects: Vec<&ChunkHeader> =
        seg.chunks.iter().map(|&id| ctx.chunk_header(id)).filter(|hdr| hdr.is_sect).collect();

    cmd.nsects = sects.len() as u32;
    cmd.cmdsize = (size_of::<SegmentCommand>() + sects.len() * size_of::<MachSection>()) as u32;
    (cmd.maxprot, cmd.initprot) = segment_prots(ctx, seg);
    // dyld makes __DATA_CONST read-only once binds are applied; not in
    // an image bound for the shared region, which ld-prime leaves to
    // the cache (or kernel collection) builder, but for dyld itself,
    // which makes its own read-only once it has slid itself.
    if seg.name == b"__DATA_CONST" && (!ctx.args.shared_region || ctx.args.is_dylinker()) {
        cmd.flags = SG_READ_ONLY;
    }
    let mut buf = to_vec(&cmd);
    for hdr in sects {
        let mut sect = MachSection {
            sectname: bytes_to_name(hdr.sectname),
            segname: bytes_to_name(seg.name),
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
    // An empty relocation table has offset 0, as in ld-prime's output.
    if ctx.chunks.contains(&ChunkId::LocalRelocs) && !ctx.local_relocs.locs.is_empty() {
        cmd.locreloff = ctx.local_relocs.hdr.fileoff as u32;
        cmd.nlocrel = ctx.local_relocs.locs.len() as u32;
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) && !ctx.extern_relocs.relocs.is_empty() {
        cmd.extreloff = ctx.extern_relocs.hdr.fileoff as u32;
        cmd.nextrel = ctx.extern_relocs.relocs.len() as u32;
    }
    to_vec(&cmd)
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
/// and for -r alike: LC_BUILD_VERSION, or for an x86-64 macOS older than
/// 10.14, which brought LC_BUILD_VERSION, the legacy
/// LC_VERSION_MIN_MACOSX {version, sdk} its loaders read (arm64 macOS
/// gets LC_BUILD_VERSION at any version).
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

fn create_source_version_cmd(version: u64) -> Vec<u8> {
    let cmd = SourceVersionCommand {
        cmd: LC_SOURCE_VERSION,
        cmdsize: size_of::<SourceVersionCommand>() as u32,
        version,
    };
    to_vec(&cmd)
}

fn create_load_dylib_cmd(dylib: &crate::input_files::DylibFile) -> Vec<u8> {
    // A dylib that is two of weak, re-exported and upward, or one whose
    // initializers wait for the image to dlopen() it, takes a
    // dylib_use_command, whose flags say all of it: the header grows by
    // them, and a marker stands in the timestamp. ld-prime writes the
    // compatibility version as 1.0.0, which dyld no longer checks.
    let weak = dylib.is_weak || dylib.is_weak_asserted;
    let flags = [
        (weak, DYLIB_USE_WEAK_LINK),
        (dylib.is_reexported, DYLIB_USE_REEXPORT),
        (dylib.is_upward, DYLIB_USE_UPWARD),
        (dylib.delay_init.is_some(), DYLIB_USE_DELAYED_INIT),
    ]
    .into_iter()
    .filter(|&(on, _)| on)
    .fold(0, |flags, (_, flag)| flags | flag);
    let use_command = flags.count_ones() > 1 || flags & DYLIB_USE_DELAYED_INIT != 0;
    let flags = if use_command { flags } else { 0 };
    let cmd = DylibCommand {
        cmd: if flags & DYLIB_USE_WEAK_LINK != 0 {
            LC_LOAD_WEAK_DYLIB
        } else if flags != 0 {
            LC_LOAD_DYLIB
        } else if dylib.is_reexported {
            LC_REEXPORT_DYLIB
        } else if weak {
            LC_LOAD_WEAK_DYLIB
        } else if dylib.is_upward {
            LC_LOAD_UPWARD_DYLIB
        } else {
            LC_LOAD_DYLIB
        },
        cmdsize: 0,
        nameoff: size_of::<DylibCommand>() as u32,
        timestamp: 2,
        current_version: dylib.current_version,
        compatibility_version: dylib.compatibility_version,
    };
    let cmd = match flags {
        0 => cmd,
        _ => DylibCommand {
            nameoff: cmd.nameoff + 4,
            timestamp: DYLIB_USE_MARKER,
            compatibility_version: encode_version(1, 0, 0),
            ..cmd
        },
    };
    let mut buf = to_vec(&cmd);
    if flags != 0 {
        buf.extend_from_slice(&flags.to_le_bytes());
    }
    append_string(buf, &dylib.install_name)
}

fn create_id_dylib_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
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
    append_string(to_vec(&cmd), ctx.args.output_install_name())
}

// LC_RPATH, LC_SUB_FRAMEWORK and the dylinker commands share the
// layout of every single-string load command: a cmd/cmdsize header
// plus the offset of an inline NUL-terminated string, padded to an
// 8-byte multiple.
fn create_string_cmd(kind: u32, path: &[u8]) -> Vec<u8> {
    let cmd =
        DylinkerCommand { cmd: kind, cmdsize: 0, nameoff: size_of::<DylinkerCommand>() as u32 };
    append_string(to_vec(&cmd), path)
}

fn create_main_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    // The entry point is a file offset into __TEXT, whose file offset
    // is zero. The layout sizes the command before the entry point has
    // an address (0) - also when it lays the segments out again, with
    // __TEXT placed by the first round.
    let text = ctx.segments.iter().find(|s| s.name == b"__TEXT").unwrap();
    let cmd = EntryPointCommand {
        cmd: LC_MAIN,
        cmdsize: size_of::<EntryPointCommand>() as u32,
        entryoff: ctx.entry_addr.saturating_sub(text.cmd.vmaddr),
        stacksize: ctx.args.stack_size,
    };
    to_vec(&cmd)
}

/// LC_ROUTINES_64 names the -init function by its unslid address, and
/// dyld runs it before the image's other initializers. (The layout
/// sizes the command before the function has an address.)
fn create_routines_cmd<E: Target>(ctx: &Context<E>, id: SymbolId) -> Vec<u8> {
    let cmd = RoutinesCommand64 {
        cmd: LC_ROUTINES_64,
        cmdsize: size_of::<RoutinesCommand64>() as u32,
        init_address: ctx.sym_addr(id),
        ..Default::default()
    };
    to_vec(&cmd)
}

/// A -static image has no dyld to read LC_MAIN, nor has dyld itself;
/// the kernel (or a boot loader) starts its thread from LC_UNIXTHREAD's
/// register state, all zero but the program counter at the entry
/// point, and the stack pointer at the top of a -stack_size stack. dyld
/// before macOS 10.8 jumped to an executable's entry point so, once it
/// had loaded the libraries.
fn create_unixthread_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let size = 16 + E::THREAD_STATE_COUNT as usize * 4;
    let mut buf = Vec::with_capacity(size);
    for word in [LC_UNIXTHREAD, size as u32, E::THREAD_STATE_FLAVOR, E::THREAD_STATE_COUNT] {
        buf.extend_from_slice(&word.to_le_bytes());
    }
    buf.resize(size, 0);
    let pc = 16 + E::THREAD_STATE_PC_OFFSET;
    buf[pc..pc + 8].copy_from_slice(&ctx.entry_addr.to_le_bytes());
    if let Some(stack) = ctx.segments.iter().find(|seg| seg.name == b"__UNIXSTACK") {
        let sp = 16 + E::THREAD_STATE_SP_OFFSET;
        buf[sp..sp + 8].copy_from_slice(&(stack.cmd.vmaddr + stack.cmd.vmsize).to_le_bytes());
    }
    buf
}

/// LC_ENCRYPTION_INFO_64: the range of the file an encryptable image's
/// code may be encrypted in, from the first section, which starts a
/// page of its own (see mach_header_size), to the page end of the last
/// __TEXT section but __oslogstring, which the OS logs read
/// unencrypted; and the encryption system, none yet (cryptid 0). (The
/// layout sizes the command before the sections have file offsets.)
fn create_encryption_info_cmd<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let text = |id: &&ChunkId| {
        let hdr = ctx.chunk_header(**id);
        **id != ChunkId::MachHeader && hdr.segname == b"__TEXT" && hdr.sectname != b"__oslogstring"
    };
    let sections = || ctx.chunks.iter().filter(text).map(|&id| ctx.chunk_header(id));
    let start = sections().map(|hdr| hdr.fileoff).min().unwrap_or(0);
    let end = sections().map(|hdr| hdr.fileoff + hdr.size).max().unwrap_or(0);
    let end = crate::util::align_to(end, ctx.args.segment_align);
    let mut buf = Vec::with_capacity(24);
    for word in [LC_ENCRYPTION_INFO_64, 24, start as u32, end.saturating_sub(start) as u32, 0, 0] {
        buf.extend_from_slice(&word.to_le_bytes());
    }
    buf
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

fn create_load_commands<E: Target>(ctx: &Context<E>) -> Vec<Vec<u8>> {
    // In ld64's order: the segments; a dylib's identity; the dyld
    // tables; the symbol tables; the dynamic linker; identification
    // (UUID, build and source versions); the entry point; the split
    // info; the libraries; the run-path list; the code tables
    // (function starts, data-in-code); the signature last.
    let mut vec = Vec::new();

    // A -preload image's __LINKEDIT is no segment: its tables follow
    // the segments in the file, and nothing maps them.
    for seg in &ctx.segments {
        if !(ctx.args.preload && seg.name == b"__LINKEDIT") {
            vec.push(create_segment_cmd(ctx, seg));
        }
    }

    if ctx.args.output_type == MH_DYLIB {
        vec.push(create_id_dylib_cmd(ctx));
    }
    if let Some(id) = ctx.init_routine {
        vec.push(create_routines_cmd(ctx, id));
    }

    // Chained fixups replace the classic dyld info; the export trie
    // then gets a load command of its own. A -static image has no dyld
    // to read either: it exports nothing, has only the fixups
    // -fixup_chains or -no_fixup_chains asks for, and has a dynamic
    // symbol table only under -pie, for the local relocations that
    // slide it without them (with them, it lists none). A kext has
    // only its relocations, and so, for dyld, has an image with legacy
    // LINKEDIT (see Args::legacy_linkedit).
    // (Decided by the options, not by the tables' sizes: the layout
    // sizes the header before the tables exist.)
    // (A -preload image under -fixup_chains gets its chains, and their
    // table after the segments, but ld-prime writes no command naming
    // the table: its loader must know where to find it. Under
    // -fixup_chains_section the starts are in __TEXT instead.)
    if ctx.use_chained_fixups() {
        if !ctx.args.preload && !ctx.args.fixup_chains_section {
            vec.push(create_linkedit_data_cmd(LC_DYLD_CHAINED_FIXUPS, &ctx.chained_fixups.hdr));
        }
        // Present even with nothing exported (an 8-byte empty trie),
        // as ld-prime writes it.
        if !ctx.args.without_dyld() {
            vec.push(create_linkedit_data_cmd(LC_DYLD_EXPORTS_TRIE, &ctx.export_trie.hdr));
        }
    } else if !ctx.args.legacy_linkedit && (!ctx.args.without_dyld() || ctx.args.no_fixup_chains) {
        vec.push(create_dyld_info_cmd(ctx));
    }
    vec.push(create_symtab_cmd(ctx));
    if !ctx.args.static_link || ctx.args.pie {
        vec.push(create_dysymtab_cmd(ctx));
    }
    // An executable names the dynamic linker that loads it, and dyld
    // names itself (as /usr/lib/dyld whatever -install_name says).
    if ctx.args.is_dylinker() {
        vec.push(create_string_cmd(LC_ID_DYLINKER, b"/usr/lib/dyld"));
    } else if ctx.args.output_type == MH_EXECUTE && !ctx.args.static_link {
        vec.push(create_string_cmd(LC_LOAD_DYLINKER, b"/usr/lib/dyld"));
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
    if let Some(version) = ctx.args.source_version {
        vec.push(create_source_version_cmd(version));
    }
    if ctx.args.unixthread {
        vec.push(create_unixthread_cmd(ctx));
    } else if ctx.args.output_type == MH_EXECUTE {
        vec.push(create_main_cmd(ctx));
    }
    if ctx.args.encryptable {
        vec.push(create_encryption_info_cmd(ctx));
    }
    if ctx.chunks.contains(&ChunkId::SplitInfo) {
        vec.push(create_linkedit_data_cmd(LC_SEGMENT_SPLIT_INFO, &ctx.split_info.hdr));
    }

    // Libraries in ordinal order (command-line order, then the
    // auto-linked ones), then those dyld loads lazily, by their records
    // (see lazy_load_info), in the order of the image's first uses.
    let mut dylibs: Vec<&crate::input_files::DylibFile> =
        ctx.dylibs.iter().filter(|d| !d.is_bundle_loader && !d.is_lazy).collect();
    dylibs.sort_by_key(|d| d.dylib_idx);
    for dylib in dylibs {
        vec.push(create_load_dylib_cmd(dylib));
    }
    let info = &ctx.lazy_load_info;
    for d in &info.dylibs {
        vec.push(to_vec(&LinkEditDataCommand {
            cmd: LC_LAZY_LOAD_DYLIB_INFO,
            cmdsize: size_of::<LinkEditDataCommand>() as u32,
            dataoff: (info.hdr.fileoff + d.offset as u64) as u32,
            datasize: d.size,
        }));
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
    // (Only a main executable has them.)
    for env in &ctx.args.dyld_envs {
        vec.push(create_string_cmd(LC_DYLD_ENVIRONMENT, env));
    }

    // Also present with no functions at all (an 8-byte empty table),
    // as ld-prime writes it; -no_function_starts drops it.
    if ctx.args.function_starts {
        vec.push(create_linkedit_data_cmd(LC_FUNCTION_STARTS, &ctx.function_starts.hdr));
    }

    // ld64 always writes LC_DATA_IN_CODE, even with no entries;
    // tooling takes its absence as "old linker".
    if ctx.chunks.contains(&ChunkId::DataInCode) {
        vec.push(create_linkedit_data_cmd(LC_DATA_IN_CODE, &ctx.data_in_code.hdr));
    }
    if ctx.chunks.contains(&ChunkId::MergeableRecord) {
        vec.push(create_linkedit_data_cmd(LC_ATOM_INFO, &ctx.mergeable_record.hdr));
    }
    if ctx.chunks.contains(&ChunkId::CodeSignature) {
        vec.push(create_linkedit_data_cmd(LC_CODE_SIGNATURE, &ctx.code_signature.hdr));
    }
    vec
}

/// Returns the size of the mach header chunk: the header, the load
/// commands and the header padding.
pub fn mach_header_size<E: Target>(ctx: &Context<E>) -> u64 {
    let cmds = create_load_commands(ctx);
    let size: usize = cmds.iter().map(Vec::len).sum();
    let size = size_of::<MachHeader>() as u64 + size as u64 + header_pad(ctx);
    // An encryptable image's code starts a page of its own, which the
    // header and load commands, left unencrypted, don't share; dyld's
    // starts a 4 KiB page of its own anyway.
    match ctx.args.encryptable && !ctx.args.is_dylinker() {
        true => crate::util::align_to(size, ctx.args.segment_align),
        false => size,
    }
}

/// The free space left after a final image's load commands, for tools
/// that add or grow commands in place: -headerpad, at least 32 bytes in
/// an image dyld loads (room for codesign's LC_CODE_SIGNATURE), or with
/// -headerpad_max_install_names room for each dylib command to grow to
/// MAXPATHLEN.
fn header_pad<E: Target>(ctx: &Context<E>) -> u64 {
    // A -preload image's header has pages of its own, ahead of the
    // segments; dyld's __text starts on the next 4 KiB boundary (see
    // create_output_sections), whatever -headerpad says.
    if ctx.args.preload || ctx.args.is_dylinker() {
        return 0;
    }
    let mut pad =
        if ctx.args.without_dyld() { ctx.args.headerpad } else { ctx.args.headerpad.max(32) };
    if ctx.args.headerpad_max_install_names {
        let loads = ctx.dylibs.iter().filter(|d| !d.is_bundle_loader && !d.is_lazy).count();
        let id = (ctx.args.output_type == MH_DYLIB) as usize;
        pad = pad.max((loads + id) as u64 * 1024);
    }
    pad
}

/// Writes the mach header and the load commands.
pub fn copy_mach_header<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let cmds = create_load_commands(ctx);
    let hdr = MachHeader {
        magic: MH_MAGIC_64,
        cputype: E::CPUTYPE,
        cpusubtype: E::CPUSUBTYPE,
        filetype: if ctx.args.preload { MH_PRELOAD } else { ctx.args.output_type },
        ncmds: cmds.len() as u32,
        sizeofcmds: cmds.iter().map(Vec::len).sum::<usize>() as u32,
        flags: mach_header_flags(ctx),
        reserved: 0,
    };
    hdr.write_to(buf);

    let mut off = size_of::<MachHeader>();
    for cmd in &cmds {
        buf[off..off + cmd.len()].copy_from_slice(cmd);
        off += cmd.len();
    }
}

/// The mach header's flags: what the image asks of dyld and promises
/// it.
fn mach_header_flags<E: Target>(ctx: &Context<E>) -> u32 {
    // Under -flat_namespace every import is a flat lookup that dyld
    // resolves at load, so ld64 does not claim MH_NOUNDEFS.
    let mut flags = if ctx.args.output_type == MH_EXECUTE && ctx.args.static_link {
        MH_NOUNDEFS
    } else if ctx.args.flat_namespace {
        MH_DYLDLINK
    } else {
        MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL
    };
    match ctx.args.output_type {
        MH_EXECUTE if ctx.args.pie => flags |= MH_PIE,
        MH_DYLIB if !ctx.dylibs.iter().any(|d| d.is_reexported) => flags |= MH_NO_REEXPORTED_DYLIBS,
        _ => {}
    }
    // -simulator_support: a macOS dylib that dyld may load into a
    // simulator process too (ld-prime marks no other kind of image).
    if ctx.args.simulator_support && ctx.args.output_type == MH_DYLIB {
        flags |= MH_SIM_SUPPORT;
    }
    if binds_to_weak(ctx) {
        flags |= MH_BINDS_TO_WEAK;
    }
    // MH_WEAK_DEFINES advertises exported weak symbols (auto-hidden and
    // private-extern weak definitions don't count, since no other
    // image can coalesce against them) and strong definitions that
    // override a dylib's weak export, which dyld must let win. An
    // exported weak definition also makes the image bind to weak in
    // ld-prime's eyes, referenced from within the image or not (a
    // dylib whose only weak definition nothing calls still gets
    // 0x118085), since another image's copy may replace it.
    if (0..ctx.symbols.syms.len()).into_par_iter().any(|i| ctx.exports_weak_def(i as u32)) {
        flags |= MH_WEAK_DEFINES | MH_BINDS_TO_WEAK;
    }
    if (0..ctx.symbols.syms.len()).into_par_iter().any(|i| ctx.overrides_weak_export(i as u32)) {
        flags |= MH_WEAK_DEFINES;
    }
    // -bind_at_load makes the stubs bind through the GOT instead of
    // lazily; ld-prime does not set MH_BINDATLOAD for it (dyld binds
    // everything at load anyway). MH_APP_EXTENSION_SAFE says a dylib
    // may be linked into an app extension; ld-prime sets it on dylibs
    // only, not on an executable or a bundle.
    if ctx.args.application_extension && ctx.args.output_type == MH_DYLIB {
        flags |= MH_APP_EXTENSION_SAFE;
    }
    if ctx.args.no_dynamic_access {
        flags |= MH_NOFIXPREBINDING;
    }
    if ctx
        .chunks
        .iter()
        .any(|&id| ctx.chunk_header(id).flags & SECTION_TYPE == S_THREAD_LOCAL_VARIABLES)
    {
        flags |= MH_HAS_TLV_DESCRIPTORS;
    }
    flags
}

/// Whether the image binds to a symbol some dylib defines weakly, or to
/// one of its own coalescable weak definitions (MH_BINDS_TO_WEAK: dyld
/// must then consider weak coalescing when it binds). ld-prime sets it
/// on an executable calling a dylib's weak definition, and on any image
/// with weak-lookup binds.
fn binds_to_weak<E: Target>(ctx: &Context<E>) -> bool {
    let uses_weak_export = ctx.symbols.syms.par_iter().any(|sym| match sym.file() {
        Some(FileId::Dylib(idx)) => {
            idx != u32::MAX
                && sym.is_used()
                && ctx.dylibs[idx as usize].weak_exports.contains(sym.name())
        }
        _ => false,
    });
    uses_weak_export
        || ctx.chained_fixups.imports.iter().any(|&(id, _)| ctx.binds_weak_lookup(id))
        || !ctx.weak_bind_info.contents.is_empty()
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
