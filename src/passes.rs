//! The linker passes, in the order the driver runs them.

use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::chunks::sectcreate::SectCreateSection;
use crate::chunks::symtab::SymtabSection;
use crate::chunks::{
    self, ChunkId, OutputSection, OutputSectionId, OutputSegment, Tail, mach_header_size,
};
use crate::cmdline::InputArg;
use crate::context::Context;
use crate::error;
use crate::fatal;
use crate::filetype::{FileType, get_file_type};
use crate::input_files;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, RelocTarget};
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::tapi;
use crate::target::RelocClass;
use crate::target::Target;
use crate::util::{align_to, leak_bytes, path_bytes};

/// Returns the directories to search for `-l` libraries, in order. An
/// absolute library path that exists under a syslibroot is looked up
/// there; the default search path is the syslibroot's /usr/lib.
pub(crate) fn library_search_dirs<E: Target>(ctx: &Context<E>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for dir in &ctx.args.library_paths {
        push_search_dir(&ctx.args.syslibroot, &mut dirs, dir);
    }

    if !ctx.args.no_standard_dirs {
        if ctx.args.syslibroot.is_empty() {
            dirs.push(PathBuf::from("/usr/lib"));
        } else {
            for root in &ctx.args.syslibroot {
                dirs.push(root.join("usr/lib"));
            }
        }
    }
    dirs
}

/// Adds a -L or -F directory to a search path. ld64 looks an absolute
/// directory up under each syslibroot, keeping those that exist and
/// falling back to the directory itself. A relative directory is never
/// put under a syslibroot, which the compiler driver always passes:
/// `-L.` would otherwise search the SDK's root, not the working
/// directory.
fn push_search_dir(syslibroot: &[PathBuf], dirs: &mut Vec<PathBuf>, dir: &Path) {
    if dir.is_absolute() {
        let len = dirs.len();
        dirs.extend(syslibroot.iter().map(|root| under_root(root, dir)).filter(|p| p.is_dir()));
        if dirs.len() > len {
            return;
        }
    }
    dirs.push(dir.to_path_buf());
}

/// `dir` looked up under a syslibroot: an absolute directory keeps its
/// path below the root.
fn under_root(root: &Path, dir: &Path) -> PathBuf {
    let mut relative = path_bytes(dir);
    while let Some(rest) = relative.strip_prefix(b"/") {
        relative = rest;
    }
    root.join(crate::util::os_str(relative))
}

/// Returns the directories to search for `-framework`, in order,
/// mirroring the library search rules.
fn framework_search_dirs<E: Target>(ctx: &Context<E>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for dir in &ctx.args.framework_paths {
        push_search_dir(&ctx.args.syslibroot, &mut dirs, dir);
    }

    if !ctx.args.no_standard_dirs {
        if ctx.args.syslibroot.is_empty() {
            dirs.push(PathBuf::from("/System/Library/Frameworks"));
            dirs.push(PathBuf::from("/Library/Frameworks"));
        } else {
            for root in &ctx.args.syslibroot {
                dirs.push(root.join("System/Library/Frameworks"));
                dirs.push(root.join("Library/Frameworks"));
            }
        }
    }
    dirs
}

fn find_framework<E: Target>(ctx: &Context<E>, name: &OsStr) -> Option<PathBuf> {
    let with_suffix = |suffix: &str| {
        let mut file = name.to_os_string();
        file.push(suffix);
        file
    };
    for dir in framework_search_dirs(ctx) {
        let fw = dir.join(with_suffix(".framework"));
        for file in [with_suffix(".tbd"), name.to_os_string()] {
            let path = fw.join(file);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn find_library<E: Target>(ctx: &Context<E>, name: &OsStr) -> Option<PathBuf> {
    // By default each directory is tried for a dylib and then an
    // archive before moving on (-search_paths_first, ld64's default
    // since Xcode 4). -search_dylibs_first restores the older ld64
    // behavior: a dylib anywhere on the path beats an archive
    // anywhere.
    // A -static link can use no dylib, so it looks for archives only.
    let passes: &[&[&str]] = if ctx.args.static_link {
        &[&["a"]]
    } else if ctx.args.search_dylibs_first {
        &[&["tbd", "dylib"], &["a"]]
    } else {
        &[&["tbd", "dylib", "a"]]
    };
    for exts in passes {
        for dir in library_search_dirs(ctx) {
            for ext in *exts {
                let mut file = OsString::from("lib");
                file.push(name);
                file.push(format!(".{ext}"));
                let path = dir.join(file);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// A parsed-input request: a file to stage as an object, with its
/// liveness and input-order priority.
struct PendingObject {
    mf: &'static MappedFile,
    alive: bool,
    hidden: bool,
    priority: u32,
}

/// Gives a dylib what one naming of it says. Namings add up: one
/// -needed_* keeps the load command under -dead_strip_dylibs, one
/// -reexport_* re-exports it, and one -weak_* makes every import from
/// it weak, in any order. A library so far loaded only as a public
/// re-export of another (Foundation's stub brings CoreFoundation) got
/// its weakness from that parent; its first command-line naming decides
/// it instead - `-weak_framework Foundation -framework CoreFoundation`
/// imports from CoreFoundation strongly - while an auto-link option,
/// a hint, changes nothing. -needed_* covers the named library only;
/// the ones its stub re-exports get a load command only if something
/// binds to them.
fn name_dylib(dylib: &mut input_files::DylibFile, rc: ReaderContext) {
    if dylib.is_implicit && !rc.autolinked {
        dylib.is_weak = rc.weak;
    } else {
        dylib.is_weak |= rc.weak;
    }
    dylib.is_reexported |= rc.reexport;
    dylib.is_needed |= rc.needed;
    dylib.is_implicit = false;
}

/// How an input was named: the flags its option gives the file, as
/// mold's ReaderContext carries --as-needed and --whole-archive.
#[derive(Clone, Copy, Default)]
struct ReaderContext {
    /// -force_load: every archive member is live.
    force_load: bool,
    /// -weak_library, -weak-l, -weak_framework: the imports are weak.
    weak: bool,
    /// -reexport_library, -reexport-l, -reexport_framework.
    reexport: bool,
    /// -hidden-l: the archive's definitions are not exported.
    hidden: bool,
    /// -needed_library, -needed-l, -needed_framework.
    needed: bool,
    /// Named by an object's auto-link option: a hint.
    autolinked: bool,
}

/// Reports a dylib that does not let this link name it directly (see
/// input_files::is_allowed_client): an error on the command line, while
/// the library an auto-link option names is left out with a warning.
fn refuses_client<E: Target>(ctx: &Context<E>, mf: &'static MappedFile, rc: ReaderContext) -> bool {
    let id = input_files::dylib_identity::<E>(mf);
    if input_files::is_allowed_client(ctx, &id) {
        return false;
    }
    let leaf = id.install_name.rsplit(|&b| b == b'/').next().unwrap_or(&[]);
    let msg = format!(
        "cannot link directly with '{}' because product being built is not an allowed client of it",
        crate::util::display(leaf)
    );
    if !rc.autolinked {
        error!("{msg}");
    } else if input_files::provides_undefined(ctx, mf) {
        // ld-prime opens an auto-linked library only for a symbol still
        // undefined, so it says nothing of one that would provide none.
        crate::warn!("Could not parse or use implicit file '{}': {msg}", mf.name.display());
    }
    true
}

/// Classifies one input file. Dylib stubs and binaries are registered
/// immediately (they are cheap and order-sensitive); objects and
/// archive members are queued for parallel staging; bitcode is
/// registered immediately since libLTO calls are kept on one thread.
fn collect_file<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    rc: ReaderContext,
    out: &mut Vec<PendingObject>,
) {
    // A library may be named more than once, on the command line and by
    // auto-link options; load each file once, and let a dylib take on
    // what every naming says.
    if !ctx.visited_files.insert(mf.name.clone()) {
        if let Some(dylib) = ctx.dylibs.iter_mut().find(|d| d.path == mf.name) {
            name_dylib(dylib, rc);
        }
        return;
    }
    match get_file_type(mf) {
        FileType::Object => {
            let priority = ctx.next_priority();
            out.push(PendingObject { mf, alive: true, hidden: rc.hidden, priority });
        }
        // A relocatable output keeps every reference undefined for the
        // final link, so a dylib named on its command line is ignored
        // with ld64's warning.
        FileType::Tapi if ctx.args.relocatable => {
            crate::warn!("{}, ignoring unexpected dylib text stub file", mf.name.display());
        }
        FileType::Dylib if ctx.args.relocatable => {
            crate::warn!("{}, ignoring unexpected dylib file", mf.name.display());
        }
        // A -static image has no dyld to load a dylib with.
        FileType::Tapi | FileType::Dylib if ctx.args.static_link => {
            crate::warn!("ignoring unexpected dylib '{}'", mf.name.display());
        }
        FileType::Tapi | FileType::Dylib if refuses_client(ctx, mf, rc) => {}
        FileType::Tapi | FileType::Dylib => {
            let first = ctx.dylibs.len();
            let idx = if get_file_type(mf) == FileType::Tapi {
                input_files::parse_dylib(ctx, mf)
            } else {
                input_files::parse_dylib_binary(ctx, mf)
            };
            // The dylibs loaded during the parse beyond this one are the
            // public libraries it re-exports; a weak parent's are weak.
            for d in &mut ctx.dylibs[first..] {
                d.is_weak |= rc.weak;
            }
            let dylib = &mut ctx.dylibs[idx];
            name_dylib(dylib, rc);
            // Ordered by naming sequence.
            if dylib.load_order == u32::MAX {
                dylib.load_order = ctx.dylib_load_seq;
                ctx.dylib_load_seq += 1;
            }
        }
        FileType::Archive => {
            // Every member is parsed eagerly; whether it is *live* -
            // whether its content reaches the output - is decided by
            // symbol resolution and the liveness walk. -all_load and
            // -force_load make every member live up front; -ObjC does
            // so for members with Objective-C metadata, which register
            // classes by their mere presence. ld64 exempts clang's
            // runtime library (libclang_rt.*.a, which the compiler
            // driver adds to every link) from -all_load: its members
            // are wanted only when referenced.
            let all_load = ctx.args.all_load
                && !mf.name.file_name().is_some_and(|f| f.as_bytes().starts_with(b"libclang_rt"));
            for member in crate::archive_file::read_archive_members(mf) {
                let alive = rc.force_load
                    || all_load
                    || (ctx.args.load_objc && input_files::has_objc_sections(member));
                match get_file_type(member) {
                    FileType::LlvmBitcode => {
                        input_files::parse_bitcode(ctx, member, alive);
                    }
                    _ => {
                        let priority = ctx.next_priority();
                        out.push(PendingObject { mf: member, alive, hidden: rc.hidden, priority });
                    }
                }
            }
        }
        FileType::Fat => {
            let slice = input_files::get_fat_slice::<E>(mf);
            collect_file(ctx, slice, rc, out);
        }
        FileType::LlvmBitcode => {
            input_files::parse_bitcode(ctx, mf, true);
        }
        FileType::Empty => {}
        _ => fatal!("{}: unknown file type", mf.name.display()),
    }
}

/// Stages the queued object files in parallel and integrates them in
/// input order - the parallel front end of the mold design.
fn load_pending<E: Target>(ctx: &mut Context<E>, pending: Vec<PendingObject>) {
    let relocatable = ctx.args.relocatable;
    let keep_all_fdes = relocatable || ctx.args.static_link;
    let t = ctx.timer("stage");
    let staged: Vec<input_files::StagedObject> = pending
        .par_iter()
        .map(|p| {
            input_files::stage_object::<E>(
                p.mf,
                p.alive,
                p.hidden,
                p.priority,
                relocatable,
                keep_all_fdes,
            )
        })
        .collect();
    drop(t);

    // Intern every staged object's global names in one parallel batch
    // (mold's sharded symbol table), so the serial integration loop
    // below does no hashing. Names carry the xxh3 hashes staging
    // computed alongside them, so nothing here touches their bytes.
    // Each object's extern (name, hash) list is filtered in parallel -
    // a debug link scans millions of nlists here - then concatenated in
    // object order into the batch the sharded intern resolves at once.
    let per_obj: Vec<Vec<(&'static str, u64)>> = staged
        .par_iter()
        .map(|st| {
            let r = st.global_range();
            st.sym_names[r.clone()]
                .iter()
                .zip(&st.sym_hashes[r.clone()])
                .zip(st.nlists[r].iter())
                .filter(|((_, _), nlist)| !nlist.is_stab() && nlist.is_extern())
                .map(|((&name, &hash), _)| (name, hash))
                .collect()
        })
        .collect();
    let counts: Vec<usize> = per_obj.iter().map(Vec::len).collect();
    let mut batch: Vec<(&'static str, u64)> = Vec::with_capacity(counts.iter().sum());
    for v in per_obj {
        batch.extend(v);
    }
    let t = ctx.timer("gather");
    let ids = ctx.symbols.gather(&batch);
    drop(t);

    let t = ctx.timer("integrate");
    input_files::integrate_objects(ctx, staged, ids, counts);
    drop(t);
}

pub fn read_input_files<E: Target>(ctx: &mut Context<E>) {
    // ld64 warns, once, about the library options given more than once,
    // each as spelled (-weak-lz repeats no -lz); build systems that
    // knowingly repeat them pass -no_warn_duplicate_libraries.
    if ctx.args.warn_duplicate_libraries {
        let mut seen = std::collections::HashSet::new();
        let mut dups = std::collections::BTreeSet::new();
        for arg in &ctx.args.inputs {
            let (option, name) = match arg {
                InputArg::Lib(name, false) => ("-l", name),
                InputArg::Lib(name, true) => ("-weak-l", name),
                InputArg::NeededLib(name) => ("-needed-l", name),
                InputArg::ReexportLib(name) => ("-reexport-l", name),
                InputArg::HiddenLib(name) => ("-hidden-l", name),
                _ => continue,
            };
            let spelled = format!("'{option}{}'", name.display());
            if !seen.insert(spelled.clone()) {
                dups.insert(spelled);
            }
        }
        if !dups.is_empty() {
            let list: Vec<String> = dups.into_iter().collect();
            crate::warn!("ignoring duplicate libraries: {}", list.join(", "));
        }
    }
    let inputs = std::mem::take(&mut ctx.args.inputs);

    // Warm the .tbd parse cache: resolve every input that will land
    // on a stub library and parse them all on all cores, then do the
    // same for the stubs they reexport - two waves cover an SDK's
    // umbrella trees. The serial loop below then finds every parse
    // already done.
    {
        let mut stubs: Vec<&'static MappedFile> = Vec::new();
        let consider = |path: &std::path::Path, stubs: &mut Vec<&'static MappedFile>| {
            if let Some(mf) = MappedFile::open(path)
                && get_file_type(mf) == FileType::Tapi
            {
                stubs.push(mf);
            }
        };
        for arg in &inputs {
            match arg {
                InputArg::File(path)
                | InputArg::WeakFile(path)
                | InputArg::ReexportFile(path)
                | InputArg::NeededFile(path) => consider(path, &mut stubs),
                InputArg::Lib(name, _)
                | InputArg::ReexportLib(name)
                | InputArg::NeededLib(name) => {
                    if let Some(path) = find_library(ctx, name) {
                        consider(&path, &mut stubs);
                    }
                }
                InputArg::Framework(name, _)
                | InputArg::NeededFramework(name)
                | InputArg::ReexportFramework(name) => {
                    if let Some(path) = find_framework(ctx, name) {
                        consider(&path, &mut stubs);
                    }
                }
                _ => {}
            }
        }
        let wave1 = tapi::prefetch(&stubs, E::NAME);
        let mut deps: Vec<&'static MappedFile> = Vec::new();
        for tbd in &wave1 {
            for name in &tbd.reexports {
                if tbd.document(name).is_some() {
                    continue;
                }
                if let Some(dep) = crate::input_files::find_reexport(ctx, name.as_bytes())
                    && get_file_type(dep) == FileType::Tapi
                {
                    deps.push(dep);
                }
            }
        }
        tapi::prefetch(&deps, E::NAME);
    }

    let lib = |ctx: &Context<E>, name: &OsStr| {
        let path = find_library(ctx, name);
        if path.is_none() {
            error!("library '{}' not found", name.display());
        }
        path
    };
    let framework = |ctx: &Context<E>, name: &OsStr| {
        let path = find_framework(ctx, name);
        if path.is_none() {
            error!("framework '{}' not found", name.display());
        }
        path
    };
    let mut queue: Vec<PendingObject> = Vec::new();
    for arg in &inputs {
        let rc = ReaderContext::default();
        let (path, rc) = match arg {
            InputArg::File(path) => (Some(path.clone()), rc),
            InputArg::ForceLoad(path) => {
                (Some(path.clone()), ReaderContext { force_load: true, ..rc })
            }
            InputArg::WeakFile(path) => (Some(path.clone()), ReaderContext { weak: true, ..rc }),
            InputArg::ReexportFile(path) => {
                (Some(path.clone()), ReaderContext { reexport: true, ..rc })
            }
            InputArg::NeededFile(path) => {
                (Some(path.clone()), ReaderContext { needed: true, ..rc })
            }
            InputArg::Lib(name, weak) => (lib(ctx, name), ReaderContext { weak: *weak, ..rc }),
            InputArg::ReexportLib(name) => (lib(ctx, name), ReaderContext { reexport: true, ..rc }),
            InputArg::HiddenLib(name) => (lib(ctx, name), ReaderContext { hidden: true, ..rc }),
            InputArg::NeededLib(name) => (lib(ctx, name), ReaderContext { needed: true, ..rc }),
            InputArg::Framework(name, weak) => {
                (framework(ctx, name), ReaderContext { weak: *weak, ..rc })
            }
            InputArg::ReexportFramework(name) => {
                (framework(ctx, name), ReaderContext { reexport: true, ..rc })
            }
            InputArg::NeededFramework(name) => {
                (framework(ctx, name), ReaderContext { needed: true, ..rc })
            }
        };
        let Some(path) = path else { continue };
        match MappedFile::try_open(&path) {
            Ok(mf) => collect_file(ctx, mf, rc, &mut queue),
            // A bare path names a file; the other forms name a library.
            Err(e) if matches!(arg, InputArg::File(_)) => error!(
                "file cannot be open()ed, {} path={p} in '{p}'",
                crate::error::errno_text(&e),
                p = path.display()
            ),
            Err(_) => error!("library '{}' not found", path.display()),
        }
    }
    ctx.args.inputs = inputs;

    // -bundle_loader: the executable that will load this bundle. Its
    // exports resolve the bundle's remaining undefined symbols, bound
    // at run time to the main executable (XCTest bundles hosted by an
    // app are linked this way).
    if let Some(path) = ctx.args.bundle_loader.clone() {
        if ctx.args.output_type != MH_BUNDLE {
            fatal!("-bundle_loader can only be used with -bundle");
        }
        let Ok(mf) = MappedFile::try_open(&path) else {
            fatal!("library '{}' not found", path.display());
        };
        crate::input_files::parse_bundle_loader(ctx, mf);
    }
    load_pending(ctx, queue);
}

/// Acts on auto-link options (LC_LINKER_OPTION) of live objects: each
/// names a library or framework the object needs, as if it had been on
/// the command line. Swift objects rely on this entirely. Returns true
/// if new inputs were loaded, in which case resolution must run again.
/// What a round of auto-linking added to the link.
pub enum Autolinked {
    Nothing,
    /// Only dylibs, starting at this index. A dylib addition cannot
    /// change object-vs-object resolution (autolinked files get later
    /// priorities than everything already loaded), so a light claim
    /// pass replaces a full re-resolution.
    DylibsOnly(usize),
    /// Objects, or dylibs that take symbols from ones already claimed:
    /// resolution runs again.
    Objects,
}

pub fn load_autolink_deps<E: Target>(ctx: &mut Context<E>) -> Autolinked {
    // ld64 does not act on auto-link options in a -r link: the
    // LC_LINKER_OPTION commands are copied into the output object and
    // the final link resolves them. Loading them here would let the
    // libraries claim symbols that the output must leave undefined
    // (Xcode's prelink of a Swift package auto-linked libc++ this way
    // and the -r symbol table then lacked operator new).
    if ctx.args.relocatable {
        return Autolinked::Nothing;
    }
    // ld64 acts on the auto-link options as a sorted set, not in the
    // order the objects mention them: its load commands list the
    // auto-linked libraries alphabetically ("-framework AppKit" ...
    // "-lswiftCore", "-lswiftCoreFoundation" ...), which fixes their
    // ordinals too.
    let mut pending: Vec<Vec<Vec<u8>>> = Vec::new();
    for obj in &ctx.objs {
        if !obj.is_alive {
            continue;
        }
        for opt in &obj.linker_options {
            if !ctx.processed_linker_options.contains(opt) {
                pending.push(opt.clone());
            }
        }
    }
    pending.sort();
    pending.dedup();
    let dylibs_before = ctx.dylibs.len();

    // An auto-link option is a hint, and ld64 says nothing when it
    // finds no library or framework for one. Header-only SDK
    // frameworks make that routine: every Swift object importing
    // CoreAudioTypes carries `-framework CoreAudioTypes`, whose
    // framework directory holds headers and a module map but no
    // binary (CotEditor's build printed the warning 317 times).
    let before = (ctx.objs.len(), ctx.dylibs.len());
    // A library already in the link as a public re-export (Foundation's
    // stub brings CoreFoundation) that an auto-link option now names
    // is a hint like any other auto-linked library: listed only if
    // something binds to it (ld-prime drops CoreFoundation from a
    // Swift program that never binds to it).
    let implicit_before: Vec<bool> = ctx.dylibs.iter().map(|d| d.is_implicit).collect();
    let mut queue: Vec<PendingObject> = Vec::new();
    for opt in pending {
        ctx.processed_linker_options.insert(opt.clone());
        let path = match opt.as_slice() {
            [flag] if flag.starts_with(b"-l") => find_library(ctx, crate::util::os_str(&flag[2..])),
            [flag, name] if flag == b"-framework" => find_framework(ctx, crate::util::os_str(name)),
            _ => {
                let spelled: Vec<_> = opt.iter().map(|s| crate::util::display(s)).collect();
                crate::warn!("unknown auto-link option: {}", spelled.join(" "));
                None
            }
        };
        if let Some(path) = path
            && let Some(mf) = MappedFile::open(&path)
        {
            let rc = ReaderContext { autolinked: true, ..Default::default() };
            collect_file(ctx, mf, rc, &mut queue);
        }
    }
    for dylib in &mut ctx.dylibs[dylibs_before..] {
        dylib.is_autolinked = true;
    }
    for (dylib, was_implicit) in ctx.dylibs.iter_mut().zip(implicit_before) {
        dylib.is_autolinked |= was_implicit && !dylib.is_implicit;
    }
    load_pending(ctx, queue);
    // A new dylib that an earlier one merged as a private re-export takes
    // its symbols from that one, which a light claim pass cannot move.
    let (old, new) = ctx.dylibs.split_at(before.1);
    let rebinds =
        new.iter().any(|d| old.iter().any(|o| o.merged_reexports.contains(&d.install_name)));
    if ctx.objs.len() != before.0 || rebinds {
        Autolinked::Objects
    } else if ctx.dylibs.len() != before.1 {
        Autolinked::DylibsOnly(before.1)
    } else {
        Autolinked::Nothing
    }
}

/// For each dylib, the dylibs of the link it merged as private
/// re-exports (see `providing_dylib`).
fn merged_providers(dylibs: &[input_files::DylibFile]) -> Vec<Vec<usize>> {
    let by_name: hashbrown::HashMap<&[u8], usize> =
        dylibs.iter().enumerate().map(|(i, d)| (d.install_name.as_slice(), i)).collect();
    dylibs
        .iter()
        .map(|d| {
            d.merged_reexports.iter().filter_map(|n| by_name.get(n.as_slice()).copied()).collect()
        })
        .collect()
}

/// The dylib a symbol found in `dylibs[idx]`'s exports binds to. A
/// private re-export's exports count as the re-exporting dylib's
/// (libswiftDarwin's include libswift_Builtin_float's), but when the
/// library that defines the symbol is in the link itself - named or
/// auto-linked - ld-prime binds to it, whichever of the two comes first.
fn providing_dylib(
    dylibs: &[input_files::DylibFile],
    providers: &[Vec<usize>],
    mut idx: usize,
    name: &str,
) -> usize {
    for _ in 0..dylibs.len() {
        match providers[idx].iter().find(|&&p| dylibs[p].exports.contains(name)) {
            Some(&p) => idx = p,
            None => break,
        }
    }
    idx
}

/// Lets newly auto-linked dylibs claim still-unresolved symbols. They
/// carry later priorities than every file already resolved, so they
/// can steal nothing - a full re-resolution would reach exactly this
/// outcome, at many times the cost.
pub fn claim_new_dylibs<E: Target>(ctx: &mut Context<E>, first: usize) {
    struct SymsPtr(*mut crate::symbol::Symbol);
    unsafe impl Sync for SymsPtr {}
    let syms_ptr = SymsPtr(ctx.symbols.syms.as_mut_ptr());
    let syms_ptr = &syms_ptr;
    let dylibs = &ctx.dylibs;
    let providers = merged_providers(dylibs);
    (0..ctx.symbols.syms.len()).into_par_iter().for_each(|i| {
        // SAFETY: each index is written only by its own iteration.
        let sym = unsafe { &mut *syms_ptr.0.add(i) };
        if !sym.is_used() || sym.is_defined() {
            return;
        }
        for (dylib_idx, dylib) in dylibs.iter().enumerate().skip(first) {
            if dylib.exports.contains(sym.name()) {
                let owner = providing_dylib(dylibs, &providers, dylib_idx, sym.name());
                sym.set_file(FileId::Dylib(owner as u32));
                sym.set_is_imported(true);
                sym.set_is_extern(true);
                sym.set_input_section(None);
                sym.set_is_common(false);
                break;
            }
        }
    });
}

/// Adds the object that owns what the linker synthesizes: the
/// sections standing for merged Objective-C records, folded class
/// references or tentative definitions, and symbols such as
/// __mh_execute_header. It takes part in every pass like an input
/// object with no symbol table of its own, so no pass has to treat
/// synthesized sections and symbols as fileless. mold's
/// create_internal_file.
pub fn create_internal_file<E: Target>(ctx: &mut Context<E>) {
    ctx.internal_obj = Some(ctx.objs.len());
    ctx.objs.push(input_files::ObjectFile::internal());
}

/// Resolves all symbols, following mold's model: every input including
/// each archive member has been parsed already, and resolution ranks
/// competing definitions (strong > weak > lazy archive member or
/// dylib > common), breaking ties by input order. A liveness walk then marks
/// the archive members whose definitions are actually referenced, and
/// a second round restricted to live files settles the final owners.
pub fn resolve_symbols<E: Target>(ctx: &mut Context<E>) {
    clear_claims(ctx);
    do_resolve(ctx, false);
    mark_live_objects(ctx);
    clear_claims(ctx);
    do_resolve(ctx, true);
    claim_locals(ctx);
}

/// Non-external symbols are private to their object and never compete:
/// each gets its definition directly. Relocations reference them by
/// symbol index just like externals, so they need locations too.
fn claim_locals<E: Target>(ctx: &mut Context<E>) {
    // A local symbol belongs to exactly one object (locals get fresh
    // slots, never interned), so the per-object claims write disjoint
    // symbols and the objects proceed in parallel.
    struct SlotPtr(*mut crate::symbol::Symbol);
    unsafe impl Sync for SlotPtr {}
    let ptr = SlotPtr(ctx.symbols.syms.as_mut_ptr());
    let ptr = &ptr;
    let isecs = &ctx.isecs;
    ctx.objs.par_iter().enumerate().for_each(|(obj_idx, obj)| {
        for i in obj.local_range() {
            let nlist = &obj.nlists[i];
            if nlist.is_stab() || nlist.is_extern() {
                continue;
            }
            // SAFETY: disjoint per object, as above.
            let sym = unsafe { &mut *ptr.0.add(obj.symbols[i] as usize) };
            match nlist.n_type() {
                N_ABS => {
                    sym.set_file(FileId::Obj((obj_idx) as u32));
                    sym.set_input_section(None);
                    sym.value = nlist.n_value;
                }
                N_SECT => {
                    if let Some((isec, off)) = crate::input_files::find_symbol_subsec(
                        isecs,
                        &obj.subsecs,
                        nlist.n_sect,
                        nlist.n_value,
                    ) {
                        sym.set_file(FileId::Obj((obj_idx) as u32));
                        sym.set_input_section(Some(isec as u32));
                        sym.value = off;
                        sym.set_no_dead_strip(
                            nlist.n_desc & (N_NO_DEAD_STRIP | REFERENCED_DYNAMICALLY) != 0,
                        );
                    }
                }
                _ => {}
            }
        }
    });
}

fn clear_claims<E: Target>(ctx: &mut Context<E>) {
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_)) | Some(FileId::Dylib(_))) || sym.is_common() {
            sym.clear_file();
            sym.set_input_section(None);
            sym.value = 0;
            sym.set_is_weak_def(false);
            sym.set_is_private_extern(false);
            sym.set_is_imported(false);
            sym.set_is_common(false);
            sym.common_p2align = 0;
            sym.set_no_dead_strip(false);
        }
    });
}

fn do_resolve<E: Target>(ctx: &mut Context<E>, only_alive: bool) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    // Symbols the command line names (-e, -u) exist even when no
    // object mentions them, so that a dylib export can claim them: an
    // app extension's entry point, _NSExtensionMain, lives in
    // Foundation and nothing in the extension references it.
    let mut named: Vec<String> = ctx.args.forced_undefined.clone();
    // Not for -r, whose output type is still the executable default: the
    // relocatable output would carry a spurious undefined _main.
    if ctx.args.output_type == MH_EXECUTE && !ctx.args.relocatable {
        named.push(ctx.args.entry.clone());
    }
    // -alias bases too: Xcode aliases an app extension's debug dylib
    // entry point to Foundation's _NSExtensionMain.
    named.extend(ctx.args.aliases.iter().map(|(existing, _)| existing.clone()));
    for name in named {
        if ctx.symbols.get(&name).is_none() {
            ctx.symbols.intern(String::leak(name));
        }
    }

    let n = ctx.symbols.syms.len();

    // Which symbols the files considered this round actually reference.
    // References from dead archive members must not count: they would
    // otherwise demand definitions nothing live needs.
    let used: Vec<AtomicBool> = (0..n).map(|_| AtomicBool::new(false)).collect();
    let weak_ref: Vec<AtomicBool> = (0..n).map(|_| AtomicBool::new(false)).collect();
    let strong_ref: Vec<AtomicBool> = (0..n).map(|_| AtomicBool::new(false)).collect();
    ctx.objs.par_iter().filter(|obj| !only_alive || obj.is_alive).for_each(|obj| {
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab() && nlist.is_extern() && nlist.n_type() == N_UNDF {
                used[sym_id as usize].store(true, Ordering::Relaxed);
                if nlist.n_desc & N_WEAK_REF != 0 {
                    weak_ref[sym_id as usize].store(true, Ordering::Relaxed);
                } else {
                    strong_ref[sym_id as usize].store(true, Ordering::Relaxed);
                }
            }
        }
    });
    for name in &ctx.args.forced_undefined {
        if let Some(id) = ctx.symbols.get(name) {
            used[id as usize].store(true, Ordering::Relaxed);
        }
    }
    if let Some(id) = ctx.symbols.get(&ctx.args.entry) {
        used[id as usize].store(true, Ordering::Relaxed);
    }
    for (existing, _) in &ctx.args.aliases {
        if let Some(id) = ctx.symbols.get(existing) {
            used[id as usize].store(true, Ordering::Relaxed);
        }
    }

    // The rank of a definition: (class << 40) | (alignment term << 32)
    // | priority, lower is better. Ranks race into `best` with an
    // atomic minimum, as in mold: the race is order-free because the
    // winner is the same whatever the interleaving, and since each
    // object has a unique priority, exactly one object ends up owning
    // each symbol. Among weak definitions ld64 keeps the copy with the
    // greatest alignment (a Swift metadata record comes 8-aligned from
    // one object and 16-aligned from another; the first copy wins only
    // at equal alignment), so a live weak definition's rank carries
    // its subsection's alignment, inverted.
    let isecs_for_rank = &ctx.isecs;
    let rank_of = |obj: &crate::input_files::ObjectFile, nlist: &NList| -> Option<u64> {
        if nlist.is_stab() || !nlist.is_extern() {
            return None;
        }
        let is_weak = nlist.n_desc & N_WEAK_DEF != 0;
        let class: u64 = match nlist.n_type() {
            N_SECT | N_ABS if obj.is_alive && !is_weak => 0,
            N_SECT | N_ABS if obj.is_alive => 1,
            N_SECT | N_ABS => 2,
            N_UNDF if nlist.is_common() && obj.is_alive => 3,
            _ => return None,
        };
        let mut align_term = 0u64;
        if class == 1
            && nlist.n_type() == N_SECT
            && let Some((isec, _)) = crate::input_files::find_symbol_subsec(
                isecs_for_rank,
                &obj.subsecs,
                nlist.n_sect,
                nlist.n_value,
            )
        {
            align_term = 63 - isecs_for_rank[isec].p2align as u64;
        }
        Some((class << 40) | (align_term << 32) | obj.priority as u64)
    };

    let best: Vec<AtomicU64> = (0..n).map(|_| AtomicU64::new(u64::MAX)).collect();
    ctx.objs.par_iter().filter(|obj| !only_alive || obj.is_alive).for_each(|obj| {
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if let Some(rank) = rank_of(obj, nlist) {
                best[sym_id as usize].fetch_min(rank, Ordering::Relaxed);
            }
        }
    });

    // Claim phase: each object writes the symbols whose race it won.
    // Ranks are unique per object, so every symbol has exactly one
    // writer and the parallel writes are disjoint.
    struct SymsPtr(*mut crate::symbol::Symbol);
    unsafe impl Sync for SymsPtr {}
    let syms_ptr = SymsPtr(ctx.symbols.syms.as_mut_ptr());
    let syms_ptr = &syms_ptr;
    let isecs = &ctx.isecs;
    let objs = &ctx.objs;

    objs.par_iter().enumerate().filter(|(_, obj)| !only_alive || obj.is_alive).for_each(
        |(obj_idx, obj)| {
            let r = obj.global_range();
            for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                let Some(rank) = rank_of(obj, nlist) else {
                    continue;
                };
                let won = best[sym_id as usize].load(Ordering::Relaxed);
                if won != rank {
                    continue;
                }
                // SAFETY: this object holds the unique minimum rank
                // for sym_id, so no other thread writes this slot.
                let sym = unsafe { &mut *syms_ptr.0.add(sym_id as usize) };
                sym.set_is_extern(true);
                sym.set_is_imported(false);
                sym.set_is_common(false);
                sym.set_is_weak_def(nlist.n_desc & N_WEAK_DEF != 0);
                sym.set_is_private_extern(nlist.n_type & N_PEXT != 0 || obj.hidden);
                sym.set_no_dead_strip(
                    nlist.n_desc & (N_NO_DEAD_STRIP | REFERENCED_DYNAMICALLY) != 0,
                );

                match nlist.n_type() {
                    N_ABS => {
                        sym.set_file(FileId::Obj((obj_idx) as u32));
                        sym.set_input_section(None);
                        sym.value = nlist.n_value;
                    }
                    N_SECT => {
                        sym.set_file(FileId::Obj((obj_idx) as u32));
                        match crate::input_files::find_symbol_subsec(
                            isecs,
                            &obj.subsecs,
                            nlist.n_sect,
                            nlist.n_value,
                        ) {
                            Some((isec, off)) => {
                                sym.set_input_section(Some(isec as u32));
                                sym.value = off;
                            }
                            None => {
                                // A symbol in a discarded (debug)
                                // section resolves as if undefined.
                                sym.clear_file();
                                best[sym_id as usize].store(u64::MAX, Ordering::Relaxed);
                            }
                        }
                    }
                    N_UNDF => {
                        // A common symbol takes a tentative claim.
                        sym.clear_file();
                        sym.set_is_common(true);
                        sym.value = nlist.n_value;
                        sym.common_p2align = ((nlist.n_desc >> 8) & 0xf) as u8;
                    }
                    _ => unreachable!(),
                }
            }
        },
    );

    // Common symbols merge: the largest size and strictest alignment
    // win regardless of input order, gathered from every common claim
    // once the class-3 winners are known.
    let commons: Vec<(crate::symbol::SymbolId, u64, u8)> = ctx
        .objs
        .par_iter()
        .filter(|obj| (!only_alive || obj.is_alive) && obj.is_alive)
        .flat_map_iter(|obj| {
            let r = obj.global_range();
            obj.nlists[r.clone()].iter().zip(&obj.symbols[r]).filter_map(|(nlist, &sym_id)| {
                if !nlist.is_stab()
                    && nlist.is_extern()
                    && nlist.n_type() == N_UNDF
                    && nlist.is_common()
                    && best[sym_id as usize].load(Ordering::Relaxed) >> 40 == 3
                {
                    Some((sym_id, nlist.n_value, ((nlist.n_desc >> 8) & 0xf) as u8))
                } else {
                    None
                }
            })
        })
        .collect();
    for (sym_id, size, p2align) in commons {
        let sym = &mut ctx.symbols[sym_id];
        sym.value = sym.value.max(size);
        sym.common_p2align = sym.common_p2align.max(p2align);
    }

    // Record the references seen this round. A symbol is a weak import
    // only if every reference to it is weak: one strong reference
    // anywhere makes it strong (ld64's default, -weak_reference_
    // mismatches non-weak), and so binds it non-weakly and keeps its
    // dylib loaded non-weakly.
    for i in 0..n {
        if strong_ref[i].load(Ordering::Relaxed) {
            let sym = &mut ctx.symbols.syms[i];
            sym.set_is_strong_ref(true);
            sym.set_is_weak_ref(false);
        } else if weak_ref[i].load(Ordering::Relaxed) && !ctx.symbols.syms[i].is_strong_ref() {
            ctx.symbols.syms[i].set_is_weak_ref(true);
        }
    }

    // Dylib exports claim unresolved (or lazily-claimed) symbols; an
    // earlier dylib beats a later archive member and vice versa. A
    // relocatable link keeps every reference undefined instead.
    if ctx.args.relocatable {
        for (i, u) in used.iter().enumerate() {
            ctx.symbols.syms[i].set_is_used(u.load(Ordering::Relaxed));
        }
        return;
    }
    let dylibs = &ctx.dylibs;
    let providers = merged_providers(dylibs);
    (0..n).into_par_iter().for_each(|i| {
        if !used[i].load(Ordering::Relaxed) {
            return;
        }
        // SAFETY: each index is written only by its own iteration.
        let sym = unsafe { &mut *syms_ptr.0.add(i) };
        if sym.is_common() || best[i].load(Ordering::Relaxed) >> 40 < 2 {
            return;
        }
        for (dylib_idx, dylib) in dylibs.iter().enumerate() {
            let rank = (2u64 << 40) | dylib.priority as u64;
            if rank < best[i].load(Ordering::Relaxed) && dylib.exports.contains(sym.name()) {
                best[i].store(rank, Ordering::Relaxed);
                let owner = providing_dylib(dylibs, &providers, dylib_idx, sym.name());
                let dylib = &dylibs[owner];
                sym.set_file(FileId::Dylib(owner as u32));
                sym.set_is_imported(true);
                sym.set_is_extern(true);
                sym.set_input_section(None);
                sym.set_is_common(false);
                // -weak_framework / -weak_library / -weak-l: every
                // import from the library is a weak import (ld64 binds
                // it weak-import and marks it N_WEAK_REF), whatever the
                // references say.
                if dylib.is_weak {
                    sym.set_is_weak_ref(true);
                }
                break;
            }
        }
    });

    // Record the final usage set for downstream passes.
    for (i, u) in used.iter().enumerate() {
        ctx.symbols.syms[i].set_is_used(u.load(Ordering::Relaxed));
    }
}

