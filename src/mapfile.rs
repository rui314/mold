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
/// includes imports from non-SDK dylibs too, grouped by install name in
/// the order of the image's load commands - those an
/// -sdk_imports_api_list lists only, if there is one, whose version the
/// report records. An image with none to report has no input in the
/// report. The JSON is laid out as ld-prime lays it out, but each
/// library's symbols are listed once, by name: ld-prime lists one a
/// time for each reference to it (its stub, its GOT slot, ...).
pub fn write_sdk_imports<E: Target>(ctx: &Context<E>) {
    use crate::macho::{format_version, platform_name};
    let Some(path) = &ctx.args.sdk_imports else { return };
    let api_list = ctx.args.sdk_imports_api_list.as_ref();
    let mut imports = std::collections::BTreeMap::<(i32, &[u8]), Vec<&str>>::new();
    for sym in &ctx.symbols.syms {
        if !sym.is_imported() || !sym.is_used() {
            continue;
        }
        if api_list.is_some_and(|list| !list.apis.contains(sym.name().as_bytes())) {
            continue;
        }
        let Some(FileId::Dylib(idx)) = sym.file() else { continue };
        // Dynamic-lookup symbols have no defining library to report.
        let Some(dylib) = ctx.dylibs.get(idx as usize) else { continue };
        let key = (dylib.dylib_idx, dylib.install_name.as_slice());
        imports.entry(key).or_default().push(sym.name());
    }

    // JSON is text: a path or install name outside UTF-8 is spelled lossily.
    let output = json_string(&ctx.args.output.to_string_lossy());
    let mut libraries = Vec::new();
    for ((_, name), mut symbols) in imports {
        symbols.sort_unstable();
        symbols.dedup();
        let symbols: Vec<String> =
            symbols.into_iter().map(|s| format!("            {}", json_string(s))).collect();
        libraries.push(format!(
            "        {{\n          \"installName\": {},\n          \"symbols\": [\n{}\n          \
             ]\n        }}",
            json_string(&crate::util::display(name)),
            symbols.join(",\n")
        ));
    }
    let inputs = match libraries.is_empty() {
        true => String::new(),
        false => format!(
            "    {{\n      \"path\": {output},\n      \"sdkImports\": [\n{}\n      ]\n    }}",
            libraries.join(",\n")
        ),
    };
    let report = format!(
        "{{\n  \"version\": 1,\n  \"output\": {output},\n  \"arch\": {},\n  \"linker\": {},\n  \
         \"apiListVersion\": {},\n  \"platform\": {},  \"deploymentVersion\": {},  \
         \"sdkVersion\": {},  \"inputs\": [\n{inputs}\n  ]\n}}\n",
        json_string(E::NAME),
        json_string(concat!("mold-macho-", env!("CARGO_PKG_VERSION"))),
        api_list.map_or(0, |list| list.version),
        json_string(&platform_name(ctx.args.platform)),
        json_string(&format_version(ctx.args.platform_minos)),
        json_string(&format_version(ctx.args.platform_sdk)),
    );
    if std::fs::write(path, report).is_err() {
        crate::warn!("can't open SDK imports file for writing at '{}'", path.display());
    }
}

/// Writes the -dependency_info file: Xcode's incremental build system
/// reads it to learn which files the link consumed and wrote. The
/// format is binary: an opcode byte then a NUL-terminated string - 0x00
/// the linker's version (ld-prime's -v banner, newline and all), 0x10
/// an input, 0x11 a file looked for and missing, 0x40 an output -, the
/// entries sorted by opcode and then path, duplicates kept. The inputs
/// are every file the command line names - objects, archives whether
/// or not a member loads, dylibs whether or not -dead_strip_dylibs
/// keeps them, the -bundle_loader -, the -filelist and -sectcreate
/// files, the libraries auto-link options load, and twice each the
/// libraries loaded only as another's re-exports (ld-prime records them
/// as it finds them and as it loads them). The missing files are those
/// the searches for inputs looked for, each once, as spelled (see
/// passes::Prober): a build system links again when one appears. The
/// outputs are the image, the -map and the -sdk_imports file. A file
/// is named as often as it is spelled differently, and a relative path
/// resolved to the file's real path, where there is one: an output's,
/// where a previous link wrote it.
pub fn write_dependency_info<E: Target>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.dependency_info else {
        return;
    };
    let mut entries: Vec<(u8, Vec<u8>)> = dependency_inputs(ctx);
    let mut missing = ctx.missing_files.lock().unwrap().clone();
    missing.sort_unstable();
    missing.dedup();
    entries.extend(missing.iter().map(|path| (0x11, path_bytes(path).to_vec())));
    let outputs = [Some(&ctx.args.output), ctx.args.map.as_ref(), ctx.args.sdk_imports.as_ref()];
    entries.extend(outputs.into_iter().flatten().map(|path| (0x40, dependency_path(path))));
    entries.sort();

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
    emit(0x00, format!("{}\n", crate::cmdline::VERSION_BANNER).as_bytes());
    for (op, path) in &entries {
        emit(*op, path);
    }
}

