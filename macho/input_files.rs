//! Input file parsing: object files, dylib stubs and archives.

use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use portable_atomic::AtomicU64;
use rayon::prelude::*;

use crate::arch::Target;
use crate::context::Context;
use crate::error::RawPath;
use crate::error::raw;
use crate::fatal;
use crate::filetype::{fat_arch_names, fat_slice, without_fat_arch};
use crate::input_sections::{
    CieRecord, FdeRecord, InputSection, NO_REPLACEMENT, RelocTarget, UNWIND_NONE, UnwindRecord,
};
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::objc::ObjcRef;
use crate::reader::{loader_rpath, resolve_dylib_ref};
use crate::symbol::{Symbol, SymbolId};
use crate::tapi;
use crate::tapi::{LdDirectives, LdSymbols, MovedExport, interpret_ld_symbols};

/// A file a symbol is owned by: an object or a dylib, by index in
/// ctx.objs or ctx.dylibs. Dylib(u32::MAX) is an import resolved by
/// dynamic lookup, which no dylib in the link provides. mold's
/// FileId.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileId {
    Obj(u32),
    Dylib(u32),
}

#[derive(Debug)]
pub struct PlatformVersion {
    pub platform: u32,
    pub minos: u32,
    /// The SDK the object was built against (a link without a
    /// -platform_version takes the first object's).
    pub sdk: u32,
}

impl PlatformVersion {
    /// The deployment target of an object file: the one its first
    /// platform load command names, if it has one.
    pub fn of_object(data: &[u8]) -> Option<Self> {
        let cputype = MachHeader::read_from(data).cputype;
        let (cmd, bytes) = load_commands(data).find(|&(cmd, _)| is_platform_cmd(cmd))?;
        Some(Self::read(cmd, bytes, cputype))
    }

    fn read(cmd: u32, data: &[u8], cputype: u32) -> Self {
        if cmd == LC_BUILD_VERSION {
            let cmd = BuildVersionCommand::read_from(data);
            return Self { platform: cmd.platform, minos: cmd.minos, sdk: cmd.sdk };
        }
        // Legacy Intel mobile objects target the simulator. Arm64
        // simulators always use LC_BUILD_VERSION.
        let simulator = cputype == CPU_TYPE_X86_64;
        let platform = match cmd {
            LC_VERSION_MIN_MACOSX => PLATFORM_MACOS,
            LC_VERSION_MIN_IPHONEOS if simulator => PLATFORM_IOSSIMULATOR,
            LC_VERSION_MIN_IPHONEOS => PLATFORM_IOS,
            LC_VERSION_MIN_TVOS if simulator => PLATFORM_TVOSSIMULATOR,
            LC_VERSION_MIN_TVOS => PLATFORM_TVOS,
            LC_VERSION_MIN_WATCHOS if simulator => PLATFORM_WATCHOSSIMULATOR,
            LC_VERSION_MIN_WATCHOS => PLATFORM_WATCHOS,
            _ => unreachable!(),
        };
        let vm = VersionMinCommand::read_from(data);
        Self { platform, minos: vm.version, sdk: vm.sdk }
    }

    /// The deployment target a bitcode file's target triple names, such
    /// as arm64-apple-macosx13.0.0 or arm64-apple-ios17.0.0-simulator,
    /// if it names an Apple platform. The SDK is not part of it.
    pub(crate) fn of_triple(triple: &str) -> Option<Self> {
        let os = triple.splitn(3, '-').nth(2)?;
        let (os, env) = os.split_once('-').unwrap_or((os, ""));
        let (name, version) =
            os.split_at(os.find(|c: char| c.is_ascii_digit()).unwrap_or(os.len()));
        let simulator = env == "simulator";
        let platform = match name {
            "macos" | "macosx" if env == "macabi" => PLATFORM_MACCATALYST,
            "ios" if env == "macabi" => PLATFORM_MACCATALYST,
            "macos" | "macosx" => PLATFORM_MACOS,
            "ios" if simulator => PLATFORM_IOSSIMULATOR,
            "ios" => PLATFORM_IOS,
            "tvos" if simulator => PLATFORM_TVOSSIMULATOR,
            "tvos" => PLATFORM_TVOS,
            "watchos" if simulator => PLATFORM_WATCHOSSIMULATOR,
            "watchos" => PLATFORM_WATCHOS,
            "xros" | "visionos" if simulator => PLATFORM_VISIONOSSIMULATOR,
            "xros" | "visionos" => PLATFORM_VISIONOS,
            "driverkit" => PLATFORM_DRIVERKIT,
            "bridgeos" => PLATFORM_BRIDGEOS,
            "firmware" => PLATFORM_FIRMWARE,
            _ => return None,
        };
        let mut nums = version.split('.').map(|n| n.parse::<u32>().unwrap_or(0));
        let mut num = || nums.next().unwrap_or(0);
        let minos = encode_version(num(), num(), num());
        Some(Self { platform, minos, sdk: 0 })
    }
}

/// Whether a load command names a deployment target: LC_BUILD_VERSION,
/// or one of the LC_VERSION_MIN_* commands that came before it.
fn is_platform_cmd(cmd: u32) -> bool {
    matches!(
        cmd,
        LC_BUILD_VERSION
            | LC_VERSION_MIN_MACOSX
            | LC_VERSION_MIN_IPHONEOS
            | LC_VERSION_MIN_TVOS
            | LC_VERSION_MIN_WATCHOS
    )
}

/// A Mach-O file's load commands, in order: each one's type and its
/// bytes.
pub(crate) fn load_commands(data: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let ncmds = MachHeader::read_from(data).ncmds;
    let mut off = size_of::<MachHeader>();
    (0..ncmds).map(move |_| {
        let lc = LoadCommand::read_from(&data[off..]);
        let bytes = &data[off..off + lc.cmdsize as usize];
        off += lc.cmdsize as usize;
        (lc.cmd, bytes)
    })
}

/// The section headers of an LC_SEGMENT_64 load command.
fn segment_sections(cmd: &[u8]) -> impl Iterator<Item = MachSection> + '_ {
    let nsects = SegmentCommand::read_from(cmd).nsects as usize;
    (0..nsects).map(move |i| {
        MachSection::read_from(&cmd[size_of::<SegmentCommand>() + i * size_of::<MachSection>()..])
    })
}

/// A Mach-O file's section headers: every segment's, in load command
/// order, the order in which section ordinals count them.
fn section_headers(data: &[u8]) -> impl Iterator<Item = MachSection> + '_ {
    load_commands(data)
        .filter(|&(cmd, _)| cmd == LC_SEGMENT_64)
        .flat_map(|(_, bytes)| segment_sections(bytes))
}

/// The NUL-terminated string a load command holds at `offset` from its
/// start, such as a dylib's install name.
fn lc_string(cmd: &[u8], offset: u32) -> &[u8] {
    let s = &cmd[offset as usize..];
    &s[..memchr::memchr(0, s).unwrap_or(s.len())]
}

/// A relocatable object file.
#[derive(Debug)]
pub struct ObjectFile {
    pub mf: &'static MappedFile,
    /// False for an archive member no live code needs (yet). Dead
    /// files' subsections never reach the output.
    pub is_reachable: bool,
    /// Position in input order, for resolution tie-breaking: the
    /// earlier file wins.
    pub priority: u32,
    /// LC_LINKER_OPTION auto-link requests, acted on only if the file
    /// is live.
    pub linker_options: Vec<Vec<Vec<u8>>>,
    /// Whether linker_options have been read (see
    /// reader::read_linker_options): what is left are the libraries to
    /// link.
    pub linker_options_read: bool,
    /// Platforms and minimum OS versions from LC_BUILD_VERSION or
    /// LC_VERSION_MIN_*. Checked only after archive selection.
    pub platform_versions: Vec<PlatformVersion>,
    /// -hidden-l: this file's external definitions become private
    /// externals.
    pub hidden: bool,
    /// MH_SUBSECTIONS_VIA_SYMBOLS was set: symbols split the sections
    /// into subsections. A -r output carries the flag only if every
    /// input had it.
    pub subsections_via_symbols: bool,
    /// Section headers in ordinal order (all segments' sections
    /// concatenated in load command order). Borrowed from the mapped
    /// file; the internal object owns its, and grows the list as the
    /// linker synthesizes sections.
    pub sect_hdrs: std::borrow::Cow<'static, [MachSection]>,
    /// This object's relocations, grouped by subsection; each
    /// subsection references a contiguous range (rel_offset/nrels).
    pub relocs: Vec<crate::input_sections::Reloc>,
    /// All of this object's subsections, sorted by input address.
    pub subsecs: Vec<crate::input_sections::InputSectionId>,
    /// The subsection each MachSym is defined in, or NONE (see
    /// symbol_subsec).
    pub sym_subsecs: Vec<crate::input_sections::InputSectionId>,
    /// The object's __objc_imageinfo, if it has one.
    pub objc_image_info: Option<ObjcImageInfo>,
    /// True if the object carries DWARF debug info, so the output gets
    /// debug stabs pointing back at it.
    pub has_debug_info: bool,
    /// For a bitcode input, the lto_module handle: the object is a
    /// placeholder that only claims symbols until LTO compiles it.
    pub lto_module: Option<usize>,
    /// Whether LTO made the object, compiling the live bitcode modules
    /// (mold's ObjectOrigin::LtoOutput; see is_lto_obj).
    pub lto_output: bool,
    pub mach_syms: std::borrow::Cow<'static, [MachSym]>,
    /// Index of the first external MachSym, if the table is partitioned
    /// locals-then-externals (see first_global_of).
    pub first_global: Option<u32>,
    /// The symbol slot for each MachSym.
    pub symbols: Vec<SymbolId>,
    /// LC_DATA_IN_CODE entries: (file offset in the object, length,
    /// kind).
    pub dice: Vec<(u32, u16, u16)>,
    /// LC_LINKER_OPTIMIZATION_HINT entries: (kind, instruction
    /// addresses in the object's address space).
    pub loh: Vec<(u8, Vec<u64>)>,
}

impl ObjectFile {
    /// The object that owns what the linker synthesizes itself: the
    /// sections standing for merged Objective-C records, folded class
    /// references or the __common zero-fill of tentative definitions,
    /// and symbols such as __mh_execute_header. It has no file behind
    /// it and no symbol table of its own; mold's
    /// ObjectFile::internal.
    pub fn internal() -> Self {
        let mf: &'static MappedFile = Box::leak(Box::new(MappedFile {
            name: PathBuf::from("<synthesized>"),
            data: &[],
            parent: None,
            mtime: None,
        }));
        Self {
            sect_hdrs: std::borrow::Cow::Owned(Vec::new()),
            mach_syms: std::borrow::Cow::Owned(Vec::new()),
            ..Self::new(mf)
        }
    }

    /// A live object of the file `mf` with no sections or symbols, for
    /// the callers to fill in.
    pub(crate) fn new(mf: &'static MappedFile) -> Self {
        Self {
            mf,
            is_reachable: true,
            priority: 0,
            linker_options: Vec::new(),
            linker_options_read: false,
            platform_versions: Vec::new(),
            hidden: false,
            subsections_via_symbols: true,
            sect_hdrs: std::borrow::Cow::Borrowed(&[]),
            relocs: Vec::new(),
            subsecs: Vec::new(),
            sym_subsecs: Vec::new(),
            objc_image_info: None,
            has_debug_info: false,
            lto_module: None,
            lto_output: false,
            mach_syms: std::borrow::Cow::Borrowed(&[]),
            first_global: None,
            symbols: Vec::new(),
            dice: Vec::new(),
            loh: Vec::new(),
        }
    }

    /// Whether the object is one LTO compiled.
    #[inline]
    pub fn is_lto_obj(&self) -> bool {
        self.lto_output
    }

    /// The subsection holding a linker optimization hint's instructions,
    /// if they are where ld64 takes a hint: one to three of them, 4-byte
    /// aligned, in one subsection of code and within 64 KiB of each
    /// other. ld64 drops any other hint as it reads the object. Without
    /// MH_SUBSECTIONS_VIA_SYMBOLS a section is one subsection here, but
    /// ld64 still splits it into subsections at every symbol, so a hint
    /// may not span one.
    pub fn hint_subsec(&self, isecs: &[InputSection], addrs: &[u64]) -> Option<usize> {
        let lo = *addrs.iter().min()?;
        let hi = *addrs.iter().max()?;
        let (id, _) = self.find_subsec(isecs, lo)?;
        let isec = &isecs[id];
        let is_code = isec.hdr(self).flags & S_ATTR_PURE_INSTRUCTIONS != 0;
        let spans_symbol = || {
            self.mach_syms.iter().any(|msym| {
                !msym.is_stab()
                    && msym.ty() == N_SECT
                    && msym.sect as u32 == isec.shndx + 1
                    && lo < msym.value
                    && msym.value <= hi
            })
        };
        (is_code
            && addrs.len() <= 3
            && addrs.iter().all(|a| a.is_multiple_of(4))
            && hi - lo <= 0xffff
            && hi + 4 <= isec.input_addr as u64 + isec.size as u64
            && (self.subsections_via_symbols || !spans_symbol()))
        .then_some(id)
    }
}

/// Adds a section the linker synthesizes to the internal object,
/// returning the (file, shndx) pair a subsection standing for it
/// carries.
pub fn add_synthetic_section<E: Target>(ctx: &mut Context<E>, hdr: MachSection) -> (u32, u32) {
    let file = ctx.internal_obj.expect("internal object not created yet");
    let hdrs = ctx.objs[file].sect_hdrs.to_mut();
    hdrs.push(hdr);
    (file as u32, (hdrs.len() - 1) as u32)
}

/// Appends a live synthetic subsection of `sect`, a section of the
/// internal object as add_synthetic_section returns it, and returns
/// it. Its output section and offset are set by hand (IS_PLACED), not
/// by create_output_sections; `offset` is its offset if already known.
pub(crate) fn add_placed_isec<E: Target>(
    ctx: &mut Context<E>,
    sect: (u32, u32),
    p2align: u8,
    size: u64,
    offset: u64,
) -> u32 {
    let (file, shndx) = sect;
    ctx.isecs.push(InputSection {
        offset: offset as u32,
        flags: InputSection::flags_placed(),
        ..InputSection::new(file, shndx, p2align, size as u32, &[])
    });
    (ctx.isecs.len() - 1) as u32
}

/// A field of a synthesized data record.
#[derive(Clone, Debug)]
pub enum DataField {
    Bytes(Vec<u8>),
    /// An 8-byte pointer, rebased at load (or null), or bound if to an
    /// import.
    Ptr(ObjcRef),
}

/// A synthesized data record (an Objective-C one, or the table of
/// bundle_hook), placed in the tail of the output section `sect`
/// (mapped to its segment like an input section of that name) as the
/// synthetic subsection `isec`.
#[derive(Debug)]
pub struct DataBlob {
    pub sect: &'static [u8],
    pub isec: u32,
    pub fields: Vec<DataField>,
}

impl DataBlob {
    pub fn size(&self) -> u64 {
        self.fields
            .iter()
            .map(|f| match f {
                DataField::Bytes(b) => b.len() as u64,
                DataField::Ptr(_) => 8,
            })
            .sum()
    }
}

/// Appends a synthesized record to the tail of __DATA,`sect` (a section
/// with the given flags) and returns its subsection.
pub(crate) fn add_data_blob<E: Target>(
    ctx: &mut Context<E>,
    sect: &'static [u8],
    flags: u32,
    fields: Vec<DataField>,
) -> u32 {
    let hdr = MachSection {
        sectname: bytes_to_name(sect),
        segname: bytes_to_name(b"__DATA"),
        p2align: 3,
        flags,
        ..Default::default()
    };
    let (file, shndx) = add_synthetic_section(ctx, hdr);
    let blob = DataBlob { sect, isec: 0, fields };
    let isec = add_placed_isec(ctx, (file, shndx), 3, blob.size(), 0);
    ctx.data_blobs.push(DataBlob { isec, ..blob });
    isec
}

/// Synthesizes a zero word of `size` bytes, aligned to its size, in
/// __DATA,__data (after the inputs'), and returns its subsection.
pub fn add_data_word<E: Target>(ctx: &mut Context<E>, size: u32) -> u32 {
    let p2align = size.trailing_zeros() as u8;
    let hdr = MachSection {
        sectname: bytes_to_name(b"__data"),
        segname: bytes_to_name(b"__DATA"),
        p2align: p2align as u32,
        flags: 0,
        ..Default::default()
    };
    let (file, shndx) = add_synthetic_section(ctx, hdr);
    ctx.isecs.push(InputSection {
        flags: InputSection::flags_placed(),
        ..InputSection::new(file, shndx, p2align, size, &[])
    });
    let isec = (ctx.isecs.len() - 1) as u32;
    let fields = vec![DataField::Bytes(vec![0; size as usize])];
    ctx.data_blobs.push(DataBlob { sect: b"__data", isec, fields });
    isec
}

/// Synthesizes a C string in __TEXT,__cstring, after the inputs', and
/// returns its subsection.
pub(crate) fn add_cstring<E: Target>(ctx: &mut Context<E>, s: &[u8]) -> u32 {
    let hdr = MachSection {
        sectname: bytes_to_name(b"__cstring"),
        segname: bytes_to_name(b"__TEXT"),
        flags: S_CSTRING_LITERALS,
        ..Default::default()
    };
    let (file, shndx) = add_synthetic_section(ctx, hdr);
    let mut bytes = s.to_vec();
    bytes.push(0);
    let bytes: &'static [u8] = Vec::leak(bytes);
    ctx.isecs.push(InputSection {
        flags: InputSection::flags_alive_no_modulus(),
        ..InputSection::new(file, shndx, 0, bytes.len() as u32, bytes)
    });
    (ctx.isecs.len() - 1) as u32
}

/// Whether a section is one ld-prime reads as a list of records -
/// CFStrings, UTF-16 strings, selector and class references, Objective-C
/// class and category lists - whose subsections no local symbol names:
/// they are "anon" in its diagnostics. The UTF-16 strings of
/// an object without subsections (`split` false) are one subsection,
/// named by its labels.
pub fn is_record_list(hdr: &MachSection, split: bool) -> bool {
    hdr.section_type() == S_LITERAL_POINTERS
        || matches!(
            hdr.sectname(),
            b"__cfstring"
                | b"__objc_classrefs"
                | b"__objc_classlist"
                | b"__objc_nlclslist"
                | b"__objc_catlist"
                | b"__objc_nlcatlist"
        )
        || split && hdr.sectname_is(b"__ustring")
}

/// Whether a section is an Objective-C list whose entries ld-prime
/// names no symbol for: __DATA's __objc_classlist, __objc_nlclslist,
/// __objc_catlist, __objc_catlist2 and __objc_nlcatlist, and
/// __objc_clsrolist, which only a -r output keeps.
fn is_unnamed_objc_list(hdr: &MachSection) -> bool {
    hdr.segname_is(b"__DATA")
        && hdr.sectname.starts_with(b"__objc_")
        && [
            "__objc_classlist",
            "__objc_nlclslist",
            "__objc_catlist",
            "__objc_catlist2",
            "__objc_nlcatlist",
            "__objc_clsrolist",
        ]
        .iter()
        .any(|name| hdr.sectname_is(name.as_bytes()))
}

/// Whether ld-prime splits a section into subsections by content and
/// names none of them: CFStrings, selector and class references,
/// UTF-16 literals and Objective-C constant literals (@42, @[...],
/// @{...}). No label of theirs is in an image's symbol table.
/// Selector references are so only of the literal-pointer type the
/// compilers give them: a regular or coalesced __objc_selrefs is data,
/// whose labels stay and whose references don't merge. Superclass and
/// protocol references of the literal-pointer type are taken for class
/// references too, which merge whatever labels them (see
/// is_class_or_protocol_ref). In an object without subsections (`split`
/// false) the UTF-16 literals' section is one subsection, whose labels
/// ld-prime keeps as any other's.
fn has_unnamed_subsecs(hdr: &MachSection, split: bool) -> bool {
    if hdr.segname_is(b"__TEXT") {
        return split && hdr.sectname_is(b"__ustring");
    }
    if hdr.sectname_is(b"__objc_selrefs") {
        return hdr.segname_is(b"__DATA") && hdr.section_type() == S_LITERAL_POINTERS;
    }
    hdr.segname_is(b"__DATA")
        && ([
            "__cfstring",
            "__objc_classrefs",
            "__objc_intobj",
            "__objc_floatobj",
            "__objc_doubleobj",
            "__objc_dateobj",
            "__objc_arraydata",
            "__objc_arrayobj",
            "__objc_dictobj",
        ]
        .iter()
        .any(|name| hdr.sectname_is(name.as_bytes()))
            || (hdr.section_type() == S_LITERAL_POINTERS && is_class_or_protocol_ref(hdr)))
}

/// Whether a section holds superclass or protocol references,
/// __DATA,__objc_superrefs or __objc_protorefs. ld-prime cuts them one
/// per pointer and merges the unlabeled ones of one target; one a
/// symbol names stays apart and keeps its label (see
/// mark_labeled_literals), unless the section has the literal-pointer
/// type, which merges them all (see has_unnamed_subsecs).
pub(crate) fn is_class_or_protocol_ref(hdr: &MachSection) -> bool {
    hdr.segname_is(b"__DATA") && is_class_or_protocol_ref_name(hdr.sectname())
}

pub(crate) fn is_class_or_protocol_ref_name(sectname: &[u8]) -> bool {
    matches!(sectname, b"__objc_superrefs" | b"__objc_protorefs")
}