/// Marks archive members whose definitions live code references,
/// walking owner links to a fixed point.
fn mark_live_objects<E: Target>(ctx: &mut Context<E>) {
    // Resolution runs in rounds and recomputes liveness each time, so
    // the -why_load record starts over with it.
    ctx.why_load.clear();
    let mut queue: Vec<usize> = (0..ctx.objs.len()).filter(|&i| ctx.objs[i].is_alive).collect();

    // The entry point and -u symbols are roots too.
    let mut root_syms: Vec<&str> = vec![ctx.args.entry.as_str()];
    root_syms.extend(ctx.args.forced_undefined.iter().map(String::as_str));
    for name in root_syms {
        if let Some(id) = ctx.symbols.get(name)
            && let Some(FileId::Obj(owner)) = ctx.symbols[id].file()
        {
            let owner = owner as usize;
            if !ctx.objs[owner].is_alive {
                ctx.objs[owner].is_alive = true;
                ctx.why_load.insert(owner, ctx.symbols[id].name());
                queue.push(owner);
            }
        }
    }

    while let Some(obj_idx) = queue.pop() {
        for i in 0..ctx.objs[obj_idx].nlists.len() {
            let nlist = ctx.objs[obj_idx].nlists[i];
            if nlist.is_stab() || !nlist.is_extern() || nlist.n_type() != N_UNDF {
                continue;
            }
            let sym_id = ctx.objs[obj_idx].symbols[i];
            if let Some(FileId::Obj(owner)) = ctx.symbols[sym_id].file() {
                let owner = owner as usize;
                if !ctx.objs[owner].is_alive {
                    ctx.objs[owner].is_alive = true;
                    ctx.why_load.insert(owner, ctx.symbols[sym_id].name());
                    queue.push(owner);
                }
            }
        }
    }
}

/// Compiles live bitcode modules into one Mach-O object and
/// replaces the placeholder objects' symbol claims with the real ones.
pub fn do_lto<E: Target>(ctx: &mut Context<E>) -> bool {
    if !ctx.lto_modules.iter().any(|&(obj, _)| ctx.objs[obj].is_alive) {
        return false;
    }
    let plugin = ctx.lto_plugin.unwrap();

    // SAFETY: libLTO calls with handles created by the same library.
    let data = unsafe {
        let cg = (plugin.codegen_create)();
        if cg.is_null() {
            fatal!("lto_codegen_create failed: {}", plugin.error_message());
        }
        (plugin.codegen_set_pic_model)(cg, crate::lto::LTO_CODEGEN_PIC_MODEL_DYNAMIC);

        for &(obj, module) in &ctx.lto_modules {
            if !ctx.objs[obj].is_alive {
                continue;
            }
            if (plugin.codegen_add_module)(cg, module as *mut _) {
                fatal!("lto_codegen_add_module failed: {}", plugin.error_message());
            }
        }

        // Everything the rest of the link can see must survive the LTO
        // internalizer. For a dylib that is every external symbol a
        // bitcode module defines - each is an export. An executable
        // exports nothing that matters, so only symbols some non-LTO
        // code references (plus the entry point, and everything under
        // -export_dynamic, which exists exactly to let executables
        // keep their globals for dlsym) must survive; the rest can be
        // internalized and dead-stripped inside the module. A reference
        // from another bitcode module does not count: libLTO resolves
        // those itself, and ld-prime lets such a function go local
        // (_times2, called only from a bitcode main, is not exported).
        // A native common does count: when a bitcode definition wins,
        // the common's code addresses that definition's storage. So do
        // -alias bases, which the linker itself references.
        let executable = ctx.args.output_type == MH_EXECUTE;
        let mut native_refs: hashbrown::HashSet<crate::symbol::SymbolId> =
            hashbrown::HashSet::new();
        for obj in &ctx.objs {
            if !obj.is_alive || obj.lto_module.is_some() {
                continue;
            }
            let r = obj.global_range();
            for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                if !nlist.is_stab() && nlist.n_type() == N_UNDF {
                    native_refs.insert(sym_id);
                }
            }
        }
        let mut preserve: Vec<std::ffi::CString> = Vec::new();
        for (i, sym) in ctx.symbols.syms.iter().enumerate() {
            if let Some(FileId::Obj(idx)) = sym.file()
                && ctx.objs[idx as usize].is_alive
                && ctx.objs[idx as usize].lto_module.is_some()
                && sym.is_extern()
            {
                if executable
                    && !ctx.args.export_dynamic
                    && (!sym.is_used() || !native_refs.contains(&(i as u32)))
                    && sym.name() != ctx.args.entry
                    && !ctx.args.forced_undefined.iter().any(|n| n == sym.name())
                    && !ctx.args.aliases.iter().any(|(existing, _)| existing == sym.name())
                    && !ctx
                        .args
                        .exported_symbols
                        .as_ref()
                        .is_some_and(|exported| exported.find(sym.name().as_bytes()) != -1)
                {
                    continue;
                }
                if let Ok(name) = std::ffi::CString::new(sym.name()) {
                    preserve.push(name);
                }
            }
        }
        if let Ok(name) = std::ffi::CString::new(ctx.args.entry.as_str()) {
            preserve.push(name);
        }
        for name in &preserve {
            (plugin.codegen_add_must_preserve_symbol)(cg, name.as_ptr());
        }

        let mut size = 0usize;
        let ptr = (plugin.codegen_compile)(cg, &raw mut size);
        if ptr.is_null() {
            fatal!("lto_codegen_compile failed: {}", plugin.error_message());
        }
        std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec()
    };

    // -object_path_lto keeps the machine-code object LTO produced.
    // Debug info stays in object files on Mach-O (the executable only
    // gets stabs pointing at them), and for LTO code that object
    // exists only inside the linker - Xcode passes a path under the
    // dSYM staging directory so dsymutil can find it afterwards.
    if let Some(path) = &ctx.args.object_path_lto
        && std::fs::write(path, &data).is_err()
    {
        fatal!("-object_path_lto: cannot write {}", path.display());
    }

    // Retire the placeholders: the compiled object provides the real
    // definitions, so they must neither claim nor reference anything in
    // the next resolution round.
    let modules = std::mem::take(&mut ctx.lto_modules);
    for &(obj_idx, _) in &modules {
        let ids = ctx.objs[obj_idx].symbols.clone();
        for id in ids {
            let sym = &mut ctx.symbols[id];
            if sym.file() == Some(FileId::Obj(obj_idx as u32)) {
                sym.clear_file();
                sym.set_input_section(None);
                sym.value = 0;
                sym.set_is_weak_def(false);
            }
        }
        let obj = &mut ctx.objs[obj_idx];
        obj.is_alive = false;
        obj.nlists = std::borrow::Cow::Borrowed(&[]);
        obj.symbols.clear();
    }

    let mf = Box::leak(Box::new(crate::mapped_file::MappedFile {
        name: PathBuf::from("<LTO>"),
        data: Vec::leak(data),
        parent: None,
        ar_date: None,
    }));
    input_files::parse_object(ctx, mf, true);
    true
}

/// Converts surviving tentative definitions (common symbols) into real
/// definitions in a synthetic __DATA,__common zero-fill section.
pub fn convert_common_symbols<E: Target>(ctx: &mut Context<E>) {
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    // Where the __common section sorts: with the first object that
    // claims a common symbol still unresolved by a definition.
    ctx.common_first_obj = ctx
        .objs
        .iter()
        .position(|obj| {
            obj.is_alive
                && obj.nlists.iter().zip(&obj.symbols).any(|(nlist, &id)| {
                    !nlist.is_stab()
                        && nlist.is_extern()
                        && nlist.n_type() == N_UNDF
                        && nlist.is_common()
                        && ctx.symbols[id].is_common()
                        && !ctx.symbols[id].is_defined()
                })
        })
        .map(|i| i as u32);
    for i in 0..ctx.symbols.syms.len() {
        let sym = &ctx.symbols[i];
        if !sym.is_common() || sym.is_defined() {
            continue;
        }
        let size = sym.value;
        // An alignment the object gave (.comm's third operand) is kept;
        // without one, ld64 aligns the symbol to its size rounded up to
        // a power of two, capped at the page on arm64 (a 100000-byte
        // array lands 16KB-aligned) and at 16 bytes on x86-64.
        let p2align = if sym.common_p2align != 0 || size == 0 {
            sym.common_p2align
        } else {
            let cap = if E::CPUTYPE == crate::macho::CPU_TYPE_ARM64 { 14 } else { 4 };
            (size.next_power_of_two().trailing_zeros() as u8).min(cap)
        };

        let (file, shndx) = ctx.add_synthetic_section(MachSection {
            sectname: str_to_name("__common"),
            segname: str_to_name("__DATA"),
            size,
            p2align: p2align as u32,
            flags: S_ZEROFILL,
            ..Default::default()
        });
        ctx.isecs.push(InputSection {
            file,
            shndx,
            p2align,
            input_addr: 0,
            size: size as u32,
            contents: 0,
            rel_offset: 0,
            nrels: 0,
            output_section: u32::MAX,
            offset: 0,
            flags: InputSection::flags_alive(),
            replacement: crate::input_sections::NO_REPLACEMENT,
            unwind_offset: 0,
            nunwind: 0,
        });

        let sym = &mut ctx.symbols[i];
        sym.set_file(FileId::Obj(internal));
        sym.set_input_section(Some((ctx.isecs.len() - 1) as u32));
        sym.value = 0;
        sym.set_is_common(false);
        sym.set_is_extern(true);
    }
}

/// With -init_offsets, replaces __mod_init_func's absolute pointers
/// (which each need a rebase) with 32-bit image-relative offsets in a
/// __TEXT,__init_offsets section (type S_INIT_FUNC_OFFSETS), which
/// dyld runs the same way but never has to fix up.
pub fn convert_init_offsets<E: Target>(ctx: &mut Context<E>) {
    // ld-prime turns this on implicitly with chained fixups: the point
    // of chains is a fixup-free __DATA_CONST, and absolute initializer
    // pointers would drag rebases back in. It follows -fixup_chains or
    // the deployment target even when -undefined dynamic_lookup sends
    // the fixups themselves back to classic dyld info; only
    // -no_fixup_chains keeps __mod_init_func.
    if !ctx.args.init_offsets
        && !ctx.args.fixup_chains.unwrap_or_else(|| ctx.chained_fixups_by_default())
    {
        return;
    }
    for i in 0..ctx.isecs.len() {
        if ctx.hdr_of(&ctx.isecs[i]).section_type() != S_MOD_INIT_FUNC_POINTERS
            || !ctx.isecs[i].is_alive()
        {
            continue;
        }
        let mut relocs = ctx.isec_relocs(i).to_vec();
        // Imported or unresolved initializers cannot be represented as
        // local offsets. Keep their pointer section and its references.
        if relocs.iter().any(|rel| ctx.reloc_target_isec(ctx.isecs[i].file as usize, rel).is_none())
        {
            continue;
        }
        relocs.sort_by_key(|r| r.offset);
        for rel in relocs {
            let obj = ctx.isecs[i].file as usize;
            let target = match ctx.reloc_target_sym(obj, &rel) {
                Some(id) => {
                    let sym = &ctx.symbols[id];
                    match sym.input_section() {
                        Some(isec) => (ctx.resolve_isec(isec as usize), sym.value),
                        None => continue,
                    }
                }
                None => match rel.target() {
                    RelocTarget::Section(isec) => {
                        (ctx.resolve_isec(isec as usize), rel.addend as u64)
                    }
                    _ => continue,
                },
            };
            ctx.init_offsets.init_funcs.push(target);
        }
        ctx.isecs[i].set_alive(false);
    }
}

/// Validates only objects selected by resolution, including the LTO
/// output. Unused archive members must not cause errors or warnings.
pub fn check_input_versions<E: Target>(ctx: &Context<E>) {
    // The deployment target the inputs are checked against: zero if none
    // was specified, but a -r output takes its first object's, and
    // ld-prime checks the later ones against that.
    let minos = if ctx.args.relocatable {
        crate::relocatable::output_target(ctx).1
    } else {
        ctx.args.platform_minos
    };
    for obj in ctx.objs.iter().filter(|obj| obj.is_alive) {
        // Old objects and the synthesized object may have no version
        // command. An object may also declare more than one platform;
        // use the deployment target for the platform being linked.
        let Some(first) = obj.platform_versions.first() else { continue };
        let Some(version) = obj.platform_versions.iter().find(|v| v.platform == ctx.args.platform)
        else {
            crate::error!(
                "building for '{}', but linking in object file ({}) built for '{}'",
                platform_name(ctx.args.platform),
                obj.mf.name.display(),
                platform_name(first.platform)
            );
            continue;
        };

        // The SDK version used to compile an input does not constrain
        // its use.
        if minos != 0 && version.minos > minos {
            crate::warn!(
                "object file ({}) was built for newer '{}' version ({}) than being linked ({})",
                resolved_file_name(obj.mf),
                platform_name(version.platform),
                format_version(version.minos),
                format_version(minos)
            );
        }
    }
}

/// Hides the subsections of archive members that resolution left
/// dead, so nothing of theirs reaches the output.
pub fn remove_unreachable_files<E: Target>(ctx: &mut Context<E>) {
    for isec in ctx.isecs.iter_mut() {
        if !ctx.objs[isec.file as usize].is_alive {
            isec.set_alive(false);
        }
    }

    // Unwind records and FDEs of dead files go too, remapping the
    // record-to-FDE links around the removals.
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    let fdes = std::mem::take(&mut ctx.fdes);
    for (i, fde) in fdes.into_iter().enumerate() {
        if ctx.isecs[fde.isec].is_alive() {
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
        if rec.fde_idx != crate::input_files::UNWIND_NONE {
            rec.fde_idx = map[rec.fde_idx as usize] as u32;
        }
        true
    });
    refresh_unwind_ranges(ctx);
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

/// Merges identical literal elements across all live inputs: the first
/// live copy wins and the rest redirect to it.
pub fn merge_literals<E: Target>(ctx: &mut Context<E>) {
    // Deduplication follows the symbol table's sharded shape: every
    // element's content hash is computed in parallel, elements bin by
    // hash, and the shards resolve independently - within a shard the
    // first occurrence in input order wins, which is exactly the
    // winner the old serial single-map walk picked.
    let hashed: Vec<(u64, u32, u32)> = ctx
        .isecs
        .par_iter()
        .enumerate()
        .filter_map(|(i, isec)| {
            if !isec.is_alive() || isec.replacement != crate::input_sections::NO_REPLACEMENT {
                return None;
            }
            let ty = ctx.hdr_of(isec).section_type();
            if !matches!(
                ty,
                S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS
            ) {
                return None;
            }
            Some((xxhash_rust::xxh3::xxh3_64(isec.data()), ty, i as u32))
        })
        .collect();

    const NUM_SHARDS: usize = 64;
    let mut bins: Vec<Vec<(u64, u32, u32)>> = vec![Vec::new(); NUM_SHARDS];
    for &e in &hashed {
        bins[(e.0 % NUM_SHARDS as u64) as usize].push(e);
    }

    let isecs = &ctx.isecs;
    let folds: Vec<Vec<(u32, u32)>> = bins
        .into_par_iter()
        .map(|bin| {
            let mut map: hashbrown::HashMap<(u64, u32, &[u8]), u32> = hashbrown::HashMap::new();
            let mut out = Vec::new();
            for (hash, ty, i) in bin {
                match map.entry((hash, ty, isecs[i as usize].data())) {
                    hashbrown::hash_map::Entry::Occupied(e) => out.push((i, *e.get())),
                    hashbrown::hash_map::Entry::Vacant(e) => {
                        e.insert(i);
                    }
                }
            }
            out
        })
        .collect();

    for fold in folds {
        for (loser, winner) in fold {
            let p2align = ctx.isecs[loser as usize].p2align;
            ctx.isecs[loser as usize].replacement = winner;
            let w = &mut ctx.isecs[winner as usize];
            w.p2align = w.p2align.max(p2align);
        }
    }

    redirect_symbols_to_replacements(ctx);
}

/// Points every symbol defined in a merged-away subsection at the
/// surviving one - mold makes the merged section's fragment the
/// symbol's origin - so a symbol's address never follows a replacement
/// chain. The copies are identical, so the symbol's offset is
/// unchanged. (Section-relative relocations still resolve through the
/// chain in isec_addr.)
fn redirect_symbols_to_replacements<E: Target>(ctx: &mut Context<E>) {
    let isecs = &ctx.isecs;
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if let Some(i) = sym.input_section() {
            let mut r = i as usize;
            while isecs[r].replacement != crate::input_sections::NO_REPLACEMENT {
                r = isecs[r].replacement as usize;
            }
            if r != i as usize {
                sym.set_input_section(Some(r as u32));
            }
        }
    });
}

/// Coalesces the Objective-C reference records the compiler emits
/// once per object: __objc_selrefs entries naming the same selector,
/// __objc_classrefs entries naming the same class, and identical
/// __cfstring constants. ld64 keeps one of each, in a -r output as in
/// a final link (NetNewsWire's RSCore prelink had 56 class references
/// where ld-prime's has 30, its debug dylib 592 selector references
/// too many); the first copy wins and the rest redirect to it, like
/// merged literals. A final link leaves class references to
/// fold_objc_classrefs, which turns them into GOT slots.
pub fn coalesce_objc_refs<E: Target>(ctx: &mut Context<E>) {
    // What a pointer relocation refers to: a place in a subsection
    // (where identical content has already been merged), or a symbol
    // defined elsewhere.
    #[derive(Hash, PartialEq, Eq)]
    enum Target {
        At(usize, i64),
        Sym(crate::symbol::SymbolId, i64),
    }
    #[derive(Hash, PartialEq, Eq)]
    enum Key {
        Sel(Target),
        Class(crate::symbol::SymbolId),
        CfString(Vec<u8>, Vec<(u32, Target)>),
    }
    let place = |ctx: &Context<E>, obj: usize, rel: &crate::input_sections::Reloc| -> Target {
        match rel.target() {
            RelocTarget::Section(t) => Target::At(ctx.resolve_isec(t as usize), rel.addend),
            RelocTarget::Sym(idx) => {
                let sym_id = ctx.objs[obj].symbols[idx as usize];
                let sym = &ctx.symbols[sym_id];
                match sym.input_section() {
                    Some(isec) => {
                        Target::At(ctx.resolve_isec(isec as usize), sym.value as i64 + rel.addend)
                    }
                    None => Target::Sym(sym_id, rel.addend),
                }
            }
        }
    };
    let mut first: hashbrown::HashMap<Key, u32> = hashbrown::HashMap::new();
    let mut folds: Vec<(usize, u32)> = Vec::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive()
            || isec.replacement != crate::input_sections::NO_REPLACEMENT
            || ctx.is_internal(isec.file as usize)
        {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if h.segname() != "__DATA" {
            continue;
        }
        let obj = isec.file as usize;
        let rels = ctx.isec_relocs(i);
        let plain_ptr = |rel: &crate::input_sections::Reloc| {
            E::classify_reloc(rel.r_type) == RelocClass::Plain
                && rel.size == 8
                && !rel.is_pcrel
                && !rel.is_subtracted
        };
        let key = match h.sectname() {
            "__objc_classrefs" if !ctx.args.relocatable && objc_refs_are_const(ctx) => continue,
            "__objc_selrefs" | "__objc_classrefs" => {
                if isec.size != 8 || rels.len() != 1 || !plain_ptr(&rels[0]) {
                    continue;
                }
                if h.sectname() == "__objc_classrefs" {
                    let RelocTarget::Sym(idx) = rels[0].target() else { continue };
                    if rels[0].addend != 0 {
                        continue;
                    }
                    Key::Class(ctx.objs[obj].symbols[idx as usize])
                } else {
                    Key::Sel(place(ctx, obj, &rels[0]))
                }
            }
            "__cfstring" => {
                if isec.size != 32 || !rels.iter().all(plain_ptr) {
                    continue;
                }
                let mut targets: Vec<(u32, Target)> =
                    rels.iter().map(|rel| (rel.offset, place(ctx, obj, rel))).collect();
                targets.sort_by_key(|t| t.0);
                // The relocated fields hold per-object addends (x86-64
                // embeds the target's address); the targets stand for
                // them.
                let mut bytes = isec.data().to_vec();
                for rel in rels {
                    let (a, b) = (rel.offset as usize, rel.offset as usize + rel.size as usize);
                    bytes[a..b].fill(0);
                }
                Key::CfString(bytes, targets)
            }
            _ => continue,
        };
        match first.entry(key) {
            hashbrown::hash_map::Entry::Occupied(e) => folds.push((i, *e.get())),
            hashbrown::hash_map::Entry::Vacant(e) => {
                e.insert(i as u32);
            }
        }
    }
    if folds.is_empty() {
        return;
    }
    for (loser, winner) in folds {
        let p2align = ctx.isecs[loser].p2align;
        ctx.isecs[loser].replacement = winner;
        let w = &mut ctx.isecs[winner as usize];
        w.p2align = w.p2align.max(p2align);
    }
    redirect_symbols_to_replacements(ctx);
}

/// Synthesizes _objc_msgSend$<selector> stubs. With selector stubs
/// (the default since Xcode 14), the compiler calls these
/// linker-provided symbols instead of setting up the selector argument
/// itself; each stub loads the interned selector and tail-calls
/// _objc_msgSend. ld-prime lays the stubs out sorted by selector,
/// bytewise, and their selector references in the same order.
pub fn create_objc_msgsend_stubs<E: Target>(ctx: &mut Context<E>) {
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    let mut stubs: Vec<(u32, String)> = Vec::new();
    for i in 0..ctx.symbols.syms.len() {
        let sym = &ctx.symbols[i];
        if sym.is_defined() || !sym.is_used() {
            continue;
        }
        if let Some(sel) = sym.name().strip_prefix("_objc_msgSend$") {
            stubs.push((i as u32, sel.to_string()));
        }
    }
    stubs.sort_by(|a, b| a.1.cmp(&b.1));
    for (idx, &(i, _)) in stubs.iter().enumerate() {
        ctx.symbols[i].set_file(FileId::Obj(internal));
        ctx.sym_aux_mut(i).objc_stub_idx = idx as u32;
    }
    ctx.objc_stubs.symbols = stubs;

    if !ctx.objc_stubs.symbols.is_empty() {
        let id = ctx.symbols.intern("_objc_msgSend");
        ctx.symbols[id].set_is_used(true);
        ctx.objc_stubs.msgsend_sym = Some(id);

        // The stub machinery itself references _objc_msgSend; resolve
        // it now, since regular resolution has already run.
        if !ctx.symbols[id].is_defined()
            && let Some(dylib) = ctx.dylibs.iter().position(|d| d.exports.contains("_objc_msgSend"))
        {
            let sym = &mut ctx.symbols[id];
            sym.set_file(FileId::Dylib((dylib) as u32));
            sym.set_is_imported(true);
            sym.set_is_extern(true);
        }
    }
}

