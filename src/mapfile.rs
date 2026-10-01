//! -map file output: a report of where every object file, section and
//! symbol ended up, in ld64's format.

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::chunks::ChunkId;
use crate::context::Context;
use crate::input_files::{DylibFile, FileId, MergedFile, NameSource};
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;
use crate::util::path_bytes;

fn json_string(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x00'..='\x1f' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Xcode's version-1 API import report. Despite its name, sdkImports
/// includes imports from non-SDK dylibs too, grouped by install name.
pub fn write_sdk_imports<E: Target>(ctx: &Context<E>) {
    use crate::macho::{format_version, platform_name};
    let Some(path) = &ctx.args.sdk_imports else { return };
    let mut imports = std::collections::BTreeMap::<&[u8], Vec<&str>>::new();
    for sym in &ctx.symbols.syms {
        if !sym.is_imported() || !sym.is_used() {
            continue;
        }
        let Some(FileId::Dylib(idx)) = sym.file() else { continue };
        // Dynamic-lookup symbols have no defining library to report.
        let Some(dylib) = ctx.dylibs.get(idx as usize) else { continue };
        imports.entry(&dylib.install_name).or_default().push(sym.name());
    }
    let libraries: Vec<String> = imports
        .into_iter()
        .map(|(name, mut symbols)| {
            symbols.sort_unstable();
            symbols.dedup();
            let symbols: Vec<String> = symbols.into_iter().map(json_string).collect();
            format!(
                "{{\"installName\":{},\"symbols\":[{}]}}",
                json_string(&crate::util::display(name)),
                symbols.join(",")
            )
        })
        .collect();
    // JSON is text: a path or install name outside UTF-8 is spelled lossily.
    let output = json_string(&ctx.args.output.to_string_lossy());
    let report = format!(
        "{{\"version\":1,\"output\":{output},\"arch\":{},\"linker\":{},\"apiListVersion\":0,\
         \"platform\":{},\"deploymentVersion\":{},\"sdkVersion\":{},\
         \"inputs\":[{{\"path\":{output},\"sdkImports\":[{}]}}]}}\n",
        json_string(E::NAME),
        json_string(concat!("mold-macho-", env!("CARGO_PKG_VERSION"))),
        json_string(&platform_name(ctx.args.platform)),
        json_string(&format_version(ctx.args.platform_minos)),
        json_string(&format_version(ctx.args.platform_sdk)),
        libraries.join(",")
    );
    if std::fs::write(path, report).is_err() {
        crate::warn!("can't open SDK imports file for writing at '{}'", path.display());
    }
}

/// Writes the -dependency_info file: Xcode's incremental build system
/// reads it to learn which files the link actually consumed. The
/// format is binary: an opcode byte then a NUL-terminated string -
/// 0x00 version, 0x10 input file, 0x11 file that was looked up but
/// missing, 0x40 output file.
pub fn write_dependency_info<E: Target>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.dependency_info else {
        return;
    };
    let Ok(file) = std::fs::File::create(path) else {
        crate::warn!("Could not open or create -dependency_info file: {}", path.display());
        return;
    };
    let mut out = std::io::BufWriter::new(file);
    let mut emit = |op: u8, s: &[u8]| {
        let _ = out.write_all(&[op]);
        let _ = out.write_all(s);
        let _ = out.write_all(&[0]);
    };

    emit(0x00, concat!("mold-macho ", env!("CARGO_PKG_VERSION")).as_bytes());
    // The object LTO compiled is no input (a build system can't depend
    // on it), whatever -object_path_lto made of it.
    let mut inputs: Vec<&Path> = ctx
        .objs
        .iter()
        .enumerate()
        .filter(|&(i, o)| o.is_alive && !ctx.is_internal(i) && ctx.lto_obj != Some(i))
        .map(|(_, o)| o.mf.parent.map_or(o.mf.name.as_path(), |p| p.name.as_path()))
        .collect();
    inputs.extend(ctx.visited_files.iter().map(PathBuf::as_path));
    inputs.sort_unstable();
    inputs.dedup();
    for name in inputs {
        emit(0x10, path_bytes(name));
    }
    emit(0x40, path_bytes(&ctx.args.output));
}

/// A line of the map's symbol list: an atom's address and size, the
/// number of the file it came from, and its name (the bytes of a
/// literal, whatever they are).
struct MapEntry<'a> {
    addr: u64,
    size: u64,
    file: usize,
    name: Cow<'a, [u8]>,
}

fn name(s: &str) -> Cow<'_, [u8]> {
    Cow::Borrowed(s.as_bytes())
}

/// The files of the link as ld-prime's map numbers them, in the order
/// it loads them: from 1 in command line order - objects, the archive
/// members that were loaded, where their archive was named, and dylibs,
/// used or not (-dead_strip_dylibs or not) -, then by install name the
/// libraries loaded because a dylib re-exports them that something
/// binds to, and last the files auto-link options named that something
/// binds to or loads from, in the order ld-prime acts on the options -
/// dylibs, and archives' members -, and those their dylibs re-export.
/// A re-exported library named too is listed where it is named, by the
/// path given. The re-exported libraries include the private ones a
/// dylib merges (libSystem's libsystem_c), which ld-prime reads from
/// files of their own and credits with the symbols they define; an
/// auto-linked dylib all of whose bound symbols such a library defines
/// is no file of the link's (libswiftDarwin, which merges
/// libswift_Builtin_float). Number 0 stands for the linker, which makes
/// the stubs, the unwind info and such. A bitcode file is listed as any
/// object, and the object LTO compiled last of all; ld-prime credits
/// each of the latter's symbols to the bitcode file it came from, when
/// it can tell (see lto::origins).
struct MapFiles<'a> {
    paths: Vec<&'a Path>,
    /// The number of each object (0 for the internal one) and dylib.
    objs: Vec<usize>,
    dylibs: Vec<usize>,
    /// The number of the merged library each symbol a dylib provides
    /// through one comes from.
    merged: hashbrown::HashMap<SymbolId, usize>,
    /// The object whose tentative definition each common symbol's
    /// subsection stands for, by subsection.
    commons: hashbrown::HashMap<u32, u32>,
    /// The object LTO compiled, and the bitcode file each of its
    /// symbols comes from, by name.
    lto_obj: Option<usize>,
    lto_origins: hashbrown::HashMap<&'static str, Option<usize>>,
}