/// An object's lookups of its subsections by address, as sold's
/// ObjectFile::find_subsection, for ObjectFile and StagedObject alike:
/// `isecs` is where the object's `subsecs` point, the link's
/// subsections for an ObjectFile and its own for a StagedObject.
macro_rules! subsec_lookups {
    () => {
        /// Finds the subsection containing `addr` among the object's
        /// `subsecs` (sorted by input address), returning it with the
        /// offset within it.
        pub fn find_subsec(&self, isecs: &[InputSection], addr: u64) -> Option<(usize, u64)> {
            let subsecs = &self.subsecs;
            let i = subsecs.partition_point(|&id| isecs[id as usize].input_addr as u64 <= addr);
            if i == 0 {
                return None;
            }
            let id = subsecs[i - 1] as usize;
            let isec = &isecs[id];
            if addr < isec.input_addr as u64 + isec.size as u64
                || (isec.size as u64 == 0 && addr == isec.input_addr as u64)
            {
                Some((id, addr - isec.input_addr as u64))
            } else {
                None
            }
        }

        /// Finds the subsection a symbol at `addr` in section `sect`
        /// (1-based, as MachSyms count) belongs to, returning it with
        /// the offset within it. The section decides where addresses
        /// alone can't: a label on an empty section starts where the
        /// next section does, and one past a section's last byte (an
        /// array's `_end`) ends where the next one starts; both belong
        /// to their own section, as in ld-prime.
        pub fn find_symbol_subsec(
            &self,
            isecs: &[InputSection],
            sect: u8,
            addr: u64,
        ) -> Option<(usize, u64)> {
            let subsecs = &self.subsecs;
            let end = subsecs.partition_point(|&id| isecs[id as usize].input_addr as u64 <= addr);
            symbol_subsec_before(isecs, &subsecs[..end], sect, addr)
        }
    };
}

/// find_symbol_subsec's answer from `before`, the object's subsections
/// that start at or before `addr`.
fn symbol_subsec_before(
    isecs: &[InputSection],
    before: &[crate::input_sections::InputSectionId],
    sect: u8,
    addr: u64,
) -> Option<(usize, u64)> {
    let shndx = u32::from(sect).wrapping_sub(1);
    // The nearest subsection of the section starting at or before
    // `addr`; any in between belong to empty sections at that address.
    let id = before.iter().rev().map(|&id| id as usize).find(|&id| isecs[id].shndx == shndx)?;
    let isec = &isecs[id];
    // A label may sit at a section's end, except in one of fixed-size
    // records, where it names no record: ld-prime ignores it, and a
    // relocation to it fails as one to an undefined symbol.
    let end = isec.input_addr as u64 + isec.size as u64;
    (addr < end || (addr == end && !isec.is_record())).then(|| (id, addr - isec.input_addr as u64))
}

/// A dynamic library, from a .tbd stub or a dylib binary.
#[derive(Debug)]
pub struct DylibFile {
    /// The path the library was loaded from, for -t.
    pub path: PathBuf,
    /// The install name, as LC_ID_DYLIB (or a .tbd) spells it.
    pub install_name: Vec<u8>,
    pub current_version: u32,
    pub compatibility_version: u32,
    /// The minimum OS version the library was built for on the link's
    /// platform, 0 if it names none.
    pub minos: u32,
    /// Found in the SDK, whose libraries' minimum OS versions ld-prime
    /// doesn't check (see passes::check_input_versions).
    pub in_sdk: bool,
    /// The 1-based ordinal used to refer to this dylib in bind records;
    /// BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE (-1) for a -bundle_loader.
    pub dylib_idx: i32,
    /// The -bundle_loader executable: its symbols bind to the main
    /// executable at run time and it gets no LC_LOAD_DYLIB.
    pub is_bundle_loader: bool,
    /// Position in input order, taken as the library is named, before
    /// the libraries it re-exports load: for resolution tie-breaking,
    /// and the order of the load commands (see named_at).
    pub priority: u32,
    /// True if loaded with LC_LOAD_WEAK_DYLIB: dyld tolerates the
    /// library missing at load time.
    pub is_weak: bool,
    /// -assert-weak-l and the like: loaded with LC_LOAD_WEAK_DYLIB, but
    /// its imports are weak only as the references say, and must all
    /// be (see passes::check_weak_assertions).
    pub is_weak_asserted: bool,
    /// True if re-exported (LC_REEXPORT_DYLIB): this image's clients
    /// resolve the library's exports through this image.
    pub is_reexported: bool,
    /// Re-exported from a location that isn't public, as two or more
    /// libraries are (see passes::bind_private_reexports_to_image): its
    /// imports bind to this image (BIND_SPECIAL_DYLIB_SELF, and library
    /// ordinal 0 in their desc), through whose re-exports dyld finds
    /// them, and their GOT slots go with the image's own. Its load
    /// command keeps its place and ordinal.
    pub binds_to_image: bool,
    /// -needed-l: keep the load command even under -dead_strip_dylibs.
    pub is_needed: bool,
    /// -upward-l: an upward dependency (LC_LOAD_UPWARD_DYLIB), one that
    /// depends on this image in turn, so dyld need not initialize it
    /// first.
    pub is_upward: bool,
    /// -lazy-l, -lazy_library, -lazy_framework from macOS 27 on, or a
    /// public library such a dylib re-exports: dyld loads it at the
    /// first use of one of its symbols (LC_LAZY_LOAD_DYLIB_INFO), so it
    /// has no LC_LOAD_DYLIB and no ordinal.
    pub is_lazy: bool,
    /// -delay-l and the like, or a public library such a dylib
    /// re-exports: dyld runs its initializers only when the image
    /// dlopen()s this install name (its own, or the re-exporting
    /// dylib's), before the first use of one of its symbols (see
    /// delay_init::create_delay_init).
    pub delay_init: Option<Vec<u8>>,
    /// For a library loaded as a public re-export that the command line
    /// or an auto-link option names later, its position among the
    /// inputs where it is named.
    pub named_at: Option<u32>,
    /// Loaded through an object's LC_LINKER_OPTION rather than the
    /// command line: a hint, so ld64 gives it a load command only if
    /// something binds to it.
    pub is_autolinked: bool,
    /// Loaded because a dylib on the command line (or auto-linked)
    /// re-exports it and it lives in a public location: symbols found
    /// through the re-export bind to it directly, and it gets a load
    /// command of its own if anything binds to it. Private re-exported
    /// libraries are not loaded this way; their symbols bind to the
    /// re-exporting dylib.
    pub is_implicit: bool,
    pub exports: hashbrown::HashSet<&'static [u8]>,
    /// The symbols of the link it exports - those of its exports that
    /// the inputs name - by which it takes part in resolution as mold's
    /// SharedFile does by its own symbols, and the size of the symbol
    /// table they were collected from (see
    /// passes::collect_dylib_symbols).
    pub symbols: Vec<SymbolId>,
    pub symbols_seen: Option<usize>,
    /// Exports that are weak definitions: binding to one sets
    /// MH_BINDS_TO_WEAK on the client image.
    pub weak_exports: hashbrown::HashSet<&'static [u8]>,
    /// Whether the library itself, not one it re-exports, exports weak
    /// definitions, which ld-prime says keep it from being delayed
    /// (see reader::name_dylib).
    pub has_weak_defs: bool,
    /// The subset of exports that are thread-local variables.
    pub tlv_exports: hashbrown::HashSet<&'static [u8]>,
    /// The install names of the private libraries this dylib re-exports,
    /// whose exports are merged into its own.
    pub merged_reexports: Vec<Vec<u8>>,
    /// The public libraries it re-exports, directly or through private
    /// ones, which are dylibs of the link of their own (see
    /// passes::dylib_ranks).
    pub reexported: Vec<ReexportEdge>,
    /// Exports (its own or merged ones) that per-symbol $ld$previous
    /// directives move to older libraries for the link's target, each
    /// with the index of the dylib that stands for the library it binds
    /// to instead (see add_moved_dylibs).
    pub moved_exports: hashbrown::HashMap<&'static [u8], usize>,
    /// Whose install name it has: its own or an older library's.
    pub name_source: NameSource,
}

impl DylibFile {
    /// A library of the link with `install_name`, read from `path`: at
    /// version 1.0.0, with no exports, and loaded as a plain dependency.
    /// The callers fill in what their file says.
    fn new(path: PathBuf, install_name: Vec<u8>) -> Self {
        Self {
            path,
            install_name,
            current_version: encode_version(1, 0, 0),
            compatibility_version: encode_version(1, 0, 0),
            minos: 0,
            in_sdk: false,
            dylib_idx: 0,
            is_bundle_loader: false,
            priority: 0,
            is_weak: false,
            is_weak_asserted: false,
            is_reexported: false,
            binds_to_image: false,
            is_needed: false,
            is_upward: false,
            is_lazy: false,
            delay_init: None,
            named_at: None,
            is_autolinked: false,
            is_implicit: false,
            exports: hashbrown::HashSet::new(),
            symbols: Vec::new(),
            symbols_seen: None,
            weak_exports: hashbrown::HashSet::new(),
            has_weak_defs: false,
            tlv_exports: hashbrown::HashSet::new(),
            merged_reexports: Vec::new(),
            reexported: Vec::new(),
            moved_exports: hashbrown::HashMap::new(),
            name_source: NameSource::Own,
        }
    }

    /// True if a dylib's exports bound here moved to older libraries
    /// ($ld$previous): ld-prime lists a library under the install names
    /// bound to it, so one all of whose bound exports moved loses its
    /// load command, named or not; libc++ does to libc++abi for macOS
    /// 13 if only char8_t's type_info binds. (It drops a -needed_* or
    /// -reexport_* library alike, not what the option asks for; those
    /// stay.)
    pub fn exports_moved_away<E: Target>(&self, ctx: &Context<E>) -> bool {
        self.moved_exports.iter().any(|(&name, &target)| {
            let file = ctx.symbols.lookup(name).and_then(|id| ctx.symbols[id].file());
            file == Some(FileId::Dylib(target as u32))
        })
    }

    /// Returns the bind ordinal for a symbol imported from this dylib:
    /// its load-command ordinal under two-level namespace (the image's
    /// own for one of its private re-exports; see binds_to_image), or
    /// the flat-lookup sentinel under -flat_namespace (`flat_namespace`).
    pub fn bind_ordinal(&self, flat_namespace: bool) -> i32 {
        if flat_namespace {
            BIND_SPECIAL_DYLIB_FLAT_LOOKUP
        } else if self.binds_to_image {
            BIND_SPECIAL_DYLIB_SELF
        } else {
            self.dylib_idx
        }
    }
}

/// Whose install name a dylib has, which decides between the dylibs of
/// the link that have the same one (see add_dylib).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameSource {
    /// Its own, as LC_ID_DYLIB or the stub spells it.
    Own,
    /// An older library's, which an $ld$previous or $ld$install_name
    /// directive gives it for the link's target.
    Directive,
    /// None: the dylib stands for an older library that exports moved
    /// by per-symbol $ld$previous directives bind to, and has no file
    /// or exports of its own.
    Moved,
}

/// A public library a dylib re-exports: its index among the link's
/// dylibs, how many re-exports away from the dylib it is (a private
/// library in between counts as one) and the install name of the
/// library that re-exports it.
#[derive(Debug)]
pub struct ReexportEdge {
    pub dylib: usize,
    pub hops: u32,
    pub via: Vec<u8>,
}

/// Returns true for sections that don't become part of the output image.
fn is_discarded_section(hdr: &MachSection) -> bool {
    // The __DWARF and __LD (__compact_unwind) segments are consumed by
    // other tools or, later, by the linker itself; they are never
    // copied into an output. Not into a -r output either: ld64 does
    // not merge DWARF (the section-relative offsets in it - abbrev,
    // line table, ranges - carry no relocations and would all have to
    // be rebased), it writes debug-note stabs naming the input objects
    // as the places debuggers read DWARF from, and a later link
    // carries those notes through. ld-prime goes by the segment alone:
    // a section with S_ATTR_DEBUG elsewhere is copied like any other
    // (the attribute is dropped in a final image), and one in those
    // segments without it is dropped all the same.
    hdr.segname() == b"__DWARF" || hdr.segname() == b"__LD"
}

/// The alignment of every record of a section of fixed-size records (see
/// record_size), as ld-prime gives its subsections: with no modulus, each
/// record starting at a multiple of it whatever its offset in the input,
/// and mostly the section's own. A literal is aligned to its size:
/// compilers emit __literal16 with p2align 3 for a 16-byte constant whose
/// type is only 8-aligned, and rely on the linker to place it where a
/// 16-byte load can reach it. An initializer, terminator or non-lazy
/// symbol pointer, a GOT slot of any type, a
/// CFString constant and a pointer-auth slot are aligned to a pointer,
/// even from a section that claims less or more, in a -r output as in an
/// image; a thread-local variable descriptor (from a section clang aligns
/// to a byte) to a pointer in an image, and in a -r output to at least
/// one.
fn record_p2align(hdr: &MachSection, relocatable: bool) -> Option<u8> {
    let size = record_size(hdr)?;
    let p2align = hdr.p2align as u8;
    Some(match hdr.section_type() {
        S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS => size.trailing_zeros() as u8,
        S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS | S_NON_LAZY_SYMBOL_POINTERS => 3,
        S_THREAD_LOCAL_VARIABLES if relocatable => p2align.max(3),
        S_THREAD_LOCAL_VARIABLES => 3,
        _ if hdr.segname() == b"__DATA"
            && matches!(hdr.sectname(), b"__cfstring" | b"__auth_ptr" | b"__got") =>
        {
            3
        }
        _ => p2align,
    })
}

/// The size of each record of a section ld-prime splits into fixed-size
/// records. Its name decides for the __DATA segment's GOT and Objective-C
/// lists, whatever their type, and for a CFString, pointer-auth,
/// lazy-load slot or compact unwind section of the regular type; its
/// type does for the others: literals, pointers to initializers,
/// terminators or GOT slots, and thread-local variable descriptors
/// (three pointers).
pub(crate) fn record_size(hdr: &MachSection) -> Option<u64> {
    let regular = hdr.section_type() == S_REGULAR;
    match (hdr.segname(), hdr.sectname()) {
        (
            b"__DATA",
            b"__got" | b"__objc_classlist" | b"__objc_catlist" | b"__objc_catlist2"
            | b"__objc_clsrolist" | b"__objc_nlclslist" | b"__objc_nlcatlist" | b"__objc_protolist"
            | b"__objc_selrefs" | b"__objc_classrefs" | b"__objc_superrefs" | b"__objc_protorefs",
        ) => return Some(8),
        (b"__DATA", b"__auth_ptr" | b"__lazy_load_got") if regular => return Some(8),
        (b"__DATA", b"__cfstring") | (b"__LD", b"__compact_unwind") if regular => return Some(32),
        _ => {}
    }
    match hdr.section_type() {
        S_4BYTE_LITERALS => Some(4),
        S_8BYTE_LITERALS => Some(8),
        S_16BYTE_LITERALS => Some(16),
        S_NON_LAZY_SYMBOL_POINTERS | S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS => Some(8),
        S_THREAD_LOCAL_VARIABLES => Some(24),
        _ => None,
    }
}

/// Fails the link on a section the linker can't read: one of fixed-size
/// records (see record_size) that doesn't end on a record boundary, or,
/// in an object with indirect symbols (`nindirect` table entries), one
/// of lazy or non-lazy symbol pointers. The indirect symbol table names
/// the targets of those slots, as 32-bit code wrote them with
/// `.indirect_symbol`, and mold, reading only relocations, would leave
/// the slots null.
fn check_sections(hdrs: &[MachSection], nindirect: u32, file: &Path) {
    for hdr in hdrs {
        let partial = record_size(hdr).is_some_and(|size| !hdr.size.is_multiple_of(size));
        let indirect = nindirect != 0
            && matches!(hdr.section_type(), S_LAZY_SYMBOL_POINTERS | S_NON_LAZY_SYMBOL_POINTERS);
        if partial || indirect {
            let what = if partial {
                "section size is not a multiple of the record size"
            } else {
                "indirect symbol pointers are not supported"
            };
            fatal!("{}:({},{}): {what}", file.raw(), raw(hdr.segname()), raw(hdr.sectname()));
        }
    }
}

/// An object's Objective-C image info record (see is_objc_image_info).
#[derive(Clone, Copy, Debug)]
pub struct ObjcImageInfo {
    /// The record's flags word.
    pub flags: u32,
    /// Whether the object has a __DATA,__objc_classlist section, empty
    /// or not, which ld-prime takes for one that defines classes: the
    /// flag of signed class_ro_t pointers speaks for theirs (see
    /// chunks::objc_imageinfo::merge_objc_info).
    pub classes: bool,
}

/// Whether a section is an object's Objective-C image info, the record
/// whose flags the link merges into the image's own (see
/// chunks::objc_imageinfo::create): ld-prime knows it in
/// __DATA alone, and links an __objc_imageinfo of another segment as
/// any other section.
pub fn is_objc_image_info(hdr: &MachSection) -> bool {
    hdr.segname() == b"__DATA" && hdr.sectname() == b"__objc_imageinfo"
}

/// Whether a section is clang -faddrsig's address-significance table,
/// which names the symbols whose addresses are significant, for a
/// linker's safe ICF, by relocations that all point into its eight
/// placeholder bytes. As mold drops .llvm_addrsig, an image drops it: our
/// ICF tells a taken address by the type of the relocation that takes it.
fn is_llvm_addrsig(hdr: &MachSection) -> bool {
    hdr.segname() == b"__DATA" && hdr.sectname() == b"__llvm_addrsig"
}

/// Whether a section is one of the __LD segment's that ld-prime doesn't
/// know. It reads only __LD,__compact_unwind and drops any other with a
/// warning; a symbol defined in one is gone.
pub fn is_unknown_ld_section(hdr: &MachSection) -> bool {
    hdr.segname() == b"__LD" && hdr.sectname() != b"__compact_unwind"
}

/// Whether -remove_swift_reflection_metadata_sections drops an input
/// section: Swift's field descriptors, associated type records and the
/// names they give (but not the type references), in any segment.
pub(crate) fn is_swift_reflection_section(hdr: &MachSection) -> bool {
    matches!(hdr.sectname(), b"__swift5_fieldmd" | b"__swift5_assocty" | b"__swift5_reflstr")
}

/// The flags ld-prime reads a section of an input object as having,
/// which decide how the link splits the section into subsections and
/// what it makes of them - mold's canonicalize_type for a section typed
/// by name alone. __TEXT,__constructor, where GCC put the constructors
/// of code built without dyld (-static, -mkernel) with the assembler's
/// .constructor directive, is a list of initializer pointers whatever
/// its type (__TEXT,__destructor stays data). ld-prime knows the
/// Objective-C runtime's sections by name too (see
/// standard_section_flags): one of another type has the table's flags,
/// so a regular __objc_methname is C strings and a list typed as
/// strings or literals is pointers still. __objc_selrefs keeps its own
/// type, which says whether its references merge, and __DATA,__got,
/// GOT slots whatever its type, its own, which
/// says whether the object asks for an indirect-symbol GOT (see
/// check_sections); but neither is ever split into strings or
/// literals. Superclass and protocol references keep the
/// literal-pointer type, whose references all merge (see
/// has_unnamed_subsecs), though a -r output has the table's flags. Its
/// own flags otherwise.
pub(crate) fn canonical_section_flags(segname: &[u8], sectname: &[u8], flags: u32) -> u32 {
    if (segname, sectname) == (b"__TEXT", b"__constructor") {
        return S_MOD_INIT_FUNC_POINTERS;
    }
    let ty = flags & SECTION_TYPE;
    let is_literal =
        matches!(ty, S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS);
    if (segname, sectname) == (b"__DATA", b"__got") {
        return if is_literal { flags & !SECTION_TYPE } else { flags };
    }
    if !sectname.starts_with(b"__objc_") {
        return flags;
    }
    let Some(table) = standard_section_flags(segname, sectname) else {
        return flags;
    };
    if sectname == b"__objc_selrefs" {
        return if is_literal { flags & !SECTION_TYPE } else { flags };
    }
    if ty == table & SECTION_TYPE
        || (ty == S_LITERAL_POINTERS && is_class_or_protocol_ref_name(sectname))
    {
        flags
    } else {
        table
    }
}

