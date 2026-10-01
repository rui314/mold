//! -map file output: a report of where every object file, section and
//! symbol ended up, in ld64's format.

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::chunks::ChunkId;
use crate::context::Context;
use crate::fatal;
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
    std::fs::write(path, report).unwrap_or_else(|e| fatal!("cannot write {}: {e}", path.display()));
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
    let file = std::fs::File::create(path)
        .unwrap_or_else(|e| fatal!("cannot open {}: {e}", path.display()));
    let mut out = std::io::BufWriter::new(file);
    let mut emit = |op: u8, s: &[u8]| {
        let _ = out.write_all(&[op]);
        let _ = out.write_all(s);
        let _ = out.write_all(&[0]);
    };

    emit(0x00, concat!("mold-macho ", env!("CARGO_PKG_VERSION")).as_bytes());
    let mut inputs: Vec<&Path> = ctx
        .objs
        .iter()
        .enumerate()
        .filter(|(i, o)| o.is_alive && !ctx.is_internal(*i))
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
/// binds to, and last the dylibs that auto-link options named and
/// something binds to, in the order ld-prime acts on the options. A
/// re-exported library named too is listed where it is named, by the
/// path given. The re-exported
/// libraries include the private ones a dylib merges (libSystem's
/// libsystem_c), which ld-prime reads from files of their own and
/// credits with the symbols they define. Number 0 stands for the
/// linker, which makes the stubs, the unwind info and such.
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
}

impl<'a> MapFiles<'a> {
    fn new<E: Target>(ctx: &'a Context<E>) -> Self {
        enum File<'a> {
            Obj(usize),
            Dylib(usize),
            Merged(&'a MergedFile),
            Stripped(&'a Path),
        }
        let mut named: Vec<(u32, File)> = Vec::new();
        for (i, obj) in ctx.objs.iter().enumerate() {
            if obj.is_alive && !ctx.is_internal(i) {
                named.push((obj.priority, File::Obj(i)));
            }
        }
        let mut autolinked: Vec<(u32, File)> = Vec::new();
        for (i, dylib) in ctx.dylibs.iter().enumerate() {
            if dylib.is_autolinked {
                autolinked.push((dylib.load_order, File::Dylib(i)));
            } else if !dylib.is_implicit {
                let priority = dylib.named_at.as_ref().map_or(dylib.priority, |&(p, _)| p);
                named.push((priority, File::Dylib(i)));
            }
        }
        for (priority, path) in &ctx.stripped_dylibs {
            named.push((*priority, File::Stripped(path)));
        }
        named.sort_by_key(|(priority, _)| *priority);
        autolinked.sort_by_key(|(priority, _)| *priority);

        // The merged library that provides each symbol bound to a dylib
        // that merged some.
        let mut providers: Vec<Option<hashbrown::HashMap<&str, &MergedFile>>> =
            (0..ctx.dylibs.len()).map(|_| None).collect();
        let mut provided: Vec<(SymbolId, &MergedFile)> = Vec::new();
        for i in 0..ctx.symbols.syms.len() as SymbolId {
            let sym = &ctx.symbols[i];
            let Some(FileId::Dylib(d)) = sym.file() else { continue };
            let Some(dylib) = ctx.dylibs.get(d as usize) else { continue };
            if dylib.merged_files.is_empty() {
                continue;
            }
            let by_name = providers[d as usize].get_or_insert_with(|| {
                let mut map = hashbrown::HashMap::new();
                for file in &dylib.merged_files {
                    for &name in &file.exports {
                        map.entry(name).or_insert(file);
                    }
                }
                map
            });
            if let Some(&file) = by_name.get(sym.name()) {
                provided.push((i, file));
            }
        }

        // A dylib that stands for a library exports moved to is no file:
        // its symbols count as the file's that moved them.
        let is_moved = |dylib: &DylibFile| dylib.name_source == NameSource::Moved;
        let mut implicit: Vec<(&[u8], File)> = Vec::new();
        for (i, dylib) in ctx.dylibs.iter().enumerate() {
            if dylib.is_implicit && !dylib.is_autolinked && !is_moved(dylib) {
                implicit.push((&dylib.install_name, File::Dylib(i)));
            }
        }
        for &(_, file) in &provided {
            if implicit.iter().all(|(name, _)| *name != file.install_name) {
                implicit.push((&file.install_name, File::Merged(file)));
            }
        }
        implicit.sort_by_key(|(name, _)| *name);

        let mut files = Self {
            paths: Vec::new(),
            objs: vec![0; ctx.objs.len()],
            dylibs: vec![0; ctx.dylibs.len()],
            merged: hashbrown::HashMap::new(),
            commons: crate::output_sections::common_owners(ctx),
        };
        let mut merged_numbers: hashbrown::HashMap<&[u8], usize> = hashbrown::HashMap::new();
        let named = named.into_iter().map(|(_, file)| file);
        let implicit = implicit.into_iter().map(|(_, file)| file);
        let autolinked = autolinked.into_iter().map(|(_, file)| file);
        for file in named.chain(implicit).chain(autolinked) {
            let number = files.paths.len() + 1;
            match file {
                File::Obj(i) => {
                    files.objs[i] = number;
                    files.paths.push(&ctx.objs[i].mf.name);
                }
                File::Dylib(i) => {
                    files.dylibs[i] = number;
                    let dylib = &ctx.dylibs[i];
                    files.paths.push(dylib.named_at.as_ref().map_or(&dylib.path, |(_, path)| path));
                }
                File::Stripped(path) => files.paths.push(path),
                File::Merged(file) => {
                    merged_numbers.insert(&file.install_name, number);
                    files.paths.push(&file.path);
                }
            }
        }
        for (sym, file) in provided {
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
            Some(FileId::Obj(i)) => {
                let owner = sym.input_section().and_then(|isec| self.commons.get(&isec));
                self.objs[owner.copied().unwrap_or(i) as usize]
            }
            Some(FileId::Dylib(i)) => self.dylibs.get(i as usize).copied().unwrap_or(0),
            None => 0,
        }
    }
}

pub fn print_map<E: Target>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.map else { return };
    let file = std::fs::File::create(path)
        .unwrap_or_else(|e| fatal!("cannot open {}: {e}", path.display()));
    let mut out = std::io::BufWriter::new(file);