impl<'a> MapFiles<'a> {
    fn new<E: Target>(ctx: &'a Context<E>) -> Self {
        enum File<'a> {
            Obj(usize),
            Dylib(usize),
            Merged(&'a MergedFile),
            Stripped(&'a Path),
        }
        // A dylib that stands for a library exports moved to is no file:
        // its symbols count as the file's that moved them, which an
        // auto-linked one stands for where nothing binds to that.
        let is_moved = |dylib: &DylibFile| dylib.name_source == NameSource::Moved;
        let (provided, merged_only) = merged_providers(ctx);

        let mut named: Vec<(u32, File)> = Vec::new();
        let mut autolinked: Vec<((u32, u32), File)> = Vec::new();
        for (i, obj) in ctx.objs.iter().enumerate() {
            if !obj.is_alive || ctx.is_internal(i) || ctx.lto_obj == Some(i) {
                continue;
            }
            match obj.mf.parent.and_then(|ar| ctx.autolinked_archives.get(&ar.name)) {
                Some(&seq) => autolinked.push(((seq, obj.priority), File::Obj(i))),
                None => named.push((obj.priority, File::Obj(i))),
            }
        }
        for input in &ctx.lto_inputs {
            named.push((ctx.objs[input.obj].priority, File::Obj(input.obj)));
        }
        for (i, dylib) in ctx.dylibs.iter().enumerate() {
            if dylib.is_autolinked && !merged_only[i] {
                autolinked.push(((dylib.load_order, is_moved(dylib) as u32), File::Dylib(i)));
            } else if !dylib.is_autolinked && !dylib.is_implicit {
                let priority = dylib.named_at.as_ref().map_or(dylib.priority, |&(p, _)| p);
                named.push((priority, File::Dylib(i)));
            }
        }
        for (priority, path) in &ctx.stripped_dylibs {
            named.push((*priority, File::Stripped(path)));
        }

        let mut implicit: Vec<(&[u8], File)> = Vec::new();
        for (i, dylib) in ctx.dylibs.iter().enumerate() {
            if dylib.is_implicit && !dylib.is_autolinked && !is_moved(dylib) {
                implicit.push((&dylib.install_name, File::Dylib(i)));
            }
        }
        // A merged library goes with the dylib that merged it, but where
        // an auto-link option named it, if one did.
        let mut seen: hashbrown::HashSet<&[u8]> = hashbrown::HashSet::new();
        for &(_, d, file) in &provided {
            let dylib = &ctx.dylibs[d];
            if implicit.iter().any(|(name, _)| *name == file.install_name)
                || !seen.insert(&file.install_name)
            {
                continue;
            }
            if dylib.is_autolinked {
                let named = dylib.named_files.iter().find(|(_, path)| *path == file.path);
                let seq = named.map_or(u32::MAX, |&(seq, _)| seq);
                autolinked.push(((seq, 0), File::Merged(file)));
            } else {
                implicit.push((&file.install_name, File::Merged(file)));
            }
        }
        named.sort_by_key(|(priority, _)| *priority);
        implicit.sort_by_key(|(name, _)| *name);
        autolinked.sort_by_key(|(key, _)| *key);

        let mut files = Self {
            paths: Vec::new(),
            objs: vec![0; ctx.objs.len()],
            dylibs: vec![0; ctx.dylibs.len()],
            merged: hashbrown::HashMap::new(),
            commons: crate::output_sections::common_owners(ctx),
            lto_obj: ctx.lto_obj,
            lto_origins: crate::lto::origins(&ctx.lto_inputs),
        };
        let mut merged_numbers: hashbrown::HashMap<&[u8], usize> = hashbrown::HashMap::new();
        let named = named.into_iter().map(|(_, file)| file);
        let implicit = implicit.into_iter().map(|(_, file)| file);
        let autolinked = autolinked.into_iter().map(|(_, file)| file);
        let lto = ctx.lto_obj.map(File::Obj);
        for file in named.chain(implicit).chain(autolinked).chain(lto) {
            let number = files.paths.len() + 1;
            match file {
                File::Obj(i) => {
                    files.objs[i] = number;
                    files.paths.push(&ctx.objs[i].mf.name);
                }
                File::Dylib(i) => {
                    let dylib = &ctx.dylibs[i];
                    if is_moved(dylib) && files.paths.contains(&dylib.path.as_path()) {
                        continue;
                    }
                    files.dylibs[i] = number;
                    files.paths.push(dylib.named_at.as_ref().map_or(&dylib.path, |(_, path)| path));
                }
                File::Stripped(path) => files.paths.push(path),
                File::Merged(file) => {
                    merged_numbers.insert(&file.install_name, number);
                    files.paths.push(&file.path);
                }
            }
        }
        for (sym, _, file) in provided {
            if let Some(&number) = merged_numbers.get(file.install_name.as_slice()) {
                files.merged.insert(sym, number);
            }
        }
        for (i, dylib) in ctx.dylibs.iter().enumerate().filter(|(_, d)| is_moved(d)) {
            let path = files.paths.iter().position(|&p| *p == dylib.path);
            files.dylibs[i] = path.map_or(0, |p| p + 1);
        }
        files
    }

    /// The number of the file that defines a symbol: of a common
    /// symbol, the object whose tentative definition won.
    fn of_symbol<E: Target>(&self, ctx: &Context<E>, sym: SymbolId) -> usize {
        if let Some(&number) = self.merged.get(&sym) {
            return number;
        }
        let sym = &ctx.symbols[sym];
        match sym.file() {
            Some(FileId::Obj(i)) => match sym.input_section().and_then(|i| self.commons.get(&i)) {
                Some(&owner) => self.objs[owner as usize],
                None => self.of_object(i as usize, sym.name()),
            },
            Some(FileId::Dylib(i)) => self.dylibs.get(i as usize).copied().unwrap_or(0),
            None => 0,
        }
    }

    /// The number of the file a symbol of an object comes from: the
    /// object's own, or for the object LTO compiled, the bitcode file's.
    fn of_object(&self, obj: usize, name: &str) -> usize {
        match self.lto_origins.get(name) {
            Some(&Some(origin)) if self.lto_obj == Some(obj) => self.objs[origin],
            _ => self.objs[obj],
        }
    }
}

/// The merged library that provides each symbol bound to a dylib that
/// merged some, as (symbol, dylib, library), and of each dylib whether
/// all symbols bound to it are such.
fn merged_providers<E: Target>(
    ctx: &Context<E>,
) -> (Vec<(SymbolId, usize, &MergedFile)>, Vec<bool>) {
    let mut providers: Vec<Option<hashbrown::HashMap<&str, &MergedFile>>> =
        (0..ctx.dylibs.len()).map(|_| None).collect();
    let mut provided = Vec::new();
    let mut bound = vec![(false, false); ctx.dylibs.len()];
    for i in 0..ctx.symbols.syms.len() as SymbolId {
        let sym = &ctx.symbols[i];
        let Some(FileId::Dylib(d)) = sym.file() else { continue };
        let d = d as usize;
        let Some(dylib) = ctx.dylibs.get(d) else { continue };
        if dylib.merged_files.is_empty() {
            bound[d].1 = true;
            continue;
        }
        let by_name = providers[d].get_or_insert_with(|| {
            let mut map = hashbrown::HashMap::new();
            for file in &dylib.merged_files {
                for &name in &file.exports {
                    map.entry(name).or_insert(file);
                }
            }
            map
        });
        match by_name.get(sym.name()) {
            Some(&file) => {
                provided.push((i, d, file));
                bound[d].0 = true;
            }
            None => bound[d].1 = true,
        }
    }
    let merged_only = bound.into_iter().map(|(merged, own)| merged && !own).collect();
    (provided, merged_only)
}

/// A line of the map's section list.
struct MapSection<'a> {
    addr: u64,
    size: u64,
    segname: &'a str,
    sectname: &'a str,
}

impl<'a> MapSection<'a> {
    fn of(hdr: &'a crate::chunks::ChunkHeader) -> Self {
        Self { addr: hdr.addr, size: hdr.size, segname: hdr.segname, sectname: &hdr.sectname }
    }
}