/// Reports references to symbols that are still unresolved; with
/// `-undefined dynamic_lookup` they become flat-namespace imports that
/// dyld resolves against any loaded image at run time.
/// Auto-hides eligible weak definitions. Compilers mark a weak
/// definition whose address is never observed with
/// .weak_def_can_be_hidden (nlist n_desc carries N_WEAK_DEF and
/// N_WEAK_REF together): no one can tell which image's copy they use,
/// so ld64 demotes such symbols to non-external in every kind of
/// output - executables, dylibs and bundles alike - gone from the
/// export trie and the external symbol table, and referenced directly
/// rather than through weak-lookup binds (ld-prime's NetNewsWire
/// dylib hides PLCrashReporter's template constructors this way).
/// The scopes of coalesced copies merge: one plain .weak_definition
/// among them pins the symbol exported, and an -exported_symbols_list
/// naming it does too.
pub fn auto_hide_weak_defs<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }

    // For each symbol, "seen a live weak def" and "every live weak def
    // may be hidden". A C++ debug link has millions of weak-def nlists
    // (every inline and template instance), so this reduces over them
    // in parallel into a dense array keyed by the symbol's id - mold's
    // pattern - rather than a serial fold into a hash map. Bit 0 marks
    // a symbol seen; bit 1 marks it as having a def that cannot hide.
    // Both bits are monotonic (only ever set), so racing relaxed
    // stores are safe.
    use std::sync::atomic::{AtomicU8, Ordering};
    const SEEN: u8 = 1;
    const NOT_HIDABLE: u8 = 2;
    let flags: Vec<AtomicU8> = (0..ctx.symbols.syms.len()).map(|_| AtomicU8::new(0)).collect();
    ctx.objs.par_iter().for_each(|obj| {
        if !obj.is_alive {
            return;
        }
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if nlist.is_stab()
                || !nlist.is_extern()
                || nlist.n_type() != N_SECT
                || nlist.n_desc & N_WEAK_DEF == 0
            {
                continue;
            }
            let bits = if nlist.n_desc & N_WEAK_REF != 0 { SEEN } else { SEEN | NOT_HIDABLE };
            flags[sym_id as usize].fetch_or(bits, Ordering::Relaxed);
        }
    });

    let exported = ctx.args.exported_symbols.as_ref();
    ctx.symbols.syms.par_iter_mut().zip(&flags).for_each(|(sym, f)| {
        let f = f.load(Ordering::Relaxed);
        if f & SEEN != 0
            && f & NOT_HIDABLE == 0
            && sym.is_weak_def()
            && sym.is_extern()
            && matches!(sym.file(), Some(FileId::Obj(_)))
            && !exported.is_some_and(|exported| exported.find(sym.name().as_bytes()) != -1)
        {
            sym.set_is_private_extern(true);
        }
    });
}

/// Hide definitions before dead stripping and relocation scanning so
/// they neither keep otherwise unused code alive nor bind as exports.
pub fn hide_all_exports<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.no_exported_symbols {
        return;
    }
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_))) {
            sym.set_is_private_extern(true);
        }
    });
}

/// -exported_symbol(s_list) and -unexported_symbol(s_list) narrow the
/// exports by scope, as -no_exported_symbols does: ld64 turns every
/// definition they leave out into a private extern, in executables,
/// dylibs, bundles and -r outputs alike. Such a symbol is then a local
/// in the symbol table, not a dead-strip root of a dylib, not bound by
/// weak lookup and not counted toward MH_WEAK_DEFINES. The exports
/// -reexported_symbols_list adds are created afterwards, so the lists
/// never hide them. Ported from sold's handle_exported_symbols_list
/// and handle_unexported_symbols_list.
pub fn handle_exported_symbols_list<E: Target>(ctx: &mut Context<E>) {
    let Some(exported) = &ctx.args.exported_symbols else {
        return;
    };
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && exported.find(sym.name().as_bytes()) == -1
        {
            sym.set_is_private_extern(true);
        }
    });
}

pub fn handle_unexported_symbols_list<E: Target>(ctx: &mut Context<E>) {
    let unexported = &ctx.args.unexported_symbols;
    if unexported.is_empty() {
        return;
    }
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && unexported.find(sym.name().as_bytes()) != -1
        {
            sym.set_is_private_extern(true);
        }
    });
}

/// Discards the losing copies of coalesced weak definitions. Symbol
/// resolution picks one definition per weak symbol, but the losing
/// objects' subsections still hold the duplicate bodies - a C++-heavy
/// link would otherwise ship every object's copy of every template
/// instantiation as anonymous dead weight (12MB of clang's 80MB
/// __text). Each losing subsection is redirected to the winner's, the
/// same replacement mechanism literal merging and ICF use, so
/// section-target relocations into a loser resolve into the winning
/// copy. Only same-shape losers are folded: the defining symbol must
/// sit at the same offset in both, and the subsections must be the
/// same size, or differ only by trailing zero padding (Swift's
/// __swift5_typeref strings come with or without a pad byte from
/// one object to the next, and ld64 discards the losers regardless):
/// C++ guarantees identical weak instantiations, but any other
/// mismatch means something odd, and keeping the copy is safe.
pub fn coalesce_weak_defs<E: Target>(ctx: &mut Context<E>) {
    // A C++ debug link has millions of weak-def nlists (every inline
    // and template instance), so the scan that finds each losing copy
    // - filtering, and a find_subsec binary search per weak def - runs
    // in parallel per object. Only pure reads happen here (resolution
    // has already set origins, and find_subsec reads stable subsection
    // extents), so each object independently emits its candidate
    // (loser, winner) subsection pairs, unresolved, in nlist order.
    let shared = &*ctx;
    let candidates: Vec<Vec<(usize, usize, u64, u64)>> = ctx
        .objs
        .par_iter()
        .enumerate()
        .map(|(obj_idx, obj)| {
            let mut out = Vec::new();
            if !obj.is_alive {
                return out;
            }
            // The addresses of the object's other symbols, to recognize
            // a losing subsection that holds more than the weak
            // definition: an object without subsections-via-symbols
            // has one subsection per section, and folding it away
            // would take every other symbol's bytes with it. ld64
            // splits at symbols regardless; we keep such a copy.
            let mut values: Option<Vec<u64>> = None;
            let r = obj.global_range();
            for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                if nlist.is_stab()
                    || !nlist.is_extern()
                    || nlist.n_type() != N_SECT
                    || nlist.n_desc & N_WEAK_DEF == 0
                {
                    continue;
                }
                let sym = &shared.symbols[sym_id];
                let Some(FileId::Obj(owner)) = sym.file() else { continue };
                if owner as usize == obj_idx {
                    continue;
                }
                let Some(winner) = sym.input_section().map(|i| i as usize) else { continue };
                let Some((loser, off)) = crate::input_files::find_symbol_subsec(
                    &shared.isecs,
                    &obj.subsecs,
                    nlist.n_sect,
                    nlist.n_value,
                ) else {
                    continue;
                };
                let values = values.get_or_insert_with(|| {
                    let mut v: Vec<u64> = obj
                        .nlists
                        .iter()
                        .filter(|n| !n.is_stab() && n.n_type() == N_SECT)
                        .map(|n| n.n_value)
                        .collect();
                    v.sort_unstable();
                    v.dedup();
                    v
                });
                let l = &shared.isecs[loser];
                let (start, end) = (l.input_addr as u64, l.input_addr as u64 + l.size as u64);
                let lo = values.partition_point(|&v| v < start);
                let hi = values.partition_point(|&v| v < end);
                if values[lo..hi].iter().any(|&v| v != nlist.n_value) {
                    continue;
                }
                out.push((loser, winner, off, sym.value));
            }
            out
        })
        .collect();

    // Applying the replacements is serial and order-dependent (a later
    // loser may resolve through an earlier one), so it stays a single
    // walk in object order - the same order and the same resolve/size
    // checks as the original loop, over only the qualifying weak defs.
    for list in candidates {
        for (loser, winner, off, sym_value) in list {
            let winner = ctx.resolve_isec(winner);
            let loser = ctx.resolve_isec(loser);
            if loser == winner
                || off != sym_value
                || ctx.isecs[loser].replacement != crate::input_sections::NO_REPLACEMENT
                || !same_shape(&ctx.isecs[loser], &ctx.isecs[winner])
            {
                continue;
            }
            ctx.isecs[loser].replacement = winner as u32;
        }
    }
}

/// Whether two copies of a weak definition can stand for each other:
/// the same size, or the longer's tail beyond the shorter is zero.
fn same_shape(
    a: &crate::input_sections::InputSection,
    b: &crate::input_sections::InputSection,
) -> bool {
    if a.size == b.size {
        return true;
    }
    let (short, long) = if a.size < b.size { (a, b) } else { (b, a) };
    long.data().get(short.size as usize..).is_some_and(|tail| tail.iter().all(|&x| x == 0))
}

/// Reports two live strong definitions of one name. Resolution keeps
/// the first strong definition it meets; a strong definition in any
/// other live object that lost to it is an error (a weak or common one
/// yields quietly). Reported after resolution settles, sorted by name,
/// so the messages are deterministic: mold's
/// check_duplicate_symbols.
pub fn check_duplicate_symbols<E: Target>(ctx: &Context<E>) {
    let mut duplicates: Vec<(crate::symbol::SymbolId, usize)> = ctx
        .objs
        .par_iter()
        .enumerate()
        .filter(|(_, obj)| obj.is_alive)
        .flat_map_iter(|(obj_idx, obj)| {
            let r = obj.global_range();
            obj.nlists[r.clone()].iter().zip(&obj.symbols[r]).filter_map(move |(nlist, &sym_id)| {
                if nlist.is_stab()
                    || !nlist.is_extern()
                    || !matches!(nlist.n_type(), N_SECT | N_ABS)
                    || nlist.n_desc & N_WEAK_DEF != 0
                {
                    return None;
                }
                match ctx.symbols[sym_id].file() {
                    Some(FileId::Obj(owner)) if owner as usize != obj_idx => {
                        Some((sym_id, obj_idx))
                    }
                    _ => None,
                }
            })
        })
        .collect();
    duplicates.sort_by_key(|&(sym_id, obj_idx)| (ctx.symbols[sym_id].name(), obj_idx));
    duplicates.dedup();
    for (sym_id, obj_idx) in duplicates {
        let prev = match ctx.symbols[sym_id].file() {
            Some(FileId::Obj(idx)) => file_display(&ctx.objs[idx as usize]),
            _ => "?".into(),
        };
        error!(
            "duplicate symbol: {}: {}: {}",
            file_display(&ctx.objs[obj_idx]),
            prev,
            ctx.symbols[sym_id]
        );
    }
}

pub fn report_undef_errors<E: Target>(ctx: &mut Context<E>) {
    use std::sync::atomic::Ordering;
    // An alive object may name a symbol undefined that nothing refers
    // to - a .globl with neither a definition nor a relocation, as XNU
    // declares `SleepToken` under !WITH_CLASSIC_S2R. ld-prime drops
    // such a name without a word: no error, and no import under
    // -undefined dynamic_lookup. Most links have no undefined symbol at
    // all, so the relocations are looked at only when there is one.
    let undef: Vec<usize> = (0..ctx.symbols.syms.len())
        .into_par_iter()
        .filter(|&i| ctx.symbols.syms[i].is_used() && !ctx.symbols.syms[i].is_defined())
        .collect();
    if undef.is_empty() {
        return;
    }
    let referenced = referenced_symbols(ctx);
    // An initial undefine (-u, or a name an export list gives without
    // wildcards) must resolve: ld-prime reports one even under
    // -undefined dynamic_lookup or -U.
    let initial: hashbrown::HashSet<crate::symbol::SymbolId> =
        ctx.args.forced_undefined.iter().filter_map(|name| ctx.symbols.get(name)).collect();

    // Errors name a file that wants the symbol; the map from symbol to
    // referencing object is built only once an error is certain.
    let mut referencers: Option<std::collections::HashMap<crate::symbol::SymbolId, usize>> = None;
    let mut who_wants = |ctx: &Context<E>, id: crate::symbol::SymbolId| -> String {
        let map = referencers.get_or_insert_with(|| {
            let mut map = std::collections::HashMap::new();
            for (obj_idx, obj) in ctx.objs.iter().enumerate() {
                if !obj.is_alive {
                    continue;
                }
                let r = obj.global_range();
                for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                    if !nlist.is_stab() && nlist.n_type() == N_UNDF && !nlist.is_common() {
                        map.entry(sym_id).or_insert(obj_idx);
                    }
                }
            }
            map
        });
        match map.get(&id) {
            Some(&obj_idx) => file_display(&ctx.objs[obj_idx]).to_string(),
            None if initial.contains(&id) => "<initial-undefines>".to_string(),
            None => "<synthesized>".to_string(),
        }
    };

    for i in undef {
        let sym = &ctx.symbols[i];
        if referenced[i].load(Ordering::Relaxed) {
            let allowed = (ctx.args.undefined_dynamic_lookup
                || ctx.args.allowed_undefined.iter().any(|n| n == sym.name()))
                && !initial.contains(&(i as crate::symbol::SymbolId));
            if allowed {
                if ctx.args.undefined_warning {
                    crate::warn!("undefined symbol: {}", ctx.symbols[i]);
                }
                let sym = &mut ctx.symbols[i];
                sym.set_file(FileId::Dylib((usize::MAX) as u32));
                sym.set_is_imported(true);
                sym.set_is_extern(true);
            } else {
                let file = who_wants(ctx, i as u32);
                error!("undefined symbol: {}: {}", file, ctx.symbols[i]);
            }
        }
    }
}

/// The symbols something in the output refers to: the target of a live
/// relocation, or a name -u, -e or -alias insists on.
fn referenced_symbols<E: Target>(ctx: &Context<E>) -> Vec<std::sync::atomic::AtomicBool> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let referenced: Vec<AtomicBool> =
        (0..ctx.symbols.syms.len()).map(|_| AtomicBool::new(false)).collect();
    ctx.isecs.par_iter().filter(|isec| isec.is_alive()).for_each(|isec| {
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) {
                referenced[id as usize].store(true, Ordering::Relaxed);
            }
        }
    });
    for name in ctx
        .args
        .forced_undefined
        .iter()
        .chain((ctx.args.output_type == MH_EXECUTE).then_some(&ctx.args.entry))
        .chain(ctx.args.aliases.iter().map(|(base, _)| base))
    {
        if let Some(id) = ctx.symbols.get(name) {
            referenced[id as usize].store(true, Ordering::Relaxed);
        }
    }
    referenced
}

/// --print-dependencies prints, for every undefined symbol of every
/// object, which file's definition satisfied it - a line per edge:
/// "referencer<TAB>provider<TAB>u<TAB>symbol". Xcode's newer ld
/// grew this for build-graph auditing; it makes questions like "why
/// is this archive member in my binary" one grep.
pub fn print_dependencies<E: Target>(ctx: &Context<E>) {
    if !ctx.args.print_dependencies {
        return;
    }
    for obj in &ctx.objs {
        if !obj.is_alive {
            continue;
        }
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if nlist.is_stab() || nlist.n_type() != N_UNDF || nlist.is_common() {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            let provider = match sym.file() {
                Some(FileId::Obj(idx)) => {
                    let idx = idx as usize;
                    if !ctx.objs[idx].is_alive || std::ptr::eq(&raw const ctx.objs[idx], obj) {
                        continue;
                    }
                    file_display(&ctx.objs[idx])
                }
                Some(FileId::Dylib(idx)) if idx != u32::MAX => {
                    crate::util::display(&ctx.dylibs[idx as usize].install_name)
                }
                _ => continue,
            };
            println!("{}\t{}\tu\t{}", file_display(obj), provider, sym.name());
        }
    }
}

/// -t traces the link's inputs: one line per file that contributes,
/// objects by path (archive members as archive(member)) and dylib
/// stubs by the path they were found at. In mold's model every input
/// is parsed eagerly, so rather than logging opens - which would list
/// archive members the link then discards - the trace reports what
/// actually took part.
pub fn print_trace<E: Target>(ctx: &Context<E>) {
    if !ctx.args.trace {
        return;
    }
    for (i, obj) in ctx.objs.iter().enumerate() {
        if obj.is_alive && !ctx.is_internal(i) {
            println!("{}", file_display(obj));
        }
    }
    for dylib in &ctx.dylibs {
        println!("{}", dylib.path.display());
    }
}

/// -why_load reports what dragged each archive member into the link:
/// "_symbol forced load of archive.a(member.o)", in ld64's wording.
/// Members loaded unconditionally (-all_load, -force_load) are
/// reported with the option as the reason.
pub fn print_why_load<E: Target>(ctx: &Context<E>) {
    if !ctx.args.why_load {
        return;
    }
    // ld-prime reports, on stderr, the members an option loads as it
    // parses the archives - an archive's last member first; archives
    // parsed in parallel interleave - before resolution names the ones
    // a symbol pulled in. -all_load counts as -force_load; -ObjC names
    // itself.
    let force_loaded: std::collections::HashSet<&Path> = ctx
        .args
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            InputArg::ForceLoad(path) => Some(path.as_path()),
            _ => None,
        })
        .collect();
    let members =
        || ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive && obj.mf.parent.is_some());
    let forced: Vec<&input_files::ObjectFile> =
        members().filter(|(idx, _)| !ctx.why_load.contains_key(idx)).map(|(_, obj)| obj).collect();
    for run in forced.chunk_by(|a, b| std::ptr::eq(a.mf.parent.unwrap(), b.mf.parent.unwrap())) {
        let archive = run[0].mf.parent.unwrap();
        let option = if ctx.args.all_load || force_loaded.contains(archive.name.as_path()) {
            "-force_load"
        } else {
            "-ObjC"
        };
        for obj in run.iter().rev() {
            eprintln!("{option} caused load of {}", resolved_file_name(obj.mf));
        }
    }
    for (idx, obj) in members() {
        if let Some(name) = ctx.why_load.get(&idx) {
            eprintln!("'{name}' caused load of {}", resolved_file_name(obj.mf));
        }
    }
}

/// A file name for diagnostics: the object's path. Archive members
/// already carry their "archive(member)" form as their mapped-file
/// name.
pub(crate) fn file_display(obj: &crate::input_files::ObjectFile) -> std::borrow::Cow<'_, str> {
    obj.mf.name.to_string_lossy()
}

/// A file name as ld-prime spells it where it says where an input is:
/// its real path (symlinks and relative steps resolved), or for an
/// archive member the archive's real path, the member's position among
/// the archive's entries and its name: "/abs/libfoo.a[2](foo.o)".
pub(crate) fn resolved_file_name(mf: &MappedFile) -> String {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if let Some(ar) = mf.parent
        && let Some(index) = crate::archive_file::member_index(mf)
    {
        let full = path_bytes(&mf.name);
        let member = full
            .strip_prefix(path_bytes(&ar.name))
            .and_then(|rest| rest.strip_prefix(b"("))
            .and_then(|rest| rest.strip_suffix(b")"))
            .unwrap_or(full);
        return format!("{}[{index}]({})", real(&ar.name).display(), crate::util::display(member));
    }
    real(&mf.name).display().to_string()
}

/// Warns about each dylib the command line links that nothing binds
/// to. ld-prime does so by default for a dylib bound for the dyld shared
/// cache - one installed in /usr/lib or /System/Library, unless
/// -not_for_dyld_shared_cache - where each needless load costs every
/// process, and for any output under -warn_unused_dylibs. A -needed_* or
/// -reexport_* library is linked on purpose, and libSystem, libc++ and
/// Foundation, which compiler drivers and project templates link by
/// habit, are let off.
fn warn_unused_dylibs<E: Target>(ctx: &Context<E>) {
    let install_name = crate::chunks::output_install_name(ctx);
    let for_shared_cache = ctx.args.output_type == MH_DYLIB
        && !ctx.args.not_for_dyld_shared_cache
        && (install_name.starts_with(b"/usr/lib/")
            || install_name.starts_with(b"/System/Library/"));
    if !ctx.args.warn_unused_dylibs.unwrap_or(for_shared_cache) {
        return;
    }
    const EXEMPT: [&[u8]; 3] = [
        b"/usr/lib/libSystem.B.dylib",
        b"/usr/lib/libc++.1.dylib",
        b"/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation",
    ];
    let mut bound = vec![false; ctx.dylibs.len()];
    for sym in &ctx.symbols.syms {
        if let Some(FileId::Dylib(idx)) = sym.file()
            && idx != u32::MAX
        {
            bound[idx as usize] = true;
        }
    }
    for (i, dylib) in ctx.dylibs.iter().enumerate() {
        if !bound[i]
            && !dylib.is_implicit
            && !dylib.is_autolinked
            && !dylib.is_needed
            && !dylib.is_reexported
            && !dylib.is_bundle_loader
            && !EXEMPT.contains(&dylib.install_name.as_slice())
        {
            crate::warn!(
                "linking with ({}) but not using any symbols from it",
                crate::util::display(&dylib.install_name)
            );
        }
    }
}

/// Drops load commands for dylibs no symbol binds to
/// (-dead_strip_dylibs). Bind records name dylibs by their 1-based
/// load-command ordinal, so surviving dylibs are renumbered and symbol
/// origins remapped.
pub fn dead_strip_dylibs<E: Target>(ctx: &mut Context<E>) {
    warn_unused_dylibs(ctx);
    // A dylib built with -mark_dead_strippable_dylib asks every
    // linker to drop it when unused, so those are stripped even
    // without -dead_strip_dylibs; so is an auto-linked one, which
    // ld64 treats as a hint: NetNewsWire's auto-link options name 43
    // frameworks and Swift overlays nothing in it binds to, and
    // ld-prime lists none of them.
    let strippable = |dylib: &crate::input_files::DylibFile| {
        ctx.args.dead_strip_dylibs
            || dylib.is_dead_strippable
            || dylib.is_autolinked
            || dylib.is_implicit
    };

    // libSystem stays whether or not anything binds to it: ld-prime
    // keeps it under -dead_strip_dylibs in an image that binds nothing
    // from it (dyld needs it to run anything), so a dylib exporting
    // only its own functions still lists it.
    let mut used = vec![false; ctx.dylibs.len()];
    for (i, dylib) in ctx.dylibs.iter().enumerate() {
        used[i] = dylib.is_needed
            || dylib.install_name == b"/usr/lib/libSystem.B.dylib"
            || !strippable(dylib);
    }
    // A dylib every reference to which is a weak import loads weakly
    // (LC_LOAD_WEAK_DYLIB), as ld64 does: the Swift overlays a program
    // reaches only through their weak __swift_FORCE_LOAD_$_ symbols
    // come out weak, libswiftCore strong.
    let mut bound = vec![0u32; ctx.dylibs.len()];
    let mut weak = vec![0u32; ctx.dylibs.len()];
    for sym in &ctx.symbols.syms {
        if let Some(FileId::Dylib(idx)) = sym.file()
            && idx != u32::MAX
        {
            used[idx as usize] = true;
            if sym.is_used() {
                bound[idx as usize] += 1;
                weak[idx as usize] += sym.is_weak_ref() as u32;
            }
        }
    }
    for (i, dylib) in ctx.dylibs.iter_mut().enumerate() {
        if bound[i] > 0 && weak[i] == bound[i] {
            dylib.is_weak = true;
        }
    }

    let mut remap = vec![usize::MAX; ctx.dylibs.len()];
    let old = std::mem::take(&mut ctx.dylibs);
    for (i, dylib) in old.into_iter().enumerate() {
        if used[i] {
            remap[i] = ctx.dylibs.len();
            ctx.dylibs.push(dylib);
        }
    }

    for sym in &mut ctx.symbols.syms {
        if let Some(FileId::Dylib(idx)) = sym.file()
            && idx != u32::MAX
        {
            sym.set_file(FileId::Dylib(remap[idx as usize] as u32));
        }
    }

    // Ordinals (and so the load commands) in ld64's order: the
    // libraries named on the command line or by auto-link options in
    // naming order, then the implicitly loaded ones by install name.
    let mut order: Vec<usize> =
        (0..ctx.dylibs.len()).filter(|&i| !ctx.dylibs[i].is_bundle_loader).collect();
    order.sort_by(|&a, &b| {
        let (da, db) = (&ctx.dylibs[a], &ctx.dylibs[b]);
        da.load_order.cmp(&db.load_order).then_with(|| da.install_name.cmp(&db.install_name))
    });
    for (ordinal, &i) in order.iter().enumerate() {
        ctx.dylibs[i].dylib_idx = ordinal as i32 + 1;
    }
}

/// Decides which symbols need a stub or a GOT slot, from how relocations
/// refer to them.
pub fn scan_relocations<E: Target>(ctx: &mut Context<E>) {
    // Classification reads only; collect it on all cores. The apply
    // loop below stays serial so GOT and stub slots keep their
    // deterministic first-seen order.
    let ctx_ref: &Context<E> = ctx;
    let classes: Vec<(crate::symbol::SymbolId, RelocClass)> = ctx_ref
        .isecs
        .par_iter()
        .filter(|isec| isec.is_alive())
        .flat_map_iter(|isec| {
            crate::input_files::isec_relocs_of(&ctx_ref.objs, isec).iter().filter_map(move |rel| {
                let id = ctx_ref.reloc_target_sym(isec.file as usize, rel)?;
                let mut class = E::classify_reloc(rel.r_type);
                // A relaxable GOT load of a local symbol needs no
                // slot at all; an unrelaxable one is an ordinary GOT
                // reference.
                if class == RelocClass::GotLoad
                    && !E::can_relax_got_load(isec.data(), rel.offset, rel.r_type)
                {
                    class = RelocClass::Got;
                }
                // Plain references need no slot of any kind, and
                // they are the overwhelming majority; dropping them
                // here keeps the collected list (and the serial
                // apply loop below) small. The TLV/regular mismatch
                // check needs the TLV side only: a plain reference
                // to a thread-local is caught because thread-locals
                // are reached exclusively through TLV relocations,
                // checked against the symbol below either way.
                if class == RelocClass::Plain && !is_thread_local_sym(ctx_ref, id) {
                    return None;
                }
                Some((id, class))
            })
        })
        .collect();

    for (id, class) in classes {
        let sym = &ctx.symbols[id];

        // Thread-locals live behind __thread_vars descriptors, so the
        // reference kind must agree with the symbol: a TLV load of
        // ordinary data would treat the variable's bytes as a
        // descriptor, and an ordinary load of a TLV would read the
        // descriptor as data. ld64 rejects both directions.
        if is_thread_local_sym(ctx, id) != matches!(class, RelocClass::Tlv) {
            fatal!("illegal thread local variable reference to regular symbol `{sym}`");
        }

        match class {
            // A call to a symbol dyld resolves by weak lookup - one of
            // this image's own coalescable weak definitions, or a
            // dylib's weak export - goes through a stub and a GOT slot,
            // never a lazy pointer, as ld64 does.
            RelocClass::Branch if ctx.binds_weak_lookup(id) => {
                add_stub(ctx, id);
                add_got(ctx, id);
            }
            RelocClass::Branch if sym.is_imported() => {
                // A stub jumps through the symbol's lazy pointer, or,
                // without lazy binding, its GOT slot.
                add_stub(ctx, id);
                if ctx.lazy_binding() {
                    ensure_stub_binder(ctx);
                } else {
                    add_got(ctx, id);
                }
            }
            RelocClass::Got => add_got(ctx, id),
            RelocClass::GotLoad if !ctx.can_relax_got(id) => add_got(ctx, id),
            // A TLV load relaxes to the descriptor's address like a GOT
            // load; one dyld must fill - an imported thread-local, or a
            // weak one coalesced across images (C++'s inline
            // thread_local) - goes through an ordinary __got entry, as
            // in ld-prime (no __thread_ptrs section, chained or classic).
            RelocClass::Tlv if !ctx.can_relax_got(id) => add_got(ctx, id),
            _ => {}
        }
    }
}

/// True if the symbol resolves to a TLV descriptor: a definition in a
/// S_THREAD_LOCAL_VARIABLES section, or a dylib export listed as
/// thread-local. Symbols left to runtime lookup pass as either.
pub fn is_thread_local_sym<E: Target>(ctx: &Context<E>, id: crate::symbol::SymbolId) -> bool {
    let sym = &ctx.symbols[id];
    match sym.file() {
        Some(FileId::Obj(_)) => sym.input_section().map(|i| i as usize).is_some_and(|isec| {
            ctx.hdr_of(&ctx.isecs[isec]).flags & SECTION_TYPE == S_THREAD_LOCAL_VARIABLES
        }),
        Some(FileId::Dylib(idx)) => {
            idx != u32::MAX && ctx.dylibs[idx as usize].tlv_exports.contains(sym.name())
        }
        _ => false,
    }
}

/// The synthesized objc stubs call _objc_msgSend through a GOT slot of
/// their own: ld-prime binds _objc_msgSend twice when a stub or a GOT
/// load elsewhere needs a slot for it as well.
pub fn scan_objc_stubs<E: Target>(ctx: &mut Context<E>) {
    if let Some(id) = ctx.objc_stubs.msgsend_sym {
        ctx.objc_stubs.msgsend_got_idx = ctx.got.got_syms.len() as u32;
        ctx.got.got_syms.push(id);
    }

    // Stub i loads slot i of the __objc_selrefs tail, which points at
    // its selector's name. ld-prime coalesces both with an input's of
    // the same selector and keeps its own: the slot replaces every
    // input selector reference to that selector (one slot per selector,
    // as the runtime uniques one __objc_selrefs), and the name is an
    // input's __objc_methname string where one spells it, only names
    // no input has going in the __objc_methname tail.
    let mut name_of: hashbrown::HashMap<&'static [u8], u32> = hashbrown::HashMap::new();
    let mut absorbed: Vec<(u32, u32)> = Vec::new();
    {
        let stub_of: hashbrown::HashMap<&[u8], u32> = (ctx.objc_stubs.symbols.iter().enumerate())
            .map(|(i, (_, sel))| (sel.as_bytes(), i as u32))
            .collect();
        for i in 0..ctx.isecs.len() {
            let isec = &ctx.isecs[i];
            if stub_of.is_empty()
                || !isec.is_alive()
                || ctx.is_internal(isec.file as usize)
                || isec.replacement != crate::input_sections::NO_REPLACEMENT
            {
                continue;
            }
            let h = ctx.hdr_of(isec);
            if h.sectname() == "__objc_methname" && h.section_type() == S_CSTRING_LITERALS {
                name_of.entry(cstring_of(isec.data())).or_insert(i as u32);
            } else if h.sectname() == "__objc_selrefs"
                && h.section_type() == S_LITERAL_POINTERS
                && isec.size == 8
                && let Some(target) = objc_pointer_at(ctx, i as u32, 0)
                && let Some((name, 0)) = objc_ref_location(ctx, target)
                && let Some(&stub) = stub_of.get(cstring_of(ctx.isecs[name as usize].data()))
            {
                absorbed.push((i as u32, stub));
            }
        }
    }
    absorb_selrefs(ctx, absorbed);

    let stubs = &mut ctx.objc_stubs;
    for i in 0..stubs.symbols.len() {
        let sel = stubs.symbols[i].1.as_bytes();
        let name = name_of.get(sel).copied();
        stubs.name_isec.push(name.unwrap_or(u32::MAX));
        stubs.methname_offs.push(stubs.methname_data.len() as u64);
        if name.is_none() {
            stubs.methname_data.extend_from_slice(sel);
            stubs.methname_data.push(0);
        }
    }
}

/// Replaces input selector references by the objc stub slots that take
/// them over: each by a synthetic subsection standing for its stub's
/// slot, placed once the __objc_selrefs tail is. `absorbed` pairs an
/// input selector reference with its stub.
fn absorb_selrefs<E: Target>(ctx: &mut Context<E>, absorbed: Vec<(u32, u32)>) {
    if absorbed.is_empty() {
        return;
    }
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__objc_selrefs"),
        segname: str_to_name("__DATA"),
        p2align: 3,
        flags: S_LITERAL_POINTERS,
        ..Default::default()
    });
    let mut synth_of: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    for (input, stub) in absorbed {
        let synth = match synth_of.get(&stub) {
            Some(&synth) => synth,
            None => {
                ctx.isecs.push(InputSection {
                    file,
                    shndx,
                    p2align: 3,
                    input_addr: 0,
                    size: 8,
                    contents: 0,
                    rel_offset: 0,
                    nrels: 0,
                    output_section: u32::MAX,
                    offset: u32::MAX,
                    flags: std::sync::atomic::AtomicU8::new(0),
                    replacement: crate::input_sections::NO_REPLACEMENT,
                    unwind_offset: 0,
                    nunwind: 0,
                });
                let synth = (ctx.isecs.len() - 1) as u32;
                synth_of.insert(stub, synth);
                ctx.objc_stubs.absorbed.push((synth, stub));
                synth
            }
        };
        ctx.isecs[input as usize].replacement = synth;
    }
}

