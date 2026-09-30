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
use crate::target::Target;

/// A relocatable object file.
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
    /// The SDK the object was built against (a -r output without a
    /// -platform_version takes the first object's).
    pub sdk: u32,
}

impl PlatformVersion {
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
}

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
            ar_date: None,
        }));
        Self {
            mf,
            is_alive: true,
            priority: 0,
            linker_options: Vec::new(),
            platform_versions: Vec::new(),
            hidden: false,
            subsections_via_symbols: true,
            sect_hdrs: std::borrow::Cow::Owned(Vec::new()),
            relocs: Vec::new(),
            subsecs: Vec::new(),
            objc_image_info: None,
            has_debug_info: false,
            lto_module: None,
            nlists: std::borrow::Cow::Owned(Vec::new()),
            first_global: None,
            symbols: Vec::new(),
            dice: Vec::new(),
            loh: Vec::new(),
        }
    }
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
    (addr <= isec.input_addr as u64 + isec.size as u64).then(|| (id, addr - isec.input_addr as u64))
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
    /// True if re-exported (LC_REEXPORT_DYLIB): this image's clients
    /// resolve the library's exports through this image.
    pub is_reexported: bool,
    /// -needed-l: keep the load command even under -dead_strip_dylibs.
    pub is_needed: bool,
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
    /// MH_DEAD_STRIPPABLE_DYLIB: drop the load command whenever no
    /// symbol binds to this dylib, even without -dead_strip_dylibs.
    pub is_dead_strippable: bool,
    pub exports: hashbrown::HashSet<&'static str>,
    /// Exports that are weak definitions: binding to one sets
    /// MH_BINDS_TO_WEAK on the client image.
    pub weak_exports: hashbrown::HashSet<&'static str>,
    /// The subset of exports that are thread-local variables.
    pub tlv_exports: hashbrown::HashSet<&'static str>,
    /// The install names of the private libraries this dylib re-exports,
    /// whose exports are merged into its own.
    pub merged_reexports: Vec<Vec<u8>>,
}

/// Returns true for sections that don't become part of the output image.
fn is_discarded_section(hdr: &MachSection) -> bool {
    // Debug sections, including __LD,__compact_unwind, are consumed by
    // other tools or, later, by the linker itself; they are never
    // copied into an output. Not into a -r output either: ld64 does
    // not merge DWARF (the section-relative offsets in it - abbrev,
    // line table, ranges - carry no relocations and would all have to
    // be rebased), it writes debug-note stabs naming the input objects
    // as the places debuggers read DWARF from, and a later link
    // carries those notes through.
    hdr.flags & S_ATTR_DEBUG != 0 || hdr.segname() == "__DWARF" || hdr.segname() == "__LD"
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
    pub isecs: Vec<InputSection>,
    pub relocs: Vec<crate::input_sections::Reloc>,
    pub subsecs: Vec<crate::input_sections::InputSectionId>,
    pub nlists: std::borrow::Cow<'static, [NList]>,
    /// Index of the first external nlist, if the table is partitioned
    /// locals-then-externals (see first_global_of).
    pub first_global: Option<u32>,
    pub sym_names: Vec<&'static str>,
    /// xxh3 of each extern non-stab name (0 otherwise), computed here
    /// so the serial intern path never hashes.
    pub sym_hashes: Vec<u64>,
    pub unwind: Vec<UnwindRecord>,
    pub cies: Vec<Cie>,
    pub fdes: Vec<Fde>,
    pub objc_image_info: Option<u32>,
    pub has_debug_info: bool,
    /// LC_DATA_IN_CODE entries: (file offset in the object, length,
    /// kind).
    pub dice: Vec<(u32, u16, u16)>,
    /// LC_LINKER_OPTIMIZATION_HINT entries: (kind, instruction
    /// addresses in the object's address space).
    pub loh: Vec<(u8, Vec<u64>)>,
}

/// Parses one object file without touching any linker state.
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
/// and nowhere else.
fn bare_sections(
    sect_hdrs: &[MachSection],
    nlists: &[NList],
    strtab: &'static [u8],
    split_ok: bool,
) -> Vec<bool> {
    let mut bare: Vec<bool> = sect_hdrs.iter().map(|s| s.size == 0).collect();
    for nlist in nlists {
        if !nlist.is_stab()
            && nlist.n_type() == N_SECT
            && let Some(b) = bare.get_mut((nlist.n_sect as usize).wrapping_sub(1))
            && *b
            && !(split_ok && symbol_name(strtab, nlist).starts_with("ltmp"))
        {
            *b = false;
        }
    }
    bare
}

