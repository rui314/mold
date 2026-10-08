//! The link's reports: the -map file (where every object file, section
//! and symbol ended up, in ld64's format), -trace_symbol_layout,
//! -sdk_imports with the API list it may be limited to, -dependency_info
//! and the trace files.

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde_json::{Value, json};

use crate::arch::Target;
use crate::chunks::{ChunkHeader, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::error::RawPath;
use crate::input_files::{DylibFile, FileId, NameSource};
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::util::path_bytes;

/// Xcode's version-1 API import report. Despite its name, sdkImports
/// includes imports from non-SDK dylibs too, grouped by install name in
/// the order of the image's load commands - those an
/// -sdk_imports_api_list lists only, if there is one, whose version the
/// report records. An image with none to report has no input in the
/// report. Each library's symbols are listed once, by name: ld-prime
/// lists one a time for each reference to it (its stub, its GOT slot,
/// ...).
pub fn write_sdk_imports<E: Target>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.sdk_imports else { return };
    let api_list = ctx.args.sdk_imports_api_list.as_ref();
    let listed = |name: &&[u8]| api_list.is_none_or(|list| list.apis.contains(*name));
    let mut imports = std::collections::BTreeMap::<(i32, &[u8]), Vec<&[u8]>>::new();
    for (dylib, names) in ctx.dylibs.iter().zip(dylib_imports(ctx)) {
        let names: Vec<&[u8]> = names.into_iter().filter(listed).collect();
        if !names.is_empty() {
            let key = (dylib.dylib_idx, dylib.install_name.as_slice());
            imports.entry(key).or_default().extend(names);
        }
    }

    // JSON is text: a path, install name or symbol name outside UTF-8 is
    // spelled lossily.
    let output = ctx.args.output.to_string_lossy();
    let libraries: Vec<Value> = (imports.into_iter())
        .map(|((_, name), mut symbols)| {
            symbols.sort_unstable();
            symbols.dedup();
            json!({ "installName": String::from_utf8_lossy(name), "symbols": json_names(&symbols) })
        })
        .collect();
    let inputs = match libraries.is_empty() {
        true => Vec::new(),
        false => vec![json!({ "path": output, "sdkImports": libraries })],
    };
    let report = json!({
        "version": 1,
        "output": output,
        "arch": E::NAME,
        "linker": concat!("mold-macho-", env!("CARGO_PKG_VERSION")),
        "apiListVersion": api_list.map_or(0, |list| list.version),
        "platform": platform_name(ctx.args.platform),
        "deploymentVersion": format_version(ctx.args.platform_minos),
        "sdkVersion": format_version(ctx.args.platform_sdk),
        "inputs": inputs,
    });
    let Ok(file) = std::fs::File::create(path) else {
        crate::warn!("can't open SDK imports file for writing at '{}'", path.raw());
        return;
    };
    let mut out = std::io::BufWriter::new(file);
    let _ = serde_json::to_writer_pretty(&mut out, &report);
    let _ = out.write_all(b"\n");
}