pub fn print_map<E: Target>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.map else { return };
    let mut sections = Vec::new();
    for seg in &ctx.segments {
        for &id in &seg.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.is_sect {
                sections.push(MapSection::of(hdr));
            }
        }
        // ld-prime models a static executable's stack as a zero-fill
        // section of a linker-made atom, which its segment's load
        // command doesn't list.
        if seg.name == "__UNIXSTACK" {
            let (addr, size) = (seg.cmd.vmaddr, seg.cmd.vmsize);
            sections.push(MapSection { addr, size, segname: "__UNIXSTACK", sectname: "__stack" });
        }
    }

    // The linker's symbols go first of those at one place, and an
    // executable's header first of all.
    let files = MapFiles::new(ctx);
    let mut entries = linker_symbol_entries(ctx);
    let linker_symbols = entries.len();
    let (named, first_labels) = symbol_entries(ctx, &files);
    entries.extend(named);
    entries.extend(unnamed_entries(ctx, &files, &first_labels));
    entries.extend(eh_frame_entries(ctx, &files, &entries[linker_symbols..]));
    entries.extend(synthetic_entries(ctx, &files));
    entries.sort_by_key(|e| (e.addr, e.size));
    if ctx.args.output_type == MH_EXECUTE && !ctx.args.preload {
        let addr = ctx.mach_header.hdr.addr;
        entries.insert(0, MapEntry { addr, size: 0, file: 0, name: name("__mh_execute_header") });
    }
    write_map(ctx, path, &files, &sections, &entries, &dead_entries(ctx, &files));
}

/// An atom of a section a -r output makes itself, as its map lists it.
pub enum RelocatableAtom {
    /// A record of __LD,__compact_unwind: unwind record `i`'s.
    Unwind(usize),
    /// A record of __TEXT,__eh_frame: CIE or FDE `i`.
    Cie(usize),
    Fde(usize),
    /// The merged __objc_imageinfo.
    ImageInfo,
}

/// -map for a -r link, which ld-prime writes as for a final image: of
/// the output's `sections`, at the addresses they have from zero, and
/// the atoms in them - the inputs', and those of the sections the
/// output makes itself (`atoms`, by address). A record of
/// __compact_unwind is its object's, named by its label, if it has one;
/// the __objc_imageinfo is the linker's, as in an image.
pub fn print_relocatable_map<E: Target>(
    ctx: &Context<E>,
    sections: &[&crate::chunks::ChunkHeader],
    atoms: &[(u64, RelocatableAtom)],
) {
    let Some(path) = &ctx.args.map else { return };
    let sections: Vec<MapSection> = sections.iter().map(|hdr| MapSection::of(hdr)).collect();
    let files = MapFiles::new(ctx);
    let mut entries = linker_symbol_entries(ctx);
    let linker_symbols = entries.len();
    let (named, first_labels) = symbol_entries(ctx, &files);
    entries.extend(named);
    entries.extend(unnamed_entries(ctx, &files, &first_labels));
    let synthetic = relocatable_atom_entries(ctx, &files, &entries[linker_symbols..], atoms);
    entries.extend(synthetic);
    entries.sort_by_key(|e| (e.addr, e.size));
    write_map(ctx, path, &files, &sections, &entries, &[]);
}

/// The map's entries of the atoms of the sections a -r output makes
/// itself (see print_relocatable_map), whose FDEs are named after the
/// `named` symbols.
fn relocatable_atom_entries<E: Target>(
    ctx: &Context<E>,
    files: &MapFiles,
    named: &[MapEntry],
    atoms: &[(u64, RelocatableAtom)],
) -> Vec<MapEntry<'static>> {
    let fde_names = FdeNames::new(named);
    let labels = unwind_labels(ctx);
    let mut entries = Vec::new();
    for &(addr, ref atom) in atoms {
        let (size, obj, name) = match *atom {
            RelocatableAtom::Unwind(i) => {
                let rec = &ctx.unwind_records[i];
                let label = labels.get(&(rec.isec, rec.input_offset)).copied();
                let obj = ctx.isecs[rec.isec as usize].file as usize;
                (32, Some(obj), Cow::Borrowed(label.unwrap_or("anon").as_bytes()))
            }
            RelocatableAtom::Cie(i) => {
                let cie = &ctx.cies[i];
                (cie.data.len() as u64, Some(cie.obj as usize), name("CFI"))
            }
            RelocatableAtom::Fde(i) => {
                let fde = &ctx.fdes[i];
                let func = ctx.isec_addr(fde.isec as usize) + fde.func_offset as u64;
                (fde.data.len() as u64, Some(fde.obj as usize), fde_names.name(func))
            }
            RelocatableAtom::ImageInfo => (8, None, name("anon")),
        };
        let file = obj.map_or(0, |obj| files.objs[obj]);
        entries.push(MapEntry { addr, size, file, name });
    }
    entries
}

/// The label of each __compact_unwind record that has one, by the
/// record's subsection and function offset (see
/// ObjectFile::unwind_labels).
fn unwind_labels<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<(u32, u32), &'static str> {
    let mut labels = hashbrown::HashMap::new();
    for obj in ctx.objs.iter().filter(|obj| obj.is_alive) {
        for &(isec, off, k) in &obj.unwind_labels {
            let name = ctx.symbols[obj.symbols[k as usize]].name();
            if !name.is_empty() && !name.starts_with('L') {
                labels.entry((isec, off)).or_insert(name);
            }
        }
    }
    labels
}

/// Writes the map: the output and its architecture, the files of the
/// link (see MapFiles), the `sections`, the `entries` - the atoms, in
/// the order given - and the `dead` ones -dead_strip took out.
fn write_map<E: Target>(
    ctx: &Context<E>,
    path: &Path,
    files: &MapFiles,
    sections: &[MapSection],
    entries: &[MapEntry],
    dead: &[MapEntry],
) {
    let Ok(file) = std::fs::File::create(path) else {
        crate::warn!("could not write map file: {}", path.display());
        return;
    };
    let mut out = std::io::BufWriter::new(file);

    let _ = write!(out, "# Path: ");
    let _ = out.write_all(path_bytes(&ctx.args.output));
    let _ = writeln!(out);
    let _ = writeln!(out, "# Arch: {}", E::NAME);

    let _ = writeln!(out, "# Object files:");
    let _ = writeln!(out, "[  0] linker synthesized");
    // A fat file's slice, and a fat archive's member, by the file's own
    // path: "libfoo.a(foo.o)".
    for (i, path) in files.paths.iter().enumerate() {
        let _ = write!(out, "[{:3}] ", i + 1);
        let _ = out.write_all(&crate::input_files::without_fat_arch(path_bytes(path)));
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "# Sections:");
    let _ = writeln!(out, "# Address\tSize    \tSegment\tSection");
    for sec in sections {
        let _ = writeln!(
            out,
            "0x{:08X}\t0x{:08X}\t{}\t{}",
            sec.addr, sec.size, sec.segname, sec.sectname
        );
    }

    let _ = writeln!(out, "# Symbols:");
    let _ = writeln!(out, "# Address\tSize    \tFile  Name");
    for e in entries {
        let _ = write!(out, "0x{:08X}\t0x{:08X}\t[{:3}] ", e.addr, e.size, e.file);
        let _ = out.write_all(&e.name);
        let _ = writeln!(out);
    }

    // Symbols removed by -dead_strip appear in their own section
    // with "<<dead>>" in the address column, the way ld64 reports
    // them; sizes are the atom extents they would have had.
    if !dead.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "# Dead Stripped Symbols:");
        let _ = writeln!(out, "#        \tSize    \tFile  Name");
        for e in dead {
            let _ = write!(out, "<<dead>>\t0x{:08X}\t[{:3}] ", e.size, e.file);
            let _ = out.write_all(&e.name);
            let _ = writeln!(out);
        }
    }
}