/// Parses an object. `keep_all_fdes` keeps the FDEs of functions a
/// compact unwind record already covers, for an output that has no
/// __unwind_info to hold that record (-r, -static).
pub fn stage_object<E: Target>(
    mf: &'static MappedFile,
    alive: bool,
    hidden: bool,
    priority: u32,
    relocatable: bool,
    keep_all_fdes: bool,
) -> StagedObject {
    let data = mf.data();
    let hdr = MachHeader::read_from(data);

    if hdr.cputype != E::CPUTYPE {
        fatal!("{}: incompatible CPU type: expected {}", mf.name.display(), E::NAME);
    }

    let mut isecs: Vec<InputSection> = Vec::new();
    let mut sect_hdrs = Vec::new();
    let mut symtab_cmd = None;
    let mut dysymtab_cmd: Option<DysymtabCommand> = None;
    let mut linker_options = Vec::new();
    let mut platform_versions = Vec::new();
    let mut dice = Vec::new();
    let mut loh = Vec::new();

    // Read load commands
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        match lc.cmd {
            LC_SEGMENT_64 => {
                let seg = SegmentCommand::read_from(&data[off..]);
                for i in 0..seg.nsects as usize {
                    let sect_off = off + size_of::<SegmentCommand>() + i * size_of::<MachSection>();
                    sect_hdrs.push(MachSection::read_from(&data[sect_off..]));
                }
            }
            LC_SYMTAB => symtab_cmd = Some(SymtabCommand::read_from(&data[off..])),
            LC_DYSYMTAB => dysymtab_cmd = Some(DysymtabCommand::read_from(&data[off..])),
            LC_BUILD_VERSION
            | LC_VERSION_MIN_MACOSX
            | LC_VERSION_MIN_IPHONEOS
            | LC_VERSION_MIN_TVOS
            | LC_VERSION_MIN_WATCHOS => {
                platform_versions.push(PlatformVersion::read(lc.cmd, &data[off..], E::CPUTYPE));
            }
            LC_LINKER_OPTION => {
                // Auto-link requests: the object names libraries it
                // needs, as NUL-terminated strings after a count.
                let count = u32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap());
                let mut strs = Vec::with_capacity(count as usize);
                let mut p = off + 12;
                for _ in 0..count {
                    let rest = &data[p..off + lc.cmdsize as usize];
                    let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                    strs.push(rest[..len].to_vec());
                    p += len + 1;
                }
                linker_options.push(strs);
            }
            LC_DATA_IN_CODE => {
                let cmd = LinkEditDataCommand::read_from(&data[off..]);
                for i in 0..cmd.datasize as usize / 8 {
                    let p = cmd.dataoff as usize + i * 8;
                    dice.push((
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
                let payload = &data[cmd.dataoff as usize..(cmd.dataoff + cmd.datasize) as usize];
                let mut pos = 0;
                while pos < payload.len() {
                    let kind = read_uleb_at(payload, &mut pos);
                    if kind == 0 {
                        break;
                    }
                    let count = read_uleb_at(payload, &mut pos);
                    let addrs = (0..count).map(|_| read_uleb_at(payload, &mut pos)).collect();
                    loh.push((kind as u8, addrs));
                }
            }
            _ => {}
        }
        off += lc.cmdsize as usize;
    }

    // The section headers are complete; leak them so subsections can
    // reference (not copy) their parent header. The leak is bounded by
    // the object's section count and lives for the whole link.
    let sect_hdrs: &'static [MachSection] = Vec::leak(sect_hdrs);

    // Read the symbol table. The nlist_64 array is used straight from
    // the mmap when it is 8-aligned (ld64 aligns it; NList is #[repr(C)]
    // nlist_64, all integer fields, so any bytes are a valid value) -
    // no copy of 16 bytes per symbol. mold borrows its ElfSym
    // array the same way (Cow, Owned only for synthesized symbols).
    let mut nlists: std::borrow::Cow<'static, [NList]> = std::borrow::Cow::Borrowed(&[]);
    let mut strtab: &'static [u8] = &[];
    if let Some(cmd) = symtab_cmd {
        nlists = match nlists_slice(data, cmd.symoff as usize, cmd.nsyms as usize) {
            Some(s) => std::borrow::Cow::Borrowed(s),
            None => {
                std::borrow::Cow::Owned(read_array(data, cmd.symoff as usize, cmd.nsyms as usize))
            }
        };
        strtab = validate_strtab(&data[cmd.stroff as usize..(cmd.stroff + cmd.strsize) as usize]);
    }
    let first_global = first_global_of(&nlists, dysymtab_cmd.as_ref());

    // Split each section into subsections at its symbols, the Mach-O
    // linking granularity, so that unreferenced pieces can later be
    // dead-stripped. Alternate entry points (N_ALT_ENTRY) don't start a
    // new subsection, and literal sections are element-oriented rather
    // than symbol-oriented, so they stay whole.
    let split_ok = hdr.flags & MH_SUBSECTIONS_VIA_SYMBOLS != 0;
    let bare = bare_sections(sect_hdrs, &nlists, strtab, split_ok);
    let mut split_points: Vec<Vec<u64>> = vec![Vec::new(); sect_hdrs.len()];
    if split_ok {
        for nlist in nlists.iter() {
            if !nlist.is_stab()
                && nlist.n_type() == N_SECT
                && nlist.n_desc & N_ALT_ENTRY == 0
                && nlist.n_sect >= 1
                && let Some(points) = split_points.get_mut(nlist.n_sect as usize - 1)
            {
                points.push(nlist.n_value);
            }
        }
    } else {
        // Without subsections a section is one atom, and ld64 takes the
        // atom's attributes from one symbol at the section's start (the
        // arm64 assembler's ltmpN labels don't count): a non-weak one
        // if there is any, local or global, else the last weak one in
        // symbol table order. An atom cannot be swapped for another
        // copy, so a weak symbol that names it is no longer weak; the
        // other symbols are labels into the atom and keep their flags.
        // A .weak_def_can_be_hidden name becomes a hidden non-weak
        // definition, except in a -r output, which keeps it as is.
        let mut named_by_strong = vec![false; sect_hdrs.len()];
        let mut last_weak: Vec<Option<usize>> = vec![None; sect_hdrs.len()];
        for (i, nlist) in nlists.iter().enumerate() {
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
        if !names.is_empty() {
            let owned = nlists.to_mut();
            for i in names {
                let nlist = &mut owned[i];
                if nlist.n_desc & N_WEAK_REF == 0 {
                    nlist.n_desc &= !N_WEAK_DEF;
                } else if !relocatable {
                    nlist.n_desc &= !(N_WEAK_DEF | N_WEAK_REF);
                    nlist.n_type |= N_PEXT;
                }
            }
        }
    }

    // Sections whose contents are fixed-shape records the linker
    // coalesces by content, as ld64 does: literal pools, literal
    // pointers, and __cfstring, whose 32-byte CFString constants
    // x86-64 compilers emit without labels.
    let is_literal = |sect: &MachSection| {
        matches!(
            sect.section_type(),
            S_CSTRING_LITERALS
                | S_4BYTE_LITERALS
                | S_8BYTE_LITERALS
                | S_16BYTE_LITERALS
                | S_LITERAL_POINTERS
        ) || (sect.segname() == "__DATA" && sect.sectname() == "__cfstring")
    };

    // Subsections of each section, by section ordinal.
    let mut by_ordinal: Vec<Vec<usize>> = vec![Vec::new(); sect_hdrs.len()];
    let mut subsecs: Vec<crate::input_sections::InputSectionId> = Vec::new();

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

        // Literal sections are element-oriented: split them per element
        // (per string, or per fixed-size literal) so identical elements
        // can be merged across objects.
        let mut points = std::mem::take(&mut split_points[i]);
        if is_literal(sect) {
            points.clear();
            let contents = &data[sect.offset as usize..(sect.offset as u64 + sect.size) as usize];
            match sect.section_type() {
                S_CSTRING_LITERALS => {
                    let mut start = 0;
                    while start < contents.len() {
                        points.push(sect.addr + start as u64);
                        let rest = &contents[start..];
                        let p = unsafe { libc::memchr(rest.as_ptr().cast(), 0, rest.len()) };
                        if p.is_null() {
                            fatal!("{}: malformed __cstring section", mf.name.display());
                        }
                        start += (p as usize - rest.as_ptr() as usize) + 1;
                    }
                }
                S_4BYTE_LITERALS => points.extend((0..sect.size).step_by(4).map(|o| sect.addr + o)),
                S_8BYTE_LITERALS => points.extend((0..sect.size).step_by(8).map(|o| sect.addr + o)),
                S_16BYTE_LITERALS => {
                    points.extend((0..sect.size).step_by(16).map(|o| sect.addr + o))
                }
                // A literal-pointer section (__objc_selrefs) is one
                // atom per pointer, as in ld64, so references to the
                // same selector can be coalesced across objects.
                S_LITERAL_POINTERS => {
                    points.extend((0..sect.size).step_by(8).map(|o| sect.addr + o))
                }
                // __cfstring: one 32-byte constant per record.
                _ => points.extend((0..sect.size).step_by(32).map(|o| sect.addr + o)),
            }
        }
        points.push(sect.addr);
        points.retain(|&a| sect.addr <= a && a <= sect.addr + sect.size);
        points.sort_unstable();
        points.dedup();

        for (j, &start) in points.iter().enumerate() {
            let end = points.get(j + 1).copied().unwrap_or(sect.addr + sect.size);
            let contents = if sect.section_type() == S_ZEROFILL
                || sect.section_type() == S_THREAD_LOCAL_ZEROFILL
            {
                &[]
            } else {
                let lo = sect.offset as u64 + (start - sect.addr);
                &data[lo as usize..(lo + (end - start)) as usize]
            };
            // A fixed-size literal is aligned to its size, whatever the
            // section header says: ld64's Literal{4,8,16}Section gives
            // every atom Alignment(2/3/4) with no modulus. Compilers
            // emit __literal16 with p2align 3 for a 16-byte constant
            // whose type is only 8-aligned, and rely on the linker to
            // place it where a 16-byte load can reach it.
            let literal_p2align = match sect.section_type() {
                S_4BYTE_LITERALS => Some(2u8),
                S_8BYTE_LITERALS => Some(3),
                S_16BYTE_LITERALS => Some(4),
                _ => None,
            };
            isecs.push(InputSection {
                file: u32::MAX,
                shndx: i as u32,
                p2align: literal_p2align.unwrap_or(sect.p2align as u8),
                input_addr: start as u32,
                size: (end - start) as u32,
                contents: if contents.is_empty() { 0 } else { contents.as_ptr() as usize },
                rel_offset: 0,
                nrels: 0,
                output_section: u32::MAX,
                offset: 0,
                flags: if bare[i] {
                    InputSection::flags_dead()
                } else if literal_p2align.is_some() {
                    InputSection::flags_alive_no_modulus()
                } else {
                    InputSection::flags_alive()
                },
                replacement: crate::input_sections::NO_REPLACEMENT,
                unwind_offset: 0,
                nunwind: 0,
            });
            by_ordinal[i].push(isecs.len() - 1);
            subsecs.push((isecs.len() - 1) as u32);
        }
    }

    subsecs.sort_by_key(|&id| isecs[id as usize].input_addr);

    // Read each section's relocations and distribute them to its
    // subsections, rebasing location offsets and section-relative
    // targets to subsections. Sorted by offset, the relocations of
    // one subsection are contiguous, so a single merge walk over the
    // subsections hands each its run. The relocs go into one per-object
    // arena and each subsection keeps a range into it (rel_offset/
    // nrels) - sold's layout - so a debug link's millions of relocs
    // are one allocation, not a Vec per subsection.
    let mut obj_relocs: Vec<crate::input_sections::Reloc> = Vec::new();
    for (i, sect) in sect_hdrs.iter().enumerate() {
        if by_ordinal[i].is_empty() || sect.nreloc == 0 {
            continue;
        }
        let raw: Vec<MachRel> = read_array(data, sect.reloff as usize, sect.nreloc as usize);
        let mut rels = E::read_relocs(&mf.name, sect_hdrs, sect, data, &raw);
        // The sort must be stable: a SUBTRACTOR and the UNSIGNED it
        // pairs with share one offset and their order is the pairing
        // (Swift's relative pointers are all such pairs). An unstable
        // sort swapped some, leaving lone 4-byte UNSIGNED relocations
        // that were then written as 8 bytes.
        rels.sort_by_key(|rel| rel.offset);

        for rel in &mut rels {
            // A section ld-prime ignores can't be a relocation's target,
            // named by section or through a label on it (an ltmpN), and
            // neither can a symbol in an __LD section it drops.
            match rel.target() {
                crate::input_sections::RelocTarget::Section(sect_pos)
                    if bare[sect_pos as usize] =>
                {
                    let addr = (sect_hdrs[sect_pos as usize].addr as i64 + rel.addend) as u64;
                    crate::error!(
                        "address=0x{addr:x} points to section({}) with no content in '{}'",
                        sect_pos + 1,
                        mf.name.display()
                    );
                    continue;
                }
                crate::input_sections::RelocTarget::Sym(idx)
                    if nlists.get(idx as usize).is_some_and(|n| {
                        let sect = (n.n_sect as usize).wrapping_sub(1);
                        n.n_type() == N_SECT
                            && (bare.get(sect) == Some(&true)
                                || sect_hdrs.get(sect).is_some_and(is_unknown_ld_section))
                    }) =>
                {
                    crate::error!("invalid r_symbolnum={idx} in '{}'", mf.name.display());
                    continue;
                }
                _ => {}
            }
            if let crate::input_sections::RelocTarget::Section(sect_pos) = rel.target() {
                let sect = &sect_hdrs[sect_pos as usize];
                let taddr = (sect.addr as i64 + rel.addend) as u64;
                let found = find_subsec(&isecs, &subsecs, taddr).or_else(|| {
                    // One past the section's end (a DWARF range end):
                    // one past its last subsection.
                    if taddr != sect.addr + sect.size {
                        return None;
                    }
                    let &last = by_ordinal[sect_pos as usize].last()?;
                    Some((last, isecs[last].size as u64))
                });
                let Some((tsub, toff)) = found else {
                    fatal!("{}: relocation against a discarded section", mf.name.display());
                };
                rel.set_target(crate::input_sections::RelocTarget::Section(tsub as u32));
                rel.addend = toff as i64;
            }
        }

        let mut pos = 0;
        for &sub in &by_ordinal[i] {
            let sub_off = (isecs[sub].input_addr as u64 - sect.addr) as u32;
            let end = sub_off + isecs[sub].size;
            let start = obj_relocs.len();
            while pos < rels.len() && rels[pos].offset < end {
                let mut rel = rels[pos];
                rel.offset -= sub_off;
                obj_relocs.push(rel);
                pos += 1;
            }
            isecs[sub].rel_offset = start as u32;
            isecs[sub].nrels = (obj_relocs.len() - start) as u32;
        }
        if pos < rels.len() {
            fatal!("{}: relocation outside its section", mf.name.display());
        }
    }

    // Record symbol names; interning happens at integration.
    let sym_names: Vec<&'static str> =
        nlists.iter().map(|nlist| symbol_name(strtab, nlist)).collect();

    let mut unwind = Vec::new();
    let mut cies = Vec::new();
    let mut fdes = Vec::new();
    if let Some(hdr) =
        sect_hdrs.iter().find(|s| s.segname() == "__LD" && s.sectname() == "__compact_unwind")
    {
        parse_compact_unwind(hdr, &isecs, &subsecs, &nlists, data, &mf.name, &mut unwind);
    }

    if let Some(hdr) =
        sect_hdrs.iter().find(|s| s.segname() == "__TEXT" && s.sectname() == "__eh_frame")
    {
        parse_eh_frame::<E>(
            hdr,
            &isecs,
            &subsecs,
            &nlists,
            data,
            &mf.name,
            &mut unwind,
            &mut cies,
            &mut fdes,
            keep_all_fdes,
        );
    }
    // A DWARF-mode record whose FDE never turned up describes nothing.
    unwind.retain(|rec| {
        rec.encoding & UNWIND_MODE_MASK != E::UNWIND_MODE_DWARF || rec.fde().is_some()
    });

    let objc_image_info = sect_hdrs.iter().find(|s| s.sectname() == "__objc_imageinfo").map(|s| {
        let off = s.offset as usize + 4;
        u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
    });
    let has_debug_info =
        sect_hdrs.iter().any(|s| s.segname() == "__DWARF" && s.sectname() == "__debug_info");

    let sym_hashes: Vec<u64> =
        nlists
            .iter()
            .zip(&sym_names)
            .map(|(nlist, name)| {
                if !nlist.is_stab() && nlist.is_extern() {
                    crate::symbol::hash_key(name)
                } else {
                    0
                }
            })
            .collect();

    StagedObject {
        mf,
        alive,
        hidden,
        priority,
        sect_hdrs,
        linker_options,
        platform_versions,
        subsections_via_symbols: split_ok,
        isecs,
        relocs: obj_relocs,
        subsecs,
        nlists,
        first_global,
        sym_names,
        sym_hashes,
        unwind,
        cies,
        dice,
        loh,
        fdes,
        objc_image_info,
        has_debug_info,
    }
}

/// Appends a staged object to the global arenas, rebasing its local
/// indices and interning its symbol names.
pub fn integrate_object<E: Target>(ctx: &mut Context<E>, staged: StagedObject) -> usize {
    integrate_object_with(ctx, staged, None)
}

/// Like integrate_object, with the global symbols' ids already interned
/// by a bulk pass (in nlist order, one entry per extern non-stab nlist).
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

    // The rebasing, in parallel; each object also reports its local
    // symbol names in order for the serial arena extension below.
    let syms_of: Vec<Vec<crate::symbol::SymbolId>> = staged
        .par_iter_mut()
        .enumerate()
        .map(|(i, st)| {
            let base = &bases[i];
            let obj_idx = obj_base + i;

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

            for isec in &mut st.isecs {
                isec.file = obj_idx as u32;
            }
            // Section relocation targets are object-local subsection
            // indices; rebase them to global once over the object's
            // reloc arena (rel_offset/nrels stay object-local).
            for rel in &mut st.relocs {
                if let crate::input_sections::RelocTarget::Section(local) = rel.target() {
                    rel.set_target(crate::input_sections::RelocTarget::Section(
                        base.isec as u32 + local,
                    ));
                }
            }
            for sub in &mut st.subsecs {
                *sub += base.isec as u32;
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
            for rec in &mut st.unwind {
                rec.isec += base.isec as u32;
                if rec.lsda_isec != UNWIND_NONE {
                    rec.lsda_isec += base.isec as u32;
                }
                if rec.fde_idx != UNWIND_NONE {
                    rec.fde_idx += base.fde as u32;
                }
                if rec.personality_sym != UNWIND_NONE {
                    rec.personality_sym = syms[rec.personality_sym as usize];
                }
            }
            for cie in &mut st.cies {
                cie.obj = obj_idx as u32;
                if let Some(p) = &mut cie.personality {
                    *p = syms[*p as usize];
                }
            }
            for fde in &mut st.fdes {
                fde.obj = obj_idx as u32;
                fde.isec += base.isec as u32;
                fde.cie += base.cie as u32;
                if let Some((lsda, _)) = &mut fde.lsda {
                    *lsda += base.isec as u32;
                }
            }
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
        ctx.objs.push(ObjectFile {
            mf: st.mf,
            is_alive: st.alive,
            priority: st.priority,
            linker_options: st.linker_options,
            platform_versions: st.platform_versions,
            hidden: st.hidden,
            subsections_via_symbols: st.subsections_via_symbols,
            sect_hdrs: std::borrow::Cow::Borrowed(st.sect_hdrs),
            relocs: st.relocs,
            subsecs: st.subsecs,
            objc_image_info: st.objc_image_info,
            has_debug_info: st.has_debug_info,
            nlists: st.nlists,
            first_global: st.first_global,
            symbols: syms,
            lto_module: None,
            dice: st.dice,
            loh: st.loh,
        });
    }
}

pub fn integrate_object_with<E: Target>(
    ctx: &mut Context<E>,
    staged: StagedObject,
    pre_interned: Option<Vec<crate::symbol::SymbolId>>,
) -> usize {
    let obj_idx = ctx.objs.len();
    let isec_base = ctx.isecs.len();
    let fde_base = ctx.fdes.len();
    let cie_base = ctx.cies.len();

    for mut isec in staged.isecs {
        isec.file = obj_idx as u32;
        ctx.isecs.push(isec);
    }
    let mut obj_relocs = staged.relocs;
    for rel in &mut obj_relocs {
        if let crate::input_sections::RelocTarget::Section(local) = rel.target() {
            rel.set_target(crate::input_sections::RelocTarget::Section(isec_base as u32 + local));
        }
    }

    let mut syms = Vec::with_capacity(staged.nlists.len());
    let mut pre = pre_interned.map(Vec::into_iter);
    for (nlist, name) in staged.nlists.iter().zip(&staged.sym_names) {
        let id = if nlist.is_stab() || !nlist.is_extern() {
            ctx.symbols.add_local(name)
        } else {
            match &mut pre {
                Some(iter) => iter.next().unwrap(),
                None => ctx.symbols.intern(name),
            }
        };
        syms.push(id);
    }

    for mut rec in staged.unwind {
        rec.isec += isec_base as u32;
        if rec.lsda_isec != UNWIND_NONE {
            rec.lsda_isec += isec_base as u32;
        }
        if rec.fde_idx != UNWIND_NONE {
            rec.fde_idx += fde_base as u32;
        }
        // The personality was recorded as a local symbol index.
        if rec.personality_sym != UNWIND_NONE {
            rec.personality_sym = syms[rec.personality_sym as usize];
        }
        // Extend or open the subsection's record range (grouped input).
        let isec = &mut ctx.isecs[rec.isec as usize];
        if isec.nunwind == 0 {
            isec.unwind_offset = ctx.unwind_records.len() as u32;
        }
        isec.nunwind += 1;
        ctx.unwind_records.push(rec);
    }
    for mut cie in staged.cies {
        cie.obj = obj_idx as u32;
        if let Some(p) = &mut cie.personality {
            *p = syms[*p as usize];
        }
        ctx.cies.push(cie);
    }
    for mut fde in staged.fdes {
        fde.obj = obj_idx as u32;
        fde.isec += isec_base as u32;
        fde.cie += cie_base as u32;
        if let Some((lsda, _)) = &mut fde.lsda {
            *lsda += isec_base as u32;
        }
        ctx.fdes.push(fde);
    }

    ctx.objs.push(ObjectFile {
        mf: staged.mf,
        is_alive: staged.alive,
        priority: staged.priority,
        linker_options: staged.linker_options,
        platform_versions: staged.platform_versions,
        hidden: staged.hidden,
        subsections_via_symbols: staged.subsections_via_symbols,
        sect_hdrs: std::borrow::Cow::Borrowed(staged.sect_hdrs),
        relocs: obj_relocs,
        subsecs: staged.subsecs.into_iter().map(|i| i + isec_base as u32).collect(),
        objc_image_info: staged.objc_image_info,
        has_debug_info: staged.has_debug_info,
        nlists: staged.nlists,
        first_global: staged.first_global,
        symbols: syms,
        lto_module: None,
        dice: staged.dice,
        loh: staged.loh,
    });
    obj_idx
}

/// Parses one object and adds it to the link immediately.
pub fn parse_object<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    alive: bool,
) -> usize {
    let priority = ctx.next_priority();
    let keep_all_fdes = ctx.args.relocatable || ctx.args.static_link;
    let staged = stage_object::<E>(mf, alive, false, priority, ctx.args.relocatable, keep_all_fdes);
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

    let obj_idx = ctx.objs.len();
    let mut syms = Vec::new();
    let mut nlists = Vec::new();

    // Symbols are expressed as synthesized nlists so that the regular
    // resolution pass handles bitcode like any object.
    for ls in lsyms {
        if !ls.is_extern && ls.is_defined {
            continue;
        }
        let name: &'static str = String::leak(ls.name);
        let id = ctx.symbols.intern(name);
        let mut nlist = NList::default();
        if ls.is_defined {
            nlist.n_type = N_ABS | N_EXT | if ls.is_private_extern { N_PEXT } else { 0 };
            if ls.is_weak_def {
                nlist.n_desc |= N_WEAK_DEF;
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
        platform_versions: Vec::new(),
        hidden: false,
        subsections_via_symbols: true,
        sect_hdrs: std::borrow::Cow::Borrowed(&[]),
        relocs: Vec::new(),
        subsecs: Vec::new(),
        objc_image_info: None,
        has_debug_info: false,
        nlists: std::borrow::Cow::Owned(nlists),
        first_global: None,
        symbols: syms,
        lto_module: Some(module),
        dice: Vec::new(),
        loh: Vec::new(),
    });
    ctx.lto_modules.push((obj_idx, module));
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

/// Parses a __LD,__compact_unwind section into unwind records. The
/// section is an array of 32-byte entries whose pointer fields are set by
/// relocations.
#[allow(clippy::too_many_arguments)]
fn parse_compact_unwind(
    hdr: &MachSection,
    isecs: &[InputSection],
    subsecs: &[crate::input_sections::InputSectionId],
    nlists: &[NList],
    data: &'static [u8],
    file_name: &Path,
    out: &mut Vec<UnwindRecord>,
) {
    // Diagnostics spell the path lossily.
    let file_name = file_name.display();
    let geo: Vec<(u64, u64, usize)> = subsecs
        .iter()
        .map(|&id| {
            (isecs[id as usize].input_addr as u64, isecs[id as usize].size as u64, id as usize)
        })
        .collect();
    let find_subsec = |addr: u64| -> Option<(usize, u32)> {
        let i = geo.partition_point(|&(start, _, _)| start <= addr);
        if i == 0 {
            return None;
        }
        let (start, size, id) = geo[i - 1];
        if addr < start + size || (size == 0 && addr == start) {
            Some((id, (addr - start) as u32))
        } else {
            None
        }
    };
    const ENTRY_SIZE: usize = 32;
    if !hdr.size.is_multiple_of(ENTRY_SIZE as u64) {
        fatal!("{file_name}: invalid __compact_unwind section size");
    }

    let read_u64 = |off: u64| {
        let off = (hdr.offset as u64 + off) as usize;
        u64::from_le_bytes(data[off..off + 8].try_into().unwrap())
    };

    let num_entries = (hdr.size / ENTRY_SIZE as u64) as usize;
    let mut records = Vec::with_capacity(num_entries);
    for i in 0..num_entries {
        records.push(UnwindRecord {
            isec: u32::MAX,
            input_offset: 0,
            code_len: u32::from_le_bytes({
                let off = hdr.offset as usize + i * ENTRY_SIZE + 8;
                data[off..off + 4].try_into().unwrap()
            }),
            encoding: u32::from_le_bytes({
                let off = hdr.offset as usize + i * ENTRY_SIZE + 12;
                data[off..off + 4].try_into().unwrap()
            }),
            personality_sym: UNWIND_NONE,
            lsda_isec: UNWIND_NONE,
            lsda_off: 0,
            fde_idx: UNWIND_NONE,
        });
    }

    let rels: Vec<MachRel> = read_array(data, hdr.reloff as usize, hdr.nreloc as usize);
    for r in &rels {
        if r.r_address as u64 >= hdr.size || r.r_length() != 3 {
            fatal!("{file_name}: __compact_unwind: unsupported relocation");
        }
        let idx = r.r_address as usize / ENTRY_SIZE;
        let value = read_u64(r.r_address as u64);

        match r.r_address as usize % ENTRY_SIZE {
            // The function the record covers. For an extern reference
            // the target is this object's own definition, located by
            // its nlist value.
            0 => {
                let addr = if r.is_extern() {
                    nlists[r.r_symbolnum() as usize].n_value + value
                } else {
                    value
                };
                let Some((isec, off)) = find_subsec(addr) else {
                    fatal!("{file_name}: __compact_unwind: bad function reference");
                };
                records[idx].isec = isec as u32;
                records[idx].input_offset = off;
            }
            // The personality function, recorded as a local symbol
            // index and mapped to a symbol at integration.
            16 => {
                let sym = if r.is_extern() {
                    Some(r.r_symbolnum() as usize)
                } else {
                    // Resolve a section-relative reference back to the
                    // symbol at that address.
                    nlists.iter().position(|n| n.is_extern() && n.n_value == value)
                };
                let Some(sym) = sym else {
                    fatal!("{file_name}: __compact_unwind: unsupported personality");
                };
                records[idx].personality_sym = sym as u32;
            }
            // The language-specific data area
            24 => {
                let addr = if r.is_extern() {
                    nlists[r.r_symbolnum() as usize].n_value + value
                } else {
                    value
                };
                let Some(lsda) = find_subsec(addr) else {
                    fatal!("{file_name}: __compact_unwind: bad LSDA reference");
                };
                records[idx].lsda_isec = lsda.0 as u32;
                records[idx].lsda_off = lsda.1;
            }
            _ => fatal!("{file_name}: __compact_unwind: unsupported relocation"),
        }
    }

    // Records that point to DWARF unwind info keep their DWARF-mode
    // encoding; parse_eh_frame attaches the FDE (a final link
    // regenerates the encoding from it, a -r output copies the record
    // as it came, like ld64). Object files usually don't contain such
    // records, but `ld -r` output does.
    records.retain(|rec| rec.isec != u32::MAX);
    out.extend(records);
}

/// A DWARF Common Information Entry from an object's __eh_frame.
#[derive(Debug)]
pub struct Cie {
    /// The owning object (u32 index).
    pub obj: u32,
    pub input_addr: u32,
    /// The CIE bytes: a slice of the object's __eh_frame (with its
    /// subtraction pairs pre-applied), not a per-record copy - mold's
    /// CieRecord borrows its contents the same way.
    pub data: &'static [u8],
    pub personality: Option<SymbolId>,
    pub personality_offset: u32,
    pub lsda_size: u8,
    pub output_offset: u32,
    pub is_alive: bool,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Cie>() == 48);

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

/// Parses a __TEXT,__eh_frame section. Unlike other sections it is not
/// copied through: the linker re-synthesizes it, keeping only FDEs for
/// functions that have no compact unwind record, patching each CIE's
/// personality cell to be GOT-relative, and dropping the rest.
#[allow(clippy::too_many_arguments)]
fn parse_eh_frame<E: Target>(
    hdr: &MachSection,
    isecs: &[InputSection],
    subsecs: &[crate::input_sections::InputSectionId],
    nlists: &[NList],
    data: &'static [u8],
    file_name: &Path,
    unwind: &mut Vec<UnwindRecord>,
    out_cies: &mut Vec<Cie>,
    out_fdes: &mut Vec<Fde>,
    // Keep the FDEs of functions a compact record already covers: a
    // -r output carries every input CIE and FDE through, as ld64's
    // does, and a -static image has no __unwind_info for the compact
    // record; any other final image has no use for them.
    keep_all_fdes: bool,
) {
    // Diagnostics spell the path lossily.
    let file_name = file_name.display();
    let geo: Vec<(u64, u64, usize)> = subsecs
        .iter()
        .map(|&id| {
            (isecs[id as usize].input_addr as u64, isecs[id as usize].size as u64, id as usize)
        })
        .collect();
    let find_local = |addr: u64| -> Option<(usize, u32)> {
        let i = geo.partition_point(|&(start, _, _)| start <= addr);
        if i == 0 {
            return None;
        }
        let (start, size, id) = geo[i - 1];
        if addr < start + size || (size == 0 && addr == start) {
            Some((id, (addr - start) as u32))
        } else {
            None
        }
    };
    let mut contents = data[hdr.offset as usize..(hdr.offset as u64 + hdr.size) as usize].to_vec();
    let rels: Vec<MachRel> = read_array(data, hdr.reloff as usize, hdr.nreloc as usize);

    // Pre-apply subtraction pairs so record contents become
    // self-relative; leave GOT-relative personality references for
    // later.
    let mut i = 0;
    while i < rels.len() {
        let r1 = rels[i];
        if r1.r_type() == E::RELOC_SUBTRACTOR {
            let r2 = rels[i + 1];
            i += 2;
            if r2.r_type() != E::RELOC_UNSIGNED || !r1.is_extern() || !r2.is_extern() {
                fatal!("{file_name}: __eh_frame: unsupported relocation pair");
            }
            let target1 = nlists[r1.r_symbolnum() as usize].n_value;
            let target2 = nlists[r2.r_symbolnum() as usize].n_value;
            let loc = &mut contents[r1.r_address as usize..];
            let delta = target2.wrapping_sub(target1);
            match r1.r_length() {
                2 => {
                    let val = u32::from_le_bytes(loc[..4].try_into().unwrap());
                    loc[..4].copy_from_slice(&val.wrapping_add(delta as u32).to_le_bytes());
                }
                3 => {
                    let val = u64::from_le_bytes(loc[..8].try_into().unwrap());
                    let add = delta as u32 as i32 as i64 as u64;
                    loc[..8].copy_from_slice(&val.wrapping_add(add).to_le_bytes());
                }
                _ => fatal!("{file_name}: __eh_frame: invalid relocation size"),
            }
        } else if r1.r_type() == E::RELOC_GOTPC {
            i += 1;
        } else {
            fatal!("{file_name}: __eh_frame: unknown relocation type");
        }
    }

    // Split the section into records: a zero ID marks a CIE, anything
    // else is an FDE pointing back at its CIE.
    let mut fdes: Vec<(u32, &'static [u8])> = Vec::new();
    let mut pos = 0;
    // The records borrow from this processed copy of the section, leaked
    // once per object (like its section headers): the CIE/FDE bytes then
    // need no per-record copy, and they carry the pre-applied pairs.
    let contents: &'static [u8] = Vec::leak(contents);
    while pos < contents.len() {
        if pos + 4 > contents.len() {
            fatal!("{file_name}: malformed __eh_frame section: truncated CFI length");
        }
        let len = u32::from_le_bytes(contents[pos..pos + 4].try_into().unwrap()) as usize;
        if len == 0xffff_ffff {
            fatal!("{file_name}: __eh_frame: extended length is not supported");
        }
        if len < 4 || pos + 4 + len > contents.len() {
            fatal!("{file_name}: malformed __eh_frame section: CFI length too long");
        }
        let rec: &'static [u8] = &contents[pos..pos + 4 + len];
        let id = u32::from_le_bytes(rec[4..8].try_into().unwrap());
        let input_addr = hdr.addr as u32 + pos as u32;
        if id == 0 {
            out_cies.push(Cie {
                obj: u32::MAX,
                input_addr,
                data: rec,
                personality: None,
                personality_offset: 0,
                lsda_size: 0,
                output_offset: 0,
                is_alive: false,
            });
        } else {
            fdes.push((input_addr, rec));
        }
        pos += 4 + len;
    }

    // Validate CIE augmentations and record LSDA encodings.
    for cie in out_cies.iter_mut() {
        let data = &cie.data;
        if data.get(9).copied() != Some(b'z') {
            continue;
        }
        let aug_start = 9;
        let aug_end = aug_start + data[aug_start..].iter().position(|&b| b == 0).unwrap();
        let mut pos = aug_end + 1;
        read_uleb_at(data, &mut pos); // code alignment
        read_uleb_at(data, &mut pos); // data alignment
        read_uleb_at(data, &mut pos); // return address register
        read_uleb_at(data, &mut pos); // augmentation data length
        for &c in &data[aug_start + 1..aug_end] {
            match c {
                b'L' => {
                    cie.lsda_size = match data[pos] & 0xf {
                        0x3 | 0xb => 4, // DW_EH_PE_udata4, DW_EH_PE_sdata4
                        0x0 => 8,       // DW_EH_PE_absptr
                        enc => fatal!("{file_name}: __eh_frame: unknown LSDA encoding: {enc:#x}"),
                    };
                    pos += 1;
                }
                b'P' => {
                    // DW_EH_PE_indirect | DW_EH_PE_pcrel | DW_EH_PE_sdata4
                    if data[pos] != 0x9b {
                        fatal!(
                            "{file_name}: __eh_frame: unknown personality encoding: {:#x}",
                            data[pos]
                        );
                    }
                    pos += 5;
                }
                b'R' => pos += 1,
                // 'S' marks a signal frame and 'B' is the GNU
                // "frameless" augmentation; neither carries
                // augmentation data.
                b'S' | b'B' => {}
                _ => fatal!("{file_name}: __eh_frame: unknown augmentation"),
            }
        }
    }

    // Personality references appear as GOT-relative relocations inside
    // a CIE.
    for r in &rels {
        if r.r_type() != E::RELOC_GOTPC {
            continue;
        }
        let addr = hdr.addr as u32 + r.r_address;
        let Some(cie) = out_cies
            .iter_mut()
            .find(|c| c.input_addr <= addr && addr < c.input_addr + c.data.len() as u32)
        else {
            fatal!("{file_name}: __eh_frame: stray personality relocation");
        };
        if !r.is_extern() {
            fatal!("{file_name}: __eh_frame: unsupported personality reference");
        }
        // A local symbol index, mapped to a symbol at integration.
        cie.personality = Some(r.r_symbolnum());
        cie.personality_offset = addr - cie.input_addr;
    }

    // Functions that already have a compact unwind record don't need
    // their FDE; the compact record wins. A DWARF-mode record is the
    // exception: it exists to point at the FDE.
    let mut covered: std::collections::HashSet<(usize, u32)> = std::collections::HashSet::new();
    let mut dwarf_recs: std::collections::HashMap<(usize, u32), usize> =
        std::collections::HashMap::new();
    for (i, rec) in unwind.iter().enumerate() {
        if rec.encoding & UNWIND_MODE_MASK == E::UNWIND_MODE_DWARF {
            dwarf_recs.insert((rec.isec as usize, rec.input_offset), i);
        } else {
            covered.insert((rec.isec as usize, rec.input_offset));
        }
    }

    for (input_addr, rec) in fdes {
        let cie_off = u32::from_le_bytes(rec[4..8].try_into().unwrap());
        let cie_addr = input_addr + 4 - cie_off;
        let Some(cie) = out_cies.iter().position(|c| c.input_addr == cie_addr) else {
            fatal!("{file_name}: __eh_frame: FDE with an invalid CIE pointer");
        };

        // The function address: the pre-applied pc_begin field is
        // relative to itself.
        let pc_begin = i64::from_le_bytes(rec[8..16].try_into().unwrap());
        let func_addr = (input_addr as u64 + 8).wrapping_add_signed(pc_begin);
        let code_len = u64::from_le_bytes(rec[16..24].try_into().unwrap()) as u32;

        let Some((isec, func_offset)) = find_local(func_addr) else {
            fatal!("{file_name}: __eh_frame: FDE with an invalid function");
        };

        let is_covered = covered.contains(&(isec, func_offset));
        if is_covered && !keep_all_fdes {
            continue;
        }

        // The LSDA pointer, if the CIE declares one: also pre-applied to
        // be self-relative.
        let mut lsda = None;
        if out_cies[cie].lsda_size != 0 {
            let mut pos = 24;
            read_uleb_at(rec, &mut pos);
            let cell = i32::from_le_bytes(rec[pos..pos + 4].try_into().unwrap());
            let lsda_addr = (input_addr as u64 + pos as u64).wrapping_add_signed(cell as i64);
            let Some((lsda_isec, lsda_off)) = find_local(lsda_addr) else {
                fatal!("{file_name}: __eh_frame: FDE with an invalid LSDA");
            };
            lsda = Some((lsda_isec, lsda_off));
        }

        let fde_idx = out_fdes.len();
        out_fdes.push(Fde {
            obj: u32::MAX,
            input_addr,
            data: rec,
            cie: cie as u32,
            isec: isec as u32,
            func_offset,
            code_len,
            lsda: lsda.map(|(i, o)| (i as u32, o)),
            output_offset: 0,
        });

        // A covered function's compact record wins; its FDE is only
        // carried. Otherwise the object's own DWARF-mode record now
        // points at the FDE, or one is synthesized so that the unwinder
        // can find the FDE through __unwind_info.
        if is_covered {
            continue;
        }
        if let Some(&i) = dwarf_recs.get(&(isec, func_offset)) {
            unwind[i].fde_idx = fde_idx as u32;
            continue;
        }
        unwind.push(UnwindRecord {
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

/// Returns the slice of a fat (universal) file matching the target's CPU
/// type. Fat headers are big-endian.
pub fn get_fat_slice<E: Target>(mf: &'static MappedFile) -> &'static MappedFile {
    let data = mf.data();
    let read_be32 = |off: usize| u32::from_be_bytes(data[off..off + 4].try_into().unwrap());

    let nfat_arch = read_be32(4) as usize;
    for i in 0..nfat_arch {
        let off = 8 + i * 20;
        if read_be32(off) == E::CPUTYPE {
            let obj_off = read_be32(off + 8) as usize;
            let obj_size = read_be32(off + 12) as usize;
            let mut name = std::ffi::OsString::from(&mf.name);
            name.push(format!("(for architecture {})", E::NAME));
            return mf.slice(name.into(), obj_off, obj_size);
        }
    }
    fatal!("{}: fat file does not contain {}", mf.name.display(), E::NAME);
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
fn is_public_location(install_name: &[u8]) -> bool {
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

/// Loads the libraries a dylib re-exports. A public one becomes an
/// implicit dylib of its own (its symbols bind to it), recursively
/// loading what it re-exports in turn; a private one's exports are
/// merged into `exports`/`tlv_exports` as the re-exporting dylib's,
/// and its own re-exports are walked the same way. A library may be a
/// file of its own or a document inlined in a stub (`documents`: the
/// re-exporting stub's). A public one is loaded from its file when one
/// exists, as ld-prime does, and from its document otherwise; a private
/// one inlined is merged from its document. Returns the install names
/// of the private libraries merged.
/// Notes an input file for -t as it is loaded. ld-prime names a fat
/// file's slice, and its members, by the file's own path.
pub fn trace_file<E: Target>(ctx: &mut Context<E>, name: &[u8]) {
    if ctx.args.trace {
        let mut name = name.to_vec();
        if let Some(i) = memchr::memmem::find(&name, b"(for architecture")
            && let Some(len) = name[i..].iter().position(|&c| c == b')')
        {
            name.drain(i..=i + len);
        }
        ctx.traced_files.push(crate::util::display(&name).to_string());
    }
}

fn load_reexports<E: Target>(
    ctx: &mut Context<E>,
    reexports: Vec<(Vec<u8>, PathBuf, Vec<PathBuf>)>,
    parent: &Path,
    documents: Vec<tapi::TbdFile>,
    exports: &mut hashbrown::HashSet<&'static str>,
    tlv_exports: &mut hashbrown::HashSet<&'static str>,
    weak_exports: &mut hashbrown::HashSet<&'static str>,
) -> Vec<Vec<u8>> {
    let mut queue = reexports;
    let mut visited = std::collections::HashSet::new();
    let mut merged = Vec::new();
    // The inlined documents a name may resolve to: the parent's, and
    // those of every stub merged along the way.
    let mut pool = documents;
    while let Some((name, loader_dir, loader_rpaths)) = queue.pop() {
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
                exports.extend(loaded.exports.iter().copied());
                tlv_exports.extend(loaded.tlv_exports.iter().copied());
                weak_exports.extend(loaded.weak_exports.iter().copied());
                merged.push(name);
            }
            continue;
        }
        let inline = pool.iter().position(|d| d.install_name.as_bytes() == name);
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
            if ctx.args.trace {
                let found = resolve_dylib_ref(ctx, &name, &loader_dir, &loader_rpaths)
                    .map(|mf| crate::util::path_bytes(&mf.name).to_vec());
                trace_file(ctx, found.as_deref().unwrap_or(&name));
            }
            let mut doc = pool[i].clone();
            if DylibIdentity::of_tbd(&doc).is_public(ctx) {
                let idx = register_tbd(ctx, parent, doc, pool.clone());
                ctx.dylibs[idx].is_implicit = true;
                continue;
            }
            interpret_ld_symbols(ctx, &mut doc);
            tlv_exports.extend(doc.tlv_exports.iter().copied());
            exports.extend(doc.tlv_exports);
            exports.extend(doc.exports);
            weak_exports.extend(doc.weak_exports.iter().copied());
            exports.extend(doc.weak_exports);
            for dep_name in doc.reexports {
                queue.push((
                    dep_name.as_bytes().to_vec(),
                    loader_dir.clone(),
                    loader_rpaths.clone(),
                ));
            }
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
        trace_file(ctx, crate::util::path_bytes(&dep.name));
        // The file found decides by its own install name, which a lookup
        // by leaf name may find to differ from the one re-exported:
        // ld-prime binds to libz a symbol of /opt/x/libz.dylib that it
        // found as the SDK's /usr/lib/libz.1.dylib, and merges a
        // /usr/lib/libq.dylib found as /opt/q/libq.dylib.
        match crate::filetype::get_file_type(dep) {
            crate::filetype::FileType::Tapi => {
                let mut dep_tbd = tapi::parse_cached(dep, E::NAME);
                if DylibIdentity::of_tbd(&dep_tbd).is_public(ctx) {
                    let idx = parse_dylib(ctx, dep);
                    ctx.dylibs[idx].is_implicit = true;
                    continue;
                }
                merged.push(dep_tbd.install_name.as_bytes().to_vec());
                interpret_ld_symbols(ctx, &mut dep_tbd);
                tlv_exports.extend(dep_tbd.tlv_exports.iter().copied());
                exports.extend(dep_tbd.tlv_exports);
                exports.extend(dep_tbd.exports);
                weak_exports.extend(dep_tbd.weak_exports.iter().copied());
                exports.extend(dep_tbd.weak_exports);
                pool.extend(dep_tbd.documents);
                for dep_name in dep_tbd.reexports {
                    queue.push((dep_name.as_bytes().to_vec(), dir_of(&dep.name), Vec::new()));
                }
            }
            crate::filetype::FileType::Dylib => {
                let found = DylibIdentity::of_binary(dep);
                if found.is_public(ctx) {
                    let idx = parse_dylib_binary(ctx, dep);
                    ctx.dylibs[idx].is_implicit = true;
                    continue;
                }
                merged.push(found.install_name);
                check_dylib_versions(ctx, dep);
                let (dep_exports, dep_tlvs, dep_reexports, dep_rpaths) = dylib_binary_exports(dep);
                exports.extend(dep_exports);
                tlv_exports.extend(dep_tlvs);
                for dep_name in dep_reexports {
                    queue.push((dep_name, dir_of(&dep.name), dep_rpaths.clone()));
                }
            }
            crate::filetype::FileType::Fat => {
                let slice = get_fat_slice::<E>(dep);
                let found = DylibIdentity::of_binary(slice);
                if found.is_public(ctx) {
                    let idx = parse_dylib_binary(ctx, slice);
                    ctx.dylibs[idx].is_implicit = true;
                    continue;
                }
                merged.push(found.install_name);
                check_dylib_versions(ctx, slice);
                let (dep_exports, dep_tlvs, dep_reexports, dep_rpaths) =
                    dylib_binary_exports(slice);
                exports.extend(dep_exports);
                tlv_exports.extend(dep_tlvs);
                for dep_name in dep_reexports {
                    queue.push((dep_name, dir_of(&dep.name), dep_rpaths.clone()));
                }
            }
            _ => crate::warn!(
                "{}: unsupported reexported library: {}",
                parent.display(),
                crate::util::display(&name)
            ),
        }
    }
    merged
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
pub fn dylib_identity<E: Target>(mf: &'static MappedFile) -> DylibIdentity {
    match crate::filetype::get_file_type(mf) {
        crate::filetype::FileType::Tapi => DylibIdentity::of_tbd(&tapi::parse_cached(mf, E::NAME)),
        _ => DylibIdentity::of_binary(mf),
    }
}

/// Whether the dylib in a stub or binary file exports a symbol that the
/// link uses and neither an object nor a dylib loaded so far defines
/// (SwiftUI, auto-linked before SwiftUICore, re-exports all of it).
pub fn provides_undefined<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> bool {
    let names: Vec<&'static str> = match crate::filetype::get_file_type(mf) {
        crate::filetype::FileType::Tapi => {
            let tbd = tapi::parse_cached(mf, E::NAME);
            [tbd.exports, tbd.weak_exports, tbd.tlv_exports].concat()
        }
        _ => dylib_binary_exports(mf).0,
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

/// Check binary dependencies, including private reexports whose symbols
/// are merged into their parent's export set instead of a DylibFile.
fn check_dylib_versions<E: Target>(ctx: &Context<E>, mf: &MappedFile) {
    let hdr = MachHeader::read_from(mf.data());
    let mut versions = Vec::new();
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let data = &mf.data()[off..];
        let lc = LoadCommand::read_from(data);
        if matches!(
            lc.cmd,
            LC_BUILD_VERSION
                | LC_VERSION_MIN_MACOSX
                | LC_VERSION_MIN_IPHONEOS
                | LC_VERSION_MIN_TVOS
                | LC_VERSION_MIN_WATCHOS
        ) {
            versions.push(PlatformVersion::read(lc.cmd, data, hdr.cputype));
        }
        off += lc.cmdsize as usize;
    }
    if let Some(first) = versions.first() {
        if let Some(version) = versions.iter().find(|v| v.platform == ctx.args.platform) {
            if ctx.args.platform_minos != 0 && version.minos > ctx.args.platform_minos {
                crate::warn!(
                    "building for {}-{}, but linking with dylib '{}' which was built for newer version {}",
                    platform_name(ctx.args.platform),
                    format_version(ctx.args.platform_minos),
                    mf.name.display(),
                    format_version(version.minos)
                );
            }
        } else {
            fatal!(
                "building for '{}', but linking in dylib ({}) built for '{}'",
                platform_name(ctx.args.platform),
                mf.name.display(),
                platform_name(first.platform)
            );
        }
    }
}

pub fn parse_dylib_binary<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) -> usize {
    check_dylib_versions(ctx, mf);
    let data = mf.data();
    let hdr = MachHeader::read_from(data);

    let mut install_name: Vec<u8> = Vec::new();
    let mut current_version = encode_version(1, 0, 0);
    let mut compatibility_version = encode_version(1, 0, 0);
    let mut symtab_cmd = None;
    let mut dysymtab_cmd = None;
    let mut reexports: Vec<Vec<u8>> = Vec::new();
    let mut rpaths: Vec<PathBuf> = Vec::new();

    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        match lc.cmd {
            LC_ID_DYLIB => {
                let cmd = DylibCommand::read_from(&data[off..]);
                let name = &data[off + cmd.nameoff as usize..off + cmd.cmdsize as usize];
                let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                install_name = name[..len].to_vec();
                current_version = cmd.current_version;
                compatibility_version = cmd.compatibility_version;
            }
            LC_SYMTAB => symtab_cmd = Some(SymtabCommand::read_from(&data[off..])),
            LC_DYSYMTAB => dysymtab_cmd = Some(DysymtabCommand::read_from(&data[off..])),
            LC_REEXPORT_DYLIB => {
                let cmd = DylibCommand::read_from(&data[off..]);
                let name = &data[off + cmd.nameoff as usize..off + cmd.cmdsize as usize];
                let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                reexports.push(name[..len].to_vec());
            }
            LC_RPATH => {
                let cmd = DylinkerCommand::read_from(&data[off..]);
                let name = &data[off + cmd.nameoff as usize..off + cmd.cmdsize as usize];
                let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                rpaths.push(loader_rpath(&mf.name, &name[..len]));
            }
            _ => {}
        }
        off += lc.cmdsize as usize;
    }

    if install_name.is_empty() {
        fatal!("{}: dylib has no LC_ID_DYLIB", mf.name.display());
    }

    let mut exports: hashbrown::HashSet<&'static str> = hashbrown::HashSet::new();
    let mut weak_exports: hashbrown::HashSet<&'static str> = hashbrown::HashSet::new();
    let mut tlv_exports: hashbrown::HashSet<&'static str> = hashbrown::HashSet::new();
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
            let name = symbol_name(strtab, nlist);
            if tlv_sects.contains(&nlist.n_sect) {
                tlv_exports.insert(name);
            }
            if nlist.n_desc & N_WEAK_DEF != 0 {
                weak_exports.insert(name);
            }
            exports.insert(name);
        }
    }
    if let Some((off, size)) = find_export_trie(data, &hdr) {
        for (name, flags) in export_trie_entries(data, off, size) {
            if flags as u32 & EXPORT_SYMBOL_FLAGS_KIND_MASK == EXPORT_SYMBOL_FLAGS_KIND_THREAD_LOCAL
            {
                tlv_exports.insert(name);
            }
            if flags as u32 & EXPORT_SYMBOL_FLAGS_WEAK_DEFINITION != 0 {
                weak_exports.insert(name);
            }
            exports.insert(name);
        }
    }

    // Each re-exported library keeps the referencing dylib's directory
    // and rpaths, since @loader_path and @rpath in an install name are
    // relative to the referrer.
    let reexports: Vec<(Vec<u8>, PathBuf, Vec<PathBuf>)> =
        reexports.into_iter().map(|name| (name, dir_of(&mf.name), rpaths.clone())).collect();
    let merged_reexports = load_reexports(
        ctx,
        reexports,
        &mf.name,
        Vec::new(),
        &mut exports,
        &mut tlv_exports,
        &mut weak_exports,
    );

    let priority = ctx.next_priority();
    add_dylib(
        ctx,
        DylibFile {
            path: mf.name.clone(),
            install_name,
            current_version,
            compatibility_version,
            dylib_idx: next_dylib_ordinal(ctx),
            is_bundle_loader: false,
            priority,
            is_weak: false,
            is_reexported: false,
            is_needed: false,
            is_autolinked: false,
            is_implicit: false,
            load_order: u32::MAX,
            is_dead_strippable: hdr.flags & MH_DEAD_STRIPPABLE_DYLIB != 0,
            exports,
            weak_exports,
            tlv_exports,
            merged_reexports,
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
    if hdr.magic != MH_MAGIC_64 || hdr.filetype != MH_EXECUTE {
        fatal!("{}: -bundle_loader is not an executable", mf.name.display());
    }

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
            dylib_idx: BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE,
            is_bundle_loader: true,
            priority,
            is_weak: false,
            is_reexported: false,
            is_needed: false,
            is_autolinked: false,
            is_implicit: false,
            load_order: u32::MAX,
            is_dead_strippable: false,
            exports,
            weak_exports: hashbrown::HashSet::new(),
            tlv_exports,
            merged_reexports: Vec::new(),
        },
    )
}

/// Reads a dylib binary's exported symbols and reexported install
/// names, for following reexport chains.
/// What a dylib binary contributes to a re-exporting parent: its
/// exports, its thread-local exports, the install names it re-exports
/// in turn, and its rpaths, resolved for its location.
type DylibExports = (Vec<&'static str>, Vec<&'static str>, Vec<Vec<u8>>, Vec<PathBuf>);

fn dylib_binary_exports(mf: &'static MappedFile) -> DylibExports {
    let data = mf.data();
    let hdr = MachHeader::read_from(data);
    let mut symtab_cmd = None;
    let mut dysymtab_cmd = None;
    let mut reexports = Vec::new();
    let mut rpaths = Vec::new();

    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(&data[off..]);
        match lc.cmd {
            LC_SYMTAB => symtab_cmd = Some(SymtabCommand::read_from(&data[off..])),
            LC_DYSYMTAB => dysymtab_cmd = Some(DysymtabCommand::read_from(&data[off..])),
            LC_REEXPORT_DYLIB => {
                let cmd = DylibCommand::read_from(&data[off..]);
                let name = &data[off + cmd.nameoff as usize..off + cmd.cmdsize as usize];
                let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                reexports.push(name[..len].to_vec());
            }
            LC_RPATH => {
                let cmd = DylinkerCommand::read_from(&data[off..]);
                let name = &data[off + cmd.nameoff as usize..off + cmd.cmdsize as usize];
                let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                rpaths.push(loader_rpath(&mf.name, &name[..len]));
            }
            _ => {}
        }
        off += lc.cmdsize as usize;
    }

    let mut exports: Vec<&'static str> = Vec::new();
    let mut tlv_exports: Vec<&'static str> = Vec::new();
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
                tlv_exports.push(name);
            }
            exports.push(name);
        }
    }
    if let Some((off, size)) = find_export_trie(data, &hdr) {
        for (name, flags) in export_trie_entries(data, off, size) {
            if flags as u32 & EXPORT_SYMBOL_FLAGS_KIND_MASK == EXPORT_SYMBOL_FLAGS_KIND_THREAD_LOCAL
            {
                tlv_exports.push(name);
            }
            exports.push(name);
        }
    }
    (exports, tlv_exports, reexports, rpaths)
}

/// The directory dyld would use for a dylib's @loader_path: that of
/// the real file, symlinks resolved. A framework's X.framework/X is a
/// symlink to Versions/A/X, and its LC_RPATH entries are written for
/// that location (XCTest's `@loader_path/../../../../PrivateFrameworks`
/// reaches XCTestCore only from Versions/A). A fat file's name may
/// carry the "(for architecture ...)" suffix the loader adds.
/// An LC_RPATH entry as a search directory: @loader_path stands for the
/// directory of the dylib that carries the entry.
fn loader_rpath(dylib: &Path, rpath: &[u8]) -> PathBuf {
    match rpath.strip_prefix(b"@loader_path/") {
        Some(rest) => dir_of(dylib).join(crate::util::os_str(rest)),
        None => PathBuf::from(crate::util::os_str(rpath)),
    }
}

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
/// names the dependency, @rpath tries that dylib's own LC_RPATH
/// entries, and @executable_path stands for the output executable's
/// directory (or -executable_path).
fn resolve_dylib_ref<E: Target>(
    ctx: &Context<E>,
    name: &[u8],
    loader_dir: &Path,
    loader_rpaths: &[PathBuf],
) -> Option<&'static MappedFile> {
    use crate::util::{os_str, path_bytes};
    // A name relative to the re-exporter, its executable or its rpaths
    // resolves as such first, and failing that like an absolute one.
    if let Some(rest) = name.strip_prefix(b"@loader_path/") {
        return find_reexport_file(ctx, path_bytes(&loader_dir.join(os_str(rest))))
            .or_else(|| find_reexport_by_leaf(ctx, name));
    }
    if let Some(rest) = name.strip_prefix(b"@executable_path/") {
        let exe = match &ctx.args.executable_path {
            Some(path) => Some(path.clone()),
            None if ctx.args.output_type == MH_EXECUTE => Some(ctx.args.output.clone()),
            None => None,
        };
        return exe
            .and_then(|exe| find_reexport_file(ctx, path_bytes(&dir_of(&exe).join(os_str(rest)))))
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
    for dir in crate::passes::library_search_dirs(ctx) {
        for ext in [".tbd", ".dylib"] {
            let mut file = stem.clone();
            file.push(ext);
            if let Some(mf) = MappedFile::open(dir.join(file)) {
                if crate::filetype::get_file_type(mf) == crate::filetype::FileType::Fat {
                    return Some(get_fat_slice::<E>(mf));
                }
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
                // A universal binary (Xcode's XCTestCore, re-exported
                // by XCTest): the target's slice.
                if crate::filetype::get_file_type(mf) == crate::filetype::FileType::Fat {
                    return Some(get_fat_slice::<E>(mf));
                }
                return Some(mf);
            }
        }
    }
    None
}

/// Interprets a .tbd's "$ld$..." export names. These are not symbols
/// but directives to the linker, invented so a stub library could
/// change shape per deployment target without a file format change:
/// $ld$add$os<ver>$<sym> exports <sym> only when the target equals
/// <ver>, $ld$hide$os<ver>$<sym> hides one, $ld$install_name$os<ver>$
/// <name> substitutes the recorded install name, and
/// $ld$previous$<name>$<compat>$<platform>$<lo>$<hi>$<sym>$ applies
/// <name> when the target platform matches and lo <= minos < hi
/// (the per-symbol form never worked in ld64 and is ignored, as sold
/// found). Apple uses these when a symbol moves between libraries:
/// old targets keep binding it where it used to live.
fn interpret_ld_symbols<E: Target>(ctx: &Context<E>, tbd: &mut tapi::TbdFile) {
    let minos = ctx.args.platform_minos;
    let mut added: Vec<&'static str> = Vec::new();
    let mut hidden: hashbrown::HashSet<&'static str> = hashbrown::HashSet::new();
    let mut install_name: Option<String> = None;

    for name in &tbd.exports {
        if let Some(rest) = name.strip_prefix("$ld$previous$") {
            let f: Vec<&str> = rest.split('$').collect();
            if f.len() < 6 {
                crate::warn!("malformed linker directive: {name}");
            } else if f[5].is_empty()
                && f[2].parse::<u32>() == Ok(ctx.args.platform)
                && tapi::parse_version(f[3]) <= minos
                && minos < tapi::parse_version(f[4])
            {
                install_name = Some(f[0].to_string());
            }
        } else if let Some(rest) = name.strip_prefix("$ld$add$os") {
            if let Some((ver, sym)) = rest.split_once('$')
                && tapi::parse_version(ver) == minos
            {
                added.push(sym);
            }
        } else if let Some(rest) = name.strip_prefix("$ld$hide$os") {
            if let Some((ver, sym)) = rest.split_once('$')
                && tapi::parse_version(ver) == minos
            {
                hidden.insert(sym);
            }
        } else if let Some(rest) = name.strip_prefix("$ld$install_name$os")
            && let Some((ver, new_name)) = rest.split_once('$')
            && tapi::parse_version(ver) == minos
        {
            install_name = Some(new_name.to_string());
        }
    }

    tbd.exports.retain(|n| !n.starts_with("$ld$") && !hidden.contains(n));
    tbd.weak_exports.retain(|n| !hidden.contains(n));
    tbd.exports.extend(added);
    if let Some(name) = install_name {
        tbd.install_name = name;
    }
}

pub fn parse_dylib<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) -> usize {
    let mut tbd = tapi::parse_cached(mf, E::NAME);
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
    interpret_ld_symbols(ctx, &mut tbd);
    let mut exports: hashbrown::HashSet<&'static str> = tbd.exports.into_iter().collect();
    let mut weak_exports: hashbrown::HashSet<&'static str> =
        tbd.weak_exports.iter().copied().collect();
    exports.extend(tbd.weak_exports);
    let mut tlv_exports: hashbrown::HashSet<&'static str> = tbd.tlv_exports.into_iter().collect();
    exports.extend(tlv_exports.iter().copied());

    let reexports: Vec<(Vec<u8>, PathBuf, Vec<PathBuf>)> = tbd
        .reexports
        .into_iter()
        .map(|name| (name.as_bytes().to_vec(), dir_of(path), Vec::new()))
        .collect();
    let merged_reexports = load_reexports(
        ctx,
        reexports,
        path,
        documents,
        &mut exports,
        &mut tlv_exports,
        &mut weak_exports,
    );

    let priority = ctx.next_priority();
    add_dylib(
        ctx,
        DylibFile {
            path: path.to_path_buf(),
            install_name: tbd.install_name.into_bytes(),
            current_version: tbd.current_version,
            compatibility_version: tbd.compatibility_version,
            dylib_idx: next_dylib_ordinal(ctx),
            is_bundle_loader: false,
            priority,
            is_weak: false,
            is_reexported: false,
            is_needed: false,
            is_autolinked: false,
            is_implicit: false,
            load_order: u32::MAX,
            is_dead_strippable: false,
            exports,
            weak_exports,
            tlv_exports,
            merged_reexports,
        },
    )
}

/// Registers a dylib, deduplicating by install name: several libraries
/// (libc, libm, ...) are stubs for the same /usr/lib/libSystem.B.dylib,
/// and dyld refuses an image that lists one install name twice.
fn add_dylib<E: Target>(ctx: &mut Context<E>, dylib: DylibFile) -> usize {
    if let Some(idx) = ctx.dylibs.iter().position(|d| d.install_name == dylib.install_name) {
        let existing = &mut ctx.dylibs[idx];
        existing.exports.extend(dylib.exports);
        existing.merged_reexports.extend(dylib.merged_reexports);
        return idx;
    }
    ctx.dylibs.push(dylib);
    ctx.dylibs.len() - 1
}