/// A C string's bytes, up to its terminating NUL.
fn cstring_of(data: &[u8]) -> &[u8] {
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}

/// Personality functions are referenced from __unwind_info through the
/// GOT.
pub fn scan_unwind_personalities<E: Target>(ctx: &mut Context<E>) {
    let mut personalities: Vec<_> =
        ctx.unwind_records.iter().filter_map(|rec| rec.personality()).collect();
    personalities.extend(ctx.fdes.iter().filter_map(|fde| ctx.cies[fde.cie as usize].personality));
    for id in personalities {
        add_got(ctx, id);
    }
}

fn add_stub<E: Target>(ctx: &mut Context<E>, id: crate::symbol::SymbolId) {
    if ctx.sym_aux(id).stub_idx == crate::symbol::NO_IDX {
        ctx.sym_aux_mut(id).stub_idx = ctx.stubs.symbols.len() as u32;
        ctx.stubs.symbols.push(id);
    }
}

fn add_got<E: Target>(ctx: &mut Context<E>, id: crate::symbol::SymbolId) {
    if ctx.sym_aux(id).got_idx == crate::symbol::NO_IDX {
        ctx.sym_aux_mut(id).got_idx = ctx.got.got_syms.len() as u32;
        ctx.got.got_syms.push(id);
    }
}

/// Lays out __stubs and __got in ld-prime's order rather than in the
/// order relocations first reached them. Stubs - and with them the
/// lazy pointers, their helper entries and the indirect symbol table -
/// sort by name across all libraries. GOT slots sort by what fills
/// them: the image's own addresses first, then the -bundle_loader
/// executable's symbols, each library's in load-command order, the
/// weak-lookup binds, and flat lookups no library provides; by name
/// within each. Runs once the dylib ordinals are final.
pub fn sort_stubs_and_got<E: Target>(ctx: &mut Context<E>) {
    let mut stubs = std::mem::take(&mut ctx.stubs.symbols);
    stubs.par_sort_by_key(|&id| crate::util::name_sort_key(ctx.symbols[id].name()));
    for (i, &id) in stubs.iter().enumerate() {
        ctx.sym_aux_mut(id).stub_idx = i as u32;
    }
    if ctx.lazy_binding() {
        ctx.stubs.lazy = (0..stubs.len() as u32)
            .filter(|&i| !ctx.binds_weak_lookup(stubs[i as usize]))
            .collect();
    }
    ctx.stubs.symbols = stubs;

    // The objc stubs' own _objc_msgSend slot goes before the one other
    // references share.
    let got = std::mem::take(&mut ctx.got.got_syms);
    let objc = ctx.objc_stubs.msgsend_got_idx as usize;
    let mut order: Vec<usize> = (0..got.len()).collect();
    order.par_sort_by_key(|&i| {
        let id = got[i];
        (got_rank(ctx, id), crate::util::name_sort_key(ctx.symbols[id].name()), i != objc)
    });
    for (slot, &i) in order.iter().enumerate() {
        if i == objc {
            ctx.objc_stubs.msgsend_got_idx = slot as u32;
        } else {
            ctx.sym_aux_mut(got[i]).got_idx = slot as u32;
        }
    }
    ctx.got.got_syms = order.iter().map(|&i| got[i]).collect();
}

/// A GOT slot's group in ld-prime's order (see sort_stubs_and_got).
fn got_rank<E: Target>(ctx: &Context<E>, id: crate::symbol::SymbolId) -> i64 {
    if ctx.binds_weak_lookup(id) {
        return i64::MAX - 1;
    }
    match ctx.symbols[id].file() {
        Some(FileId::Dylib(u32::MAX)) => i64::MAX,
        Some(FileId::Dylib(d)) if ctx.dylibs[d as usize].is_bundle_loader => 0,
        Some(FileId::Dylib(d)) => ctx.dylibs[d as usize].dylib_idx as i64,
        _ => -1,
    }
}

/// Folds __objc_classrefs into __got, as ld-prime does from a
/// deployment target of macOS 15 on. A class reference is an 8-byte
/// slot holding a class's address, fixed up by dyld - exactly what a
/// GOT entry for the class symbol is. So the code that loads a class
/// from its slot (adrp/ldr on arm64, a RIP-relative mov on x86-64) is
/// retargeted at the class symbol as a GOT load: an imported class is
/// then read from its GOT entry, shared by every reference in the
/// image, and a class defined in the image relaxes to computing the
/// address directly (adrp/add, lea), needing no slot at all. The image
/// has no __objc_classrefs section and none of the slots' local
/// symbols (_OBJC_CLASSLIST_REFERENCES_$_n); the runtime only ever
/// read the section to remap references to swapped classes, which
/// macOS 15's dyld handles through the GOT. ld-prime turned
/// NetNewsWire's 581 class references into 168 GOT entries.
///
/// A reference that cannot become a GOT load (the slot's address
/// taken, or a pointer to it) keeps the slot: it is replaced by a
/// synthetic subsection standing for the class's GOT entry.
///
/// On arm64 ld-prime relaxes a class's loads only if each adrp of its
/// slot is followed, within the function, by one @PAGEOFF use before
/// the next adrp of it (-O0 code can load twice through one adrp). If
/// any reference of the class's, in any object, pairs up otherwise,
/// no reference is rewritten: the slot moves to the GOT and every
/// load reads it there.
pub fn fold_objc_classrefs<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !objc_refs_are_const(ctx) {
        return;
    }
    let slots: Vec<_> = (0..ctx.objs.len()).map(|i| classref_slots(ctx, i)).collect();
    let uses: Vec<_> = (0..ctx.objs.len()).map(|i| classref_uses(ctx, i, &slots[i])).collect();
    let mut unpaired = hashbrown::HashSet::new();
    let pairs: Vec<_> =
        (0..ctx.objs.len()).map(|i| pair_classref_uses(ctx, i, &uses[i], &mut unpaired)).collect();
    let mut got_hdr: Option<(u32, u32)> = None;
    for obj_idx in 0..ctx.objs.len() {
        let slots = &slots[obj_idx];
        if slots.is_empty() {
            continue;
        }

        // Retarget the loads; note the slots something else refers to.
        // A pair's halves go together, as its offset half decides.
        let mut keep: hashbrown::HashSet<u32> = hashbrown::HashSet::new();
        let partner: hashbrown::HashMap<usize, usize> =
            pairs[obj_idx].iter().flat_map(|&(page, off)| [(page, off), (off, off)]).collect();
        for u in &uses[obj_idx] {
            let rel = ctx.objs[obj_idx].relocs[u.k];
            let decider = ctx.objs[obj_idx].relocs[partner.get(&u.k).copied().unwrap_or(u.k)];
            let data = ctx.isecs[u.isec as usize].data();
            match (E::got_load_form(rel.r_type), E::got_load_form(decider.r_type)) {
                (Some(form), Some(dform))
                    if !unpaired.contains(&u.class)
                        && E::can_relax_got_load(data, decider.offset, dform) =>
                {
                    let r = &mut ctx.objs[obj_idx].relocs[u.k];
                    r.r_type = form;
                    r.set_target(RelocTarget::Sym(slots[&u.slot].0));
                }
                _ => {
                    keep.insert(u.slot);
                }
            }
        }

        // In input order: the GOT slots the classes get (and with
        // them the slot addresses every load encodes) follow the
        // object's class-reference order, not the hash map's, which
        // hashbrown seeds afresh for every process.
        let mut ordered: Vec<(u32, crate::symbol::SymbolId)> =
            slots.iter().map(|(&slot, &(_, class))| (slot, class)).collect();
        ordered.sort_unstable_by_key(|&(slot, _)| slot);
        for (slot, class) in ordered {
            if ctx.symbols[class].is_imported() || keep.contains(&slot) {
                add_got(ctx, class);
            }
            if !keep.contains(&slot) {
                ctx.isecs[slot as usize].set_alive(false);
                continue;
            }
            // A synthetic subsection standing for the GOT entry; not
            // alive, since the __got chunk writes the slot and the
            // slot's local symbol is not emitted.
            let (file, shndx) = *got_hdr.get_or_insert_with(|| {
                let (file, shndx) = ctx.add_synthetic_section(MachSection {
                    sectname: str_to_name("__got"),
                    segname: str_to_name(data_seg(ctx)),
                    p2align: 3,
                    flags: S_NON_LAZY_SYMBOL_POINTERS,
                    ..Default::default()
                });
                (file, shndx)
            });
            ctx.isecs.push(InputSection {
                file,
                shndx,
                p2align: 3,
                input_addr: 0,
                size: 8,
                contents: 0,
                rel_offset: 0,
                nrels: 0,
                output_section: u32::MAX,
                offset: u32::MAX,
                flags: std::sync::atomic::AtomicU8::new(0),
                replacement: crate::input_sections::NO_REPLACEMENT,
                unwind_offset: 0,
                nunwind: 0,
            });
            let synth = (ctx.isecs.len() - 1) as u32;
            ctx.isecs[slot as usize].replacement = synth;
            ctx.got.objc_classref_slots.push((synth, class));
        }
    }
}

/// An object's class-reference slots: slot subsection -> the class
/// symbol (its index in the object, and globally).
fn classref_slots<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
) -> hashbrown::HashMap<u32, (u32, crate::symbol::SymbolId)> {
    let mut slots = hashbrown::HashMap::new();
    if !ctx.objs[obj_idx].is_alive {
        return slots;
    }
    for &i in &ctx.objs[obj_idx].subsecs {
        let isec = &ctx.isecs[i];
        if !isec.is_alive()
            || isec.replacement != crate::input_sections::NO_REPLACEMENT
            || isec.size != 8
        {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if h.segname() != "__DATA" || h.sectname() != "__objc_classrefs" {
            continue;
        }
        let rels = ctx.isec_relocs(i as usize);
        if rels.len() != 1 {
            continue;
        }
        let rel = rels[0];
        let RelocTarget::Sym(idx) = rel.target() else { continue };
        if E::classify_reloc(rel.r_type) != RelocClass::Plain
            || rel.size != 8
            || rel.is_pcrel
            || rel.is_subtracted
            || rel.addend != 0
        {
            continue;
        }
        slots.insert(i, (idx, ctx.objs[obj_idx].symbols[idx as usize]));
    }
    slots
}

/// A reference to a class-reference slot: relocation `k` of the
/// object, in subsection `isec`.
struct ClassrefUse {
    isec: u32,
    k: usize,
    slot: u32,
    class: crate::symbol::SymbolId,
}

/// The references an object's code and data make to its class-reference
/// slots, in subsection and then address order.
fn classref_uses<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    slots: &hashbrown::HashMap<u32, (u32, crate::symbol::SymbolId)>,
) -> Vec<ClassrefUse> {
    let mut uses = Vec::new();
    if slots.is_empty() {
        return uses;
    }
    let obj = &ctx.objs[obj_idx];
    for &i in &obj.subsecs {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || slots.contains_key(&i) {
            continue;
        }
        for k in isec.rel_offset as usize..(isec.rel_offset + isec.nrels) as usize {
            let rel = obj.relocs[k];
            let slot = match rel.target() {
                RelocTarget::Section(t) if rel.addend == 0 => t,
                RelocTarget::Sym(idx) => {
                    let sym = &ctx.symbols[obj.symbols[idx as usize]];
                    match sym.input_section() {
                        Some(t) if sym.value == 0 && rel.addend == 0 => t,
                        _ => continue,
                    }
                }
                _ => continue,
            };
            if let Some(&(_, class)) = slots.get(&slot) {
                uses.push(ClassrefUse { isec: i, k, slot, class });
            }
        }
    }
    uses
}

/// Pairs an object's page-and-offset references to class slots (arm64's
/// adrp then ldr or add), each page half with the next offset half of
/// the same class in its subsection, and returns the pairs' relocation
/// indices. A class with a half left over joins `unpaired`.
fn pair_classref_uses<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    uses: &[ClassrefUse],
    unpaired: &mut hashbrown::HashSet<crate::symbol::SymbolId>,
) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut open: hashbrown::HashMap<crate::symbol::SymbolId, usize> = hashbrown::HashMap::new();
    for (n, u) in uses.iter().enumerate() {
        match E::page_pair_half(ctx.objs[obj_idx].relocs[u.k].r_type) {
            Some(true) => {
                if open.insert(u.class, u.k).is_some() {
                    unpaired.insert(u.class);
                }
            }
            Some(false) => match open.remove(&u.class) {
                Some(page) => pairs.push((page, u.k)),
                None => {
                    unpaired.insert(u.class);
                }
            },
            None => {}
        }
        if uses.get(n + 1).is_none_or(|next| next.isec != u.isec) {
            unpaired.extend(open.drain().map(|(class, _)| class));
        }
    }
    pairs
}

/// A reference held by a rewritten method-list entry, resolved to an
/// address when the list is written.
#[derive(Clone, Copy, Debug)]
pub enum ObjcRef {
    /// A subsection plus offset.
    Isec(u32, u64),
    /// A symbol plus addend.
    Sym(crate::symbol::SymbolId, i64),
    /// Slot `n` of the synthesized selector references in the
    /// __objc_selrefs tail (the objc stubs' slots come first).
    TailSelref(usize),
    Null,
}

#[derive(Clone, Copy, Debug)]
pub struct ObjcMethod {
    /// The selector reference the entry points at (a slot holding the
    /// uniqued selector), not the selector string.
    pub name: ObjcRef,
    pub types: ObjcRef,
    pub imp: ObjcRef,
}

#[derive(Debug)]
pub struct ObjcMethList {
    /// The synthetic subsection standing for the rewritten list in
    /// __TEXT,__objc_methlist.
    pub isec: u32,
    pub methods: Vec<ObjcMethod>,
}

fn objc_relative_method_lists<E: Target>(ctx: &Context<E>) -> bool {
    // ld-prime converts method lists in every arm64 image, and on
    // x86-64 in dylibs and bundles only: an x86-64 executable keeps
    // the compiler's absolute lists at any deployment target.
    ctx.args.objc_relative_method_lists.unwrap_or_else(|| {
        (E::CPUTYPE == crate::macho::CPU_TYPE_ARM64 || ctx.args.output_type != MH_EXECUTE)
            && ctx.args.platform == crate::macho::PLATFORM_MACOS
            && ctx.args.platform_minos >= crate::macho::encode_version(11, 0, 0)
    })
}

/// A class's ro data: class_t.data at offset 32, whose low two bits a
/// Swift class uses as flags (FAST_IS_SWIFT_STABLE), so the record
/// itself sits at the pointer with those bits cleared.
fn objc_class_ro<E: Target>(ctx: &Context<E>, cls: (u32, u64)) -> Option<(u32, u64)> {
    let (isec, off) =
        objc_pointer_at(ctx, cls.0, cls.1 + 32).and_then(|r| objc_ref_location(ctx, r))?;
    Some((isec, off & !3))
}

/// The relocation of the pointer field at `off` in a subsection, as
/// (object, index into its relocation arena), for rewriting it.
fn objc_pointer_reloc<E: Target>(ctx: &Context<E>, isec: u32, off: u64) -> Option<(usize, usize)> {
    let sec = &ctx.isecs[isec as usize];
    if ctx.is_internal(sec.file as usize) {
        return None;
    }
    let k = ctx
        .isec_relocs(isec as usize)
        .iter()
        .position(|r| r.offset as u64 == off && r.size == 8 && !r.is_pcrel && !r.is_subtracted)?;
    Some((sec.file as usize, sec.rel_offset as usize + k))
}

/// The pointer stored at `off` in a subsection: the target of the
/// 8-byte relocation there, if any.
fn objc_pointer_at<E: Target>(ctx: &Context<E>, isec: u32, off: u64) -> Option<ObjcRef> {
    let sec = &ctx.isecs[isec];
    if ctx.is_internal(sec.file as usize) {
        return None;
    }
    let rel = ctx
        .isec_relocs(isec as usize)
        .iter()
        .find(|r| r.offset as u64 == off && r.size == 8 && !r.is_pcrel && !r.is_subtracted)?;
    if E::classify_reloc(rel.r_type) != RelocClass::Plain {
        return None;
    }
    Some(match rel.target() {
        RelocTarget::Sym(idx) => {
            ObjcRef::Sym(ctx.objs[sec.file as usize].symbols[idx as usize], rel.addend)
        }
        RelocTarget::Section(t) => ObjcRef::Isec(t, rel.addend as u64),
    })
}

/// A reference's location as (live subsection, offset), for data
/// defined in this link; None for an import or an absolute.
fn objc_ref_location<E: Target>(ctx: &Context<E>, r: ObjcRef) -> Option<(u32, u64)> {
    let (isec, off) = match r {
        ObjcRef::Isec(isec, off) => (isec, off),
        ObjcRef::Sym(id, addend) => {
            let sym = &ctx.symbols[id];
            let isec = sym.input_section()?;
            (isec, (sym.value as i64 + addend) as u64)
        }
        _ => return None,
    };
    let isec = ctx.resolve_isec(isec as usize) as u32;
    if !ctx.isecs[isec as usize].is_alive() {
        return None;
    }
    Some((isec, off))
}

/// Rewrites the Objective-C method lists in the relative form, as
/// ld64 does from a deployment target of macOS 11 (its
/// -objc_relative_method_lists). A classic entry is three pointers
/// (selector string, type string, implementation), each a fixup dyld
/// must apply and the runtime must then unique; a relative entry is
/// three 32-bit self-relative offsets, the first to a selector
/// reference slot (already uniqued by dyld), needing no fixups at
/// all, and the lists move to read-only __TEXT,__objc_methlist.
///
/// The lists are found the way the runtime finds them: through
/// __objc_classlist (class and metaclass ro data), __objc_catlist,
/// __objc_protolist (all four lists of a protocol) and Swift's
/// __objc_clsrolist. A selector with no reference in any input gets
/// one synthesized in the __objc_selrefs tail. A list is left alone
/// when it is not the whole of its subsection or not in the classic
/// 24-byte form.
pub fn convert_objc_method_lists<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !objc_relative_method_lists(ctx) {
        return;
    }

    // Selector references the inputs already have, by the selector
    // string subsection they point at.
    let mut selref_of: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) || isec.size != 8 {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if h.sectname() != "__objc_selrefs" || h.section_type() != S_LITERAL_POINTERS {
            continue;
        }
        let Some(target) = objc_pointer_at(ctx, i as u32, 0) else { continue };
        if let Some((name, 0)) = objc_ref_location(ctx, target) {
            let slot = ctx.resolve_isec(i) as u32;
            selref_of.entry(name).or_insert(slot);
        }
    }

    // Every method list the runtime would visit.
    let mut lists: Vec<u32> = Vec::new();
    let mut seen: hashbrown::HashSet<u32> = hashbrown::HashSet::new();
    let mut classes_seen: hashbrown::HashSet<(u32, u64)> = hashbrown::HashSet::new();
    let mut note = |ctx: &Context<E>, r: Option<ObjcRef>, lists: &mut Vec<u32>| {
        if let Some((isec, 0)) = r.and_then(|r| objc_ref_location(ctx, r))
            && seen.insert(isec)
        {
            lists.push(isec);
        }
    };
    fn visit_class<E: Target>(
        ctx: &Context<E>,
        cls: (u32, u64),
        classes_seen: &mut hashbrown::HashSet<(u32, u64)>,
        note: &mut impl FnMut(&Context<E>, Option<ObjcRef>, &mut Vec<u32>),
        lists: &mut Vec<u32>,
    ) {
        if !classes_seen.insert(cls) {
            return;
        }
        // class_t: isa, superclass, cache, vtable, data (the ro).
        if let Some(ro) = objc_class_ro(ctx, cls) {
            // class_ro_t: baseMethods at 32.
            note(ctx, objc_pointer_at(ctx, ro.0, ro.1 + 32), lists);
        }
        if let Some(meta) = objc_pointer_at(ctx, cls.0, 0).and_then(|r| objc_ref_location(ctx, r)) {
            visit_class(ctx, meta, classes_seen, note, lists);
        }
    }
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if !h.segname().starts_with("__DATA") {
            continue;
        }
        match h.sectname() {
            "__objc_classlist" | "__objc_nlclslist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(cls) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        visit_class(ctx, cls, &mut classes_seen, &mut note, &mut lists);
                    }
                }
            }
            "__objc_catlist" | "__objc_nlcatlist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(cat) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        // category_t: name, cls, instanceMethods, classMethods.
                        note(ctx, objc_pointer_at(ctx, cat.0, cat.1 + 16), &mut lists);
                        note(ctx, objc_pointer_at(ctx, cat.0, cat.1 + 24), &mut lists);
                    }
                }
            }
            "__objc_protolist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(proto) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        // protocol_t: isa, name, protocols, then the four
                        // method lists.
                        for field in [24, 32, 40, 48] {
                            note(ctx, objc_pointer_at(ctx, proto.0, proto.1 + field), &mut lists);
                        }
                    }
                }
            }
            "__objc_clsrolist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(ro) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        note(ctx, objc_pointer_at(ctx, ro.0, ro.1 + 32), &mut lists);
                    }
                }
            }
            _ => {}
        }
    }
    if lists.is_empty() {
        return;
    }

    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__objc_methlist"),
        segname: str_to_name("__TEXT"),
        p2align: 2,
        flags: S_REGULAR,
        ..Default::default()
    });
    let mut extra_of: hashbrown::HashMap<u32, usize> = hashbrown::HashMap::new();
    let stub_of: hashbrown::HashMap<Vec<u8>, usize> = ctx
        .objc_stubs
        .symbols
        .iter()
        .enumerate()
        .map(|(i, (_, sel))| (sel.as_bytes().to_vec(), i))
        .collect();
    let mut repoint: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    let mut offset: u64 = 0;
    for list in lists {
        let sec = &ctx.isecs[list as usize];
        let data = sec.data();
        if data.len() < 8 {
            continue;
        }
        let entsize_flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as u64;
        if entsize_flags & 0x8000_0000 != 0
            || entsize_flags & 0xffff != 24
            || 8 + 24 * count != data.len() as u64
        {
            continue;
        }
        let mut methods = Vec::with_capacity(count as usize);
        let mut ok = true;
        for i in 0..count {
            let at = 8 + 24 * i;
            let name = objc_pointer_at(ctx, list, at);
            let types = objc_pointer_at(ctx, list, at + 8).unwrap_or(ObjcRef::Null);
            let imp = objc_pointer_at(ctx, list, at + 16).unwrap_or(ObjcRef::Null);
            let Some((sel, 0)) = name.and_then(|r| objc_ref_location(ctx, r)) else {
                ok = false;
                break;
            };
            let name = match selref_of.get(&sel) {
                Some(&slot) => ObjcRef::Isec(slot, 0),
                None => {
                    // A selector stub's slot serves the same selector
                    // (ld64 keeps one slot per selector); else a new
                    // one in the tail.
                    let data = ctx.isecs[sel as usize].data();
                    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                    match stub_of.get(&data[..end]) {
                        Some(&i) => ObjcRef::TailSelref(i),
                        None => {
                            let n = *extra_of.entry(sel).or_insert_with(|| {
                                ctx.objc_stubs.extra_selrefs.push(sel);
                                ctx.objc_stubs.extra_selrefs.len() - 1
                            });
                            ObjcRef::TailSelref(ctx.objc_stubs.symbols.len() + n)
                        }
                    }
                }
            };
            methods.push(ObjcMethod { name, types, imp });
        }
        if !ok {
            continue;
        }
        let size = 8 + 12 * count;
        offset = align_to(offset, 4);
        ctx.isecs.push(InputSection {
            file,
            shndx,
            p2align: 2,
            input_addr: 0,
            size: size as u32,
            contents: 0,
            rel_offset: 0,
            nrels: 0,
            output_section: u32::MAX,
            offset: offset as u32,
            flags: InputSection::flags_placed(),
            replacement: crate::input_sections::NO_REPLACEMENT,
            unwind_offset: 0,
            nunwind: 0,
        });
        offset += size;
        let synth = (ctx.isecs.len() - 1) as u32;
        ctx.isecs[list as usize].replacement = synth;
        repoint.insert(list, synth);
        ctx.objc_methlist.lists.push(ObjcMethList { isec: synth, methods });
    }
    // The lists' own symbols (__OBJC_$_INSTANCE_METHODS_Foo ...) follow
    // them into __objc_methlist.
    for id in 0..ctx.symbols.syms.len() {
        if let Some(isec) = ctx.symbols[id].input_section()
            && let Some(&synth) = repoint.get(&isec)
        {
            ctx.symbols[id].set_input_section(Some(synth));
        }
    }
}