/// The flags of a section ld-prime's table of standard sections names:
/// those a compiler marks a section of that name with, or ld-prime its
/// own sections - code (the stubs and helpers too), literals, pointer
/// lists, the thread-local and zero-fill types, no-dead-strip for the
/// lists the Objective-C runtime scans, and none for the rest of the
/// data. None for another name, or in another segment.
pub(crate) fn standard_section_flags(segname: &[u8], sectname: &[u8]) -> Option<u32> {
    let flags = match (segname, sectname) {
        (
            b"__TEXT",
            b"__text" | b"__StaticInit" | b"__stub_helper" | b"__objc_stubs" | b"__objc_clsstubs"
            | b"__delay_stubs" | b"__delay_helper" | b"__lazy_helpers" | b"__resolver_help",
        ) => S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS,
        (
            b"__TEXT",
            b"__cstring" | b"__objc_classname" | b"__objc_methname" | b"__objc_methtype"
            | b"__oslogstring",
        ) => S_CSTRING_LITERALS,
        (b"__TEXT", b"__literal4") => S_4BYTE_LITERALS,
        (b"__TEXT", b"__literal8") => S_8BYTE_LITERALS,
        (b"__TEXT", b"__literal16") => S_16BYTE_LITERALS,
        (b"__TEXT", b"__const" | b"__ustring" | b"__gcc_except_tab" | b"__objc_methlist") => {
            S_REGULAR
        }
        (b"__DATA", b"__got" | b"__auth_got" | b"__weak_got" | b"__weak_auth_got") => {
            S_NON_LAZY_SYMBOL_POINTERS
        }
        (b"__DATA", b"__la_symbol_ptr" | b"__la_resolver") => S_LAZY_SYMBOL_POINTERS,
        (b"__DATA", b"__mod_init_func") => S_MOD_INIT_FUNC_POINTERS,
        (b"__DATA", b"__mod_term_func") => S_MOD_TERM_FUNC_POINTERS,
        (
            b"__DATA",
            b"__objc_classlist" | b"__objc_nlclslist" | b"__objc_catlist" | b"__objc_catlist2"
            | b"__objc_nlcatlist" | b"__objc_classrefs" | b"__objc_superrefs" | b"__objc_clsrolist",
        ) => S_ATTR_NO_DEAD_STRIP,
        (b"__DATA", b"__objc_protolist") => S_COALESCED,
        (b"__DATA", b"__objc_protorefs") => S_COALESCED | S_ATTR_NO_DEAD_STRIP,
        (b"__DATA", b"__objc_selrefs") => S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP,
        (b"__DATA", b"__thread_vars") => S_THREAD_LOCAL_VARIABLES,
        (b"__DATA", b"__thread_ptrs") => S_THREAD_LOCAL_VARIABLE_POINTERS,
        (b"__DATA", b"__thread_data") => S_THREAD_LOCAL_REGULAR,
        (b"__DATA", b"__thread_bss") => S_THREAD_LOCAL_ZEROFILL,
        (b"__DATA", b"__bss" | b"__common") => S_ZEROFILL,
        (
            b"__DATA",
            b"__data" | b"__const" | b"__cfstring" | b"__auth_ptr" | b"__objc_data"
            | b"__objc_const" | b"__objc_ivar" | b"__objc_imageinfo" | b"__objc_intobj"
            | b"__objc_floatobj" | b"__objc_doubleobj" | b"__objc_dateobj" | b"__objc_dictobj"
            | b"__objc_arrayobj" | b"__objc_arraydata" | b"__const_cfobj2",
        ) => S_REGULAR,
        // The compiler's records for the linker to encode into
        // __unwind_info, which no output carries (but a boundary
        // symbol's empty section).
        (b"__LD", b"__compact_unwind") => S_ATTR_DEBUG,
        _ => return None,
    };
    Some(flags)
}

/// An object file parsed in isolation: all cross-references are local
/// indices, so staging runs in parallel across files with no shared
/// state; `integrate_object` rebases them into the global arenas.
/// `stage_object` builds it; the fields mean what ObjectFile's do.
pub struct StagedObject {
    pub mf: &'static MappedFile,
    pub alive: bool,
    pub hidden: bool,
    pub priority: u32,
    pub sect_hdrs: &'static [MachSection],
    pub linker_options: Vec<Vec<Vec<u8>>>,
    pub platform_versions: Vec<PlatformVersion>,
    /// MH_SUBSECTIONS_VIA_SYMBOLS: symbols split sections into
    /// subsections.
    pub subsections_via_symbols: bool,
    /// The object's subsections, in section order and by address
    /// within a section; the other fields refer to them by their index
    /// here.
    pub isecs: Vec<InputSection>,
    pub relocs: Vec<crate::input_sections::Reloc>,
    /// Indices into `isecs`, sorted by input address.
    pub subsecs: Vec<crate::input_sections::InputSectionId>,
    /// Each MachSym's subsection, an index into `isecs`, or NONE (see
    /// find_symbol_subsecs).
    pub sym_subsecs: Vec<crate::input_sections::InputSectionId>,
    pub mach_syms: std::borrow::Cow<'static, [MachSym]>,
    /// Index of the first external MachSym, if the table is partitioned
    /// locals-then-externals (see first_global_of).
    pub first_global: Option<u32>,
    /// Each MachSym's name, interned at integration.
    pub sym_names: Vec<&'static [u8]>,
    /// xxh3 of each extern non-stab name (0 otherwise), computed here
    /// so the serial intern path never hashes.
    pub sym_hashes: Vec<u64>,
    /// The object's unwind info: a record per function (from
    /// __compact_unwind, or made for a function that has only an FDE),
    /// and the CIEs and FDEs of its __eh_frame.
    pub unwind: Vec<UnwindRecord>,
    pub cies: Vec<CieRecord>,
    pub fdes: Vec<FdeRecord>,
    pub objc_image_info: Option<ObjcImageInfo>,
    pub has_debug_info: bool,
    /// LC_DATA_IN_CODE entries: (file offset in the object, length,
    /// kind).
    pub dice: Vec<(u32, u16, u16)>,
    /// LC_LINKER_OPTIMIZATION_HINT entries: (kind, instruction
    /// addresses in the object's address space).
    pub loh: Vec<(u8, Vec<u64>)>,
}

/// The object's MachSym array as a slice of the mapped file, or None
/// if it is unaligned or truncated (then the caller copies it).
fn mach_syms_slice(data: &'static [u8], off: usize, n: usize) -> Option<&'static [MachSym]> {
    let bytes = n.checked_mul(size_of::<MachSym>())?;
    if off.checked_add(bytes)? > data.len()
        || !(data.as_ptr() as usize + off).is_multiple_of(std::mem::align_of::<MachSym>())
    {
        return None;
    }
    // SAFETY: in bounds and aligned (checked above); MachSym is a
    // #[repr(C)] struct of plain integers, valid for every bit pattern;
    // the mapping lives for the whole link.
    Some(unsafe { std::slice::from_raw_parts(data.as_ptr().add(off).cast::<MachSym>(), n) })
}

/// The MachSym index ranges of an object's local (with stab) and external
/// (defined and undefined) symbols. With a partitioned table these are
/// the two halves; without one, both are the whole table and callers'
/// per-entry filters still decide.
macro_rules! symbol_ranges {
    () => {
        #[inline]
        pub fn local_range(&self) -> std::ops::Range<usize> {
            0..self.first_global.map_or(self.mach_syms.len(), |g| g as usize)
        }
        #[inline]
        pub fn global_range(&self) -> std::ops::Range<usize> {
            self.first_global.map_or(0, |g| g as usize)..self.mach_syms.len()
        }
    };
}
impl ObjectFile {
    symbol_ranges!();
    subsec_lookups!();

    /// The subsection MachSym `i` is defined in, and its offset there,
    /// as find_symbol_subsec finds them: None for a symbol not defined
    /// in a section, or in one that has no subsections (debug info).
    #[inline]
    pub fn symbol_subsec(&self, isecs: &[InputSection], i: usize) -> Option<(usize, u64)> {
        let id = self.sym_subsecs[i];
        (id != crate::symbol::NONE).then(|| {
            let id = id as usize;
            (id, self.mach_syms[i].value - isecs[id].input_addr as u64)
        })
    }
}
impl StagedObject {
    symbol_ranges!();
    subsec_lookups!();
}

/// Where the object's external symbols start in its MachSym array, or
/// None if the table is not partitioned locals-then-externals.
///
/// An object's LC_DYSYMTAB names the local, external-defined and
/// undefined runs; when they tile the table in that order (as ld64 and
/// clang always lay it out) the split is free, and the passes that
/// only want externals - resolution, weak-def coalescing, the intern
/// batch - or only locals - the anonymous symbol slots, the local
/// symtab - walk their half instead of testing every entry: mold's
/// first_global. Without a usable LC_DYSYMTAB the table is scanned once
/// and the split is used only if it really is partitioned.
fn first_global_of(mach_syms: &[MachSym], dysym: Option<&DysymtabCommand>) -> Option<u32> {
    let n = mach_syms.len() as u32;
    if let Some(d) = dysym
        && d.ilocalsym == 0
        && d.iextdefsym == d.nlocalsym
        && d.iundefsym == d.iextdefsym + d.nextdefsym
        && d.iundefsym + d.nundefsym == n
    {
        return Some(d.iextdefsym);
    }
    let is_local = |msym: &MachSym| msym.is_stab() || !msym.is_extern();
    let first = mach_syms.iter().position(|msym| !is_local(msym)).unwrap_or(mach_syms.len());
    mach_syms[first..].iter().all(|msym| !is_local(msym)).then_some(first as u32)
}

/// Which of an object's sections are left out: those with no bytes in
/// which no symbol naming a subsection is defined. Such a section makes
/// no output section and takes no part in ordering, in a final link and
/// in -r alike. The arm64 assembler's ltmpN label, which it puts at the
/// start of every section, names a subsection only in an object without
/// subsections, so it keeps an empty section there and nowhere else.
fn bare_sections(
    sect_hdrs: &[MachSection],
    mach_syms: &[MachSym],
    strtab: &'static [u8],
    split_ok: bool,
) -> Vec<bool> {
    let mut bare: Vec<bool> = sect_hdrs.iter().map(|s| s.size == 0).collect();
    for msym in mach_syms {
        if !msym.is_stab()
            && msym.ty() == N_SECT
            && let Some(b) = bare.get_mut((msym.sect as usize).wrapping_sub(1))
            && !(split_ok && symbol_name(strtab, msym).starts_with(b"ltmp"))
        {
            *b = false;
        }
    }
    bare
}

/// The load commands of an object that staging reads: its section
/// headers (every segment's sections in load command order, the ordinal
/// order MachSyms and relocations number them by), where its symbol table
/// is, and the per-object records the link carries along.
#[derive(Default)]
struct LoadCommands {
    sect_hdrs: Vec<MachSection>,
    symtab: Option<SymtabCommand>,
    dysymtab: Option<DysymtabCommand>,
    linker_options: Vec<Vec<Vec<u8>>>,
    platform_versions: Vec<PlatformVersion>,
    dice: Vec<(u32, u16, u16)>,
    loh: Vec<(u8, Vec<u64>)>,
}

impl LoadCommands {
    fn read<E: Target>(mf: &MappedFile) -> Self {
        let data = mf.data();
        let mut cmds = Self::default();
        for (cmd, bytes) in load_commands(data) {
            match cmd {
                LC_SEGMENT_64 => {
                    for mut sect in segment_sections(bytes) {
                        sect.flags =
                            canonical_section_flags(sect.segname(), sect.sectname(), sect.flags);
                        cmds.sect_hdrs.push(sect);
                    }
                }
                LC_SYMTAB => cmds.symtab = Some(SymtabCommand::read_from(bytes)),
                LC_DYSYMTAB => cmds.dysymtab = Some(DysymtabCommand::read_from(bytes)),
                cmd if is_platform_cmd(cmd) => {
                    let version = PlatformVersion::read(cmd, bytes, E::CPUTYPE);
                    cmds.platform_versions.push(version);
                }
                LC_LINKER_OPTION => {
                    // Auto-link requests: the object names libraries it
                    // needs, as NUL-terminated strings after a count -
                    // an option and its argument, if it takes one, and
                    // no more, as ld-prime sees it.
                    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
                    if !(1..=2).contains(&count) {
                        let file = mf.name.raw();
                        fatal!("{file}: LC_LINKER_OPTION has count={count}, only 1 or 2 is valid");
                    }
                    let mut strs = Vec::with_capacity(count as usize);
                    let mut p = 12;
                    for _ in 0..count {
                        let s = lc_string(bytes, p);
                        strs.push(s.to_vec());
                        p += s.len() as u32 + 1;
                    }
                    cmds.linker_options.push(strs);
                }
                LC_DATA_IN_CODE => {
                    let cmd = LinkEditDataCommand::read_from(bytes);
                    for i in 0..cmd.datasize as usize / 8 {
                        let p = cmd.dataoff as usize + i * 8;
                        cmds.dice.push((
                            u32::from_le_bytes(data[p..p + 4].try_into().unwrap()),
                            u16::from_le_bytes(data[p + 4..p + 6].try_into().unwrap()),
                            u16::from_le_bytes(data[p + 6..p + 8].try_into().unwrap()),
                        ));
                    }
                }
                LC_LINKER_OPTIMIZATION_HINT => {
                    // A stream of ULEB128 triples-and-more: kind, argument
                    // count, then that many instruction addresses.
                    let cmd = LinkEditDataCommand::read_from(bytes);
                    use crate::util::read_uleb;
                    let mut payload =
                        &data[cmd.dataoff as usize..(cmd.dataoff + cmd.datasize) as usize];
                    while !payload.is_empty() {
                        let kind = read_uleb(&mut payload);
                        if kind == 0 {
                            break;
                        }
                        let count = read_uleb(&mut payload);
                        let addrs = (0..count).map(|_| read_uleb(&mut payload)).collect();
                        cmds.loh.push((kind as u8, addrs));
                    }
                }
                _ => {}
            }
        }
        cmds
    }
}

/// Reads an object's symbol table: its MachSyms and string table. The
/// MachSym array is used straight from the mmap when it is 8-aligned
/// (ld64 aligns it; MachSym is #[repr(C)], all integer fields,
/// so any bytes are a valid value) - no copy of 16 bytes per symbol.
/// mold borrows its ElfSym array the same way (Cow, Owned only for
/// synthesized symbols).
fn read_symtab(
    data: &'static [u8],
    cmd: Option<&SymtabCommand>,
) -> (std::borrow::Cow<'static, [MachSym]>, &'static [u8]) {
    let Some(cmd) = cmd else {
        return (std::borrow::Cow::Borrowed(&[]), &[]);
    };
    let (off, n) = (cmd.symoff as usize, cmd.nsyms as usize);
    let mach_syms = match mach_syms_slice(data, off, n) {
        Some(s) => std::borrow::Cow::Borrowed(s),
        None => std::borrow::Cow::Owned(read_array(data, off, n)),
    };
    let strtab = &data[cmd.stroff as usize..(cmd.stroff + cmd.strsize) as usize];
    (mach_syms, strtab)
}

/// Which FDEs of an object's __eh_frame the link keeps.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeptFdes {
    /// Those of functions no compact unwind record covers.
    Uncovered,
    /// Every one (see Args::keeps_all_fdes).
    All,
    /// None: -no_dwarf_unwind leaves the DWARF unwind info out of the
    /// output, a -r one too. ld-prime keeps a function's DWARF-mode
    /// compact record all the same, its FDE offset 0.
    None,
}

impl KeptFdes {
    pub fn of(args: &crate::cmdline::Args) -> Self {
        if args.no_dwarf_unwind {
            KeptFdes::None
        } else if args.keeps_all_fdes() {
            KeptFdes::All
        } else {
            KeptFdes::Uncovered
        }
    }
}

/// Parses one object file without touching any linker state.
/// `relocatable` is set for a -r link.
pub fn stage_object<E: Target>(
    mf: &'static MappedFile,
    alive: bool,
    hidden: bool,
    priority: u32,
    relocatable: bool,
    kept_fdes: KeptFdes,
) -> StagedObject {
    let data = mf.data();
    let hdr = MachHeader::read_from(data);

    if hdr.cputype != E::CPUTYPE {
        fatal!("{}: incompatible CPU type: expected {}", mf.name.raw(), E::NAME);
    }

    let cmds = LoadCommands::read::<E>(mf);

    // The section headers are complete; leak them so subsections can
    // reference (not copy) their parent header. The leak is bounded by
    // the object's section count and lives for the whole link.
    let sect_hdrs: &'static [MachSection] = Vec::leak(cmds.sect_hdrs);

    let (mach_syms, strtab) = read_symtab(data, cmds.symtab.as_ref());
    let first_global = first_global_of(&mach_syms, cmds.dysymtab.as_ref());
    let nindirect = cmds.dysymtab.as_ref().map_or(0, |d| d.nindirectsyms);
    check_sections(sect_hdrs, nindirect, &mf.name);

    // ld-prime ignores a record shorter than its 8 bytes and reads a
    // longer one's first 8.
    let objc_image_info =
        sect_hdrs.iter().find(|s| is_objc_image_info(s) && s.size >= 8).map(|s| {
            let off = s.offset as usize + 4;
            let classes = sect_hdrs
                .iter()
                .any(|s| s.segname() == b"__DATA" && s.sectname() == b"__objc_classlist");
            ObjcImageInfo {
                flags: u32::from_le_bytes(data[off..off + 4].try_into().unwrap()),
                classes,
            }
        });
    let has_debug_info =
        sect_hdrs.iter().any(|s| s.segname() == b"__DWARF" && s.sectname() == b"__debug_info");

    let mut obj = StagedObject {
        mf,
        alive,
        hidden,
        priority,
        sect_hdrs,
        linker_options: cmds.linker_options,
        platform_versions: cmds.platform_versions,
        subsections_via_symbols: hdr.flags & MH_SUBSECTIONS_VIA_SYMBOLS != 0,
        isecs: Vec::new(),
        relocs: Vec::new(),
        subsecs: Vec::new(),
        sym_subsecs: Vec::new(),
        mach_syms,
        first_global,
        sym_names: Vec::new(),
        sym_hashes: Vec::new(),
        unwind: Vec::new(),
        cies: Vec::new(),
        fdes: Vec::new(),
        objc_image_info,
        has_debug_info,
        dice: cmds.dice,
        loh: cmds.loh,
    };

    let bare = bare_sections(sect_hdrs, &obj.mach_syms, strtab, obj.subsections_via_symbols);
    obj.demote_unnamed_subsec_names();
    obj.demote_thread_local_zerofill_names();
    let sect_isecs = obj.initialize_sections(&bare, relocatable);
    obj.read_symbol_names(strtab);
    obj.read_relocations::<E>(&bare, &sect_isecs);
    obj.check_init_pointers();
    obj.parse_unwind_info::<E>(kept_fdes);
    obj.find_symbol_subsecs();
    obj
}

impl StagedObject {
    /// Demotes the external symbols of the sections whose subsections
    /// ld-prime makes by content and names none of (see
    /// has_unnamed_subsecs and is_unnamed_objc_list) to locals that were
    /// private externals, as ld -r does: such a symbol defines nothing,
    /// so another object's reference to its name is undefined, another
    /// definition is no duplicate and no output lists it, while its own
    /// object's relocations still reach the subsection.
    fn demote_unnamed_subsec_names(&mut self) {
        let split = self.subsections_via_symbols;
        let unnamed: Vec<bool> = self
            .sect_hdrs
            .iter()
            .map(|h| is_unnamed_objc_list(h) || has_unnamed_subsecs(h, split))
            .collect();
        for msym in self.demote_externals_in(&unnamed) {
            msym.n_type = msym.n_type & !N_EXT | N_PEXT;
        }
    }

    /// Demotes the external symbols of the thread-local zero-fill
    /// sections (__thread_bss) to plain locals, as ld-prime does: such a
    /// symbol names a thread-local variable's initial storage, which
    /// only its own object's __thread_vars descriptor refers to, so
    /// another object's reference to its name is undefined, another
    /// definition is no duplicate, and the output, -r included, lists it
    /// as a non-external symbol, neither private external nor weak.
    fn demote_thread_local_zerofill_names(&mut self) {
        let zerofill: Vec<bool> =
            self.sect_hdrs.iter().map(|h| h.section_type() == S_THREAD_LOCAL_ZEROFILL).collect();
        for msym in self.demote_externals_in(&zerofill) {
            msym.n_type &= !(N_EXT | N_PEXT);
            msym.desc &= !(N_WEAK_DEF | N_WEAK_REF);
        }
    }

    /// The external symbols defined in the sections `demoted` marks (by
    /// ordinal), for the caller to make local. The table then no longer
    /// runs locals, then externals.
    fn demote_externals_in(&mut self, demoted: &[bool]) -> Vec<&mut MachSym> {
        if !demoted.contains(&true) {
            return Vec::new();
        }
        let is_demoted = |msym: &MachSym| {
            !msym.is_stab()
                && msym.is_extern()
                && msym.ty() == N_SECT
                && msym.sect != 0
                && demoted[msym.sect as usize - 1]
        };
        if !self.mach_syms.iter().any(is_demoted) {
            return Vec::new();
        }
        self.first_global = None;
        self.mach_syms.to_mut().iter_mut().filter(|msym| is_demoted(msym)).collect()
    }