/// Writes the -dependency_info file: Xcode's incremental build system
/// reads it to learn which files the link consumed and wrote. The
/// format is binary: an opcode byte then a NUL-terminated string - 0x00
/// the linker's version, 0x10 an input, 0x11 a file looked for and
/// missing, 0x40 an output -, the entries sorted by opcode and then
/// path. The inputs are every file the command line names - objects,
/// archives whether or not a member loads, dylibs whether or not
/// -dead_strip_dylibs keeps them, the -bundle_loader -, the -filelist
/// and -sectcreate files, the libraries auto-link options load and
/// those loaded as another's re-exports, each once. The missing files
/// are those the searches for inputs looked for, each once, as spelled
/// (see reader::Prober): a build system links again when one appears.
/// The outputs are the image, the -map and the -sdk_imports file.
/// Inputs and outputs are named by absolute paths.
pub fn write_dependency_info<E: Target>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.dependency_info else {
        return;
    };
    let mut entries: Vec<(u8, Vec<u8>)> =
        dependency_inputs(ctx).into_iter().map(|path| (0x10, path)).collect();
    let missing = ctx.missing_files.lock().unwrap();
    let mut missing: Vec<&[u8]> = missing.iter().map(|path| path_bytes(path)).collect();
    missing.sort_unstable();
    missing.dedup();
    entries.extend(missing.into_iter().map(|path| (0x11, path.to_vec())));
    let outputs = [Some(&ctx.args.output), ctx.args.map.as_ref(), ctx.args.sdk_imports.as_ref()];
    entries.extend(outputs.into_iter().flatten().map(|path| (0x40, dependency_path(path))));
    entries.sort();

    let Ok(file) = std::fs::File::create(path) else {
        crate::warn!("Could not open or create -dependency_info file: {}", path.raw());
        return;
    };
    let mut out = std::io::BufWriter::new(file);
    let mut emit = |op: u8, s: &[u8]| {
        let _ = out.write_all(&[op]);
        let _ = out.write_all(s);
        let _ = out.write_all(&[0]);
    };
    emit(0x00, format!("{}\n", crate::cmdline::VERSION_BANNER).as_bytes());
    for (op, path) in &entries {
        emit(*op, path);
    }
}

