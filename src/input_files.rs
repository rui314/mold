//! Input file parsing: object files, dylib stubs and archives.

use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::context::Context;
use crate::fatal;
use crate::input_sections::InputSection;
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::symbol::SymbolId;
use crate::tapi;
use crate::target::{BadReloc, RelocError, Target};

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
        let hdr = MachHeader::read_from(data);
        let mut off = size_of::<MachHeader>();
        for _ in 0..hdr.ncmds {
            let lc = LoadCommand::read_from(&data[off..]);
            if is_platform_cmd(lc.cmd) {
                return Some(Self::read(lc.cmd, &data[off..], hdr.cputype));
            }
            off += lc.cmdsize as usize;
        }
        None
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
    pub fn of_triple(triple: &str) -> Option<Self> {
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

/// A relocatable object file.
#[derive(Debug)]
pub struct ObjectFile {
    pub mf: &'static MappedFile,
    /// False for an archive member no live code needs (yet). Dead
    /// files' subsections never reach the output.
    pub is_alive: bool,
    /// Position in input order, for resolution tie-breaking: the
    /// earlier file wins.
    pub priority: u32,
    /// LC_LINKER_OPTION auto-link requests, acted on only if the file
    /// is live.
    pub linker_options: Vec<Vec<Vec<u8>>>,
    /// Whether linker_options have been read (see
    /// passes::read_linker_options): what is left are the libraries to
    /// link.
    pub linker_options_read: bool,
    /// Platforms and minimum OS versions from LC_BUILD_VERSION or
    /// LC_VERSION_MIN_*. Checked only after archive selection.
    pub platform_versions: Vec<PlatformVersion>,
    /// -hidden-l: this file's external definitions become private
    /// externals.
    pub hidden: bool,
    /// MH_SUBSECTIONS_VIA_SYMBOLS was set: symbols split the sections
    /// into atoms. A -r output carries the flag only if every input
    /// had it.
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
    /// The flags word of the object's __objc_imageinfo, if it has one.
    pub objc_image_info: Option<u32>,
    /// True if the object carries DWARF debug info, so the output gets
    /// debug stabs pointing back at it.
    pub has_debug_info: bool,
    /// The __compact_unwind pointer fields a 4-byte relocation set (see
    /// StagedObject::unwind_ptr32), by global subsection.
    pub unwind_ptr32: Vec<(u32, u32, u8)>,
    /// The labels of __compact_unwind records (see
    /// StagedObject::unwind_labels), by global subsection.
    pub unwind_labels: Vec<(u32, u32, u32)>,
    /// For a bitcode input, the lto_module handle: the object is a
    /// placeholder that only claims symbols until LTO compiles it.
    pub lto_module: Option<usize>,
    pub nlists: std::borrow::Cow<'static, [NList]>,
    /// Index of the first external nlist, if the table is partitioned
    /// locals-then-externals (see first_global_of).
    pub first_global: Option<u32>,
    /// The symbol slot for each nlist entry.
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
            is_lto_output: false,
        }));
        Self {
            mf,
            is_alive: true,
            priority: 0,
            linker_options: Vec::new(),
            linker_options_read: false,
            platform_versions: Vec::new(),
            hidden: false,
            subsections_via_symbols: true,
            sect_hdrs: std::borrow::Cow::Owned(Vec::new()),
            relocs: Vec::new(),
            subsecs: Vec::new(),
            objc_image_info: None,
            has_debug_info: false,
            unwind_ptr32: Vec::new(),
            unwind_labels: Vec::new(),
            lto_module: None,
            nlists: std::borrow::Cow::Owned(Vec::new()),
            first_global: None,
            symbols: Vec::new(),
            dice: Vec::new(),
            loh: Vec::new(),
        }
    }

    /// The subsection - ld64's atom - holding a linker optimization
    /// hint's instructions, if they are where ld64 takes a hint: one to
    /// three of them, 4-byte aligned, in one subsection of code and
    /// within 64 KiB of each other. ld64 drops any other hint as it
    /// reads the object. Without MH_SUBSECTIONS_VIA_SYMBOLS a section
    /// is one subsection here, but ld64 still cuts its atoms at every
    /// symbol, so a hint may not span one.
    pub fn hint_subsec(&self, isecs: &[InputSection], addrs: &[u64]) -> Option<usize> {
        let lo = *addrs.iter().min()?;
        let hi = *addrs.iter().max()?;
        let (id, _) = find_subsec(isecs, &self.subsecs, lo)?;
        let isec = &isecs[id];
        let is_code = self.sect_hdrs[isec.shndx as usize].flags & S_ATTR_PURE_INSTRUCTIONS != 0;
        let spans_symbol = || {
            self.nlists.iter().any(|nlist| {
                !nlist.is_stab()
                    && nlist.n_type() == N_SECT
                    && nlist.n_sect as u32 == isec.shndx + 1
                    && lo < nlist.n_value
                    && nlist.n_value <= hi
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

/// Whether ld64 names no atom after a label (its ignoreLabel): in a
/// section of C strings or of 4-, 8- or 16-byte literals, which it
/// splits into one atom per literal, a private label (see
/// is_private_label) names nothing, and the literal is known by its
/// contents or size.
pub fn is_ignored_literal_label(section_type: u32, name: &str) -> bool {
    matches!(
        section_type,
        S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS
    ) && is_private_label(name)
}

/// Whether a label is one a compiler or assembler makes for itself: an
/// assembler temporary (L...) or a linker-private label (l...) - the
/// compiler's lCPI0_0 constant-pool and l_.str string labels, the arm64
/// assembler's ltmpN.
pub fn is_private_label(name: &str) -> bool {
    name.starts_with('L') || name.starts_with('l')
}

/// Whether a section is one ld-prime reads as a list of records -
/// CFStrings, UTF-16 strings, selector and class references, Objective-C
/// class and category lists - whose atoms no local symbol names: they
/// are "anon" in its diagnostics and -map. The UTF-16 strings of an
/// object without subsections (`split` false) are one atom, named by
/// its labels.
pub fn is_record_list(hdr: &MachSection, split: bool) -> bool {
    hdr.section_type() == S_LITERAL_POINTERS
        || matches!(
            hdr.sectname(),
            "__cfstring"
                | "__objc_classrefs"
                | "__objc_classlist"
                | "__objc_nlclslist"
                | "__objc_catlist"
                | "__objc_nlcatlist"
        )
        || split && hdr.sectname_is("__ustring")
}

/// How ld-prime prefers a symbol at an atom's start to name the atom in
/// a diagnostic: an exported one before a private extern, a local, a
/// weak definition and an ltmpN label; among equals, the greatest name.
pub fn atom_name_rank(nlist: &NList, name: &str) -> u8 {
    if name.starts_with("ltmp") {
        0
    } else if nlist.n_desc & N_WEAK_DEF != 0 {
        1
    } else if !nlist.is_extern() {
        2
    } else if nlist.n_type & N_PEXT != 0 {
        3
    } else {
        4
    }
}

/// Reports a relocation record ld-prime rejects, in its words. `atom`
/// is the name of the atom holding it, and `bounds` the atom's place
/// in the section.
fn report_bad_reloc(file: &Path, nsects: usize, bad: &BadReloc, atom: &str, bounds: (u32, u32)) {
    let r = &bad.rel;
    let fields = || {
        format!(
            "r_address=0x{:X}, r_type={}, r_extern={}, r_pcrel={}, r_length={}",
            r.r_address,
            r.r_type(),
            r.is_extern() as u8,
            r.is_pcrel() as u8,
            r.r_length()
        )
    };
    let name = file.display();
    match bad.error {
        RelocError::OutOfBounds => {
            report_out_of_bounds(file, 1 << r.r_length(), r.r_address, bounds)
        }
        // The first word of a scattered record holds, from the least
        // significant bit, address:24, type:4, length:2, pcrel:1 and
        // the scattered bit.
        RelocError::Scattered => crate::error!(
            "scattered relocation in '{atom}' is not supported: r_address=0x{:X}, r_type={}, \
             r_pcrel={}, r_length={} in '{name}'",
            r.r_address & 0xff_ffff,
            (r.r_address >> 24) & 0xf,
            (r.r_address >> 30) & 1,
            (r.r_address >> 28) & 3
        ),
        RelocError::Unsupported => {
            crate::error!("relocation in '{atom}' is not supported: {} in '{name}'", fields())
        }
        RelocError::Invalid(what) => crate::error!("{what}: {} in '{name}'", fields()),
        RelocError::SymbolOutOfRange => {
            crate::error!("r_symbolnum={} out of range in '{name}'", r.r_symbolnum())
        }
        RelocError::SectionOutOfRange => {
            crate::error!("sectionNum={} out of range (size={nsects}) in '{name}'", r.r_section())
        }
    }
}

/// Reports a relocated field of `size` bytes at `offset` in a section
/// that runs past the end of its atom, which spans `bounds`.
fn report_out_of_bounds(file: &Path, size: u8, offset: u32, bounds: (u32, u32)) {
    crate::error!(
        "{size} byte relocaton at r_address (0x{offset:04X}) is not fully within bounds of atom \
         0x{:04X}->0x{:04X} in '{}'",
        bounds.0,
        bounds.1,
        file.display()
    );
}

/// A subsection's relocations, sliced from its object's reloc arena.
/// A free function (not a Context method) so callers already holding a
/// borrow of `ctx.isecs` can pass `&ctx.objs` alongside an `&isec`.
pub fn isec_relocs_of<'a>(
    objs: &'a [ObjectFile],
    isec: &InputSection,
) -> &'a [crate::input_sections::Reloc] {
    let off = isec.rel_offset as usize;
    &objs[isec.file as usize].relocs[off..off + isec.nrels as usize]
}

/// Finds the subsection containing `addr` among `subsecs` (sorted by
/// input address), returning it with the offset within it.
pub fn find_subsec(
    isecs: &[InputSection],
    subsecs: &[crate::input_sections::InputSectionId],
    addr: u64,
) -> Option<(usize, u64)> {
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

/// Finds the subsection a symbol at `addr` in section `n_sect` (1-based,
/// as nlists count) belongs to, returning it with the offset within it.
/// The section decides where addresses alone can't: a label on an empty
/// section starts where the next section does, and one past a section's
/// last byte (an array's `_end`) ends where the next one starts; both
/// belong to their own section, as in ld-prime.
pub fn find_symbol_subsec(
    isecs: &[InputSection],
    subsecs: &[crate::input_sections::InputSectionId],
    n_sect: u8,
    addr: u64,
) -> Option<(usize, u64)> {
    let shndx = u32::from(n_sect).wrapping_sub(1);
    let end = subsecs.partition_point(|&id| isecs[id as usize].input_addr as u64 <= addr);
    // The nearest subsection of the section starting at or before
    // `addr`; any in between belong to empty sections at that address.
    let id =
        subsecs[..end].iter().rev().map(|&id| id as usize).find(|&id| isecs[id].shndx == shndx)?;
    let isec = &isecs[id];
    // A label may sit at a section's end, except in one of fixed-size
    // records, where it names no record (see extraneous_labels).
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
    /// Read from a Mach-O file, not a .tbd stub. (A mergeable dylib
    /// records whether every input was split into atoms by its
    /// symbols, which a dylib's header never says.)
    pub from_binary: bool,
    /// The 1-based ordinal used to refer to this dylib in bind records;
    /// BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE (-1) for a -bundle_loader.
    pub dylib_idx: i32,
    /// The -bundle_loader executable: its symbols bind to the main
    /// executable at run time and it gets no LC_LOAD_DYLIB.
    pub is_bundle_loader: bool,
    /// Position in input order, for resolution tie-breaking.
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
    /// ordinal 0 in their n_desc), through whose re-exports dyld finds
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
    /// Named by -lazy-l, -lazy_library or -lazy_framework, whatever the
    /// deployment target. Below macOS 27 such a dylib loads as any
    /// other, but its load command follows those of the other libraries
    /// the command line names (see passes::dead_strip_dylibs).
    pub named_lazily: bool,
    /// -delay-l and the like, or a public library such a dylib
    /// re-exports: dyld runs its initializers only when the image
    /// dlopen()s this install name (its own, or the re-exporting
    /// dylib's), before the first use of one of its symbols (see
    /// delay_init::create_delay_init).
    pub delay_init: Option<Vec<u8>>,
    /// For a library loaded as a public re-export that the command line
    /// or an auto-link option names later, where it is named: its
    /// position among the inputs and the path given, by which ld-prime's
    /// -map lists it.
    pub named_at: Option<(u32, PathBuf)>,
    /// Loaded through an object's LC_LINKER_OPTION rather than the
    /// command line: a hint, so ld64 gives it a load command only if
    /// something binds to it.
    pub is_autolinked: bool,
    /// Loaded because a dylib on the command line (or auto-linked)
    /// re-exports it and it lives in a public location: symbols found
    /// through the re-export bind to it directly, and it gets a load
    /// command after the explicitly named libraries if anything binds
    /// to it. Private re-exported libraries are not loaded this way;
    /// their symbols bind to the re-exporting dylib.
    pub is_implicit: bool,
    /// Load-command order: the sequence in which command-line and
    /// auto-linked libraries were named (u32::MAX for implicit ones,
    /// which follow, sorted by install name).
    pub load_order: u32,
    pub exports: hashbrown::HashSet<&'static str>,
    /// Exports that are weak definitions: binding to one sets
    /// MH_BINDS_TO_WEAK on the client image.
    pub weak_exports: hashbrown::HashSet<&'static str>,
    /// Whether the library itself, not one it re-exports, exports weak
    /// definitions, which ld-prime says keep it from being delayed
    /// (see passes::name_dylib).
    pub has_weak_defs: bool,
    /// The subset of exports that are thread-local variables.
    pub tlv_exports: hashbrown::HashSet<&'static str>,
    /// The install names of the private libraries this dylib re-exports,
    /// whose exports are merged into its own.
    pub merged_reexports: Vec<Vec<u8>>,
    /// With -map, those libraries as the files ld-prime reads them from,
    /// to which it attributes the symbols they define.
    pub merged_files: Vec<MergedFile>,
    /// Exports (its own or merged ones) that per-symbol $ld$previous
    /// directives move to older libraries for the link's target, each
    /// with the index of the dylib that stands for the library it binds
    /// to instead (see add_moved_dylibs).
    pub moved_exports: hashbrown::HashMap<&'static str, usize>,
    /// The files of libraries auto-link options named that merged into
    /// this one, with their naming sequence numbers (see note_naming).
    pub named_files: Vec<(u32, PathBuf)>,
    /// Whose install name it has: its own or an older library's.
    pub name_source: NameSource,
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

/// A private library a dylib re-exports, merged into it: the file
/// ld-prime reads it from (the stub's own inlined document names none),
/// with its exports.
#[derive(Debug)]
pub struct MergedFile {
    pub install_name: Vec<u8>,
    pub path: PathBuf,
    pub exports: Vec<&'static str>,
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
    hdr.segname() == "__DWARF" || hdr.segname() == "__LD"
}

/// The alignment of every record of a section of fixed-size records (see
/// record_size), as ld-prime gives its atoms: with no modulus, each
/// record starting at a multiple of it whatever its offset in the
/// input, and mostly the section's own. A literal is aligned to its
/// size: compilers emit __literal16 with p2align 3 for a 16-byte
/// constant whose type is only 8-aligned, and rely on the linker to
/// place it where a 16-byte load can reach it. An initializer,
/// terminator or non-lazy symbol pointer, a GOT slot of any type (see
/// fold_input_got), a CFString constant and a pointer-auth slot are
/// aligned to a pointer, even from a section that claims less or more,
/// in a -r output as in an image; a thread-local variable descriptor
/// (from a section clang aligns to a byte) to a pointer in an image,
/// and in a -r output to at least one.
fn record_p2align(hdr: &MachSection, relocatable: bool) -> Option<u8> {
    let size = record_size(hdr)?;
    let p2align = hdr.p2align as u8;
    Some(match hdr.section_type() {
        S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS => size.trailing_zeros() as u8,
        S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS | S_NON_LAZY_SYMBOL_POINTERS => 3,
        S_THREAD_LOCAL_VARIABLES if relocatable => p2align.max(3),
        S_THREAD_LOCAL_VARIABLES => 3,
        _ if hdr.segname() == "__DATA"
            && matches!(hdr.sectname(), "__cfstring" | "__auth_ptr" | "__got") =>
        {
            3
        }
        _ => p2align,
    })
}

/// The size of each record of a section ld-prime splits into fixed-size
/// atoms. Its name decides for the __DATA segment's GOT and Objective-C
/// lists, whatever their type, and for a CFString, pointer-auth or
/// compact unwind section of the regular type; its type does for the
/// others: literals, pointers to initializers, terminators or GOT
/// slots, and thread-local variable descriptors (three pointers).
pub(crate) fn record_size(hdr: &MachSection) -> Option<u64> {
    let regular = hdr.section_type() == S_REGULAR;
    match (hdr.segname(), hdr.sectname()) {
        (
            "__DATA",
            "__got" | "__objc_classlist" | "__objc_catlist" | "__objc_catlist2"
            | "__objc_clsrolist" | "__objc_nlclslist" | "__objc_nlcatlist" | "__objc_protolist"
            | "__objc_selrefs" | "__objc_classrefs" | "__objc_superrefs" | "__objc_protorefs",
        ) => return Some(8),
        ("__DATA", "__auth_ptr") if regular => return Some(8),
        ("__DATA", "__cfstring") | ("__LD", "__compact_unwind") if regular => return Some(32),
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

/// Reports the first section of an object ld-prime refuses to split
/// into atoms, and returns its index if there is one: a section of
/// fixed-size records that doesn't end on a record boundary, or a
/// non-empty one of the pointers only ld-prime makes (see
/// linker_pointer_content). `nindirect` is the number of the object's
/// indirect symbol table entries.
fn check_sections(hdrs: &[MachSection], nindirect: u32, file: &Path) -> Option<usize> {
    for (i, hdr) in hdrs.iter().enumerate() {
        if hdr.size != 0
            && let Some(content) = linker_pointer_content(hdr)
        {
            crate::error!(
                "unknown fixed size section __DATA,{} with content type: {content} in '{}'",
                hdr.sectname(),
                file.display()
            );
            return Some(i);
        }
        if let Some(size) = record_size(hdr)
            && !hdr.size.is_multiple_of(size)
        {
            crate::error!(
                "section {}/{} size {} is not a multiple of {size} in '{}'",
                hdr.segname(),
                hdr.sectname(),
                hdr.size,
                file.display()
            );
            return Some(i);
        }
    }

    // A non-lazy pointer section asked the linker to fill each slot with
    // the address of the symbol the indirect symbol table names for it,
    // as a 32-bit object's GOT did, and mold would leave the slots null.
    // ld-prime refuses every such section of an object with indirect
    // symbols, an empty one or one the table names no slot of too; in
    // an object without, one is pointers its relocations fill.
    if nindirect != 0 && hdrs.iter().any(|h| h.section_type() == S_NON_LAZY_SYMBOL_POINTERS) {
        crate::error!(
            "non-lazy pointers sections no longer supported for 64-bit architectures in '{}'",
            file.display()
        );
        return Some(hdrs.len());
    }
    None
}

/// The kind of pointers ld-prime reads a __DATA section as holding by
/// its name and type if they are ones only it makes, which it knows no
/// record size of: a 64-bit object has no business with the classic
/// lazy pointers only dyld's lazy binder fills, nor with the signed or
/// weak GOTs of an image (an input __got is GOT slots, see
/// fold_input_got). A section of one of those names but of another type
/// is data.
fn linker_pointer_content(hdr: &MachSection) -> Option<&'static str> {
    if hdr.segname() != "__DATA" {
        return None;
    }
    match (hdr.section_type(), hdr.sectname()) {
        (S_LAZY_SYMBOL_POINTERS, "__la_symbol_ptr") => Some("lazy-pointer"),
        (S_NON_LAZY_SYMBOL_POINTERS, "__auth_got") => Some("auth-got"),
        (S_NON_LAZY_SYMBOL_POINTERS, "__weak_got") => Some("weak-got"),
        (S_NON_LAZY_SYMBOL_POINTERS, "__weak_auth_got") => Some("weak-auth-got"),
        _ => None,
    }
}

/// Whether a section is one of the __LD segment's that ld-prime doesn't
/// know. It reads only __LD,__compact_unwind and drops any other with a
/// warning; a symbol defined in one is gone.
pub fn is_unknown_ld_section(hdr: &MachSection) -> bool {
    hdr.segname() == "__LD" && hdr.sectname() != "__compact_unwind"
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
    /// MH_SUBSECTIONS_VIA_SYMBOLS: symbols split sections into atoms.
    pub subsections_via_symbols: bool,
    /// The object's subsections, in section order and by address
    /// within a section; the other fields refer to them by their index
    /// here.
    pub isecs: Vec<InputSection>,
    pub relocs: Vec<crate::input_sections::Reloc>,
    /// Indices into `isecs`, sorted by input address.
    pub subsecs: Vec<crate::input_sections::InputSectionId>,
    pub nlists: std::borrow::Cow<'static, [NList]>,
    /// Index of the first external nlist, if the table is partitioned
    /// locals-then-externals (see first_global_of).
    pub first_global: Option<u32>,
    /// Each nlist's name, interned at integration.
    pub sym_names: Vec<&'static str>,
    /// xxh3 of each extern non-stab name (0 otherwise), computed here
    /// so the serial intern path never hashes.
    pub sym_hashes: Vec<u64>,
    /// The object's unwind info: a record per function (from
    /// __compact_unwind, or made for a function that has only an FDE),
    /// and the CIEs and FDEs of its __eh_frame.
    pub unwind: Vec<UnwindRecord>,
    pub cies: Vec<Cie>,
    pub fdes: Vec<Fde>,
    /// An FDE describes a function in a section of data, which
    /// ld-prime refuses (see add_fdes).
    pub data_fde: bool,
    /// The first pointer that has no relocation to name its target
    /// where ld-prime requires one, as its refusal words it (unless it
    /// stopped at a bad relocation first): see pointer_without_target.
    pub pointer_without_target: Option<&'static str>,
    /// The __compact_unwind pointer fields a 4-byte relocation set, as
    /// (subsection, function offset, 1 << field offset / 8) of their
    /// records: x86-64 takes those as well as 8-byte ones, and a -r
    /// output keeps them 4 bytes, as ld-prime does.
    pub unwind_ptr32: Vec<(u32, u32, u8)>,
    /// In a -r link, the labels at the start of __compact_unwind
    /// records - an arm64 assembler's ltmpN at the section's -, as
    /// (subsection, function offset, nlist) of their records: one names
    /// its record in the map, of which ld-prime makes an atom.
    pub unwind_labels: Vec<(u32, u32, u32)>,
    pub objc_image_info: Option<u32>,
    pub has_debug_info: bool,
    /// LC_DATA_IN_CODE entries: (file offset in the object, length,
    /// kind).
    pub dice: Vec<(u32, u16, u16)>,
    /// LC_LINKER_OPTIMIZATION_HINT entries: (kind, instruction
    /// addresses in the object's address space).
    pub loh: Vec<(u8, Vec<u64>)>,
    /// The labels ld-prime ignores (see extraneous_labels), sorted.
    pub extraneous_labels: Vec<u32>,
    /// Where ld-prime gave up reading the object, if it did: at the
    /// section check_sections refused, or past the sections at a bad
    /// relocation. It reads (and warns of) no section after it.
    pub failed_at: Option<usize>,
}

/// The object's nlist_64 array as a slice of the mapped file, or None
/// if it is unaligned or truncated (then the caller copies it).
fn nlists_slice(data: &'static [u8], off: usize, n: usize) -> Option<&'static [NList]> {
    let bytes = n.checked_mul(size_of::<NList>())?;
    if off.checked_add(bytes)? > data.len()
        || !(data.as_ptr() as usize + off).is_multiple_of(std::mem::align_of::<NList>())
    {
        return None;
    }
    // SAFETY: in bounds and aligned (checked above); NList is a
    // #[repr(C)] struct of plain integers, valid for every bit pattern;
    // the mapping lives for the whole link.
    Some(unsafe { std::slice::from_raw_parts(data.as_ptr().add(off).cast::<NList>(), n) })
}

/// The nlist index ranges of an object's local (with stab) and external
/// (defined and undefined) symbols. With a partitioned table these are
/// the two halves; without one, both are the whole table and callers'
/// per-entry filters still decide.
macro_rules! symbol_ranges {
    () => {
        #[inline]
        pub fn local_range(&self) -> std::ops::Range<usize> {
            0..self.first_global.map_or(self.nlists.len(), |g| g as usize)
        }
        #[inline]
        pub fn global_range(&self) -> std::ops::Range<usize> {
            self.first_global.map_or(0, |g| g as usize)..self.nlists.len()
        }
    };
}
impl ObjectFile {
    symbol_ranges!();
}
impl StagedObject {
    symbol_ranges!();
}

/// Where the object's external symbols start in its nlist array, or
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
fn first_global_of(nlists: &[NList], dysym: Option<&DysymtabCommand>) -> Option<u32> {
    let n = nlists.len() as u32;
    if let Some(d) = dysym
        && d.ilocalsym == 0
        && d.iextdefsym == d.nlocalsym
        && d.iundefsym == d.iextdefsym + d.nextdefsym
        && d.iundefsym + d.nundefsym == n
    {
        return Some(d.iextdefsym);
    }
    let is_local = |nl: &NList| nl.is_stab() || !nl.is_extern();
    let first = nlists.iter().position(|nl| !is_local(nl)).unwrap_or(nlists.len());
    nlists[first..].iter().all(|nl| !is_local(nl)).then_some(first as u32)
}

/// Which of an object's sections ld-prime ignores: those with no bytes
/// that define no symbol naming an atom there. Such a section makes no
/// output section and takes no part in ordering, in a final link and
/// in -r alike. An arm64 assembler's ltmpN label names an atom only in
/// an object without subsections, so it keeps an empty section there
/// and nowhere else - but for one of fixed-size records, where no label
/// at the end names anything (see extraneous_labels).
fn bare_sections(
    sect_hdrs: &[MachSection],
    nlists: &[NList],
    strtab: &'static [u8],
    split_ok: bool,
    record_ends: &[Option<u64>],
) -> Vec<bool> {
    let mut bare: Vec<bool> = sect_hdrs.iter().map(|s| s.size == 0).collect();
    for nlist in nlists {
        if !nlist.is_stab()
            && nlist.n_type() == N_SECT
            && let Some(b) = bare.get_mut((nlist.n_sect as usize).wrapping_sub(1))
            && *b
            && !(split_ok && symbol_name(strtab, nlist).starts_with("ltmp"))
            && !is_at_record_end(record_ends, nlist)
        {
            *b = false;
        }
    }
    bare
}

/// Where each section of fixed-size records (see record_size) ends.
fn record_ends(sect_hdrs: &[MachSection]) -> Vec<Option<u64>> {
    sect_hdrs.iter().map(|hdr| record_size(hdr).map(|_| hdr.addr + hdr.size)).collect()
}

/// Whether a symbol is at the end of a section of fixed-size records,
/// where it names no record.
fn is_at_record_end(record_ends: &[Option<u64>], nlist: &NList) -> bool {
    !nlist.is_stab()
        && nlist.n_type() == N_SECT
        && record_ends.get((nlist.n_sect as usize).wrapping_sub(1)) == Some(&Some(nlist.n_value))
}

/// The labels ld-prime ignores at the end of a section of fixed-size
/// records, by nlist index: with a warning ("ignoring extranenous
/// label"), but for the ltmpN label an arm64 assembler puts at an empty
/// section's start in an object with subsections, which names nothing
/// there anyway. A relocation can't refer to one.
fn extraneous_labels(
    nlists: &[NList],
    strtab: &'static [u8],
    split_ok: bool,
    record_ends: &[Option<u64>],
) -> Vec<u32> {
    (0..nlists.len() as u32)
        .filter(|&i| {
            let nlist = &nlists[i as usize];
            is_at_record_end(record_ends, nlist)
                && !(split_ok && symbol_name(strtab, nlist).starts_with("ltmp"))
        })
        .collect()
}

/// The load commands of an object that staging reads: its section
/// headers (every segment's sections in load command order, the ordinal
/// order nlists and relocations number them by), where its symbol table
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
    fn read<E: Target>(mf: &MappedFile, hdr: &MachHeader) -> Self {
        let data = mf.data();
        let mut cmds = Self::default();
        let mut off = size_of::<MachHeader>();
        for _ in 0..hdr.ncmds {
            let lc = LoadCommand::read_from(&data[off..]);
            match lc.cmd {
                LC_SEGMENT_64 => {
                    let seg = SegmentCommand::read_from(&data[off..]);
                    for i in 0..seg.nsects as usize {
                        let sect_off =
                            off + size_of::<SegmentCommand>() + i * size_of::<MachSection>();
                        let mut sect = MachSection::read_from(&data[sect_off..]);
                        sect.flags = crate::output_sections::canonical_section_flags(
                            sect.segname(),
                            sect.sectname(),
                            sect.flags,
                        );
                        cmds.sect_hdrs.push(sect);
                    }
                }
                LC_SYMTAB => cmds.symtab = Some(SymtabCommand::read_from(&data[off..])),
                LC_DYSYMTAB => cmds.dysymtab = Some(DysymtabCommand::read_from(&data[off..])),
                cmd if is_platform_cmd(cmd) => {
                    let version = PlatformVersion::read(cmd, &data[off..], E::CPUTYPE);
                    cmds.platform_versions.push(version);
                }
                LC_LINKER_OPTION => {
                    // Auto-link requests: the object names libraries it
                    // needs, as NUL-terminated strings after a count -
                    // an option and its argument, if it takes one, and
                    // no more, as ld-prime sees it.
                    let count = u32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap());
                    if !(1..=2).contains(&count) {
                        let file = mf.name.display();
                        fatal!(
                            "LC_LINKER_OPTION has count={count}, only 1 or 2 is valid in '{file}' in '{file}'"
                        );
                    }
                    let mut strs = Vec::with_capacity(count as usize);
                    let mut p = off + 12;
                    for _ in 0..count {
                        let rest = &data[p..off + lc.cmdsize as usize];
                        let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                        strs.push(rest[..len].to_vec());
                        p += len + 1;
                    }
                    cmds.linker_options.push(strs);
                }
                LC_DATA_IN_CODE => {
                    let cmd = LinkEditDataCommand::read_from(&data[off..]);
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
                    let cmd = LinkEditDataCommand::read_from(&data[off..]);
                    let payload =
                        &data[cmd.dataoff as usize..(cmd.dataoff + cmd.datasize) as usize];
                    let mut pos = 0;
                    while pos < payload.len() {
                        let kind = read_uleb_at(payload, &mut pos);
                        if kind == 0 {
                            break;
                        }
                        let count = read_uleb_at(payload, &mut pos);
                        let addrs = (0..count).map(|_| read_uleb_at(payload, &mut pos)).collect();
                        cmds.loh.push((kind as u8, addrs));
                    }
                }
                _ => {}
            }
            off += lc.cmdsize as usize;
        }
        cmds
    }
}

/// Reads an object's symbol table: its nlists and string table. The
/// nlist_64 array is used straight from the mmap when it is 8-aligned
/// (ld64 aligns it; NList is #[repr(C)] nlist_64, all integer fields,
/// so any bytes are a valid value) - no copy of 16 bytes per symbol.
/// mold borrows its ElfSym array the same way (Cow, Owned only for
/// synthesized symbols).
fn read_symtab(
    data: &'static [u8],
    cmd: Option<&SymtabCommand>,
) -> (std::borrow::Cow<'static, [NList]>, &'static [u8]) {
    let Some(cmd) = cmd else {
        return (std::borrow::Cow::Borrowed(&[]), &[]);
    };
    let (off, n) = (cmd.symoff as usize, cmd.nsyms as usize);
    let nlists = match nlists_slice(data, off, n) {
        Some(s) => std::borrow::Cow::Borrowed(s),
        None => std::borrow::Cow::Owned(read_array(data, off, n)),
    };
    let strtab = validate_strtab(&data[cmd.stroff as usize..(cmd.stroff + cmd.strsize) as usize]);
    (nlists, strtab)
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
/// `relocatable` is set for a -r link, which keeps the flags of a
/// .weak_def_can_be_hidden symbol that names a whole section (see
/// unweaken_section_atom_names).
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
        fatal!("{}: incompatible CPU type: expected {}", mf.name.display(), E::NAME);
    }

    let cmds = LoadCommands::read::<E>(mf, &hdr);

    // The section headers are complete; leak them so subsections can
    // reference (not copy) their parent header. The leak is bounded by
    // the object's section count and lives for the whole link.
    let sect_hdrs: &'static [MachSection] = Vec::leak(cmds.sect_hdrs);

    let (nlists, strtab) = read_symtab(data, cmds.symtab.as_ref());
    let first_global = first_global_of(&nlists, cmds.dysymtab.as_ref());
    let nindirect = cmds.dysymtab.as_ref().map_or(0, |d| d.nindirectsyms);

    // ld-prime ignores a record shorter than its 8 bytes (with a warning
    // unless empty, see warn_about_sections) and reads a longer one's
    // first 8.
    let objc_image_info =
        sect_hdrs.iter().find(|s| s.sectname() == "__objc_imageinfo" && s.size >= 8).map(|s| {
            let off = s.offset as usize + 4;
            u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
        });
    let has_debug_info =
        sect_hdrs.iter().any(|s| s.segname() == "__DWARF" && s.sectname() == "__debug_info");

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
        nlists,
        first_global,
        sym_names: Vec::new(),
        sym_hashes: Vec::new(),
        unwind: Vec::new(),
        cies: Vec::new(),
        fdes: Vec::new(),
        data_fde: false,
        pointer_without_target: None,
        unwind_ptr32: Vec::new(),
        unwind_labels: Vec::new(),
        objc_image_info,
        has_debug_info,
        dice: cmds.dice,
        loh: cmds.loh,
        extraneous_labels: Vec::new(),
        failed_at: None,
    };

    let split_ok = obj.subsections_via_symbols;
    let record_ends = record_ends(sect_hdrs);
    obj.extraneous_labels = extraneous_labels(&obj.nlists, strtab, split_ok, &record_ends);
    let bare = bare_sections(sect_hdrs, &obj.nlists, strtab, split_ok, &record_ends);
    if !obj.subsections_via_symbols {
        obj.unweaken_section_atom_names(strtab, relocatable);
    }
    obj.demote_unnamed_atom_names();
    obj.demote_thread_local_zerofill_names();
    let sect_isecs = obj.initialize_sections(&bare, relocatable);
    obj.read_symbol_names(strtab);
    obj.warn_referenced_dynamically();
    obj.failed_at = check_sections(sect_hdrs, nindirect, &mf.name);
    let mut relocs_ok = obj.failed_at.is_none() && obj.read_relocations::<E>(&bare, &sect_isecs);

    // ld-prime checks the relocations of __compact_unwind as any
    // section's, each 32-byte record being an atom.
    if relocs_ok
        && let Some(i) = sect_hdrs
            .iter()
            .position(|s| s.segname() == "__LD" && s.sectname() == "__compact_unwind")
    {
        let end = sect_hdrs[i].size as u32;
        let record_at = |off: u32| {
            let start = off.min(end.saturating_sub(1)) & !31;
            (start, start + 32)
        };
        match obj.read_section_relocs::<E>(i, record_at) {
            Some(rels) => obj.parse_compact_unwind(i, &rels, relocatable),
            None => relocs_ok = false,
        }
    }
    if relocs_ok && let Some((shndx, why)) = obj.bad_cfstring() {
        crate::error!("{why} in '{}'", crate::passes::resolved_file_name(mf));
        obj.failed_at = Some(shndx);
        relocs_ok = false;
    }
    if !relocs_ok {
        obj.failed_at.get_or_insert(sect_hdrs.len());
    }
    if relocs_ok
        && kept_fdes != KeptFdes::None
        && let Some(hdr) =
            sect_hdrs.iter().find(|s| s.segname() == "__TEXT" && s.sectname() == "__eh_frame")
    {
        obj.data_fde = obj.parse_eh_frame::<E>(hdr, kept_fdes == KeptFdes::All);
    }
    if relocs_ok {
        obj.pointer_without_target = obj.pointer_without_target();
    }
    // A DWARF-mode record whose FDE never turned up describes nothing.
    if kept_fdes != KeptFdes::None {
        obj.unwind.retain(|rec| {
            rec.encoding & UNWIND_MODE_MASK != E::UNWIND_MODE_DWARF || rec.fde().is_some()
        });
    }
    obj.group_unwind_records();
    obj
}

impl StagedObject {
    /// Without subsections a section is one atom, and ld64 takes the
    /// atom's attributes from one symbol at the section's start (the
    /// arm64 assembler's ltmpN labels don't count): a non-weak one if
    /// there is any, local or global, else the last weak one in symbol
    /// table order. An atom cannot be swapped for another copy, so a
    /// weak symbol that names it is no longer weak; the other symbols
    /// are labels into the atom and keep their flags. A
    /// .weak_def_can_be_hidden name becomes a hidden non-weak
    /// definition, except in a -r output, which keeps it as is.
    /// REFERENCED_DYNAMICALLY, which ld-prime ignores on a weak
    /// definition, stays ignored.
    fn unweaken_section_atom_names(&mut self, strtab: &'static [u8], relocatable: bool) {
        let sect_hdrs = self.sect_hdrs;
        let mut named_by_strong = vec![false; sect_hdrs.len()];
        let mut last_weak: Vec<Option<usize>> = vec![None; sect_hdrs.len()];
        for (i, nlist) in self.nlists.iter().enumerate() {
            if nlist.is_stab() || nlist.n_type() != N_SECT || nlist.n_sect == 0 {
                continue;
            }
            let sect = nlist.n_sect as usize - 1;
            if sect_hdrs.get(sect).is_none_or(|h| h.addr != nlist.n_value)
                || symbol_name(strtab, nlist).starts_with("ltmp")
            {
                continue;
            }
            if nlist.is_extern() && nlist.n_desc & N_WEAK_DEF != 0 {
                last_weak[sect] = Some(i);
            } else {
                named_by_strong[sect] = true;
            }
        }
        let names: Vec<usize> = last_weak
            .iter()
            .zip(&named_by_strong)
            .filter_map(|(&weak, &strong)| if strong { None } else { weak })
            .collect();
        if names.is_empty() {
            return;
        }
        let nlists = self.nlists.to_mut();
        for i in names {
            let nlist = &mut nlists[i];
            if nlist.n_desc & N_WEAK_REF == 0 {
                nlist.n_desc &= !(N_WEAK_DEF | REFERENCED_DYNAMICALLY);
            } else if !relocatable {
                nlist.n_desc &= !(N_WEAK_DEF | N_WEAK_REF | REFERENCED_DYNAMICALLY);
                nlist.n_type |= N_PEXT;
            }
        }
    }

    /// Demotes the external symbols of the sections whose atoms ld-prime
    /// makes by content and names none of (see has_unnamed_atoms and
    /// is_unnamed_objc_list) to locals that were private externals, as
    /// ld -r does: such a symbol defines nothing, so another object's
    /// reference to its name is undefined, another definition is no
    /// duplicate and no output lists it, while its own object's
    /// relocations still reach the atom.
    fn demote_unnamed_atom_names(&mut self) {
        use crate::passes::{has_unnamed_atoms, is_unnamed_objc_list};
        let split = self.subsections_via_symbols;
        let unnamed: Vec<bool> = self
            .sect_hdrs
            .iter()
            .map(|h| is_unnamed_objc_list(h) || has_unnamed_atoms(h, split))
            .collect();
        for nlist in self.demote_externals_in(&unnamed) {
            nlist.n_type = nlist.n_type & !N_EXT | N_PEXT;
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
        for nlist in self.demote_externals_in(&zerofill) {
            nlist.n_type &= !(N_EXT | N_PEXT);
            nlist.n_desc &= !(N_WEAK_DEF | N_WEAK_REF);
        }
    }

    /// The external symbols defined in the sections `demoted` marks (by
    /// ordinal), for the caller to make local. The table then no longer
    /// runs locals, then externals.
    fn demote_externals_in(&mut self, demoted: &[bool]) -> Vec<&mut NList> {
        if !demoted.contains(&true) {
            return Vec::new();
        }
        let is_demoted = |nlist: &NList| {
            !nlist.is_stab()
                && nlist.is_extern()
                && nlist.n_type() == N_SECT
                && nlist.n_sect != 0
                && demoted[nlist.n_sect as usize - 1]
        };
        if !self.nlists.iter().any(is_demoted) {
            return Vec::new();
        }
        self.first_global = None;
        self.nlists.to_mut().iter_mut().filter(|nlist| is_demoted(nlist)).collect()
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
            // __objc_imageinfo sections are merged into one synthesized
            // record; neither is copied through.
            if is_discarded_section(sect)
                || (sect.segname() == "__TEXT" && sect.sectname() == "__eh_frame")
                || sect.sectname() == "__objc_imageinfo"
            {
                continue;
            }

            let mut points = if is_literal_section(sect) {
                literal_split_points(sect, data)
            } else {
                std::mem::take(&mut split_points[i])
            };
            // Each initializer, terminator or non-lazy symbol pointer is
            // an atom of its own too, which ld-prime's diagnostics name.
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
                let mut contents: &[u8] = if is_zerofill {
                    &[]
                } else {
                    let lo = sect.offset as u64 + (start - sect.addr);
                    &data[lo as usize..(lo + (end - start)) as usize]
                };
                let mut size = end - start;
                // ld-prime takes the unterminated string that may end a
                // C-string section for a string too, and completes it
                // with a NUL (see is_unterminated_string).
                if sect.section_type() == S_CSTRING_LITERALS
                    && contents.last().is_some_and(|&b| b != 0)
                {
                    contents = Vec::leak([contents, &[0]].concat());
                    size += 1;
                }
                self.isecs.push(InputSection {
                    file: u32::MAX,
                    shndx: i as u32,
                    p2align: record_p2align.unwrap_or(sect.p2align as u8),
                    input_addr: start as u32,
                    size: size as u32,
                    contents: if contents.is_empty() { 0 } else { contents.as_ptr() as usize },
                    rel_offset: 0,
                    nrels: 0,
                    output_section: u32::MAX,
                    offset: 0,
                    flags: if bare[i] {
                        InputSection::flags_dead()
                    } else if record_p2align.is_some() {
                        InputSection::flags_alive_no_modulus()
                    } else {
                        InputSection::flags_alive()
                    },
                    replacement: crate::input_sections::NO_REPLACEMENT,
                    unwind_offset: 0,
                    nunwind: 0,
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
        for nlist in self.nlists.iter() {
            if !nlist.is_stab()
                && nlist.n_type() == N_SECT
                && nlist.n_desc & N_ALT_ENTRY == 0
                && nlist.n_sect >= 1
                && let Some(points) = points.get_mut(nlist.n_sect as usize - 1)
            {
                points.push(nlist.n_value);
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
    ///
    /// ld-prime stops reading an object at its first bad relocation;
    /// so does this, returning false once it has reported one.
    fn read_relocations<E: Target>(
        &mut self,
        bare: &[bool],
        sect_isecs: &[std::ops::Range<usize>],
    ) -> bool {
        use crate::input_sections::RelocTarget;

        let sect_hdrs = self.sect_hdrs;
        for (i, sect) in sect_hdrs.iter().enumerate() {
            if sect_isecs[i].is_empty() || sect.nreloc == 0 {
                continue;
            }
            let atom_at = |off| self.subsec_at(sect_isecs[i].clone(), off);
            let Some(mut rels) = self.read_section_relocs::<E>(i, atom_at) else {
                return false;
            };
            // The sort must be stable: a SUBTRACTOR and the UNSIGNED it
            // pairs with share one offset and their order is the pairing
            // (Swift's relative pointers are all such pairs). An unstable
            // sort swapped some, leaving lone 4-byte UNSIGNED relocations
            // that were then written as 8 bytes.
            rels.sort_by_key(|rel| rel.offset);

            for rel in &mut rels {
                if !self.check_reloc_target(rel, bare) {
                    continue;
                }
                if let RelocTarget::Section(sect_pos) = rel.target() {
                    let (isec, offset) = self.section_target(sect_pos, rel.addend, sect_isecs);
                    rel.set_target(RelocTarget::Section(isec as u32));
                    rel.addend = offset as i64;
                }
            }

            // read_relocs has checked that each field lies within the
            // section; ld-prime also wants it within its atom.
            let mut pos = 0;
            let mut straddles = false;
            for sub in sect_isecs[i].clone() {
                let sub_off = (self.isecs[sub].input_addr as u64 - sect.addr) as u32;
                let end = sub_off + self.isecs[sub].size;
                let start = self.relocs.len();
                while pos < rels.len() && rels[pos].offset < end {
                    let mut rel = rels[pos];
                    if rel.offset + rel.size as u32 > end && !straddles {
                        report_out_of_bounds(&self.mf.name, rel.size, rel.offset, (sub_off, end));
                        straddles = true;
                    }
                    rel.offset -= sub_off;
                    self.relocs.push(rel);
                    pos += 1;
                }
                self.isecs[sub].rel_offset = start as u32;
                self.isecs[sub].nrels = (self.relocs.len() - start) as u32;
            }
            if straddles {
                return false;
            }
        }
        true
    }

    /// Reads the relocations of section `i`. If ld-prime would reject
    /// one, reports the first such and returns None. `atom_at` gives the
    /// place in the section of the atom holding an offset, which the
    /// diagnostic names.
    fn read_section_relocs<E: Target>(
        &self,
        i: usize,
        atom_at: impl Fn(u32) -> (u32, u32),
    ) -> Option<Vec<crate::input_sections::Reloc>> {
        let mf = self.mf;
        let sect = &self.sect_hdrs[i];
        // ld-prime crashes on these.
        if matches!(sect.section_type(), S_ZEROFILL | S_THREAD_LOCAL_ZEROFILL) {
            crate::error!(
                "section '{}/{}' has a non-zero nreloc field in '{}'",
                sect.segname(),
                sect.sectname(),
                mf.name.display()
            );
            return None;
        }
        let data = mf.data();
        let raw: Vec<MachRel> = read_array(data, sect.reloff as usize, sect.nreloc as usize);
        let contents = &data[sect.offset as usize..][..sect.size as usize];
        let nsyms = self.nlists.len();
        let bad = match E::read_relocs(&mf.name, self.sect_hdrs, sect, contents, &raw, nsyms) {
            Ok(rels) => return Some(rels),
            Err(bad) => bad,
        };
        let bounds = atom_at(bad.rel.r_address);
        let name = self.atom_name(i + 1, sect.addr + bounds.0 as u64);
        report_bad_reloc(&mf.name, self.sect_hdrs.len(), &bad, name, bounds);
        None
    }

    /// The place in its section of the subsection holding `offset`, the
    /// atom ld-prime names in a diagnostic: the last one for an offset
    /// past the end. `isecs` are the section's subsections, in address
    /// order.
    fn subsec_at(&self, isecs: std::ops::Range<usize>, offset: u32) -> (u32, u32) {
        let isecs = &self.isecs[isecs];
        let sect = &self.sect_hdrs[isecs[0].shndx as usize];
        let addr = sect.addr + offset as u64;
        let i = isecs.partition_point(|isec| isec.input_addr as u64 <= addr);
        let isec = &isecs[i.saturating_sub(1)];
        let start = (isec.input_addr as u64 - sect.addr) as u32;
        (start, start + isec.size)
    }

    /// The name ld-prime gives the atom at `addr` in section `n_sect` in
    /// a diagnostic: that of a symbol there, ranked by atom_name_rank,
    /// or none.
    fn atom_name(&self, n_sect: usize, addr: u64) -> &'static str {
        self.nlists
            .iter()
            .zip(&self.sym_names)
            .filter(|(n, _)| {
                !n.is_stab()
                    && n.n_type() == N_SECT
                    && n.n_sect as usize == n_sect
                    && n.n_value == addr
            })
            .map(|(n, &name)| (atom_name_rank(n, name), name))
            .max()
            .map_or("", |(_, name)| name)
    }

    /// Reports a relocation whose target ld-prime ignores, returning
    /// false for it. A bare section can't be a relocation's target, named
    /// by section or through a label on it (an ltmpN), and neither can a
    /// symbol in an __LD section ld-prime drops or an extraneous label.
    fn check_reloc_target(&self, rel: &crate::input_sections::Reloc, bare: &[bool]) -> bool {
        use crate::input_sections::RelocTarget;

        match rel.target() {
            RelocTarget::Section(sect_pos) if bare[sect_pos as usize] => {
                let addr = (self.sect_hdrs[sect_pos as usize].addr as i64 + rel.addend) as u64;
                crate::error!(
                    "address=0x{addr:x} points to section({}) with no content in '{}'",
                    sect_pos + 1,
                    self.mf.name.display()
                );
                false
            }
            RelocTarget::Sym(idx) if self.extraneous_labels.binary_search(&idx).is_ok() => {
                // ld-prime's symbol table ends at the last symbol it
                // keeps.
                let ignored_tail = (self.extraneous_labels.iter().rev())
                    .zip((0..self.nlists.len() as u32).rev())
                    .take_while(|&(&a, b)| a == b)
                    .count();
                if idx as usize >= self.nlists.len() - ignored_tail {
                    crate::error!("r_symbolnum={idx} out of range in '{}'", self.mf.name.display());
                } else {
                    crate::error!("invalid r_symbolnum={idx} in '{}'", self.mf.name.display());
                }
                false
            }
            RelocTarget::Sym(idx)
                if self.nlists.get(idx as usize).is_some_and(|n| {
                    let sect = (n.n_sect as usize).wrapping_sub(1);
                    n.n_type() == N_SECT
                        && (bare.get(sect) == Some(&true)
                            || self.sect_hdrs.get(sect).is_some_and(is_unknown_ld_section))
                }) =>
            {
                crate::error!("invalid r_symbolnum={idx} in '{}'", self.mf.name.display());
                false
            }
            _ => true,
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
            fatal!("{}: relocation against a discarded section", self.mf.name.display());
        }
        let n = self.isecs[range.clone()].partition_point(|isec| isec.input_addr as u64 <= addr);
        let isec = range.start + n.saturating_sub(1);
        (isec, addr.wrapping_sub(self.isecs[isec].input_addr as u64))
    }

    /// Records each symbol's name, and for an external symbol the hash
    /// its name is interned by; the interning itself happens at
    /// integration, in one batch for all objects.
    /// ld-prime warns about REFERENCED_DYNAMICALLY, the flag that has
    /// strip(1) keep a symbol dyld looks up by name, on each exported
    /// non-weak definition in a section, as it reads the object (an
    /// archive member it never loads too). The output still carries it.
    fn warn_referenced_dynamically(&self) {
        let r = self.global_range();
        for (nlist, name) in self.nlists[r.clone()].iter().zip(&self.sym_names[r]) {
            if !nlist.is_stab()
                && nlist.n_type & (N_EXT | N_PEXT) == N_EXT
                && nlist.n_type() == N_SECT
                && nlist.n_desc & (REFERENCED_DYNAMICALLY | N_WEAK_DEF) == REFERENCED_DYNAMICALLY
            {
                crate::warn!("REFERENCED_DYNAMICALLY flag on symbol '{name}' is deprecated");
            }
        }
    }

    fn read_symbol_names(&mut self, strtab: &'static [u8]) {
        self.sym_names = self.nlists.iter().map(|nlist| symbol_name(strtab, nlist)).collect();
        self.sym_hashes = self
            .nlists
            .iter()
            .zip(&self.sym_names)
            .map(|(nlist, name)| {
                if !nlist.is_stab() && nlist.is_extern() {
                    crate::symbol::hash_key(name)
                } else {
                    0
                }
            })
            .collect();
    }
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
    ) || (sect.segname() == "__DATA" && (sect.sectname() == "__cfstring" || is_pointer_list(sect)))
}

/// Whether ld-prime merges a section's atoms by their content - the
/// literal pools, C strings, selector references and CFStrings, not the
/// pointer lists it takes one by one - which the labels an assembler
/// makes for itself name none of (see Context::atom_label).
pub fn has_merged_atoms(sect: &MachSection) -> bool {
    is_literal_section(sect) && !(sect.segname() == "__DATA" && is_pointer_list(sect))
}

/// Whether a section's atoms are its literals or fixed-size records,
/// whatever its labels (see initialize_sections).
pub fn is_record_section(sect: &MachSection) -> bool {
    is_literal_section(sect)
        || matches!(
            sect.section_type(),
            S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS | S_NON_LAZY_SYMBOL_POINTERS
        )
}

/// Whether a __DATA section is one of pointers the linker takes one by
/// one: class, superclass and protocol references, or GOT slots.
fn is_pointer_list(sect: &MachSection) -> bool {
    matches!(
        sect.sectname(),
        "__objc_classrefs" | "__objc_superrefs" | "__objc_protorefs" | "__got"
    )
}

/// Where the elements of a literal section start: each NUL-terminated
/// string of a __cstring section (and the unterminated one that may end
/// it, see initialize_sections), each fixed-size record of the others.
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
        // A literal-pointer section (__objc_selrefs) is one atom per
        // pointer, as in ld64, so references to the same selector can be
        // coalesced across objects; so are the other pointer lists.
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
    /// the given bases. `syms` maps its nlists to symbols, for the
    /// personality functions its unwind info names by nlist index.
    fn rebase(
        &mut self,
        obj_idx: usize,
        isec_base: usize,
        cie_base: usize,
        fde_base: usize,
        syms: &[crate::symbol::SymbolId],
    ) {
        use crate::input_sections::RelocTarget;

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
        for (isec, _, _) in &mut self.unwind_ptr32 {
            *isec += isec_base as u32;
        }
        for (isec, _, _) in &mut self.unwind_labels {
            *isec += isec_base as u32;
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
            is_alive: self.alive,
            priority: self.priority,
            linker_options: self.linker_options,
            linker_options_read: false,
            platform_versions: self.platform_versions,
            hidden: self.hidden,
            subsections_via_symbols: self.subsections_via_symbols,
            sect_hdrs: std::borrow::Cow::Borrowed(self.sect_hdrs),
            relocs: self.relocs,
            subsecs: self.subsecs,
            objc_image_info: self.objc_image_info,
            has_debug_info: self.has_debug_info,
            unwind_ptr32: self.unwind_ptr32,
            unwind_labels: self.unwind_labels,
            nlists: self.nlists,
            first_global: self.first_global,
            symbols,
            lto_module: None,
            dice: self.dice,
            loh: self.loh,
        }
    }
}

/// Integrates a whole staging batch at once, mold-style: every
/// object's arena positions (subsection, CIE, FDE and local-symbol
/// bases) come from prefix sums over the batch, so the rebasing of
/// indices - the actual work - runs on all cores, and the serial
/// remainder is moving the rebased vectors into the global arenas.
/// Produces exactly the layout the one-at-a-time path would.
pub fn integrate_objects<E: Target>(
    ctx: &mut Context<E>,
    mut staged: Vec<StagedObject>,
    ids: Vec<crate::symbol::SymbolId>,
    counts: Vec<usize>,
) {
    let obj_base = ctx.objs.len();
    let mut isec_base = ctx.isecs.len();
    let mut cie_base = ctx.cies.len();
    let mut fde_base = ctx.fdes.len();
    let mut unwind_base = ctx.unwind_records.len();
    let mut locals_base = ctx.symbols.syms.len();
    let mut id_base = 0usize;

    struct Bases {
        isec: usize,
        cie: usize,
        fde: usize,
        unwind: usize,
        locals: usize,
        ids: usize,
    }
    // The per-object local-symbol counts drive the prefix sum below.
    // Counting scans every nlist of every object, so on a debug link
    // (millions of nlists) it runs in parallel; the prefix sum itself
    // stays a cheap serial arithmetic walk.
    let n_locals_all: Vec<usize> = staged
        .par_iter()
        .map(|st| {
            st.first_global.map_or_else(
                || st.nlists.iter().filter(|n| n.is_stab() || !n.is_extern()).count(),
                |g| g as usize,
            )
        })
        .collect();
    let mut bases = Vec::with_capacity(staged.len());
    for (i, (st, &nids)) in staged.iter().zip(&counts).enumerate() {
        bases.push(Bases {
            isec: isec_base,
            cie: cie_base,
            fde: fde_base,
            unwind: unwind_base,
            locals: locals_base,
            ids: id_base,
        });
        isec_base += st.isecs.len();
        cie_base += st.cies.len();
        fde_base += st.fdes.len();
        unwind_base += st.unwind.len();
        locals_base += n_locals_all[i];
        id_base += nids;
    }

    // The rebasing, in parallel. Each object's nlists map to symbols
    // first: its locals to the slots its prefix sum reserved (they are
    // initialized below), its globals to the ids interned for the batch.
    let syms_of: Vec<Vec<crate::symbol::SymbolId>> = staged
        .par_iter_mut()
        .enumerate()
        .map(|(i, st)| {
            let base = &bases[i];
            let mut syms = Vec::with_capacity(st.nlists.len());
            let mut next_local = base.locals as u32;
            let mut next_id = base.ids;
            for nlist in st.nlists.iter() {
                if nlist.is_stab() || !nlist.is_extern() {
                    syms.push(next_local);
                    next_local += 1;
                } else {
                    syms.push(ids[next_id]);
                    next_id += 1;
                }
            }

            // Hand each subsection its compact-unwind range (records
            // arrive grouped by function), before the indices rebase.
            let mut run = 0;
            while run < st.unwind.len() {
                let isec = st.unwind[run].isec;
                let start = run;
                while run < st.unwind.len() && st.unwind[run].isec == isec {
                    run += 1;
                }
                st.isecs[isec as usize].unwind_offset = (base.unwind + start) as u32;
                st.isecs[isec as usize].nunwind = (run - start) as u32;
            }
            st.rebase(obj_base + i, base.isec, base.cie, base.fde, &syms);
            syms
        })
        .collect();

    // Local symbols initialize in parallel into pre-reserved disjoint
    // ranges - mold's ParallelSymbolAllocator contract: the arena is
    // sized up front, each object owns the exclusive range its prefix
    // sum assigned, and init writes every slot in it.
    {
        let total_locals = locals_base - ctx.symbols.syms.len();
        let old_len = ctx.symbols.syms.len();
        ctx.symbols.syms.reserve(total_locals);
        struct SlotPtr(*mut crate::symbol::Symbol);
        unsafe impl Sync for SlotPtr {}
        let ptr = SlotPtr(ctx.symbols.syms.as_mut_ptr());
        let ptr = &ptr;
        staged.par_iter().zip(&bases).for_each(|(st, base)| {
            let mut slot = base.locals;
            let r = st.local_range();
            for (nlist, name) in st.nlists[r.clone()].iter().zip(&st.sym_names[r]) {
                if nlist.is_stab() || !nlist.is_extern() {
                    // SAFETY: [base.locals, base.locals+n) ranges
                    // are disjoint across objects and lie within
                    // the reserved capacity.
                    unsafe {
                        ptr.0.add(slot).write(crate::symbol::Symbol::new(name));
                    }
                    slot += 1;
                }
            }
        });
        // SAFETY: every slot in old_len..old_len+total_locals was
        // initialized by exactly one object above.
        unsafe { ctx.symbols.syms.set_len(old_len + total_locals) };
    }

    // Arena extension: each object's staged vectors move into the
    // arenas at the exclusive ranges the prefix sums assigned - the
    // same contract as the local symbols above, so hundreds of
    // megabytes of subsections move on all cores instead of one.
    fn par_moves<T: Send>(dst: &mut Vec<T>, parts: Vec<(usize, Vec<T>)>) {
        struct RawPtr<T>(*mut T);
        unsafe impl<T> Sync for RawPtr<T> {}
        let add: usize = parts.iter().map(|(_, v)| v.len()).sum();
        let old = dst.len();
        dst.reserve(add);
        let ptr = RawPtr(dst.as_mut_ptr());
        let ptr = &ptr;
        parts.into_par_iter().for_each(|(base, items)| {
            for (p, item) in (base..).zip(items) {
                // SAFETY: the ranges are disjoint across parts and lie
                // within the reserved capacity; every slot is written
                // exactly once.
                unsafe { ptr.0.add(p).write(item) };
            }
        });
        unsafe { dst.set_len(old + add) };
    }
    macro_rules! take_parts {
        ($field:ident, $base:ident) => {
            staged
                .iter_mut()
                .zip(&bases)
                .map(|(st, b)| (b.$base, std::mem::take(&mut st.$field)))
                .collect()
        };
    }
    par_moves(&mut ctx.isecs, take_parts!(isecs, isec));
    par_moves(&mut ctx.unwind_records, take_parts!(unwind, unwind));
    par_moves(&mut ctx.cies, take_parts!(cies, cie));
    par_moves(&mut ctx.fdes, take_parts!(fdes, fde));

    for (st, syms) in staged.into_iter().zip(syms_of) {
        ctx.objs.push(st.into_object_file(syms));
    }
}

/// Appends a staged object to the global arenas, rebasing its local
/// indices and interning its symbol names: integrate_objects for a
/// batch of one, done serially.
pub fn integrate_object<E: Target>(ctx: &mut Context<E>, mut staged: StagedObject) -> usize {
    let obj_idx = ctx.objs.len();
    let isec_base = ctx.isecs.len();

    let mut syms = Vec::with_capacity(staged.nlists.len());
    for (nlist, name) in staged.nlists.iter().zip(&staged.sym_names) {
        let id = if nlist.is_stab() || !nlist.is_extern() {
            ctx.symbols.add_local(name)
        } else {
            ctx.symbols.intern(name)
        };
        syms.push(id);
    }

    staged.rebase(obj_idx, isec_base, ctx.cies.len(), ctx.fdes.len(), &syms);
    ctx.isecs.append(&mut staged.isecs);
    for rec in std::mem::take(&mut staged.unwind) {
        // Extend or open the subsection's record range (grouped input).
        let isec = &mut ctx.isecs[rec.isec as usize];
        if isec.nunwind == 0 {
            isec.unwind_offset = ctx.unwind_records.len() as u32;
        }
        isec.nunwind += 1;
        ctx.unwind_records.push(rec);
    }
    ctx.cies.append(&mut staged.cies);
    ctx.fdes.append(&mut staged.fdes);
    ctx.objs.push(staged.into_object_file(syms));
    obj_idx
}

/// What check_unwind_sections found of an object, to report once the
/// object's atoms are warned of (see passes::load_pending): the warnings
/// and, for an FDE in a section of data, the object's name.
pub struct UnwindCheck {
    warnings: Vec<String>,
    data_fde: Option<String>,
}

impl UnwindCheck {
    pub fn report(self) {
        for msg in self.warnings {
            crate::warn!("{msg}");
        }
        if let Some(file) = self.data_fde {
            fatal!("invalid function target for dwarf unwind in '{file}'");
        }
    }
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
    staged.check_unwind_sections().report();
    integrate_object(ctx, staged)
}

/// Loads the LTO plugin on first use.
pub fn ensure_lto_plugin<E: Target>(ctx: &mut Context<E>) -> crate::lto::Plugin {
    if ctx.lto_plugin.is_none() {
        ctx.lto_plugin = Some(crate::lto::load_plugin(ctx.args.lto_library.as_deref()));
    }
    ctx.lto_plugin.unwrap()
}

/// Registers a bitcode input: a placeholder object that claims the
/// module's symbols so resolution works, compiled for real by LTO once
/// all inputs are known.
pub fn parse_bitcode<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    alive: bool,
) -> usize {
    let plugin = ensure_lto_plugin(ctx);
    let (module, lsyms) = crate::lto::parse_module(&plugin, mf.data(), &mf.name);
    // ld-prime checks the target triple's OS and version as it checks
    // a Mach-O object's platform load command.
    let triple = crate::lto::module_triple(&plugin, module);
    let platform_versions = PlatformVersion::of_triple(&triple).into_iter().collect();

    let obj_idx = ctx.objs.len();
    let mut syms = Vec::new();
    let mut nlists = Vec::new();

    // Symbols are expressed as synthesized nlists so that the regular
    // resolution pass handles bitcode like any object.
    let mut defined = Vec::new();
    for ls in lsyms {
        let name: &'static str = String::leak(ls.name);
        if ls.is_defined {
            defined.push(name);
        }
        if !ls.is_extern && ls.is_defined {
            continue;
        }
        let id = ctx.symbols.intern(name);
        let mut nlist = NList::default();
        if ls.is_defined {
            nlist.n_type = N_ABS | N_EXT | if ls.is_private_extern { N_PEXT } else { 0 };
            if ls.is_weak_def {
                nlist.n_desc |= N_WEAK_DEF;
            }
            if ls.is_weak_def && ls.can_be_hidden {
                nlist.n_desc |= N_WEAK_REF;
            }
        } else {
            nlist.n_type = N_UNDF | N_EXT;
        }
        nlists.push(nlist);
        syms.push(id);
    }

    let priority = ctx.next_priority();
    ctx.objs.push(ObjectFile {
        mf,
        is_alive: alive,
        priority,
        linker_options: Vec::new(),
        linker_options_read: false,
        platform_versions,
        hidden: false,
        subsections_via_symbols: true,
        sect_hdrs: std::borrow::Cow::Borrowed(&[]),
        relocs: Vec::new(),
        subsecs: Vec::new(),
        objc_image_info: None,
        has_debug_info: false,
        unwind_ptr32: Vec::new(),
        unwind_labels: Vec::new(),
        nlists: std::borrow::Cow::Owned(nlists),
        first_global: None,
        symbols: syms,
        lto_module: Some(module),
        dice: Vec::new(),
        loh: Vec::new(),
    });
    let is_thin = crate::lto::module_is_thin(&plugin, module);
    ctx.lto_modules.push(crate::lto::BitcodeModule {
        obj: obj_idx,
        handle: module,
        defined,
        is_thin,
    });
    obj_idx
}

/// Extracts one NUL-terminated name from a string table already
/// validated as UTF-8 by validate_strtab. The NUL scan goes through
/// libc's memchr, which is vectorized; a per-name from_utf8 was a
/// quarter of all staging time on big links.
fn symbol_name(strtab: &'static [u8], nlist: &NList) -> &'static str {
    let off = nlist.n_strx as usize;
    if off >= strtab.len() {
        return "";
    }
    let rest = &strtab[off..];
    // SAFETY: memchr reads within `rest`; the result is bounded by
    // its length.
    let len = unsafe {
        let p = libc::memchr(rest.as_ptr().cast(), 0, rest.len());
        if p.is_null() { rest.len() } else { (p as usize) - (rest.as_ptr() as usize) }
    };
    // SAFETY: the whole table was checked as UTF-8 up front; any
    // slice of it on a codepoint boundary is valid, and a NUL
    // boundary always is.
    unsafe { std::str::from_utf8_unchecked(&rest[..len]) }
}

/// Checks a whole string table as UTF-8 once - vastly cheaper than
/// validating millions of short names one by one. Returns an empty
/// table (degrading names to "") for the pathological non-UTF-8 case.
fn validate_strtab(strtab: &'static [u8]) -> &'static [u8] {
    match std::str::from_utf8(strtab) {
        Ok(_) => strtab,
        Err(e) => &strtab[..e.valid_up_to()],
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
/// which is 16 bytes each - as mold's Fde/Cie hold u32 indices.
/// Read the optional fields through `personality()`, `lsda()`, `fde()`.
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
}

impl StagedObject {
    /// Parses a __LD,__compact_unwind section into unwind records. The
    /// section is an array of 32-byte entries whose pointer fields are
    /// set by relocations, `rels` as read_relocs made them of the
    /// section's.
    ///
    /// Records that point to DWARF unwind info keep their DWARF-mode
    /// encoding; parse_eh_frame attaches the FDE (a final link
    /// regenerates the encoding from it, a -r output copies the record
    /// as it came, like ld64). Object files usually don't contain such
    /// records, but `ld -r` output does.
    fn parse_compact_unwind(
        &mut self,
        sect: usize,
        rels: &[crate::input_sections::Reloc],
        labels: bool,
    ) {
        use crate::input_sections::RelocTarget;

        const ENTRY_SIZE: usize = 32;
        let mf = self.mf;
        let hdr = &self.sect_hdrs[sect];
        // Diagnostics spell the path lossily.
        let file_name = mf.name.display();
        if !hdr.size.is_multiple_of(ENTRY_SIZE as u64) {
            fatal!("{file_name}: invalid __compact_unwind section size");
        }

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

        let subsec_at = |addr: u64| find_subsec(&self.isecs, &self.subsecs, addr);

        // A pointer field may take a 4-byte relocation (on x86-64, the
        // only target with 4-byte pointers) as well as an 8-byte one,
        // but the other fields none. The 4-byte ones, by record.
        let mut ptr32: Vec<(usize, u8)> = Vec::new();
        for r in rels {
            let field = r.offset as usize % ENTRY_SIZE;
            if !matches!(field, 0 | 16 | 24) {
                fatal!(
                    "compact unwind fixup at offset of {field} but expected 16 or 24 in '{}'",
                    crate::passes::resolved_file_name(mf)
                );
            }
            if r.size == 4 {
                ptr32.push((r.offset as usize / ENTRY_SIZE, 1 << (field / 8)));
            }
            let rec = &mut records[r.offset as usize / ENTRY_SIZE];
            // The address a pointer field refers to, and the section
            // (1-based, as nlists count) it is in. For an extern
            // reference the target is this object's own definition,
            // located by its nlist.
            let (n_sect, addr) = match r.target() {
                RelocTarget::Sym(sym) => {
                    let nlist = &self.nlists[sym as usize];
                    let n_sect = if nlist.n_type() == N_SECT { nlist.n_sect } else { 0 };
                    (n_sect, nlist.n_value.wrapping_add_signed(r.addend))
                }
                RelocTarget::Section(sect) => {
                    let addr = self.sect_hdrs[sect as usize].addr;
                    (sect as u8 + 1, addr.wrapping_add_signed(r.addend))
                }
            };

            match field {
                // The function the record covers, looked for in that
                // section as a label's place is: the section's end is
                // its last atom's, not the next section's first one's.
                0 => {
                    let Some((isec, off)) =
                        find_symbol_subsec(&self.isecs, &self.subsecs, n_sect, addr)
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
                            self.nlists.iter().position(|n| n.is_extern() && n.n_value == addr)
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
                _ => unreachable!(),
            }
        }

        for (i, bit) in ptr32 {
            if records[i].isec != u32::MAX {
                self.unwind_ptr32.push((records[i].isec, records[i].input_offset, bit));
            }
        }

        if labels {
            self.label_unwind_records(sect, &records);
        }

        // A record no relocation gave a function describes nothing.
        records.retain(|rec| rec.isec != u32::MAX);
        self.unwind.extend(records);
    }

    /// Notes the labels at the start of records of __compact_unwind
    /// (section `sect`) in unwind_labels.
    fn label_unwind_records(&mut self, sect: usize, records: &[UnwindRecord]) {
        let hdr = &self.sect_hdrs[sect];
        for (k, nlist) in self.nlists.iter().enumerate() {
            let off = nlist.n_value.wrapping_sub(hdr.addr);
            if nlist.is_stab()
                || nlist.n_type() != N_SECT
                || nlist.n_sect as usize != sect + 1
                || off >= hdr.size
                || off % 32 != 0
            {
                continue;
            }
            let rec = &records[off as usize / 32];
            if rec.isec != u32::MAX {
                self.unwind_labels.push((rec.isec, rec.input_offset, k as u32));
            }
        }
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

/// A DWARF Common Information Entry from an object's __eh_frame.
#[derive(Debug)]
pub struct Cie {
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
    /// Whether an FDE of the input points at it, whether or not the
    /// FDE is kept (see Context::keeps_lone_cie).
    pub has_fdes: bool,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Cie>() == 48);

impl Cie {
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
pub struct Fde {
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
const _: () = assert!(std::mem::size_of::<Fde>() == 56);

pub fn read_uleb_at(data: &[u8], pos: &mut usize) -> u64 {
    let mut val = 0;
    let mut shift = 0;
    loop {
        let byte = data[*pos];
        *pos += 1;
        val |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return val;
        }
        shift += 7;
    }
}

impl StagedObject {
    /// Parses a __TEXT,__eh_frame section. Unlike other sections it is not
    /// copied through: the linker re-synthesizes it, keeping only FDEs for
    /// functions that have no compact unwind record, patching each CIE's
    /// personality cell to be GOT-relative, and dropping the rest.
    /// Returns whether an FDE describes a function in a section of data.
    fn parse_eh_frame<E: Target>(&mut self, hdr: &MachSection, keep_all_fdes: bool) -> bool {
        let mf = self.mf;
        // Diagnostics spell the path lossily.
        let file_name = mf.name.display();
        let data = mf.data();
        let rels: Vec<MachRel> = read_array(data, hdr.reloff as usize, hdr.nreloc as usize);

        // The records borrow from a processed copy of the section, leaked
        // once per object (like its section headers): the CIE/FDE bytes
        // then need no per-record copy, and they carry the pre-applied
        // relocations.
        let mut contents =
            data[hdr.offset as usize..(hdr.offset as u64 + hdr.size) as usize].to_vec();
        apply_eh_frame_relocs::<E>(&mut contents, &rels, &self.nlists, &mf.name);
        let contents: &'static [u8] = Vec::leak(contents);

        // Split the section into records: a zero ID marks a CIE, anything
        // else is an FDE pointing back at its CIE. The checks and their
        // words are ld-prime's, but a record's fields must lie within it:
        // ld-prime reads them wherever they fall, into the next record or,
        // as it lets a record's length (leaving out the length field
        // itself) reach the section's end, past it.
        let word = |pos: usize| {
            contents.get(pos..pos + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        let mut fdes: Vec<(u32, &'static [u8], u32)> = Vec::new();
        let mut pos = 0;
        while pos < contents.len() {
            // An extended length (0xffffffff, then 64 bits) is not
            // supported: it extends beyond any section.
            let len = match word(pos) {
                Some(len) if pos + 4 + len as usize <= contents.len() => len as usize,
                _ => fatal!("CFI at 0x{pos:08X} extends beyond end of section in '{file_name}'"),
            };
            // ld-prime takes the word after the length for the ID even
            // if the length leaves it no room - the next record's length
            // after an empty one - and checks the CIE pointer of an FDE
            // before its size.
            let id = word(pos + 4).filter(|&id| len >= 4 || id != 0);
            let Some(id) = id else {
                if len == 0 {
                    fatal!("empty CIE in '{file_name}'");
                }
                truncated_cfi(&mf.name, pos);
            };
            let rec: &'static [u8] = &contents[pos..pos + 4 + len];
            let input_addr = hdr.addr as u32 + pos as u32;
            if id == 0 {
                let Some((fde_enc, lsda_enc)) = parse_cie_augmentation(rec, &mf.name) else {
                    truncated_cfi(&mf.name, pos);
                };
                self.cies.push(Cie {
                    obj: u32::MAX,
                    input_addr,
                    data: rec,
                    personality: None,
                    personality_offset: 0,
                    fde_enc,
                    lsda_enc,
                    output_offset: 0,
                    is_alive: false,
                    has_fdes: false,
                });
            } else {
                // The ID is how far back the CIE is from the ID itself,
                // and ld-prime takes whatever it finds there for one.
                let Some(cie_pos) = (pos + 4).checked_sub(id as usize) else {
                    fatal!("FDE points to CIE outside __eh_frame section in '{file_name}'");
                };
                let cie_addr = hdr.addr as u32 + cie_pos as u32;
                let Some(cie) = self.cies.iter().position(|c| c.input_addr == cie_addr) else {
                    if word(cie_pos) == Some(0) {
                        fatal!("empty CIE in '{file_name}'");
                    }
                    if word(cie_pos + 4) != Some(0) {
                        fatal!("CIE ID is not zero in '{file_name}'");
                    }
                    fatal!("{file_name}: __eh_frame: FDE with an invalid CIE pointer");
                };
                if len < 4 {
                    truncated_cfi(&mf.name, pos);
                }
                self.cies[cie].has_fdes = true;
                fdes.push((input_addr, rec, cie as u32));
            }
            pos += 4 + len;
        }

        // Personality references appear as GOT-relative relocations inside
        // a CIE, whatever the personality's encoding says; ld-prime takes
        // no other reference from a CIE.
        for r in &rels {
            let addr = hdr.addr as u32 + r.r_address;
            let i = self.cies.partition_point(|c| c.input_addr <= addr);
            let cie = i
                .checked_sub(1)
                .filter(|&i| addr < self.cies[i].input_addr + self.cies[i].data.len() as u32);
            if r.r_type() != E::RELOC_GOTPC {
                if cie.is_some() {
                    fatal!("CIE reference to personality function not supported in '{file_name}'");
                }
                continue;
            }
            let Some(cie) = cie.map(|i| &mut self.cies[i]) else {
                fatal!("{file_name}: __eh_frame: stray personality relocation");
            };
            if !r.is_extern() {
                fatal!("{file_name}: __eh_frame: unsupported personality reference");
            }
            // A local symbol index, mapped to a symbol at integration.
            cie.personality = Some(r.r_symbolnum());
            cie.personality_offset = addr - cie.input_addr;
        }

        self.add_fdes::<E>(&fdes, hdr.addr as u32, keep_all_fdes)
    }

    /// Adds the FDEs of the __eh_frame at `sect_addr`, given as (input
    /// address, bytes, CIE index), and ties them to the functions'
    /// unwind records. A function that already has a compact unwind
    /// record doesn't need its FDE; the compact record wins. A
    /// DWARF-mode record is the exception: it exists to point at the
    /// FDE. `keep_all_fdes` keeps the FDEs of covered functions too: a
    /// -r output carries every input CIE and FDE through, as ld64's
    /// does, and a -static image or one linked with -no_compact_unwind
    /// has no __unwind_info for the compact record (which ld-prime
    /// drops, turning none into an FDE), and one for a macOS before
    /// 10.9 keeps them for its old unwinders (see Args::keeps_all_fdes);
    /// any other final image has no use for them.
    ///
    /// ld-prime unwinds only code. An FDE for a function in a section
    /// of data it refuses: returns true for that. One in a section of
    /// no kind it knows it carries, but gives the function no entry of
    /// __unwind_info. (check_unwind_sections warns about both.)
    fn add_fdes<E: Target>(
        &mut self,
        fdes: &[(u32, &'static [u8], u32)],
        sect_addr: u32,
        keep_all_fdes: bool,
    ) -> bool {
        // Diagnostics spell the path lossily.
        let file_name = self.mf.name.display();
        let mut data_fde = false;
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
            // augmentation data's length and the LSDA pointer. ld-prime
            // reads any pointer of 4 or 8 bytes, absolute or relative to
            // itself, and looks the function up before it refuses all
            // but the pc-relative ones compilers write.
            let enc = self.cies[cie as usize].fde_enc;
            let Some(size) = pointer_size(enc) else {
                fatal!("unsupported pointer encoding 0x{enc:02X} in '{file_name}'");
            };
            if rec.len() < 8 + 2 * size {
                truncated_cfi(&self.mf.name, (input_addr - sect_addr) as usize);
            }
            // The size is in the same format, but absolute.
            let func_addr = read_pointer(rec, 8, enc, input_addr);
            let code_len = read_pointer(rec, 8 + size, enc & 0xf, 0) as u32;

            let Some((isec, func_offset)) = find_subsec(&self.isecs, &self.subsecs, func_addr)
            else {
                fatal!("address=0x{func_addr:X} not in any section in '{file_name}'");
            };
            if enc != DW_EH_PE_PCREL && enc != DW_EH_PE_PCREL | DW_EH_PE_SDATA4 {
                fatal!("unsupported FDE pointer encoding 0x{enc:02X} in FDE in '{file_name}'");
            }
            let func_offset = func_offset as u32;
            let sect = &self.sect_hdrs[self.isecs[isec].shndx as usize];
            let is_code = is_code_section(sect);
            data_fde |= is_typed_data_section(sect);
            let lsda = self.cies[cie as usize]
                .lsda_enc
                .and_then(|enc| self.fde_lsda(rec, input_addr, 8 + 2 * size, enc, sect_addr));

            let is_covered = covered.contains(&(isec, func_offset));
            if is_covered && !keep_all_fdes {
                continue;
            }

            let fde_idx = self.fdes.len();
            self.fdes.push(Fde {
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
        data_fde
    }

    /// Reads the LSDA pointer of an FDE `rec` at `input_addr` whose
    /// augmentation data's length is at `pos`, in encoding `enc`, and
    /// returns the subsection and offset it points to. As libunwind
    /// reads it, an FDE with no augmentation data or with a zero pointer
    /// has none (GCC writes one for a function with no LSDA under a CIE
    /// that declares them). Like the function's, ld-prime reads the
    /// pointer in any encoding it knows, but takes only 0x10 and 0x1b
    /// once it has found what it points to.
    fn fde_lsda(
        &self,
        rec: &[u8],
        input_addr: u32,
        mut pos: usize,
        enc: u8,
        sect_addr: u32,
    ) -> Option<(u32, u32)> {
        // Diagnostics spell the path lossily.
        let file_name = self.mf.name.display();
        let truncated = || truncated_cfi(&self.mf.name, (input_addr - sect_addr) as usize);
        if skip_uleb(rec, pos).is_none() {
            truncated();
        }
        if read_uleb_at(rec, &mut pos) == 0 {
            return None;
        }
        let Some(size) = pointer_size(enc) else {
            fatal!("unsupported pointer encoding 0x{enc:02X} in '{file_name}'");
        };
        if pos + size > rec.len() {
            truncated();
        }
        if read_pointer(rec, pos, enc & 0xf, 0) == 0 {
            return None;
        }
        let addr = read_pointer(rec, pos, enc, input_addr);
        let Some((isec, off)) = find_subsec(&self.isecs, &self.subsecs, addr) else {
            fatal!("address=0x{addr:X} not in any section in '{file_name}'");
        };
        // ld-prime names the FDE's function encoding here.
        if enc != DW_EH_PE_PCREL && enc != DW_EH_PE_PCREL | DW_EH_PE_SDATA4 {
            fatal!("unsupported FDE pointer encoding 0x{enc:02X} in FDE to LSDA in '{file_name}'");
        }
        Some((isec as u32, off as u32))
    }

    /// Checks the object's unwind info as ld-prime does: it warns
    /// about each section that has unwind info (compact or DWARF) but no
    /// code, then refuses an FDE for a function in a section of data
    /// (see add_fdes).
    pub fn check_unwind_sections(&self) -> UnwindCheck {
        let isecs = self.unwind.iter().map(|rec| rec.isec).chain(self.fdes.iter().map(|f| f.isec));
        let mut sects: Vec<u32> = isecs
            .map(|isec| self.isecs[isec as usize].shndx)
            .filter(|&shndx| !is_code_section(&self.sect_hdrs[shndx as usize]))
            .collect();
        sects.sort_unstable();
        sects.dedup();
        let file = crate::passes::resolved_file_name(self.mf);
        let warnings = sects
            .into_iter()
            .map(|shndx| {
                let sect = &self.sect_hdrs[shndx as usize];
                format!(
                    "symbols in {},{} ({file}) have unwind information, but it's not a code \
                     section (missing 'regular,pure_instructions' section flag)",
                    sect.segname(),
                    sect.sectname(),
                )
            })
            .collect();
        UnwindCheck { warnings, data_fde: self.data_fde.then_some(file) }
    }

    /// The first CFString constant ld-prime refuses as it reads the
    /// object, by its section and what is wrong: one of a
    /// __DATA,__cfstring section's 32-byte records must have just two
    /// relocations, one setting its class pointer at offset 0 and one
    /// its string's at 16.
    fn bad_cfstring(&self) -> Option<(usize, &'static str)> {
        self.isecs.iter().find_map(|isec| {
            let shndx = isec.shndx as usize;
            let hdr = &self.sect_hdrs[shndx];
            if (hdr.segname(), hdr.sectname()) != ("__DATA", "__cfstring")
                || hdr.section_type() != S_REGULAR
                || isec.size == 0
            {
                return None;
            }
            let rels = &self.relocs[isec.rel_offset as usize..][..isec.nrels as usize];
            let at = |off| rels.iter().any(|r| r.offset == off);
            let why = if rels.len() != 2 {
                "cfstring constant does not have two fixups"
            } else if !at(0) {
                "cfstring constant isa not at offset 0 in cfstring object"
            } else if !at(16) {
                "cfstring constant string-data not at offset 16 in cfstring object"
            } else {
                return None;
            };
            Some((shndx, why))
        })
    }

    /// The first pointer, an atom of ld-prime's own, that has no
    /// relocation to name its target though ld-prime requires one: an
    /// initializer or terminator pointer, which names a function and is
    /// an atom of mold's too, or an entry of __objc_clsrolist, which
    /// lists the class_ro_t records of Swift's generic classes. mold
    /// keeps that list whole: the compiler marks only the symbol at its
    /// start no-dead-strip, and what it lists must stay for the method
    /// lists to be rewritten (ld-prime reads it before dead stripping).
    /// ld-prime's check of the list is preceded by an assertion that
    /// trips on it instead.
    fn pointer_without_target(&self) -> Option<&'static str> {
        self.isecs.iter().find_map(|isec| {
            let hdr = &self.sect_hdrs[isec.shndx as usize];
            let rels = &self.relocs[isec.rel_offset as usize..][..isec.nrels as usize];
            if matches!(hdr.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS) {
                (isec.size != 0 && rels.is_empty()).then_some("initializer pointer")
            } else if hdr.segname() == "__DATA" && hdr.sectname() == "__objc_clsrolist" {
                let bare = |off| !rels.iter().any(|r| r.offset as u64 == off);
                (0..isec.size as u64).step_by(8).any(bare).then_some("__objc_clsrolist pointer")
            } else {
                None
            }
        })
    }
}

/// Whether ld-prime takes a section for code, which is what unwind info
/// describes: __TEXT,__text and __TEXT,__StaticInit by their names, and
/// any other with S_ATTR_PURE_INSTRUCTIONS that is not one it knows for
/// data.
fn is_code_section(hdr: &MachSection) -> bool {
    matches!((hdr.segname(), hdr.sectname()), ("__TEXT", "__text" | "__StaticInit"))
        || (hdr.flags & S_ATTR_PURE_INSTRUCTIONS != 0 && !is_typed_data_section(hdr))
}

/// Whether ld-prime gives a section's contents a kind of their own that
/// no function has: by the section's type (strings, literals, non-lazy
/// pointers, zero fill, thread-local data) or, for a regular section,
/// by a name it knows data by, whatever its attributes. Any other
/// section that is not code holds contents of no kind it knows, which
/// an FDE may describe.
fn is_typed_data_section(hdr: &MachSection) -> bool {
    match hdr.section_type() {
        S_ZEROFILL
        | S_GB_ZEROFILL
        | S_THREAD_LOCAL_ZEROFILL
        | S_THREAD_LOCAL_REGULAR
        | S_CSTRING_LITERALS
        | S_4BYTE_LITERALS
        | S_8BYTE_LITERALS
        | S_16BYTE_LITERALS
        | S_NON_LAZY_SYMBOL_POINTERS => true,
        S_REGULAR => matches!(
            (hdr.segname(), hdr.sectname()),
            (
                "__TEXT",
                "__const"
                    | "__ustring"
                    | "__gcc_except_tab"
                    | "__objc_classname"
                    | "__objc_methname"
                    | "__objc_methtype"
                    | "__objc_methlist"
            ) | (
                "__DATA",
                "__data"
                    | "__const"
                    | "__got"
                    | "__auth_ptr"
                    | "__const_cfobj2"
                    | "__objc_data"
                    | "__objc_const"
                    | "__objc_ivar"
                    | "__objc_selrefs"
                    | "__objc_classrefs"
                    | "__objc_superrefs"
                    | "__objc_protorefs"
                    | "__objc_protolist"
                    | "__objc_nlclslist"
                    | "__objc_nlcatlist"
                    | "__objc_intobj"
                    | "__objc_floatobj"
                    | "__objc_doubleobj"
                    | "__objc_dateobj"
                    | "__objc_dictobj"
                    | "__objc_arrayobj"
                    | "__objc_arraydata"
            ) | ("__LD", "__func_variants")
        ),
        _ => false,
    }
}

/// Pre-applies an __eh_frame's relocations to its contents, as ld-prime
/// reads them, so that the records' pointers become plain values: a
/// SUBTRACTOR adds the next relocation's target, whatever its type, less
/// its own, and an UNSIGNED of no pair adds its target. Its GOT-relative
/// relocations, a CIE's personality reference, are left for
/// parse_eh_frame; there may be no other kind.
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
    nlists: &[NList],
    file_name: &Path,
) {
    // Diagnostics spell the path lossily.
    let file_name = file_name.display();
    let target = |r: MachRel| {
        if r.is_extern() { nlists[r.r_symbolnum() as usize].n_value } else { 0 }
    };
    let mut i = 0;
    while i < rels.len() {
        // ld-prime checks a relocation's offset, type and size, in that
        // order, but not those of a SUBTRACTOR's partner. It lets a
        // field start at the section's end, which this does not.
        let r = rels[i];
        let off = r.r_address as usize;
        let beyond_end = || -> ! {
            fatal!(
                "malformed __eh_frame relocation, offset (0x{off:08X}) is beyond end of \
                 section, in '{file_name}'"
            )
        };
        if off > contents.len() {
            beyond_end();
        }
        let ty = r.r_type();
        if ty != E::RELOC_UNSIGNED && ty != E::RELOC_SUBTRACTOR && ty != E::RELOC_GOTPC {
            fatal!(
                "__eh_frame unexpected relocation type ({ty}) at r_address=0x{off:08X} in \
                 '{file_name}'"
            );
        }
        let size = match r.r_length() {
            2 => 4,
            3 => 8,
            len => fatal!(
                "__eh_frame unexpected relocation size ({len}) at r_address=0x{off:08X} in \
                 '{file_name}'"
            ),
        };
        if off + size > contents.len() {
            beyond_end();
        }
        i += 1;

        let val = if ty == E::RELOC_SUBTRACTOR {
            let Some(&plus) = rels.get(i) else {
                fatal!(
                    "malformed __eh_frame relocation, SUBTRACTOR at offset (0x{off:08X}) has \
                     no pair, in '{file_name}'"
                );
            };
            i += 1;
            target(plus).wrapping_sub(target(r))
        } else if ty == E::RELOC_UNSIGNED {
            target(r)
        } else {
            continue;
        };
        let loc = &mut contents[off..off + size];
        if size == 4 {
            let old = u32::from_le_bytes(loc.try_into().unwrap());
            loc.copy_from_slice(&old.wrapping_add(val as u32).to_le_bytes());
        } else {
            let old = u64::from_le_bytes(loc.try_into().unwrap());
            loc.copy_from_slice(&old.wrapping_add(val).to_le_bytes());
        }
    }
}

/// Reports an __eh_frame record, at `pos` in the section, too short for
/// its fields.
fn truncated_cfi(file_name: &Path, pos: usize) -> ! {
    fatal!(
        "{}: malformed __eh_frame section: CFI at 0x{pos:08X} is truncated",
        file_name.display()
    );
}

/// Returns the position past the ULEB128 number at `pos` in `data`, or
/// None if the number runs past the end.
fn skip_uleb(data: &[u8], pos: usize) -> Option<usize> {
    let len = data.get(pos..)?.iter().position(|&b| b & 0x80 == 0)?;
    Some(pos + len + 1)
}

// DWARF pointer encodings (DW_EH_PE_*): the low four bits give the
// format, the next three what the value is relative to.
const DW_EH_PE_ABSPTR: u8 = 0x00;
const DW_EH_PE_SDATA4: u8 = 0x0b;
const DW_EH_PE_SDATA8: u8 = 0x0c;
const DW_EH_PE_PCREL: u8 = 0x10;

/// The size of a pointer __eh_frame encodes with `enc`, if ld-prime
/// reads that encoding: an 8-byte value (DW_EH_PE_absptr or
/// DW_EH_PE_sdata8) or a sign-extended 4-byte one (DW_EH_PE_sdata4),
/// absolute or relative to its own address (DW_EH_PE_pcrel). The top
/// bit, an indirection (DW_EH_PE_indirect), does not change it.
fn pointer_size(enc: u8) -> Option<usize> {
    if enc & 0x70 != DW_EH_PE_ABSPTR && enc & 0x70 != DW_EH_PE_PCREL {
        return None;
    }
    match enc & 0xf {
        DW_EH_PE_ABSPTR | DW_EH_PE_SDATA8 => Some(8),
        DW_EH_PE_SDATA4 => Some(4),
        _ => None,
    }
}

/// Reads the pointer at `pos` of an __eh_frame record at input address
/// `rec_addr`, in encoding `enc` (one pointer_size takes), and returns
/// the address it names. ld-prime reads an indirect pointer, whatever
/// it is relative to, as the address itself; it refuses the encoding
/// only afterwards.
fn read_pointer(rec: &[u8], pos: usize, enc: u8, rec_addr: u32) -> u64 {
    let val = match enc & 0xf {
        DW_EH_PE_SDATA4 => i32::from_le_bytes(rec[pos..pos + 4].try_into().unwrap()) as i64,
        _ => i64::from_le_bytes(rec[pos..pos + 8].try_into().unwrap()),
    };
    if enc & 0xf0 == DW_EH_PE_PCREL {
        (rec_addr as u64 + pos as u64).wrapping_add_signed(val)
    } else {
        val as u64
    }
}

/// Reads a CIE's version and augmentation, checking that they are ones
/// the linker knows, and returns how its FDEs encode their function and
/// their LSDA pointer (see Cie::fde_enc and Cie::lsda_enc); None if the
/// CIE ends before its augmentation data does.
fn parse_cie_augmentation(data: &[u8], file_name: &Path) -> Option<(u8, Option<u8>)> {
    // Diagnostics spell the path lossily.
    let file_name = file_name.display();
    // The version byte follows the length and the CIE ID, then the
    // augmentation string.
    let version = *data.get(8)?;
    if version != 1 && version != 3 {
        fatal!("CIE version is not 1 or 3 in '{file_name}'");
    }
    let aug_start = 9;
    if data.get(aug_start).copied() != Some(b'z') {
        return Some((DW_EH_PE_ABSPTR, None));
    }
    let aug_end = aug_start + data[aug_start..].iter().position(|&b| b == 0)?;
    // The code and data alignment factors, the return address register
    // and the augmentation data's length.
    let mut pos = aug_end + 1;
    for _ in 0..4 {
        pos = skip_uleb(data, pos)?;
    }
    let mut fde_enc = DW_EH_PE_ABSPTR;
    let mut lsda_enc = None;
    for &c in &data[aug_start + 1..aug_end] {
        match c {
            b'L' => {
                lsda_enc = Some(*data.get(pos)?);
                pos += 1;
            }
            // The personality's encoding, then the pointer: compilers
            // write 0x9b, a 4-byte pc-relative reference to its GOT slot
            // (DW_EH_PE_indirect|DW_EH_PE_pcrel|DW_EH_PE_sdata4), but
            // ld-prime reads any encoding it knows, finding the
            // personality by the GOT-relative relocation alone.
            b'P' => {
                let enc = *data.get(pos)?;
                let Some(size) = pointer_size(enc) else {
                    fatal!("unsupported pointer encoding 0x{enc:02X} in '{file_name}'");
                };
                pos += 1 + size;
            }
            b'R' => {
                fde_enc = *data.get(pos)?;
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
    (pos <= data.len()).then_some((fde_enc, lsda_enc))
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
    let hdr = MachHeader::read_from(data);
    if hdr.magic != MH_MAGIC_64 {
        return false;
    }
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        if lc.cmd == LC_SEGMENT_64 {
            let seg = SegmentCommand::read_from(&data[off..]);
            for i in 0..seg.nsects as usize {
                let sect_off = off + size_of::<SegmentCommand>() + i * size_of::<MachSection>();
                let sect = MachSection::read_from(&data[sect_off..]);
                if matches!(
                    sect.sectname(),
                    "__objc_classlist" | "__objc_catlist" | "__objc_nlclslist" | "__objc_nlcatlist"
                ) || (sect.segname() == "__TEXT" && sect.sectname().starts_with("__swift"))
                {
                    return true;
                }
            }
        }
        off += lc.cmdsize as usize;
    }
    false
}

/// An architecture's name as ld-prime spells it, from a Mach-O CPU type
/// and subtype.
fn arch_name(cputype: u32, cpusubtype: u32) -> &'static str {
    match (cputype, cpusubtype & !CPU_SUBTYPE_MASK) {
        (CPU_TYPE_X86_64, CPU_SUBTYPE_X86_64_H) => "x86_64h",
        (CPU_TYPE_X86_64, _) => "x86_64",
        (CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E) => "arm64e",
        (CPU_TYPE_ARM64, _) => "arm64",
        (CPU_TYPE_ARM64_32, _) => "arm64_32",
        (CPU_TYPE_I386, _) => "i386",
        (CPU_TYPE_ARM, 6) => "armv6",
        (CPU_TYPE_ARM, 9) => "armv7",
        (CPU_TYPE_ARM, 11) => "armv7s",
        (CPU_TYPE_ARM, 12) => "armv7k",
        (CPU_TYPE_ARM, 14) => "armv6m",
        (CPU_TYPE_ARM, 15) => "armv7m",
        (CPU_TYPE_ARM, 16) => "armv7em",
        (CPU_TYPE_ARM, _) => "arm",
        (CPU_TYPE_POWERPC, _) => "ppc",
        _ => "unknown",
    }
}

/// Whether a Mach-O file of `filetype` built for `cputype` and
/// `cpusubtype` is one the link takes: an object must be for exactly its
/// architecture, while a dylib serves every link of its CPU type (an
/// arm64e one an arm64 link too).
fn takes_arch<E: Target>(filetype: u32, cputype: u32, cpusubtype: u32) -> bool {
    match filetype {
        MH_DYLIB => cputype == E::CPUTYPE,
        _ => arch_name(cputype, cpusubtype) == E::NAME,
    }
}

/// The architecture of a thin object or dylib the link doesn't take
/// (see takes_arch), which ld-prime ignores with a warning.
pub fn foreign_arch<E: Target>(mf: &MappedFile) -> Option<&'static str> {
    let hdr = MachHeader::read_from(mf.data());
    (!takes_arch::<E>(hdr.filetype, hdr.cputype, hdr.cpusubtype))
        .then(|| arch_name(hdr.cputype, hdr.cpusubtype))
}

/// Whether a thin file the link doesn't take is of its CPU type all the
/// same, an x86_64h object in an x86_64 link: -allow_sub_type_mismatches
/// has ld-prime take it, but for arm64e, whose pointers are signed.
pub fn is_subtype_mismatch<E: Target>(mf: &MappedFile) -> bool {
    let hdr = MachHeader::read_from(mf.data());
    hdr.cputype == E::CPUTYPE && arch_name(hdr.cputype, hdr.cpusubtype) != "arm64e"
}

/// A fat (universal) file's slices: each one's CPU type, subtype, file
/// offset and size. Fat headers are big-endian.
fn fat_arches(mf: &MappedFile) -> impl Iterator<Item = (u32, u32, usize, usize)> + '_ {
    let data = mf.data();
    let read_be32 = |off: usize| u32::from_be_bytes(data[off..off + 4].try_into().unwrap());
    (0..read_be32(4) as usize).map(move |i| {
        let off = 8 + i * 20;
        (
            read_be32(off),
            read_be32(off + 4),
            read_be32(off + 8) as usize,
            read_be32(off + 12) as usize,
        )
    })
}

/// The architectures a fat file has slices for.
fn fat_arch_names(mf: &MappedFile) -> Vec<&'static str> {
    fat_arches(mf).map(|(cputype, cpusubtype, _, _)| arch_name(cputype, cpusubtype)).collect()
}

/// What ld-prime refuses of an object file's layout before it reads the
/// object: load commands, a symbol table or its strings running past
/// the end of the file. (One a few bytes short of a mach header is
/// "buffer too small", see passes::collect_file.)
pub fn malformed_object(data: &[u8]) -> Option<&'static str> {
    let hdr = MachHeader::read_from(data);
    let end = data.len() as u64;
    let cmds_end = size_of::<MachHeader>() + hdr.sizeofcmds as usize;
    if cmds_end as u64 > end {
        return Some("mh.sizeofcmds extends beyond buffer size");
    }
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        if off + size_of::<SymtabCommand>() > cmds_end {
            break;
        }
        let lc = SymtabCommand::read_from(&data[off..]);
        if lc.cmd == LC_SYMTAB {
            if lc.symoff as u64 + lc.nsyms as u64 * size_of::<NList>() as u64 > end {
                return Some("LINKEDIT content 'symbol table' extends beyond end of segment");
            }
            if lc.stroff as u64 + lc.strsize as u64 > end {
                return Some("LINKEDIT content 'symbol strings' extends beyond end of segment");
            }
        }
        if lc.cmdsize < 8 {
            break;
        }
        off += lc.cmdsize as usize;
    }
    None
}

/// The slice of a fat file the link takes (see takes_arch), if any: the
/// one for exactly its architecture first, but for a dylib whose subtype
/// must match (Args::dylib_subtypes_must_match). With
/// -allow_sub_type_mismatches, one of another subtype of its CPU type
/// will do too, as for a thin file (see is_subtype_mismatch).
pub fn fat_slice<E: Target>(
    args: &crate::cmdline::Args,
    mf: &'static MappedFile,
) -> Option<&'static MappedFile> {
    let slices: Vec<_> = fat_arches(mf).collect();
    let (_, _, off, size) = slices
        .iter()
        .find(|&&(cputype, cpusubtype, _, _)| arch_name(cputype, cpusubtype) == E::NAME)
        .or_else(|| {
            slices.iter().find(|&&(cputype, cpusubtype, off, _)| {
                let filetype = MachHeader::read_from(&mf.data()[off..]).filetype;
                !args.dylib_subtypes_must_match && takes_arch::<E>(filetype, cputype, cpusubtype)
            })
        })
        .or_else(|| {
            slices.iter().find(|&&(cputype, cpusubtype, _, _)| {
                args.allow_sub_type_mismatches
                    && cputype == E::CPUTYPE
                    && arch_name(cputype, cpusubtype) != "arm64e"
            })
        })
        .copied()?;
    let mut name = std::ffi::OsString::from(&mf.name);
    name.push(format!("(for architecture {})", E::NAME));
    Some(mf.slice(name.into(), off, size))
}

/// Ignores an input file without the link's architecture, as ld-prime
/// does: with a warning, or with an error under -arch_errors_fatal.
pub fn ignore_foreign_file<E: Target>(ctx: &Context<E>, mf: &MappedFile, why: &str) {
    if ctx.args.arch_errors_fatal {
        crate::error!("{why} in '{}'", mf.name.display());
    } else {
        crate::warn!("ignoring file '{}': {why}", mf.name.display());
    }
}

/// Ignores a fat file without a slice the link takes.
pub fn warn_fat_missing_arch<E: Target>(ctx: &Context<E>, mf: &MappedFile) {
    let arches = fat_arch_names(mf).join(",");
    let why = format!("fat file missing arch '{}', file has '{arches}'", E::NAME);
    ignore_foreign_file(ctx, mf, &why);
}

/// Parses a Mach-O dylib binary: its identity from LC_ID_DYLIB and its
/// exported symbols. The defined-external range of the symbol table
/// serves as the export list; the authoritative source is the export
/// trie, but the symbol table matches it for the dylibs we link against.
/// Whether a re-exported dylib at this install path may be bound to
/// directly: ld64's "public location" rule. /usr/lib/lib*.dylib (not
/// /usr/lib/system/) and a top-level /System/Library/Frameworks
/// framework are public; a private framework, a sub-framework or a
/// libSystem component is not, and its symbols bind to the dylib that
/// re-exports it (AppKit re-exports Foundation, public, and
/// UIFoundation, private: ld-prime binds NSHomeDirectory to Foundation
/// and NSAttachmentAttributeName to AppKit).
pub fn is_public_location(install_name: &[u8]) -> bool {
    if let Some(rest) = install_name.strip_prefix(b"/usr/lib/") {
        return !rest.contains(&b'/');
    }
    if let Some(rest) = install_name.strip_prefix(b"/System/Library/Frameworks/") {
        // Only a top-level framework: X.framework/... with no further
        // Frameworks directory in the path.
        if let Some(dot) = memchr::memmem::find(rest, b".framework/") {
            return memchr::memmem::find(&rest[dot + ".framework/".len()..], b".framework/")
                .is_none();
        }
    }
    false
}

/// Notes an input file for -t as it is loaded. ld-prime names a fat
/// file's slice, and its members, by the file's own path.
pub fn trace_file<E: Target>(ctx: &mut Context<E>, name: &[u8]) {
    if ctx.args.trace {
        ctx.traced_files.push(trace_name(name));
    }
}

/// Takes back the -t line of a library loaded as another's re-export
/// that a naming finds at another path (libobjc.tbd for Foundation's
/// libobjc.A.tbd): ld-prime lists the dylib by the naming's file.
pub fn untrace_file<E: Target>(ctx: &mut Context<E>, name: &[u8]) {
    if ctx.args.trace {
        let name = trace_name(name);
        if let Some(i) = ctx.traced_files.iter().position(|traced| *traced == name) {
            ctx.traced_files.remove(i);
        }
    }
}

pub fn trace_name(name: &[u8]) -> String {
    crate::util::display(&without_fat_arch(name)).to_string()
}

/// Splits the name fat_slice gives a fat file's slice into the file's
/// path and the slice's architecture.
pub fn split_fat_arch(name: &[u8]) -> (&[u8], Option<&[u8]>) {
    const TAG: &[u8] = b"(for architecture ";
    match memchr::memmem::find(name, TAG) {
        Some(i) if name.ends_with(b")") => (&name[..i], Some(&name[i + TAG.len()..name.len() - 1])),
        _ => (name, None),
    }
}

/// A file's name without the "(for architecture ...)" that fat_slice
/// gives a fat file's slice, which ld-prime never shows: it names the
/// slice, and the members of a fat archive, by the file's own path.
pub fn without_fat_arch(name: &[u8]) -> Vec<u8> {
    let mut name = name.to_vec();
    if let Some(i) = memchr::memmem::find(&name, b"(for architecture")
        && let Some(len) = name[i..].iter().position(|&c| c == b')')
    {
        name.drain(i..=i + len);
    }
    name
}

/// Loads the libraries a dylib re-exports. A public one becomes an
/// implicit dylib of its own (its symbols bind to it), recursively
/// loading what it re-exports in turn; a private one's exports are
/// merged into `exports`, `tlv_exports` and `weak_exports` as the
/// re-exporting dylib's, and its own re-exports are walked the same
/// way. A library may be a file of its own or a document inlined in a
/// stub (`documents`: the re-exporting stub's). A public one is loaded
/// from its file when one exists, as ld-prime does, and from its
/// document otherwise; a private one inlined is merged from its
/// document. Returns the install names of the private libraries merged,
/// with -map or -why_live the files they are, and the exports of theirs
/// that $ld$previous directives move to older libraries.
/// Notes the file of a library loaded as another's re-export for the
/// -dependency_info file, which names it.
fn note_reexport_file<E: Target>(ctx: &mut Context<E>, path: &Path) {
    if ctx.args.dependency_info.is_some() {
        ctx.reexport_files.push(path.to_path_buf());
    }
}

/// A dylib whose re-exports load_reexports loads: its install name,
/// its file, and how many platforms it has for the target (one for a
/// binary; see trace_reexports).
struct ReexportParent<'a> {
    install_name: &'a [u8],
    path: &'a Path,
    platforms: usize,
}

/// Notes for -trace_implicit_libraries the libraries `names` that
/// `parent` re-exports: a stub's once for each platform it has for the
/// target (a zippered one's for macOS and Mac Catalyst alike), a
/// binary's once.
fn trace_reexports<'a, E: Target>(
    ctx: &mut Context<E>,
    parent: &ReexportParent,
    names: impl Iterator<Item = &'a [u8]>,
) {
    use crate::passes::ImplicitTrace;
    let args = &ctx.args;
    if !args.trace_implicit_libraries && args.trace_implicit_library.is_empty() {
        return;
    }
    let file = crate::passes::real_path(parent.path).0;
    for name in names.filter(|name| crate::passes::traces_implicit(args, name)) {
        let line = format!(
            "indirect library '{}' from file '{}'",
            crate::util::display(name),
            file.display()
        );
        for _ in 0..parent.platforms {
            let (parent, name) = (parent.install_name.to_vec(), name.to_vec());
            ctx.implicit_trace.push(ImplicitTrace::Reexport { parent, name, line: line.clone() });
        }
    }
}

fn load_reexports<E: Target>(
    ctx: &mut Context<E>,
    reexports: Vec<(Vec<u8>, PathBuf, Vec<PathBuf>)>,
    parent: ReexportParent,
    documents: Vec<tapi::TbdFile>,
    exports: &mut hashbrown::HashSet<&'static str>,
    tlv_exports: &mut hashbrown::HashSet<&'static str>,
    weak_exports: &mut hashbrown::HashSet<&'static str>,
) -> (Vec<Vec<u8>>, Vec<MergedFile>, Vec<MovedExport>) {
    trace_reexports(ctx, &parent, reexports.iter().map(|(name, ..)| name.as_slice()));
    let parent = parent.path;
    let mut walk = ReexportWalk {
        queue: reexports,
        pool: documents,
        exports,
        tlv_exports,
        weak_exports,
        moved: Vec::new(),
    };
    let mut visited = std::collections::HashSet::new();
    let mut merged = Vec::new();
    let mut merged_files = Vec::new();
    let map = ctx.args.merged_files;
    let mut record = |install_name: &[u8], path: &Path, exports: Vec<&'static str>| {
        let (install_name, path) = (install_name.to_vec(), path.to_path_buf());
        merged_files.push(MergedFile { install_name, path, exports });
    };
    let all_exports = |tbd: &tapi::TbdFile| -> Vec<&'static str> {
        let all = [&tbd.exports, &tbd.weak_exports, &tbd.tlv_exports];
        all.into_iter().flatten().copied().collect()
    };
    while let Some((name, loader_dir, loader_rpaths)) = walk.queue.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let public = !ctx.args.no_implicit_dylibs && is_public_location(&name);
        // A library already in the link, matched by install name
        // (libXCTestSwiftSupport re-exports @rpath/XCTest.framework/...,
        // which its own rpaths cannot reach but -framework XCTest has
        // loaded): its symbols bind to it if it is public, else count
        // as this dylib's.
        if let Some(loaded) = ctx.dylibs.iter().find(|d| d.install_name == name) {
            if !public {
                walk.exports.extend(loaded.exports.iter().copied());
                walk.tlv_exports.extend(loaded.tlv_exports.iter().copied());
                walk.weak_exports.extend(loaded.weak_exports.iter().copied());
                if map {
                    record(&name, &loaded.path, loaded.exports.iter().copied().collect());
                }
                merged.push(name);
            }
            continue;
        }
        let inline = walk.pool.iter().position(|d| d.install_name.as_bytes() == name);
        let on_disk = if inline.is_some() && !public {
            None
        } else {
            resolve_dylib_ref(ctx, &name, &loader_dir, &loader_rpaths)
        };
        if on_disk.is_none()
            && let Some(i) = inline
        {
            // ld-prime names an inlined library by the file it would
            // find for it, where there is one.
            if ctx.args.trace || ctx.args.dependency_info.is_some() {
                let found = resolve_dylib_ref(ctx, &name, &loader_dir, &loader_rpaths);
                if let Some(mf) = found {
                    note_reexport_file(ctx, &mf.name);
                }
                let found = found.map(|mf| crate::util::path_bytes(&mf.name).to_vec());
                trace_file(ctx, found.as_deref().unwrap_or(&name));
            }
            let mut doc = walk.pool[i].clone();
            if DylibIdentity::of_tbd(&doc).is_public(ctx) {
                let idx = register_tbd(ctx, parent, doc, walk.pool.clone());
                ctx.dylibs[idx].is_implicit = true;
                continue;
            }
            walk.moved.extend(interpret_ld_symbols(ctx, &mut doc).moved);
            if map {
                let found = resolve_dylib_ref(ctx, &name, &loader_dir, &loader_rpaths);
                let path = found.map_or(Path::new(crate::util::os_str(&name)), |mf| &mf.name);
                record(&name, path, all_exports(&doc));
            }
            walk.merge_tbd(doc, &loader_dir, &loader_rpaths);
            merged.push(name);
            continue;
        }
        let Some(dep) = on_disk else {
            crate::warn!(
                "ignoring missing indirect library: library for install name '{}' not found",
                crate::util::display(&name)
            );
            continue;
        };
        // A file of another kind, which only a -dylib_file names, is
        // one ld-prime loads as any input.
        use crate::filetype::FileType;
        let ty = crate::filetype::get_file_type(dep);
        if !matches!(ty, FileType::Tapi | FileType::Dylib | FileType::Fat) {
            ctx.indirect_files.push(dep);
            continue;
        }
        trace_file(ctx, crate::util::path_bytes(&dep.name));
        note_reexport_file(ctx, &dep.name);
        // The file found decides by its own install name, which a lookup
        // by leaf name may find to differ from the one re-exported:
        // ld-prime binds to libz a symbol of /opt/x/libz.dylib that it
        // found as the SDK's /usr/lib/libz.1.dylib, and merges a
        // /usr/lib/libq.dylib found as /opt/q/libq.dylib.
        match ty {
            FileType::Tapi => {
                let Some(mut dep_tbd) = load_tbd(ctx, dep) else { continue };
                if DylibIdentity::of_tbd(&dep_tbd).is_public(ctx) {
                    let idx = register_tbd_file(ctx, dep, dep_tbd);
                    ctx.dylibs[idx].is_implicit = true;
                    continue;
                }
                merged.push(dep_tbd.install_name.as_bytes().to_vec());
                walk.moved.extend(interpret_ld_symbols(ctx, &mut dep_tbd).moved);
                if map {
                    record(dep_tbd.install_name.as_bytes(), &dep.name, all_exports(&dep_tbd));
                }
                walk.merge_tbd(dep_tbd, &dir_of(&dep.name), &[]);
            }
            _ => {
                // A universal binary (Xcode's XCTestCore, re-exported by
                // XCTest) is read for the target's slice, if it has one.
                let binary = match ty {
                    FileType::Dylib => dep,
                    _ => match fat_slice::<E>(&ctx.args, dep) {
                        Some(slice) => slice,
                        None => {
                            warn_fat_missing_arch(ctx, dep);
                            continue;
                        }
                    },
                };
                let found = DylibIdentity::of_binary(binary);
                if found.is_public(ctx) {
                    let idx = parse_dylib_binary(ctx, binary);
                    ctx.dylibs[idx].is_implicit = true;
                    continue;
                }
                check_dylib_platform(ctx, binary);
                let mut dylib = read_dylib_binary(binary);
                walk.moved.extend(interpret_binary_ld_symbols(ctx, &mut dylib).moved);
                if map {
                    record(&found.install_name, &dep.name, dylib.exports.clone());
                }
                merged.push(found.install_name);
                walk.merge_binary(dylib, &dir_of(&dep.name));
            }
        }
    }
    (merged, merged_files, walk.moved)
}

/// A dylib's re-exported libraries as load_reexports walks them: those
/// left to visit, each with the directory and rpaths its install name
/// resolves from; the inlined documents a name may resolve to (the
/// dylib's, and those of every stub merged along the way); and the
/// dylib's export sets, which the private ones merge into, with those
/// of their exports that move to older libraries.
struct ReexportWalk<'a> {
    queue: Vec<(Vec<u8>, PathBuf, Vec<PathBuf>)>,
    pool: Vec<tapi::TbdFile>,
    exports: &'a mut hashbrown::HashSet<&'static str>,
    tlv_exports: &'a mut hashbrown::HashSet<&'static str>,
    weak_exports: &'a mut hashbrown::HashSet<&'static str>,
    moved: Vec<MovedExport>,
}

impl ReexportWalk<'_> {
    /// Merges a private library's stub into the dylib: its exports join
    /// the dylib's, by kind, its inlined documents the pool, and the
    /// libraries it re-exports in turn the queue, to resolve from
    /// `loader_dir` and `loader_rpaths`.
    fn merge_tbd(&mut self, tbd: tapi::TbdFile, loader_dir: &Path, loader_rpaths: &[PathBuf]) {
        self.tlv_exports.extend(tbd.tlv_exports.iter().copied());
        self.exports.extend(tbd.tlv_exports);
        self.exports.extend(tbd.exports);
        self.weak_exports.extend(tbd.weak_exports.iter().copied());
        self.exports.extend(tbd.weak_exports);
        self.pool.extend(tbd.documents);
        for name in tbd.reexports {
            self.queue.push((
                name.as_bytes().to_vec(),
                loader_dir.to_path_buf(),
                loader_rpaths.to_vec(),
            ));
        }
    }

    /// Merges what a private library's binary contributes into the
    /// dylib likewise: the libraries it re-exports resolve from
    /// `loader_dir` and the binary's rpaths.
    fn merge_binary(&mut self, dylib: DylibBinary, loader_dir: &Path) {
        self.exports.extend(dylib.exports);
        self.tlv_exports.extend(dylib.tlv_exports);
        for name in dylib.reexports {
            self.queue.push((name, loader_dir.to_path_buf(), dylib.rpaths.clone()));
        }
    }
}

/// A dylib's install name and who may link it directly: the umbrella it
/// belongs to (LC_SUB_FRAMEWORK, a stub's parent-umbrella) and the
/// clients it names (LC_SUB_CLIENT, allowable-clients).
pub struct DylibIdentity {
    pub install_name: Vec<u8>,
    umbrella: Option<Vec<u8>>,
    clients: Vec<Vec<u8>>,
}

impl DylibIdentity {
    fn of_tbd(tbd: &tapi::TbdFile) -> Self {
        Self {
            install_name: tbd.install_name.as_bytes().to_vec(),
            umbrella: tbd.parent_umbrella.map(|u| u.as_bytes().to_vec()),
            clients: tbd.allowable_clients.iter().map(|c| c.as_bytes().to_vec()).collect(),
        }
    }

    fn of_binary(mf: &MappedFile) -> Self {
        let data = mf.data();
        let hdr = MachHeader::read_from(data);
        let mut id = Self { install_name: Vec::new(), umbrella: None, clients: Vec::new() };
        let mut off = size_of::<MachHeader>();
        for _ in 0..hdr.ncmds {
            let lc = LoadCommand::read_from(&data[off..]);
            let string = |nameoff: u32| {
                let name = &data[off + nameoff as usize..off + lc.cmdsize as usize];
                name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())].to_vec()
            };
            match lc.cmd {
                LC_ID_DYLIB => {
                    id.install_name = string(DylibCommand::read_from(&data[off..]).nameoff)
                }
                LC_SUB_FRAMEWORK => {
                    id.umbrella = Some(string(DylinkerCommand::read_from(&data[off..]).nameoff));
                }
                LC_SUB_CLIENT => {
                    id.clients.push(string(DylinkerCommand::read_from(&data[off..]).nameoff))
                }
                _ => {}
            }
            off += lc.cmdsize as usize;
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
        crate::filetype::FileType::Tapi => {
            DylibIdentity::of_tbd(&read_tbd(ctx, mf).unwrap_or_default())
        }
        _ => DylibIdentity::of_binary(mf),
    }
}

/// Whether the dylib in a stub or binary file exports a symbol that the
/// link uses and neither an object nor a dylib loaded so far defines
/// (SwiftUI, auto-linked before SwiftUICore, re-exports all of it).
pub fn provides_undefined<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> bool {
    let names: Vec<&'static str> = match crate::filetype::get_file_type(mf) {
        crate::filetype::FileType::Tapi => {
            let tbd = read_tbd(ctx, mf).unwrap_or_default();
            [tbd.exports, tbd.weak_exports, tbd.tlv_exports].concat()
        }
        _ => read_dylib_binary(mf).exports,
    };
    names.iter().any(|name| {
        ctx.symbols.get(name).is_some_and(|id| {
            let sym = &ctx.symbols[id];
            sym.is_used() && !sym.is_defined()
        }) && !ctx.dylibs.iter().any(|d| d.exports.contains(name))
    })
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
fn check_dylib_platform<E: Target>(ctx: &mut Context<E>, mf: &MappedFile) -> u32 {
    let hdr = MachHeader::read_from(mf.data());
    let mut versions = Vec::new();
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let data = &mf.data()[off..];
        let lc = LoadCommand::read_from(data);
        if is_platform_cmd(lc.cmd) {
            versions.push(PlatformVersion::read(lc.cmd, data, hdr.cputype));
        }
        off += lc.cmdsize as usize;
    }
    if let Some(version) = versions.iter().find(|v| v.platform == ctx.args.platform) {
        return version.minos;
    }
    if let Some(first) = versions.first() {
        // A zippered dylib has a build version for macOS and one for
        // Mac Catalyst.
        let platforms: Vec<u32> = versions.iter().map(|v| v.platform).collect();
        let name =
            if platforms.contains(&PLATFORM_MACOS) && platforms.contains(&PLATFORM_MACCATALYST) {
                "zippered(macOS/Catalyst)".to_string()
            } else {
                platform_name(first.platform)
            };
        check_dylib_platforms(ctx, mf, &platforms, &name);
    }
    0
}

/// Notes a dylib built for none of the link's platform - for
/// `platforms`, which `name` names - for ld-prime's message, which it
/// gives once it has read all the inputs, where it checks the input
/// with which the dylib came (see passes::check_input_versions).
fn check_dylib_platforms<E: Target>(
    ctx: &mut Context<E>,
    mf: &MappedFile,
    platforms: &[u32],
    name: &str,
) {
    if platforms.is_empty() || platforms.contains(&ctx.args.platform) {
        return;
    }
    let msg = format!(
        "building for '{}', but linking in dylib ({}) built for '{name}'",
        platform_name(ctx.args.platform),
        crate::passes::resolved_file_name(mf),
    );
    // The input's priority, or its parent's for a re-exported library.
    let priority = ctx.priority_counter + 1;
    ctx.foreign_platform_dylibs.push((priority, msg));
}

pub fn parse_dylib_binary<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) -> usize {
    let minos = check_dylib_platform(ctx, mf);
    let mut dylib = read_dylib_binary(mf);
    if dylib.install_name.is_empty() {
        fatal!("{}: dylib has no LC_ID_DYLIB", mf.name.display());
    }
    let directives = interpret_binary_ld_symbols(ctx, &mut dylib);
    let DylibBinary {
        install_name,
        current_version,
        compatibility_version,
        exports,
        weak_exports,
        tlv_exports,
        reexports,
        rpaths,
        ..
    } = dylib;
    let mut exports: hashbrown::HashSet<&'static str> = exports.into_iter().collect();
    let mut weak_exports: hashbrown::HashSet<&'static str> = weak_exports.into_iter().collect();
    let has_weak_defs = !weak_exports.is_empty();
    let mut tlv_exports: hashbrown::HashSet<&'static str> = tlv_exports.into_iter().collect();

    // Each re-exported library keeps the referencing dylib's directory
    // and rpaths, since @loader_path and @rpath in an install name are
    // relative to the referrer.
    let reexports: Vec<(Vec<u8>, PathBuf, Vec<PathBuf>)> =
        reexports.into_iter().map(|name| (name, dir_of(&mf.name), rpaths.clone())).collect();
    let parent = ReexportParent { install_name: &install_name, path: &mf.name, platforms: 1 };
    let (merged_reexports, merged_files, mut moved) = load_reexports(
        ctx,
        reexports,
        parent,
        Vec::new(),
        &mut exports,
        &mut tlv_exports,
        &mut weak_exports,
    );
    moved.extend(directives.moved);
    let moved_exports = add_moved_dylibs(ctx, &mf.name, moved, &exports);
    let name_source = if directives.renamed { NameSource::Directive } else { NameSource::Own };

    let priority = ctx.next_priority();
    add_dylib(
        ctx,
        DylibFile {
            path: mf.name.clone(),
            install_name,
            current_version,
            compatibility_version,
            minos,
            in_sdk: false,
            from_binary: true,
            dylib_idx: next_dylib_ordinal(ctx),
            is_bundle_loader: false,
            priority,
            is_weak: false,
            is_weak_asserted: false,
            is_reexported: false,
            binds_to_image: false,
            is_needed: false,
            is_upward: false,
            is_lazy: false,
            named_lazily: false,
            delay_init: None,
            named_at: None,
            is_autolinked: false,
            is_implicit: false,
            load_order: u32::MAX,
            exports,
            weak_exports,
            has_weak_defs,
            tlv_exports,
            merged_reexports,
            merged_files,
            moved_exports,
            named_files: Vec::new(),
            name_source,
        },
    )
}

/// Returns the 1-based ordinals of S_THREAD_LOCAL_VARIABLES sections.
fn thread_local_section_ordinals(data: &[u8], hdr: &MachHeader) -> Vec<u8> {
    let mut ordinals = Vec::new();
    let mut ordinal = 0u8;
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        if lc.cmd == LC_SEGMENT_64 {
            let seg = SegmentCommand::read_from(&data[off..]);
            for i in 0..seg.nsects as usize {
                let sect = MachSection::read_from(
                    &data[off + size_of::<SegmentCommand>() + i * size_of::<MachSection>()..],
                );
                ordinal += 1;
                if sect.flags & SECTION_TYPE == S_THREAD_LOCAL_VARIABLES {
                    ordinals.push(ordinal);
                }
            }
        }
        off += lc.cmdsize as usize;
    }
    ordinals
}