    /// Splits each section into subsections, the Mach-O linking
    /// granularity, so that unreferenced pieces can later be
    /// dead-stripped: at its symbols (see symbol_split_points), or per
    /// element for a literal section, whose elements are merged by
    /// content across objects. A bare section's subsections start dead.
    /// Fills `isecs` and `subsecs` and returns each section's subsections
    /// as a range of `isecs`, by section ordinal; a section that is not
    /// copied through has none. `relocatable` is set for a -r link.
    fn initialize_sections(
        &mut self,
        bare: &[bool],
        relocatable: bool,
    ) -> Vec<std::ops::Range<usize>> {
        let data = self.mf.data();
        let sect_hdrs = self.sect_hdrs;
        let mut split_points = self.symbol_split_points();
        let mut sect_isecs = vec![0..0; sect_hdrs.len()];

        for (i, sect) in sect_hdrs.iter().enumerate() {
            // __eh_frame is re-synthesized from parsed CIE/FDE records, and
            // the __objc_imageinfo records are merged into one synthesized
            // record; neither is copied through. An image drops
            // __llvm_addrsig, and a -r output keeps it for the next link.
            if is_discarded_section(sect)
                || (sect.segname() == b"__TEXT" && sect.sectname() == b"__eh_frame")
                || is_objc_image_info(sect)
                || (!relocatable && is_llvm_addrsig(sect))
            {
                continue;
            }

            let mut points = if is_literal_section(sect) {
                literal_split_points(sect, data)
            } else {
                std::mem::take(&mut split_points[i])
            };
            // Each initializer, terminator or non-lazy symbol pointer is
            // a subsection of its own too, which ld-prime's diagnostics
            // name.
            if matches!(
                sect.section_type(),
                S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS | S_NON_LAZY_SYMBOL_POINTERS
            ) {
                points.extend((0..sect.size).step_by(8).map(|off| sect.addr + off));
            }
            points.push(sect.addr);
            points.retain(|&a| sect.addr <= a && a <= sect.addr + sect.size);
            points.sort_unstable();
            points.dedup();

            let record_p2align = record_p2align(sect, relocatable);
            let is_zerofill = sect.is_zerofill();

            let first = self.isecs.len();
            for (j, &start) in points.iter().enumerate() {
                let end = points.get(j + 1).copied().unwrap_or(sect.addr + sect.size);
                let contents: &[u8] = if is_zerofill {
                    &[]
                } else {
                    let lo = sect.offset as u64 + (start - sect.addr);
                    &data[lo as usize..(lo + (end - start)) as usize]
                };
                let size = end - start;
                let p2align = record_p2align.unwrap_or(sect.p2align as u8);
                self.isecs.push(InputSection {
                    input_addr: start as u32,
                    flags: if bare[i] {
                        InputSection::flags_dead()
                    } else if record_p2align.is_some() {
                        InputSection::flags_alive_no_modulus()
                    } else {
                        InputSection::flags_alive()
                    },
                    ..InputSection::new(u32::MAX, i as u32, p2align, size as u32, contents)
                });
            }
            sect_isecs[i] = first..self.isecs.len();
        }

        self.subsecs = (0..self.isecs.len() as u32).collect();
        self.subsecs.sort_by_key(|&id| self.isecs[id as usize].input_addr);
        sect_isecs
    }

    /// The addresses at which the object's symbols split each section
    /// (by ordinal) into subsections: every symbol's except an alternate
    /// entry point's (N_ALT_ENTRY), which labels a place inside another
    /// symbol's subsection. Without MH_SUBSECTIONS_VIA_SYMBOLS there are
    /// none, and each section stays whole.
    fn symbol_split_points(&self) -> Vec<Vec<u64>> {
        let mut points: Vec<Vec<u64>> = vec![Vec::new(); self.sect_hdrs.len()];
        if !self.subsections_via_symbols {
            return points;
        }
        for msym in self.mach_syms.iter() {
            if !msym.is_stab()
                && msym.ty() == N_SECT
                && msym.desc & N_ALT_ENTRY == 0
                && msym.sect >= 1
                && let Some(points) = points.get_mut(msym.sect as usize - 1)
            {
                points.push(msym.value);
            }
        }
        points
    }

    /// Reads each section's relocations and hands them to its
    /// subsections, rebasing each location to its subsection and each
    /// section-relative target to the subsection at that address.
    /// Sorted by offset, the relocations of one subsection are
    /// contiguous, so a single merge walk over the subsections hands
    /// each its run. The relocs go into one per-object arena and each
    /// subsection keeps a range into it (rel_offset/nrels) - sold's
    /// layout - so a debug link's millions of relocs are one
    /// allocation, not a Vec per subsection.
    fn read_relocations<E: Target>(
        &mut self,
        bare: &[bool],
        sect_isecs: &[std::ops::Range<usize>],
    ) {
        let sect_hdrs = self.sect_hdrs;
        for (i, sect) in sect_hdrs.iter().enumerate() {
            if sect_isecs[i].is_empty() || sect.nreloc == 0 {
                continue;
            }
            let mut rels = self.read_section_relocs::<E>(i);
            for rel in &rels {
                self.check_reloc_target(rel, bare);
            }
            // The sort must be stable: a SUBTRACTOR and the UNSIGNED it
            // pairs with share one offset and their order is the pairing
            // (Swift's relative pointers are all such pairs). An unstable
            // sort swapped some, leaving lone 4-byte UNSIGNED relocations
            // that were then written as 8 bytes.
            rels.sort_by_key(|rel| rel.offset);

            for rel in &mut rels {
                if let RelocTarget::Section(sect_pos) = rel.target() {
                    let (isec, offset) = self.section_target(sect_pos, rel.addend, sect_isecs);
                    rel.set_target(RelocTarget::Section(isec as u32));
                    rel.addend = offset as i64;
                }
            }

            let mut pos = 0;
            for sub in sect_isecs[i].clone() {
                let sub_off = (self.isecs[sub].input_addr as u64 - sect.addr) as u32;
                let end = sub_off + self.isecs[sub].size;
                let start = self.relocs.len();
                while pos < rels.len() && rels[pos].offset < end {
                    let mut rel = rels[pos];
                    rel.offset -= sub_off;
                    self.relocs.push(rel);
                    pos += 1;
                }
                self.isecs[sub].rel_offset = start as u32;
                self.isecs[sub].nrels = (self.relocs.len() - start) as u32;
            }
        }
    }

    /// Reads the relocations of section `i`.
    fn read_section_relocs<E: Target>(&self, i: usize) -> Vec<crate::input_sections::Reloc> {
        let sect = &self.sect_hdrs[i];
        let data = self.mf.data();
        let raw: Vec<MachRel> = read_array(data, sect.reloff as usize, sect.nreloc as usize);
        let contents = &data[sect.offset as usize..][..sect.size as usize];
        E::read_relocs(&self.mf.name, self.sect_hdrs, sect, contents, &raw)
    }

    /// Fails the link on a relocation to a section the link drops for
    /// having no bytes and no symbol to name a subsection there (see
    /// bare_sections): one naming it by its ordinal or by an arm64
    /// assembler's ltmpN label, as a reference to a label in an empty
    /// section is made. The target would have no address.
    fn check_reloc_target(&self, rel: &crate::input_sections::Reloc, bare: &[bool]) {
        let sect = match rel.target() {
            RelocTarget::Section(sect) => sect as usize,
            RelocTarget::Sym(idx) => match &self.mach_syms[idx as usize] {
                n if n.ty() == N_SECT => (n.sect as usize).wrapping_sub(1),
                _ => return,
            },
        };
        if bare.get(sect) == Some(&true) {
            let hdr = &self.sect_hdrs[sect];
            fatal!(
                "{}: relocation against empty section {},{}",
                self.mf.name.raw(),
                raw(hdr.segname()),
                raw(hdr.sectname())
            );
        }
    }

    /// The subsection at a section-relative relocation target, section
    /// `sect_pos` plus `addend`, and the target's offset within it. It
    /// is one of that section's: an address one past its end is its
    /// last subsection's, and one outside it (which read_relocs warns
    /// of) its first or last subsection's, as in ld-prime.
    fn section_target(
        &self,
        sect_pos: u32,
        addend: i64,
        sect_isecs: &[std::ops::Range<usize>],
    ) -> (usize, u64) {
        let sect = &self.sect_hdrs[sect_pos as usize];
        let addr = sect.addr.wrapping_add_signed(addend);
        let range = sect_isecs[sect_pos as usize].clone();
        if range.is_empty() {
            fatal!("{}: relocation against a discarded section", self.mf.name.raw());
        }
        let n = self.isecs[range.clone()].partition_point(|isec| isec.input_addr as u64 <= addr);
        let isec = range.start + n.saturating_sub(1);
        (isec, addr.wrapping_sub(self.isecs[isec].input_addr as u64))
    }

    /// Finds the subsection of each symbol defined in a section, once,
    /// in parallel with the other objects: every resolution round takes
    /// it from sym_subsecs. An ELF symbol names its section outright (as
    /// mold's resolve_symbol takes it), but a Mach-O symbol names a
    /// section that symbols split into subsections, and its own one
    /// takes a search (see find_symbol_subsec).
    fn find_symbol_subsecs(&mut self) {
        // The subsections' addresses in a row of their own, which the
        // searches go through rather than the subsections themselves.
        let addrs: Vec<u32> =
            self.subsecs.iter().map(|&id| self.isecs[id as usize].input_addr).collect();
        self.sym_subsecs = (self.mach_syms.iter())
            .map(|msym| {
                if msym.is_stab() || msym.ty() != N_SECT {
                    return crate::symbol::NONE;
                }
                let end = addrs.partition_point(|&a| a as u64 <= msym.value);
                let before = &self.subsecs[..end];
                symbol_subsec_before(&self.isecs, before, msym.sect, msym.value)
                    .map_or(crate::symbol::NONE, |(id, _)| id as u32)
            })
            .collect();
    }

    /// Records each symbol's name, and for an external symbol the hash
    /// its name is interned by; the interning itself happens at
    /// integration, in one batch for all objects.
    fn read_symbol_names(&mut self, strtab: &'static [u8]) {
        self.sym_names = self.mach_syms.iter().map(|msym| symbol_name(strtab, msym)).collect();
        self.sym_hashes = self
            .mach_syms
            .iter()
            .zip(&self.sym_names)
            .map(|(msym, name)| {
                if !msym.is_stab() && msym.is_extern() { crate::symbol::hash_key(name) } else { 0 }
            })
            .collect();
    }

    /// ld-prime warns of some sections of every object it parses -
    /// archive members the link doesn't use included: it drops each
    /// __LD section it doesn't know, and aligns the constants of a
    /// __DATA,__cfstring to a pointer whatever the section says.
    /// Staging runs in parallel, so reader::load_pending asks each
    /// object for the diagnostics after it, in input order.
    pub fn warn_about_sections(&self) {
        for (i, hdr) in self.sect_hdrs.iter().enumerate() {
            if is_unknown_ld_section(hdr) {
                crate::warn!(
                    "unknown section: __LD/{} in {}",
                    raw(hdr.sectname()),
                    self.mf.name.raw()
                );
            } else if hdr.segname() == b"__DATA"
                && hdr.sectname() == b"__cfstring"
                && hdr.p2align != 3
                && self.isecs.iter().any(|isec| isec.shndx == i as u32 && isec.is_alive())
            {
                crate::warn!(
                    "section __DATA/__cfstring is not pointer aligned in {}",
                    self.mf.name.raw()
                );
            }
        }
    }
}

/// The section a non-extern record `r` of object `file` refers to, and
/// the offset in it of `addr`, the address the record points at. The
/// section is the one its sect names (a 1-based ordinal), wherever
/// `addr` lies: only the ordinal tells apart sections that share an
/// address - an empty one and its successor, or one section's end and
/// the next one's start.
pub fn section_target(
    file: &Path,
    sections: &[MachSection],
    r: &MachRel,
    addr: u64,
) -> (RelocTarget, i64) {
    let i = (r.sect() as usize).wrapping_sub(1);
    let Some(sec) = sections.get(i) else {
        crate::fatal!("{}: bad relocation: {}", file.raw(), r.offset);
    };
    (RelocTarget::Section(i as u32), addr.wrapping_sub(sec.addr) as i64)
}

/// Whether a section's contents are fixed-shape records the linker
/// coalesces by content, as ld64 does: literal pools, literal pointers,
/// __cfstring, whose 32-byte CFString constants x86-64 compilers emit
/// without labels, and __objc_classrefs, __objc_superrefs,
/// __objc_protorefs and __got, whose pointers ld64 takes one by one
/// whatever the labels (an x86-64 -r output has none).
pub fn is_literal_section(sect: &MachSection) -> bool {
    matches!(
        sect.section_type(),
        S_CSTRING_LITERALS
            | S_4BYTE_LITERALS
            | S_8BYTE_LITERALS
            | S_16BYTE_LITERALS
            | S_LITERAL_POINTERS
    ) || (sect.segname() == b"__DATA"
        && (sect.sectname() == b"__cfstring" || is_pointer_list(sect)))
}

/// Whether ld-prime merges a section's subsections by their content - the
/// literal pools, C strings, selector references and CFStrings, not the
/// pointer lists it takes one by one - which the labels an assembler
/// makes for itself name none of (see InputSection::label).
pub fn has_merged_subsecs(sect: &MachSection) -> bool {
    is_literal_section(sect) && !(sect.segname() == b"__DATA" && is_pointer_list(sect))
}

/// Whether ld-prime merges a literal element with identical ones: a C
/// string of a section of any name, but a fixed-size record only of the
/// standard pool of its size, __TEXT,__literal4, __literal8 or
/// __literal16 of that type - its records in a section of another name
/// or type stay, however many copies there are. Nor does an element
/// that carries a relocation merge, as identical bytes may point at
/// different targets (ld-prime merges a __literal8 record by its bytes,
/// making every copy point where the first does).
///
/// __TEXT,__ustring, which holds the UTF-16 strings of CFString
/// constants (and C's u"" literals), is a regular section that ld-prime
/// cuts at its symbols, like ld64, but merges each subsection with
/// identical ones whatever labels it: every object that spells @"é" has
/// its own copy, and so its own CFString, which merges only once the
/// strings have (iTerm2's debug dylib had 67 CFStrings too many).
pub(crate) fn is_mergeable_literal(hdr: &MachSection, isec: &InputSection) -> bool {
    if isec.nrels != 0 {
        return false;
    }
    match hdr.section_type() {
        S_CSTRING_LITERALS => true,
        S_4BYTE_LITERALS => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__literal4"),
        S_8BYTE_LITERALS => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__literal8"),
        S_16BYTE_LITERALS => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__literal16"),
        S_REGULAR => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__ustring"),
        _ => false,
    }
}

/// Whether a __DATA section is one of pointers the linker takes one by
/// one: class, superclass and protocol references, or GOT slots.
fn is_pointer_list(sect: &MachSection) -> bool {
    matches!(
        sect.sectname(),
        b"__objc_classrefs" | b"__objc_superrefs" | b"__objc_protorefs" | b"__got"
    )
}

/// Where the elements of a literal section start: each NUL-terminated
/// string of a __cstring section (and the bytes after its last NUL, if
/// any), each fixed-size record of the others.
fn literal_split_points(sect: &MachSection, data: &[u8]) -> Vec<u64> {
    let elem_size = match sect.section_type() {
        S_CSTRING_LITERALS => {
            let contents = &data[sect.offset as usize..(sect.offset as u64 + sect.size) as usize];
            let mut points = Vec::new();
            let mut start = 0;
            while start < contents.len() {
                points.push(sect.addr + start as u64);
                match memchr::memchr(0, &contents[start..]) {
                    Some(len) => start += len + 1,
                    None => break,
                }
            }
            return points;
        }
        S_4BYTE_LITERALS => 4,
        S_8BYTE_LITERALS => 8,
        S_16BYTE_LITERALS => 16,
        // A literal-pointer section (__objc_selrefs) is one subsection
        // per pointer, as in ld64, so references to the same selector can
        // be coalesced across objects; so are the other pointer lists.
        S_LITERAL_POINTERS => 8,
        _ if is_pointer_list(sect) => 8,
        // __cfstring: one 32-byte constant per record.
        _ => 32,
    };
    (0..sect.size).step_by(elem_size).map(|o| sect.addr + o).collect()
}

impl StagedObject {
    /// Rebases the object's local indices to the global arenas, where
    /// it is object `obj_idx` and its subsections, CIEs and FDEs start at
    /// the given bases. `syms` maps its MachSyms to symbols, for the
    /// personality functions its unwind info names by MachSym index.
    fn rebase(
        &mut self,
        obj_idx: usize,
        isec_base: usize,
        cie_base: usize,
        fde_base: usize,
        syms: &[crate::symbol::SymbolId],
    ) {
        for isec in &mut self.isecs {
            isec.file = obj_idx as u32;
        }
        // Section relocation targets are object-local subsection
        // indices; rebase them to global once over the object's reloc
        // arena (rel_offset/nrels stay object-local).
        for rel in &mut self.relocs {
            if let RelocTarget::Section(local) = rel.target() {
                rel.set_target(RelocTarget::Section(isec_base as u32 + local));
            }
        }
        for sub in &mut self.subsecs {
            *sub += isec_base as u32;
        }
        for sub in self.sym_subsecs.iter_mut().filter(|sub| **sub != crate::symbol::NONE) {
            *sub += isec_base as u32;
        }
        for rec in &mut self.unwind {
            rec.isec += isec_base as u32;
            if rec.lsda_isec != UNWIND_NONE {
                rec.lsda_isec += isec_base as u32;
            }
            if rec.fde_idx != UNWIND_NONE {
                rec.fde_idx += fde_base as u32;
            }
            if rec.personality_sym != UNWIND_NONE {
                rec.personality_sym = syms[rec.personality_sym as usize];
            }
        }
        for cie in &mut self.cies {
            cie.obj = obj_idx as u32;
            if let Some(p) = &mut cie.personality {
                *p = syms[*p as usize];
            }
        }
        for fde in &mut self.fdes {
            fde.obj = obj_idx as u32;
            fde.isec += isec_base as u32;
            fde.cie += cie_base as u32;
            if let Some((lsda, _)) = &mut fde.lsda {
                *lsda += isec_base as u32;
            }
        }
    }

    /// The object file a rebased staged object becomes, once its
    /// subsections, unwind records, CIEs and FDEs have moved to the
    /// global arenas.
    fn into_object_file(self, symbols: Vec<crate::symbol::SymbolId>) -> ObjectFile {
        ObjectFile {
            mf: self.mf,
            is_reachable: self.alive,
            priority: self.priority,
            linker_options: self.linker_options,
            linker_options_read: false,
            platform_versions: self.platform_versions,
            hidden: self.hidden,
            subsections_via_symbols: self.subsections_via_symbols,
            sect_hdrs: std::borrow::Cow::Borrowed(self.sect_hdrs),
            relocs: self.relocs,
            subsecs: self.subsecs,
            sym_subsecs: self.sym_subsecs,
            objc_image_info: self.objc_image_info,
            has_debug_info: self.has_debug_info,
            mach_syms: self.mach_syms,
            first_global: self.first_global,
            symbols,
            lto_module: None,
            lto_output: false,
            dice: self.dice,
            loh: self.loh,
        }
    }
}

/// Where a staged object of a batch goes in the global arenas: the
/// first of its subsections, CIEs, FDEs, unwind records and local
/// symbols there, and the first of its globals' ids among those
/// interned for the batch. Prefix sums over the batch (see
/// integrate_objects).
#[derive(Clone, Copy)]
struct ArenaBases {
    isec: usize,
    cie: usize,
    fde: usize,
    unwind: usize,
    locals: usize,
    ids: usize,
}

impl StagedObject {
    /// How many local symbols (stabs included) the object has.
    fn num_locals(&self) -> usize {
        self.first_global.map_or_else(
            || self.mach_syms.iter().filter(|n| n.is_stab() || !n.is_extern()).count(),
            |g| g as usize,
        )
    }

    /// The symbol of each of the object's MachSyms: its locals' are the
    /// slots from `first_local` on, its globals' the `ids` interned for
    /// them, in order.
    fn symbol_ids(&self, first_local: usize, ids: &[SymbolId]) -> Vec<SymbolId> {
        let mut syms = Vec::with_capacity(self.mach_syms.len());
        let mut next_local = first_local as u32;
        let mut ids = ids.iter();
        for msym in self.mach_syms.iter() {
            if msym.is_stab() || !msym.is_extern() {
                syms.push(next_local);
                next_local += 1;
            } else {
                syms.push(*ids.next().unwrap());
            }
        }
        syms
    }

    /// Hands each subsection its run of the unwind records (see
    /// group_unwind_records), which start at `base` in the global arena.
    fn set_unwind_ranges(&mut self, base: usize) {
        let mut start = 0;
        for run in self.unwind.chunk_by(|a, b| a.isec == b.isec) {
            let isec = &mut self.isecs[run[0].isec as usize];
            isec.unwind_offset = (base + start) as u32;
            isec.nunwind = run.len() as u32;
            start += run.len();
        }
    }
}

/// Rebuilds each subsection's compact-unwind record range after the
/// records vector was compacted; the records stay grouped by
/// subsection, so one walk over runs restores every range.
pub fn refresh_unwind_ranges<E: Target>(ctx: &mut Context<E>) {
    let mut i = 0;
    while i < ctx.unwind_records.len() {
        let isec = ctx.unwind_records[i].isec;
        let start = i;
        while i < ctx.unwind_records.len() && ctx.unwind_records[i].isec == isec {
            i += 1;
        }
        ctx.isecs[isec as usize].unwind_offset = start as u32;
        ctx.isecs[isec as usize].nunwind = (i - start) as u32;
    }
}

/// Drops the unwind records and FDEs of the subsections that are no
/// longer alive, and refreshes the ranges of the records that stay.
pub fn remove_dead_unwind_info<E: Target>(ctx: &mut Context<E>) {
    // Remap the record-to-FDE links around the dropped FDEs.
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    let fdes = std::mem::take(&mut ctx.fdes);
    for (i, fde) in fdes.into_iter().enumerate() {
        if ctx.isecs[fde.isec as usize].is_alive() {
            fde_map[i] = kept_fdes.len();
            kept_fdes.push(fde);
        }
    }
    ctx.fdes = kept_fdes;
    let isecs = &ctx.isecs;
    let map = &fde_map;
    ctx.unwind_records.retain_mut(|rec| {
        if !isecs[rec.isec as usize].is_alive() {
            return false;
        }
        if rec.fde_idx != UNWIND_NONE {
            // usize::MAX (a dropped FDE) narrows to UNWIND_NONE.
            rec.fde_idx = map[rec.fde_idx as usize] as u32;
        }
        true
    });

    // The compaction moved the surviving records; refresh the ranges.
    refresh_unwind_ranges(ctx);
}