/// The -dependency_info file's inputs (see write_dependency_info).
fn dependency_inputs<E: Target>(ctx: &Context<E>) -> Vec<(u8, Vec<u8>)> {
    use crate::cmdline::InputArg;
    // The object LTO compiled is no input (a build system can't depend
    // on it), whatever -object_path_lto made of it, nor is the hook for
    // the classes of mergeable libraries (see bundle_hook).
    let mut named: Vec<&Path> = ctx
        .objs
        .iter()
        .enumerate()
        .filter(|&(i, o)| {
            o.is_alive && !ctx.is_internal(i) && !ctx.is_bundle_hook(i) && !ctx.is_lto_obj(i)
        })
        .map(|(_, o)| o.mf.parent.map_or(o.mf.name.as_path(), |p| p.name.as_path()))
        .collect();
    named.extend(ctx.visited_files.iter().map(PathBuf::as_path));
    named.extend(ctx.args.inputs.iter().filter_map(|arg| match arg {
        InputArg::BundleLoader(path) => Some(path.as_path()),
        _ => None,
    }));
    named.extend(ctx.args.filelists.iter().map(PathBuf::as_path));
    named.extend(ctx.args.sectcreate.iter().filter_map(|sc| sc.path.as_deref()));
    // A fat file's slice is the file's.
    let mut named: Vec<Vec<u8>> = named
        .into_iter()
        .map(|path| crate::input_files::without_fat_arch(path_bytes(path)))
        .collect();
    named.sort_unstable();
    named.dedup();
    let named: Vec<Vec<u8>> =
        named.iter().map(|path| dependency_path(Path::new(crate::util::os_str(path)))).collect();

    let mut reexports: Vec<Vec<u8>> =
        ctx.reexport_files.iter().map(|path| dependency_path(path)).collect();
    reexports.sort_unstable();
    reexports.dedup();
    reexports.retain(|path| !named.contains(path));
    let reexports = reexports.into_iter().flat_map(|path| [path.clone(), path]);
    named.into_iter().chain(reexports).map(|path| (0x10, path)).collect()
}