/// The -dependency_info file's inputs (see write_dependency_info).
fn dependency_inputs<E: Target>(ctx: &Context<E>) -> Vec<Vec<u8>> {
    use crate::cmdline::InputArg;
    // The object LTO compiled is no input (a build system can't depend
    // on it), whatever -object_path_lto made of it, nor is the hook for
    // the classes of mergeable libraries (see bundle_hook).
    let mut paths: Vec<&Path> = ctx
        .objs
        .iter()
        .enumerate()
        .filter(|&(i, o)| {
            o.is_reachable && !ctx.is_internal(i) && !ctx.is_bundle_hook(i) && !ctx.is_lto_obj(i)
        })
        .map(|(_, o)| o.mf.parent.map_or(o.mf.name.as_path(), |p| p.name.as_path()))
        .collect();
    paths.extend(ctx.visited_files.iter().map(PathBuf::as_path));
    paths.extend(ctx.args.inputs.iter().filter_map(|arg| match arg {
        InputArg::BundleLoader(path) => Some(path.as_path()),
        _ => None,
    }));
    paths.extend(ctx.args.filelists.iter().map(PathBuf::as_path));
    paths.extend(ctx.args.sectcreate.iter().filter_map(|sc| sc.path.as_deref()));
    paths.extend(ctx.reexport_files.iter().map(PathBuf::as_path));
    // A fat file's slice is the file's.
    let mut paths: Vec<Vec<u8>> = (paths.into_iter())
        .map(|path| crate::filetype::without_fat_arch(&dependency_path(path)))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// A path as the -dependency_info file has it: absolute.
fn dependency_path(path: &Path) -> Vec<u8> {
    match std::path::absolute(path) {
        Ok(path) => path_bytes(&path).to_vec(),
        Err(_) => path_bytes(path).to_vec(),
    }
}

/// -trace_file, -trace_file_shared_cache and -trace_symbols_file: the
/// records Apple's build system has a final link append to these files,
/// a line of JSON each, as ld-prime writes them, naming the output by
/// its leaf name and UUID. -trace_file lists the files of the dylibs it
/// loads, by kind - "dynamic" (weak ones too), "upward-dynamic",
/// "re-exports", "weak", "delay-init" -, in load command order, and the
/// archives a member loads from; -trace_file_shared_cache the same
/// dylibs by install name, but for the archives and the weak ones among
/// the "dynamic", and the output by its path (a dylib by its install
/// name). -trace_symbols_file
/// gives more: a dylib's exports, the symbols each dylib provides (and
/// a re-exported one exports), and the global symbols the members
/// loaded from each archive define, with the archives one isn't loaded
/// from. A library dyld loads lazily is in that one alone. Without a
/// UUID (-no_uuid) ld-prime writes no trace but that last, which then
/// lacks its "uuid". A file it can't write fails the link.
pub fn write_trace_files<E: Target>(ctx: &Context<E>) {
    let args = &ctx.args;
    if args.trace_file.is_none()
        && args.trace_file_shared_cache.is_none()
        && args.trace_symbols_file.is_none()
    {
        return;
    }
    let uuid = args.uuid.then(|| {
        let u = *ctx.uuid.lock().unwrap();
        let hex = |r: std::ops::Range<usize>| -> String {
            u[r].iter().map(|b| format!("{b:02X}")).collect()
        };
        format!("{}-{}-{}-{}-{}", hex(0..4), hex(4..6), hex(6..8), hex(8..10), hex(10..16))
    });
    let traces = TraceInputs::new(ctx);
    if let Some(uuid) = &uuid {
        if let Some(path) = &args.trace_file {
            append_trace(path, &traces.dylibs_json(ctx, uuid, false));
        }
        if let Some(path) = &args.trace_file_shared_cache {
            append_trace(path, &traces.dylibs_json(ctx, uuid, true));
        }
    }
    if let Some(path) = &args.trace_symbols_file {
        append_trace(path, &traces.symbols_json(ctx, uuid.as_deref()));
    }
    if let Some(path) = args.trace_symbols_dir.as_deref().and_then(trace_symbols_dir_file::<E>) {
        append_trace(&path, &traces.symbols_json(ctx, uuid.as_deref()));
    }
}

/// The file of its own -trace_symbols_file's record goes to in `dir`,
/// $LD_TRACE_SYMBOLS_DIR (see cmdline::trace_env), which ld-prime makes
/// first as need be: <parent pid>.<pid>.<arch>.<microseconds since
/// 1970>.json, a name no other link takes.
fn trace_symbols_dir_file<E: Target>(dir: &Path) -> Option<PathBuf> {
    if std::fs::create_dir_all(dir).is_err() {
        // ld-prime reports errno, which mkpath_np leaves alone.
        crate::error!("call to mkpath_np({}) failed due to: Undefined error: 0", dir.raw());
        return None;
    }
    // SAFETY: getppid has no preconditions.
    let ppid = unsafe { libc::getppid() };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    let usec = now.map_or(0, |d| d.as_micros());
    let mut path = dir.as_os_str().to_os_string();
    path.push(format!("/{ppid}.{}.{}.{usec}.json", std::process::id(), E::NAME));
    Some(PathBuf::from(path))
}

/// Appends a record, in a single write, as other links may be appending
/// theirs to the same file.
fn append_trace(path: &Path, record: &Value) {
    let file = std::fs::OpenOptions::new().append(true).create(true).open(path);
    match file {
        Ok(mut file) => {
            let mut line = Vec::new();
            let _ = serde_json::to_writer(&mut line, record);
            line.push(b'\n');
            let _ = file.write_all(&line);
        }
        Err(e) => crate::error!(
            "Could not open or create trace file (errno={}): {}",
            e.raw_os_error().unwrap_or(0),
            path.raw()
        ),
    }
}

/// What the traces list (see write_trace_files): the dylibs with load
/// commands, in their order, and those dyld loads lazily; the archives
/// a member loads from, with the global symbols the loaded members
/// define, and those with a member that doesn't load, by path.
struct TraceInputs<'a> {
    dylibs: Vec<(usize, &'a DylibFile)>,
    lazy: Vec<(usize, &'a DylibFile)>,
    archives: Vec<(String, Vec<&'a [u8]>)>,
    unused_archives: Vec<String>,
}

impl<'a> TraceInputs<'a> {
    fn new<E: Target>(ctx: &'a Context<E>) -> Self {
        let mut dylibs: Vec<(usize, &DylibFile)> = ctx.dylibs.iter().enumerate().collect();
        dylibs.retain(|(_, d)| !d.is_bundle_loader);
        dylibs.sort_by_key(|(_, d)| d.dylib_idx);
        let (lazy, dylibs) = dylibs.into_iter().partition(|(_, d)| d.is_lazy);

        // By archive: whether a member loads, whether one doesn't, and
        // the global symbols the loaded ones define.
        let mut members: hashbrown::HashMap<&Path, (bool, bool, Vec<&[u8]>)> =
            hashbrown::HashMap::new();
        for obj in &ctx.objs {
            let Some(archive) = obj.mf.parent else { continue };
            let entry = members.entry(archive.name.as_path()).or_default();
            if !obj.is_reachable {
                entry.1 = true;
                continue;
            }
            entry.0 = true;
            for (msym, &sym) in obj.mach_syms.iter().zip(&obj.symbols) {
                if msym.is_extern() && !msym.is_stab() && matches!(msym.ty(), N_SECT | N_ABS) {
                    entry.2.push(ctx.symbols[sym].name());
                }
            }
        }
        let mut archives = Vec::new();
        let mut unused_archives = Vec::new();
        for (path, (used, unused, mut syms)) in members {
            let path = trace_path(path);
            if unused {
                unused_archives.push(path.clone());
            }
            if used {
                syms.sort_unstable();
                syms.dedup();
                archives.push((path, syms));
            }
        }
        archives.sort_unstable();
        unused_archives.sort_unstable();
        Self { dylibs, lazy, archives, unused_archives }
    }

    /// The archives a member loads from.
    fn archive_paths(&self) -> Vec<&str> {
        self.archives.iter().map(|(path, _)| path.as_str()).collect()
    }

    /// The record of -trace_file, or with `shared_cache` of
    /// -trace_file_shared_cache.
    fn dylibs_json<E: Target>(&self, ctx: &Context<E>, uuid: &str, shared_cache: bool) -> Value {
        let name = match ctx.args.output_type {
            MH_DYLIB if shared_cache => {
                String::from_utf8_lossy(ctx.args.output_install_name()).into_owned()
            }
            _ if shared_cache => ctx.args.output.to_string_lossy().into_owned(),
            _ => output_leaf(ctx),
        };
        let mut record = json!({ "uuid": uuid, "name": name, "arch": E::NAME });
        let id = |d: &DylibFile| match shared_cache {
            true => String::from_utf8_lossy(&d.install_name).into_owned(),
            false => trace_path(&d.path),
        };
        let weak = |d: &DylibFile| d.is_weak || d.is_weak_asserted;
        // The list a dylib is in, but for "weak", which takes the weak
        // ones besides.
        let list_of = |d: &DylibFile| match () {
            _ if d.is_reexported => "re-exports",
            _ if d.is_upward => "upward-dynamic",
            _ if d.delay_init.is_some() => "delay-init",
            _ if shared_cache && weak(d) => "",
            _ => "dynamic",
        };
        for key in ["dynamic", "upward-dynamic", "re-exports", "weak", "delay-init"] {
            let items: Vec<String> = (self.dylibs.iter())
                .filter(|(_, d)| if key == "weak" { weak(d) } else { list_of(d) == key })
                .map(|(_, d)| id(d))
                .collect();
            if !items.is_empty() {
                record[key] = json!(items);
            }
        }
        if !shared_cache && !self.archives.is_empty() {
            record["archives"] = json!(self.archive_paths());
        }
        record
    }

    /// The record of -trace_symbols_file.
    fn symbols_json<E: Target>(&self, ctx: &Context<E>, uuid: Option<&str>) -> Value {
        let args = &ctx.args;
        let imports = dylib_imports(ctx);
        let dylib_entry = |i: usize, d: &DylibFile, attrs: Vec<&str>| {
            json!({
                "path": trace_path(&d.path),
                "install-name": String::from_utf8_lossy(&d.install_name),
                "arch": E::NAME,
                "attributes": attrs,
                "imported-symbols": json_names(&imports[i]),
            })
        };
        let mut dylibs: Vec<Value> = (self.dylibs.iter())
            .map(|&(i, d)| {
                let attrs = [
                    (d.is_reexported, "re-export"),
                    (d.is_weak || d.is_weak_asserted, "weak"),
                    (d.is_upward, "upward"),
                    (d.delay_init.is_some(), "delay-init"),
                ];
                let attrs = attrs.iter().filter(|a| a.0).map(|a| a.1).collect();
                let mut entry = dylib_entry(i, d, attrs);
                if d.is_reexported {
                    let mut exports: Vec<&[u8]> = d.exports.iter().copied().collect();
                    exports.sort_unstable();
                    entry["exported-symbols"] = json!(json_names(&exports));
                }
                entry
            })
            .collect();
        dylibs.extend(self.lazy.iter().map(|&(i, d)| dylib_entry(i, d, vec!["lazy-load"])));
        let archives: Vec<Value> = (self.archives.iter())
            .map(|(path, syms)| {
                json!({ "path": path, "arch": E::NAME, "imported-symbols": json_names(syms) })
            })
            .collect();

        let version = args.platform_minos;
        let mut record = json!({
            "version": "2",
            "minor-version": 1,
            "name": output_leaf(ctx),
            "arch": E::NAME,
            "platforms": [{
                "name": platform_name(args.platform),
                "min-version": {
                    "major": (version >> 16).to_string(),
                    "minor": ((version >> 8) & 0xff).to_string(),
                },
            }],
            "exports": json_names(&own_exports(ctx)),
            "linked-dylibs": dylibs,
            "archives": self.archive_paths(),
            "unused-archives": self.unused_archives,
            "linked-archives": archives,
        });
        if args.output_type == MH_DYLIB {
            let eligible = if args.shared_region { "yes" } else { "no" };
            record["install-name"] = json!(String::from_utf8_lossy(args.output_install_name()));
            record["shared-cache-eligible"] = json!(eligible);
        }
        if let Some(uuid) = uuid {
            record["uuid"] = json!(uuid);
        }
        record
    }
}

/// Symbol names as JSON strings, which are text: a name with a byte
/// that isn't UTF-8 is spelled with U+FFFD in its place (ld-prime writes
/// the byte, which leaves the JSON malformed).
fn json_names<'a>(names: &[&'a [u8]]) -> Vec<Cow<'a, str>> {
    names.iter().map(|name| String::from_utf8_lossy(name)).collect()
}