/// The ordinal the next LC_LOAD_DYLIB will have: dylibs are numbered
/// in load-command order, and a -bundle_loader has no load command.
pub fn next_dylib_ordinal<E: Target>(ctx: &Context<E>) -> i32 {
    ctx.dylibs.iter().filter(|d| !d.is_bundle_loader).count() as i32 + 1
}

/// Where an image keeps its export trie: LC_DYLD_EXPORTS_TRIE, or the
/// export section of LC_DYLD_INFO(_ONLY).
fn find_export_trie(data: &[u8], hdr: &MachHeader) -> Option<(usize, usize)> {
    let mut trie = None;
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        match lc.cmd {
            LC_DYLD_EXPORTS_TRIE => {
                let cmd = LinkEditDataCommand::read_from(&data[off..]);
                trie = Some((cmd.dataoff as usize, cmd.datasize as usize));
            }
            LC_DYLD_INFO | LC_DYLD_INFO_ONLY => {
                let cmd = DyldInfoCommand::read_from(&data[off..]);
                if cmd.export_size != 0 {
                    trie = Some((cmd.export_off as usize, cmd.export_size as usize));
                }
            }
            _ => {}
        }
        off += lc.cmdsize as usize;
    }
    trie
}

/// The names in an export trie: what a stripped executable or dylib
/// exports.
fn export_trie_names(data: &[u8], off: usize, size: usize) -> Vec<&'static str> {
    export_trie_entries(data, off, size).into_iter().map(|(name, _)| name).collect()
}