/// Points every symbol defined in a merged-away subsection at the
/// surviving one - mold makes the merged section's fragment the
/// symbol's origin - so a symbol's address never follows a replacement
/// chain. The copies are identical, so the symbol's offset is
/// unchanged. (Section-relative relocations still resolve through the
/// chain in InputSection::addr.)
pub(crate) fn redirect_symbols_to_replacements<E: Target>(ctx: &mut Context<E>) {
    let isecs = &ctx.isecs;
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if let Some(i) = sym.input_section() {
            let mut r = i as usize;
            while isecs[r].replacement != NO_REPLACEMENT {
                r = isecs[r].replacement as usize;
            }
            if r != i as usize {
                sym.set_input_section(Some(r as u32));
            }
        }
    });
}

/// Each staged object's place in the global arenas, after what they
/// hold already, for objects with `num_locals` local symbols and
/// `counts` globals each.
fn arena_bases<E: Target>(
    ctx: &Context<E>,
    staged: &[StagedObject],
    num_locals: &[usize],
    counts: &[usize],
) -> Vec<ArenaBases> {
    let mut next = ArenaBases {
        isec: ctx.isecs.len(),
        cie: ctx.cies.len(),
        fde: ctx.fdes.len(),
        unwind: ctx.unwind_records.len(),
        locals: ctx.symbols.syms.len(),
        ids: 0,
    };
    let mut bases = Vec::with_capacity(staged.len());
    for ((st, &nlocals), &nids) in staged.iter().zip(num_locals).zip(counts) {
        bases.push(next);
        next.isec += st.isecs.len();
        next.cie += st.cies.len();
        next.fde += st.fdes.len();
        next.unwind += st.unwind.len();
        next.locals += nlocals;
        next.ids += nids;
    }
    bases
}

/// Integrates a whole staging batch at once, mold-style: every
/// object's arena positions come from prefix sums over the batch (see
/// arena_bases), so the rebasing of indices - the actual work - runs on
/// all cores, and so does moving the rebased vectors into the global
/// arenas. `ids` are the symbols interned for the batch's globals,
/// `counts[i]` of them object i's. Subsections, unwind records, CIEs
/// and FDEs end up where integrate_object, one object at a time, would
/// put them.
pub fn integrate_objects<E: Target>(
    ctx: &mut Context<E>,
    mut staged: Vec<StagedObject>,
    ids: Vec<SymbolId>,
    counts: Vec<usize>,
) {
    // Counting an object's locals scans its MachSyms, so on a debug link
    // (millions of MachSyms) it runs in parallel; the prefix sums
    // themselves are a cheap serial walk.
    let num_locals: Vec<usize> = staged.par_iter().map(StagedObject::num_locals).collect();
    let bases = arena_bases(ctx, &staged, &num_locals, &counts);
    let obj_base = ctx.objs.len();

    // The rebasing, in parallel. Each object's MachSyms map to symbols
    // first: its locals to the slots its prefix sum reserved (they are
    // initialized below), its globals to the ids interned for the batch.
    let syms_of: Vec<Vec<SymbolId>> = (staged.par_iter_mut().zip(&bases).enumerate())
        .map(|(i, (st, base))| {
            let syms = st.symbol_ids(base.locals, &ids[base.ids..]);
            st.set_unwind_ranges(base.unwind);
            st.rebase(obj_base + i, base.isec, base.cie, base.fde, &syms);
            syms
        })
        .collect();

    // Local symbols initialize in parallel into pre-reserved disjoint
    // ranges - mold's ParallelSymbolAllocator contract: the arena is
    // sized up front, each object owns the exclusive range its prefix
    // sum assigned, and init writes every slot in it.
    let syms = &mut ctx.symbols.syms;
    let old_len = syms.len();
    let slots = spare_ranges(syms, &num_locals);
    staged.par_iter().zip(slots).for_each(|(st, slots)| {
        let r = st.local_range();
        let locals = (st.mach_syms[r.clone()].iter().zip(&st.sym_names[r]))
            .filter(|(msym, _)| msym.is_stab() || !msym.is_extern());
        for (slot, (_, name)) in slots.iter_mut().zip(locals) {
            slot.write(crate::symbol::Symbol::new(name));
        }
    });
    // SAFETY: the loop above initialized every slot spare_ranges handed
    // out, each object its own range.
    unsafe { syms.set_len(old_len + num_locals.iter().sum::<usize>()) };

    // Each object's staged vectors move into the arenas at the ranges
    // the prefix sums assigned, the same way, so hundreds of megabytes
    // of subsections move on all cores instead of one.
    append_in_parallel(&mut ctx.isecs, staged.iter_mut().map(|st| std::mem::take(&mut st.isecs)));
    append_in_parallel(
        &mut ctx.unwind_records,
        staged.iter_mut().map(|st| std::mem::take(&mut st.unwind)),
    );
    append_in_parallel(&mut ctx.cies, staged.iter_mut().map(|st| std::mem::take(&mut st.cies)));
    append_in_parallel(&mut ctx.fdes, staged.iter_mut().map(|st| std::mem::take(&mut st.fdes)));

    for (st, syms) in staged.into_iter().zip(syms_of) {
        ctx.objs.push(st.into_object_file(syms));
    }
}

/// Reserves room in an arena `v` for runs of `lens` elements after its
/// own and returns them, uninitialized, for the caller to fill in
/// parallel.
fn spare_ranges<'a, T>(v: &'a mut Vec<T>, lens: &[usize]) -> Vec<&'a mut [MaybeUninit<T>]> {
    crate::util::reserve_arena(v, lens.iter().sum());
    let mut spare = v.spare_capacity_mut();
    let mut ranges = Vec::with_capacity(lens.len());
    for &len in lens {
        let (range, rest) = spare.split_at_mut(len);
        ranges.push(range);
        spare = rest;
    }
    ranges
}

/// Appends `parts` to `v`, in order, moving the parts in parallel.
fn append_in_parallel<T: Send>(v: &mut Vec<T>, parts: impl Iterator<Item = Vec<T>>) {
    let parts: Vec<Vec<T>> = parts.collect();
    let lens: Vec<usize> = parts.iter().map(Vec::len).collect();
    let old_len = v.len();
    let ranges = spare_ranges(v, &lens);
    ranges.into_par_iter().zip(parts).for_each(|(range, part)| {
        for (slot, item) in range.iter_mut().zip(part) {
            slot.write(item);
        }
    });
    // SAFETY: every slot of the ranges was written above.
    unsafe { v.set_len(old_len + lens.iter().sum::<usize>()) };
}

/// Appends a staged object to the global arenas, rebasing its local
/// indices and interning its symbol names: integrate_objects for a
/// batch of one, done serially.
fn integrate_object<E: Target>(ctx: &mut Context<E>, mut staged: StagedObject) -> usize {
    let obj_idx = ctx.objs.len();
    let syms: Vec<SymbolId> = (staged.mach_syms.iter().zip(&staged.sym_names))
        .map(|(msym, name)| {
            if msym.is_stab() || !msym.is_extern() {
                ctx.symbols.add_local(name)
            } else {
                ctx.symbols.intern(name)
            }
        })
        .collect();
    staged.set_unwind_ranges(ctx.unwind_records.len());
    staged.rebase(obj_idx, ctx.isecs.len(), ctx.cies.len(), ctx.fdes.len(), &syms);
    ctx.isecs.append(&mut staged.isecs);
    ctx.unwind_records.append(&mut staged.unwind);
    ctx.cies.append(&mut staged.cies);
    ctx.fdes.append(&mut staged.fdes);
    ctx.objs.push(staged.into_object_file(syms));
    obj_idx
}

/// Parses one object and adds it to the link immediately.
pub fn parse_object<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    alive: bool,
) -> usize {
    let priority = ctx.next_priority();
    let kept_fdes = KeptFdes::of(&ctx.args);
    let staged = stage_object::<E>(mf, alive, false, priority, ctx.args.relocatable, kept_fdes);
    integrate_object(ctx, staged)
}