/// The output's name in the traces: its leaf name, a dylib's install
/// name's.
fn output_leaf<E: Target>(ctx: &Context<E>) -> String {
    let path = match ctx.args.output_type {
        MH_DYLIB => Path::new(crate::util::os_str(ctx.args.output_install_name())),
        _ => ctx.args.output.as_path(),
    };
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

/// A file's path as the traces give it: its real path, a fat file's
/// slice by the file's.
fn trace_path(path: &Path) -> String {
    let (path, _) = crate::filetype::split_fat_arch(crate::util::path_bytes(path));
    let path = Path::new(crate::util::os_str(path));
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.to_string_lossy().into_owned()
}

/// The symbols a dylib output exports of its own definitions, sorted:
/// what its export trie lists but for re-exports.
fn own_exports<E: Target>(ctx: &Context<E>) -> Vec<&'static [u8]> {
    if ctx.args.output_type != MH_DYLIB {
        return Vec::new();
    }
    let mut names: Vec<&[u8]> = (0..ctx.symbols.syms.len() as SymbolId)
        .filter(|&id| {
            let sym = &ctx.symbols[id];
            matches!(sym.file(), Some(FileId::Obj(obj)) if !ctx.is_internal(obj as usize))
                && sym.is_extern()
                && !sym.is_private_extern()
                && sym
                    .input_section()
                    .is_none_or(|isec| ctx.isecs[ctx.isecs.resolve(isec as usize)].is_alive())
                && !ctx.indirect_aliases.iter().any(|&(alias, _)| alias == id)
        })
        .map(|id| ctx.symbols[id].name())
        .collect();
    names.sort_unstable();
    names
}