/// Whether a symbol names its atom in the map, as ld-prime names atoms.
/// An assembler temporary (L...) doesn't, nor does a linker-private
/// label (l...) of a fixed-size literal, such as the compiler's lCPI0_0
/// constant-pool labels: the literal is known by its size, as a C
/// string is by its contents whatever labels it. The atoms of the
/// sections ld-prime reads as lists of records - CFStrings, UTF-16
/// strings, selector and class references, Objective-C class and
/// category lists - are named by no local symbol: they are "anon", as
/// unnamed atoms are. (An ltmpN label may be shadowed besides; see
/// drop_shadowed_ltmps.)
fn is_named<E: Target>(ctx: &Context<E>, sym: &crate::symbol::Symbol) -> bool {
    let isec = &ctx.isecs[ctx.resolve_isec(sym.input_section().unwrap() as usize)];
    let split = ctx.objs[isec.file as usize].subsections_via_symbols;
    names_atom(ctx.hdr_of(isec), split, sym.is_extern(), sym.name())
}

/// is_named for a label of a section with header `hdr`, of an object
/// with subsections or not (`split`).
fn names_atom(hdr: &MachSection, split: bool, is_extern: bool, name: &str) -> bool {
    if name.is_empty()
        || hdr.section_type() == S_CSTRING_LITERALS
        || crate::input_files::is_ignored_literal_label(hdr.section_type(), name)
    {
        return false;
    }
    is_extern || (!crate::input_files::is_record_list(hdr, split) && !name.starts_with('L'))
}

/// Drops from the map's named symbols each ltmpN label another of them
/// shares a place with. An arm64 assembler puts the label where each
/// section starts; where symbols split the sections, an atom there
/// takes the label's name only if it has no other, as ld-prime ranks
/// the labels an atom may be named after (without subsections the
/// section's first atom has its name, which the other symbols there
/// alias).
fn drop_shadowed_ltmps<E: Target, T>(
    ctx: &Context<E>,
    syms: &mut Vec<T>,
    id: impl Fn(&T) -> SymbolId,
) {
    let is_ltmp = |sym: SymbolId| {
        let sym = &ctx.symbols[sym];
        matches!(sym.file(), Some(FileId::Obj(obj)) if ctx.objs[obj as usize].subsections_via_symbols)
            && sym.name().starts_with("ltmp")
    };
    let place = |sym: SymbolId| (ctx.symbols[sym].input_section(), ctx.symbols[sym].value);
    let ltmps: hashbrown::HashSet<_> =
        syms.iter().map(&id).filter(|&sym| is_ltmp(sym)).map(place).collect();
    if ltmps.is_empty() {
        return;
    }
    let shadowed: hashbrown::HashSet<_> = syms
        .iter()
        .map(&id)
        .filter(|&sym| !is_ltmp(sym))
        .map(place)
        .filter(|at| ltmps.contains(at))
        .collect();
    syms.retain(|t| !is_ltmp(id(t)) || !shadowed.contains(&place(id(t))));
}

/// How a symbol ranks to name its atom among the symbols at its place,
/// lowest first: as ld-prime ranks the labels at an atom's start (see
/// atom_name_rank) - _zb names the atom of `_zb: _ab: lc:`, _loc5 that
/// of a weak definition _wd it labels too -, but in an object without
/// subsections, where the first in the symbol table names it (an ltmpN
/// label before an exported function).
fn naming_rank<E: Target>(
    ctx: &Context<E>,
    nlists: &hashbrown::HashMap<SymbolId, (u32, bool)>,
    sym: SymbolId,
) -> LabelRank {
    match (nlists.get(&sym), ctx.symbols[sym].file()) {
        (Some(&(k, _)), Some(FileId::Obj(obj))) => {
            label_rank(&ctx.objs[obj as usize], k, ctx.symbols[sym].name())
        }
        _ => (0, "", std::cmp::Reverse(0)),
    }
}

/// See naming_rank: the rank of nlist `k` of an object, named `name`.
type LabelRank = (u8, &'static str, std::cmp::Reverse<u32>);

fn label_rank(obj: &crate::input_files::ObjectFile, k: u32, name: &'static str) -> LabelRank {
    use crate::input_files::atom_name_rank;
    match obj.subsections_via_symbols {
        true => (atom_name_rank(&obj.nlists[k as usize], name), name, std::cmp::Reverse(k)),
        false => (0, "", std::cmp::Reverse(k)),
    }
}

/// The nlist of each defined symbol in its object's symbol table: its
/// index, and whether it is an alternate entry point (N_ALT_ENTRY).
fn defining_nlists<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<SymbolId, (u32, bool)> {
    let mut nlists = hashbrown::HashMap::new();
    for (i, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        for (k, (nlist, &sym)) in obj.nlists.iter().zip(&obj.symbols).enumerate() {
            if ctx.symbols[sym].file() == Some(FileId::Obj(i as u32)) {
                nlists.entry(sym).or_insert((k as u32, nlist.n_desc & N_ALT_ENTRY != 0));
            }
        }
    }
    nlists
}

/// Defined symbols with their addresses, sizes and owning objects. A
/// symbol's size is the span to the next symbol in its subsection (or
/// the subsection's end) - the same atom size ld64 reports; of several
/// at one place, the one naming the atom has the size (see
/// naming_rank) and the others are aliases of none, as is an alternate
/// entry point (Swift's type metadata inside its full metadata).
/// ld-prime counts a thread-local variable's descriptor, which it
/// rewrites, as its own, and a common symbol as the object's whose
/// tentative definition won. It credits itself with an atom it rewrote
/// (a method list in the relative form), and with the alias it makes
/// of a function folded into an identical one (-deduplicate), which
/// has no size; an ltmpN label of a weak definition another file's won
/// names nothing. Also returns where in its subsection the first
/// symbol is, by subsection.
fn symbol_entries<'a, E: Target>(
    ctx: &'a Context<E>,
    files: &MapFiles,
) -> (Vec<MapEntry<'a>>, hashbrown::HashMap<usize, u64>) {
    use crate::chunks::symtab::is_coalesced_away;
    let nlists = defining_nlists(ctx);
    let mut syms: Vec<(SymbolId, usize)> = Vec::new();
    for i in 0..ctx.symbols.syms.len() as SymbolId {
        let sym = &ctx.symbols[i];
        let (Some(FileId::Obj(obj)), Some(own)) = (sym.file(), sym.input_section()) else {
            continue;
        };
        let isec = ctx.resolve_isec(own as usize);
        if !ctx.isecs[isec].is_alive() || !is_named(ctx, sym) {
            continue;
        }
        // Of the name of the function a folded one folded into, ld-prime
        // lists one of each scope (see icf::folded_atom_names).
        let folded = is_coalesced_away(ctx, own as usize);
        if folded
            && (sym.name().starts_with("ltmp") || ctx.folded_atom_names.get(&i) == Some(&true))
        {
            continue;
        }
        let file = match files.commons.get(&(isec as u32)) {
            _ if folded => 0,
            _ if ctx.hdr_of(&ctx.isecs[isec]).section_type() == S_THREAD_LOCAL_VARIABLES => 0,
            Some(&owner) => files.objs[owner as usize],
            None if is_rewritten_method_list(ctx, isec) => 0,
            None => files.of_object(obj as usize, sym.name()),
        };
        syms.push((i, file));
    }
    drop_shadowed_ltmps(ctx, &mut syms, |&(sym, _)| sym);

    // Sizes: sort the symbols by place, the one naming the atom last
    // of those at one, and measure to the next place.
    let key = |sym: SymbolId| {
        let isec = ctx.resolve_isec(ctx.symbols[sym].input_section().unwrap() as usize);
        (isec, ctx.symbols[sym].value, naming_rank(ctx, &nlists, sym))
    };
    let is_alias = |sym: SymbolId| {
        nlists.get(&sym).is_some_and(|&(_, alt)| alt)
            || is_coalesced_away(ctx, ctx.symbols[sym].input_section().unwrap() as usize)
    };
    let mut order: Vec<usize> = (0..syms.len()).filter(|&i| !is_alias(syms[i].0)).collect();
    order.sort_by_key(|&i| key(syms[i].0));
    let mut sizes = vec![0u64; syms.len()];
    for (i, &idx) in order.iter().enumerate() {
        let (isec, value, _) = key(syms[idx].0);
        let end = match order.get(i + 1).map(|&next| key(syms[next].0)) {
            Some((next_isec, next_value, _)) if next_isec == isec => next_value,
            _ => ctx.isecs[isec].size as u64,
        };
        sizes[idx] = end.saturating_sub(value);
    }

    let mut first_labels: hashbrown::HashMap<usize, u64> = hashbrown::HashMap::new();
    for &(sym, _) in &syms {
        let (isec, value, _) = key(sym);
        let first = first_labels.entry(isec).or_insert(value);
        *first = (*first).min(value);
    }
    let entries = syms
        .iter()
        .zip(sizes)
        .map(|(&(sym, file), size)| MapEntry {
            addr: ctx.sym_addr(sym),
            size,
            file,
            name: name(ctx.symbols[sym].name()),
        })
        .collect();
    (entries, first_labels)
}