/// Extracts one NUL-terminated name from a string table: its bytes, any
/// but NUL, as ld-prime takes a symbol name, UTF-8 or not. The NUL scan
/// goes through memchr, which is vectorized.
fn symbol_name(strtab: &'static [u8], msym: &MachSym) -> &'static [u8] {
    let rest = strtab.get(msym.stroff as usize..).unwrap_or_default();
    memchr::memchr(0, rest).map_or(rest, |len| &rest[..len])
}

impl StagedObject {
    /// Reads the object's unwind info: the records of its
    /// __compact_unwind, and the CIEs of its __eh_frame with the FDEs
    /// that `kept_fdes` keeps, a function with only an FDE getting a
    /// record of its own. Each subsection's records end up in one run.
    fn parse_unwind_info<E: Target>(&mut self, kept_fdes: KeptFdes) {
        let sect_hdrs = self.sect_hdrs;
        if let Some(i) = sect_hdrs
            .iter()
            .position(|s| s.segname() == b"__LD" && s.sectname() == b"__compact_unwind")
        {
            let rels = self.read_section_relocs::<E>(i);
            self.parse_compact_unwind(i, &rels);
        }
        if kept_fdes != KeptFdes::None {
            if let Some(hdr) =
                sect_hdrs.iter().find(|s| s.segname() == b"__TEXT" && s.sectname() == b"__eh_frame")
            {
                self.parse_ehframe::<E>(hdr, kept_fdes == KeptFdes::All);
            }
            // A DWARF-mode record whose FDE never turned up describes
            // nothing.
            self.unwind.retain(|rec| {
                rec.encoding & UNWIND_MODE_MASK != E::UNWIND_MODE_DWARF || rec.fde().is_some()
            });
        }
        self.group_unwind_records();
        self.warn_unwind_outside_code();
    }

    /// Parses a __LD,__compact_unwind section into unwind records. The
    /// section is an array of 32-byte entries whose pointer fields are
    /// set by relocations, `rels` as read_relocs made them of the
    /// section's.
    ///
    /// Records that point to DWARF unwind info keep their DWARF-mode
    /// encoding; parse_ehframe attaches the FDE (a final link
    /// regenerates the encoding from it, a -r output copies the record
    /// as it came, like ld64). Object files usually don't contain such
    /// records, but `ld -r` output does.
    fn parse_compact_unwind(&mut self, sect: usize, rels: &[crate::input_sections::Reloc]) {
        const ENTRY_SIZE: usize = 32;
        let mf = self.mf;
        let hdr = &self.sect_hdrs[sect];
        // Diagnostics print the path as its bytes are.
        let file_name = mf.name.raw();
        let data = mf.data();
        let read_u32 = |off: usize| {
            let off = hdr.offset as usize + off;
            u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
        };
        // An entry holds the function's address, its length, the
        // encoding, the personality and the LSDA, at offsets 0, 8, 12, 16
        // and 24. The pointers are read through their relocations below.
        let num_entries = hdr.size as usize / ENTRY_SIZE;
        let mut records: Vec<UnwindRecord> = (0..num_entries)
            .map(|i| UnwindRecord {
                isec: u32::MAX,
                input_offset: 0,
                code_len: read_u32(i * ENTRY_SIZE + 8),
                encoding: read_u32(i * ENTRY_SIZE + 12),
                personality_sym: UNWIND_NONE,
                lsda_isec: UNWIND_NONE,
                lsda_off: 0,
                fde_idx: UNWIND_NONE,
            })
            .collect();

        let subsec_at = |addr: u64| self.find_subsec(&self.isecs, addr);

        // Only the pointer fields take relocations (on x86-64 a 4-byte
        // one as well as an 8-byte one).
        for r in rels {
            let field = r.offset as usize % ENTRY_SIZE;
            let rec = &mut records[r.offset as usize / ENTRY_SIZE];
            // The address a pointer field refers to, and the section
            // (1-based, as MachSyms count) it is in. For an extern
            // reference the target is this object's own definition,
            // located by its MachSym.
            let (sect_idx, addr) = match r.target() {
                RelocTarget::Sym(sym) => {
                    let msym = &self.mach_syms[sym as usize];
                    let sect_idx = if msym.ty() == N_SECT { msym.sect } else { 0 };
                    (sect_idx, msym.value.wrapping_add_signed(r.addend))
                }
                RelocTarget::Section(sect) => {
                    let addr = self.sect_hdrs[sect as usize].addr;
                    (sect as u8 + 1, addr.wrapping_add_signed(r.addend))
                }
            };

            match field {
                // The function the record covers, looked for in that
                // section as a label's place is: the section's end is its
                // last subsection's, not the next section's first one's.
                0 => {
                    let Some((isec, off)) = self.find_symbol_subsec(&self.isecs, sect_idx, addr)
                    else {
                        fatal!("{file_name}: __compact_unwind: bad function reference");
                    };
                    rec.isec = isec as u32;
                    rec.input_offset = off as u32;
                }
                // The personality function, recorded as a local symbol
                // index and mapped to a symbol at integration.
                16 => {
                    let sym = match r.target() {
                        RelocTarget::Sym(sym) => Some(sym as usize),
                        // Resolve a section-relative reference back to
                        // the symbol at that address.
                        RelocTarget::Section(_) => {
                            self.mach_syms.iter().position(|n| n.is_extern() && n.value == addr)
                        }
                    };
                    let Some(sym) = sym else {
                        fatal!("{file_name}: __compact_unwind: unsupported personality");
                    };
                    rec.personality_sym = sym as u32;
                }
                // The language-specific data area
                24 => {
                    let Some((isec, off)) = subsec_at(addr) else {
                        fatal!("{file_name}: __compact_unwind: bad LSDA reference");
                    };
                    rec.lsda_isec = isec as u32;
                    rec.lsda_off = off as u32;
                }
                _ => fatal!("{file_name}: __compact_unwind: unsupported relocation"),
            }
        }

        // A record no relocation gave a function describes nothing.
        records.retain(|rec| rec.isec != u32::MAX);
        self.unwind.extend(records);
    }

    /// Gathers each subsection's unwind records into one run, the range
    /// its unwind_offset and nunwind name. The records come in the order
    /// of __compact_unwind, then the FDE-only functions', so those of a
    /// subsection may lie apart: a section without subsections holds
    /// many functions, and a subsection's function may have a record
    /// and one of its alt entries an FDE alone. The runs keep the order
    /// of their first records, so records already in runs, as every
    /// compiler writes them, keep the input order a -r output copies.
    fn group_unwind_records(&mut self) {
        if self.unwind.is_empty() {
            return;
        }
        let mut rank = vec![u32::MAX; self.isecs.len()];
        let mut runs = 0;
        let mut grouped = true;
        for (i, rec) in self.unwind.iter().enumerate() {
            if i > 0 && self.unwind[i - 1].isec == rec.isec {
                continue;
            }
            let r = &mut rank[rec.isec as usize];
            if *r == u32::MAX {
                *r = runs;
                runs += 1;
            } else {
                grouped = false;
            }
        }
        if !grouped {
            self.unwind.sort_by_key(|rec| rank[rec.isec as usize]);
        }
    }
}

impl StagedObject {
    /// Parses a __TEXT,__eh_frame section. Unlike other sections it is not
    /// copied through: the linker re-synthesizes it, keeping only FDEs for
    /// functions that have no compact unwind record, patching each CIE's
    /// personality cell to be GOT-relative, and dropping the rest.
    fn parse_ehframe<E: Target>(&mut self, hdr: &MachSection, keep_all_fdes: bool) {
        let mf = self.mf;
        let data = mf.data();
        let rels: Vec<MachRel> = read_array(data, hdr.reloff as usize, hdr.nreloc as usize);

        // The records borrow from a processed copy of the section, leaked
        // once per object (like its section headers): the CIE/FDE bytes
        // then need no per-record copy, and they carry the pre-applied
        // relocations.
        let mut contents =
            data[hdr.offset as usize..(hdr.offset as u64 + hdr.size) as usize].to_vec();
        apply_eh_frame_relocs::<E>(&mut contents, &rels, &self.mach_syms, &mf.name);
        let contents: &'static [u8] = Vec::leak(contents);

        // Split the section into records: a zero ID marks a CIE, anything
        // else is an FDE whose ID is how far back its CIE is from the ID.
        let word = |pos: usize| u32::from_le_bytes(contents[pos..pos + 4].try_into().unwrap());
        let mut fdes: Vec<(u32, &'static [u8], u32)> = Vec::new();
        let mut personality_encs = Vec::new();
        let mut pos = 0;
        while pos < contents.len() {
            let rec: &'static [u8] = &contents[pos..pos + 4 + word(pos) as usize];
            let id = word(pos + 4);
            let input_addr = hdr.addr as u32 + pos as u32;
            if id == 0 {
                let (fde_enc, lsda_enc, personality_enc) = parse_cie_augmentation(rec, &mf.name);
                personality_encs.push(personality_enc);
                self.cies.push(CieRecord {
                    obj: u32::MAX,
                    input_addr,
                    data: rec,
                    personality: None,
                    personality_offset: 0,
                    fde_enc,
                    lsda_enc,
                    output_offset: 0,
                    is_alive: false,
                });
            } else {
                let cie_addr = (input_addr + 4).wrapping_sub(id);
                let Some(cie) = self.cies.iter().position(|c| c.input_addr == cie_addr) else {
                    fatal!("{}: __eh_frame: bad FDE pointer", mf.name.raw());
                };
                fdes.push((input_addr, rec, cie as u32));
            }
            pos += rec.len();
        }

        // The one relocation a CIE can have is its personality's: a
        // 4-byte pc-relative reference to its GOT slot (0x9b,
        // DW_EH_PE_indirect|pcrel|sdata4), which the linker rewrites
        // into the output's GOT (see chunks::eh_frame). It would write
        // any other wrong, as `.cfi_personality 0x10, sym` makes one.
        for r in &rels {
            let addr = hdr.addr as u32 + r.offset;
            let i = self.cies.partition_point(|c| c.input_addr <= addr);
            let Some(i) = i.checked_sub(1) else { continue };
            let cie = &mut self.cies[i];
            if addr >= cie.input_addr + cie.data.len() as u32 {
                continue;
            }
            const GOT_PCREL_SDATA4: u8 = DW_EH_PE_INDIRECT | DW_EH_PE_PCREL | DW_EH_PE_SDATA4;
            if r.ty() != E::RELOC_GOTPC
                || r.p2size() != 2
                || personality_encs[i] != Some(GOT_PCREL_SDATA4)
            {
                fatal!("{}: __eh_frame: unsupported personality reference", mf.name.raw());
            }
            // A local symbol index, mapped to a symbol at integration.
            cie.personality = Some(r.idx());
            cie.personality_offset = addr - cie.input_addr;
        }

        self.add_fdes::<E>(&fdes, keep_all_fdes);
    }

    /// Adds the FDEs of an __eh_frame, given as (input address, bytes,
    /// CIE index), and ties them to the functions' unwind records. A
    /// function that already has a compact unwind record doesn't need
    /// its FDE; the compact record wins. A DWARF-mode record is the
    /// exception: it exists to point at the FDE. `keep_all_fdes` keeps
    /// the FDEs of covered functions too: a -r output carries every
    /// input CIE and FDE through, as ld64's does, and a -static image or
    /// one linked with -no_compact_unwind has no __unwind_info for the
    /// compact record (which ld-prime drops, turning none into an FDE),
    /// and one for a macOS before 10.9 keeps them for its old unwinders
    /// (see Args::keeps_all_fdes); any other final image has no use for
    /// them.
    ///
    /// Only code is unwound: the FDE of a function in a section that
    /// is not is carried, but gives the function no entry of
    /// __unwind_info (see warn_unwind_outside_code).
    fn add_fdes<E: Target>(&mut self, fdes: &[(u32, &'static [u8], u32)], keep_all_fdes: bool) {
        let mut covered: std::collections::HashSet<(usize, u32)> = std::collections::HashSet::new();
        let mut dwarf_recs: std::collections::HashMap<(usize, u32), usize> =
            std::collections::HashMap::new();
        for (i, rec) in self.unwind.iter().enumerate() {
            if rec.encoding & UNWIND_MODE_MASK == E::UNWIND_MODE_DWARF {
                dwarf_recs.insert((rec.isec as usize, rec.input_offset), i);
            } else {
                covered.insert((rec.isec as usize, rec.input_offset));
            }
        }

        for &(input_addr, rec, cie) in fdes {
            // The function's address and size follow the CIE pointer, in
            // the CIE's encoding, then, if the CIE has an LSDA, the
            // augmentation data's length and the LSDA pointer. The
            // function's is relative to itself, as compilers write it.
            let enc = self.cies[cie as usize].fde_enc;
            let size = check_pointer_encoding(enc, &self.mf.name);
            let func_addr = read_pcrel(rec, 8, size, input_addr);
            // The size is in the same format, but absolute.
            let code_len = read_value(rec, 8 + size, size) as u32;
            let Some((isec, func_offset)) = self.find_subsec(&self.isecs, func_addr) else {
                fatal!("{}: __eh_frame: FDE for no function", self.mf.name.raw());
            };
            let func_offset = func_offset as u32;
            let sect = &self.sect_hdrs[self.isecs[isec].shndx as usize];
            let is_code = is_code_section(sect);
            let lsda = self.cies[cie as usize]
                .lsda_enc
                .and_then(|enc| self.fde_lsda(rec, input_addr, 8 + 2 * size, enc));

            let is_covered = covered.contains(&(isec, func_offset));
            if is_covered && !keep_all_fdes {
                continue;
            }

            let fde_idx = self.fdes.len();
            self.fdes.push(FdeRecord {
                obj: u32::MAX,
                input_addr,
                data: rec,
                cie,
                isec: isec as u32,
                func_offset,
                code_len,
                lsda,
                output_offset: 0,
            });

            // A covered function's compact record wins; its FDE is only
            // carried. Otherwise the object's own DWARF-mode record now
            // points at the FDE, or, for code, one is synthesized so that
            // the unwinder can find the FDE through __unwind_info. Of
            // several FDEs of a function, all carried, the last has the
            // record, as ld-prime's entry points at it.
            if is_covered {
                continue;
            }
            if let Some(&i) = dwarf_recs.get(&(isec, func_offset)) {
                self.unwind[i].fde_idx = fde_idx as u32;
                continue;
            }
            if !is_code {
                continue;
            }
            dwarf_recs.insert((isec, func_offset), self.unwind.len());
            self.unwind.push(UnwindRecord {
                isec: isec as u32,
                input_offset: func_offset,
                code_len,
                encoding: 0,
                personality_sym: UNWIND_NONE,
                lsda_isec: UNWIND_NONE,
                lsda_off: 0,
                fde_idx: fde_idx as u32,
            });
        }
    }

    /// Reads the LSDA pointer of an FDE `rec` at `input_addr` whose
    /// augmentation data's length is at `pos`, in encoding `enc`, and
    /// returns the subsection and offset it points to. As libunwind
    /// reads it, an FDE with no augmentation data or with a zero pointer
    /// has none (GCC writes one for a function with no LSDA under a CIE
    /// that declares them).
    fn fde_lsda(&self, rec: &[u8], input_addr: u32, pos: usize, enc: u8) -> Option<(u32, u32)> {
        let mut aug = &rec[pos..];
        if crate::util::read_uleb(&mut aug) == 0 {
            return None;
        }
        let pos = rec.len() - aug.len();
        let size = if enc & 0xf == DW_EH_PE_SDATA4 { 4 } else { 8 };
        if read_value(rec, pos, size) == 0 {
            return None;
        }
        check_pointer_encoding(enc, &self.mf.name);
        let addr = read_pcrel(rec, pos, size, input_addr);
        let Some((isec, off)) = self.find_subsec(&self.isecs, addr) else {
            fatal!("{}: __eh_frame: FDE for no LSDA", self.mf.name.raw());
        };
        Some((isec as u32, off as u32))
    }

    /// Warns of each section that has unwind info (compact or DWARF) but
    /// no code.
    fn warn_unwind_outside_code(&self) {
        let isecs = self.unwind.iter().map(|rec| rec.isec).chain(self.fdes.iter().map(|f| f.isec));
        let mut sects: Vec<u32> = isecs
            .map(|isec| self.isecs[isec as usize].shndx)
            .filter(|&shndx| !is_code_section(&self.sect_hdrs[shndx as usize]))
            .collect();
        sects.sort_unstable();
        sects.dedup();
        for shndx in sects {
            let sect = &self.sect_hdrs[shndx as usize];
            crate::warn!(
                "symbols in {},{} ({}) have unwind information, but it's not a code section",
                raw(sect.segname()),
                raw(sect.sectname()),
                self.mf.name.raw()
            );
        }
    }

    /// Fails the link on an initializer or terminator pointer, a
    /// subsection of its own, that has no relocation to name its
    /// function (`.quad 0` makes one): mold would either copy its bytes,
    /// an address nothing slides, or leave it out of __init_offsets.
    fn check_init_pointers(&self) {
        for isec in &self.isecs {
            let hdr = &self.sect_hdrs[isec.shndx as usize];
            if matches!(hdr.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS)
                && isec.size != 0
                && isec.nrels == 0
            {
                fatal!(
                    "{}:({},{}): initializer pointer without a relocation",
                    self.mf.name.raw(),
                    raw(hdr.segname()),
                    raw(hdr.sectname())
                );
            }
        }
    }
}

/// Whether a section holds code, which is what unwind info describes:
/// one with instructions, or __TEXT,__text.
fn is_code_section(hdr: &MachSection) -> bool {
    hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0
        || (hdr.segname(), hdr.sectname()) == (b"__TEXT", b"__text")
}

/// Pre-applies an __eh_frame's relocations to its contents, so that the
/// records' pointers become plain values: a SUBTRACTOR adds the next
/// relocation's target less its own, and an UNSIGNED of no pair adds
/// its target. Its GOT-relative relocations, a CIE's personality
/// reference, are left for parse_ehframe; there may be no other kind.
///
/// Either half of a pair may be non-extern, naming a section instead
/// of a symbol. The x86_64 assembler writes one for a label that no
/// named label precedes in its section (typically the CIE an FDE
/// points back at, once a named label starts the FDE) and folds the
/// label's address into the contents instead, so such a half adds
/// nothing here.
fn apply_eh_frame_relocs<E: Target>(
    contents: &mut [u8],
    rels: &[MachRel],
    mach_syms: &[MachSym],
    file_name: &Path,
) {
    let target = |r: MachRel| {
        if r.is_extern() { mach_syms[r.idx() as usize].value } else { 0 }
    };
    let mut i = 0;
    while i < rels.len() {
        let r = rels[i];
        let ty = r.ty();
        i += 1;
        let val = if ty == E::RELOC_SUBTRACTOR {
            i += 1;
            target(rels[i - 1]).wrapping_sub(target(r))
        } else if ty == E::RELOC_UNSIGNED {
            target(r)
        } else if ty == E::RELOC_GOTPC {
            continue;
        } else {
            fatal!("{}: unsupported relocation in __eh_frame: type={ty}", file_name.raw());
        };
        let loc = &mut contents[r.offset as usize..];
        if r.p2size() == 2 {
            let old = u32::from_le_bytes(loc[..4].try_into().unwrap());
            loc[..4].copy_from_slice(&old.wrapping_add(val as u32).to_le_bytes());
        } else {
            let old = u64::from_le_bytes(loc[..8].try_into().unwrap());
            loc[..8].copy_from_slice(&old.wrapping_add(val).to_le_bytes());
        }
    }
}

// DWARF pointer encodings (DW_EH_PE_*): the low four bits give the
// format, the next three what the value is relative to, and the top
// bit (DW_EH_PE_indirect) makes it the address of the pointer.
const DW_EH_PE_ABSPTR: u8 = 0x00;
pub(crate) const DW_EH_PE_SDATA4: u8 = 0x0b;
const DW_EH_PE_PCREL: u8 = 0x10;
const DW_EH_PE_INDIRECT: u8 = 0x80;

/// Fails the link on a function or LSDA pointer encoding, `enc`, other
/// than the ones compilers write: pc-relative, of 8 bytes
/// (DW_EH_PE_absptr, clang's) or 4 (DW_EH_PE_sdata4, GCC's). Returns
/// the size of a pointer.
fn check_pointer_encoding(enc: u8, file_name: &Path) -> usize {
    if enc == DW_EH_PE_PCREL {
        8
    } else if enc == DW_EH_PE_PCREL | DW_EH_PE_SDATA4 {
        4
    } else {
        fatal!("{}: __eh_frame: unsupported pointer encoding: 0x{enc:x}", file_name.raw())
    }
}

/// Reads the signed value of `size` bytes at `pos` of an __eh_frame
/// record.
fn read_value(rec: &[u8], pos: usize, size: usize) -> i64 {
    match size {
        4 => i32::from_le_bytes(rec[pos..pos + 4].try_into().unwrap()) as i64,
        _ => i64::from_le_bytes(rec[pos..pos + 8].try_into().unwrap()),
    }
}

/// The address a pc-relative pointer of `size` bytes at `pos` of an
/// __eh_frame record at `rec_addr` names.
fn read_pcrel(rec: &[u8], pos: usize, size: usize, rec_addr: u32) -> u64 {
    (rec_addr as u64 + pos as u64).wrapping_add_signed(read_value(rec, pos, size))
}

/// Reads a CIE's version and augmentation and returns how its FDEs
/// encode their function and their LSDA pointer (see
/// CieRecord::fde_enc and CieRecord::lsda_enc), and its personality
/// pointer. Fails the link on a version other than 1 or 3.
fn parse_cie_augmentation(data: &[u8], file_name: &Path) -> (u8, Option<u8>, Option<u8>) {
    // The version byte follows the length and the CIE ID, then the
    // augmentation string.
    let version = data[8];
    if version != 1 && version != 3 {
        fatal!("{}: __eh_frame: unsupported CIE version: {version}", file_name.raw());
    }
    let aug_start = 9;
    if data[aug_start] != b'z' {
        return (DW_EH_PE_ABSPTR, None, None);
    }
    let aug_end = aug_start + data[aug_start..].iter().position(|&b| b == 0).unwrap();
    // The code and data alignment factors, the return address register
    // and the augmentation data's length.
    let mut rest = &data[aug_end + 1..];
    for _ in 0..4 {
        crate::util::read_uleb(&mut rest);
    }
    let mut pos = data.len() - rest.len();
    let mut fde_enc = DW_EH_PE_ABSPTR;
    let mut lsda_enc = None;
    let mut personality_enc = None;
    for &c in &data[aug_start + 1..aug_end] {
        match c {
            b'L' => {
                lsda_enc = Some(data[pos]);
                pos += 1;
            }
            // The personality's encoding, then the pointer, whose value
            // its relocation gives.
            b'P' => {
                let enc = data[pos];
                personality_enc = Some(enc);
                pos += if enc & 0xf == DW_EH_PE_SDATA4 { 5 } else { 9 };
            }
            b'R' => {
                fde_enc = data[pos];
                pos += 1;
            }
            // The rest carry no augmentation data: 'S' marks a signal
            // frame and, on AArch64, 'B' return addresses signed with
            // the pointer-authentication B key and 'G' an MTE-tagged
            // frame. ld64 parses CIEs with libunwind, which ignores any
            // letter it does not know.
            _ => {}
        }
    }
    (fde_enc, lsda_enc, personality_enc)
}

/// Returns true if an object contains Objective-C class or category
/// metadata, which -ObjC forces to be linked from archives. ld64 also
/// counts Swift metadata (any __TEXT section named __swift*): a Swift
/// type with no Objective-C class list still registers with the
/// runtime through its type descriptors, and a Swift archive member
/// nobody references by symbol (iTerm2's libiTerm2SharedARC.a members
/// exported from the app's debug dylib) is only linked by this rule.
/// An __objc_imageinfo alone does not qualify.
pub fn has_objc_sections(mf: &MappedFile) -> bool {
    let data = mf.data();
    if data.len() < size_of::<MachHeader>() {
        return false;
    }
    if MachHeader::read_from(data).magic != MH_MAGIC_64 {
        return false;
    }
    section_headers(data).any(|sect| {
        matches!(
            sect.sectname(),
            b"__objc_classlist" | b"__objc_catlist" | b"__objc_nlclslist" | b"__objc_nlcatlist"
        ) || (sect.segname() == b"__TEXT" && sect.sectname().starts_with(b"__swift"))
    })
}

/// Ignores an input file without the link's architecture, as ld-prime
/// does: with a warning, or with an error under -arch_errors_fatal.
pub fn ignore_foreign_file<E: Target>(
    ctx: &Context<E>,
    mf: &MappedFile,
    why: &dyn std::fmt::Display,
) {
    if ctx.args.arch_errors_fatal {
        crate::error!("{why} in '{}'", mf.name.raw());
    } else {
        crate::warn!("ignoring file '{}': {why}", mf.name.raw());
    }
}

/// Ignores a fat file without a slice the link takes.
pub fn warn_fat_missing_arch<E: Target>(ctx: &Context<E>, mf: &MappedFile) {
    let arches = fat_arch_names(mf).join(",");
    let why = format!("fat file missing arch '{}', file has '{arches}'", E::NAME);
    ignore_foreign_file(ctx, mf, &why);
}

/// Whether a re-exported dylib at this install path may be bound to
/// directly: ld64's "public location" rule. /usr/lib/lib*.dylib (not
/// /usr/lib/system/) and a top-level /System/Library/Frameworks
/// framework are public; a private framework, a sub-framework or a
/// libSystem component is not, and its symbols bind to the dylib that
/// re-exports it (AppKit re-exports Foundation, public, and
/// UIFoundation, private: ld-prime binds NSHomeDirectory to Foundation
/// and NSAttachmentAttributeName to AppKit). A framework's binary is
/// told by its name: the path must end in the name before the first dot
/// after /System/Library/Frameworks/ (Foo for Foo.framework/Versions/A/
/// Foo), so a library inside one is not public either (OpenGL
/// re-exports Libraries/libGL.dylib, whose symbols bind to OpenGL).
pub fn is_public_location(install_name: &[u8]) -> bool {
    if let Some(rest) = install_name.strip_prefix(b"/usr/lib/") {
        return !rest.contains(&b'/');
    }
    if let Some(rest) = install_name.strip_prefix(b"/System/Library/Frameworks/")
        && let Some(dot) = memchr::memchr(b'.', rest)
    {
        let name = &rest[..dot];
        return install_name.len() > name.len()
            && install_name[install_name.len() - name.len() - 1] == b'/'
            && install_name.ends_with(name);
    }
    false
}

/// Notes an input file for -t as it is loaded. ld-prime names a fat
/// file's slice, and its members, by the file's own path.
pub fn trace_file<E: Target>(ctx: &mut Context<E>, name: &[u8]) {
    if ctx.args.trace {
        ctx.traced_files.push(without_fat_arch(name));
    }
}

/// Notes the file of a library loaded as another's re-export for the
/// -dependency_info file, which names it.
fn note_reexport_file<E: Target>(ctx: &mut Context<E>, path: &Path) {
    if ctx.args.dependency_info.is_some() {
        ctx.reexport_files.push(path.to_path_buf());
    }
}

/// Adds a library to the link: `dylib`, as a stub or a binary gives it
/// with its own exports, once the libraries it re-exports (`reexports`,
/// which may resolve to the inlined `documents`) have loaded, and with
/// the older libraries its exports move to for the link's target, which
/// its "$ld$..." names (`directives`) or the merged libraries' say.
fn add_library<E: Target>(
    ctx: &mut Context<E>,
    mut dylib: DylibFile,
    reexports: Vec<ReexportRef>,
    documents: Vec<Arc<StubLibrary>>,
    directives: LdDirectives,
) -> usize {
    // Named before the libraries it re-exports, which load now.
    dylib.priority = ctx.next_priority();
    let mut moved = load_reexports(ctx, &mut dylib, reexports, documents);
    moved.extend(directives.moved);
    dylib.moved_exports = add_moved_dylibs(ctx, &dylib.path, moved, &dylib.exports);
    if directives.renamed {
        dylib.name_source = NameSource::Directive;
    }
    dylib.dylib_idx = next_dylib_ordinal(ctx);
    add_dylib(ctx, dylib)
}

/// Loads the libraries `dylib` re-exports. A public one becomes an
/// implicit dylib of its own (its symbols bind to it), recursively
/// loading what it re-exports in turn; a private one is merged into
/// `dylib` - its exports join `dylib`'s, and its own re-exports are
/// walked the same way. A library may be a file of its own or a
/// document inlined in a stub (`documents`: the re-exporting stub's).
/// Returns the exports of the private ones that $ld$previous
/// directives move to older libraries.
fn load_reexports<E: Target>(
    ctx: &mut Context<E>,
    dylib: &mut DylibFile,
    reexports: Vec<ReexportRef>,
    documents: Vec<Arc<StubLibrary>>,
) -> Vec<MovedExport> {
    let mut walk = ReexportWalk { dylib, queue: reexports, pool: documents, moved: Vec::new() };
    let mut visited = std::collections::HashSet::new();
    while let Some(r) = walk.queue.pop() {
        if visited.insert(r.name.clone()) {
            walk.load(ctx, r);
        }
    }
    walk.moved
}

/// A library a dylib re-exports, as load_reexports walks it: its
/// install name, with the file of the library that names it and the
/// rpaths it resolves from, how many re-exports away from the dylib it
/// is and the library that re-exports it.
struct ReexportRef {
    name: Vec<u8>,
    loader: PathBuf,
    loader_rpaths: Vec<PathBuf>,
    hops: u32,
    via: Vec<u8>,
}

impl ReexportRef {
    /// The libraries that a library with `install_name`, `hops`
    /// re-exports away from the dylib, re-exports.
    fn of(
        names: Vec<Vec<u8>>,
        install_name: &[u8],
        loader: &Path,
        rpaths: &[PathBuf],
        hops: u32,
    ) -> Vec<Self> {
        let refs = names.into_iter().map(|name| ReexportRef {
            name,
            loader: loader.to_path_buf(),
            loader_rpaths: rpaths.to_vec(),
            hops: hops + 1,
            via: install_name.to_vec(),
        });
        refs.collect()
    }
}

/// A dylib's re-exported libraries as load_reexports walks them: the
/// dylib, which the private ones merge into and the public ones become
/// edges of; the libraries left to visit; the inlined documents a name
/// may resolve to (the dylib's, and those of every stub merged along
/// the way); and the merged libraries' exports that move to older
/// libraries.
struct ReexportWalk<'a> {
    dylib: &'a mut DylibFile,
    queue: Vec<ReexportRef>,
    pool: Vec<Arc<StubLibrary>>,
    moved: Vec<MovedExport>,
}

impl ReexportWalk<'_> {
    /// Loads a re-exported library: one the link has, matched by install
    /// name, else the file the name resolves to, else the document
    /// inlined for it. A public library is loaded from its file when one
    /// exists, as ld-prime does, and from its document otherwise; a
    /// private one inlined is merged from its document.
    fn load<E: Target>(&mut self, ctx: &mut Context<E>, r: ReexportRef) {
        let public = !ctx.args.no_implicit_dylibs && is_public_location(&r.name);
        // A library already in the link (libXCTestSwiftSupport re-exports
        // @rpath/XCTest.framework/..., which its own rpaths cannot reach
        // but -framework XCTest has loaded): its symbols bind to it if it
        // is public, else count as this dylib's.
        if let Some(idx) = ctx.dylibs.iter().position(|d| d.install_name == r.name) {
            if public {
                self.add_edge(idx, &r);
            } else {
                let loaded = &ctx.dylibs[idx];
                self.dylib.exports.extend(loaded.exports.iter().copied());
                self.dylib.tlv_exports.extend(loaded.tlv_exports.iter().copied());
                self.dylib.weak_exports.extend(loaded.weak_exports.iter().copied());
                self.dylib.merged_reexports.push(r.name);
            }
            return;
        }
        let inline = self.pool.iter().position(|d| d.identity.install_name == r.name);
        let on_disk = if inline.is_some() && !public {
            None
        } else {
            resolve_dylib_ref(ctx, &r.name, &r.loader, &r.loader_rpaths, inline.is_some())
        };
        match (on_disk, inline) {
            (Some(mf), _) => self.load_file(ctx, mf, r),
            (None, Some(i)) => self.load_inlined(ctx, i, r),
            (None, None) => crate::warn!(
                "ignoring missing indirect library: library for install name '{}' not found",
                crate::error::raw(&r.name)
            ),
        }
    }

    /// Loads a re-exported library from the stub document inlined for it
    /// at `pool[i]`.
    fn load_inlined<E: Target>(&mut self, ctx: &mut Context<E>, i: usize, r: ReexportRef) {
        // ld-prime names an inlined library by the file it would find
        // for it, where there is one.
        if ctx.args.trace || ctx.args.dependency_info.is_some() {
            let found = resolve_dylib_ref(ctx, &r.name, &r.loader, &r.loader_rpaths, true);
            if let Some(mf) = found {
                note_reexport_file(ctx, &mf.name);
            }
            let found = found.map(|mf| crate::util::path_bytes(&mf.name).to_vec());
            trace_file(ctx, found.as_deref().unwrap_or(&r.name));
        }
        let doc = self.pool[i].clone();
        if doc.identity.is_public(ctx) {
            let idx = register_tbd(ctx, &self.dylib.path, &doc, self.pool.clone());
            self.add_implicit(ctx, idx, &r);
            return;
        }
        self.merge_tbd(&doc, &[], &r.loader, &r.loader_rpaths, r.hops);
        self.dylib.merged_reexports.push(r.name);
    }

    /// Loads a re-exported library from the file `dep` its name resolves
    /// to. The file decides by its own install name, which a lookup by
    /// leaf name may find to differ from the one re-exported: ld-prime
    /// binds to libz a symbol of /opt/x/libz.dylib that it found as the
    /// SDK's /usr/lib/libz.1.dylib, and merges a /usr/lib/libq.dylib
    /// found as /opt/q/libq.dylib.
    fn load_file<E: Target>(
        &mut self,
        ctx: &mut Context<E>,
        dep: &'static MappedFile,
        r: ReexportRef,
    ) {
        // A file of another kind, which only a -dylib_file names, is
        // one ld-prime loads as any input.
        use crate::filetype::FileType;
        let ty = crate::filetype::get_file_type(dep);
        if !matches!(ty, FileType::Tapi | FileType::Dylib | FileType::Fat) {
            ctx.indirect_files.push(dep);
            return;
        }
        trace_file(ctx, crate::util::path_bytes(&dep.name));
        note_reexport_file(ctx, &dep.name);

        if ty == FileType::Tapi {
            let Some(stub) = load_tbd(ctx, dep) else { return };
            if stub.main.identity.is_public(ctx) {
                let idx = register_tbd_file(ctx, dep, &stub);
                self.add_implicit(ctx, idx, &r);
                return;
            }
            self.dylib.merged_reexports.push(stub.main.identity.install_name.clone());
            self.merge_tbd(&stub.main, &stub.documents, &dep.name, &[], r.hops);
            return;
        }

        // A universal binary (Xcode's XCTestCore, re-exported by XCTest)
        // is read for the target's slice, if it has one.
        let binary = match ty {
            FileType::Dylib => dep,
            _ => match fat_slice::<E>(&ctx.args, dep) {
                Some(slice) => slice,
                None => {
                    warn_fat_missing_arch(ctx, dep);
                    return;
                }
            },
        };
        let found = DylibIdentity::of_binary(binary);
        if found.is_public(ctx) {
            let idx = parse_dylib_binary(ctx, binary);
            self.add_implicit(ctx, idx, &r);
            return;
        }
        check_dylib_platform(ctx, binary);
        let mut dylib = read_dylib_binary(binary);
        self.moved.extend(interpret_binary_ld_symbols(ctx, &mut dylib).moved);
        self.merge_binary(dylib, &found.install_name, &dep.name, r.hops);
        self.dylib.merged_reexports.push(found.install_name);
    }

    /// Notes that the dylib re-exports the public library `dylib` of the
    /// link, as `r` reached it.
    fn add_edge(&mut self, dylib: usize, r: &ReexportRef) {
        let edge = ReexportEdge { dylib, hops: r.hops, via: r.via.clone() };
        self.dylib.reexported.push(edge);
    }

    /// Notes that the dylib re-exports the public library `dylib`, which
    /// the link has loaded for that alone.
    fn add_implicit<E: Target>(&mut self, ctx: &mut Context<E>, dylib: usize, r: &ReexportRef) {
        ctx.dylibs[dylib].is_implicit = true;
        self.add_edge(dylib, r);
    }

    /// Merges a private library of a stub, `hops` re-exports away, into
    /// the dylib: its exports join the dylib's, by kind, and those it
    /// moves to older libraries the walk's; the libraries the stub
    /// inlines (`documents`) join the pool, and those the library
    /// re-exports in turn the queue, to resolve from `loader` and
    /// `loader_rpaths`.
    fn merge_tbd(
        &mut self,
        lib: &StubLibrary,
        documents: &[Arc<StubLibrary>],
        loader: &Path,
        loader_rpaths: &[PathBuf],
        hops: u32,
    ) {
        let dylib = &mut *self.dylib;
        add_names(&mut dylib.exports, &lib.exports);
        add_names(&mut dylib.weak_exports, &lib.weak_exports);
        add_names(&mut dylib.tlv_exports, &lib.tlv_exports);
        self.moved.extend(lib.directives.moved.iter().cloned());
        self.pool.extend(documents.iter().cloned());
        let names = lib.tbd.reexports.iter().map(|name| name.to_vec()).collect();
        let name = lib.tbd.install_name;
        self.queue.extend(ReexportRef::of(names, name, loader, loader_rpaths, hops));
    }

    /// Merges what a private library's binary, `name`, contributes into
    /// the dylib likewise: the libraries it re-exports resolve from
    /// `loader`, the binary's file, and its rpaths.
    fn merge_binary(&mut self, dylib: DylibBinary, name: &[u8], loader: &Path, hops: u32) {
        self.dylib.exports.extend(dylib.exports);
        self.dylib.tlv_exports.extend(dylib.tlv_exports);
        let refs = ReexportRef::of(dylib.reexports, name, loader, &dylib.rpaths, hops);
        self.queue.extend(refs);
    }
}

/// Adds the names of a merged library to a set of the dylib it merges
/// into. A set smaller than the one it joins is the one added: the other
/// is copied whole, which hashes nothing (SwiftUI merges the 53,000
/// exports of SwiftUICore into its 21,000).
fn add_names(to: &mut hashbrown::HashSet<&'static [u8]>, from: &hashbrown::HashSet<&'static [u8]>) {
    if from.len() > to.len() {
        let smaller = std::mem::replace(to, from.clone());
        to.extend(smaller);
    } else {
        to.extend(from);
    }
}

/// A dylib's install name and who may link it directly: the umbrella it
/// belongs to (LC_SUB_FRAMEWORK, a stub's parent-umbrella) and the
/// clients it names (LC_SUB_CLIENT, allowable-clients).
#[derive(Clone)]
pub struct DylibIdentity {
    pub install_name: Vec<u8>,
    umbrella: Option<Vec<u8>>,
    clients: Vec<Vec<u8>>,
}

impl DylibIdentity {
    fn of_tbd(tbd: &tapi::TbdFile) -> Self {
        Self {
            install_name: tbd.install_name.to_vec(),
            umbrella: tbd.parent_umbrella.map(<[u8]>::to_vec),
            clients: tbd.allowable_clients.iter().map(|c| c.to_vec()).collect(),
        }
    }

    fn of_binary(mf: &MappedFile) -> Self {
        let mut id = Self { install_name: Vec::new(), umbrella: None, clients: Vec::new() };
        for (cmd, bytes) in load_commands(mf.data()) {
            // LC_SUB_FRAMEWORK and LC_SUB_CLIENT have the layout of
            // LC_LOAD_DYLINKER: a string's offset after the header.
            let string = |nameoff| lc_string(bytes, nameoff).to_vec();
            match cmd {
                LC_ID_DYLIB => id.install_name = string(DylibCommand::read_from(bytes).nameoff),
                LC_SUB_FRAMEWORK => {
                    id.umbrella = Some(string(DylinkerCommand::read_from(bytes).nameoff));
                }
                LC_SUB_CLIENT => id.clients.push(string(DylinkerCommand::read_from(bytes).nameoff)),
                _ => {}
            }
        }
        id
    }

    /// Whether a re-export of this library binds to it rather than to
    /// the re-exporter: ld64's public install name, which a library
    /// that names its clients never has (rdar://20627554).
    fn is_public<E: Target>(&self, ctx: &Context<E>) -> bool {
        !ctx.args.no_implicit_dylibs
            && is_public_location(&self.install_name)
            && self.clients.is_empty()
    }
}

/// The identity of the dylib in a stub or binary file.
pub fn dylib_identity<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> DylibIdentity {
    match crate::filetype::get_file_type(mf) {
        crate::filetype::FileType::Tapi => match read_stub(ctx, mf) {
            Some(stub) => stub.main.identity.clone(),
            None => DylibIdentity::of_tbd(&tapi::TbdFile::default()),
        },
        _ => DylibIdentity::of_binary(mf),
    }
}

/// Whether this link may name a dylib directly. ld-prime restricts only
/// a dylib that lists the clients it allows (SwiftUICore lists AppKit,
/// SwiftUI, UIKit and a few more): the output may link it if its client
/// name - -client_name, else the leaf of its install name (a dylib) or
/// path, less a "lib" prefix and cut at the first '.' or '_' - is the
/// dylib's umbrella or begins one of the clients (ld64's strncmp makes
/// it a prefix match), or if it is a sibling under the same -umbrella.
pub fn is_allowed_client<E: Target>(ctx: &Context<E>, dylib: &DylibIdentity) -> bool {
    if dylib.clients.is_empty() {
        return true;
    }
    let umbrella = dylib.umbrella.as_deref();
    if umbrella.is_some() && ctx.args.umbrella.as_deref() == umbrella {
        return true;
    }
    let name = match &ctx.args.client_name {
        Some(name) => name.clone(),
        None => {
            let path = match &ctx.args.install_name {
                Some(name) if ctx.args.output_type == MH_DYLIB => name.as_slice(),
                _ => crate::util::path_bytes(&ctx.args.output),
            };
            let leaf = path.rsplit(|&b| b == b'/').next().unwrap_or(path);
            let leaf = leaf.strip_prefix(b"lib").unwrap_or(leaf);
            let end = leaf.iter().position(|&b| b == b'.' || b == b'_').unwrap_or(leaf.len());
            leaf[..end].to_vec()
        }
    };
    umbrella == Some(name.as_slice()) || dylib.clients.iter().any(|c| c.starts_with(&name))
}

/// Checks that a dylib binary - a private re-export, whose exports merge
/// into its parent's, too - was built for the link's platform, and
/// returns the minimum OS version it names for that platform (0 for
/// none).
///
/// A macOS dylib marked MH_SIM_SUPPORT (-simulator_support) counts as
/// built for every simulator too, whose processes run on the Mac and may
/// load it, at no particular version.
fn check_dylib_platform<E: Target>(ctx: &Context<E>, mf: &MappedFile) -> u32 {
    let hdr = MachHeader::read_from(mf.data());
    let versions: Vec<PlatformVersion> = load_commands(mf.data())
        .filter(|&(cmd, _)| is_platform_cmd(cmd))
        .map(|(cmd, bytes)| PlatformVersion::read(cmd, bytes, hdr.cputype))
        .collect();
    if let Some(version) = versions.iter().find(|v| v.platform == ctx.args.platform) {
        return version.minos;
    }
    let mut platforms: Vec<u32> = versions.iter().map(|v| v.platform).collect();
    if hdr.flags & MH_SIM_SUPPORT != 0 && platforms.contains(&PLATFORM_MACOS) {
        platforms.extend([
            PLATFORM_IOSSIMULATOR,
            PLATFORM_WATCHOSSIMULATOR,
            PLATFORM_TVOSSIMULATOR,
            PLATFORM_VISIONOSSIMULATOR,
        ]);
    }
    check_dylib_platforms(ctx, mf, &platforms);
    0
}

/// Refuses a dylib built for `platforms`, none of them the link's,
/// which a firmware link takes with a warning.
fn check_dylib_platforms<E: Target>(ctx: &Context<E>, mf: &MappedFile, platforms: &[u32]) {
    if platforms.is_empty() || platforms.contains(&ctx.args.platform) {
        return;
    }
    let msg = format_args!(
        "building for '{}', but linking in dylib ({}) built for '{}'",
        platform_name(ctx.args.platform),
        mf.name.raw(),
        platforms_name(platforms),
    );
    if ctx.args.platform == PLATFORM_FIRMWARE {
        crate::warn!("{msg}");
    } else {
        crate::error!("{msg}");
    }
}

/// Adds a dylib binary to the link.
pub fn parse_dylib_binary<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) -> usize {
    let minos = check_dylib_platform(ctx, mf);
    let mut binary = read_dylib_binary(mf);
    if binary.install_name.is_empty() {
        fatal!("{}: dylib has no LC_ID_DYLIB", mf.name.raw());
    }
    let directives = interpret_binary_ld_symbols(ctx, &mut binary);
    // Each re-exported library keeps the referencing dylib's directory
    // and rpaths, since @loader_path and @rpath in an install name are
    // relative to the referrer.
    let reexports =
        ReexportRef::of(binary.reexports, &binary.install_name, &mf.name, &binary.rpaths, 0);
    let weak_exports: hashbrown::HashSet<&'static [u8]> = binary.weak_exports.into_iter().collect();
    let dylib = DylibFile {
        current_version: binary.current_version,
        compatibility_version: binary.compatibility_version,
        minos,
        exports: binary.exports.into_iter().collect(),
        has_weak_defs: !weak_exports.is_empty(),
        weak_exports,
        tlv_exports: binary.tlv_exports.into_iter().collect(),
        ..DylibFile::new(mf.name.clone(), binary.install_name)
    };
    add_library(ctx, dylib, reexports, Vec::new(), directives)
}

/// The defined external symbols of an image's symbol table, the run its
/// LC_DYSYMTAB names: each one's name, whether it is a weak definition
/// and whether it is a thread-local variable, which its section tells -
/// an S_THREAD_LOCAL_VARIABLES section, of the variables' descriptors.
fn defined_externals(data: &'static [u8]) -> Vec<(&'static [u8], bool, bool)> {
    let mut symtab = None;
    let mut dysymtab = None;
    for (cmd, bytes) in load_commands(data) {
        match cmd {
            LC_SYMTAB => symtab = Some(SymtabCommand::read_from(bytes)),
            LC_DYSYMTAB => dysymtab = Some(DysymtabCommand::read_from(bytes)),
            _ => {}
        }
    }
    let (Some(symtab), Some(dysym)) = (symtab, dysymtab) else {
        return Vec::new();
    };
    let (mach_syms, strtab) = read_symtab(data, Some(&symtab));
    let tlv_sects: Vec<u8> = section_headers(data)
        .enumerate()
        .filter(|(_, sect)| sect.section_type() == S_THREAD_LOCAL_VARIABLES)
        .map(|(i, _)| (i + 1) as u8)
        .collect();
    let range = dysym.iextdefsym as usize..(dysym.iextdefsym + dysym.nextdefsym) as usize;
    let defs = mach_syms[range].iter().map(|msym| {
        let weak = msym.desc & N_WEAK_DEF != 0;
        (symbol_name(strtab, msym), weak, tlv_sects.contains(&msym.sect))
    });
    defs.collect()
}

/// The ordinal the next LC_LOAD_DYLIB will have: dylibs are numbered
/// in load-command order, and a -bundle_loader has no load command.
fn next_dylib_ordinal<E: Target>(ctx: &Context<E>) -> i32 {
    ctx.dylibs.iter().filter(|d| !d.is_bundle_loader).count() as i32 + 1
}

/// Where an image keeps its export trie: LC_DYLD_EXPORTS_TRIE, or the
/// export section of LC_DYLD_INFO(_ONLY).
fn find_export_trie(data: &[u8]) -> Option<(usize, usize)> {
    let mut trie = None;
    for (cmd, bytes) in load_commands(data) {
        match cmd {
            LC_DYLD_EXPORTS_TRIE => {
                let cmd = LinkEditDataCommand::read_from(bytes);
                trie = Some((cmd.dataoff as usize, cmd.datasize as usize));
            }
            LC_DYLD_INFO | LC_DYLD_INFO_ONLY => {
                let cmd = DyldInfoCommand::read_from(bytes);
                if cmd.export_size != 0 {
                    trie = Some((cmd.export_off as usize, cmd.export_size as usize));
                }
            }
            _ => {}
        }
    }
    trie
}

/// The (name, flags) entries of an export trie. The trie is what dyld
/// binds against, and the one authoritative list of a dylib's exports:
/// a dylib's symbol table may keep only a handful of its defined
/// externals (Lottie.xcframework's ships 16 of 1846, the rest stripped),
/// so a linker that reads just the symbol table finds nothing to
/// resolve against. ld64 reads the trie.
fn export_trie_entries(data: &[u8], off: usize, size: usize) -> Vec<(&'static [u8], u64)> {
    let trie = &data[off..(off + size).min(data.len())];
    let mut names = Vec::new();
    let mut stack: Vec<(usize, Vec<u8>)> = vec![(0, Vec::new())];
    let read_uleb = |pos: &mut usize| -> u64 {
        let mut val = 0u64;
        let mut shift = 0;
        while *pos < trie.len() {
            let b = trie[*pos];
            *pos += 1;
            val |= ((b & 0x7f) as u64) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        val
    };
    while let Some((node, prefix)) = stack.pop() {
        if node >= trie.len() {
            continue;
        }
        let mut pos = node;
        let terminal = read_uleb(&mut pos) as usize;
        if terminal > 0 {
            let mut p = pos;
            let flags = read_uleb(&mut p);
            names.push((crate::util::leak_bytes(prefix.clone()), flags));
            pos += terminal;
        }
        let Some(&nchildren) = trie.get(pos) else { continue };
        pos += 1;
        for _ in 0..nchildren {
            let end = trie[pos..].iter().position(|&b| b == 0).map_or(trie.len(), |n| pos + n);
            let label = &trie[pos..end];
            pos = end + 1;
            let child = read_uleb(&mut pos) as usize;
            let mut name = prefix.clone();
            name.extend_from_slice(label);
            stack.push((child, name));
        }
    }
    names
}

/// Registers the -bundle_loader executable as the library the bundle's
/// remaining undefined symbols may resolve to. Like a dylib, minus the
/// install name; bound at run time as the main executable (ordinal 0)
/// and without a load command of its own. Exports come from the symbol
/// table's defined externals and the export trie (Xcode's test hosts
/// are linked with -export_dynamic, and an executable may be stripped).
pub fn parse_bundle_loader<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) -> usize {
    let data = mf.data();
    let mut exports: hashbrown::HashSet<&'static [u8]> = hashbrown::HashSet::new();
    let mut tlv_exports: hashbrown::HashSet<&'static [u8]> = hashbrown::HashSet::new();
    for (name, _, tlv) in defined_externals(data) {
        if tlv {
            tlv_exports.insert(name);
        }
        exports.insert(name);
    }
    if let Some((off, size)) = find_export_trie(data) {
        exports.extend(export_trie_entries(data, off, size).into_iter().map(|(name, _)| name));
    }

    let priority = ctx.next_priority();
    let install_name = crate::util::path_bytes(&mf.name).to_vec();
    let dylib = DylibFile {
        dylib_idx: BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE,
        is_bundle_loader: true,
        priority,
        exports,
        tlv_exports,
        ..DylibFile::new(mf.name.clone(), install_name)
    };
    add_dylib(ctx, dylib)
}

/// What a dylib binary says of itself: its install name and versions;
/// its exports - all of them, the weak and the thread-local ones again
/// by kind - and apart from them its "$ld$..." names (see
/// LdSymbols); and the install names it re-exports, with its rpaths,
/// resolved for its location, to look them up by.
#[derive(Default)]
pub(crate) struct DylibBinary {
    pub(crate) install_name: Vec<u8>,
    current_version: u32,
    compatibility_version: u32,
    pub(crate) exports: Vec<&'static [u8]>,
    weak_exports: Vec<&'static [u8]>,
    tlv_exports: Vec<&'static [u8]>,
    pub(crate) ld_symbols: Vec<&'static [u8]>,
    reexports: Vec<Vec<u8>>,
    rpaths: Vec<PathBuf>,
}

/// Whether a dylib is mergeable: -make_mergeable gave it its record
/// (LC_ATOM_INFO), which a stub never has.
pub fn is_mergeable(mf: &MappedFile) -> bool {
    if crate::filetype::get_file_type(mf) != crate::filetype::FileType::Dylib {
        return false;
    }
    load_commands(mf.data()).any(|(cmd, _)| cmd == LC_ATOM_INFO)
}

/// Whether an image - a dylib, an executable - has an LC_UUID. ld
/// -no_uuid makes one without, which dyld refuses to load and ld-prime
/// to link with.
pub fn has_uuid(data: &[u8]) -> bool {
    load_commands(data).any(|(cmd, _)| cmd == LC_UUID)
}

pub(crate) fn read_dylib_binary(mf: &'static MappedFile) -> DylibBinary {
    let data = mf.data();
    let mut dylib = DylibBinary {
        current_version: encode_version(1, 0, 0),
        compatibility_version: encode_version(1, 0, 0),
        ..Default::default()
    };
    for (cmd, bytes) in load_commands(data) {
        match cmd {
            LC_ID_DYLIB => {
                let cmd = DylibCommand::read_from(bytes);
                dylib.install_name = lc_string(bytes, cmd.nameoff).to_vec();
                dylib.current_version = cmd.current_version;
                dylib.compatibility_version = cmd.compatibility_version;
            }
            LC_REEXPORT_DYLIB => {
                let cmd = DylibCommand::read_from(bytes);
                dylib.reexports.push(lc_string(bytes, cmd.nameoff).to_vec());
            }
            LC_RPATH => {
                let cmd = DylinkerCommand::read_from(bytes);
                dylib.rpaths.push(loader_rpath(&mf.name, lc_string(bytes, cmd.nameoff)));
            }
            _ => {}
        }
    }

    let mut add = |name: &'static [u8], weak: bool, tlv: bool| {
        if name.starts_with(b"$ld$") {
            dylib.ld_symbols.push(name);
            return;
        }
        if weak {
            dylib.weak_exports.push(name);
        }
        if tlv {
            dylib.tlv_exports.push(name);
        }
        dylib.exports.push(name);
    };
    for (name, weak, tlv) in defined_externals(data) {
        add(name, weak, tlv);
    }
    if let Some((off, size)) = find_export_trie(data) {
        for (name, flags) in export_trie_entries(data, off, size) {
            let flags = flags as u32;
            let weak = flags & EXPORT_SYMBOL_FLAGS_WEAK_DEFINITION != 0;
            let tlv =
                flags & EXPORT_SYMBOL_FLAGS_KIND_MASK == EXPORT_SYMBOL_FLAGS_KIND_THREAD_LOCAL;
            add(name, weak, tlv);
        }
    }
    // Both the symbol table and the export trie name each directive;
    // ld-prime reads them by name.
    dylib.ld_symbols.sort_unstable();
    dylib.ld_symbols.dedup();
    dylib
}