/// Appends a synthesized data record for an Objective-C rewrite and
/// returns its subsection; see merge_objc_categories.
pub type NewBlob<E> = dyn FnMut(&mut Context<E>, &'static str, Vec<DataField>) -> u32;

/// A field of a synthesized Objective-C data record.
#[derive(Clone, Debug)]
pub enum DataField {
    Bytes(Vec<u8>),
    /// An 8-byte pointer, rebased at load (or null).
    Ptr(ObjcRef),
}

/// A synthesized Objective-C data record, placed in the tail of the
/// output section `sect` (mapped to its segment like an input section
/// of that name) as the synthetic subsection `isec`.
#[derive(Debug)]
pub struct DataBlob {
    pub sect: &'static str,
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

/// The address a synthesized record's reference resolves to.
pub fn objc_ref_addr<E: Target>(ctx: &Context<E>, r: ObjcRef) -> u64 {
    match r {
        ObjcRef::Isec(isec, off) => ctx.isec_addr(isec as usize) + off,
        ObjcRef::Sym(id, addend) => (ctx.sym_addr(id) as i64 + addend) as u64,
        ObjcRef::TailSelref(n) => ctx.objc_selref_addr(n),
        ObjcRef::Null => 0,
    }
}

fn objc_cstring_at<E: Target>(ctx: &Context<E>, r: Option<ObjcRef>) -> Option<String> {
    let (isec, off) = objc_ref_location(ctx, r?)?;
    let data = ctx.isecs[isec as usize].data();
    let bytes = data.get(off as usize..)?;
    let end = bytes.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

/// Merges the categories of a class defined in the image into the
/// class itself, as ld64 does by default (-objc_category_merging):
/// the runtime then has no categories to attach at load. The merged
/// method list holds the categories' methods, last category first,
/// then the class's own (a category's method precedes the class's,
/// as after attachment); the protocol list likewise; the property
/// lists take the categories in order, then the class's. The
/// class_ro_t records are rewritten to point at the merged lists
/// (their symbols follow), the categories leave __objc_catlist, and a
/// class that absorbed a +load category joins __objc_nlclslist. A
/// category on a class from another image, or one whose data is not
/// in the expected shape, is left alone. Runs after the method lists
/// have been rewritten in relative form, when it merges those; with
/// classic lists the merged list is a classic one.
pub fn merge_objc_categories<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !ctx.args.objc_category_merging.unwrap_or(true) {
        return;
    }
    let relative = objc_relative_method_lists(ctx);

    // Classes defined here: class_t location -> (metaclass, ro, meta ro).
    struct Class {
        meta: (u32, u64),
        ro: (u32, u64),
        meta_ro: (u32, u64),
        nonlazy: bool,
    }
    let mut classes: hashbrown::HashMap<(u32, u64), Class> = hashbrown::HashMap::new();
    let mut class_order: Vec<(u32, u64)> = Vec::new();
    let mut nlclslist_sects = false;
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let h = ctx.hdr_of(isec);
        let nonlazy = match h.sectname() {
            "__objc_classlist" => false,
            "__objc_nlclslist" => {
                nlclslist_sects = true;
                true
            }
            _ => continue,
        };
        for off in (0..isec.size as u64).step_by(8) {
            let Some(cls) =
                objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
            else {
                continue;
            };
            let ro = objc_class_ro(ctx, cls);
            let meta = objc_pointer_at(ctx, cls.0, cls.1).and_then(|r| objc_ref_location(ctx, r));
            let meta_ro = meta.and_then(|m| objc_class_ro(ctx, m));
            let (Some(ro), Some(meta), Some(meta_ro)) = (ro, meta, meta_ro) else { continue };
            if !classes.contains_key(&cls) {
                class_order.push(cls);
            }
            let entry = classes.entry(cls).or_insert(Class { meta, ro, meta_ro, nonlazy: false });
            entry.nonlazy |= nonlazy;
        }
    }
    if classes.is_empty() {
        return;
    }

    // The categories, in __objc_catlist order, by class. A category
    // sits in __objc_catlist and, if it has a +load, in
    // __objc_nlcatlist too; a list subsection may hold several.
    struct Category {
        cat: (u32, u64),
        nonlazy: bool,
        merged: bool,
        name: String,
    }
    struct ListSect {
        isec: u32,
        nonlazy: bool,
        /// Each entry: the category it names, if mergeable.
        entries: Vec<Option<usize>>,
        /// The entries as references, for rebuilding the list.
        refs: Vec<ObjcRef>,
    }
    let mut cats: Vec<Category> = Vec::new();
    let mut cat_index: hashbrown::HashMap<(u32, u64), usize> = hashbrown::HashMap::new();
    let mut cats_of: hashbrown::HashMap<(u32, u64), Vec<usize>> = hashbrown::HashMap::new();
    let mut list_sects: Vec<ListSect> = Vec::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let h = ctx.hdr_of(isec);
        let nonlazy = match h.sectname() {
            "__objc_catlist" => false,
            "__objc_nlcatlist" => true,
            _ => continue,
        };
        let mut ls = ListSect { isec: i as u32, nonlazy, entries: Vec::new(), refs: Vec::new() };
        for off in (0..isec.size as u64).step_by(8) {
            let r = objc_pointer_at(ctx, i as u32, off).unwrap_or(ObjcRef::Null);
            ls.refs.push(r);
            let mergeable = (|| {
                let cat = objc_ref_location(ctx, r)?;
                if cat.1 != 0 || ctx.isecs[cat.0 as usize].size < 48 {
                    return None;
                }
                let cls = objc_pointer_at(ctx, cat.0, 8).and_then(|r| objc_ref_location(ctx, r))?;
                if !classes.contains_key(&cls) {
                    return None;
                }
                let idx = match cat_index.get(&cat) {
                    Some(&idx) => idx,
                    None => {
                        let name = objc_cstring_at(ctx, objc_pointer_at(ctx, cat.0, 0))?;
                        cats.push(Category { cat, nonlazy: false, merged: false, name });
                        cat_index.insert(cat, cats.len() - 1);
                        cats_of.entry(cls).or_default().push(cats.len() - 1);
                        cats.len() - 1
                    }
                };
                cats[idx].nonlazy |= nonlazy;
                Some(idx)
            })();
            ls.entries.push(mergeable);
        }
        list_sects.push(ls);
    }
    if cats_of.is_empty() {
        return;
    }

    // The methods of a list (already rewritten in relative form, or
    // classic), or None if the list is not in a shape we can merge.
    let methods_of = |ctx: &Context<E>, r: Option<ObjcRef>| -> Option<Vec<ObjcMethod>> {
        let Some(r) = r else { return Some(Vec::new()) };
        let (isec, off) = objc_ref_location(ctx, r)?;
        if off != 0 {
            return None;
        }
        let resolved = ctx.resolve_isec(isec as usize) as u32;
        if let Some(list) = ctx.objc_methlist.lists.iter().find(|l| l.isec == resolved) {
            return Some(list.methods.clone());
        }
        if relative {
            return None;
        }
        let data = ctx.isecs[isec as usize].data();
        if data.len() < 8 {
            return None;
        }
        let entsize_flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as u64;
        if entsize_flags != 24 || 8 + 24 * count != data.len() as u64 {
            return None;
        }
        let mut methods = Vec::new();
        for i in 0..count {
            let at = 8 + 24 * i;
            methods.push(ObjcMethod {
                name: objc_pointer_at(ctx, isec, at)?,
                types: objc_pointer_at(ctx, isec, at + 8).unwrap_or(ObjcRef::Null),
                imp: objc_pointer_at(ctx, isec, at + 16).unwrap_or(ObjcRef::Null),
            });
        }
        Some(methods)
    };
    // A protocol list: count (8 bytes) then pointers.
    let protocols_of = |ctx: &Context<E>, r: Option<ObjcRef>| -> Option<Vec<ObjcRef>> {
        let Some(r) = r else { return Some(Vec::new()) };
        let (isec, off) = objc_ref_location(ctx, r)?;
        let data = ctx.isecs[isec as usize].data();
        let count =
            u64::from_le_bytes(data.get(off as usize..off as usize + 8)?.try_into().unwrap());
        (0..count).map(|i| objc_pointer_at(ctx, isec, off + 8 + 8 * i)).collect()
    };
    // A property list: entsize (16), count, then (name, attributes).
    let properties_of = |ctx: &Context<E>, r: Option<ObjcRef>| -> Option<Vec<(ObjcRef, ObjcRef)>> {
        let Some(r) = r else { return Some(Vec::new()) };
        let (isec, off) = objc_ref_location(ctx, r)?;
        let data = ctx.isecs[isec as usize].data();
        let entsize =
            u32::from_le_bytes(data.get(off as usize..off as usize + 4)?.try_into().unwrap());
        let count =
            u32::from_le_bytes(data.get(off as usize + 4..off as usize + 8)?.try_into().unwrap())
                as u64;
        if entsize != 16 {
            return None;
        }
        (0..count)
            .map(|i| {
                let at = off + 8 + 16 * i;
                Some((
                    objc_pointer_at(ctx, isec, at)?,
                    objc_pointer_at(ctx, isec, at + 8).unwrap_or(ObjcRef::Null),
                ))
            })
            .collect()
    };
    // A pointer field's reference must be to data in this image (or
    // null) for a synthesized record to hold it as a plain rebase.
    let local = |ctx: &Context<E>, r: ObjcRef| -> bool {
        match r {
            ObjcRef::Null | ObjcRef::Isec(..) | ObjcRef::TailSelref(_) => true,
            ObjcRef::Sym(id, _) => {
                !ctx.symbols[id].is_imported() && ctx.symbols[id].input_section().is_some()
            }
        }
    };

    let mut methlist_hdr: Option<(u32, u32)> = None;
    let mut methlist_off: u64 = ctx
        .objc_methlist
        .lists
        .last()
        .map(|l| ctx.isecs[l.isec as usize].offset as u64 + ctx.isecs[l.isec as usize].size as u64)
        .unwrap_or(0);
    let mut nonlazy_classes: Vec<(u32, u64)> = Vec::new();

    for cls in class_order {
        let Some(cat_ids) = cats_of.get(&cls) else { continue };
        let cat_ids = cat_ids.clone();
        let info = &classes[&cls];
        // Gather everything first; give up on the class if anything is
        // not in shape.
        struct Merged {
            imethods: Vec<ObjcMethod>,
            cmethods: Vec<ObjcMethod>,
            protocols: Vec<ObjcRef>,
            iprops: Vec<(ObjcRef, ObjcRef)>,
            cprops: Vec<(ObjcRef, ObjcRef)>,
            any_imethods: bool,
            any_cmethods: bool,
            any_protocols: bool,
            any_iprops: bool,
            any_cprops: bool,
        }
        let mut m = Merged {
            imethods: Vec::new(),
            cmethods: Vec::new(),
            protocols: Vec::new(),
            iprops: Vec::new(),
            cprops: Vec::new(),
            any_imethods: false,
            any_cmethods: false,
            any_protocols: false,
            any_iprops: false,
            any_cprops: false,
        };
        let mut ok = true;
        for &ci in cat_ids.iter().rev() {
            let (c, coff) = cats[ci].cat;
            let im = objc_pointer_at(ctx, c, coff + 16);
            let cm = objc_pointer_at(ctx, c, coff + 24);
            let pr = objc_pointer_at(ctx, c, coff + 32);
            match (methods_of(ctx, im), methods_of(ctx, cm), protocols_of(ctx, pr)) {
                (Some(a), Some(b), Some(p)) => {
                    m.any_imethods |= im.is_some();
                    m.any_cmethods |= cm.is_some();
                    m.any_protocols |= pr.is_some();
                    m.imethods.extend(a);
                    m.cmethods.extend(b);
                    m.protocols.extend(p);
                }
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            for &ci in cat_ids.iter() {
                let (c, coff) = cats[ci].cat;
                let ip = objc_pointer_at(ctx, c, coff + 40);
                let cp = if ctx.isecs[c as usize].size >= coff as u32 + 56 {
                    objc_pointer_at(ctx, c, coff + 48)
                } else {
                    None
                };
                match (properties_of(ctx, ip), properties_of(ctx, cp)) {
                    (Some(a), Some(b)) => {
                        m.any_iprops |= ip.is_some();
                        m.any_cprops |= cp.is_some();
                        m.iprops.extend(a);
                        m.cprops.extend(b);
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
        }
        // The class's own lists follow.
        let (ro, meta_ro) = (info.ro, info.meta_ro);
        let base_im = objc_pointer_at(ctx, ro.0, ro.1 + 32);
        let base_cm = objc_pointer_at(ctx, meta_ro.0, meta_ro.1 + 32);
        let base_pr = objc_pointer_at(ctx, ro.0, ro.1 + 40);
        let base_ip = objc_pointer_at(ctx, ro.0, ro.1 + 64);
        let base_cp = objc_pointer_at(ctx, meta_ro.0, meta_ro.1 + 64);
        if ok {
            match (
                methods_of(ctx, base_im),
                methods_of(ctx, base_cm),
                protocols_of(ctx, base_pr),
                properties_of(ctx, base_ip),
                properties_of(ctx, base_cp),
            ) {
                (Some(a), Some(b), Some(p), Some(ip), Some(cp)) => {
                    m.imethods.extend(a);
                    m.cmethods.extend(b);
                    m.protocols.extend(p);
                    m.iprops.extend(ip);
                    m.cprops.extend(cp);
                }
                _ => ok = false,
            }
        }
        if ok && !relative {
            ok = m
                .imethods
                .iter()
                .chain(&m.cmethods)
                .all(|x| local(ctx, x.name) && local(ctx, x.types) && local(ctx, x.imp));
        }
        if ok {
            ok = m.protocols.iter().all(|&r| local(ctx, r))
                && m.iprops.iter().chain(&m.cprops).all(|&(a, b)| local(ctx, a) && local(ctx, b));
        }
        // The class's and metaclass's data pointers must be rewritable
        // to point at new ro records. Nothing is changed until
        // everything checks out.
        let ro_ok = objc_pointer_reloc(ctx, cls.0, cls.1 + 32).is_some() && info.meta.1 == 0
            || objc_pointer_reloc(ctx, info.meta.0, info.meta.1 + 32).is_some();
        let ro_ok = ro_ok
            && ctx.isecs[ro.0 as usize].size as u64 >= ro.1 + 72
            && ctx.isecs[meta_ro.0 as usize].size as u64 >= meta_ro.1 + 72
            && objc_pointer_reloc(ctx, info.meta.0, info.meta.1 + 32).is_some();
        if !ok || !ro_ok {
            if std::env::var_os("MOLD_OBJC_DEBUG").is_some() {
                eprintln!(
                    "category merging: class at {:?} with {} categories skipped (lists in shape: {}, ro reachable: {})",
                    cls,
                    cat_ids.len(),
                    ok,
                    ro_ok
                );
            }
            continue;
        }

        // Emit the merged lists.
        let mut new_blob =
            |ctx: &mut Context<E>, sect: &'static str, fields: Vec<DataField>| -> u32 {
                let (file, shndx) = ctx.add_synthetic_section(MachSection {
                    sectname: str_to_name(sect),
                    segname: str_to_name("__DATA"),
                    p2align: 3,
                    flags: 0,
                    ..Default::default()
                });
                let blob = DataBlob { sect, isec: 0, fields };
                let size = blob.size();
                ctx.isecs.push(InputSection {
                    file,
                    shndx,
                    p2align: 3,
                    input_addr: 0,
                    size: size as u32,
                    contents: 0,
                    rel_offset: 0,
                    nrels: 0,
                    output_section: u32::MAX,
                    offset: 0,
                    flags: InputSection::flags_placed(),
                    replacement: crate::input_sections::NO_REPLACEMENT,
                    unwind_offset: 0,
                    nunwind: 0,
                });
                let isec = (ctx.isecs.len() - 1) as u32;
                ctx.data_blobs.push(DataBlob { isec, ..blob });
                isec
            };
        let mut new_methlist = |ctx: &mut Context<E>, methods: Vec<ObjcMethod>| -> u32 {
            if relative {
                let (file, shndx) = *methlist_hdr.get_or_insert_with(|| {
                    let (file, shndx) = ctx.add_synthetic_section(MachSection {
                        sectname: str_to_name("__objc_methlist"),
                        segname: str_to_name("__TEXT"),
                        p2align: 2,
                        flags: S_REGULAR,
                        ..Default::default()
                    });
                    (file, shndx)
                });
                let size = 8 + 12 * methods.len() as u64;
                methlist_off = align_to(methlist_off, 4);
                ctx.isecs.push(InputSection {
                    file,
                    shndx,
                    p2align: 2,
                    input_addr: 0,
                    size: size as u32,
                    contents: 0,
                    rel_offset: 0,
                    nrels: 0,
                    output_section: u32::MAX,
                    offset: methlist_off as u32,
                    flags: InputSection::flags_placed(),
                    replacement: crate::input_sections::NO_REPLACEMENT,
                    unwind_offset: 0,
                    nunwind: 0,
                });
                methlist_off += size;
                let isec = (ctx.isecs.len() - 1) as u32;
                ctx.objc_methlist.lists.push(ObjcMethList { isec, methods });
                isec
            } else {
                let mut fields = vec![
                    DataField::Bytes(24u32.to_le_bytes().to_vec()),
                    DataField::Bytes((methods.len() as u32).to_le_bytes().to_vec()),
                ];
                for m in &methods {
                    fields.push(DataField::Ptr(m.name));
                    fields.push(DataField::Ptr(m.types));
                    fields.push(DataField::Ptr(m.imp));
                }
                // ld-prime writes a merged absolute list into
                // __objc_data (the protocol and property lists stay in
                // __objc_const).
                new_blob(ctx, "__objc_data", fields)
            }
        };
        // The class's original lists and the categories' are dropped
        // (the merged list carries ld64's name); a superseded list
        // that is still referred to resolves to the merged one.
        let supersede = |ctx: &mut Context<E>, r: Option<ObjcRef>, merged: Option<u32>| {
            if let Some((isec, 0)) = r.and_then(|r| objc_ref_location(ctx, r)) {
                let resolved = ctx.resolve_isec(isec as usize);
                if Some(resolved as u32) != merged {
                    ctx.objc_methlist.lists.retain(|l| l.isec as usize != resolved);
                    ctx.isecs[resolved].set_alive(false);
                    if let Some(merged) = merged {
                        ctx.isecs[resolved].replacement = merged;
                        if resolved != isec as usize {
                            ctx.isecs[isec as usize].replacement = merged;
                        }
                    }
                }
            }
        };

        // ld64 names the merged lists after the class and its
        // categories: __OBJC_$_INSTANCE_METHODS_Foo(A|B).
        let class_name =
            objc_cstring_at(ctx, objc_pointer_at(ctx, ro.0, ro.1 + 24)).unwrap_or_default();
        let suffix = format!(
            "{}({})",
            class_name,
            cat_ids.iter().map(|&ci| cats[ci].name.as_str()).collect::<Vec<_>>().join("|")
        );
        let name_it = |ctx: &mut Context<E>, prefix: &str, isec: u32| {
            let name: &'static str = String::leak(format!("{prefix}{suffix}"));
            ctx.extra_local_syms.push((name, isec));
        };
        let imethods = if m.any_imethods {
            Some(new_methlist(ctx, std::mem::take(&mut m.imethods)))
        } else {
            None
        };
        let cmethods = if m.any_cmethods {
            Some(new_methlist(ctx, std::mem::take(&mut m.cmethods)))
        } else {
            None
        };
        let protocols = if m.any_protocols {
            let mut fields =
                vec![DataField::Bytes((m.protocols.len() as u64).to_le_bytes().to_vec())];
            fields.extend(m.protocols.iter().map(|&r| DataField::Ptr(r)));
            Some(new_blob(ctx, "__objc_const", fields))
        } else {
            None
        };
        if let Some(l) = imethods {
            name_it(ctx, "__OBJC_$_INSTANCE_METHODS_", l);
        }
        if let Some(l) = cmethods {
            name_it(ctx, "__OBJC_$_CLASS_METHODS_", l);
        }
        if let Some(l) = protocols {
            name_it(ctx, "__OBJC_CLASS_PROTOCOLS_$_", l);
        }
        let props = |ctx: &mut Context<E>, list: &[(ObjcRef, ObjcRef)]| -> u32 {
            let mut fields = vec![
                DataField::Bytes(16u32.to_le_bytes().to_vec()),
                DataField::Bytes((list.len() as u32).to_le_bytes().to_vec()),
            ];
            for &(n, a) in list {
                fields.push(DataField::Ptr(n));
                fields.push(DataField::Ptr(a));
            }
            new_blob(ctx, "__objc_const", fields)
        };
        let iprops = if m.any_iprops { Some(props(ctx, &m.iprops)) } else { None };
        let cprops = if m.any_cprops { Some(props(ctx, &m.cprops)) } else { None };

        if imethods.is_some() {
            supersede(ctx, base_im, None);
        }
        if cmethods.is_some() {
            supersede(ctx, base_cm, None);
        }
        // Superseded protocol and property lists go away (ld64's output
        // keeps only the merged ones).
        let retire = |ctx: &mut Context<E>, r: Option<ObjcRef>| {
            if let Some((isec, 0)) = r.and_then(|r| objc_ref_location(ctx, r)) {
                ctx.isecs[isec as usize].set_alive(false);
            }
        };
        if protocols.is_some() {
            retire(ctx, base_pr);
        }
        if iprops.is_some() {
            retire(ctx, base_ip);
        }
        if cprops.is_some() {
            retire(ctx, base_cp);
        }
        for &ci in cat_ids.iter() {
            let (c, coff) = cats[ci].cat;
            supersede(ctx, objc_pointer_at(ctx, c, coff + 16), None);
            supersede(ctx, objc_pointer_at(ctx, c, coff + 24), None);
            for field in [32, 40, 48] {
                if ctx.isecs[c as usize].size as u64 >= coff + field + 8 {
                    retire(ctx, objc_pointer_at(ctx, c, coff + field));
                }
            }
        }

        // New ro records with the merged lists, replacing the class's
        // (their symbols follow). class_ro_t: flags, instanceStart,
        // instanceSize, reserved, then ivarLayout, name, baseMethods,
        // baseProtocols, ivars, weakIvarLayout, baseProperties - and,
        // when the flags carry RO_HAS_SWIFT_INITIALIZER (1 << 6), a
        // Swift class's metadata initializer pointer at 72, which the
        // runtime calls while realizing the class (dropping it from
        // the rewritten record sent NetNewsWire's AppDelegate into a
        // garbage address in objc_copyClassList).
        let rewrite_ro = |ctx: &mut Context<E>,
                          ro: (u32, u64),
                          methods: Option<u32>,
                          protocols: Option<u32>,
                          props: Option<u32>,
                          new_blob: &mut NewBlob<E>| {
            let data = ctx.isecs[ro.0 as usize].data()[ro.1 as usize..ro.1 as usize + 16].to_vec();
            let flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
            let has_swift_initializer = flags & (1 << 6) != 0;
            let mut fields = vec![DataField::Bytes(data)];
            let mut ptr_fields: Vec<u64> = vec![16, 24, 32, 40, 48, 56, 64];
            if has_swift_initializer {
                ptr_fields.push(72);
            }
            let record_len = ptr_fields.last().unwrap() + 8;
            for (k, field) in ptr_fields.into_iter().enumerate() {
                let sub = match k {
                    2 => methods,
                    3 => protocols,
                    6 => props,
                    _ => None,
                };
                let r = match sub {
                    Some(isec) => ObjcRef::Isec(isec, 0),
                    None => objc_pointer_at(ctx, ro.0, ro.1 + field).unwrap_or(ObjcRef::Null),
                };
                fields.push(DataField::Ptr(r));
            }
            // The new record goes where the old one was: Swift puts a
            // class's ro data in __objc_data (ld64's output keeps
            // __DATA__TtC... there), clang's in __objc_const.
            let sect: &'static str = match ctx.hdr_of(&ctx.isecs[ro.0 as usize]).sectname() {
                "__objc_data" => "__objc_data",
                _ => "__objc_const",
            };
            let blob = new_blob(ctx, sect, fields);
            let isec = ro.0 as usize;
            if ro.1 == 0 && ctx.isecs[isec].size as u64 == record_len {
                // The record was a subsection of its own: replace it, so
                // its symbol names the new record too (ld64 keeps
                // __OBJC_CLASS_RO_$_Foo).
                ctx.isecs[isec].set_alive(false);
                ctx.isecs[isec].replacement = blob;
            }
            blob
        };
        // Point a class's data field at its new ro record (keeping the
        // flag bits a Swift class stores in the pointer's low bits).
        let retarget = |ctx: &mut Context<E>, cls: (u32, u64), blob: u32| {
            let (obj, k) = objc_pointer_reloc(ctx, cls.0, cls.1 + 32).unwrap();
            let rel = ctx.objs[obj].relocs[k];
            let flags = match rel.target() {
                RelocTarget::Sym(idx) => {
                    let id = ctx.objs[obj].symbols[idx as usize];
                    (ctx.symbols[id].value as i64 + rel.addend) & 3
                }
                RelocTarget::Section(_) => rel.addend & 3,
            };
            let rel = &mut ctx.objs[obj].relocs[k];
            rel.set_target(RelocTarget::Section(blob));
            rel.addend = flags;
        };
        let ro_blob = rewrite_ro(ctx, ro, imethods, protocols, iprops, &mut new_blob);
        let meta_blob = rewrite_ro(ctx, meta_ro, cmethods, protocols, cprops, &mut new_blob);
        retarget(ctx, cls, ro_blob);
        retarget(ctx, info.meta, meta_blob);
        let mut any_nonlazy = false;
        for &ci in cat_ids.iter() {
            ctx.isecs[cats[ci].cat.0 as usize].set_alive(false);
            cats[ci].merged = true;
            any_nonlazy |= cats[ci].nonlazy;
        }
        if any_nonlazy && !info.nonlazy {
            nonlazy_classes.push(cls);
        }
    }

    // The category lists lose the merged entries: a subsection all of
    // whose entries merged goes away, one with survivors is rebuilt.
    for ls in &list_sects {
        let merged: Vec<bool> =
            ls.entries.iter().map(|e| e.is_some_and(|ci| cats[ci].merged)).collect();
        if !merged.iter().any(|&m| m) {
            continue;
        }
        ctx.isecs[ls.isec as usize].set_alive(false);
        let survivors: Vec<DataField> = ls
            .refs
            .iter()
            .zip(&merged)
            .filter(|&(_, &m)| !m)
            .map(|(&r, _)| DataField::Ptr(r))
            .collect();
        if survivors.is_empty() {
            continue;
        }
        let sect: &'static str = if ls.nonlazy { "__objc_nlcatlist" } else { "__objc_catlist" };
        let (file, shndx) = ctx.add_synthetic_section(MachSection {
            sectname: str_to_name(sect),
            segname: str_to_name("__DATA"),
            p2align: 3,
            flags: S_ATTR_NO_DEAD_STRIP,
            ..Default::default()
        });
        ctx.isecs.push(InputSection {
            file,
            shndx,
            p2align: 3,
            input_addr: 0,
            size: (survivors.len() * 8) as u32,
            contents: 0,
            rel_offset: 0,
            nrels: 0,
            output_section: u32::MAX,
            offset: 0,
            flags: InputSection::flags_placed(),
            replacement: crate::input_sections::NO_REPLACEMENT,
            unwind_offset: 0,
            nunwind: 0,
        });
        let isec = (ctx.isecs.len() - 1) as u32;
        ctx.data_blobs.push(DataBlob { sect, isec, fields: survivors });
    }

    // Classes that absorbed a +load category become non-lazy.
    if !nonlazy_classes.is_empty() {
        let _ = nlclslist_sects;
        for cls in nonlazy_classes {
            let (file, shndx) = ctx.add_synthetic_section(MachSection {
                sectname: str_to_name("__objc_nlclslist"),
                segname: str_to_name("__DATA"),
                p2align: 3,
                flags: S_ATTR_NO_DEAD_STRIP,
                ..Default::default()
            });
            ctx.isecs.push(InputSection {
                file,
                shndx,
                p2align: 3,
                input_addr: 0,
                size: 8,
                contents: 0,
                rel_offset: 0,
                nrels: 0,
                output_section: u32::MAX,
                offset: 0,
                flags: InputSection::flags_placed(),
                replacement: crate::input_sections::NO_REPLACEMENT,
                unwind_offset: 0,
                nunwind: 0,
            });
            let isec = (ctx.isecs.len() - 1) as u32;
            ctx.data_blobs.push(DataBlob {
                sect: "__objc_nlclslist",
                isec,
                fields: vec![DataField::Ptr(ObjcRef::Isec(cls.0, cls.1))],
            });
        }
    }
}

/// Publishes selected imports without reexporting their whole dylib:
/// those -reexported_symbols_list names, and those an export list
/// matches - an export list re-exports a dylib's symbol it names (a
/// plain name is an initial undefine, so it is always in the link) or a
/// pattern of it matches one the link imports anyway.
pub fn create_symbol_reexports<E: Target>(ctx: &mut Context<E>) {
    let exported = ctx.args.exported_symbols.as_ref();
    if ctx.args.reexported_symbols.is_empty() && exported.is_none() {
        return;
    }
    for name in &ctx.args.reexported_names {
        if ctx.symbols.get(name).is_none_or(|id| !ctx.symbols[id].is_defined()) {
            error!(
                "-reexported_symbols_list: undefined symbol: {}",
                crate::util::demangle::display_name(name)
            );
        }
    }
    let targets: Vec<_> = ctx
        .symbols
        .syms
        .iter()
        .enumerate()
        .filter(|(_, sym)| {
            let name = sym.name().as_bytes();
            matches!(sym.file(), Some(FileId::Dylib(_)))
                && (ctx.args.reexported_symbols.find(name) != -1
                    || exported.is_some_and(|exported| exported.find(name) != -1))
        })
        .map(|(i, _)| i as u32)
        .collect();
    let internal = ctx.internal_obj.unwrap() as u32;
    for target in targets {
        let name = ctx.symbols[target].name();
        // Keep the import as the target of references within this
        // image, and add a separate, same-name N_INDR export. Apple
        // emits both entries too; the export has no local address.
        let alias = ctx.symbols.add_local(name);
        let sym = &mut ctx.symbols[alias];
        sym.set_file(FileId::Obj(internal));
        sym.set_is_extern(true);
        ctx.indirect_aliases.push((alias, target));
        ctx.args.forced_undefined.push(name.to_string());
    }
}

/// Defines the symbols the linker itself provides.
pub fn add_synthetic_symbols<E: Target>(ctx: &mut Context<E>) {
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    if ctx.args.output_type == MH_EXECUTE {
        let id = ctx.symbols.intern("__mh_execute_header");
        let sym = &mut ctx.symbols[id];
        if !sym.is_defined() {
            sym.set_file(FileId::Obj(internal));
            sym.value = ctx.args.image_base.unwrap_or(ctx.args.pagezero_size);
            sym.set_is_extern(true);
        }
    }

    // ___dso_handle identifies the image; C++ static destructors pass it
    // to __cxa_atexit. It resolves to the mach header but is never
    // exported.
    let id = ctx.symbols.intern("___dso_handle");
    let sym = &mut ctx.symbols[id];
    if !sym.is_defined() {
        sym.set_file(FileId::Obj(internal));
        sym.value = ctx.args.image_base.unwrap_or(ctx.args.pagezero_size);
        sym.set_is_extern(false);
    }

    // -alias gives an existing definition a second name: the new
    // symbol shares the original's subsection and offset, so it lands
    // at the same address and is exported alongside it. Apple uses
    // aliases to publish compatibility names (e.g. libSystem's dozens
    // of $VARIANT names) without touching the source.
    let aliases = std::mem::take(&mut ctx.args.aliases);
    for (existing, new) in &aliases {
        let Some(src) = ctx.symbols.get(existing) else {
            error!(
                "-alias: undefined base symbol: {}",
                crate::util::demangle::display_name(existing)
            );
            continue;
        };
        if !ctx.symbols[src].is_defined() {
            error!("-alias: undefined base symbol: {}", ctx.symbols[src]);
            continue;
        }
        let dst = ctx.symbols.intern(String::leak(new.clone()));
        if ctx.symbols[src].is_imported() {
            // An alias of a dylib symbol is an indirect symbol
            // (N_INDR) whose export trie entry re-exports the dylib's
            // symbol under the new name; nothing here has an address.
            // ld64 does this for Xcode's
            // `-alias _NSExtensionMain ___debug_main_executable_dylib_entry_point`.
            if !ctx.symbols[dst].is_defined() {
                let sym = &mut ctx.symbols[dst];
                sym.set_file(FileId::Obj(internal));
                sym.set_input_section(None);
                sym.value = 0;
                sym.set_is_extern(true);
                ctx.indirect_aliases.push((dst, src));
            }
            continue;
        }
        if !ctx.symbols[dst].is_defined() {
            let (file, isec, value) = {
                let s = &ctx.symbols[src];
                (s.file(), s.input_section(), s.value)
            };
            let sym = &mut ctx.symbols[dst];
            sym.set_file(file.expect("alias of a defined symbol"));
            sym.set_input_section(isec);
            sym.value = value;
            sym.set_is_extern(true);
        }
    }
    ctx.args.aliases = aliases;

    // ld64's layout-boundary symbols: an undefined reference to
    // section$start$__SEG$__sect (or $end$, or segment$start$__SEG /
    // segment$end$__SEG) resolves to the boundary's final address, and
    // wills the named section into existence if nothing else creates
    // it. Their values can only be known after layout, so they are
    // claimed here and patched in fix_synthetic_symbols.
    for id in 0..ctx.symbols.syms.len() {
        let sym = &ctx.symbols[id];
        if !sym.is_used() || sym.is_defined() {
            continue;
        }
        let parsed = if let Some(rest) = sym.name().strip_prefix("section$") {
            rest.split_once('$').and_then(|(which, rest)| {
                rest.split_once('$')
                    .map(|(seg, sect)| (which == "start", seg.to_string(), Some(sect.to_string())))
            })
        } else if let Some(rest) = sym.name().strip_prefix("segment$") {
            rest.split_once('$').map(|(which, seg)| (which == "start", seg.to_string(), None))
        } else {
            None
        };
        let Some((is_start, seg, sect)) = parsed else {
            continue;
        };
        let sym = &mut ctx.symbols[id];
        sym.set_file(FileId::Obj(internal));
        sym.set_is_extern(false);
        ctx.boundary_syms.push((id as u32, is_start, seg, sect));
    }
}

/// Fills in the boundary symbols' addresses once every chunk and
/// segment has one.
pub fn fix_synthetic_symbols<E: Target>(ctx: &mut Context<E>) {
    for i in 0..ctx.boundary_syms.len() {
        let (id, is_start, seg, sect) = ctx.boundary_syms[i].clone();
        let value = match &sect {
            Some(sect) => {
                let Some(hdr) = ctx
                    .chunks
                    .iter()
                    .map(|&id| ctx.chunk_header(id))
                    .find(|hdr| hdr.is_sect && hdr.segname == seg && hdr.sectname == *sect)
                else {
                    fatal!("no section for boundary symbol: {}", ctx.symbols[id]);
                };
                if is_start { hdr.addr } else { hdr.addr + hdr.size }
            }
            None => {
                let Some(segment) = ctx.segments.iter().find(|s| s.name == seg) else {
                    fatal!("no segment for boundary symbol: {}", ctx.symbols[id]);
                };
                if is_start { segment.cmd.vmaddr } else { segment.cmd.vmaddr + segment.cmd.vmsize }
            }
        };
        ctx.symbols[id].value = value;
    }
}

/// Where a section sits within its segment in a final image, as
/// ld-prime 27037 orders them; sections of one rank keep input order.
/// __text leads __TEXT, other code sections follow in input order, then
/// the synthesized code and method lists; dyld's tables lead
/// __DATA_CONST, then the read-only ObjC lists in a fixed order; the
/// ObjC runtime data leads __DATA in the order the compiler emits it,
/// and __data follows in input order among the unknown sections.
fn output_section_rank(segname: &str, sectname: &str, flags: u32) -> u32 {
    match (segname, sectname) {
        ("__TEXT", "__text") => 0,
        ("__TEXT", "__stubs") => 2,
        ("__TEXT", "__stub_helper") => 3,
        ("__TEXT", "__objc_stubs") => 4,
        ("__TEXT", "__init_offsets") => 5,
        ("__TEXT", "__objc_methlist") => 6,
        ("__TEXT", _) if flags & S_ATTR_PURE_INSTRUCTIONS != 0 => 1,
        ("__TEXT", _) => 10,
        ("__DATA_CONST", "__mod_init_func") => 1,
        ("__DATA_CONST", "__mod_term_func") => 2,
        ("__DATA_CONST", "__const") => 3,
        ("__DATA_CONST", "__cfstring") => 4,
        ("__DATA_CONST", "__objc_classlist") => 5,
        ("__DATA_CONST", "__objc_nlclslist") => 6,
        ("__DATA_CONST", "__objc_catlist") => 7,
        ("__DATA_CONST", "__objc_nlcatlist") => 8,
        ("__DATA_CONST", "__objc_protolist") => 9,
        ("__DATA_CONST", "__objc_imageinfo") => 10,
        ("__DATA_CONST", "__objc_protorefs") => 11,
        ("__DATA_CONST", "__objc_superrefs") => 12,
        // The GOT closes __DATA_CONST, after every input-derived
        // section (ld-prime: __cfstring, __objc_classlist,
        // __objc_imageinfo, then __got).
        ("__DATA_CONST", "__got") => 25,
        ("__DATA_CONST", _) => 20,
        ("__DATA", "__la_symbol_ptr") => 0,
        ("__DATA", "__got") => 1,
        ("__DATA", "__objc_const") => 2,
        ("__DATA", "__objc_selrefs") => 3,
        ("__DATA", "__objc_classrefs") => 4,
        ("__DATA", "__objc_superrefs") => 5,
        ("__DATA", "__objc_ivar") => 6,
        ("__DATA", "__objc_data") => 7,
        // The thread-local initialization image must be contiguous:
        // __thread_data last among file-backed __DATA sections, and
        // __thread_bss first among zero-fill ones (zero-fill sections
        // sort after all file-backed ones).
        ("__DATA", "__thread_vars") => 30,
        ("__DATA", "__thread_data") => 31,
        ("__DATA", "__thread_bss") => 0,
        // __bss and __common in first-seen order: the synthesized
        // __common counts from the first object with a common symbol.
        ("__DATA", "__bss") => 3,
        ("__DATA", "__common") => 3,
        _ => 10,
    }
}

/// The segment for read-only-after-fixup data: __DATA_CONST unless
/// -no_data_const.
fn data_seg<E: Target>(ctx: &Context<E>) -> &'static str {
    if ctx.args.data_const { "__DATA_CONST" } else { "__DATA" }
}

/// Sections a final link places in __DATA_CONST: data that needs no
/// writes after dyld's fixups. ld64's list, as seen in ld-prime's
/// output across the app corpus.
const DATA_CONST_SECTIONS: &[&str] = &[
    "__cfstring",
    "__const",
    "__got",
    "__mod_init_func",
    "__mod_term_func",
    "__objc_arraydata",
    "__objc_arrayobj",
    "__objc_boolobj",
    "__objc_dictobj",
    "__objc_doubleobj",
    "__objc_floatobj",
    "__objc_intobj",
    "__objc_catlist",
    "__objc_classlist",
    "__objc_imageinfo",
    "__objc_nlcatlist",
    "__objc_nlclslist",
    "__objc_protolist",
];

/// Protocol and superclass references are written by the Objective-C
/// runtime on older systems, so they stay in __DATA - with their input
/// flags - unless the deployment target is macOS 15 or later, where
/// ld64 moves them to __DATA_CONST (dyld fixes them up there).
fn objc_refs_are_const<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.platform == crate::macho::PLATFORM_MACOS
        && ctx.args.platform_minos >= crate::macho::encode_version(15, 0, 0)
}