/// A path as the -dependency_info file has it: a relative one resolved
/// to the file's real path, if it exists, an absolute one as it is.
fn dependency_path(path: &Path) -> Vec<u8> {
    if !path.is_absolute()
        && let Ok(real) = std::fs::canonicalize(path)
    {
        return path_bytes(&real).to_vec();
    }
    path_bytes(path).to_vec()
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
/// lacks its "uuid" (and has a stray comma, that mold leaves out). A
/// file it can't write fails the link.
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
        crate::error!("call to mkpath_np({}) failed due to: Undefined error: 0", dir.display());
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

fn append_trace(path: &Path, record: &str) {
    let file = std::fs::OpenOptions::new().append(true).create(true).open(path);
    match file {
        Ok(mut file) => {
            let _ = file.write_all(format!("{record}\n").as_bytes());
        }
        Err(e) => crate::error!(
            "Could not open or create trace file (errno={}): {}",
            e.raw_os_error().unwrap_or(0),
            path.display()
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
    archives: Vec<(String, Vec<&'a str>)>,
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
        let mut members: hashbrown::HashMap<&Path, (bool, bool, Vec<&str>)> =
            hashbrown::HashMap::new();
        for obj in &ctx.objs {
            let Some(archive) = obj.mf.parent else { continue };
            let entry = members.entry(archive.name.as_path()).or_default();
            if !obj.is_alive {
                entry.1 = true;
                continue;
            }
            entry.0 = true;
            for (nlist, &sym) in obj.nlists.iter().zip(&obj.symbols) {
                if nlist.is_extern() && !nlist.is_stab() && matches!(nlist.n_type(), N_SECT | N_ABS)
                {
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

    /// The record of -trace_file, or with `shared_cache` of
    /// -trace_file_shared_cache.
    fn dylibs_json<E: Target>(&self, ctx: &Context<E>, uuid: &str, shared_cache: bool) -> String {
        let name = match ctx.args.output_type {
            MH_DYLIB if shared_cache => {
                crate::util::display(ctx.args.output_install_name()).to_string()
            }
            _ if shared_cache => ctx.args.output.to_string_lossy().into_owned(),
            _ => output_leaf(ctx),
        };
        let mut out = format!(
            "{{\"uuid\":\"{uuid}\",\"name\":{},\"arch\":\"{}\"",
            json_string(&name),
            E::NAME
        );
        let id = |d: &DylibFile| match shared_cache {
            true => json_string(&crate::util::display(&d.install_name)),
            false => json_string(&trace_path(&d.path)),
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
                out.push_str(&format!(",\"{key}\":[{}]", items.join(",")));
            }
        }
        if !shared_cache && !self.archives.is_empty() {
            let items: Vec<String> =
                self.archives.iter().map(|(path, _)| json_string(path)).collect();
            out.push_str(&format!(",\"archives\":[{}]", items.join(",")));
        }
        out.push('}');
        out
    }

    /// The record of -trace_symbols_file.
    fn symbols_json<E: Target>(&self, ctx: &Context<E>, uuid: Option<&str>) -> String {
        use std::fmt::Write;
        let args = &ctx.args;
        let mut out = format!(
            "{{ \"version\":\"2\", \"minor-version\":1, \"name\":{}",
            json_string(&output_leaf(ctx))
        );
        if args.output_type == MH_DYLIB {
            let eligible = if args.shared_region { "yes" } else { "no" };
            let _ = write!(
                out,
                ", \"install-name\":{}, \"shared-cache-eligible\":\"{eligible}\"",
                json_string(&crate::util::display(args.output_install_name()))
            );
        }
        if let Some(uuid) = uuid {
            let _ = write!(out, ", \"uuid\":\"{uuid}\"");
        }
        let version = args.platform_minos;
        let _ = write!(
            out,
            ", \"arch\":\"{}\", \"platforms\": [ {{ \"name\" : \"{}\", \"min-version\" : {{ \"major\": \"{}\", \"minor\": \"{}\" }} }} ]",
            E::NAME,
            crate::macho::platform_name(args.platform),
            version >> 16,
            (version >> 8) & 0xff
        );
        let spaced = |items: &[&str]| -> String {
            items.iter().map(|s| format!(" {}", json_string(s))).collect::<Vec<_>>().join(",")
        };
        let _ = write!(out, ", \"exports\": [{} ]", spaced(&own_exports(ctx)));

        let imports = dylib_imports(ctx);
        let list = |syms: &[&str]| -> String {
            let syms: Vec<String> = syms.iter().map(|s| json_string(s)).collect();
            format!("[ {} ]", syms.join(", "))
        };
        let mut entries = Vec::new();
        for &(i, d) in &self.dylibs {
            let mut attrs = Vec::new();
            if d.is_reexported {
                attrs.push("\"re-export\"");
            }
            if d.is_weak || d.is_weak_asserted {
                attrs.push("\"weak\"");
            }
            if d.is_upward {
                attrs.push("\"upward\"");
            }
            if d.delay_init.is_some() {
                attrs.push("\"delay-init\"");
            }
            let mut entry = format!(
                " {{ \"path\": {}, \"install-name\": {}, \"arch\": \"{}\", \"attributes\": [{} ], \"imported-symbols\": {}",
                json_string(&trace_path(&d.path)),
                json_string(&crate::util::display(&d.install_name)),
                E::NAME,
                attrs.join(", "),
                list(&imports[i])
            );
            if d.is_reexported {
                let mut exports: Vec<&str> = d.exports.iter().copied().collect();
                exports.sort_unstable();
                let _ = write!(entry, ",  \"exported-symbols\": {}", list(&exports));
            }
            entry.push_str(" }");
            entries.push(entry);
        }
        for &(i, d) in &self.lazy {
            entries.push(format!(
                " {{ \"arch\": \"{}\", \"path\": {}, \"install-name\": {}, \"attributes\": [ \"lazy-load\" ], \"imported-symbols\": {} }}",
                E::NAME,
                json_string(&trace_path(&d.path)),
                json_string(&crate::util::display(&d.install_name)),
                list(&imports[i])
            ));
        }
        let _ = write!(out, ", \"linked-dylibs\":[{} ]", entries.join(","));

        let archives: Vec<&str> = self.archives.iter().map(|(path, _)| path.as_str()).collect();
        let unused: Vec<&str> = self.unused_archives.iter().map(String::as_str).collect();
        let _ = write!(out, ", \"archives\": [{} ]", spaced(&archives));
        let _ = write!(out, ", \"unused-archives\": [{} ]", spaced(&unused));
        let linked: Vec<String> = self
            .archives
            .iter()
            .map(|(path, syms)| {
                let syms: Vec<String> = syms.iter().map(|s| json_string(s)).collect();
                format!(
                    "{{ \"arch\": \"{}\", \"path\": {},\"imported-symbols\":[{}]}}",
                    E::NAME,
                    json_string(path),
                    syms.join(",")
                )
            })
            .collect();
        let _ = write!(out, ",\"linked-archives\":[{}] }}", linked.join(","));
        out
    }
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
    crate::passes::real_path(path).0.to_string_lossy().into_owned()
}

/// The symbols a dylib output exports of its own definitions, sorted:
/// what its export trie lists but for re-exports.
fn own_exports<E: Target>(ctx: &Context<E>) -> Vec<&'static str> {
    if ctx.args.output_type != MH_DYLIB {
        return Vec::new();
    }
    let mut names: Vec<&str> = (0..ctx.symbols.syms.len() as SymbolId)
        .filter(|&id| {
            let sym = &ctx.symbols[id];
            matches!(sym.file(), Some(FileId::Obj(obj)) if !ctx.is_internal(obj as usize))
                && sym.is_extern()
                && !sym.is_private_extern()
                && sym
                    .input_section()
                    .is_none_or(|isec| ctx.isecs[ctx.resolve_isec(isec as usize)].is_alive())
                && !ctx.indirect_aliases.iter().any(|&(alias, _)| alias == id)
        })
        .map(|id| ctx.symbols[id].name())
        .collect();
    names.sort_unstable();
    names
}

/// The symbols the output imports from each dylib, sorted, by dylib.
fn dylib_imports<E: Target>(ctx: &Context<E>) -> Vec<Vec<&'static str>> {
    let mut imports: Vec<Vec<&str>> = vec![Vec::new(); ctx.dylibs.len()];
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

/// A line of the map's symbol list: a subsection's address and size,
/// the number of the file it came from, and its name (the bytes of a
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
/// the stubs, the unwind info, the hook for the classes of mergeable
/// libraries and such. A bitcode file is listed as any
/// object, and the object LTO compiled last of all; ld-prime credits
/// each of the latter's symbols to the bitcode file it came from, when
/// it can tell (see lto::origins).
struct MapFiles<'a> {
    paths: Vec<&'a Path>,
    /// The number of each object (0 for the internal one) and dylib.
    objs: Vec<usize>,
    dylibs: Vec<usize>,
    /// The number of the file each -sectcreate or -add_empty_section
    /// option stands for: ld-prime makes each a file of its own, named
    /// by the option's path ("(null)" for an empty section), in its
    /// place on the command line - but the first, whose section it
    /// takes for its own (file 0).
    sectcreate: Vec<usize>,
    /// The number of the merged library each symbol a dylib provides
    /// through one comes from.
    merged: hashbrown::HashMap<SymbolId, usize>,
    /// The object whose tentative definition each common symbol's
    /// subsection stands for, by subsection.
    commons: hashbrown::HashMap<u32, u32>,
    /// The objects LTO compiled, and the bitcode file each of their
    /// symbols comes from, by name.
    lto_objs: std::ops::Range<usize>,
    lto_origins: hashbrown::HashMap<&'static str, Option<usize>>,
}

impl<'a> MapFiles<'a> {
    fn new<E: Target>(ctx: &'a Context<E>) -> Self {
        enum File<'a> {
            Obj(usize),
            Dylib(usize),
            Merged(&'a MergedFile),
            Stripped(&'a Path),
            SectCreate(usize),
        }
        // A dylib that stands for a library exports moved to is no file:
        // its symbols count as the file's that moved them, which an
        // auto-linked one stands for where nothing binds to that.
        let is_moved = |dylib: &DylibFile| dylib.name_source == NameSource::Moved;
        let (provided, merged_only) = merged_providers(ctx);

        let mut named: Vec<(u32, File)> = Vec::new();
        let mut autolinked: Vec<((u32, u32), File)> = Vec::new();
        for (i, obj) in ctx.objs.iter().enumerate() {
            if !obj.is_alive || ctx.is_internal(i) || ctx.is_bundle_hook(i) || ctx.is_lto_obj(i) {
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
        for (i, &priority) in ctx.sectcreate_priority.iter().enumerate().skip(1) {
            named.push((priority, File::SectCreate(i)));
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
            sectcreate: vec![0; ctx.args.sectcreate.len()],
            merged: hashbrown::HashMap::new(),
            commons: crate::output_sections::common_owners(ctx),
            lto_objs: ctx.lto_objs.clone(),
            lto_origins: crate::lto::origins(&ctx.lto_inputs),
        };
        let mut merged_numbers: hashbrown::HashMap<&[u8], usize> = hashbrown::HashMap::new();
        let named = named.into_iter().map(|(_, file)| file);
        let implicit = implicit.into_iter().map(|(_, file)| file);
        let autolinked = autolinked.into_iter().map(|(_, file)| file);
        let lto = ctx.lto_objs.clone().map(File::Obj);
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
                File::SectCreate(i) => {
                    files.sectcreate[i] = number;
                    let path = ctx.args.sectcreate[i].path.as_deref();
                    files.paths.push(path.unwrap_or(Path::new("(null)")));
                }
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
            Some(&Some(origin)) if self.lto_objs.contains(&obj) => self.objs[origin],
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
        // section holding one linker-made subsection, which its
        // segment's load command doesn't list.
        if seg.name == "__UNIXSTACK" {
            let (addr, size) = (seg.cmd.vmaddr, seg.cmd.vmsize);
            sections.push(MapSection { addr, size, segname: "__UNIXSTACK", sectname: "__stack" });
        }
    }

    // The linker's symbols go first of those at one place, and an
    // executable's header first of all.
    let files = MapFiles::new(ctx);
    let mut entries = linker_symbol_entries(ctx, &files);
    let linker_symbols = entries.len();
    let (named, first_labels, literal_aliases) = symbol_entries(ctx, &files);
    entries.extend(named.entries);
    entries.extend(unnamed_entries(ctx, &files, &first_labels));
    entries.extend(eh_frame_entries(ctx, &files, &entries[linker_symbols..]));
    entries.extend(synthetic_entries(ctx, &files));
    let mut entries = sort_entries(ctx, entries, linker_symbols, &named.ids);
    insert_literal_aliases(&mut entries, literal_aliases);
    if ctx.args.output_type == MH_EXECUTE && !ctx.args.preload {
        let addr = ctx.mach_header.hdr.addr;
        entries.insert(0, MapEntry { addr, size: 0, file: 0, name: name("__mh_execute_header") });
    }
    write_map(ctx, path, &files, &sections, &entries, &dead_entries(ctx, &files));
}

/// The rows of the defined symbols (see symbol_entries), with the
/// symbols of those but the fixed-size literals', which follow.
struct NamedEntries<'a> {
    entries: Vec<MapEntry<'a>>,
    ids: Vec<SymbolId>,
}

/// Sorts the map's rows by address: the linker's symbols first of those
/// at one place (the first `linker_symbols` rows), then the row with
/// the size, then the labels of no size that alias it, in the symbol
/// table's order (see symtab::put_subsec_names_last); `ids` are the
/// symbols of the rows after the linker's.
fn sort_entries<'a, E: Target>(
    ctx: &Context<E>,
    entries: Vec<MapEntry<'a>>,
    linker_symbols: usize,
    ids: &[SymbolId],
) -> Vec<MapEntry<'a>> {
    let indices = &ctx.symtab.output_sym_indices;
    let place = |i: usize, e: &MapEntry| match ids.get(i.wrapping_sub(linker_symbols)) {
        _ if i < linker_symbols => (e.addr, 0, 0),
        _ if e.size > 0 => (e.addr, 1, e.size),
        Some(&id) => (e.addr, 2, indices.get(id as usize).copied().unwrap_or(u32::MAX) as u64),
        None => (e.addr, 2, 0),
    };
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| place(i, &entries[i]));
    let mut entries: Vec<Option<MapEntry>> = entries.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| entries[i].take()).collect()
}

/// Puts each of a literal's other labels right after the literal's own
/// row in `entries`, sorted by address (see literal_labels).
fn insert_literal_aliases<'a>(entries: &mut Vec<MapEntry<'a>>, aliases: Vec<MapEntry<'a>>) {
    for alias in aliases {
        let at = entries.partition_point(|e| e.addr <= alias.addr);
        entries.insert(at, alias);
    }
}

/// A record of a section a -r output makes itself, as its map lists it.
pub enum RelocatableRecord {
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
/// the subsections in them - the inputs', and the records of the
/// sections the output makes itself (`records`, by address). A record
/// of __compact_unwind is its object's, named by its label, if it has
/// one; the __objc_imageinfo is the linker's, as in an image.
pub fn print_relocatable_map<E: Target>(
    ctx: &Context<E>,
    sections: &[&crate::chunks::ChunkHeader],
    records: &[(u64, RelocatableRecord)],
) {
    let Some(path) = &ctx.args.map else { return };
    let sections: Vec<MapSection> = sections.iter().map(|hdr| MapSection::of(hdr)).collect();
    let files = MapFiles::new(ctx);
    let mut entries = linker_symbol_entries(ctx, &files);
    let linker_symbols = entries.len();
    let (named, first_labels, literal_aliases) = symbol_entries(ctx, &files);
    entries.extend(named.entries);
    entries.extend(unnamed_entries(ctx, &files, &first_labels));
    let synthetic = relocatable_record_entries(ctx, &files, &entries[linker_symbols..], records);
    entries.extend(synthetic);
    let mut entries = sort_entries(ctx, entries, linker_symbols, &named.ids);
    insert_literal_aliases(&mut entries, literal_aliases);
    write_map(ctx, path, &files, &sections, &entries, &[]);
}

/// The map's entries of the records of the sections a -r output makes
/// itself (see print_relocatable_map), whose FDEs are named after the
/// `named` symbols.
fn relocatable_record_entries<E: Target>(
    ctx: &Context<E>,
    files: &MapFiles,
    named: &[MapEntry],
    records: &[(u64, RelocatableRecord)],
) -> Vec<MapEntry<'static>> {
    let fde_names = FdeNames::new(named);
    let labels = unwind_labels(ctx);
    let mut entries = Vec::new();
    for &(addr, ref record) in records {
        let (size, obj, name) = match *record {
            RelocatableRecord::Unwind(i) => {
                let rec = &ctx.unwind_records[i];
                let label = labels.get(&(rec.isec, rec.input_offset)).copied();
                let obj = ctx.isecs[rec.isec as usize].file as usize;
                (32, Some(obj), Cow::Borrowed(label.unwrap_or("anon").as_bytes()))
            }
            RelocatableRecord::Cie(i) => {
                let cie = &ctx.cies[i];
                (cie.data.len() as u64, Some(cie.obj as usize), name("CFI"))
            }
            RelocatableRecord::Fde(i) => {
                let fde = &ctx.fdes[i];
                let func = ctx.isec_addr(fde.isec as usize) + fde.func_offset as u64;
                (fde.data.len() as u64, Some(fde.obj as usize), fde_names.name(func))
            }
            RelocatableRecord::ImageInfo => (8, None, name("anon")),
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
/// link (see MapFiles), the `sections`, the `entries` - the
/// subsections, in the order given - and the `dead` ones -dead_strip
/// took out.
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
    // them; sizes are the subsection extents they would have had.
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

/// Whether a symbol names its subsection in the map, as ld-prime names
/// subsections. An assembler temporary (L...) doesn't, nor does a
/// linker-private label (l...) of a fixed-size literal, such as the
/// compiler's lCPI0_0 constant-pool labels: the literal is known by its
/// size, as a C string is by its contents whatever labels it. The
/// entries of the sections ld-prime reads as lists of records -
/// CFStrings, UTF-16 strings, selector and class references,
/// Objective-C class and category lists - are named by no local symbol:
/// they are "anon", as unnamed subsections are. (An ltmpN label may be
/// shadowed besides; see drop_shadowed_ltmps.)
fn is_named<E: Target>(ctx: &Context<E>, sym: &crate::symbol::Symbol) -> bool {
    let isec = &ctx.isecs[ctx.resolve_isec(sym.input_section().unwrap() as usize)];
    let split = ctx.objs[isec.file as usize].subsections_via_symbols;
    names_subsec(ctx.hdr_of(isec), split, sym.is_extern(), sym.name())
}

/// is_named for a label of a section with header `hdr`, of an object
/// with subsections or not (`split`).
fn names_subsec(hdr: &MachSection, split: bool, is_extern: bool, name: &str) -> bool {
    if name.is_empty()
        || hdr.section_type() == S_CSTRING_LITERALS
        || crate::input_files::is_ignored_literal_label(hdr.section_type(), name)
    {
        return false;
    }
    is_extern || (!crate::input_files::is_record_list(hdr, split) && !name.starts_with('L'))
}

/// Whether a defined symbol of an object names its subsection in the
/// map: is_named, and not an ltmpN label another symbol there shadows
/// (see drop_shadowed_ltmps), its subsection in the output.
pub(crate) fn names_its_subsec<E: Target>(ctx: &Context<E>, id: SymbolId) -> bool {
    let sym = &ctx.symbols[id];
    let (Some(FileId::Obj(obj)), Some(own)) = (sym.file(), sym.input_section()) else {
        return false;
    };
    let isec = ctx.resolve_isec(own as usize);
    if !ctx.isecs[isec].is_alive()
        || crate::chunks::symtab::is_coalesced_away(ctx, own as usize)
        || !is_named(ctx, sym)
    {
        return false;
    }
    if !sym.name().starts_with("ltmp") {
        return true;
    }
    let mut at: Vec<SymbolId> = ctx.objs[obj as usize]
        .symbols
        .iter()
        .copied()
        .filter(|&other| {
            let other = &ctx.symbols[other];
            other.file() == sym.file()
                && other.input_section() == sym.input_section()
                && other.value == sym.value
                && is_named(ctx, other)
        })
        .collect();
    drop_shadowed_ltmps(ctx, &mut at, |&sym| sym);
    at.contains(&id)
}

/// Drops from the map's named symbols each ltmpN label another of them
/// shares a place with. An arm64 assembler puts the label where each
/// section starts; where symbols split the sections, a subsection there
/// takes the label's name only if it has no other, as ld-prime ranks
/// the labels a subsection may be named after (without subsections the
/// section's first subsection has its name, which the other symbols
/// there alias).
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

/// How a symbol ranks to name its subsection among the symbols at its
/// place, lowest first: as ld-prime ranks the labels at a subsection's
/// start (see subsec_name_rank) - _zb names the subsection of `_zb: _ab:
/// lc:`, _loc5 that of a weak definition _wd it labels too -, but in an
/// object without subsections, where the first in the symbol table
/// names it (an ltmpN label before an exported function).
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
    use crate::input_files::subsec_name_rank;
    match obj.subsections_via_symbols {
        true => (subsec_name_rank(&obj.nlists[k as usize], name), name, std::cmp::Reverse(k)),
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
/// the subsection's end) - the size of the subsection ld64 splits off
/// at it; of several at one place, the one naming the subsection has
/// the size (see naming_rank) and the others are aliases of none, as is
/// an alternate entry point (Swift's type metadata inside its full
/// metadata). ld-prime counts a thread-local variable's descriptor,
/// which it rewrites, as its own, and a common symbol as the object's
/// whose tentative definition won. It credits itself with a subsection
/// it rewrote (a method list in the relative form), with the alias it
/// makes of a function folded into an identical one (-deduplicate),
/// which has no size, and with the names -alias gives; an ltmpN label
/// of a weak definition another file's won names nothing. Also returns
/// the symbols of the entries but those of fixed-size literals, which
/// come last, where in its subsection the first symbol is, by
/// subsection, and the rows of the labels of fixed-size literals that
/// follow the literals' own (see literal_labels).
fn symbol_entries<'a, E: Target>(
    ctx: &'a Context<E>,
    files: &MapFiles,
) -> (NamedEntries<'a>, hashbrown::HashMap<usize, u64>, Vec<MapEntry<'a>>) {
    use crate::chunks::symtab::is_coalesced_away;
    let nlists = defining_nlists(ctx);
    // The names -alias gives that no object defines itself.
    let aliases: hashbrown::HashSet<&str> =
        ctx.args.aliases.iter().map(|(_, alias)| alias.as_str()).collect();
    let is_alias_name = |obj: u32, id: SymbolId| {
        aliases.contains(ctx.symbols[id].name())
            && !nlists
                .get(&id)
                .is_some_and(|&(k, _)| ctx.objs[obj as usize].nlists[k as usize].n_type() == N_SECT)
    };
    let mut syms: Vec<(SymbolId, usize)> = Vec::new();
    let mut literal_syms: Vec<SymbolId> = Vec::new();
    for i in 0..ctx.symbols.syms.len() as SymbolId {
        let sym = &ctx.symbols[i];
        let (Some(FileId::Obj(obj)), Some(own)) = (sym.file(), sym.input_section()) else {
            continue;
        };
        let isec = ctx.resolve_isec(own as usize);
        if !ctx.isecs[isec].is_alive() {
            continue;
        }
        if isec == own as usize && is_split_fixed_literal(ctx, isec) {
            literal_syms.push(i);
            continue;
        }
        if !is_named(ctx, sym) {
            continue;
        }
        // Of the name of the function a folded one folded into, ld-prime
        // lists one of each scope (see icf::folded_subsec_names).
        let folded = is_coalesced_away(ctx, own as usize);
        if folded
            && (sym.name().starts_with("ltmp") || ctx.folded_subsec_names.get(&i) == Some(&true))
        {
            continue;
        }
        // The alias ld-prime makes of a folded function is its own, but
        // the function's other labels stay their file's.
        let file = match files.commons.get(&(isec as u32)) {
            _ if folded && ctx.folded_subsec_names.contains_key(&i) => 0,
            _ if !aliases.is_empty() && is_alias_name(obj, i) => 0,
            _ if ctx.hdr_of(&ctx.isecs[isec]).section_type() == S_THREAD_LOCAL_VARIABLES => 0,
            Some(&owner) => files.objs[owner as usize],
            None if is_rewritten_method_list(ctx, isec) => 0,
            None => files.of_object(obj as usize, sym.name()),
        };
        syms.push((i, file));
    }
    drop_shadowed_ltmps(ctx, &mut syms, |&(sym, _)| sym);

    // Sizes: sort the symbols by place, the one naming the subsection
    // last of those at one, and measure to the next place.
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
    let mut entries: Vec<MapEntry> = syms
        .iter()
        .zip(sizes)
        .map(|(&(sym, file), size)| MapEntry {
            addr: ctx.sym_addr(sym),
            size,
            file,
            name: name(ctx.symbols[sym].name()),
        })
        .collect();
    let ids = syms.iter().map(|&(sym, _)| sym).collect();
    let aliases =
        literal_labels(ctx, files, &nlists, literal_syms, &mut entries, &mut first_labels);
    (NamedEntries { entries, ids }, first_labels, aliases)
}

/// Whether a subsection is a fixed-size literal (4, 8 or 16 bytes) of an
/// object with subsections, which ld-prime names its own way (see
/// literal_labels).
fn is_split_fixed_literal<E: Target>(ctx: &Context<E>, isec: usize) -> bool {
    let isec = &ctx.isecs[isec];
    matches!(
        ctx.hdr_of(isec).section_type(),
        S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS
    ) && ctx.objs[isec.file as usize].subsections_via_symbols
}

/// The rows of the labels of fixed-size literals (see
/// is_split_fixed_literal): of a literal's labels, the best (see
/// naming_rank) names the literal - unless it is linker-private
/// (lCPI0_0), which leaves the literal known by its size - and each
/// other label, linker-private or not, has a row of no size after the
/// literal's, the better first. (An ltmpN label is none.) Adds the
/// literals' rows to `entries`, noting them in `first_labels`, and
/// returns the others.
fn literal_labels<'a, E: Target>(
    ctx: &'a Context<E>,
    files: &MapFiles,
    nlists: &hashbrown::HashMap<SymbolId, (u32, bool)>,
    syms: Vec<SymbolId>,
    entries: &mut Vec<MapEntry<'a>>,
    first_labels: &mut hashbrown::HashMap<usize, u64>,
) -> Vec<MapEntry<'a>> {
    let mut labels: Vec<(usize, u64, std::cmp::Reverse<LabelRank>, SymbolId)> = (syms.into_iter())
        .filter(|&id| {
            let name = ctx.symbols[id].name();
            !name.is_empty() && !name.starts_with("ltmp") && !name.starts_with('L')
        })
        .map(|id| {
            let (sym, rank) = (&ctx.symbols[id], naming_rank(ctx, nlists, id));
            (sym.input_section().unwrap() as usize, sym.value, std::cmp::Reverse(rank), id)
        })
        .collect();
    labels.sort_unstable();
    let mut aliases = Vec::new();
    for place in labels.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
        let (isec, value, _, best) = place[0];
        let row = |id: SymbolId, size: u64| {
            let Some(FileId::Obj(obj)) = ctx.symbols[id].file() else { unreachable!() };
            let file = files.of_object(obj as usize, ctx.symbols[id].name());
            MapEntry { addr: ctx.sym_addr(id), size, file, name: name(ctx.symbols[id].name()) }
        };
        if !crate::input_files::is_private_label(ctx.symbols[best].name()) {
            entries.push(row(best, ctx.isecs[isec].size as u64 - value));
            first_labels.insert(isec, value);
        }
        aliases.extend(place[1..].iter().map(|&(.., id)| row(id, 0)));
    }
    aliases
}

/// Whether a subsection is an Objective-C method list the linker
/// rewrote in the relative form.
pub(crate) fn is_rewritten_method_list<E: Target>(ctx: &Context<E>, isec: usize) -> bool {
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
/// "anon" - one per record of a section of fixed-size records, which
/// ld-prime splits into a subsection per record (an __objc_classlist
/// listing two classes is two).
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

/// The subsections of the input files no symbol names, up to the first
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
/// category's +load made non-lazy is file 0's, as is a merged property
/// list, which no symbol names.
fn objc_list_entries<'a, E: Target>(ctx: &'a Context<E>, files: &MapFiles) -> Vec<MapEntry<'a>> {
    use crate::objc::{DataField, ObjcRef};
    let mut entries: Vec<MapEntry> = (ctx.objc_property_lists.iter())
        .map(|&isec| {
            let (addr, size) = (ctx.isec_addr(isec as usize), ctx.isecs[isec as usize].size as u64);
            MapEntry { addr, size, file: 0, name: name("anon") }
        })
        .collect();
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
/// name of the function's subsection.
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

/// How the map names an FDE: "FDE for: " and the name of the
/// subsection of its function, which of the `named` symbols at its
/// place is listed first (has the size).
struct FdeNames<'a> {
    /// The name of the subsection at each address, with its size.
    names: hashbrown::HashMap<u64, (u64, &'a [u8])>,
}

impl<'a> FdeNames<'a> {
    fn new(named: &'a [MapEntry<'_>]) -> Self {
        let mut names: hashbrown::HashMap<u64, (u64, &[u8])> = hashbrown::HashMap::new();
        for e in named {
            let best = names.entry(e.addr).or_insert((e.size, &e.name));
            if e.size < best.0 {
                *best = (e.size, &e.name);
            }
        }
        Self { names }
    }

    fn name(&self, func: u64) -> Cow<'static, [u8]> {
        let func = self.names.get(&func).map_or(&b"anon"[..], |&(_, name)| name);
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
/// __mh_execute_header comes first whether or not anything does. Each
/// -sectcreate or -add_empty_section option's input section is listed
/// (see sectcreate_entries); the empty section a boundary symbol makes
/// is named by the section's name alone.
fn linker_symbol_entries<'a, E: Target>(
    ctx: &'a Context<E>,
    files: &MapFiles,
) -> Vec<MapEntry<'a>> {
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
    entries.extend(sectcreate_entries(ctx, files));
    for sec in ctx.sectcreate_sections.iter().filter(|sec| !sec.from_option) {
        let hdr = &sec.hdr;
        let name = format!("{},{}", hdr.segname, hdr.sectname).into_bytes();
        entries.push(MapEntry { addr: hdr.addr, size: 0, file: 0, name: Cow::Owned(name) });
    }
    entries
}