/// The (name, flags) entries of an export trie. The trie is what dyld
/// binds against, and the one authoritative list of a dylib's exports:
/// a dylib's symbol table may keep only a handful of its defined
/// externals (Lottie.xcframework's ships 16 of 1846, the rest stripped),
/// so a linker that reads just the symbol table finds nothing to
/// resolve against. ld64 reads the trie.
fn export_trie_entries(data: &[u8], off: usize, size: usize) -> Vec<(&'static str, u64)> {
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
            // Individual edge labels need not end at UTF-8 boundaries.
            // Decode only after assembling the complete symbol name.
            names.push((
                String::leak(String::from_utf8_lossy(&prefix).into_owned()) as &'static str,
                flags,
            ));
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
    let hdr = MachHeader::read_from(data);

    let mut symtab_cmd = None;
    let mut dysymtab_cmd = None;
    let mut trie: Option<(usize, usize)> = None;
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        match lc.cmd {
            LC_SYMTAB => symtab_cmd = Some(SymtabCommand::read_from(&data[off..])),
            LC_DYSYMTAB => dysymtab_cmd = Some(DysymtabCommand::read_from(&data[off..])),
            LC_DYLD_EXPORTS_TRIE => {
                let cmd = LinkEditDataCommand::read_from(&data[off..]);
                trie = Some((cmd.dataoff as usize, cmd.datasize as usize));
            }
            LC_DYLD_INFO | LC_DYLD_INFO_ONLY => {
                let cmd = DyldInfoCommand::read_from(&data[off..]);
                if cmd.export_size != 0 {
                    trie = Some((cmd.export_off as usize, cmd.export_size as usize));
                }
            }
            _ => {}
        }
        off += lc.cmdsize as usize;
    }

    let mut exports: hashbrown::HashSet<&'static str> = hashbrown::HashSet::new();
    let mut tlv_exports: hashbrown::HashSet<&'static str> = hashbrown::HashSet::new();
    if let (Some(sym), Some(dysym)) = (symtab_cmd, dysymtab_cmd) {
        let nlists: Vec<NList> = read_array(data, sym.symoff as usize, sym.nsyms as usize);
        let strtab = &data[sym.stroff as usize..(sym.stroff + sym.strsize) as usize];
        // SAFETY: input files are leaked, so the string table lives for
        // the rest of the process.
        let strtab: &'static [u8] =
            validate_strtab(unsafe { std::mem::transmute::<&[u8], &'static [u8]>(strtab) });
        let tlv_sects = thread_local_section_ordinals(data, &hdr);
        let range = dysym.iextdefsym as usize..(dysym.iextdefsym + dysym.nextdefsym) as usize;
        for nlist in &nlists[range] {
            let name = symbol_name(strtab, nlist);
            if tlv_sects.contains(&nlist.n_sect) {
                tlv_exports.insert(name);
            }
            exports.insert(name);
        }
    }
    if let Some((off, size)) = trie {
        exports.extend(export_trie_names(data, off, size));
    }

    let priority = ctx.next_priority();
    add_dylib(
        ctx,
        DylibFile {
            path: mf.name.clone(),
            install_name: crate::util::path_bytes(&mf.name).to_vec(),
            current_version: encode_version(1, 0, 0),
            compatibility_version: encode_version(1, 0, 0),
            minos: 0,
            in_sdk: false,
            from_binary: true,
            dylib_idx: BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE,
            is_bundle_loader: true,
            priority,
            is_weak: false,
            is_weak_asserted: false,
            is_reexported: false,
            binds_to_image: false,
            is_needed: false,
            is_upward: false,
            is_lazy: false,
            named_lazily: false,
            delay_init: None,
            named_at: None,
            is_autolinked: false,
            is_implicit: false,
            load_order: u32::MAX,
            exports,
            weak_exports: hashbrown::HashSet::new(),
            has_weak_defs: false,
            tlv_exports,
            merged_reexports: Vec::new(),
            merged_files: Vec::new(),
            moved_exports: hashbrown::HashMap::new(),
            named_files: Vec::new(),
            name_source: NameSource::Own,
        },
    )
}

/// What a dylib binary says of itself: its install name and versions;
/// its exports - all of them, the weak and the thread-local ones again
/// by kind - and apart from them its "$ld$..." names (see
/// LdSymbols); and the install names it re-exports, with its rpaths,
/// resolved for its location, to look them up by.
#[derive(Default)]
struct DylibBinary {
    install_name: Vec<u8>,
    current_version: u32,
    compatibility_version: u32,
    exports: Vec<&'static str>,
    weak_exports: Vec<&'static str>,
    tlv_exports: Vec<&'static str>,
    ld_symbols: Vec<&'static str>,
    reexports: Vec<Vec<u8>>,
    rpaths: Vec<PathBuf>,
}

/// Whether a dylib is mergeable: -make_mergeable gave it its atoms
/// (LC_ATOM_INFO), which a stub never has.
pub fn is_mergeable(mf: &MappedFile) -> bool {
    if crate::filetype::get_file_type(mf) != crate::filetype::FileType::Dylib {
        return false;
    }
    let data = mf.data();
    let hdr = MachHeader::read_from(data);
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        if lc.cmd == LC_ATOM_INFO {
            return true;
        }
        off += lc.cmdsize as usize;
    }
    false
}