/// Whether a subsection is an Objective-C method list the linker
/// rewrote in the relative form.
fn is_rewritten_method_list<E: Target>(ctx: &Context<E>, isec: usize) -> bool {
    let isec = &ctx.isecs[isec];
    ctx.is_internal(isec.file as usize) && ctx.hdr_of(isec).sectname() == "__objc_methlist"
}

/// What ld-prime calls a literal no symbol names, wherever it ends up
/// (fixed-size literals go to __const): a C string by its contents
/// ("literal string: " and the string's bytes, its newlines, tabs,
/// carriage returns and quotes escaped) - named or not -, a fixed-size
/// literal by its size.
fn literal_name<E: Target>(
    ctx: &Context<E>,
    isec: &crate::input_sections::InputSection,
) -> Option<Cow<'static, [u8]>> {
    match ctx.hdr_of(isec).section_type() {
        S_CSTRING_LITERALS => {
            let mut name = b"literal string: ".to_vec();
            escape_literal(&mut name, isec.data());
            Some(Cow::Owned(name))
        }
        S_4BYTE_LITERALS => Some(Cow::Borrowed(b"4-byte-literal")),
        S_8BYTE_LITERALS => Some(Cow::Borrowed(b"8-byte-literal")),
        S_16BYTE_LITERALS => Some(Cow::Borrowed(b"16-byte-literal")),
        _ => None,
    }
}

fn escape_literal(out: &mut Vec<u8>, data: &[u8]) {
    let s = data.strip_suffix(b"\0").unwrap_or(data);
    for &c in s {
        match c {
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'"' => out.extend_from_slice(b"\\\""),
            _ => out.push(c),
        }
    }
}

/// The rows of the start of subsection `id` no symbol names, its first
/// `len` bytes, by offset in it: a literal by what it is, any other
/// "anon" - one per record of a section of fixed-size records, of which
/// ld-prime makes each an atom of its own (an __objc_classlist listing
/// two classes is two).
fn unnamed_rows<E: Target>(
    ctx: &Context<E>,
    id: usize,
    len: u64,
) -> impl Iterator<Item = (u64, u64, Cow<'static, [u8]>)> {
    let isec = &ctx.isecs[id];
    let literal = literal_name(ctx, isec);
    let record = match literal {
        Some(_) => None,
        None => crate::input_files::record_size(ctx.hdr_of(isec)),
    };
    let step = record.unwrap_or(len).max(1);
    (0..len).step_by(step as usize).map(move |off| {
        let name = literal.clone().unwrap_or(Cow::Borrowed(b"anon"));
        (off, step.min(len - off), name)
    })
}

/// The atoms of the input files no symbol names, up to the first
/// symbol in one (`first_labels` has where it is, by subsection). The
/// selector names of Objective-C stubs are file 0's: ld-prime makes
/// them itself, and an input's copy merges into its own.
fn unnamed_entries<'a, E: Target>(
    ctx: &'a Context<E>,
    files: &MapFiles,
    first_labels: &hashbrown::HashMap<usize, u64>,
) -> Vec<MapEntry<'a>> {
    let stub_names = stub_name_isecs(ctx);
    let mut entries = Vec::new();
    for osec in &ctx.output_sections {
        for &id in &osec.members {
            let id = id as usize;
            let isec = &ctx.isecs[id];
            if ctx.is_internal(isec.file as usize) {
                continue;
            }
            let file = if stub_names.contains(&id) { 0 } else { files.objs[isec.file as usize] };
            let len = first_labels.get(&id).copied().unwrap_or(isec.size as u64);
            let addr = ctx.isec_addr(id);
            for (off, size, name) in unnamed_rows(ctx, id, len) {
                entries.push(MapEntry { addr: addr + off, size, file, name });
            }
        }
    }
    // The selector names synthesized for Objective-C stubs.
    for (i, (_, sel)) in ctx.objc_stubs.symbols.iter().enumerate() {
        if ctx.objc_stubs.name_isec[i] == u32::MAX {
            let name = Cow::Owned(format!("literal string: {sel}").into_bytes());
            let size = sel.len() as u64 + 1;
            entries.push(MapEntry { addr: ctx.objc_methname_addr(i), size, file: 0, name });
        }
    }
    entries.extend(objc_list_entries(ctx, files));
    entries
}

/// The input C strings Objective-C stubs take their selector names
/// from, which the map credits to file 0.
fn stub_name_isecs<E: Target>(ctx: &Context<E>) -> hashbrown::HashSet<usize> {
    (ctx.objc_stubs.name_isec.iter())
        .filter(|&&isec| isec != u32::MAX)
        .map(|&isec| ctx.resolve_isec(isec as usize))
        .collect()
}

/// The entries of the Objective-C lists the linker writes itself: a
/// category list rebuilt without the categories merged into their
/// classes lists the others, each its category's file's, as ld-prime
/// keeps those entries; an __objc_nlclslist entry for a class that a
/// category's +load made non-lazy is file 0's.
fn objc_list_entries<'a, E: Target>(ctx: &'a Context<E>, files: &MapFiles) -> Vec<MapEntry<'a>> {
    use crate::objc::{DataField, ObjcRef};
    let mut entries = Vec::new();
    for blob in &ctx.data_blobs {
        let of_categories = matches!(blob.sect, "__objc_catlist" | "__objc_nlcatlist");
        if !of_categories && blob.sect != "__objc_nlclslist" {
            continue;
        }
        let addr = ctx.isec_addr(blob.isec as usize);
        for (i, field) in blob.fields.iter().enumerate() {
            let file = match field {
                DataField::Ptr(ObjcRef::Isec(isec, _)) if of_categories => {
                    let file = ctx.isecs[*isec as usize].file as usize;
                    if ctx.is_internal(file) { 0 } else { files.objs[file] }
                }
                DataField::Ptr(ObjcRef::Sym(sym, _)) if of_categories => files.of_symbol(ctx, *sym),
                _ => 0,
            };
            let addr = addr + i as u64 * 8;
            entries.push(MapEntry { addr, size: 8, file, name: name("anon") });
        }
    }
    entries
}