/// The output section an input section lands in, or None for one a
/// final link consumes or drops. Like ld64: __StaticInit joins
/// __text; the fixed-size literal pools (__literal4/8/16), already
/// merged per element, join __TEXT,__const; the __LLVM segment
/// (bitcode, __swift_modhash, __cmdline, __asm) is never copied into
/// an image; __objc_clsrolist is a compiler-to-linker list of the
/// class_ro_t records of generic Swift classes (nothing references
/// it and ld-prime emits no such section); and the __DATA sections
/// that need no writes after fixups move to __DATA_CONST. A -r output
/// keeps every input section as it came.
fn output_section_for(
    relocatable: bool,
    data_const: bool,
    objc_const_refs: bool,
    segname: &str,
    sectname: &str,
) -> Option<(&'static str, &'static str)> {
    let intern_seg = |seg: &str| -> &'static str {
        match seg {
            "__TEXT" => "__TEXT",
            "__DATA_CONST" => "__DATA_CONST",
            "__DATA" => "__DATA",
            other => String::leak(other.to_string()),
        }
    };
    // The __LLVM segment (bitcode, Swift's module hash) is dropped from
    // every output, -r included.
    if segname == "__LLVM" {
        return None;
    }
    if relocatable {
        return Some((intern_seg(segname), String::leak(sectname.to_string())));
    }
    match (segname, sectname) {
        ("__DATA", "__objc_clsrolist") => None,
        ("__TEXT", "__StaticInit") => Some(("__TEXT", "__text")),
        ("__TEXT", "__literal4" | "__literal8" | "__literal16") => Some(("__TEXT", "__const")),
        ("__DATA", sect) if data_const && DATA_CONST_SECTIONS.contains(&sect) => {
            Some(("__DATA_CONST", String::leak(sect.to_string())))
        }
        ("__DATA", sect @ ("__objc_protorefs" | "__objc_superrefs"))
            if data_const && objc_const_refs =>
        {
            Some(("__DATA_CONST", String::leak(sect.to_string())))
        }
        _ => Some((intern_seg(segname), String::leak(sectname.to_string()))),
    }
}

/// The flags an output section carries. In a final image ld64 keeps
/// the section type (a coalesced input section becomes regular; a
/// literal pool folded into __TEXT,__const is regular) and the
/// instruction attributes, drops every other input attribute
/// (no_dead_strip, live_support, strip_static_syms, no_toc: they
/// direct the linker, not dyld), and marks just the ObjC list sections
/// the runtime scans as no-dead-strip. A -r output is input to another
/// link, so ld-prime copies the first input section's type and
/// attributes verbatim - but for __objc_imageinfo and
/// __objc_protolist, which lose no_dead_strip. __eh_frame carries the
/// compiler's fixed flags in both.
fn output_section_flags(segname: &str, sectname: &str, input: u32, relocatable: bool) -> u32 {
    if segname == "__TEXT" && sectname == "__eh_frame" {
        return S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT;
    }
    if relocatable {
        if segname == "__DATA" && matches!(sectname, "__objc_imageinfo" | "__objc_protolist") {
            return input & !S_ATTR_NO_DEAD_STRIP;
        }
        return input;
    }
    // The two reference lists the runtime may still write keep the
    // flags they came with (coalesced, no-dead-strip) while in __DATA
    // of a final image.
    if segname == "__DATA" && matches!(sectname, "__objc_protorefs" | "__objc_superrefs") {
        return input & (SECTION_TYPE | S_ATTR_NO_DEAD_STRIP);
    }
    let mut ty = input & SECTION_TYPE;
    if ty == S_COALESCED || (segname == "__TEXT" && sectname == "__const") {
        ty = S_REGULAR;
    }
    let mut attrs = input & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS);
    if attrs & S_ATTR_PURE_INSTRUCTIONS != 0 {
        attrs |= S_ATTR_SOME_INSTRUCTIONS;
    }
    if matches!(
        sectname,
        "__objc_classlist"
            | "__objc_catlist"
            | "__objc_nlclslist"
            | "__objc_nlcatlist"
            | "__objc_selrefs"
            | "__objc_classrefs"
    ) {
        attrs |= S_ATTR_NO_DEAD_STRIP;
    }
    ty | attrs
}

/// Applies ld64's -rename_section and -rename_segment to an input
/// section. An exact -rename_section match names the output section
/// outright (it wins over the built-in mapping, as in ld64); otherwise
/// the built-in mapping decides and -rename_segment then renames the
/// resulting segment. Neither applies to a -r output.
fn renamed_output_section(
    args: &crate::cmdline::Args,
    relocatable: bool,
    segname: &str,
    sectname: &str,
    builtin: impl FnOnce() -> Option<(&'static str, &'static str)>,
) -> Option<(&'static str, &'static str)> {
    if !relocatable
        && let Some((_, _, seg, sect)) =
            args.rename_sections.iter().find(|(seg, sect, _, _)| seg == segname && sect == sectname)
    {
        return Some((String::leak(seg.clone()), String::leak(sect.clone())));
    }
    let (seg, sect) = builtin()?;
    if !relocatable && let Some((_, new)) = args.rename_segments.iter().find(|(old, _)| old == seg)
    {
        return Some((String::leak(new.clone()), sect));
    }
    Some((seg, sect))
}

/// Whether an input subsection is an __objc_methname string that the
/// selector name synthesized for an objc_msgSend$ stub absorbs. ld-prime
/// keeps the synthesized string of the two, so such an input string
/// does not place its output section: when every input string is one,
/// __objc_methname follows every input-derived __TEXT section.
fn is_stub_selector_name<E: Target>(
    ctx: &Context<E>,
    isec: &InputSection,
    stub_sels: &hashbrown::HashSet<&[u8]>,
) -> bool {
    !stub_sels.is_empty()
        && ctx.hdr_of(isec).sectname() == "__objc_methname"
        && stub_sels.contains(cstring_of(isec.data()))
}

/// Creates output section chunks and appends each input section to its
/// chunk, and groups chunks into segments.
pub fn create_output_sections<E: Target>(ctx: &mut Context<E>) {
    ctx.chunks.push(ChunkId::MachHeader);

    // Assign each input section to an output section, creating output
    // sections as needed. Keyed by the raw 16-byte name pairs, so the
    // hot loop does no allocation and no linear scans; chunks are
    // still created in first-encounter order.
    let relocatable = ctx.args.relocatable;
    let objc_const_refs = objc_refs_are_const(ctx);
    let mut by_name: hashbrown::HashMap<([u8; 16], [u8; 16]), Option<OutputSectionId>> =
        hashbrown::HashMap::new();
    // Output sections by their (possibly renamed) names: several input
    // section names can land in one output section.
    let mut by_out: hashbrown::HashMap<(&'static str, &'static str), OutputSectionId> =
        hashbrown::HashMap::new();
    // All subsections of one input section share the exact same leaked
    // header pointer and are contiguous in the arena, and a header
    // uniquely names one (object, section) - so a section's whole run
    // of subsections maps to the same output chunk. Cache the last
    // header pointer to skip the 32-byte name hash for all but the
    // first subsection of each section; on a debug link this turns
    // millions of hash lookups into a handful of thousands.
    let mut last_hdr: *const crate::macho::MachSection = std::ptr::null();
    let mut last_osec: Option<OutputSectionId> = None;
    // A -r output section's flags come from its first non-empty input
    // section (ld-prime skips empty ones); whether that one was seen.
    let mut flags_from_data: Vec<bool> = Vec::new();
    for i in 0..ctx.isecs.len() {
        if !ctx.isecs[i].is_alive()
            || ctx.isecs[i].replacement != crate::input_sections::NO_REPLACEMENT
            || ctx.isecs[i].is_placed()
        {
            continue;
        }
        let hdr_ref = ctx.hdr_of(&ctx.isecs[i]);
        let hdr_ptr = std::ptr::from_ref::<crate::macho::MachSection>(hdr_ref);
        // A copy: the header lives in its object, which stays borrowed
        // while the section is placed below otherwise.
        let hdr = *hdr_ref;
        let osec_id = if hdr_ptr == last_hdr {
            last_osec
        } else {
            let key = (hdr.segname, hdr.sectname);
            let id = match by_name.get(&key) {
                Some(&id) => id,
                None => {
                    let id = renamed_output_section(
                        &ctx.args,
                        relocatable,
                        hdr.segname(),
                        hdr.sectname(),
                        || {
                            output_section_for(
                                relocatable,
                                ctx.args.data_const,
                                objc_const_refs,
                                hdr.segname(),
                                hdr.sectname(),
                            )
                        },
                    )
                    .map(|out| match by_out.get(&out) {
                        Some(&id) => id,
                        None => {
                            let mut osec = OutputSection::new(out.0, out.1);
                            osec.hdr.flags =
                                output_section_flags(out.0, out.1, hdr.flags, relocatable);
                            let id = OutputSectionId::new(ctx.output_sections.len() as u32);
                            ctx.output_sections.push(osec);
                            ctx.chunks.push(ChunkId::Output(id));
                            by_out.insert(out, id);
                            id
                        }
                    });
                    by_name.insert(key, id);
                    id
                }
            };
            last_hdr = hdr_ptr;
            last_osec = id;
            id
        };
        let Some(osec_id) = osec_id else {
            // Consumed by the link: no output section.
            ctx.isecs[i].set_alive(false);
            continue;
        };

        let osec = &mut ctx.output_sections[osec_id.index()];
        osec.hdr.p2align = osec.hdr.p2align.max(ctx.isecs[i].p2align as u32);
        // __thread_vars contains pointers but clang emits it with an
        // alignment of 1, so override.
        if osec.hdr.flags & SECTION_TYPE == S_THREAD_LOCAL_VARIABLES {
            osec.hdr.p2align = osec.hdr.p2align.max(3);
        }
        // A final image merges the members' attributes; a -r output
        // takes those of the first non-empty member alone, as ld-prime
        // does.
        if !relocatable {
            osec.hdr.flags |=
                output_section_flags(osec.hdr.segname, &osec.hdr.sectname, hdr.flags, false)
                    & !SECTION_TYPE;
        } else if hdr.size != 0 {
            let idx = osec_id.index();
            if flags_from_data.len() <= idx {
                flags_from_data.resize(idx + 1, false);
            }
            if !flags_from_data[idx] {
                osec.hdr.flags =
                    output_section_flags(osec.hdr.segname, &osec.hdr.sectname, hdr.flags, true);
                flags_from_data[idx] = true;
            }
        }
        osec.members.push(i as u32);
        ctx.isecs[i].set_output_section(ChunkId::Output(osec_id));
    }

    // A final image always has a __TEXT,__text section, empty if no
    // code reached it (a dylib of only data; ld-prime writes one of
    // size 0, byte-aligned).
    if !relocatable && !by_out.contains_key(&("__TEXT", "__text")) {
        let mut osec = OutputSection::new("__TEXT", "__text");
        osec.hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        let id = OutputSectionId::new(ctx.output_sections.len() as u32);
        ctx.output_sections.push(osec);
        ctx.chunks.push(ChunkId::Output(id));
        by_out.insert(("__TEXT", "__text"), id);
    }

    // The thread-local template (__thread_data followed by
    // __thread_bss) is one image dyld copies per thread, so ld64 gives
    // both sections the stricter of their alignments.
    if let (Some(&data), Some(&bss)) =
        (by_out.get(&("__DATA", "__thread_data")), by_out.get(&("__DATA", "__thread_bss")))
    {
        let p2align = ctx.output_sections[data.index()]
            .hdr
            .p2align
            .max(ctx.output_sections[bss.index()].hdr.p2align);
        ctx.output_sections[data.index()].hdr.p2align = p2align;
        ctx.output_sections[bss.index()].hdr.p2align = p2align;
    }

    // A section cannot be aligned beyond the segment's page: ld64
    // reduces the alignment with a warning (an x86-64 .align 16 asks
    // for 64KB).
    if !relocatable {
        let max = E::PAGE_SIZE.trailing_zeros();
        for osec in &mut ctx.output_sections {
            if osec.hdr.p2align > max {
                crate::warn!(
                    "reducing alignment of section {},{} from 0x{:x} to 0x{:x} because it exceeds segment maximum alignment",
                    osec.hdr.segname,
                    osec.hdr.sectname,
                    1u64 << osec.hdr.p2align,
                    1u64 << max
                );
                osec.hdr.p2align = max;
            }
        }
    }

    // -sectalign overrides an output section's alignment, e.g. to
    // page-align a blob that will be mapped or measured separately.
    // It can only raise the alignment: subsections were placed by
    // their own requirements, which must still hold.
    for (seg, sect, p2align) in &ctx.args.sectalign.clone() {
        for osec in &mut ctx.output_sections {
            if osec.hdr.segname == *seg && osec.hdr.sectname == *sect {
                osec.hdr.p2align = osec.hdr.p2align.max(*p2align as u32);
            }
        }
    }

    // -order_file moves the atoms it names to the front of their
    // output sections, in the file's order; everything else keeps its
    // input order behind them. A stable sort by rank does both.
    if let Some(ranks) = order_file_ranks(ctx) {
        for osec in &mut ctx.output_sections {
            osec.members.sort_by_key(|&id| ranks[id as usize]);
        }
    }

    // Cold code last: clang marks the rarely-run part it splits off a
    // function (foo.cold.1, and the function it came from) N_COLD_FUNC,
    // and ld64 lays those atoms out after every other atom of their
    // section - in final images and -r outputs alike - so hot code
    // stays dense.
    {
        let mut cold = vec![false; ctx.isecs.len()];
        let mut any = false;
        for obj in &ctx.objs {
            if !obj.is_alive {
                continue;
            }
            for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
                if nlist.is_stab() || nlist.n_type() != N_SECT || nlist.n_desc & N_COLD_FUNC == 0 {
                    continue;
                }
                if let Some(isec) = ctx.symbols[sym_id].input_section() {
                    cold[isec as usize] = true;
                    any = true;
                }
            }
        }
        if any {
            for osec in &mut ctx.output_sections {
                {
                    let isecs = &mut osec.members;
                    isecs.sort_by_key(|&id| cold[id as usize]);
                }
            }
        }
    }

    // Compute each input section's offset within its output section.
    // Following mold's design, sections lay out in parallel: each
    // output section's offsets depend only on its own members, so the
    // per-section prefix sums run on all cores and the results are
    // written back serially. The exception is a __TEXT section big
    // enough to need range-extension thunks, whose creation scans and
    // annotates relocations; those (at most one per link in practice)
    // stay on the serial path.
    {
        // Branches are not confined to their own section: __text,
        // __StaticInit, the stubs and every other executable section
        // share the __TEXT segment's address space, so once their
        // combined size comes near the branch reach, a branch from any
        // of them can be out of range. One gate over the total decides
        // for all of them (a small late section like __StaticInit is
        // exactly the one whose backward branches span the farthest).
        let mut exec_total: u64 = 0;
        for osec in &ctx.output_sections {
            if osec.hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0 {
                exec_total +=
                    osec.members.iter().map(|&id| ctx.isecs[id].size as u64 + 16).sum::<u64>();
            }
        }
        let need_thunks = exec_total > E::BRANCH_RANGE / 2 - 64 * 1024 * 1024;

        let mut thunked: Vec<usize> = Vec::new();
        let mut plain: Vec<(usize, Vec<crate::input_sections::InputSectionId>)> = Vec::new();
        for (i, osec) in ctx.output_sections.iter().enumerate() {
            let is_exec =
                osec.hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0;
            if is_exec && need_thunks {
                thunked.push(i);
            } else {
                plain.push((i, osec.members.clone()));
            }
        }

        let offsets: Vec<(usize, Vec<u64>, u64)> = plain
            .par_iter()
            .map(|(i, isecs)| {
                let mut offs = Vec::with_capacity(isecs.len());
                let mut off = 0;
                for &id in isecs {
                    let isec = &ctx.isecs[id];
                    off = isec.align_offset(off);
                    offs.push(off);
                    off += isec.size as u64;
                }
                (*i, offs, off)
            })
            .collect();
        for (i, offs, size) in offsets {
            for (&id, off) in ctx.output_sections[i].members.clone().iter().zip(offs) {
                ctx.isecs[id].offset = off as u32;
            }
            ctx.output_sections[i].hdr.size = size;
        }

        for i in thunked {
            let isecs = ctx.output_sections[i].members.clone();
            let thunks = crate::thunks::create_range_extension_thunks::<E>(ctx, &isecs);
            let end = match thunks.last() {
                Some(t) => t.offset + t.syms.len() as u64 * E::THUNK_SIZE,
                None => 0,
            };
            let data_end = isecs
                .last()
                .map(|&id| ctx.isecs[id].offset as u64 + ctx.isecs[id].size as u64)
                .unwrap_or(0);
            let osec = &mut ctx.output_sections[i];
            osec.hdr.size = end.max(data_end);
            osec.thunks = thunks;
        }
    }

    if !ctx.stubs.symbols.is_empty() {
        ctx.stubs.hdr.reserved2 = E::STUB_SIZE as u32;
        ctx.stubs.hdr.size = ctx.stubs.symbols.len() as u64 * E::STUB_SIZE;
        // ld-prime's x86-64 stubs are byte-aligned when all of them go
        // through the lazy-binding helper and 2-byte aligned as soon as
        // one doesn't (chained fixups, -bind_at_load, a weak-lookup
        // stub): each stub has its own alignment and the section takes
        // the largest. arm64's are instruction-aligned.
        if E::CPUTYPE == crate::macho::CPU_TYPE_X86_64 {
            let lazy = ctx.lazy_binding()
                && ctx.stubs.symbols.iter().all(|&id| !ctx.binds_weak_lookup(id));
            ctx.stubs.hdr.p2align = if lazy { 0 } else { 1 };
        }
        ctx.chunks.push(ChunkId::Stubs);
    }
    // (A stub bound by weak lookup goes through the GOT; only lazily
    // bound stubs need the helper and lazy pointers.)
    if !ctx.stubs.lazy.is_empty() {
        ctx.stub_helper.hdr.size = E::STUB_HELPER_HEADER_SIZE
            + ctx.stubs.lazy.len() as u64 * E::STUB_HELPER_ENTRY_SIZE
            - E::STUB_HELPER_ENTRY_PADDING;
        ctx.chunks.push(ChunkId::StubHelper);
        // Indirect symbol table entries: stubs, the GOT's, then these.
        ctx.lazy_ptrs.hdr.reserved1 = (ctx.stubs.symbols.len() + ctx.got.got_syms.len()) as u32;
        ctx.lazy_ptrs.hdr.size = ctx.stubs.lazy.len() as u64 * 8;
        ctx.chunks.push(ChunkId::LazyPtrs);
    }

    if !ctx.got.got_syms.is_empty() {
        ctx.got.hdr.segname = data_seg(ctx);
        // Indirect symbol table entries for stubs come first, then the
        // GOT's.
        ctx.got.hdr.reserved1 = ctx.stubs.symbols.len() as u32;
        ctx.got.hdr.size = ctx.got.got_syms.len() as u64 * 8;
        ctx.chunks.push(ChunkId::Got);
        for i in 0..ctx.got.objc_classref_slots.len() {
            let (slot, class) = ctx.got.objc_classref_slots[i];
            ctx.isecs[slot as usize].offset = ctx.sym_aux(class).got_idx * 8;
            ctx.isecs[slot as usize].set_output_section(ChunkId::Got);
        }
    }

    if !ctx.init_offsets.init_funcs.is_empty() {
        ctx.init_offsets.hdr.size = ctx.init_offsets.init_funcs.len() as u64 * 4;
        ctx.chunks.push(ChunkId::InitOffsets);
    }

    if !ctx.objc_stubs.symbols.is_empty() {
        ctx.objc_stubs.hdr.size = ctx.objc_stubs.symbols.len() as u64 * E::OBJC_STUB_SIZE;
        // 32-byte stubs on arm64; ld-prime leaves x86-64's byte-aligned.
        if E::CPUTYPE == crate::macho::CPU_TYPE_X86_64 {
            ctx.objc_stubs.hdr.p2align = 0;
        }
        ctx.chunks.push(ChunkId::ObjcStubs);
    }
    {
        // The stubs' selector strings and reference slots join the
        // sections of those names (as their tail): the Objective-C
        // runtime uniques the selectors of one __objc_selrefs section
        // per image, and a second one would leave every compiler-
        // emitted @selector() unregistered.
        // The input subsections are placed already, so the tail's
        // offset and the section's final size are known here.
        let tail_section = |ctx: &mut Context<E>,
                            seg: &'static str,
                            sect: &str,
                            flags: u32,
                            p2align: u32,
                            tail: Tail,
                            tail_size: u64| {
            let id = match ctx
                .output_sections
                .iter()
                .position(|o| o.hdr.segname == seg && o.hdr.sectname == sect)
            {
                Some(i) => OutputSectionId::new(i as u32),
                None => {
                    let mut osec = OutputSection::new(seg, sect);
                    osec.hdr.flags = flags;
                    let id = OutputSectionId::new(ctx.output_sections.len() as u32);
                    ctx.output_sections.push(osec);
                    ctx.chunks.push(ChunkId::Output(id));
                    id
                }
            };
            let osec = ctx.output_section_mut(id);
            osec.hdr.p2align = osec.hdr.p2align.max(p2align);
            osec.tail = tail;
            osec.tail_off = align_to(osec.hdr.size, 1 << p2align);
            osec.hdr.size = osec.tail_off + tail_size;
            id
        };
        let methname_size = ctx.objc_stubs.methname_data.len() as u64;
        let selrefs_size =
            (ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len()) as u64 * 8;
        if methname_size > 0 {
            let id = tail_section(
                ctx,
                "__TEXT",
                "__objc_methname",
                S_CSTRING_LITERALS,
                0,
                Tail::ObjcMethname,
                methname_size,
            );
            ctx.objc_stubs.methname = Some(id);
        }
        if selrefs_size > 0 {
            let id = tail_section(
                ctx,
                "__DATA",
                "__objc_selrefs",
                S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP,
                3,
                Tail::ObjcSelrefs,
                selrefs_size,
            );
            ctx.objc_stubs.selrefs = Some(id);
            let tail_off = ctx.output_section(id).tail_off;
            for i in 0..ctx.objc_stubs.absorbed.len() {
                let (synth, slot) = ctx.objc_stubs.absorbed[i];
                let isec = &mut ctx.isecs[synth as usize];
                isec.set_output_section(ChunkId::Output(id));
                isec.offset = (tail_off + slot as u64 * 8) as u32;
            }
        }
        // Synthesized Objective-C records go in the tail of the section
        // they name; each blob's subsection is placed there.
        if !ctx.data_blobs.is_empty() {
            let mut sects: Vec<&'static str> = ctx.data_blobs.iter().map(|b| b.sect).collect();
            sects.sort();
            sects.dedup();
            for sect in sects {
                let (seg, out) = output_section_for(
                    false,
                    ctx.args.data_const,
                    objc_refs_are_const(ctx),
                    "__DATA",
                    sect,
                )
                .unwrap();
                let flags = output_section_flags(seg, out, 0, false);
                let mut size = 0u64;
                let mut offs = Vec::new();
                for b in ctx.data_blobs.iter().filter(|b| b.sect == sect) {
                    size = align_to(size, 8);
                    offs.push((b.isec, size));
                    size += b.size();
                }
                let id = tail_section(ctx, seg, out, flags, 3, Tail::DataBlobs, size);
                let tail_off = ctx.output_section(id).tail_off;
                for (isec, off) in offs {
                    ctx.isecs[isec as usize].set_output_section(ChunkId::Output(id));
                    ctx.isecs[isec as usize].offset = (tail_off + off) as u32;
                }
            }
        }
    }

    if !ctx.objc_methlist.lists.is_empty() {
        // ld64 lays the lists out sorted by their symbol's name, each
        // 8-byte aligned; category merging also retires some after
        // their first placement.
        let mut name_of: hashbrown::HashMap<u32, &'static str> = hashbrown::HashMap::new();
        for sym in ctx.symbols.syms.iter() {
            if let Some(isec) = sym.input_section() {
                let r = ctx.resolve_isec(isec as usize) as u32;
                let e = name_of.entry(r).or_insert(sym.name());
                if sym.name() < *e {
                    *e = sym.name();
                }
            }
        }
        let mut order: Vec<usize> = (0..ctx.objc_methlist.lists.len()).collect();
        order.sort_by_key(|&i| {
            (name_of.get(&ctx.objc_methlist.lists[i].isec).copied().unwrap_or(""), i)
        });
        let mut off = 0u64;
        for i in order {
            let isec = ctx.objc_methlist.lists[i].isec as usize;
            off = align_to(off, 8);
            ctx.isecs[isec].offset = off as u32;
            off += ctx.isecs[isec].size as u64;
        }
        ctx.objc_methlist.hdr.size = off;
        ctx.chunks.push(ChunkId::ObjcMethlist);
        for i in 0..ctx.objc_methlist.lists.len() {
            let isec = ctx.objc_methlist.lists[i].isec as usize;
            ctx.isecs[isec].set_output_section(ChunkId::ObjcMethlist);
        }
    }

    // Sections synthesized from files by -sectcreate.
    let sectcreate = std::mem::take(&mut ctx.args.sectcreate);
    for (seg, sect, path) in &sectcreate {
        let data = std::fs::read(path).unwrap_or_else(|e| {
            let errno = crate::error::errno_text(&e);
            fatal!("file cannot be open()ed, {errno} path={}", path.display())
        });
        let segname: &'static str = String::leak(seg.clone());
        add_sectcreate(ctx, SectCreateSection::new(segname, sect, Vec::leak(data), true));
    }
    ctx.args.sectcreate = sectcreate;

    // -add_empty_section synthesizes a zero-length section, giving
    // tools a named anchor (its section$start/end addresses) without
    // any content.
    let empties = std::mem::take(&mut ctx.args.add_empty_section);
    for (seg, sect) in &empties {
        let segname: &'static str = String::leak(seg.clone());
        add_sectcreate(ctx, SectCreateSection::new(segname, sect, &[], true));
    }
    ctx.args.add_empty_section = empties;

    // Sections that exist only because a boundary symbol names them.
    for i in 0..ctx.boundary_syms.len() {
        let (_, _, seg, Some(sect)) = &ctx.boundary_syms[i] else {
            continue;
        };
        if !ctx.chunks.iter().any(|&id| {
            let hdr = ctx.chunk_header(id);
            hdr.is_sect && hdr.segname == *seg && hdr.sectname == *sect
        }) {
            let segname: &'static str = String::leak(seg.clone());
            add_sectcreate(ctx, SectCreateSection::new(segname, sect, &[], false));
        }
    }

    // Merge the objects' __objc_imageinfo records: the Swift version
    // must agree, the Swift language version is the newest, and the
    // category-class-properties bit holds only if every Objective-C
    // object has it.
    let infos: Vec<u32> =
        ctx.objs.iter().filter(|o| o.is_alive).filter_map(|o| o.objc_image_info).collect();
    if !infos.is_empty() {
        let mut swift_version = 0;
        for &flags in &infos {
            let v = (flags >> 8) & 0xff;
            if swift_version == 0 {
                swift_version = v;
            } else if v != 0 && v != swift_version {
                error!("incompatible __objc_imageinfo swift versions");
            }
        }
        let lang = infos.iter().map(|f| f >> 16).max().unwrap();
        let cat = infos.iter().all(|f| f & 0x40 != 0);
        let flags = (lang << 16) | (swift_version << 8) | if cat { 0x40 } else { 0 };

        ctx.objc_imageinfo.flags = flags;
        ctx.objc_imageinfo.hdr.segname = data_seg(ctx);
        ctx.objc_imageinfo.hdr.size = 8;
        ctx.chunks.push(ChunkId::ObjcImageInfo);
    }

    // A -static image unwinds by its __eh_frame alone.
    if !ctx.unwind_records.is_empty() && !ctx.args.static_link {
        ctx.chunks.push(ChunkId::UnwindInfo);
    }

    // Lay out the surviving DWARF records: live CIEs first, then FDEs.
    // Their offsets are needed before layout, because the __unwind_info
    // encoding embeds each FDE's offset.
    // FDEs of folded copies duplicate their leader's; drop them, and
    // remap the unwind records' FDE indices around the removals as
    // the dead-strip pass does (a record left pointing past the
    // shortened table crashed the encoder).
    {
        let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
        let mut kept_fdes = Vec::new();
        let fdes = std::mem::take(&mut ctx.fdes);
        for (i, fde) in fdes.into_iter().enumerate() {
            if ctx.isecs[fde.isec].replacement == crate::input_sections::NO_REPLACEMENT {
                fde_map[i] = kept_fdes.len();
                kept_fdes.push(fde);
            }
        }
        ctx.fdes = kept_fdes;
        let map = &fde_map;
        ctx.unwind_records.retain_mut(|rec| {
            if rec.fde_idx == crate::input_files::UNWIND_NONE {
                return true;
            }
            let mapped = map[rec.fde_idx as usize];
            if mapped == usize::MAX {
                // A folded copy's record; its leader has its own.
                return false;
            }
            rec.fde_idx = mapped as u32;
            true
        });
    }
    if !ctx.fdes.is_empty() {
        for fde in &ctx.fdes {
            ctx.cies[fde.cie as usize].is_alive = true;
        }
        let mut off = 0;
        for cie in &mut ctx.cies {
            if cie.is_alive {
                cie.output_offset = off;
                off += cie.data.len() as u32;
            }
        }
        for fde in &mut ctx.fdes {
            fde.output_offset = off;
            off += fde.data.len() as u32;
        }

        ctx.eh_frame.hdr.flags = output_section_flags("__TEXT", "__eh_frame", 0, false);
        ctx.eh_frame.hdr.size = off as u64;
        ctx.chunks.push(ChunkId::EhFrame);
    }

    // What dyld reads: a -static image goes without.
    if !ctx.args.static_link {
        ctx.chunks.push(ChunkId::ChainedFixups);
        ctx.chunks.push(ChunkId::RebaseInfo);
        ctx.chunks.push(ChunkId::BindInfo);
        ctx.chunks.push(ChunkId::WeakBindInfo);
        ctx.chunks.push(ChunkId::LazyBindInfo);
        ctx.chunks.push(ChunkId::ExportTrie);
    } else if ctx.args.pie {
        ctx.chunks.push(ChunkId::LocalRelocs);
    }
    ctx.chunks.push(ChunkId::FunctionStarts);
    if ctx.args.data_in_code_info {
        ctx.chunks.push(ChunkId::DataInCode);
    }
    if ctx.args.split_seg_info {
        ctx.chunks.push(ChunkId::SplitInfo);
    }
    ctx.chunks.push(ChunkId::Symtab);
    if !ctx.stubs.symbols.is_empty() || !ctx.got.got_syms.is_empty() {
        let lazy = ctx.stubs.lazy.len();
        ctx.indirect_symtab.hdr.size =
            (ctx.stubs.symbols.len() + ctx.got.got_syms.len() + lazy) as u64 * 4;
        ctx.chunks.push(ChunkId::IndirectSymtab);
    }
    ctx.chunks.push(ChunkId::Strtab);
    if ctx.adhoc_codesign() {
        ctx.chunks.push(ChunkId::CodeSignature);
    }

    // Sort the chunks into file order: the standard segment order, and
    // section ranks within a segment. Sections of one rank follow the
    // order their first input section was seen in - object, then
    // section ordinal - as ld-prime lays them out. Segment ranks honor
    // -segment_order, then the standard order; segments stay together,
    // and __LINKEDIT is always last.
    let mut section_first_seen: Vec<u64> = vec![u64::MAX; ctx.output_sections.len()];
    let stub_sels: hashbrown::HashSet<&[u8]> =
        ctx.objc_stubs.symbols.iter().map(|(_, sel)| sel.as_bytes()).collect();
    for (i, isec) in ctx.isecs.iter().enumerate() {
        if ctx.is_internal(isec.file as usize) || is_stub_selector_name(ctx, isec, &stub_sels) {
            continue;
        }
        let Some(ChunkId::Output(id)) = ctx.isecs[ctx.resolve_isec(i)].output_section() else {
            continue;
        };
        let key = ((isec.file as u64) << 32) | isec.shndx as u64;
        let slot = &mut section_first_seen[id.index()];
        *slot = (*slot).min(key);
    }
    drop(stub_sels);
    if let Some(obj) = ctx.common_first_obj {
        for (i, osec) in ctx.output_sections.iter().enumerate() {
            if osec.hdr.segname == "__DATA" && osec.hdr.sectname == "__common" {
                section_first_seen[i] = ((obj as u64) << 32) | u32::MAX as u64;
            }
        }
    }
    let mut order = ctx.chunks.clone();
    let mut first_seen: hashbrown::HashMap<&'static str, usize> = hashbrown::HashMap::new();
    for &id in &order {
        let n = first_seen.len();
        first_seen.entry(ctx.chunk_header(id).segname).or_insert(n);
    }
    let segment_order = &ctx.args.segment_order;
    order.sort_by_key(|&id| {
        let hdr = ctx.chunk_header(id);
        // A -static image's __DATA_CONST (only with -data_const) comes
        // after __DATA, as ld-prime places it.
        let standard = match hdr.segname {
            "__TEXT" => 0,
            "__DATA_CONST" if !ctx.args.static_link => 1,
            "__DATA" => 2,
            "__DATA_CONST" => 3,
            _ => 4,
        };
        // -segment_order orders the rest: __TEXT, which holds the
        // mach header, stays first and __LINKEDIT last.
        let seg_rank = match hdr.segname {
            "__TEXT" => 0,
            "__LINKEDIT" => usize::MAX,
            name => match segment_order.iter().position(|s| s == name) {
                Some(i) => 1 + i,
                None => 1 + segment_order.len() + standard,
            },
        };
        let seg_rank = (seg_rank, first_seen[hdr.segname]);
        let sect_rank = match id {
            ChunkId::MachHeader => 0,
            ChunkId::UnwindInfo => 100,
            ChunkId::EhFrame => 101,
            ChunkId::CodeSignature => u32::MAX,
            _ => 1 + output_section_rank(hdr.segname, &hdr.sectname, hdr.flags),
        };
        let seen = match id {
            ChunkId::Output(osec) => section_first_seen[osec.index()],
            _ => u64::MAX,
        };
        // Zero-fill sections go last in their segment so that they don't
        // occupy file space in the middle of it.
        (seg_rank, hdr.is_zerofill(), sect_rank, seen)
    });

    // Group them into segments, and number the sections: an nlist's
    // n_sect is the 1-based ordinal of its section in the load
    // commands.
    let mut segments = Vec::new();
    if ctx.args.pagezero_size > 0 {
        segments.push(OutputSegment::new("__PAGEZERO"));
    }
    let mut n_sect = 1u8;
    for &id in &order {
        let segname = ctx.chunk_header(id).segname;
        if segments.last().map(|s: &OutputSegment| s.name) != Some(segname) {
            segments.push(OutputSegment::new(segname));
        }
        segments.last_mut().unwrap().chunks.push(id);
        let hdr = ctx.chunk_header_mut(id);
        if hdr.is_sect {
            hdr.n_sect = n_sect;
            n_sect = n_sect.wrapping_add(1);
        }
    }
    ctx.segments = segments;
    ctx.chunks = order;
    check_segment_order(ctx);
}