/// The input sections of -sectcreate and -add_empty_section, each
/// named "l<sect-create>" and its section's name as the option spelled
/// it, of the file the option stands for - empty ones at one address
/// in section and file order.
fn sectcreate_entries<E: Target>(ctx: &Context<E>, files: &MapFiles) -> Vec<MapEntry<'static>> {
    let mut inputs: Vec<(u64, u8, u32, usize)> = (ctx.sectcreate_inputs.iter().enumerate())
        .map(|(i, input)| {
            let (addr, n_sect) = input.place(ctx);
            (addr, n_sect, crate::chunks::sectcreate::file_priority(ctx, i), i)
        })
        .collect();
    inputs.sort_unstable();
    (inputs.into_iter())
        .map(|(addr, .., i)| {
            let sc = &ctx.args.sectcreate[i];
            let name = format!("l<sect-create>{},{}", sc.segname, sc.sectname).into_bytes();
            let (size, file) = (ctx.sectcreate_inputs[i].size, files.sectcreate[i]);
            MapEntry { addr, size, file, name: Cow::Owned(name) }
        })
        .collect()
}

/// The subsections the linker makes, file 0's but for the stubs and
/// pointers it makes for a symbol: a stub, a GOT slot or a lazy pointer
/// counts as the file that defines the symbol, and takes its name with
/// ".stub", ".got" or ".lazy_ptr" after it. The stub helper's entries
/// are anonymous.
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

    // The lazy-load helpers are file 0's, as is the empty subsection
    // that keeps __dyld_lazy_load alive, and each slot its symbol's
    // file's.
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
    // The DOF of each provider of DTrace probes, which the symbol table
    // doesn't name.
    for dof in &ctx.dof_sections {
        let (addr, size) = (ctx.isec_addr(dof.isec as usize), ctx.isecs[dof.isec].size as u64);
        entries.push(MapEntry { addr, size, file: 0, name: name(&dof.subsec_name) });
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

/// The subsections of the input files that the output doesn't have,
/// which ld-prime lists under -dead_strip, whatever took them out: dead
/// stripping, coalescing - a literal or an Objective-C reference equal
/// to another file's, a weak definition another file's won, a function
/// folded into an identical one, a tentative definition (a common
/// symbol) a larger or a real one took the place of - or an
/// Objective-C rewrite (a method list in the relative form, a category
/// merged into its class). They come by the file they came from, in
/// the order they were in it, its tentative definitions last; the
/// sizes are the subsections', as for the live ones (see
/// dead_entries_of).
fn dead_entries<'a, E: Target>(ctx: &'a Context<E>, files: &MapFiles) -> Vec<MapEntry<'a>> {
    use rayon::prelude::*;
    if !ctx.args.dead_strip {
        return Vec::new();
    }
    let gone = GoneSubsecs::new(ctx);
    let mut dead: Vec<(usize, DeadKey, MapEntry)> = (0..ctx.objs.len())
        .into_par_iter()
        .filter(|&i| ctx.objs[i].is_alive && !ctx.is_internal(i))
        .flat_map_iter(|i| {
            let file = files.objs[i];
            dead_entries_of(ctx, &gone, &files.commons, i, file).into_iter().map(
                move |(key, mut entry)| {
                    // What LTO compiled goes by the bitcode file it is
                    // credited to (see MapFiles::of_object).
                    if ctx.is_lto_obj(i)
                        && let Ok(name) = std::str::from_utf8(&entry.name)
                    {
                        entry.file = files.of_object(i, name);
                    }
                    (entry.file, key, entry)
                },
            )
        })
        .collect();
    // A class category merging gave a list of a kind it had none of
    // has, after its file's other dead rows, one for each such list.
    for (i, &obj) in ctx.objc_filled_ro_fields.iter().enumerate() {
        let file = files.objs[obj as usize];
        let entry = MapEntry { addr: 0, size: 8, file, name: name("anon") };
        dead.push((file, (u64::MAX, u32::MAX, i as u32), entry));
    }
    dead.sort_by_key(|&(file, key, _)| (file, key));
    let mut dead: Vec<MapEntry> = dead.into_iter().map(|(_, _, entry)| entry).collect();
    dead.extend(dead_header_entries(ctx));
    dead
}