/// The records of the __eh_frame the linker writes, each credited to
/// the object it came from: a CIE is "CFI", an FDE "FDE for: " and the
/// name of the function's atom.
fn eh_frame_entries<'a, E: Target>(
    ctx: &'a Context<E>,
    files: &MapFiles,
    named: &[MapEntry<'a>],
) -> Vec<MapEntry<'a>> {
    if !ctx.chunks.contains(&ChunkId::EhFrame) {
        return Vec::new();
    }
    let fde_names = FdeNames::new(named);
    let base = ctx.eh_frame.hdr.addr;
    let mut entries = Vec::new();
    for cie in ctx.cies.iter().filter(|cie| cie.is_alive) {
        let addr = base + cie.output_offset as u64;
        let (size, file) = (cie.data.len() as u64, files.objs[cie.obj as usize]);
        entries.push(MapEntry { addr, size, file, name: name("CFI") });
    }
    for fde in &ctx.fdes {
        let func = ctx.isec_addr(fde.isec as usize) + fde.func_offset as u64;
        let addr = base + fde.output_offset as u64;
        let (size, file) = (fde.data.len() as u64, files.objs[fde.obj as usize]);
        entries.push(MapEntry { addr, size, file, name: fde_names.name(func) });
    }
    entries
}

/// How the map names an FDE: "FDE for: " and the name of the atom of
/// its function, which of the `named` symbols at its place is listed
/// first (has the size).
struct FdeNames<'a> {
    atoms: hashbrown::HashMap<u64, (u64, &'a [u8])>,
}

impl<'a> FdeNames<'a> {
    fn new(named: &'a [MapEntry<'_>]) -> Self {
        let mut atoms: hashbrown::HashMap<u64, (u64, &[u8])> = hashbrown::HashMap::new();
        for e in named {
            let atom = atoms.entry(e.addr).or_insert((e.size, &e.name));
            if e.size < atom.0 {
                *atom = (e.size, &e.name);
            }
        }
        Self { atoms }
    }

    fn name(&self, func: u64) -> Cow<'static, [u8]> {
        let func = self.atoms.get(&func).map_or(&b"anon"[..], |&(_, name)| name);
        let mut name = b"FDE for: ".to_vec();
        name.extend_from_slice(func);
        Cow::Owned(name)
    }
}

/// The symbols the linker defines, as ld-prime lists them once
/// something refers to them, file 0's and of no size: ___dso_handle and
/// the header's name for the kind of image (a dylib's
/// __mh_dylib_header) - the lazy-load helpers pass ___dso_handle to
/// __dyld_lazy_load -, and the bounds of sections
/// (section$start$__TEXT$__text), but not of segments. An executable's
/// __mh_execute_header comes first whether or not anything does. A
/// section -sectcreate or -add_empty_section makes is an atom named
/// "l<sect-create>" and the section's name, one a boundary symbol makes
/// by the name alone.
fn linker_symbol_entries<'a, E: Target>(ctx: &'a Context<E>) -> Vec<MapEntry<'a>> {
    let headers =
        ["___dso_handle", "__mh_dylib_header", "__mh_bundle_header", "__mh_dylinker_header"];
    let mut ids: Vec<SymbolId> = headers
        .iter()
        .filter_map(|&name| ctx.symbols.get(name))
        .filter(|&id| {
            let sym = &ctx.symbols[id];
            let is_ours = matches!(sym.file(), Some(FileId::Obj(obj)) if ctx.is_internal(obj as usize))
                && sym.input_section().is_none();
            let lazy_load = sym.name() == "___dso_handle" && !ctx.lazy_helpers.helpers.is_empty();
            is_ours && (sym.is_used() || lazy_load)
        })
        .collect();
    ids.extend(ctx.boundary_syms.iter().filter(|(.., sect)| sect.is_some()).map(|&(id, ..)| id));
    let mut entries: Vec<MapEntry> = ids
        .into_iter()
        .map(|id| MapEntry {
            addr: ctx.sym_addr(id),
            size: 0,
            file: 0,
            name: name(ctx.symbols[id].name()),
        })
        .collect();
    for sec in &ctx.sectcreate_sections {
        let (hdr, size) = (&sec.hdr, sec.contents.len() as u64);
        let prefix = if sec.from_option { "l<sect-create>" } else { "" };
        let name = format!("{prefix}{},{}", hdr.segname, hdr.sectname).into_bytes();
        entries.push(MapEntry { addr: hdr.addr, size, file: 0, name: Cow::Owned(name) });
    }
    entries
}