/// Applies a dylib binary's "$ld$..." names (see LdSymbols) to it.
/// ld-prime knows fewer kinds of directive in a binary than TAPI does
/// in a stub: not $ld$compatibility_version.
fn interpret_binary_ld_symbols<E: Target>(
    ctx: &Context<E>,
    dylib: &mut DylibBinary,
) -> LdDirectives {
    let ld = LdSymbols::read(ctx, &dylib.ld_symbols);
    dylib.exports.retain(|n| ld.keeps(n));
    dylib.weak_exports.retain(|n| ld.keeps(n));
    dylib.tlv_exports.retain(|n| ld.keeps(n));
    dylib.exports.extend(&ld.added);
    if let Some(name) = ld.renamed_install_name() {
        dylib.install_name = name.to_vec();
    }
    if let Some(version) = ld.renamed_version() {
        dylib.current_version = version;
        dylib.compatibility_version = version;
    }
    ld.finish(dylib.current_version, dylib.compatibility_version)
}

/// A stub's library, read for the link's architecture and platform;
/// None if it has no target on the architecture.
pub(crate) fn read_tbd<E: Target>(
    ctx: &Context<E>,
    mf: &'static MappedFile,
) -> Option<tapi::TbdFile> {
    tapi::parse_cached(mf, E::NAME, ctx.args.platform)
}