fn read_dylib_binary(mf: &'static MappedFile) -> DylibBinary {
    let data = mf.data();
    let hdr = MachHeader::read_from(data);
    let mut dylib = DylibBinary {
        current_version: encode_version(1, 0, 0),
        compatibility_version: encode_version(1, 0, 0),
        ..Default::default()
    };
    let mut symtab_cmd = None;
    let mut dysymtab_cmd = None;

    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        let string = |nameoff: u32| {
            let name = &data[off + nameoff as usize..off + lc.cmdsize as usize];
            &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())]
        };
        match lc.cmd {
            LC_ID_DYLIB => {
                let cmd = DylibCommand::read_from(&data[off..]);
                dylib.install_name = string(cmd.nameoff).to_vec();
                dylib.current_version = cmd.current_version;
                dylib.compatibility_version = cmd.compatibility_version;
            }
            LC_SYMTAB => symtab_cmd = Some(SymtabCommand::read_from(&data[off..])),
            LC_DYSYMTAB => dysymtab_cmd = Some(DysymtabCommand::read_from(&data[off..])),
            LC_REEXPORT_DYLIB => {
                let cmd = DylibCommand::read_from(&data[off..]);
                dylib.reexports.push(string(cmd.nameoff).to_vec());
            }
            LC_RPATH => {
                let cmd = DylinkerCommand::read_from(&data[off..]);
                dylib.rpaths.push(loader_rpath(&mf.name, string(cmd.nameoff)));
            }
            _ => {}
        }
        off += lc.cmdsize as usize;
    }

    let mut add = |name: &'static str, weak: bool, tlv: bool| {
        if name.starts_with("$ld$") {
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
    if let (Some(sym), Some(dysym)) = (symtab_cmd, dysymtab_cmd) {
        let nlists: Vec<NList> = read_array(data, sym.symoff as usize, sym.nsyms as usize);
        let strtab = &data[sym.stroff as usize..(sym.stroff + sym.strsize) as usize];
        // SAFETY: input files are leaked, so the string table lives for
        // the rest of the process.
        let strtab: &'static [u8] =
            validate_strtab(unsafe { std::mem::transmute::<&[u8], &'static [u8]>(strtab) });
        // A TLV export is recognizable by its section: n_sect names a
        // S_THREAD_LOCAL_VARIABLES section (the __thread_vars
        // descriptors).
        let tlv_sects = thread_local_section_ordinals(data, &hdr);
        let range = dysym.iextdefsym as usize..(dysym.iextdefsym + dysym.nextdefsym) as usize;
        for nlist in &nlists[range] {
            let weak = nlist.n_desc & N_WEAK_DEF != 0;
            add(symbol_name(strtab, nlist), weak, tlv_sects.contains(&nlist.n_sect));
        }
    }
    if let Some((off, size)) = find_export_trie(data, &hdr) {
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

/// An LC_RPATH entry as a search directory: @loader_path stands for the
/// directory of the dylib that carries the entry.
fn loader_rpath(dylib: &Path, rpath: &[u8]) -> PathBuf {
    match rpath.strip_prefix(b"@loader_path/") {
        Some(rest) => dir_of(dylib).join(crate::util::os_str(rest)),
        None => PathBuf::from(crate::util::os_str(rpath)),
    }
}

/// The directory dyld would use for a dylib's @loader_path: that of
/// the real file, symlinks resolved. A framework's X.framework/X is a
/// symlink to Versions/A/X, and its LC_RPATH entries are written for
/// that location (XCTest's `@loader_path/../../../../PrivateFrameworks`
/// reaches XCTestCore only from Versions/A). A fat file's name may
/// carry the "(for architecture ...)" suffix the loader adds.
fn dir_of(path: &Path) -> PathBuf {
    let bytes = crate::util::path_bytes(path);
    let end = memchr::memmem::find(bytes, b"(for architecture").unwrap_or(bytes.len());
    let path = Path::new(crate::util::os_str(&bytes[..end]));
    if let Ok(real) = std::fs::canonicalize(path)
        && let Some(dir) = real.parent()
    {
        return dir.to_path_buf();
    }
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Resolves a dependent dylib's install name the way dyld would, but
/// at link time: @loader_path is the directory of the dylib that
/// names the dependency, and @rpath tries that dylib's own LC_RPATH
/// entries. ld-prime expands no @executable_path (ld64 took the output
/// executable's directory, or -executable_path's), so such a name
/// resolves only by its leaf. A -dylib_file for the name comes first,
/// unless its file isn't there; one that is ld-prime reads as any input
/// (see passes::unreadable_input).
fn resolve_dylib_ref<E: Target>(
    ctx: &Context<E>,
    name: &[u8],
    loader_dir: &Path,
    loader_rpaths: &[PathBuf],
) -> Option<&'static MappedFile> {
    use crate::util::{os_str, path_bytes};
    let dylib_files = ctx.args.dylib_files.iter().filter(|(install_name, _)| install_name == name);
    for (_, file) in dylib_files {
        match MappedFile::try_open(file) {
            Ok(mf) if mf.size() > 0 => return Some(mf),
            Ok(_) => fatal!("file is empty in '{}'", file.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !file.exists() => {}
            Err(e) => fatal!("{}", crate::passes::unreadable_input(file, &e)),
        }
    }
    // A name relative to the re-exporter or its rpaths resolves as
    // such first, and failing that like an absolute one.
    if let Some(rest) = name.strip_prefix(b"@loader_path/") {
        return find_reexport_file(ctx, path_bytes(&loader_dir.join(os_str(rest))))
            .or_else(|| find_reexport_by_leaf(ctx, name));
    }
    if let Some(rest) = name.strip_prefix(b"@rpath/") {
        return loader_rpaths
            .iter()
            .find_map(|rpath| find_reexport_file(ctx, path_bytes(&rpath.join(os_str(rest)))))
            .or_else(|| find_reexport_by_leaf(ctx, name));
    }
    find_reexport(ctx, name)
}

/// Locates a re-exported library by an absolute install name. As in
/// ld-prime, a library of the name's leaf in the library search path
/// (-L, then the SDK's /usr/lib) comes first - a stub for a library not
/// yet installed, found next to the re-exporter's own - and the install
/// path only after that.
pub fn find_reexport<E: Target>(ctx: &Context<E>, name: &[u8]) -> Option<&'static MappedFile> {
    find_reexport_by_leaf(ctx, name).or_else(|| find_reexport_file(ctx, name))
}

/// Looks a re-exported library up in the library search path by its
/// install name's leaf, less its extension: /opt/x/libfoo.1.dylib as
/// libfoo.1.tbd or libfoo.1.dylib. A framework is not looked up this way.
fn find_reexport_by_leaf<E: Target>(ctx: &Context<E>, name: &[u8]) -> Option<&'static MappedFile> {
    let leaf = Path::new(crate::util::os_str(name)).file_name()?;
    if memchr::memmem::find(name, b".framework/").is_some() {
        return None;
    }
    let stem = Path::new(leaf).with_extension("").into_os_string();
    for dir in &ctx.args.library_paths {
        for ext in [".tbd", ".dylib"] {
            let mut file = stem.clone();
            file.push(ext);
            if let Some(mf) = MappedFile::open(dir.join(file)) {
                return Some(mf);
            }
        }
    }
    None
}

/// Locates the stub or binary for a reexported library's install name
/// under the syslibroot.
pub fn find_reexport_file<E: Target>(
    ctx: &Context<E>,
    install_name: &[u8],
) -> Option<&'static MappedFile> {
    // Try under each syslibroot, then the raw path: reexports between
    // freshly built dylibs use absolute install names outside any SDK.
    let mut roots: Vec<PathBuf> = ctx.args.syslibroot.clone();
    roots.push(PathBuf::new());

    for root in &roots {
        let base = if root.as_os_str().is_empty() {
            PathBuf::from(crate::util::os_str(install_name))
        } else {
            let mut relative = install_name;
            while let Some(rest) = relative.strip_prefix(b"/") {
                relative = rest;
            }
            root.join(crate::util::os_str(relative))
        };
        let mut with_tbd = base.clone().into_os_string();
        with_tbd.push(".tbd");
        let candidates = [base.with_extension("tbd"), PathBuf::from(with_tbd), base];
        for path in candidates {
            if let Some(mf) = MappedFile::open(&path) {
                return Some(mf);
            }
        }
    }
    None
}

/// An export that a per-symbol $ld$previous directive moves to an older
/// library for the link's target: it binds to the library with that
/// install name, at the directive's version or else the defining
/// library's.
struct MovedExport {
    name: &'static str,
    install_name: &'static str,
    current_version: u32,
    compatibility_version: u32,
}

/// What a library's "$ld$..." names say for the link's target beyond its
/// exports: whether its install name is an older library's, and the
/// exports that move to one.
struct LdDirectives {
    renamed: bool,
    moved: Vec<MovedExport>,
}

/// A library's "$ld$..." names, read for the link's target. These are
/// not symbols but directives to the linker, invented so a library could
/// change shape per deployment target without a file format change:
/// $ld$add$os<ver>$<sym> exports <sym> only when the target equals
/// <ver>, $ld$hide$os<ver>$<sym> hides one, $ld$install_name$os<ver>$
/// <name> substitutes the recorded install name,
/// $ld$compatibility_version$os<ver>$<version> the compatibility
/// version, and $ld$previous$<name>$<compat>$<platform>$<lo>$<hi>$<sym>$
/// applies <name> (at version <compat>, if given) when the target
/// platform matches and lo <= minos < hi: to the whole library if <sym>
/// is empty, else to that export alone. Apple uses these when symbols
/// move between libraries: old targets keep binding them where they
/// used to live (AppKit's Swift overlay functions in libswiftAppKit
/// before macOS 14). A stub lists them among its exports, and a binary
/// dylib exports them as absolute symbols; ld-prime obeys both, and
/// passes over one it can't read without a word. Of several directives
/// of a kind that apply, it takes the one with the first $ld$previous
/// install name, the last $ld$install_name one and the first
/// $ld$compatibility_version directive by name; a library's
/// $ld$previous beats its $ld$install_name.
struct LdSymbols {
    added: Vec<&'static str>,
    hidden: hashbrown::HashSet<&'static str>,
    /// The install name an $ld$install_name directive gives.
    install_name: Option<&'static str>,
    /// The install name a whole-library $ld$previous directive gives,
    /// with the version it gives, if any.
    previous: Option<(&'static str, Option<u32>)>,
    /// The $ld$compatibility_version directive that applies: its name
    /// and version.
    compatibility_version: Option<(&'static str, u32)>,
    /// The exports that move: each with the install name it moves to
    /// and the version the directive gives, if any.
    moved: Vec<(&'static str, &'static str, Option<u32>)>,
}

impl LdSymbols {
    /// Reads the directives among `names`, which may hold other names.
    fn read<E: Target>(ctx: &Context<E>, names: &[&'static str]) -> Self {
        let minos = ctx.args.platform_minos;
        let mut ld = Self {
            added: Vec::new(),
            hidden: hashbrown::HashSet::new(),
            install_name: None,
            previous: None,
            compatibility_version: None,
            moved: Vec::new(),
        };
        for &name in names {
            let Some(rest) = name.strip_prefix("$ld$") else { continue };
            if let Some(rest) = rest.strip_prefix("previous$") {
                let Some(p) = PreviousDirective::parse(rest) else { continue };
                if p.platform != ctx.args.platform || minos < p.lo || p.hi <= minos {
                    continue;
                }
                let moved = (p.sym, p.install_name, p.version);
                match ld.moved.iter_mut().find(|(sym, ..)| *sym == p.sym) {
                    _ if p.sym.is_empty() => {
                        if ld.previous.is_none_or(|(first, _)| p.install_name < first) {
                            ld.previous = Some((p.install_name, p.version));
                        }
                    }
                    Some(old) if p.install_name < old.1 => *old = moved,
                    Some(_) => {}
                    None => ld.moved.push(moved),
                }
                continue;
            }
            // $ld$<action>$os<version>$<arg>, for the target's version.
            let Some((action, rest)) = rest.split_once('$') else { continue };
            let Some((version, arg)) = rest.strip_prefix("os").and_then(|r| r.split_once('$'))
            else {
                continue;
            };
            if arg.is_empty() || directive_version(version) != Some(minos) {
                continue;
            }
            match action {
                "add" => ld.added.push(arg),
                "hide" => _ = ld.hidden.insert(arg),
                "install_name" if ld.install_name.is_none_or(|last| last < arg) => {
                    ld.install_name = Some(arg);
                }
                "compatibility_version"
                    if ld.compatibility_version.is_none_or(|(first, _)| name < first) =>
                {
                    ld.compatibility_version = Some((name, directive_version(arg).unwrap_or(0)));
                }
                _ => {}
            }
        }
        ld
    }

    /// Whether the library keeps an export: it is no directive and not
    /// hidden.
    fn keeps(&self, name: &str) -> bool {
        !name.starts_with("$ld$") && !self.hidden.contains(name)
    }

    /// The install name the library takes from a directive, if any.
    fn renamed_install_name(&self) -> Option<&'static str> {
        self.previous.map(|(name, _)| name).or(self.install_name)
    }

    /// The version the library takes with an older one's install name,
    /// if the directive gives one.
    fn renamed_version(&self) -> Option<u32> {
        self.previous.and_then(|(_, version)| version)
    }

    /// The directives' effect beyond the exports, for a library at
    /// `current_version` and `compatibility_version` (after renaming).
    fn finish(self, current_version: u32, compatibility_version: u32) -> LdDirectives {
        let renamed = self.renamed_install_name().is_some();
        let moved = self
            .moved
            .into_iter()
            .map(|(name, install_name, version)| MovedExport {
                name,
                install_name,
                current_version: version.unwrap_or(current_version),
                compatibility_version: version.unwrap_or(compatibility_version),
            })
            .collect();
        LdDirectives { renamed, moved }
    }
}

/// Applies a .tbd's "$ld$..." export names (see LdSymbols) to it.
fn interpret_ld_symbols<E: Target>(ctx: &Context<E>, tbd: &mut tapi::TbdFile) -> LdDirectives {
    let ld = LdSymbols::read(ctx, &tbd.exports);
    tbd.exports.retain(|n| ld.keeps(n));
    tbd.weak_exports.retain(|n| ld.keeps(n));
    tbd.exports.extend(&ld.added);
    if let Some(name) = ld.renamed_install_name() {
        tbd.install_name = name.to_string();
    }
    if let Some((_, version)) = ld.compatibility_version {
        tbd.compatibility_version = version;
    }
    if let Some(version) = ld.renamed_version() {
        tbd.current_version = version;
        tbd.compatibility_version = version;
    }
    ld.finish(tbd.current_version, tbd.compatibility_version)
}

/// Applies a dylib binary's "$ld$..." names (see LdSymbols) to it.
/// ld-prime knows fewer kinds of directive in a binary than TAPI does
/// in a stub - not $ld$compatibility_version - and warns of each name
/// of another kind, by name, each time it reads the file.
fn interpret_binary_ld_symbols<E: Target>(
    ctx: &Context<E>,
    dylib: &mut DylibBinary,
) -> LdDirectives {
    for name in &dylib.ld_symbols {
        let kind = name["$ld$".len()..].split('$').next().unwrap();
        if !matches!(kind, "previous" | "add" | "hide" | "install_name" | "weak") {
            crate::warn!("unknown link constraint kind: {kind}");
        }
    }
    let ld = LdSymbols::read(ctx, &dylib.ld_symbols);
    dylib.exports.retain(|n| ld.keeps(n));
    dylib.weak_exports.retain(|n| ld.keeps(n));
    dylib.tlv_exports.retain(|n| ld.keeps(n));
    dylib.exports.extend(&ld.added);
    if let Some(name) = ld.renamed_install_name() {
        dylib.install_name = name.as_bytes().to_vec();
    }
    if let Some(version) = ld.renamed_version() {
        dylib.current_version = version;
        dylib.compatibility_version = version;
    }
    ld.finish(dylib.current_version, dylib.compatibility_version)
}

/// An $ld$previous directive, as ld-prime reads one:
/// <install name>$<compat>$<platform>$<lo>$<hi>[$[<sym>[$]]], the
/// symbol - which may itself contain '$', as Swift's do - less a final
/// '$'. A field it can't read makes it ignore the directive.
struct PreviousDirective {
    install_name: &'static str,
    version: Option<u32>,
    platform: u32,
    lo: u32,
    hi: u32,
    sym: &'static str,
}

impl PreviousDirective {
    fn parse(rest: &'static str) -> Option<Self> {
        let mut f = rest.splitn(6, '$');
        let (install_name, compat, platform) = (f.next()?, f.next()?, f.next()?);
        let (lo, hi) = (f.next()?, f.next()?);
        let sym = f.next().unwrap_or("");
        let version = |s: &str| if s.is_empty() { Some(0) } else { previous_version(s) };
        if install_name.is_empty()
            || platform.is_empty()
            || !platform.bytes().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        Some(Self {
            install_name,
            version: if compat.is_empty() { None } else { Some(previous_version(compat)?) },
            platform: strtoul32(platform)?,
            lo: version(lo)?,
            hi: version(hi)?,
            sym: sym.strip_suffix('$').unwrap_or(sym),
        })
    }
}

/// A number of a directive's version, as strtoul reads one into 32
/// bits: digits only, none for 0.
fn strtoul32(s: &str) -> Option<u32> {
    if !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(if s.is_empty() { 0 } else { s.parse::<u64>().map_or(u32::MAX, |n| n as u32) })
}

/// A version in an $ld$previous directive, as ld-prime reads one, packed
/// as a Mach-O version: up to five dot-separated numbers, of which the
/// fourth and fifth must be 0, the first below 65536 and the others
/// below 256. An empty number is 0, but not as the last of the first
/// four ("1..2" is 1.0.2, "1." no version).
fn previous_version(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 5 {
        return None;
    }
    let mut nums = [0; 5];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() && i + 1 == parts.len() && i < 4 {
            return None;
        }
        nums[i] = strtoul32(part)?;
    }
    (nums[0] <= 0xffff && nums[1] <= 0xff && nums[2] <= 0xff && nums[3] == 0 && nums[4] == 0)
        .then(|| (nums[0] << 16) | (nums[1] << 8) | nums[2])
}

/// The OS version of a directive, or the version of an
/// $ld$compatibility_version one, as ld-prime reads it: the first three
/// numbers of those separated by dots (empty ones skipped), the first
/// below 65536 and the others below 256.
fn directive_version(s: &str) -> Option<u32> {
    let mut version = 0;
    for (i, part) in s.split('.').filter(|p| !p.is_empty()).take(3).enumerate() {
        let n = if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse::<u32>().ok()?
        } else {
            return None;
        };
        if n > if i == 0 { 0xffff } else { 0xff } {
            return None;
        }
        version |= n << (16 - 8 * i);
    }
    Some(version)
}

/// A stub's library, read for the link's architecture and platform;
/// None if it has no target on the architecture.
fn read_tbd<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> Option<tapi::TbdFile> {
    tapi::parse_cached(mf, E::NAME, ctx.args.platform)
}

/// A stub's library to load, or None, with ld-prime's warning, if it
/// has no target on the architecture: the file is then ignored.
pub fn load_tbd<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> Option<tapi::TbdFile> {
    let tbd = read_tbd(ctx, mf);
    if tbd.is_none() {
        let path = crate::passes::resolved_file_name(mf);
        let why = format!("tapi error: missing required architecture {} in file {path}", E::NAME);
        ignore_foreign_file(ctx, mf, &why);
    }
    tbd
}

/// The first Objective-C or Swift class (see is_class_export) that a
/// dylib or stub exports itself for the link's target, after its
/// $ld$hide and $ld$add directives: the classes of the libraries it
/// re-exports don't count, those it re-exports one by one (an alias, a
/// -reexported_symbols_list entry) do. None for a file the link
/// ignores, or one that is no library. (For such a class ld-prime adds
/// its hook to an image that re-exports the library with -no_merge_*;
/// see passes::check_mergeable_libraries.)
pub fn exported_class<E: Target>(
    ctx: &Context<E>,
    mf: &'static MappedFile,
) -> Option<&'static str> {
    use crate::filetype::{FileType, get_file_type};
    let mf = match get_file_type(mf) {
        FileType::Fat => fat_slice::<E>(&ctx.args, mf)?,
        _ => mf,
    };
    let (ld, exports) = match get_file_type(mf) {
        FileType::Tapi => {
            let tbd = read_tbd(ctx, mf)?;
            let ld = LdSymbols::read(ctx, &tbd.exports);
            (ld, [tbd.exports, tbd.weak_exports, tbd.tlv_exports].concat())
        }
        FileType::Dylib
            if foreign_arch::<E>(mf).is_none()
                || (ctx.args.allow_sub_type_mismatches && is_subtype_mismatch::<E>(mf)) =>
        {
            let dylib = read_dylib_binary(mf);
            (LdSymbols::read(ctx, &dylib.ld_symbols), dylib.exports)
        }
        _ => return None,
    };
    let mut own = exports.into_iter().filter(|name| ld.keeps(name)).chain(ld.added.iter().copied());
    own.find(|name| is_class_export(name))
}

/// Whether an export is that of a class, as ld-prime's hook for the
/// classes of mergeable libraries goes by its name: an Objective-C
/// class or metaclass object (_OBJC_CLASS_$_Foo, _OBJC_METACLASS_$_Foo),
/// or a Swift class's type metadata (_$s...CN, of any class, Objective-C
/// or not). A Swift class's other symbols (its nominal type descriptor,
/// metaclass or accessor), and an Objective-C class's exception type or
/// instance variables, don't count.
fn is_class_export(name: &str) -> bool {
    name.starts_with("_OBJC_CLASS_$_")
        || name.starts_with("_OBJC_METACLASS_$_")
        || (name.starts_with("_$s") && name.ends_with("CN"))
}

pub fn parse_dylib<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) -> Option<usize> {
    let tbd = load_tbd(ctx, mf)?;
    Some(register_tbd_file(ctx, mf, tbd))
}