/// The linker's own symbols dead stripping removed, last among the dead:
/// the names of the mach header only stripped code used, and - when
/// nothing kept refers to the header in an image whose header is no
/// root - the start of __TEXT they name.
fn dead_header_entries<E: Target>(ctx: &Context<E>) -> Vec<MapEntry<'static>> {
    let header_root = ctx.args.output_type == MH_EXECUTE && !ctx.args.preload;
    let mut names: Vec<&'static str> = ctx
        .dead_header_names
        .iter()
        .map(|&id| ctx.symbols[id].name())
        .filter(|&n| !(header_root && n == "__mh_execute_header"))
        .collect();
    names.sort();
    let header_live = crate::dead_strip::HEADER_NAMES.iter().any(|n| {
        ctx.symbols.get(n).is_some_and(|id| {
            let sym = &ctx.symbols[id];
            sym.is_used()
                && matches!(sym.file(), Some(FileId::Obj(o)) if ctx.is_internal(o as usize))
        })
    });
    if !names.is_empty() && !header_root && !header_live {
        names.insert(0, "segment$start$__TEXT");
    }
    names.into_iter().map(|n| MapEntry { addr: 0, size: 0, file: 0, name: name(n) }).collect()
}

/// Where a dead row was in its file: its address in the object, the
/// subsection and the row's place among those of the subsection.
type DeadKey = (u64, u32, u32);