    let _ = write!(out, "# Path: ");
    let _ = out.write_all(path_bytes(&ctx.args.output));
    let _ = writeln!(out);
    let _ = writeln!(out, "# Arch: {}", E::NAME);

    let files = MapFiles::new(ctx);
    let _ = writeln!(out, "# Object files:");
    let _ = writeln!(out, "[  0] linker synthesized");
    for (i, path) in files.paths.iter().enumerate() {
        let _ = write!(out, "[{:3}] ", i + 1);
        let _ = out.write_all(path_bytes(path));
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "# Sections:");
    let _ = writeln!(out, "# Address\tSize    \tSegment\tSection");
    for seg in &ctx.segments {
        for &id in &seg.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.is_sect {
                let _ = writeln!(
                    out,
                    "0x{:08X}\t0x{:08X}\t{}\t{}",
                    hdr.addr, hdr.size, hdr.segname, hdr.sectname
                );
            }
        }
        // ld-prime models a static executable's stack as a zero-fill
        // section of a linker-made atom, which its segment's load
        // command doesn't list.
        if seg.name == "__UNIXSTACK" {
            let _ = writeln!(
                out,
                "0x{:08X}\t0x{:08X}\t__UNIXSTACK\t__stack",
                seg.cmd.vmaddr, seg.cmd.vmsize
            );
        }
    }

    let mut entries = symbol_entries(ctx, &files);
    entries.extend(unnamed_entries(ctx, &files, &entries));
    entries.extend(eh_frame_entries(ctx, &files, &entries));
    entries.extend(synthetic_entries(ctx, &files));
    entries.sort_by_key(|e| (e.addr, e.size));

    let _ = writeln!(out, "# Symbols:");
    let _ = writeln!(out, "# Address\tSize    \tFile  Name");
    if ctx.args.output_type == MH_EXECUTE && !ctx.args.preload {
        let _ = writeln!(
            out,
            "0x{:08X}\t0x00000000\t[  0] __mh_execute_header",
            ctx.mach_header.hdr.addr
        );
    }
    for e in &entries {
        let _ = write!(out, "0x{:08X}\t0x{:08X}\t[{:3}] ", e.addr, e.size, e.file);
        let _ = out.write_all(&e.name);
        let _ = writeln!(out);
    }

    // Symbols removed by -dead_strip appear in their own section
    // with "<<dead>>" in the address column, the way ld64 reports
    // them; sizes are the atom extents they would have had.
    let dead = dead_entries(ctx, &files);
    if !dead.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "# Dead Stripped Symbols:");
        let _ = writeln!(out, "#        \tSize    \tFile  Name");
        for e in &dead {
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
    let name = sym.name();
    let isec = ctx.resolve_isec(sym.input_section().unwrap() as usize);
    let hdr = ctx.hdr_of(&ctx.isecs[isec]);
    if name.is_empty()
        || hdr.section_type() == S_CSTRING_LITERALS
        || crate::input_files::is_ignored_literal_label(hdr.section_type(), name)
    {
        return false;
    }
    if sym.is_extern() {
        return true;
    }
    let split = ctx.objs[ctx.isecs[isec].file as usize].subsections_via_symbols;
    !crate::input_files::is_record_list(hdr, split) && !name.starts_with('L')
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
/// at one place, the first in the object's symbol table has the size
/// and the others are aliases of none, as is an alternate entry point
/// (Swift's type metadata inside its full metadata). ld-prime counts a
/// thread-local variable's descriptor, which it rewrites, as its own,
/// and a common symbol as the object's whose tentative definition won.
/// It credits itself with an atom it rewrote (a method list in the
/// relative form).
fn symbol_entries<'a, E: Target>(ctx: &'a Context<E>, files: &MapFiles) -> Vec<MapEntry<'a>> {
    let nlists = defining_nlists(ctx);
    let mut syms: Vec<(SymbolId, usize)> = Vec::new();
    for i in 0..ctx.symbols.syms.len() as SymbolId {
        let sym = &ctx.symbols[i];
        let (Some(FileId::Obj(obj)), Some(isec)) = (sym.file(), sym.input_section()) else {
            continue;
        };
        let isec = ctx.resolve_isec(isec as usize);
        if !ctx.isecs[isec].is_alive() {
            continue;
        }
        let file = match files.commons.get(&(isec as u32)) {
            _ if ctx.hdr_of(&ctx.isecs[isec]).section_type() == S_THREAD_LOCAL_VARIABLES => 0,
            Some(&owner) => files.objs[owner as usize],
            None if is_rewritten_method_list(ctx, isec) => 0,
            None => files.objs[obj as usize],
        };
        if is_named(ctx, sym) {
            syms.push((i, file));
        }
    }
    drop_shadowed_ltmps(ctx, &mut syms, |&(sym, _)| sym);

    // Sizes: sort the atoms' first symbols by place and measure to the
    // next one.
    let key = |sym: SymbolId| {
        let isec = ctx.resolve_isec(ctx.symbols[sym].input_section().unwrap() as usize);
        let first = std::cmp::Reverse(nlists.get(&sym).map_or(0, |&(k, _)| k));
        (isec, ctx.symbols[sym].value, first)
    };
    let is_alt_entry = |sym: SymbolId| nlists.get(&sym).is_some_and(|&(_, alt)| alt);
    let mut order: Vec<usize> = (0..syms.len()).filter(|&i| !is_alt_entry(syms[i].0)).collect();
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

    syms.iter()
        .zip(sizes)
        .map(|(&(sym, file), size)| MapEntry {
            addr: ctx.sym_addr(sym),
            size,
            file,
            name: name(ctx.symbols[sym].name()),
        })
        .collect()
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