/// The symbols the output imports from each dylib, sorted, by dylib.
fn dylib_imports<E: Target>(ctx: &Context<E>) -> Vec<Vec<&'static [u8]>> {
    let mut imports: Vec<Vec<&[u8]>> = vec![Vec::new(); ctx.dylibs.len()];
    for sym in &ctx.symbols.syms {
        if let Some(FileId::Dylib(i)) = sym.file()
            && sym.is_imported()
            && sym.is_used()
            && let Some(list) = imports.get_mut(i as usize)
        {
            list.push(sym.name());
        }
    }
    for list in &mut imports {
        list.sort_unstable();
        list.dedup();
    }
    imports
}

/// -trace_symbol_layout prints, or -trace_symbol_layout_file writes,
/// the output section each symbol the map lists (see is_map_symbol)
/// went to, in symbol order: "symbol '_x', mapped to __TEXT/__text". A
/// file that can't be written is a warning, ending with a blank line as
/// ld-prime's does, and the trace goes nowhere then. A -r link reports
/// nothing.
pub fn trace_symbol_layout<E: Target>(ctx: &Context<E>) {
    let args = &ctx.args;
    if args.relocatable {
        return;
    }
    let mut out: Box<dyn std::io::Write> = match &args.trace_symbol_layout_file {
        Some(path) => match std::fs::File::create(path) {
            Ok(file) => Box::new(std::io::BufWriter::new(file)),
            Err(e) => {
                crate::warn!(
                    "could not open -trace_symbol_layout_file {} for writing ({})\n",
                    path.raw(),
                    e.raw_os_error().unwrap_or(0)
                );
                return;
            }
        },
        None if args.trace_symbol_layout => Box::new(std::io::stdout().lock()),
        None => return,
    };
    for sym in &ctx.symbols.syms {
        let Some(isec) = sym.input_section() else { continue };
        if !is_map_symbol(sym) {
            continue;
        }
        let Some(chunk) = ctx.isecs[ctx.isecs.resolve(isec as usize)].output_section() else {
            continue;
        };
        let hdr = ctx.chunk_header(chunk);
        let line = [b"symbol '", sym.name(), b"', mapped to ", hdr.segname, b"/", hdr.sectname];
        let _ = out.write_all(&line.concat());
        let _ = out.write_all(b"\n");
    }
}