/// ld-prime's warnings for a -segment_order that places __TEXT or
/// __LINKEDIT where they cannot go, or leaves segments out (they follow
/// the listed ones in the usual order).
fn check_segment_order<E: Target>(ctx: &Context<E>) {
    let order = &ctx.args.segment_order;
    if order.is_empty() {
        return;
    }
    let (text_pos, text_place) =
        if ctx.args.pagezero_size > 0 { (1, "second") } else { (0, "first") };
    if order.iter().position(|s| s == "__TEXT").is_some_and(|i| i != text_pos) {
        crate::warn!(
            "-segment_order of __TEXT is ignored, the segment must be ordered {text_place}"
        );
    }
    if order.iter().position(|s| s == "__LINKEDIT").is_some_and(|i| i != order.len() - 1) {
        crate::warn!("-segment_order of __LINKEDIT is ignored, the segment must be ordered last");
    }
    for seg in &ctx.segments {
        if !matches!(seg.name, "__PAGEZERO" | "__TEXT" | "__LINKEDIT")
            && !order.iter().any(|s| s == seg.name)
        {
            crate::warn!("-segment_order should list all segments, {} is missing", seg.name);
        }
    }
}

/// Adds a synthesized section with fixed contents to the output.
fn add_sectcreate<E: Target>(ctx: &mut Context<E>, sec: SectCreateSection) {
    let idx = ctx.sectcreate_sections.len() as u32;
    ctx.sectcreate_sections.push(sec);
    ctx.chunks.push(ChunkId::SectCreate(idx));
}

/// Returns true if a local symbol should appear in the output symbol
/// table. Assembler temporaries, which begin with 'l' or 'L', are
/// dropped.
fn keep_local_symbol(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('l') && !name.starts_with('L')
}

/// Returns true if a non-external local symbol defined in `isec`
/// appears in a final image's symbol table: its name must not be a
/// label, and it must not live in one of the Objective-C list
/// sections, whose entries ld64 never names in an output (Swift's
/// _objc_classes_* in __objc_classlist: ld-prime's NetNewsWire has
/// none of the 127 ours carried). A demoted private external in those
/// sections stays (clang's __OBJC_LABEL_PROTOCOL_$_X does).
fn keep_local_symbol_in<E: Target>(ctx: &Context<E>, name: &str, isec: Option<u32>) -> bool {
    if !keep_local_symbol(name) {
        return false;
    }
    match isec {
        Some(isec) => {
            let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
            if ctx.is_internal(isec.file as usize) {
                return true;
            }
            !matches!(
                ctx.hdr_of(isec).sectname(),
                "__objc_classlist"
                    | "__objc_nlclslist"
                    | "__objc_catlist"
                    | "__objc_nlcatlist"
                    | "__objc_protolist"
                    | "__objc_selrefs"
                    | "__objc_classrefs"
                    | "__objc_superrefs"
                    | "__objc_protorefs"
                    | "__objc_imageinfo"
            )
        }
        None => true,
    }
}

/// Plans one object's debug-note stabs, in output symbol-table form
/// (name, entry, and the symbol whose address the entry takes, if
/// any). An object with DWARF gets the run ld64 writes: N_SO, N_OSO
/// naming the object, N_FUN pairs and N_GSYM/N_STSYM for its
/// symbols, and a closing N_SO. An object that already carries such a
/// run (a -r output: ld64 does not merge DWARF, it writes these
/// notes) has it copied through, the address-bearing entries rebased
/// to their subsections' output addresses and those of dead
/// subsections dropped. Shared by the final link and -r.
/// One planned stab entry: its name and nlist, the symbol whose final
/// address fills in n_value, and the symbol the name is, if any -
/// ld-prime points the entry at that symbol's own string.
#[derive(Clone, Copy)]
pub struct Stab {
    pub name: &'static [u8],
    pub ent: NList,
    pub value_of: Option<crate::symbol::SymbolId>,
    pub name_of: Option<crate::symbol::SymbolId>,
}

impl Stab {
    fn new(name: &'static [u8], ent: NList, value_of: Option<crate::symbol::SymbolId>) -> Self {
        Self { name, ent, value_of, name_of: None }
    }
}

/// An object's planned stab entries.
pub type StabPlan = Vec<Stab>;

pub fn plan_object_stabs<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    cwd: &Path,
    commons: &hashbrown::HashMap<crate::symbol::SymbolId, usize>,
) -> StabPlan {
    let obj = &ctx.objs[obj_idx];
    let mut out: StabPlan = Vec::new();
    if !obj.is_alive {
        return out;
    }

    if obj.nlists.iter().any(|n| n.n_type == N_OSO) {
        // Entries whose n_value is an address in the object (n_sect
        // says which section); an N_FUN with an empty name holds the
        // function's size instead.
        let addressed = |n: &NList| {
            n.n_sect != 0
                && matches!(
                    n.n_type,
                    N_FUN
                        | N_BNSYM
                        | N_ENSYM
                        | N_GSYM
                        | N_STSYM
                        | N_LCSYM
                        | N_SLINE
                        | N_ECOMM
                        | N_ECOML
                )
        };
        // The object's own local symbols by name, for the notes that
        // name them.
        let r = obj.local_range();
        let locals: hashbrown::HashMap<&str, crate::symbol::SymbolId> = obj.nlists[r.clone()]
            .iter()
            .zip(&obj.symbols[r])
            .filter(|(n, _)| !n.is_stab())
            .map(|(_, &id)| (ctx.symbols[id].name(), id))
            .collect();
        let mut skip_size = false;
        for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if !nlist.is_stab() {
                continue;
            }
            let mut ent = *nlist;
            let name = ctx.symbols[sym_id].name();
            // The string table starts " \0": offset 1 is the empty
            // name (a closing N_SO, an N_FUN size entry); offset 0
            // would read as the name " ", and lldb then never sees
            // the unit's end.
            ent.n_strx = if name.is_empty() { 1 } else { 0 };
            if addressed(nlist) {
                let placed = crate::input_files::find_symbol_subsec(
                    &ctx.isecs,
                    &obj.subsecs,
                    nlist.n_sect,
                    nlist.n_value,
                )
                .map(|(isec, off)| (ctx.resolve_isec(isec), off))
                .filter(|&(isec, _)| ctx.isecs[isec].is_alive());
                let Some((isec, off)) = placed else {
                    // Dead code: drop the note, and a function's size
                    // entry with it.
                    skip_size = nlist.n_type == N_FUN;
                    continue;
                };
                ent.n_value = ctx.isec_addr(isec) + off;
                ent.n_sect = ctx.isec_n_sect(&ctx.isecs[isec]);
            } else if nlist.n_type == N_FUN && skip_size {
                skip_size = false;
                continue;
            }
            let name_of = match nlist.n_type {
                N_FUN | N_STSYM | N_GSYM | N_LCSYM if !name.is_empty() => {
                    locals.get(name).copied().or_else(|| ctx.symbols.get(name))
                }
                _ => None,
            };
            out.push(Stab { name: name.as_bytes(), ent, value_of: None, name_of });
        }
        return sort_copied_stabs(ctx, out);
    }

    if !obj.has_debug_info {
        return out;
    }

    // ld64 opens each object's run with two N_SO entries, the
    // compilation directory (with a trailing slash) and the source
    // file, both from the DWARF compile unit; its own stab reader
    // takes an N_SO with an empty name as the closing one, so a -r
    // output without them crashed it. N_OSO then points at the
    // object (or "archive(member)"), as an absolute path.
    let (dir, file) = match crate::dwarf::compile_unit_name(obj.mf.data(), &obj.sect_hdrs) {
        Some((dir, file)) => (dir, file),
        None => {
            let leaf = obj.mf.name.file_name().map_or(&[][..], |f| f.as_bytes());
            (Vec::new(), leaf.to_vec())
        }
    };
    let mut dir = if dir.is_empty() { path_bytes(cwd).to_vec() } else { dir };
    if !dir.ends_with(b"/") {
        dir.push(b'/');
    }
    for name in [dir, file] {
        out.push(Stab::new(leak_bytes(name), NList { n_type: N_SO, ..Default::default() }, None));
    }
    let mut oso_name: Vec<u8> = match obj.mf.parent {
        Some(parent) if parent.name.is_absolute() => path_bytes(&obj.mf.name).to_vec(),
        Some(_) | None if obj.mf.name.is_absolute() => path_bytes(&obj.mf.name).to_vec(),
        _ => path_bytes(&cwd.join(&obj.mf.name)).to_vec(),
    };
    // -oso_prefix strips a leading path from every N_OSO, so
    // debug builds relocated to another machine (or built in a
    // sandbox) can still find their objects relative to a
    // debugger's source map. "." means the current directory.
    if let Some(prefix) = &ctx.args.oso_prefix {
        let mut cwd_prefix = path_bytes(cwd).to_vec();
        cwd_prefix.push(b'/');
        let prefix: &[u8] = if prefix == b"." { &cwd_prefix } else { prefix };
        if let Some(rest) = oso_name.strip_prefix(prefix) {
            oso_name = rest.to_vec();
        }
    }
    // n_value is the object's modification time, which dsymutil and
    // lldb compare against the file they find (0 disables the check):
    // an archive member's is its header's. ZERO_AR_DATE, set to
    // anything, zeroes them all for reproducible builds.
    let mtime = if ctx.args.zero_ar_date {
        0
    } else if let Some(date) = obj.mf.ar_date {
        date
    } else {
        std::fs::metadata(&obj.mf.name)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    };
    out.push(Stab::new(
        leak_bytes(std::mem::take(&mut oso_name)),
        NList { n_strx: 0, n_type: N_OSO, n_sect: E::CPUSUBTYPE as u8, n_desc: 1, n_value: mtime },
        None,
    ));

    // A symbol's notes, by its address: ld-prime lists them in address
    // order, functions and data alike.
    let mut groups: Vec<(u64, StabPlan)> = Vec::new();
    for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
        let sym = &ctx.symbols[sym_id];
        // A tentative definition gets its note in the first object that
        // declares it.
        let common = nlist.is_common() && commons.get(&sym_id) == Some(&obj_idx);
        if nlist.is_stab()
            || (!common && !matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx))
            || (!nlist.is_extern() && !keep_local_symbol_in(ctx, sym.name(), sym.input_section()))
        {
            continue;
        }
        groups.extend(symbol_stabs(ctx, sym_id, nlist.is_extern(), common));
    }
    groups.sort_by_key(|g| g.0);
    out.extend(groups.into_iter().flat_map(|g| g.1));

    out.push(Stab::new(b"", STAB_END, None));
    out
}

/// A symbol's debug notes, with the address they sort by: N_BNSYM, the
/// N_FUN pair and N_ENSYM for a function, an N_GSYM for global data,
/// an N_STSYM for a local's.
fn symbol_stabs<E: Target>(
    ctx: &Context<E>,
    sym_id: crate::symbol::SymbolId,
    is_extern: bool,
    common: bool,
) -> Option<(u64, StabPlan)> {
    let sym = &ctx.symbols[sym_id];
    let name = sym.name().as_bytes();
    let Some(isec) = sym.input_section().map(|i| i as usize) else {
        // A -r output keeps a common undefined; it has no address.
        return common.then(|| (0, vec![global_stab(name, sym_id)]));
    };
    let isec = &ctx.isecs[ctx.resolve_isec(isec)];
    if !isec.is_alive() {
        return None;
    }
    let addr = ctx.sym_addr(sym_id);
    let sect = ctx.isec_n_sect(isec);
    let is_text = ctx.hdr_of(isec).segname() == "__TEXT"
        && ctx.hdr_of(isec).flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0;
    // Named entries get their string offsets later; the rest keep 1,
    // the empty string.
    let stab = |n_type, n_sect| NList { n_strx: 1, n_type, n_sect, ..Default::default() };
    let group = if is_text {
        // ld64's shape: N_BNSYM, the N_FUN pair (the function's
        // address, then its size), N_ENSYM. Its stab reader takes
        // an N_FUN without the bracketing symbols badly (a crash
        // on a -r output that had only the pair).
        let fun = NList { n_strx: 0, ..stab(N_FUN, sect) };
        vec![
            Stab::new(b"", stab(N_BNSYM, sect), Some(sym_id)),
            Stab { name_of: Some(sym_id), ..Stab::new(name, fun, Some(sym_id)) },
            Stab::new(b"", NList { n_value: isec.size as u64, ..stab(N_FUN, 0) }, None),
            Stab::new(b"", stab(N_ENSYM, sect), Some(sym_id)),
        ]
    } else if is_extern {
        vec![global_stab(name, sym_id)]
    } else {
        let ent = NList { n_strx: 0, ..stab(N_STSYM, sect) };
        vec![Stab { name_of: Some(sym_id), ..Stab::new(name, ent, Some(sym_id)) }]
    };
    Some((addr, group))
}

/// An N_GSYM: a global's debug note, which names it only, with no
/// section or address - the debugger looks the address up by name.
fn global_stab(name: &'static [u8], sym_id: crate::symbol::SymbolId) -> Stab {
    let ent = NList { n_type: N_GSYM, ..Default::default() };
    Stab { name, ent, value_of: None, name_of: Some(sym_id) }
}

/// The object whose stabs note each tentative definition that no real
/// one overrode: the first live object that declares it.
pub fn common_stab_owners<E: Target>(
    ctx: &Context<E>,
) -> hashbrown::HashMap<crate::symbol::SymbolId, usize> {
    let per_obj: Vec<Vec<crate::symbol::SymbolId>> = ctx
        .objs
        .par_iter()
        .map(|obj| {
            if !obj.is_alive || !obj.has_debug_info {
                return Vec::new();
            }
            let r = obj.global_range();
            obj.nlists[r.clone()]
                .iter()
                .zip(&obj.symbols[r])
                .filter(|&(nlist, &id)| {
                    let sym = &ctx.symbols[id];
                    !nlist.is_stab()
                        && nlist.is_common()
                        && (sym.is_common()
                            || matches!(sym.file(), Some(FileId::Obj(o)) if ctx.is_internal(o as usize)))
                })
                .map(|(_, &id)| id)
                .collect()
        })
        .collect();
    let mut owners = hashbrown::HashMap::new();
    for (obj_idx, ids) in per_obj.into_iter().enumerate() {
        for id in ids {
            owners.entry(id).or_insert(obj_idx);
        }
    }
    owners
}

/// Stabs an input carries through (a -r output's) in final-link order:
/// one run per compilation unit, the symbol notes of each sorted by
/// their addresses now - an N_GSYM by its global's - and without the
/// closing N_SO that opened the input's stabs (the output has its own).
fn sort_copied_stabs<E: Target>(ctx: &Context<E>, plan: StabPlan) -> StabPlan {
    let is_end = |e: &NList, name: &[u8]| e.n_type == N_SO && name.is_empty();
    let mut out: StabPlan = Vec::with_capacity(plan.len());
    let mut groups: Vec<(u64, StabPlan)> = Vec::new();
    let mut in_unit = false;
    let mut i = 0;
    while i < plan.len() {
        let Stab { name, ent, .. } = plan[i];
        if is_end(&ent, name) {
            groups.sort_by_key(|g| g.0);
            out.extend(groups.drain(..).flat_map(|g| g.1));
            if in_unit {
                out.push(plan[i]);
            }
            in_unit = false;
            i += 1;
            continue;
        }
        in_unit = true;
        // A function's notes run from N_BNSYM to N_ENSYM, or are an
        // N_FUN pair; any other note stands alone.
        let len = match ent.n_type {
            N_BNSYM => plan[i..].iter().position(|e| e.ent.n_type == N_ENSYM).map_or(1, |n| n + 1),
            N_FUN if !name.is_empty() => 2.min(plan.len() - i),
            _ => 1,
        };
        let addr = match ent.n_type {
            N_GSYM => ctx
                .symbols
                .get(std::str::from_utf8(name).unwrap_or(""))
                .map_or(0, |id| ctx.sym_addr(id)),
            N_SO | N_OSO | N_AST => {
                out.push(plan[i]);
                i += 1;
                continue;
            }
            _ => ent.n_value,
        };
        groups.push((addr, plan[i..i + len].to_vec()));
        i += len;
    }
    groups.sort_by_key(|g| g.0);
    out.extend(groups.into_iter().flat_map(|g| g.1));
    out
}

/// An N_SO with an empty name: it closes an object's stabs, and
/// ld-prime opens the stabs of an image with one too.
const STAB_END: NList = NList { n_strx: 1, n_type: N_SO, n_sect: 1, n_desc: 0, n_value: 0 };

/// A final image's local symbols in ld-prime's order: the non-external
/// symbols it keeps, the private externals it demotes, the linker's own
/// names and the objc_msgSend$ stubs, all by address. Names at one
/// address are aliases of one atom, which ld-prime names by its
/// highest-ranked symbol - a strong external, then a private external,
/// a local, a weak definition, each rank by descending name - and it
/// lists the other names in that order before the atom's own (a strong
/// external's goes with the externals). -x keeps only private externals.
fn plan_local_symbols<E: Target>(
    ctx: &Context<E>,
    pexts: &[usize],
    sorted_globals: &[crate::symbol::SymbolId],
) -> Vec<(&'static [u8], NList, Option<crate::symbol::SymbolId>)> {
    // (address, rank, name, entry, symbol whose address fills n_value)
    type Ent = (u64, u8, &'static [u8], NList, Option<crate::symbol::SymbolId>);
    const PEXT: u8 = 0;
    const LOCAL: u8 = 1;
    const WEAK: u8 = 2;
    let local =
        |n_sect: u8, n_value: u64| NList { n_strx: 0, n_type: N_SECT, n_sect, n_desc: 0, n_value };

    let mut ents: Vec<Ent> = Vec::new();
    if !ctx.args.strip_locals {
        let per_obj: Vec<Vec<Ent>> = ctx
            .objs
            .par_iter()
            .map(|obj| {
                let mut out = Vec::new();
                if !obj.is_alive {
                    return out;
                }
                let r = obj.local_range();
                for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                    let sym = &ctx.symbols[sym_id];
                    if nlist.is_stab()
                        || nlist.is_extern()
                        || !keep_local_symbol_in(ctx, sym.name(), sym.input_section())
                    {
                        continue;
                    }
                    // -non_global_symbols_keep_list / _strip_list
                    // filter local symbols by name; stabs unaffected.
                    if let Some(keep) = &ctx.args.local_keep_list
                        && keep.find(sym.name().as_bytes()) == -1
                    {
                        continue;
                    }
                    if ctx.args.local_strip_list.find(sym.name().as_bytes()) != -1 {
                        continue;
                    }
                    let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
                    let isec = ctx.resolve_isec(isec);
                    if !matches!(sym.file(), Some(FileId::Obj(_))) || !ctx.isecs[isec].is_alive() {
                        continue;
                    }
                    let ent = local(ctx.isec_n_sect(&ctx.isecs[isec]), 0);
                    out.push((
                        ctx.sym_addr(sym_id),
                        LOCAL,
                        sym.name().as_bytes(),
                        ent,
                        Some(sym_id),
                    ));
                }
                out
            })
            .collect();
        ents = per_obj.concat();

        // Locals the linker named itself, on synthesized data whose
        // addresses are final by now.
        for &(name, isec) in &ctx.extra_local_syms {
            let sec = &ctx.isecs[isec as usize];
            if sec.is_alive() && sec.output_section().is_some() {
                let addr = ctx.isec_addr(isec as usize);
                ents.push((addr, LOCAL, name.as_bytes(), local(ctx.isec_n_sect(sec), addr), None));
            }
        }
        // The selector stubs, each a non-external symbol with N_PEXT
        // set (nm: "was a private external"), as ld64 lists them -
        // NetNewsWire's debug dylib has 851 _objc_msgSend$... entries.
        let hdr = &ctx.objc_stubs.hdr;
        for (i, &(sym, _)) in ctx.objc_stubs.symbols.iter().enumerate() {
            let addr = hdr.addr + i as u64 * E::OBJC_STUB_SIZE;
            let ent = NList { n_type: N_PEXT | N_SECT, ..local(hdr.n_sect, addr) };
            ents.push((addr, PEXT, ctx.symbols[sym].name().as_bytes(), ent, None));
        }
    }

    // Private external symbols resolve globally but appear as locals
    // (with N_PEXT still set) in the output.
    for &i in pexts {
        let sym = &ctx.symbols[i];
        let id = Some(i as crate::symbol::SymbolId);
        let (ent, id) = match (sym.file(), sym.input_section()) {
            (_, Some(isec)) => {
                let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
                (NList { n_type: N_SECT | N_PEXT, ..local(ctx.isec_n_sect(isec), 0) }, id)
            }
            // A hidden __mh_execute_header (an export list that omits
            // it, or -no_exported_symbols) sits in the first section,
            // the mach header.
            (Some(FileId::Obj(o)), None) if ctx.is_internal(o as usize) => {
                (NList { n_type: N_SECT | N_PEXT, ..local(1, 0) }, id)
            }
            (_, None) => (NList { n_type: N_ABS | N_PEXT, ..local(0, sym.value) }, None),
        };
        // A demoted weak definition keeps N_WEAK_DEF.
        let (rank, ent) = if sym.is_weak_def() {
            (WEAK, NList { n_desc: N_WEAK_DEF, ..ent })
        } else {
            (PEXT, ent)
        };
        ents.push((ctx.sym_addr(i as u32), rank, sym.name().as_bytes(), ent, id));
    }

    ents.par_sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(b.2.cmp(a.2)));

    // Put each atom's own name after its aliases, unless a strong
    // external names the atom.
    let mut strong: Vec<u64> = sorted_globals
        .par_iter()
        .filter(|&&i| {
            let sym = &ctx.symbols[i];
            !sym.is_weak_def() && sym.input_section().is_some()
        })
        .map(|&i| ctx.sym_addr(i))
        .collect();
    strong.par_sort_unstable();
    let mut i = 0;
    while i < ents.len() {
        let addr = ents[i].0;
        let n = ents[i..].iter().take_while(|e| e.0 == addr).count();
        if n > 1 && strong.binary_search(&addr).is_err() {
            ents[i..i + n].rotate_left(1);
        }
        i += n;
    }
    ents.into_iter().map(|(_, _, name, ent, sym)| (name, ent, sym)).collect()
}