/// Which input subsections the output doesn't have (see dead_entries):
/// those dead stripping or an Objective-C rewrite took out and those
/// another took the place of, but for a class's ro data the merged
/// record stands for (ld-prime rewrites it in place) - a category list
/// rebuilt in its place is gone all the same, its merged entries dead
/// (see is_kept_category_entry); and the C strings objc stubs take
/// their selector names from, for which ld-prime makes its own.
/// ld-prime makes no subsections of a section the link consumes, or of
/// one -remove_swift_reflection_metadata_sections drops as it reads it.
struct GoneSubsecs {
    /// The records the Objective-C passes wrote for input ones.
    rewritten: hashbrown::HashSet<u32>,
    stub_names: hashbrown::HashSet<usize>,
}

impl GoneSubsecs {
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
            let list = matches!(hdr.sectname(), "__objc_catlist" | "__objc_nlcatlist");
            return list || !self.rewritten.contains(&isec.replacement);
        }
        !isec.is_alive() || self.stub_names.contains(&id)
    }
}

/// A label of a gone subsection (see dead_entries_of).
struct DeadLabel {
    isec: usize,
    off: u64,
    /// How the label ranks to name the subsection, the best first.
    rank: std::cmp::Reverse<LabelRank>,
    name: &'static str,
    /// An alternate entry point (N_ALT_ENTRY), an alias of none.
    alt: bool,
}