/// Whether a symbol names a row of the map: any named one but a local
/// label a compiler or assembler makes for itself (see
/// input_sections::is_private_label).
pub(crate) fn is_map_symbol(sym: &crate::symbol::Symbol) -> bool {
    let name = sym.name();
    !name.is_empty() && (sym.is_extern() || !crate::input_sections::is_private_label(name))
}

/// A row of the map's symbol list: the address and size of a
/// subsection, or of the part of one a symbol names, the number of the
/// file it came from (see MapFiles), and its name.
struct Row<'a> {
    addr: u64,
    size: u64,
    file: usize,
    name: Cow<'a, [u8]>,
}

/// A section of the map: its header, and the output section whose
/// members it holds, if the inputs make it.
pub type MapSection<'a> = (&'a ChunkHeader, Option<OutputSectionId>);

/// A symbol the map lists, by place: (subsection, offset, name).
type Label = (u32, u64, &'static [u8]);

/// The files of the map, numbered from 1: the live objects in link
/// order (an archive's member as "lib.a(foo.o)", the object LTO
/// compiled as itself), then the dylibs, each once. Number 0 stands
/// for the linker, which makes the stubs, the unwind info and such,
/// and the subsections of its own object.
struct MapFiles<'a> {
    paths: Vec<&'a Path>,
    /// The number of each object, 0 for one not listed.
    objs: Vec<usize>,
}

impl<'a> MapFiles<'a> {
    fn new<E: Target>(ctx: &'a Context<E>) -> Self {
        let mut paths = Vec::new();
        let mut objs = vec![0; ctx.objs.len()];
        for (i, obj) in ctx.objs.iter().enumerate() {
            if obj.is_reachable && !ctx.is_internal(i) && !ctx.is_bundle_hook(i) {
                paths.push(obj.mf.name.as_path());
                objs[i] = paths.len();
            }
        }
        // (A dylib that stands for a library exports moved to is no
        // file of its own.)
        let dylibs = ctx.dylibs.iter().filter(|dylib| dylib.name_source != NameSource::Moved);
        paths.extend(dylibs.map(|dylib| dylib.path.as_path()));
        Self { paths, objs }
    }
}