/// Registers the library of a stub file as a dylib of the link.
fn register_tbd_file<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    mut tbd: tapi::TbdFile,
) -> usize {
    check_dylib_platforms(ctx, mf, &tbd.platforms, &platforms_name(&tbd.platforms));
    let documents = std::mem::take(&mut tbd.documents);
    register_tbd(ctx, &mf.name, tbd, documents)
}

/// Registers a stub's library - a file's main document, or a
/// re-exported one inlined in it - as a dylib of the link. `documents`
/// are the inlined libraries its re-exports may resolve to.
fn register_tbd<E: Target>(
    ctx: &mut Context<E>,
    path: &Path,
    mut tbd: tapi::TbdFile,
    documents: Vec<tapi::TbdFile>,
) -> usize {
    let directives = interpret_ld_symbols(ctx, &mut tbd);
    let mut exports: hashbrown::HashSet<&'static str> = tbd.exports.into_iter().collect();
    let mut weak_exports: hashbrown::HashSet<&'static str> =
        tbd.weak_exports.iter().copied().collect();
    let has_weak_defs = !weak_exports.is_empty();
    exports.extend(tbd.weak_exports);
    let mut tlv_exports: hashbrown::HashSet<&'static str> = tbd.tlv_exports.into_iter().collect();
    exports.extend(tlv_exports.iter().copied());

    let reexports: Vec<(Vec<u8>, PathBuf, Vec<PathBuf>)> = tbd
        .reexports
        .into_iter()
        .map(|name| (name.as_bytes().to_vec(), dir_of(path), Vec::new()))
        .collect();
    let parent = ReexportParent {
        install_name: tbd.install_name.as_bytes(),
        path,
        platforms: tbd.platforms.len(),
    };
    let (merged_reexports, merged_files, mut moved) = load_reexports(
        ctx,
        reexports,
        parent,
        documents,
        &mut exports,
        &mut tlv_exports,
        &mut weak_exports,
    );
    moved.extend(directives.moved);
    let moved_exports = add_moved_dylibs(ctx, path, moved, &exports);
    let name_source = if directives.renamed { NameSource::Directive } else { NameSource::Own };

    let priority = ctx.next_priority();
    add_dylib(
        ctx,
        DylibFile {
            path: path.to_path_buf(),
            install_name: tbd.install_name.into_bytes(),
            current_version: tbd.current_version,
            compatibility_version: tbd.compatibility_version,
            minos: tbd.minos,
            in_sdk: false,
            from_binary: false,
            dylib_idx: next_dylib_ordinal(ctx),
            is_bundle_loader: false,
            priority,
            is_weak: false,
            is_weak_asserted: false,
            is_reexported: false,
            binds_to_image: false,
            is_needed: false,
            is_upward: false,
            is_lazy: false,
            named_lazily: false,
            delay_init: None,
            named_at: None,
            is_autolinked: false,
            is_implicit: false,
            load_order: u32::MAX,
            exports,
            weak_exports,
            has_weak_defs,
            tlv_exports,
            merged_reexports,
            merged_files,
            moved_exports,
            named_files: Vec::new(),
            name_source,
        },
    )
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
    exports: &hashbrown::HashSet<&'static str>,
) -> hashbrown::HashMap<&'static str, usize> {
    let mut moved_exports = hashbrown::HashMap::new();
    let mut targets: Vec<(&str, usize)> = Vec::new();
    for export in moved.into_iter().filter(|e| exports.contains(e.name)) {
        let idx = match targets.iter().find(|(name, _)| *name == export.install_name) {
            Some(&(_, idx)) => idx,
            None => {
                let priority = ctx.next_priority();
                let dylib = DylibFile {
                    path: path.to_path_buf(),
                    install_name: export.install_name.as_bytes().to_vec(),
                    current_version: export.current_version,
                    compatibility_version: export.compatibility_version,
                    minos: 0,
                    in_sdk: false,
                    from_binary: false,
                    dylib_idx: next_dylib_ordinal(ctx),
                    is_bundle_loader: false,
                    priority,
                    is_weak: false,
                    is_weak_asserted: false,
                    is_reexported: false,
                    binds_to_image: false,
                    is_needed: false,
                    is_upward: false,
                    is_lazy: false,
                    named_lazily: false,
                    delay_init: None,
                    named_at: None,
                    is_autolinked: false,
                    is_implicit: true,
                    load_order: u32::MAX,
                    exports: hashbrown::HashSet::new(),
                    weak_exports: hashbrown::HashSet::new(),
                    has_weak_defs: false,
                    tlv_exports: hashbrown::HashSet::new(),
                    merged_reexports: Vec::new(),
                    merged_files: Vec::new(),
                    moved_exports: hashbrown::HashMap::new(),
                    named_files: Vec::new(),
                    name_source: NameSource::Moved,
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
/// passes::add_merged_dependencies), as one named on the command line
/// after the others, which has the exports the merged atoms import.
pub fn add_merged_dependency<E: Target>(ctx: &mut Context<E>, dep: crate::mergeable::Dependency) {
    let before = ctx.dylibs.len();
    let priority = ctx.next_priority();
    let weak_exports: hashbrown::HashSet<&'static str> = dep.weak_exports.into_iter().collect();
    let dylib = DylibFile {
        path: dep.path,
        install_name: dep.info.install_name,
        current_version: dep.info.current_version,
        compatibility_version: dep.info.compatibility_version,
        minos: 0,
        in_sdk: false,
        from_binary: false,
        dylib_idx: next_dylib_ordinal(ctx),
        is_bundle_loader: false,
        priority,
        is_weak: false,
        is_weak_asserted: false,
        is_reexported: false,
        binds_to_image: false,
        is_needed: false,
        is_upward: false,
        is_lazy: false,
        named_lazily: false,
        delay_init: None,
        named_at: None,
        is_autolinked: false,
        is_implicit: false,
        load_order: u32::MAX,
        exports: dep.exports.into_iter().collect(),
        has_weak_defs: !weak_exports.is_empty(),
        weak_exports,
        tlv_exports: hashbrown::HashSet::new(),
        merged_reexports: Vec::new(),
        merged_files: Vec::new(),
        moved_exports: hashbrown::HashMap::new(),
        named_files: Vec::new(),
        name_source: NameSource::Own,
    };
    let idx = add_dylib(ctx, dylib);
    if idx >= before {
        ctx.dylibs[idx].load_order = ctx.dylib_load_seq;
        ctx.dylib_load_seq += 1;
    }
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
    if let Some(idx) = ctx
        .dylibs
        .iter()
        .position(|d| d.install_name == dylib.install_name && is_moved(d) == is_moved(&dylib))
    {
        let existing = &mut ctx.dylibs[idx];
        if dylib.name_source < existing.name_source {
            existing.current_version = dylib.current_version;
            existing.compatibility_version = dylib.compatibility_version;
            existing.name_source = dylib.name_source;
        }
        existing.exports.extend(dylib.exports);
        existing.merged_reexports.extend(dylib.merged_reexports);
        existing.merged_files.extend(dylib.merged_files);
        existing.moved_exports.extend(dylib.moved_exports);
        return idx;
    }
    ctx.dylibs.push(dylib);
    ctx.dylibs.len() - 1
}