/// The atoms of the input files no symbol names, up to the first
/// symbol in one: literals by what they are, any other "anon". The
/// selector names of Objective-C stubs are file 0's: ld-prime makes
/// them itself, and an input's copy merges into its own.
fn unnamed_entries<'a, E: Target>(
    ctx: &'a Context<E>,
    files: &MapFiles,
    named: &[MapEntry],
) -> Vec<MapEntry<'a>> {
    let named: std::collections::BTreeSet<u64> = named.iter().map(|e| e.addr).collect();
    let stub_names: hashbrown::HashSet<usize> = (ctx.objc_stubs.name_isec.iter())
        .filter(|&&isec| isec != u32::MAX)
        .map(|&isec| ctx.resolve_isec(isec as usize))
        .collect();
    let mut entries = Vec::new();
    for osec in &ctx.output_sections {
        for &id in &osec.members {
            let isec = &ctx.isecs[id];
            let addr = ctx.isec_addr(id as usize);
            let size = isec.size as u64;
            if ctx.is_internal(isec.file as usize) || size == 0 {
                continue;
            }
            let file = if stub_names.contains(&(id as usize)) {
                0
            } else {
                files.objs[isec.file as usize]
            };
            let name = literal_name(ctx, isec).unwrap_or(name("anon"));
            let next = named.range(addr..addr + size).next().copied();
            if next != Some(addr) {
                let size = next.unwrap_or(addr + size) - addr;
                entries.push(MapEntry { addr, size, file, name });
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
    // Of the symbols at one place, the one listed first names the atom.
    let mut atoms: hashbrown::HashMap<u64, (u64, &[u8])> = hashbrown::HashMap::new();
    for e in named {
        let atom = atoms.entry(e.addr).or_insert((e.size, &e.name));
        if e.size < atom.0 {
            *atom = (e.size, &e.name);
        }
    }
    let base = ctx.eh_frame.hdr.addr;
    let mut entries = Vec::new();
    for cie in ctx.cies.iter().filter(|cie| cie.is_alive) {
        let addr = base + cie.output_offset as u64;
        let (size, file) = (cie.data.len() as u64, files.objs[cie.obj as usize]);
        entries.push(MapEntry { addr, size, file, name: name("CFI") });
    }
    for fde in &ctx.fdes {
        let func = ctx.isec_addr(fde.isec as usize) + fde.func_offset as u64;
        let func = atoms.get(&func).map_or(&b"anon"[..], |&(_, name)| name);
        let mut name = b"FDE for: ".to_vec();
        name.extend_from_slice(func);
        let addr = base + fde.output_offset as u64;
        let (size, file) = (fde.data.len() as u64, files.objs[fde.obj as usize]);
        entries.push(MapEntry { addr, size, file, name: Cow::Owned(name) });
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
        let addr = objc_stubs.hdr.addr + i as u64 * E::OBJC_STUB_SIZE;
        let name = name(ctx.symbols[sym].name());
        entries.push(MapEntry { addr, size: E::OBJC_STUB_SIZE, file: 0, name });
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

/// The atoms -dead_strip removed, by the file they came from and where
/// they were in it: the symbols with the sizes their atoms would have
/// had, and the literals no symbol names.
fn dead_entries<'a, E: Target>(ctx: &'a Context<E>, files: &MapFiles) -> Vec<MapEntry<'a>> {
    if !ctx.args.dead_strip {
        return Vec::new();
    }
    let is_dead = |isec: &crate::input_sections::InputSection| {
        !isec.is_alive()
            && isec.replacement == crate::input_sections::NO_REPLACEMENT
            && ctx.objs[isec.file as usize].is_alive
            && !ctx.is_internal(isec.file as usize)
    };

    // (isec, offset, name) of each symbol, sized as live ones are.
    let mut ids: Vec<SymbolId> = Vec::new();
    for i in 0..ctx.symbols.syms.len() as SymbolId {
        let sym = &ctx.symbols[i];
        let (Some(FileId::Obj(_)), Some(isec)) = (sym.file(), sym.input_section()) else {
            continue;
        };
        let isec = ctx.resolve_isec(isec as usize);
        if is_dead(&ctx.isecs[isec]) && is_named(ctx, sym) {
            ids.push(i);
        }
    }
    drop_shadowed_ltmps(ctx, &mut ids, |&sym| sym);
    let mut syms: Vec<(usize, u64, &str)> = ids
        .iter()
        .map(|&i| {
            let sym = &ctx.symbols[i];
            (ctx.resolve_isec(sym.input_section().unwrap() as usize), sym.value, sym.name())
        })
        .collect();
    syms.sort();
    let mut dead: Vec<(usize, u64, MapEntry)> = Vec::new();
    for (i, &(isec, value, name)) in syms.iter().enumerate() {
        let end = match syms.get(i + 1) {
            Some(&(next_isec, next_value, _)) if next_isec == isec => next_value,
            _ => ctx.isecs[isec].size as u64,
        };
        let file = files.objs[ctx.isecs[isec].file as usize];
        let size = end.saturating_sub(value);
        let entry = MapEntry { addr: 0, size, file, name: name.as_bytes().into() };
        dead.push((file, u64::from(ctx.isecs[isec].input_addr) + value, entry));
    }

    for (id, isec) in ctx.isecs.iter().enumerate() {
        if !is_dead(isec) {
            continue;
        }
        let Some(name) = literal_name(ctx, isec) else { continue };
        if syms.binary_search_by_key(&(id, 0), |&(isec, value, _)| (isec, value)).is_err() {
            let file = files.objs[isec.file as usize];
            let entry = MapEntry { addr: 0, size: isec.size as u64, file, name };
            dead.push((file, u64::from(isec.input_addr), entry));
        }
    }
    dead.sort_by_key(|(file, addr, _)| (*file, *addr));
    dead.into_iter().map(|(_, _, entry)| entry).collect()
}