/// Writes the -map file of a final image, if asked for (see
/// print_map_of).
pub fn print_map<E: Target>(ctx: &Context<E>) {
    if ctx.args.map.is_none() {
        return;
    }
    let mut sections: Vec<MapSection> = Vec::new();
    for &id in ctx.segments.iter().flat_map(|seg| &seg.chunks) {
        let hdr = ctx.chunk_header(id);
        if hdr.is_sect {
            let osec = match id {
                ChunkId::Output(osec) => Some(osec),
                _ => None,
            };
            sections.push((hdr, osec));
        }
    }
    print_map_of(ctx, &sections);
}

/// Writes the -map file, if asked for, of an output of `sections`: the
/// output and its architecture, its files (see MapFiles), its sections
/// and its symbols, and under -dead_strip, the subsections it dropped.
/// Each subsection has a row for each symbol of it the map lists (see
/// subsec_rows); each section the linker makes, and the part of one
/// past its subsections that the linker makes, one row of the
/// linker's, named after the section; and each branch island one, as
/// the symbol table names it. A -r link calls this with the sections
/// it writes.
pub fn print_map_of<E: Target>(ctx: &Context<E>, sections: &[MapSection]) {
    let Some(path) = &ctx.args.map else { return };
    let files = MapFiles::new(ctx);
    let labels = subsec_labels(ctx);
    let section_name = |hdr: &ChunkHeader| Cow::Owned([hdr.segname, b",", hdr.sectname].concat());

    let mut rows = Vec::new();
    if ctx.args.output_type == MH_EXECUTE && !ctx.args.preload && !ctx.args.relocatable {
        let addr = ctx.mach_header.hdr.addr;
        rows.push(Row { addr, size: 0, file: 0, name: Cow::Borrowed(b"__mh_execute_header") });
    }
    for &(hdr, osec) in sections {
        let Some(osec) = osec else {
            if hdr.size > 0 {
                rows.push(Row { addr: hdr.addr, size: hdr.size, file: 0, name: section_name(hdr) });
            }
            continue;
        };
        let osec = ctx.output_section(osec);
        let members: Vec<Vec<Row>> = (osec.members.par_iter())
            .fold(Vec::new, |mut rows, &id| {
                let addr = ctx.isecs[id as usize].addr(ctx);
                subsec_rows(ctx, &files, &labels, id as usize, addr, &mut rows);
                rows
            })
            .collect();
        rows.extend(members.into_iter().flatten());
        // What the linker adds after the members: a tail.
        if osec.tail != crate::chunks::output_section::Tail::None {
            let (addr, size) = (hdr.addr + osec.tail_off, hdr.size - osec.tail_off);
            rows.push(Row { addr, size, file: 0, name: section_name(hdr) });
        }
    }
    for (addr, _, name) in crate::thunks::island_symbols(ctx) {
        rows.push(Row { addr, size: E::THUNK_SIZE, file: 0, name: Cow::Borrowed(name) });
    }
    rows.par_sort_by_key(|row| row.addr);
    let dead = dead_rows(ctx, &files, &labels);
    write_map(ctx, path, &files, sections, &rows, &dead);
}

/// The symbols the map lists (see is_map_symbol), sorted by place: a
/// symbol of a subsection merged into an identical one is at the
/// survivor's place, after the survivor's own.
fn subsec_labels<E: Target>(ctx: &Context<E>) -> Vec<Label> {
    let mut labels: Vec<_> = (ctx.symbols.syms.par_iter())
        .filter_map(|sym| {
            let own = sym.input_section()? as usize;
            let isec = ctx.isecs.resolve(own);
            is_map_symbol(sym).then_some(((isec as u32, sym.value, isec != own), sym.name()))
        })
        .collect();
    labels.par_sort_by_key(|&(place, _)| place);
    labels.into_iter().map(|((isec, value, _), name)| (isec, value, name)).collect()
}