/// A stub's libraries, read for the link (see StubLibrary): the
/// library itself and those it inlines, which its re-exports may
/// resolve to.
pub struct Stub {
    main: Arc<StubLibrary>,
    documents: Vec<Arc<StubLibrary>>,
}

impl Stub {
    /// The install names the stub's library re-exports.
    pub fn reexports(&self) -> &[&'static [u8]] {
        &self.main.tbd.reexports
    }

    /// Whether the stub inlines the library with `install_name`.
    pub fn inlines(&self, install_name: &[u8]) -> bool {
        self.documents.iter().any(|d| d.identity.install_name == install_name)
    }
}

/// A library of a stub - the stub's own or one it inlines - read for the
/// link's target: its "$ld$..." names applied (see interpret_ld_symbols)
/// and its exports gathered into the sets a DylibFile keeps. That is
/// most of the work of loading a stub, and it needs nothing of the link
/// but its target: the stubs are read on all cores ahead of the serial
/// loop loading them (see reader::prefetch_stubs), as mold parses its
/// shared libraries in parallel.
pub struct StubLibrary {
    /// Who the stub says the library is, before a directive renames it.
    identity: DylibIdentity,
    /// The library as its directives leave it, less its exports and the
    /// libraries it inlines.
    tbd: tapi::TbdFile,
    directives: LdDirectives,
    /// Its exports: all of them, and the weak and the thread-local ones
    /// again by kind.
    exports: hashbrown::HashSet<&'static [u8]>,
    weak_exports: hashbrown::HashSet<&'static [u8]>,
    tlv_exports: hashbrown::HashSet<&'static [u8]>,
}

impl StubLibrary {
    fn read<E: Target>(ctx: &Context<E>, mut tbd: tapi::TbdFile) -> Self {
        let identity = DylibIdentity::of_tbd(&tbd);
        let directives = interpret_ld_symbols(ctx, &mut tbd);
        let mut exports: hashbrown::HashSet<&'static [u8]> =
            std::mem::take(&mut tbd.exports).into_iter().collect();
        let weak_exports: hashbrown::HashSet<&'static [u8]> =
            tbd.weak_exports.iter().copied().collect();
        exports.extend(std::mem::take(&mut tbd.weak_exports));
        let tlv_exports: hashbrown::HashSet<&'static [u8]> =
            std::mem::take(&mut tbd.tlv_exports).into_iter().collect();
        exports.extend(tlv_exports.iter().copied());
        Self { identity, tbd, directives, exports, weak_exports, tlv_exports }
    }
}

/// A stub read for the link's target (see StubLibrary); None if it has
/// no target on the architecture. Memoized by the file's address and
/// the link's target, as tapi::parse_cached memoizes a parse.
pub fn read_stub<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> Option<Arc<Stub>> {
    type Cache = hashbrown::HashMap<(usize, &'static str, u32, u32), Option<Arc<Stub>>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (mf.data().as_ptr() as usize, E::NAME, ctx.args.platform, ctx.args.platform_minos);
    if let Some(stub) = CACHE.lock().unwrap().get_or_insert_with(Cache::new).get(&key) {
        return stub.clone();
    }
    let stub = tapi::parse(mf, E::NAME, ctx.args.platform).map(|mut tbd| {
        let documents = std::mem::take(&mut tbd.documents);
        let read = |tbd| Arc::new(StubLibrary::read(ctx, tbd));
        Arc::new(Stub { main: read(tbd), documents: documents.into_iter().map(read).collect() })
    });
    CACHE.lock().unwrap().get_or_insert_with(Cache::new).insert(key, stub.clone());
    stub
}

/// A stub to load, or None, with ld-prime's warning, if it has no
/// target on the architecture: the file is then ignored.
pub fn load_tbd<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> Option<Arc<Stub>> {
    let stub = read_stub(ctx, mf);
    if stub.is_none() {
        let path = mf.name.raw();
        let why =
            format_args!("tapi error: missing required architecture {} in file {path}", E::NAME);
        ignore_foreign_file(ctx, mf, &why);
    }
    stub
}

/// Adds a .tbd stub's library to the link; None if the stub has no
/// target on the link's architecture, and the link ignores it.
pub fn parse_tbd<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) -> Option<usize> {
    let stub = load_tbd(ctx, mf)?;
    Some(register_tbd_file(ctx, mf, &stub))
}

/// Registers the library of a stub file as a dylib of the link.
fn register_tbd_file<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    stub: &Stub,
) -> usize {
    check_dylib_platforms(ctx, mf, &stub.main.tbd.platforms);
    register_tbd(ctx, &mf.name, &stub.main, stub.documents.clone())
}

/// Registers a stub's library - a file's own, or a re-exported one
/// inlined in it - as a dylib of the link. `documents` are the inlined
/// libraries its re-exports may resolve to.
fn register_tbd<E: Target>(
    ctx: &mut Context<E>,
    path: &Path,
    lib: &StubLibrary,
    documents: Vec<Arc<StubLibrary>>,
) -> usize {
    let tbd = &lib.tbd;
    let names = tbd.reexports.iter().map(|name| name.to_vec()).collect();
    let reexports = ReexportRef::of(names, tbd.install_name, path, &[], 0);
    let dylib = DylibFile {
        current_version: tbd.current_version,
        compatibility_version: tbd.compatibility_version,
        minos: tbd.minos,
        exports: lib.exports.clone(),
        has_weak_defs: !lib.weak_exports.is_empty(),
        weak_exports: lib.weak_exports.clone(),
        tlv_exports: lib.tlv_exports.clone(),
        ..DylibFile::new(path.to_path_buf(), tbd.install_name.to_vec())
    };
    add_library(ctx, dylib, reexports, documents, lib.directives.clone())
}

/// Makes a dylib stand for each older library that exports of the
/// dylib at `path` move to, and returns which one each export binds to.
/// It has only the install name and the versions the directive gives:
/// ld-prime binds 81 of iTerm2's AppKit imports to /usr/lib/swift/
/// libswiftAppKit.dylib 1.0.0 for macOS 13, which has no stub for
/// arm64. It is one with a library of the link that has the install
/// name as its own only if both get a load command (see
/// dead_strip_dylibs).
fn add_moved_dylibs<E: Target>(
    ctx: &mut Context<E>,
    path: &Path,
    moved: Vec<MovedExport>,
    exports: &hashbrown::HashSet<&'static [u8]>,
) -> hashbrown::HashMap<&'static [u8], usize> {
    let mut moved_exports = hashbrown::HashMap::new();
    let mut targets: Vec<(&[u8], usize)> = Vec::new();
    for export in moved.into_iter().filter(|e| exports.contains(e.name)) {
        let idx = match targets.iter().find(|(name, _)| *name == export.install_name) {
            Some(&(_, idx)) => idx,
            None => {
                let priority = ctx.next_priority();
                let dylib = DylibFile {
                    current_version: export.current_version,
                    compatibility_version: export.compatibility_version,
                    dylib_idx: next_dylib_ordinal(ctx),
                    priority,
                    is_implicit: true,
                    name_source: NameSource::Moved,
                    ..DylibFile::new(path.to_path_buf(), export.install_name.to_vec())
                };
                let idx = add_dylib(ctx, dylib);
                targets.push((export.install_name, idx));
                idx
            }
        };
        moved_exports.insert(export.name, idx);
    }
    moved_exports
}

/// Adds a dylib a merged mergeable dylib links (see
/// reader::add_merged_dependencies), as one named on the command line
/// after the others, which has the exports the merged dylib's entries
/// import.
pub fn add_merged_dependency<E: Target>(ctx: &mut Context<E>, dep: crate::mergeable::Dependency) {
    let priority = ctx.next_priority();
    let weak_exports: hashbrown::HashSet<&'static [u8]> = dep.weak_exports.into_iter().collect();
    let dylib = DylibFile {
        current_version: dep.info.current_version,
        compatibility_version: dep.info.compatibility_version,
        dylib_idx: next_dylib_ordinal(ctx),
        priority,
        exports: dep.exports.into_iter().collect(),
        has_weak_defs: !weak_exports.is_empty(),
        weak_exports,
        ..DylibFile::new(dep.path, dep.info.install_name)
    };
    add_dylib(ctx, dylib);
}

/// Registers a dylib, deduplicating by install name: several libraries
/// (libc, libm, ...) are stubs for the same /usr/lib/libSystem.B.dylib,
/// and dyld refuses an image that lists one install name twice. The
/// first one registered speaks for them, unless a later one has the
/// install name as its own and the first by an $ld$previous directive
/// only: CoreLocation's stub decides its load command, not that of
/// _LocationEssentials, which it re-exports and which takes the name
/// CoreLocation before macOS 16. A dylib standing for a library that
/// exports moved to stays apart from the others (see add_moved_dylibs).
fn add_dylib<E: Target>(ctx: &mut Context<E>, dylib: DylibFile) -> usize {
    let is_moved = |d: &DylibFile| d.name_source == NameSource::Moved;
    let versions = |d: &DylibFile| (d.current_version, d.compatibility_version);
    if let Some(idx) = ctx.dylibs.iter().position(|d| {
        d.install_name == dylib.install_name
            && is_moved(d) == is_moved(&dylib)
            && (!is_moved(d) || versions(d) == versions(&dylib))
    }) {
        let existing = &mut ctx.dylibs[idx];
        if dylib.name_source < existing.name_source {
            existing.current_version = dylib.current_version;
            existing.compatibility_version = dylib.compatibility_version;
            existing.name_source = dylib.name_source;
        }
        existing.exports.extend(dylib.exports);
        existing.merged_reexports.extend(dylib.merged_reexports);
        existing.reexported.extend(dylib.reexported);
        existing.moved_exports.extend(dylib.moved_exports);
        return idx;
    }
    ctx.dylibs.push(dylib);
    ctx.dylibs.len() - 1
}

/// The symbol table's slots, for a parallel loop that writes each symbol
/// from one thread at most. (mold's SymbolEditor locks each symbol
/// instead, for loops in which files race to write one.)
pub struct SymbolSlots<'a> {
    ptr: *mut Symbol,
    _marker: std::marker::PhantomData<&'a mut [Symbol]>,
}

unsafe impl Sync for SymbolSlots<'_> {}

impl<'a> SymbolSlots<'a> {
    pub fn new(syms: &'a mut [Symbol]) -> Self {
        Self { ptr: syms.as_mut_ptr(), _marker: std::marker::PhantomData }
    }

    /// Symbol `id`.
    ///
    /// # Safety
    ///
    /// No other thread may access the symbol while the result lives.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self, id: SymbolId) -> &mut Symbol {
        // SAFETY: the caller has the symbol to itself.
        unsafe { &mut *self.ptr.add(id as usize) }
    }
}

impl ObjectFile {
    /// The rank of a definition: (class << 40) | (weak term << 32) |
    /// priority, lower is better. The classes are mold's
    /// (symbol_rank_from_fields):
    ///
    ///   0. a live file's strong definition
    ///   1. a live file's weak definition
    ///   2. a lazy archive member's or a dylib's strong definition
    ///   3. a lazy archive member's or a dylib's weak definition
    ///   4. a live file's tentative definition (a common symbol)
    ///   5. a lazy archive member's tentative definition
    ///
    /// so a strong definition in an archive or a dylib beats a weak one
    /// whatever their order, which only breaks ties, and a tentative
    /// definition loses to every real one: a live file's loads the
    /// member with a real definition (see passes::mark_live_objects),
    /// but no member for that member's own tentative definition. A live
    /// weak definition's rank carries the order in which ld-prime, like
    /// ld64, prefers the copies of one (see weak_definition_rank); the
    /// first copy wins only among equals. A lazy archive member from an
    /// archive that an auto-link option named, one of
    /// `autolink_priority` or later, comes after the libraries the
    /// command line's dylibs re-export (see passes::dylib_ranks).
    pub fn definition_rank(
        &self,
        isecs: &[InputSection],
        i: usize,
        autolink_priority: u32,
    ) -> Option<u64> {
        let msym = &self.mach_syms[i];
        if msym.is_stab() || !msym.is_extern() {
            return None;
        }
        let is_weak = msym.desc & N_WEAK_DEF != 0;
        let class: u64 = match msym.ty() {
            N_SECT | N_ABS if self.is_reachable && !is_weak => 0,
            N_SECT | N_ABS if self.is_reachable => 1,
            N_SECT | N_ABS if !is_weak => 2,
            N_SECT | N_ABS => 3,
            N_UNDF if msym.is_common() && self.is_reachable => 4,
            N_UNDF if msym.is_common() => 5,
            _ => return None,
        };
        let mut weak_term = 0u64;
        if class == 1
            && msym.ty() == N_SECT
            && let Some((isec, _)) = self.symbol_subsec(isecs, i)
        {
            weak_term = self.weak_definition_rank(&isecs[isec], msym);
        }
        let lazy = class == 2 || class == 3;
        let phase = if lazy && self.priority >= autolink_priority { 2 } else { 0 };
        Some((class << 40) | ((weak_term | phase) << 32) | self.priority as u64)
    }

    /// How ld-prime, like ld64, orders the copies of a weak definition,
    /// lower first: a copy that can't be auto-hidden before one that
    /// can (.weak_def_can_be_hidden, a global's N_WEAK_DEF |
    /// N_WEAK_REF), then a global before a private extern (unless both
    /// can be hidden), then the more aligned. A subsection's alignment
    /// is its section's with the subsection's address as the modulus,
    /// so a copy at 8 mod 16 is 8-aligned: a Swift metadata record
    /// comes at 16 from one object and at 8 from another, and the first
    /// copy wins only if equally aligned.
    fn weak_definition_rank(&self, isec: &InputSection, msym: &MachSym) -> u64 {
        let private = msym.n_type & N_PEXT != 0 || self.hidden;
        let auto_hide = !private && msym.desc & N_WEAK_REF != 0;
        let p2align = isec.p2align_at(msym.value) as u64;
        ((auto_hide as u64) << 7) | ((private as u64) << 6) | (63 - p2align)
    }

    /// Makes `sym` what MachSym `i` of this object, object `obj_idx`,
    /// the definition that won the race for it, defines. Returns false
    /// for a symbol in a section that was discarded (debug info), which
    /// resolves as if undefined.
    pub fn claim_definition(
        &self,
        sym: &mut Symbol,
        obj_idx: usize,
        i: usize,
        isecs: &[InputSection],
    ) -> bool {
        let msym = &self.mach_syms[i];
        sym.set_extern(true);
        sym.set_imported(false);
        sym.set_common(false);
        sym.set_weak_def(msym.desc & N_WEAK_DEF != 0);
        sym.set_private_extern(msym.n_type & N_PEXT != 0 || self.hidden);
        sym.set_no_dead_strip(msym.desc & (N_NO_DEAD_STRIP | REFERENCED_DYNAMICALLY) != 0);
        sym.set_referenced_dynamically(
            msym.ty() == N_SECT
                && msym.desc & (REFERENCED_DYNAMICALLY | N_WEAK_DEF) == REFERENCED_DYNAMICALLY,
        );
        sym.set_alt_entry(msym.desc & N_ALT_ENTRY != 0);

        let file = FileId::Obj(obj_idx as u32);
        match msym.ty() {
            N_ABS => {
                sym.set_file(file);
                sym.set_input_section(None);
                sym.value = msym.value;
            }
            N_SECT => {
                let Some((isec, off)) = self.symbol_subsec(isecs, i) else {
                    sym.clear_file();
                    return false;
                };
                sym.set_file(file);
                sym.set_input_section(Some(isec as u32));
                sym.value = off;
            }
            // A lazy member's tentative definition claims the symbol
            // for the member, for the liveness walk to load it for a
            // reference, but not for a live file's tentative definition
            // (see passes::mark_live_objects).
            N_UNDF if !self.is_reachable => {
                sym.set_file(file);
                sym.set_input_section(None);
                sym.set_common(true);
                sym.value = 0;
            }
            // A live common symbol takes a tentative claim.
            N_UNDF => {
                sym.clear_file();
                sym.set_common(true);
                sym.value = msym.value;
                sym.common_p2align = msym.common_p2align();
            }
            _ => unreachable!(),
        }
        true
    }

    /// Races the ranks of this object's definitions into `best`, the
    /// best rank of each symbol (see passes::race_definitions).
    pub fn race_definitions(
        &self,
        isecs: &[InputSection],
        autolink_priority: u32,
        best: &[AtomicU64],
    ) {
        use std::sync::atomic::Ordering;
        for i in self.global_range() {
            let sym_id = self.symbols[i];
            let rank = self.definition_rank(isecs, i, autolink_priority);
            if let Some(rank) = rank {
                best[sym_id as usize].fetch_min(rank, Ordering::Relaxed);
            }
        }
    }

    /// Writes the symbols whose race this object, object `obj_idx`, won
    /// (see passes::claim_definitions).
    pub fn claim_definitions(
        &self,
        syms: &SymbolSlots,
        obj_idx: usize,
        isecs: &[InputSection],
        autolink_priority: u32,
        best: &[AtomicU64],
    ) {
        use std::sync::atomic::Ordering;
        for i in self.global_range() {
            let sym_id = self.symbols[i];
            let rank = self.definition_rank(isecs, i, autolink_priority);
            let Some(rank) = rank else { continue };
            if best[sym_id as usize].load(Ordering::Relaxed) != rank {
                continue;
            }
            // SAFETY: this object holds the unique minimum rank for
            // sym_id, so no other thread writes this slot.
            let sym = unsafe { syms.get(sym_id) };
            if !self.claim_definition(sym, obj_idx, i, isecs) {
                best[sym_id as usize].store(u64::MAX, Ordering::Relaxed);
            }
        }
    }

    /// Gives each non-external symbol of this object, object `obj_idx`,
    /// its definition (see passes::claim_locals).
    pub fn initialize_local_symbols(
        &self,
        syms: &SymbolSlots,
        obj_idx: usize,
        isecs: &[InputSection],
    ) {
        for i in self.local_range() {
            let msym = &self.mach_syms[i];
            if msym.is_stab() || msym.is_extern() {
                continue;
            }
            // SAFETY: a local symbol belongs to this object alone (see
            // passes::claim_locals).
            let sym = unsafe { syms.get(self.symbols[i]) };
            let file = FileId::Obj(obj_idx as u32);
            match msym.ty() {
                N_ABS => {
                    sym.set_file(file);
                    sym.set_input_section(None);
                    sym.value = msym.value;
                }
                N_SECT => {
                    if let Some((isec, off)) = self.symbol_subsec(isecs, i) {
                        sym.set_file(file);
                        sym.set_input_section(Some(isec as u32));
                        sym.value = off;
                        sym.set_no_dead_strip(
                            msym.desc & (N_NO_DEAD_STRIP | REFERENCED_DYNAMICALLY) != 0,
                        );
                        sym.set_alt_entry(msym.desc & N_ALT_ENTRY != 0);
                    }
                }
                _ => {}
            }
        }
    }

    /// The subsections of this object, object `obj_idx`, that hold a
    /// losing copy of a weak definition, each with the winning copy's
    /// subsection, in MachSym order. The definition must be at the same
    /// offset in both copies, and the losing subsection hold no other
    /// symbol: an object without subsections-via-symbols has one
    /// subsection per section, and folding it away would take every
    /// other symbol's bytes with it. ld64 splits at symbols regardless;
    /// we keep such a copy.
    pub fn weak_def_losers<E: Target>(
        &self,
        ctx: &Context<E>,
        obj_idx: usize,
    ) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if !self.is_reachable {
            return out;
        }
        // The addresses of the object's symbols, sorted, once there is a
        // losing copy to check.
        let mut values: Option<Vec<u64>> = None;
        for i in self.global_range() {
            let (msym, sym_id) = (&self.mach_syms[i], self.symbols[i]);
            if !msym.is_weak_def() {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            let Some(FileId::Obj(owner)) = sym.file() else { continue };
            if owner as usize == obj_idx {
                continue;
            }
            let Some(winner) = sym.input_section() else { continue };
            let Some((loser, off)) = self.symbol_subsec(&ctx.isecs, i) else { continue };
            if off != sym.value {
                continue;
            }
            let values = values.get_or_insert_with(|| {
                let mut v: Vec<u64> = (self.mach_syms.iter())
                    .filter(|n| !n.is_stab() && n.ty() == N_SECT)
                    .map(|n| n.value)
                    .collect();
                v.sort_unstable();
                v.dedup();
                v
            });
            let l = &ctx.isecs[loser];
            let (start, end) = (l.input_addr as u64, l.input_addr as u64 + l.size as u64);
            let lo = values.partition_point(|&v| v < start);
            let hi = values.partition_point(|&v| v < end);
            if values[lo..hi].iter().all(|&v| v == msym.value) {
                out.push((loser, winner as usize));
            }
        }
        out
    }

    /// The imports this object references, each once, and whether
    /// weakly (its undefined symbol is N_WEAK_REF).
    pub fn import_references<E: Target>(&self, ctx: &Context<E>) -> Vec<(SymbolId, bool)> {
        use crate::input_sections::RelocTarget;
        let mut seen = hashbrown::HashSet::new();
        let mut out = Vec::new();
        for &id in &self.subsecs {
            for rel in ctx.isecs[id].rels(self) {
                let RelocTarget::Sym(idx) = rel.target() else { continue };
                let sym_id = self.symbols[idx as usize];
                if ctx.symbols[sym_id].is_imported() && seen.insert(sym_id) {
                    out.push((sym_id, self.mach_syms[idx as usize].desc & N_WEAK_REF != 0));
                }
            }
        }
        out
    }
}