/// Builds the output symbol table contents: local symbols in input order,
/// then defined globals and undefined symbols, each sorted by name.
/// Symbol values are filled in when the table is copied out, after
/// addresses are assigned.
pub fn create_output_symtab<E: Target>(
    ctx: &Context<E>,
    sorted_globals: &[crate::symbol::SymbolId],
) -> SymtabSection {
    let mut data = SymtabSection::new();

    // Names are collected alongside the entries and the string table
    // is built afterwards in one parallel pass (below); an entry
    // whose name is the empty sentinel keeps whatever fixed n_strx
    // its loop assigned (the "" and "-" placeholders).
    let mut names: Vec<&'static [u8]> = Vec::new();

    let t = ctx.timer("symtab-classify");
    // An import is listed only while live code or data refers to it:
    // after -dead_strip, ld-prime drops the imports only stripped
    // functions used. A reference is a relocation from a live
    // subsection or a stub or GOT slot (unwind personalities, the
    // selector stubs' _objc_msgSend and dyld_stub_binder have slots).
    let live_ref: Vec<std::sync::atomic::AtomicBool> =
        (0..ctx.symbols.syms.len()).map(|_| std::sync::atomic::AtomicBool::new(false)).collect();
    {
        use std::sync::atomic::Ordering;
        ctx.isecs
            .par_iter()
            .filter(|isec| {
                isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT
            })
            .for_each(|isec| {
                for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
                    if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) {
                        live_ref[id as usize].store(true, Ordering::Relaxed);
                    }
                }
            });
        let slots = ctx
            .stubs
            .symbols
            .iter()
            .chain(&ctx.got.got_syms)
            .copied()
            .chain(ctx.objc_stubs.msgsend_sym)
            .chain(ctx.stub_helper.dyld_stub_binder);
        for id in slots {
            live_ref[id as usize].store(true, Ordering::Relaxed);
        }
        // The pointer fields of synthesized records (merged category
        // lists, the class registrations) refer to symbols too.
        for blob in &ctx.data_blobs {
            for field in &blob.fields {
                if let DataField::Ptr(ObjcRef::Sym(id, _)) = field {
                    live_ref[*id as usize].store(true, Ordering::Relaxed);
                }
            }
        }
        // -u names an import the program must keep whether or not
        // anything refers to it, and an -alias of an import re-exports
        // it by name (the N_INDR entry points at the import's).
        for name in &ctx.args.forced_undefined {
            if let Some(id) = ctx.symbols.get(name) {
                live_ref[id as usize].store(true, Ordering::Relaxed);
            }
        }
        for &(_, target) in &ctx.indirect_aliases {
            live_ref[target as usize].store(true, Ordering::Relaxed);
        }
    }

    // One parallel pass classifies the whole symbol table - private
    // externals (emitted among the locals), defined globals and
    // undefineds - instead of three full scans over millions of
    // slots.
    #[derive(Clone, Copy, PartialEq)]
    enum Class {
        No,
        Pext,
        Undef,
    }
    let classes: Vec<Class> = (0..ctx.symbols.syms.len())
        .into_par_iter()
        .map(|i| {
            let sym = &ctx.symbols[i];
            if matches!(sym.file(), Some(FileId::Dylib(_))) {
                return if live_ref[i].load(std::sync::atomic::Ordering::Relaxed) {
                    Class::Undef
                } else {
                    Class::No
                };
            }
            // A private external in a live object: a definition in a
            // live subsection, or a sectionless one - an absolute
            // symbol (N_ABS) or a hidden __mh_execute_header, which
            // ld64 keeps as locals too, but not a hidden -alias of an
            // import, which it drops.
            if sym.is_extern()
                && sym.is_private_extern()
                && matches!(sym.file(), Some(FileId::Obj(o)) if ctx.objs[o as usize].is_alive)
                && match sym.input_section() {
                    Some(isec) => ctx.isecs[ctx.resolve_isec(isec as usize)].is_alive(),
                    None => !ctx.indirect_aliases.iter().any(|&(a, _)| a == i as u32),
                }
            {
                // A private external becomes a local, and a label
                // is not emitted (ld-prime keeps clang's
                // __OBJC_LABEL_PROTOCOL_$_X, demoted, but not an
                // l_OBJC_LABEL_PROTOCOL_$_X).
                if !keep_local_symbol(sym.name()) {
                    return Class::No;
                }
                return Class::Pext;
            }
            Class::No
        })
        .collect();
    drop(t);

    // Local symbols, then the debugger's notes: N_AST paths and stabs.
    let pexts: Vec<usize> =
        classes.iter().enumerate().filter(|&(_, &c)| c == Class::Pext).map(|(i, _)| i).collect();
    let t = ctx.timer("symtab-locals");
    for (name, ent, sym) in plan_local_symbols(ctx, &pexts, sorted_globals) {
        names.push(name);
        data.entries.push((ent, sym));
    }
    let nplain = data.entries.len();
    drop(t);

    // Swift AST paths for the debugger (-add_ast_path), as N_AST stabs.
    for path in &ctx.args.add_ast_paths {
        let n_strx = 0;
        names.push(leak_bytes(path_bytes(path).to_vec()));
        data.entries.push((NList { n_strx, n_type: N_AST, ..Default::default() }, None));
    }

    let mut t = ctx.timer("symtab-stabs");
    let mut stabs_start = data.entries.len();
    let mut stab_names_of: Vec<Option<crate::symbol::SymbolId>> = Vec::new();
    // Debug stabs. Mach-O binaries don't carry DWARF; instead, for each
    // object with debug info the symbol table gets stab entries telling
    // the debugger where the object file is (N_OSO) and where its
    // functions and globals ended up, and the debugger reads the DWARF
    // from the objects.
    if !ctx.args.strip_debug {
        let cwd = std::env::current_dir().unwrap_or_default();
        let cwd = &cwd;
        let commons = common_stab_owners(ctx);

        // Each object's stab run is independent; plan them in
        // parallel and append in object order, the same shape as the
        // per-object locals planning below.
        let planned: Vec<StabPlan> = ctx
            .objs
            .par_iter()
            .enumerate()
            .map(|(obj_idx, _)| plan_object_stabs(ctx, obj_idx, cwd, &commons))
            .collect();
        // ld-prime opens the stabs with a closing N_SO of its own.
        if planned.iter().any(|plan| !plan.is_empty()) {
            names.push(b"");
            data.entries.push((STAB_END, None));
        }
        // Write the planned stabs into prefix-summed ranges in
        // parallel, instead of appending object by object - mold's
        // populate_symtab shape. Each object owns a disjoint range
        // starting after whatever entries (e.g. AST paths) precede it.
        let start = data.entries.len();
        debug_assert_eq!(names.len(), start);
        let mut bases = Vec::with_capacity(planned.len());
        let mut total = start;
        for plan in &planned {
            bases.push(total);
            total += plan.len();
        }
        names.reserve(total - start);
        data.entries.reserve(total - start);
        struct NamePtr(*mut &'static [u8]);
        unsafe impl Sync for NamePtr {}
        struct EntPtr(*mut (NList, Option<crate::symbol::SymbolId>));
        unsafe impl Sync for EntPtr {}
        let np = NamePtr(names.as_mut_ptr());
        let ep = EntPtr(data.entries.as_mut_ptr());
        let (np, ep) = (&np, &ep);
        planned.par_iter().zip(&bases).for_each(|(plan, &base)| {
            for (k, stab) in plan.iter().enumerate() {
                // SAFETY: [base, base+plan.len()) ranges are disjoint
                // across objects and lie within the reserved capacity.
                unsafe {
                    np.0.add(base + k).write(stab.name);
                    ep.0.add(base + k).write((stab.ent, stab.value_of));
                }
            }
        });
        stab_names_of = planned.par_iter().flat_map_iter(|p| p.iter().map(|s| s.name_of)).collect();
        stabs_start = start;
        // SAFETY: every slot in start..total was written above.
        unsafe {
            names.set_len(total);
            data.entries.set_len(total);
        }
    }
    t.stop();
    data.nlocal = data.entries.len() as u32;
    let t = ctx.timer("symtab-globals");

    // Defined global symbols, sorted by name; the caller sorted them
    // once for this table and the export trie both.
    for &i in sorted_globals {
        let sym = &ctx.symbols[i];
        let n_strx = 0;
        names.push(sym.name().as_bytes());
        let (n_type, n_sect, mut n_desc) = match (sym.file(), sym.input_section()) {
            (_, Some(isec)) => {
                (N_SECT | N_EXT, ctx.isec_n_sect(&ctx.isecs[ctx.resolve_isec(isec as usize)]), 0)
            }
            // A synthesized symbol with no section (__mh_execute_header)
            // sits in the first section: the mach header. Nothing slides
            // a -static image without -pie, and there it is absolute.
            (Some(FileId::Obj(o)), None) if ctx.is_internal(o as usize) => {
                if ctx.args.static_link && !ctx.args.pie {
                    (N_ABS | N_EXT, 0, REFERENCED_DYNAMICALLY)
                } else {
                    (N_SECT | N_EXT, 1, REFERENCED_DYNAMICALLY)
                }
            }
            (_, None) => (N_ABS | N_EXT, 0, 0),
        };
        if sym.is_weak_def() {
            n_desc |= N_WEAK_DEF;
        }
        let ent = NList { n_strx, n_type, n_sect, n_desc, n_value: 0 };
        data.entries.push((ent, Some(i)));
    }
    data.nextdef = data.entries.len() as u32 - data.nlocal;

    // Undefined (imported) symbols, sorted by name. The library ordinal
    // lives in the high byte of n_desc.
    let mut undefs: Vec<usize> = classes
        .par_iter()
        .enumerate()
        .filter(|&(_, &c)| c == Class::Undef)
        .map(|(i, _)| i)
        .collect();
    undefs.par_sort_unstable_by_key(|&i| crate::util::name_sort_key(ctx.symbols[i].name()));

    for &i in &undefs {
        let sym = &ctx.symbols[i];
        let Some(FileId::Dylib(dylib)) = sym.file() else { unreachable!() };
        let n_strx = 0;
        names.push(sym.name().as_bytes());
        // A flat-namespace import records the DYNAMIC_LOOKUP ordinal, a
        // -bundle_loader import the EXECUTABLE ordinal.
        let ordinal = ctx.nlist_library_ordinal(dylib) as u16;
        let mut n_desc = ordinal << 8;
        if sym.is_weak_ref() {
            n_desc |= N_WEAK_REF;
        }
        let ent = NList { n_strx, n_type: N_UNDF | N_EXT, n_sect: 0, n_desc, n_value: 0 };
        data.entries.push((ent, None));
    }
    data.nundef = undefs.len() as u32;
    // The string table, in ld-prime's layout.
    debug_assert_eq!(names.len(), data.entries.len());
    let (uniques, size) = crate::chunks::symtab::layout_strings(
        &mut data.entries,
        &names,
        nplain,
        (stabs_start, &stab_names_of),
        data.nlocal as usize,
        ctx.symbols.syms.len(),
    );
    data.strtab_uniques = uniques;
    data.strtab_size = size;

    // Record each global symbol's index for the indirect symbol table.
    data.output_sym_indices = vec![u32::MAX; ctx.symbols.syms.len()];
    for (i, (_, sym)) in data.entries.iter().enumerate() {
        if let Some(id) = sym
            && ctx.symbols[*id].is_extern()
        {
            data.output_sym_indices[*id as usize] = i as u32;
        }
    }

    for (i, &id) in undefs.iter().enumerate() {
        data.output_sym_indices[id] = data.nlocal + data.nextdef + i as u32;
    }

    // An alias of an imported symbol is an N_INDR entry whose n_value
    // is the string-table offset of the name it stands for; that
    // name is in the table already as the import's own entry. The
    // slot is detached from the symbol so copy_symtab leaves n_value
    // alone.
    for &(alias, target) in &ctx.indirect_aliases {
        let a = data.output_sym_indices[alias as usize];
        let t = data.output_sym_indices[target as usize];
        if a == u32::MAX || t == u32::MAX {
            continue;
        }
        let strx = data.entries[t as usize].0.n_strx;
        let ent = &mut data.entries[a as usize];
        ent.0.n_type = N_INDR | N_EXT;
        ent.0.n_sect = 0;
        ent.0.n_desc = 0;
        ent.0.n_value = strx as u64;
        ent.1 = None;
    }

    drop(t);

    data
}

/// -image_base (or -seg1addr) sets the address of the first segment
/// after __PAGEZERO, for an image that stays where it was linked. dyld
/// slides a PIE executable, and a dylib or bundle with chained fixups,
/// wherever it likes; ld-prime ignores the option for those with a
/// warning, and rounds a base up to a page.
pub fn resolve_image_base<E: Target>(ctx: &mut Context<E>) {
    let Some(base) = ctx.args.image_base else { return };
    if ctx.args.output_type == MH_EXECUTE && ctx.args.pie && !ctx.args.static_link {
        crate::warn!("Linking with PIE, -image_base will be ignored");
        ctx.args.image_base = None;
    } else if matches!(ctx.args.output_type, MH_DYLIB | MH_BUNDLE) && ctx.use_chained_fixups() {
        crate::warn!("prefered load addresses (-seg1addr) are disabled with chained fixups");
        ctx.args.image_base = None;
    } else if base % E::PAGE_SIZE != 0 {
        let aligned = align_to(base, E::PAGE_SIZE);
        crate::warn!("base address {base:#x} is not properly aligned. Changing it to {aligned:#x}");
        ctx.args.image_base = Some(aligned);
    }
}

/// An -image_base below __PAGEZERO's end would map the image into the
/// zero page.
fn check_image_base<E: Target>(ctx: &Context<E>) {
    let Some(base) = ctx.args.image_base else { return };
    if base < ctx.args.pagezero_size
        && let Some(seg) = ctx.segments.iter().find(|s| s.name != "__PAGEZERO")
    {
        error!(
            "custom segments overlap: __PAGEZERO(0x0-{:#x}) {}({:#x}-{:#x})",
            ctx.args.pagezero_size,
            seg.name,
            seg.cmd.vmaddr,
            seg.cmd.vmaddr + seg.cmd.vmsize
        );
    }
}

pub fn set_osec_offsets<E: Target>(ctx: &mut Context<E>) {
    let page = E::PAGE_SIZE;
    let mut addr = ctx.args.image_base.unwrap_or(0);
    let mut fileoff = 0;

    // Chunk sizes that are independent of the layout.
    let header_size = mach_header_size(ctx);

    for seg_idx in 0..ctx.segments.len() {
        // Everything the bind stream describes (the GOT, data sections)
        // is laid out by the time we reach __LINKEDIT.
        if ctx.segments[seg_idx].name == "__LINKEDIT" {
            // Every code and data address is final by now; the tables
            // below would read ones inside __PAGEZERO as negative.
            check_image_base(ctx);
            crate::error::checkpoint();
            // The LINKEDIT tables are independent of one another and
            // every address they read is final (the symbol table needs
            // none at all), so they build as one parallel task group;
            // the chunk loop below just consumes the cached bytes.
            // sold sizes its __LINKEDIT members with the same
            // parallel-for.
            enum Streams {
                Chained(chunks::chained_fixups::ChainedFixups),
                Classic(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u32>),
            }
            let use_chained = ctx.use_chained_fixups();
            let shared = &*ctx;
            // The defined globals, sorted by name, feed both the
            // symbol table and the export trie (identical filters);
            // sort once and share - on a debug link this is hundreds
            // of thousands of long mangled names.
            // The name sort feeds only the symtab and the trie, so it
            // runs inside their arm of the task group and the fixup
            // streams, function starts and data-in-code build under it.
            let sorted_globals_of = || -> Vec<crate::symbol::SymbolId> {
                let _t = shared.timer("globals_sort");
                {
                    let mut v: Vec<crate::symbol::SymbolId> = (0..shared.symbols.syms.len())
                        .into_par_iter()
                        .filter(|&i| {
                            let sym = &shared.symbols[i];
                            sym.is_extern()
                                && !sym.is_private_extern()
                                && matches!(sym.file(), Some(FileId::Obj(_)))
                                && sym.input_section().map(|i| i as usize).is_none_or(|isec| {
                                    shared.isecs[shared.resolve_isec(isec)].is_alive()
                                })
                        })
                        .map(|i| i as u32)
                        .collect();
                    v.par_sort_unstable_by_key(|&i| {
                        crate::util::name_sort_key(shared.symbols[i].name())
                    });
                    v
                }
            };
            let ((symtab, trie), (streams, (starts, dice))) = rayon::join(
                || {
                    let sorted_globals = sorted_globals_of();
                    let sorted_globals = &sorted_globals;
                    rayon::join(
                        || {
                            let _t = shared.timer("symtab");
                            create_output_symtab(shared, sorted_globals)
                        },
                        || {
                            let _t = shared.timer("trie_encode");
                            chunks::export_trie::encode_export_trie(shared, sorted_globals)
                        },
                    )
                },
                || {
                    rayon::join(
                        || {
                            if use_chained {
                                let _t = shared.timer("chained_fixups");
                                Streams::Chained(chunks::chained_fixups::build_chained_fixups(
                                    shared,
                                ))
                            } else {
                                let (rebase, bind) = rayon::join(
                                    || {
                                        let _t = shared.timer("rebase_info");
                                        chunks::rebase_info::build(shared)
                                    },
                                    || {
                                        let _t = shared.timer("bind_info");
                                        chunks::bind_info::build(shared)
                                    },
                                );
                                let (lazy, lazy_offsets) = chunks::lazy_bind_info::build(shared);
                                let weak = chunks::weak_bind_info::build(shared);
                                Streams::Classic(rebase, bind, weak, lazy, lazy_offsets)
                            }
                        },
                        || {
                            rayon::join(
                                || {
                                    let _t = shared.timer("function_starts");
                                    chunks::function_starts::build(shared)
                                },
                                || {
                                    let _t = shared.timer("data_in_code");
                                    let dice = chunks::data_in_code::build(shared);
                                    let split = chunks::split_info::build(shared);
                                    (dice, split)
                                },
                            )
                        },
                    )
                },
            );
            // Each table's size follows from its contents; the chunk
            // loop below places them.
            let (dice, split) = dice;
            ctx.symtab = symtab;
            ctx.symtab.hdr.size = (ctx.symtab.entries.len() * size_of::<NList>()) as u64;
            ctx.strtab.hdr.size = ctx.symtab.strtab_size as u64;
            ctx.data_in_code.hdr.size = (dice.len() * 8) as u64;
            ctx.data_in_code.entries = dice;
            ctx.split_info.hdr.size = split.len() as u64;
            ctx.split_info.contents = split;
            match streams {
                Streams::Chained((contents, fixups, imports, ordinals)) => {
                    let sec = &mut ctx.chained_fixups;
                    sec.hdr.size = contents.len() as u64;
                    sec.contents = contents;
                    sec.fixups = fixups;
                    sec.imports = imports;
                    sec.ordinals = ordinals;
                }
                Streams::Classic(rebase, bind, weak, lazy, lazy_offsets) => {
                    ctx.rebase_info.hdr.size = rebase.len() as u64;
                    ctx.rebase_info.contents = rebase;
                    ctx.bind_info.hdr.size = bind.len() as u64;
                    ctx.bind_info.contents = bind;
                    ctx.weak_bind_info.hdr.size = weak.len() as u64;
                    ctx.weak_bind_info.contents = weak;
                    ctx.lazy_bind_info.hdr.size = lazy.len() as u64;
                    ctx.lazy_bind_info.contents = lazy;
                    ctx.lazy_bind_info.offsets = lazy_offsets;
                }
            }
            ctx.function_starts.hdr.size = starts.len() as u64;
            ctx.function_starts.contents = starts;
            ctx.export_trie.hdr.size = trie.len() as u64;
            ctx.export_trie.contents = trie;
            if ctx.chunks.contains(&ChunkId::LocalRelocs) {
                ctx.local_relocs.locs = chunks::local_relocs::build(ctx);
                ctx.local_relocs.hdr.size =
                    (ctx.local_relocs.locs.len() * size_of::<MachRel>()) as u64;
            }
        }

        if ctx.segments[seg_idx].name == "__PAGEZERO" {
            let seg = &mut ctx.segments[seg_idx];
            seg.cmd.vmaddr = 0;
            seg.cmd.vmsize = ctx.args.pagezero_size;
            addr = ctx.args.image_base.unwrap_or(ctx.args.pagezero_size);
            continue;
        }

        // -segaddr pins a segment's address; the running address
        // resumes where it was, or past the pinned segment if that
        // lies above it.
        let pinned = ctx
            .args
            .segaddrs
            .iter()
            .find(|(name, _)| name == ctx.segments[seg_idx].name)
            .map(|&(_, a)| a);
        let resume = addr;
        let seg_vmaddr = pinned.unwrap_or(addr);
        let seg_fileoff = fileoff;
        let mut cursor = fileoff;

        let chunk_ids = ctx.segments[seg_idx].chunks.clone();

        // The output sections with range-extension thunks (executable
        // sections of __TEXT); their entries' addresses are recorded on
        // the symbols once this segment is placed.
        let thunked: Vec<OutputSectionId> = chunk_ids
            .iter()
            .filter_map(|&id| match id {
                ChunkId::Output(id) if !ctx.output_section(id).thunks.is_empty() => Some(id),
                _ => None,
            })
            .collect();
        // Regular chunks, in file order
        for &id in &chunk_ids {
            if ctx.chunk_header(id).is_zerofill() {
                continue;
            }
            let size = match id {
                ChunkId::MachHeader => header_size,
                // Encoded once its segment's addresses are known (the
                // __LINKEDIT tables are built ahead, above, but
                // __unwind_info embeds __TEXT offsets); the personality
                // cells the encoding cannot know yet (GOT addresses)
                // come back as a patch list for the copy phase.
                ChunkId::UnwindInfo => {
                    let (data, personalities) = {
                        let _t = ctx.timer("unwind_encode");
                        chunks::unwind_info::encode_unwind_info(ctx)
                    };
                    let len = data.len() as u64;
                    ctx.unwind_info.contents = data;
                    ctx.unwind_info.personalities = personalities;
                    len
                }
                ChunkId::CodeSignature => {
                    cursor = align_to(cursor, 16);
                    chunks::code_signature::size(&ctx.args.output, cursor)
                }
                _ => ctx.chunk_header(id).size,
            };
            let p2align = match id {
                ChunkId::Symtab
                | ChunkId::Strtab
                | ChunkId::RebaseInfo
                | ChunkId::BindInfo
                | ChunkId::WeakBindInfo
                | ChunkId::LazyBindInfo
                | ChunkId::ChainedFixups
                | ChunkId::ExportTrie
                | ChunkId::FunctionStarts
                | ChunkId::DataInCode
                | ChunkId::SplitInfo
                | ChunkId::LocalRelocs => 3,
                ChunkId::IndirectSymtab => 2,
                ChunkId::CodeSignature => 4,
                _ => ctx.chunk_header(id).p2align,
            };
            cursor = align_to(cursor, 1 << p2align);
            let hdr = ctx.chunk_header_mut(id);
            hdr.fileoff = cursor;
            hdr.addr = seg_vmaddr + (cursor - seg_fileoff);
            hdr.size = size;
            cursor += size;
        }

        if !thunked.is_empty() {
            crate::thunks::gather_thunk_addresses(ctx, &thunked);
        }

        let filesize = cursor - seg_fileoff;
        let mut vm_end = seg_vmaddr + filesize;

        // Zero-fill chunks occupy address space after the file-backed
        // part of the segment.
        for &id in &chunk_ids {
            let hdr = ctx.chunk_header_mut(id);
            if !hdr.is_zerofill() {
                continue;
            }
            vm_end = align_to(vm_end, 1 << hdr.p2align);
            hdr.addr = vm_end;
            hdr.fileoff = 0;
            vm_end += hdr.size;
        }

        // __LINKEDIT's file contents end exactly at the code signature;
        // other segments are padded to a page boundary in the file.
        let seg = &mut ctx.segments[seg_idx];
        seg.cmd.vmaddr = seg_vmaddr;
        seg.cmd.fileoff = seg_fileoff;
        if seg.name == "__LINKEDIT" {
            seg.cmd.filesize = filesize;
        } else {
            seg.cmd.filesize = align_to(filesize, page);
        }
        seg.cmd.vmsize = align_to(vm_end - seg_vmaddr, page).max(seg.cmd.filesize);

        addr = seg_vmaddr + seg.cmd.vmsize;
        if pinned.is_some() {
            addr = addr.max(resume);
        }
        fileoff = seg_fileoff + seg.cmd.filesize;
    }

    ctx.output_size = fileoff;

    // Thread pointers are relative to the start of the first
    // thread-local data section.
    ctx.tls_begin = ctx
        .chunks
        .iter()
        .map(|&id| ctx.chunk_header(id))
        .filter(|hdr| {
            matches!(hdr.flags & SECTION_TYPE, S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL)
        })
        .map(|hdr| hdr.addr)
        .min()
        .unwrap_or(0);
}

/// Builds the LC_FUNCTION_STARTS payload: the addresses of all
/// functions in __TEXT,__text, ULEB128 delta-encoded starting from the
/// image base. Debuggers and crash reporters use it to attribute
/// addresses to functions even for stripped binaries.
/// Reads the -order_file lists and ranks every subsection: the
/// subsection defining the file's first symbol gets rank 0 and so on;
/// unlisted subsections rank last. ld64's format is one
/// [arch:][object:]symbol per line with #-comments; the qualifiers
/// narrow a match, which this implementation approximates by
/// matching the bare symbol name.
fn order_file_ranks<E: Target>(ctx: &Context<E>) -> Option<Vec<u64>> {
    if ctx.args.order_files.is_empty() {
        return None;
    }

    // A line is [arch:][object-file:]symbol. An arch qualifier gates
    // the whole line; an object qualifier narrows the match to
    // symbols from that file (compared by leaf name, as ld64 does).
    const ARCHS: [&str; 6] = ["arm64", "arm64e", "x86_64", "i386", "armv7", "ppc"];
    let mut rank_of: std::collections::HashMap<String, Vec<(Option<String>, u64)>> =
        std::collections::HashMap::new();
    let mut next = 0u64;
    for path in &ctx.args.order_files {
        // ld64 links on without the order a missing file would give.
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                let errno = crate::error::errno_text(&e);
                crate::warn!("order file '{}' could not be opened, {errno}", path.display());
                continue;
            }
        };
        for line in text.lines() {
            let mut line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some((first, rest)) = line.split_once(':')
                && ARCHS.contains(&first.trim())
            {
                if first.trim() != E::NAME {
                    continue;
                }
                line = rest.trim();
            }
            let (file, name) = match line.split_once(':') {
                Some((file, name)) => (Some(file.trim().to_string()), name.trim()),
                None => (None, line),
            };
            rank_of.entry(name.to_string()).or_default().push((file, next));
            next += 1;
        }
    }

    let mut ranks = vec![u64::MAX; ctx.isecs.len()];
    for sym in &ctx.symbols.syms {
        let Some(FileId::Obj(obj)) = sym.file() else {
            continue;
        };
        let obj = obj as usize;
        let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
        let Some(entries) = rank_of.get(sym.name()) else {
            continue;
        };
        let leaf = ctx.objs[obj].mf.name.file_name().map_or(&[][..], |f| f.as_bytes());
        for (file, r) in entries {
            let applies = match file {
                Some(f) => {
                    leaf == f.as_bytes()
                        || path_bytes(&ctx.objs[obj].mf.name).ends_with(f.as_bytes())
                }
                None => true,
            };
            if applies {
                let isec = ctx.resolve_isec(isec);
                ranks[isec] = ranks[isec].min(*r);
            }
        }
    }
    Some(ranks)
}

/// Resolves the entry point symbol.
pub fn resolve_entry<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.output_type != MH_EXECUTE {
        return;
    }
    match ctx.symbols.get(&ctx.args.entry) {
        // An entry point in a dylib (an app extension's
        // _NSExtensionMain): LC_MAIN must point into __TEXT, so it
        // names the symbol's stub, as ld64 does.
        Some(id) if ctx.symbols[id].is_imported() => ctx.entry_addr = ctx.sym_stub_addr(id),
        Some(id) if ctx.symbols[id].is_defined() => ctx.entry_addr = ctx.sym_addr(id),
        _ => {
            error!(
                "undefined symbol for entry point: {}",
                crate::util::demangle::display_name(&ctx.args.entry)
            )
        }
    }
}

/// Gives an entry point that resolved to a dylib export the stub that
/// LC_MAIN will name; runs after scan_relocations, with the stubs of
/// the branch targets.
pub fn add_entry_stub<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.output_type != MH_EXECUTE {
        return;
    }
    if let Some(id) = ctx.symbols.get(&ctx.args.entry)
        && ctx.symbols[id].is_imported()
    {
        add_stub(ctx, id);
        if ctx.lazy_binding() {
            ensure_stub_binder(ctx);
        } else {
            add_got(ctx, id);
        }
    }
}

/// With lazy binding, the stub helper enters dyld through
/// dyld_stub_binder (libSystem's): the symbol is bound from whichever
/// loaded dylib exports it, given a GOT slot, and __dyld_private (the
/// word dyld_stub_binder is handed, ld64 puts it in __DATA,__data) is
/// synthesized. Once, on the first stub.
fn ensure_stub_binder<E: Target>(ctx: &mut Context<E>) {
    if ctx.stub_helper.dyld_stub_binder.is_some() {
        return;
    }
    let name = "dyld_stub_binder";
    let Some(dylib) = ctx.dylibs.iter().position(|d| d.exports.contains(name)) else {
        fatal!("lazy binding needs dyld_stub_binder, which no loaded dylib exports");
    };
    let id = ctx.symbols.intern(name);
    let sym = &mut ctx.symbols[id];
    if !sym.is_defined() {
        sym.set_file(FileId::Dylib(dylib as u32));
        sym.set_is_imported(true);
        sym.set_is_extern(true);
        sym.set_input_section(None);
    }
    sym.set_is_used(true);
    add_got(ctx, id);
    ctx.stub_helper.dyld_stub_binder = Some(id);

    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__data"),
        segname: str_to_name("__DATA"),
        p2align: 3,
        flags: 0,
        ..Default::default()
    });
    ctx.isecs.push(InputSection {
        file,
        shndx,
        p2align: 3,
        input_addr: 0,
        size: 8,
        contents: 0,
        rel_offset: 0,
        nrels: 0,
        output_section: u32::MAX,
        offset: 0,
        flags: InputSection::flags_placed(),
        replacement: crate::input_sections::NO_REPLACEMENT,
        unwind_offset: 0,
        nunwind: 0,
    });
    let isec = (ctx.isecs.len() - 1) as u32;
    ctx.data_blobs.push(DataBlob {
        sect: "__data",
        isec,
        fields: vec![DataField::Bytes(vec![0; 8])],
    });
    ctx.stub_helper.dyld_private_isec = isec;
    ctx.extra_local_syms.push(("__dyld_private", isec));
}

/// Copies all chunks to the output buffer and applies relocations. The
/// code signature is computed last, over everything else.
/// Copies one chunk's contents into its slice of the output buffer.
/// The slice covers exactly [fileoff, fileoff + size).
/// Copies all chunks to the output buffer and applies relocations, in
/// parallel: the buffer is carved into disjoint per-chunk slices, and
/// every chunk writes only within its own. The mach header, symbol
/// table (which also fills the string table), UUID and code signature
/// run serially afterwards, in that order, since each depends on the
/// bytes before it. Each range of the buffer is queued to `out` the
/// moment it is final, so the file is written while the rest is
/// produced: everything between the header and the symbol table after
/// the copy and its fix-ups, the symbol and string tables after
/// copy_symtab, the header after the UUID, the signature last.
pub fn copy_chunks<E: Target>(
    ctx: &Context<E>,
    buf: &mut [u8],
    out: &crate::output_file::OutputFile,
) {
    let jobs: Vec<(ChunkId, Range<u64>)> = ctx
        .chunks
        .iter()
        .map(|&id| (id, ctx.chunk_header(id)))
        .filter(|(id, hdr)| {
            !matches!(
                id,
                ChunkId::MachHeader | ChunkId::Symtab | ChunkId::Strtab | ChunkId::CodeSignature
            ) && !hdr.is_zerofill()
                // An empty section (every subsection of a coverage
                // section dead, say) shares its file offset with its
                // neighbor; it has nothing to copy, and its range would
                // start inside the neighbor's.
                && hdr.size != 0
        })
        .map(|(id, hdr)| (id, hdr.fileoff..hdr.fileoff + hdr.size))
        .collect();
    let ranges: Vec<Range<u64>> = jobs.iter().map(|(_, range)| range.clone()).collect();
    let slices = crate::output_file::split_ranges(buf, &ranges);

    let t = ctx.timer("copy_chunks");
    jobs.par_iter().zip(slices).for_each(|(&(id, _), slice)| chunks::copy_buf(ctx, id, slice));
    drop(t);

    if ctx.use_chained_fixups() {
        let _t = ctx.timer("write_fixup_chains");
        chunks::chained_fixups::write_fixup_chains(ctx, buf);
    }
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        chunks::local_relocs::write(ctx, buf);
    }
    let t = ctx.timer("apply_optimization_hints");
    E::apply_optimization_hints(ctx, buf);
    drop(t);

    let hdr_end = ctx.mach_header.hdr.size as usize;
    let sig_start = if ctx.chunks.contains(&ChunkId::CodeSignature) {
        ctx.code_signature.hdr.fileoff as usize
    } else {
        buf.len()
    };
    let symtab_start = (ctx.symtab.hdr.fileoff as usize).min(ctx.strtab.hdr.fileoff as usize);

    // Nothing below writes between the header and the symbol table.
    out.queue(hdr_end, symtab_start - hdr_end);
    let t = ctx.timer("copy_symtab");
    chunks::symtab::copy_symtab(ctx, buf);
    drop(t);
    out.queue(symtab_start, sig_start - symtab_start);
    chunks::copy_mach_header(ctx, buf);

    // The code signature is SHA256 hashes of every 4KiB page before it,
    // and the UUID that identifies this build is derived from that same
    // hash array rather than from a second pass over the contents: the
    // pages are hashed while the LC_UUID field is still zero, the array is
    // hashed once more and stamped as a version-4 UUID, which is written
    // into the header's LC_UUID, and only the pages the header spans are
    // hashed again for the signature. The circularity - the signature
    // covers the header, the header holds the UUID - is broken by the
    // zeroed field, the way ld64 hashes with the UUID zeroed. Like ld64's,
    // the UUID depends on the contents before the signature only, not on
    // the signature blob (whose identifier is the output's basename);
    // unsigned output hashes its pages the same way.
    let mut hashes: Vec<[u8; 32]> = Vec::new();
    if ctx.args.uuid || ctx.adhoc_codesign() {
        let _t = ctx.timer("page_hashes");
        hashes = chunks::code_signature::page_hashes(&buf[..sig_start]);
    }
    if ctx.args.uuid {
        let _t = ctx.timer("uuid");
        {
            let flat: Vec<u8> = hashes.concat();
            let mut hash = [0; 32];
            crate::util::sha256(&flat, &mut hash);
            let mut uuid: [u8; 16] = hash[..16].try_into().unwrap();
            uuid[6] = (uuid[6] & 0x0f) | 0x40; // version 4
            uuid[8] = (uuid[8] & 0x3f) | 0x80; // RFC 4122 variant
            *ctx.uuid.lock().unwrap() = uuid;
            chunks::write_uuid(ctx, buf);
            chunks::code_signature::rehash_pages(&buf[..sig_start], &mut hashes, 0..hdr_end);
        }
    }
    out.queue(0, hdr_end);

    if ctx.adhoc_codesign() {
        let _t = ctx.timer("write_code_signature");
        chunks::code_signature::write(ctx, buf, &hashes);
    }
    out.queue(sig_start, buf.len() - sig_start);
}