/// Appends the rows of subsection `id` at `addr` to `rows`: one for
/// each of its `labels`, the first at a place sized to the next place
/// or the subsection's end, the others there of no size; and an "anon"
/// one for the bytes before the first, if any.
fn subsec_rows<E: Target>(
    ctx: &Context<E>,
    files: &MapFiles,
    labels: &[Label],
    id: usize,
    addr: u64,
    rows: &mut Vec<Row>,
) {
    let isec = &ctx.isecs[id];
    let (size, file) = (isec.size as u64, files.objs[isec.file as usize]);
    let start = labels.partition_point(|l| (l.0 as usize) < id);
    let len = labels[start..].partition_point(|l| l.0 as usize == id);
    let labels = &labels[start..start + len];
    let first = labels.first().map_or(size, |l| l.1);
    if first > 0 {
        rows.push(Row { addr, size: first, file, name: Cow::Borrowed(b"anon") });
    }
    let mut places = labels.chunk_by(|a, b| a.1 == b.1).peekable();
    while let Some(place) = places.next() {
        let value = place[0].1;
        let end = places.peek().map_or(size, |next| next[0].1);
        for (j, &(_, _, name)) in place.iter().enumerate() {
            let size = if j == 0 { end.saturating_sub(value) } else { 0 };
            rows.push(Row { addr: addr + value, size, file, name: Cow::Borrowed(name) });
        }
    }
}

/// The rows of the dead-stripped list: under -dead_strip, the
/// subsections of the listed objects that the output doesn't have,
/// but those merged into an identical one, by object, as the live
/// ones' (see subsec_rows).
fn dead_rows<E: Target>(ctx: &Context<E>, files: &MapFiles, labels: &[Label]) -> Vec<Row<'static>> {
    if !ctx.args.dead_strip {
        return Vec::new();
    }
    let objs: Vec<Vec<Row>> = (0..ctx.objs.len())
        .into_par_iter()
        .filter(|&i| files.objs[i] != 0)
        .map(|i| {
            let mut rows = Vec::new();
            for &id in &ctx.objs[i].subsecs {
                let isec = &ctx.isecs[id];
                if !isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT {
                    subsec_rows(ctx, files, labels, id as usize, 0, &mut rows);
                }
            }
            rows
        })
        .collect();
    objs.into_iter().flatten().collect()
}

/// Writes the map: the output and its architecture, the `files`, the
/// `sections`, the `rows` of the subsections and those of the `dead`
/// ones.
fn write_map<E: Target>(
    ctx: &Context<E>,
    path: &Path,
    files: &MapFiles,
    sections: &[MapSection],
    rows: &[Row],
    dead: &[Row],
) {
    let Ok(file) = std::fs::File::create(path) else {
        crate::warn!("could not write map file: {}", path.raw());
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
        let _ = out.write_all(&crate::filetype::without_fat_arch(path_bytes(path)));
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "# Sections:");
    let _ = writeln!(out, "# Address\tSize    \tSegment\tSection");
    for (hdr, _) in sections {
        let _ = write!(out, "0x{:08X}\t0x{:08X}\t", hdr.addr, hdr.size);
        let _ = out.write_all(&[hdr.segname, b"\t", hdr.sectname].concat());
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "# Symbols:");
    let _ = writeln!(out, "# Address\tSize    \tFile  Name");
    for row in rows {
        let _ = write!(out, "0x{:08X}\t0x{:08X}\t[{:3}] ", row.addr, row.size, row.file);
        let _ = out.write_all(&row.name);
        let _ = writeln!(out);
    }

    // The subsections -dead_strip removed come in a list of their own,
    // with "<<dead>>" in the address column, as in ld64's map.
    if !dead.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "# Dead Stripped Symbols:");
        let _ = writeln!(out, "#        \tSize    \tFile  Name");
        for row in dead {
            let _ = write!(out, "<<dead>>\t0x{:08X}\t[{:3}] ", row.size, row.file);
            let _ = out.write_all(&row.name);
            let _ = writeln!(out);
        }
    }
}