/// The atoms the linker makes, file 0's but for the stubs and pointers
/// it makes for a symbol: a stub, a GOT slot or a lazy pointer counts as
/// the file that defines the symbol, and takes its name with ".stub",
/// ".got" or ".lazy_ptr" after it. The stub helper's entries are
/// anonymous.
fn synthetic_entries<'a, E: Target>(ctx: &'a Context<E>, files: &MapFiles) -> Vec<MapEntry<'a>> {
    let mut entries = Vec::new();
    let slot = |sym: SymbolId, addr: u64, size: u64, suffix: &str| MapEntry {
        addr,
        size,
        file: files.of_symbol(ctx, sym),
        name: Cow::Owned(format!("{}{suffix}", ctx.symbols[sym].name()).into_bytes()),
    };
    let anon = |addr: u64, size: u64| MapEntry { addr, size, file: 0, name: name("anon") };

    let stubs = &ctx.stubs;
    for (i, &sym) in stubs.symbols.iter().enumerate() {
        entries.push(slot(sym, stubs.hdr.addr + i as u64 * E::STUB_SIZE, E::STUB_SIZE, ".stub"));
    }
    for (i, &sym) in ctx.got.got_syms.iter().enumerate() {
        entries.push(slot(sym, ctx.got.slot_addr(i), 8, ".got"));
    }
    for (i, &stub) in stubs.lazy.iter().enumerate() {
        let sym = stubs.symbols[stub as usize];
        entries.push(slot(sym, ctx.lazy_ptrs.hdr.addr + i as u64 * 8, 8, ".lazy_ptr"));
    }
    if !stubs.lazy.is_empty() {
        let helper = ctx.stub_helper.hdr.addr;
        entries.push(anon(helper, E::STUB_HELPER_HEADER_SIZE));
        for i in 0..stubs.lazy.len() as u64 {
            let addr = helper + E::STUB_HELPER_HEADER_SIZE + i * E::STUB_HELPER_ENTRY_SIZE;
            entries.push(anon(addr, E::STUB_HELPER_ENTRY_SIZE - E::STUB_HELPER_ENTRY_PADDING));
        }
    }

    let objc_stubs = &ctx.objc_stubs;
    for (i, &(sym, _)) in objc_stubs.symbols.iter().enumerate() {
        let addr = objc_stubs.hdr.addr + i as u64 * ctx.objc_stub_size();
        let name = name(ctx.symbols[sym].name());
        entries.push(MapEntry { addr, size: ctx.objc_stub_size(), file: 0, name });
    }
    if objc_stubs.selrefs.is_some() {
        for i in 0..objc_stubs.symbols.len() + objc_stubs.extra_selrefs.len() {
            entries.push(anon(ctx.objc_selref_addr(i), 8));
        }
    }

    // The lazy-load helpers are file 0's, as is the empty atom that
    // keeps __dyld_lazy_load alive, and each slot its symbol's file's.
    if ctx.lazy_helpers.keep_alive != u32::MAX {
        entries.push(anon(ctx.isec_addr(ctx.lazy_helpers.keep_alive as usize), 0));
    }
    for (i, helper) in ctx.lazy_helpers.helpers.iter().enumerate() {
        let (addr, size) = (ctx.lazy_helper_addr(i), E::lazy_helper_size(helper.kind) as u64);
        entries.push(MapEntry { addr, size, file: 0, name: name(helper.name) });
    }
    for (i, &(sym, slot)) in ctx.lazy_load_got.slots.iter().enumerate() {
        let (addr, file) = (ctx.lazy_load_got.slot_addr(i as u32), files.of_symbol(ctx, sym));
        entries.push(MapEntry { addr, size: 8, file, name: name(slot) });
    }
    // The delay-init stubs and load helpers count as their symbols'
    // files, the dlopen helpers and their C strings as file 0.
    let delay = &ctx.delay_init;
    for (i, stub) in delay.stubs.iter().enumerate() {
        let (addr, file) = (ctx.delay_stub_addr(i), files.of_symbol(ctx, stub.sym));
        entries.push(MapEntry { addr, size: E::DELAY_STUB_SIZE, file, name: name(stub.name) });
    }
    for (i, h) in delay.helpers.iter().enumerate() {
        let (addr, size) = (ctx.delay_helper_addr(i), E::delay_helper_size(h.kind) as u64);
        let file = files.of_symbol(ctx, h.sym);
        entries.push(MapEntry { addr, size, file, name: name(h.name) });
    }
    for (i, d) in delay.dlopens.iter().enumerate() {
        let (addr, size) = (ctx.dlopen_helper_addr(i), E::DLOPEN_HELPER_SIZE as u64);
        entries.push(MapEntry { addr, size, file: 0, name: name(d.name) });
        let string = &ctx.isecs[d.string as usize];
        let mut label = b"literal string: ".to_vec();
        escape_literal(&mut label, string.data());
        let (addr, size) = (ctx.isec_addr(d.string as usize), string.size as u64);
        entries.push(MapEntry { addr, size, file: 0, name: Cow::Owned(label) });
    }
    for &(sym, isec) in &ctx.extra_local_syms {
        let size = ctx.isecs[isec as usize].size as u64;
        let addr = ctx.isec_addr(isec as usize);
        entries.push(MapEntry { addr, size, file: 0, name: name(sym) });
    }
    // The branch islands, as the symbol table names them.
    for (addr, _, island) in crate::thunks::island_symbols(ctx) {
        let name = Cow::Borrowed(island);
        entries.push(MapEntry { addr, size: E::THUNK_SIZE, file: 0, name });
    }
    if ctx.chunks.contains(&ChunkId::UnwindInfo) {
        let hdr = &ctx.unwind_info.hdr;
        let name = name("compact unwind info");
        entries.push(MapEntry { addr: hdr.addr, size: hdr.size, file: 0, name });
    }
    let init_offsets = &ctx.init_offsets;
    for i in 0..init_offsets.init_funcs.len() as u64 {
        let addr = init_offsets.hdr.addr + i * 4;
        entries.push(MapEntry { addr, size: 4, file: 0, name: name("init-offset") });
    }
    if ctx.chunks.contains(&ChunkId::ObjcImageInfo) {
        entries.push(anon(ctx.objc_imageinfo.hdr.addr, ctx.objc_imageinfo.hdr.size));
    }
    if let Some(stack) = ctx.segments.iter().find(|seg| seg.name == "__UNIXSTACK") {
        let (addr, size) = (stack.cmd.vmaddr, stack.cmd.vmsize);
        entries.push(MapEntry { addr, size, file: 0, name: name("l__unixstack") });
    }
    entries
}

/// The atoms of the input files that the output doesn't have, which
/// ld-prime lists under -dead_strip, whatever took them out: dead
/// stripping, coalescing - a literal or an Objective-C reference equal
/// to another file's, a weak definition another file's won, a function
/// folded into an identical one, a tentative definition (a common
/// symbol) a larger or a real one took the place of - or an
/// Objective-C rewrite (a method list in the relative form, a category
/// merged into its class). They come by the file they came from, in
/// the order they were in it, its tentative definitions last; the
/// sizes are the atoms', as for the live ones (see dead_entries_of).
fn dead_entries<'a, E: Target>(ctx: &'a Context<E>, files: &MapFiles) -> Vec<MapEntry<'a>> {
    use rayon::prelude::*;
    if !ctx.args.dead_strip {
        return Vec::new();
    }
    let gone = GoneAtoms::new(ctx);
    let mut dead: Vec<(usize, DeadKey, MapEntry)> = (0..ctx.objs.len())
        .into_par_iter()
        .filter(|&i| ctx.objs[i].is_alive && !ctx.is_internal(i))
        .flat_map_iter(|i| {
            let file = files.objs[i];
            dead_entries_of(ctx, &gone, &files.commons, i, file)
                .into_iter()
                .map(move |(key, entry)| (file, key, entry))
        })
        .collect();
    dead.sort_by_key(|&(file, key, _)| (file, key));
    dead.into_iter().map(|(_, _, entry)| entry).collect()
}

/// Where a dead atom was in its file: its address in the object, the
/// subsection and the row's place among those of the subsection.
type DeadKey = (u64, u32, u32);

/// Which input subsections the output doesn't have (see dead_entries):
/// those dead stripping or an Objective-C rewrite took out and those
/// another took the place of, but for a class's ro data the merged
/// record stands for (ld-prime rewrites it in place); and the C strings
/// objc stubs take their selector names from, for which ld-prime makes
/// its own. A section the link consumes, or that
/// -remove_swift_reflection_metadata_sections drops as ld-prime reads
/// it, has no atoms.
struct GoneAtoms {
    /// The records the Objective-C passes wrote for input ones.
    rewritten: hashbrown::HashSet<u32>,
    stub_names: hashbrown::HashSet<usize>,
}

impl GoneAtoms {
    fn new<E: Target>(ctx: &Context<E>) -> Self {
        let rewritten = ctx.data_blobs.iter().map(|blob| blob.isec).collect();
        Self { rewritten, stub_names: stub_name_isecs(ctx) }
    }

    fn contains<E: Target>(&self, ctx: &Context<E>, id: usize) -> bool {
        let isec = &ctx.isecs[id];
        let hdr = ctx.hdr_of(isec);
        if crate::output_sections::is_consumed_in_image(hdr.segname(), hdr.sectname())
            || (ctx.args.remove_swift_reflection_metadata_sections
                && crate::passes::is_swift_reflection_section(hdr))
        {
            return false;
        }
        if isec.replacement != crate::input_sections::NO_REPLACEMENT {
            return !self.rewritten.contains(&isec.replacement);
        }
        !isec.is_alive() || self.stub_names.contains(&id)
    }
}

/// A label of a gone atom (see dead_entries_of).
struct DeadLabel {
    isec: usize,
    off: u64,
    /// How the label ranks to name the atom, the best first.
    rank: std::cmp::Reverse<LabelRank>,
    name: &'static str,
    /// An alternate entry point (N_ALT_ENTRY), an alias of none.
    alt: bool,
}