/// The labels naming the gone subsections of an object (see
/// dead_entries_of), by subsection, the alternate entry points last,
/// and by place, the one naming the subsection first; and how many
/// labels each C string has at its start. An ltmpN label of an empty
/// section names nothing (ld-prime makes no subsection of the section),
/// nor does one of a C string in an object with subsections.
fn gone_labels<E: Target>(
    ctx: &Context<E>,
    gone: &GoneSubsecs,
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
            || !names_subsec(hdr, split, nlist.is_extern(), name)
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

/// The dead subsections of object `obj_idx` (see dead_entries), the
/// `file`th of the map, keyed by where they were. A subsection is named
/// by the best of the labels at its start, as ld-prime ranks them (see
/// subsec_name_rank) - in an object without subsections, by the first
/// in its symbol table -, which has the subsection's size; the others
/// are aliases of none, but for linker-private ones (l...), which the
/// list leaves out. ld-prime makes a subsection of a C string per label
/// at its start, and all but one of them are always dead, merged into
/// that one.
fn dead_entries_of<'a, E: Target>(
    ctx: &'a Context<E>,
    gone: &GoneSubsecs,
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

    // The unnamed rows, from a gone subsection's start up to its first
    // label, and the C strings' extra subsections.
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
/// other subsections (see dead_entries_of): all but the one whose common
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