/// The labels naming the gone atoms of an object (see dead_entries_of),
/// by subsection, the alternate entry points last, and by place, the
/// one naming the atom first; and how many labels each C string has at
/// its start. An ltmpN label of an empty section names nothing (the
/// section has no atom), nor does one of a C string in an object with
/// subsections.
fn gone_labels<E: Target>(
    ctx: &Context<E>,
    gone: &GoneAtoms,
    obj: &crate::input_files::ObjectFile,
) -> (Vec<DeadLabel>, hashbrown::HashMap<usize, u32>) {
    let split = obj.subsections_via_symbols;
    let is_ltmp = |name: &str| split && name.starts_with("ltmp");
    let mut labels = Vec::new();
    let mut cstring_labels: hashbrown::HashMap<usize, u32> = hashbrown::HashMap::new();
    for (k, nlist) in obj.nlists.iter().enumerate() {
        if nlist.is_stab() || nlist.n_type() != N_SECT {
            continue;
        }
        let Some((isec, off)) = crate::input_files::find_symbol_subsec(
            &ctx.isecs,
            &obj.subsecs,
            nlist.n_sect,
            nlist.n_value,
        ) else {
            continue;
        };
        let name = ctx.symbols[obj.symbols[k]].name();
        let hdr = ctx.hdr_of(&ctx.isecs[isec]);
        if hdr.section_type() == S_CSTRING_LITERALS && off == 0 && !is_ltmp(name) {
            *cstring_labels.entry(isec).or_default() += 1;
        }
        if !gone.contains(ctx, isec)
            || !names_atom(hdr, split, nlist.is_extern(), name)
            || (is_ltmp(name) && hdr.size == 0)
        {
            continue;
        }
        let rank = std::cmp::Reverse(label_rank(obj, k as u32, name));
        let alt = nlist.n_desc & N_ALT_ENTRY != 0;
        labels.push(DeadLabel { isec, off, rank, name, alt });
    }
    labels.sort_unstable_by_key(|l| (l.isec, l.alt, l.off, l.rank));
    (labels, cstring_labels)
}

/// The dead atoms of object `obj_idx` (see dead_entries), the `file`th
/// of the map, keyed by where they were. An atom is named by the best
/// of the labels at its start, as ld-prime ranks them (see
/// atom_name_rank) - in an object without subsections, by the first
/// in its symbol table -, which has the atom's size; the others are
/// aliases of none, but for linker-private ones (l...), which the list
/// leaves out. ld-prime makes a C string an atom per label at its
/// start, and all but one of them are always dead, merged into that
/// one.
fn dead_entries_of<'a, E: Target>(
    ctx: &'a Context<E>,
    gone: &GoneAtoms,
    commons: &hashbrown::HashMap<u32, u32>,
    obj_idx: usize,
    file: usize,
) -> Vec<(DeadKey, MapEntry<'a>)> {
    let obj = &ctx.objs[obj_idx];
    let (labels, cstring_labels) = gone_labels(ctx, gone, obj);
    let mut rows: Vec<(usize, u64, u64, Cow<'a, [u8]>)> = Vec::new();
    let mut first_label: hashbrown::HashMap<usize, u64> = hashbrown::HashMap::new();
    for (i, l) in labels.iter().enumerate() {
        let first = first_label.entry(l.isec).or_insert(l.off);
        *first = (*first).min(l.off);
        let prev = i.checked_sub(1).map(|p| (labels[p].isec, labels[p].off));
        let is_alias = l.alt || prev == Some((l.isec, l.off));
        if is_alias && l.name.starts_with('l') {
            continue;
        }
        let size = match is_alias {
            true => 0,
            false => {
                let next = labels[i + 1..]
                    .iter()
                    .take_while(|next| next.isec == l.isec && !next.alt)
                    .find(|next| next.off != l.off)
                    .map_or(ctx.isecs[l.isec].size as u64, |next| next.off);
                next - l.off
            }
        };
        rows.push((l.isec, l.off, size, Cow::Borrowed(l.name.as_bytes())));
    }

    // The unnamed atoms, from a gone subsection's start up to its first
    // label, and the C strings' extra atoms.
    for &id in &obj.subsecs {
        let id = id as usize;
        if gone.contains(ctx, id) {
            let len = first_label.get(&id).copied().unwrap_or(ctx.isecs[id].size as u64);
            for (off, size, name) in unnamed_rows(ctx, id, len) {
                if !is_kept_category_entry(ctx, id, off) {
                    rows.push((id, off, size, name));
                }
            }
        }
        if let Some(&n) = cstring_labels.get(&id) {
            let name = literal_name(ctx, &ctx.isecs[id]).unwrap();
            for _ in 1..n {
                rows.push((id, 0, ctx.isecs[id].size as u64, name.clone()));
            }
        }
    }

    let mut entries: Vec<(DeadKey, MapEntry)> = rows
        .into_iter()
        .enumerate()
        .map(|(seq, (isec, off, size, name))| {
            let key = (u64::from(ctx.isecs[isec].input_addr) + off, isec as u32, seq as u32);
            (key, MapEntry { addr: 0, size, file, name })
        })
        .collect();
    entries.extend(dead_commons(ctx, commons, obj_idx, file));
    entries
}

/// An object's tentative definitions that the output lacks, after its
/// other atoms (see dead_entries_of): all but the one whose common
/// symbol the output has (see MapFiles::commons), of the sizes they
/// give.
fn dead_commons<'a, E: Target>(
    ctx: &'a Context<E>,
    commons: &hashbrown::HashMap<u32, u32>,
    obj_idx: usize,
    file: usize,
) -> Vec<(DeadKey, MapEntry<'a>)> {
    let obj = &ctx.objs[obj_idx];
    let r = obj.global_range();
    let mut entries = Vec::new();
    for (k, (nlist, &sym)) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]).enumerate() {
        let won = || {
            ctx.symbols[sym].input_section().is_some_and(|isec| {
                commons.get(&isec) == Some(&(obj_idx as u32)) && ctx.isecs[isec as usize].is_alive()
            })
        };
        if nlist.is_common() && !won() {
            let name = Cow::Borrowed(ctx.symbols[sym].name().as_bytes());
            let entry = MapEntry { addr: 0, size: nlist.n_value, file, name };
            entries.push(((u64::MAX, u32::MAX, k as u32), entry));
        }
    }
    entries
}

/// Whether the 8-byte record at `off` of a category list the
/// Objective-C passes rebuilt names a category that wasn't merged into
/// its class, which the rebuilt list keeps (see objc_list_entries).
fn is_kept_category_entry<E: Target>(ctx: &Context<E>, id: usize, off: u64) -> bool {
    if !matches!(ctx.hdr_of(&ctx.isecs[id]).sectname(), "__objc_catlist" | "__objc_nlcatlist") {
        return false;
    }
    let obj = &ctx.objs[ctx.isecs[id].file as usize];
    let rel = ctx.isec_relocs(id).iter().find(|r| u64::from(r.offset) == off && r.size == 8);
    let target = rel.and_then(|rel| match rel.target() {
        crate::input_sections::RelocTarget::Section(isec) => Some(isec),
        crate::input_sections::RelocTarget::Sym(idx) => {
            ctx.symbols[obj.symbols[idx as usize]].input_section()
        }
    });
    target.is_some_and(|isec| ctx.isecs[ctx.resolve_isec(isec as usize)].is_alive())
}
