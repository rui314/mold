//! The linker passes, in the order the driver runs them.

use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::chunks::sectcreate::SectCreateSection;
use crate::chunks::symtab::SymtabSection;
use crate::chunks::{
    self, ChunkHeader, ChunkId, OutputSection, OutputSectionId, OutputSegment, Tail,
    mach_header_size,
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
use crate::objc::{DataBlob, DataField, ObjcRef, cstring_of, objc_relative_method_lists};
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
    // An image no dyld loads (a -static one or a kext) can use no
    // dylib, so it looks for archives only. A relocatable output looks
    // for dylibs too, only to ignore them (collect_file).
    let passes: &[&[&str]] = if ctx.args.without_dyld() {
        &[&["a"]]
    } else if ctx.args.search_dylibs_first {
        &[&["tbd", "dylib"], &["a"]]
    } else {
        &[&["tbd", "dylib", "a"]]
    };
    search_library(ctx, name, passes)
}

/// Looks for a dylib only, as -upward-l does.
fn find_dylib<E: Target>(ctx: &Context<E>, name: &OsStr) -> Option<PathBuf> {
    search_library(ctx, name, &[&["tbd", "dylib"]])
}

/// Looks for lib<name>.<ext> in the library search path, for each pass
/// of extensions in turn.
fn search_library<E: Target>(
    ctx: &Context<E>,
    name: &OsStr,
    passes: &[&[&str]],
) -> Option<PathBuf> {
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
/// binds to them. One -upward_* makes it an upward dependency.
fn name_dylib<E: Target>(ctx: &mut Context<E>, idx: usize, mf: &MappedFile, rc: ReaderContext) {
    if ctx.dylibs[idx].is_implicit {
        ctx.dylibs[idx].named_at = Some((ctx.next_priority(), mf.name.clone()));
    }
    let dylib = &mut ctx.dylibs[idx];
    if dylib.is_implicit && !rc.autolinked {
        dylib.is_weak = rc.weak;
    } else {
        dylib.is_weak |= rc.weak;
    }
    dylib.is_reexported |= rc.reexport;
    dylib.is_needed |= rc.needed;
    dylib.is_upward |= rc.upward;
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
    /// -upward_library, -upward-l, -upward_framework.
    upward: bool,
    /// Named by an object's auto-link option: a hint.
    autolinked: bool,
}

/// Reports a dylib that does not let this link name it directly (see
/// input_files::is_allowed_client): an error on the command line, while
/// the library an auto-link option names is left out with a warning.
fn refuses_client<E: Target>(ctx: &Context<E>, mf: &'static MappedFile, rc: ReaderContext) -> bool {
    let id = input_files::dylib_identity(ctx, mf);
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

/// Whether an object or dylib is for an architecture the link doesn't
/// take (see input_files::takes_arch), which ld-prime ignores with a
/// warning - an archive member too, whether the link needs it or not.
fn is_foreign<E: Target>(mf: &MappedFile) -> bool {
    let Some(arch) = input_files::foreign_arch::<E>(mf) else { return false };
    crate::warn!(
        "ignoring file '{}': found architecture '{arch}', required architecture '{}'",
        mf.name.display(),
        E::NAME
    );
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
        if let Some(idx) = ctx.dylibs.iter().position(|d| d.path == mf.name) {
            name_dylib(ctx, idx, mf, rc);
        }
        return;
    }
    if !matches!(get_file_type(mf), FileType::Archive | FileType::Fat) {
        input_files::trace_file(ctx, path_bytes(&mf.name));
    }
    if matches!(get_file_type(mf), FileType::Object | FileType::Dylib) && is_foreign::<E>(mf) {
        return;
    }
    match get_file_type(mf) {
        FileType::Object => {
            let priority = ctx.next_priority();
            out.push(PendingObject { mf, alive: true, hidden: rc.hidden, priority });
        }
        // A relocatable output keeps every reference undefined for the
        // final link, and an image no dyld loads (a -static one or a
        // kext) has nothing to load a dylib with: ld-prime reads a dylib
        // on their command lines (and ignores a stub without the
        // architecture as ever), then ignores it with a warning.
        FileType::Tapi | FileType::Dylib if ctx.args.relocatable || ctx.args.without_dyld() => {
            if get_file_type(mf) == FileType::Dylib || input_files::load_tbd(ctx, mf).is_some() {
                crate::warn!("ignoring unexpected dylib '{}'", resolved_file_name(mf));
            }
        }
        FileType::Tapi | FileType::Dylib if refuses_client(ctx, mf, rc) => {}
        FileType::Tapi | FileType::Dylib => {
            let first = ctx.dylibs.len();
            let idx = if get_file_type(mf) == FileType::Tapi {
                input_files::parse_dylib(ctx, mf)
            } else {
                Some(input_files::parse_dylib_binary(ctx, mf))
            };
            let Some(idx) = idx else { return };
            // The dylibs loaded during the parse beyond this one are the
            // public libraries it re-exports; a weak parent's are weak.
            for d in &mut ctx.dylibs[first..] {
                d.is_weak |= rc.weak;
            }
            name_dylib(ctx, idx, mf, rc);
            let dylib = &mut ctx.dylibs[idx];
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
                input_files::trace_file(ctx, path_bytes(&member.name));
                let alive = rc.force_load
                    || all_load
                    || (ctx.args.load_objc && input_files::has_objc_sections(member));
                match get_file_type(member) {
                    FileType::LlvmBitcode => {
                        input_files::parse_bitcode(ctx, member, alive);
                    }
                    FileType::Object if is_foreign::<E>(member) => {}
                    _ => {
                        let priority = ctx.next_priority();
                        out.push(PendingObject { mf: member, alive, hidden: rc.hidden, priority });
                    }
                }
            }
        }
        FileType::Fat => match input_files::fat_slice::<E>(mf) {
            Some(slice) => collect_file(ctx, slice, rc, out),
            None => crate::warn!(
                "ignoring file '{}': fat file missing arch '{}', file has '{}'",
                mf.name.display(),
                E::NAME,
                input_files::fat_arch_names(mf).join(",")
            ),
        },
        FileType::LlvmBitcode => {
            input_files::parse_bitcode(ctx, mf, true);
        }
        FileType::Empty => {}
        _ => fatal!("{}: unknown file type", mf.name.display()),
    }
}

/// ld-prime warns of some sections of every object it parses - archive
/// members the link doesn't use included: it drops each __LD section it
/// doesn't know, aligns the constants of a __DATA,__cfstring to a
/// pointer whatever the section says, reads an __objc_imageinfo record
/// only if it has its 8 bytes and no more than their worth, and ignores
/// a label at the end of a section of fixed-size records. It fails the
/// link on an initializer or terminator pointer with no relocation.
/// Staging runs in parallel, so the diagnostics come here, in input
/// order.
fn warn_about_sections(staged: &[input_files::StagedObject]) {
    for obj in staged {
        for (i, hdr) in obj.sect_hdrs.iter().enumerate() {
            if input_files::is_unknown_ld_section(hdr) {
                crate::warn!(
                    "unknown section: __LD/{} in {}",
                    hdr.sectname(),
                    resolved_file_name(obj.mf)
                );
            } else if hdr.segname() == "__DATA"
                && hdr.sectname() == "__cfstring"
                && hdr.p2align != 3
                && obj.isecs.iter().any(|isec| isec.shndx == i as u32 && isec.is_alive())
            {
                crate::warn!(
                    "section __DATA/__cfstring is not pointer aligned in {}",
                    resolved_file_name(obj.mf)
                );
            } else if hdr.sectname() == "__objc_imageinfo" && hdr.size > 8 {
                crate::warn!(
                    "section {}/{} has unexpectedly large size {} in {}",
                    hdr.segname(),
                    hdr.sectname(),
                    hdr.size,
                    resolved_file_name(obj.mf)
                );
            } else if hdr.sectname() == "__objc_imageinfo" && hdr.size != 0 && hdr.size < 8 {
                crate::warn!(
                    "can't parse {}/{} section in {}",
                    hdr.segname(),
                    hdr.sectname(),
                    resolved_file_name(obj.mf)
                );
            }
        }
        for &i in &obj.extraneous_labels {
            let nlist = &obj.nlists[i as usize];
            crate::warn!(
                "ignoring extranenous label '{}' at end of section '{}'",
                obj.sym_names[i as usize],
                obj.sect_hdrs[nlist.n_sect as usize - 1].sectname()
            );
        }
        if obj.has_init_pointer_without_target() {
            error!("initializer pointer has no target in '{}'", resolved_file_name(obj.mf));
        }
    }
}

/// Stages the queued object files in parallel and integrates them in
/// input order - the parallel front end of the mold design.
fn load_pending<E: Target>(ctx: &mut Context<E>, pending: Vec<PendingObject>) {
    let relocatable = ctx.args.relocatable;
    let keep_all_fdes = !ctx.args.unwind_info();
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
    warn_about_sections(&staged);
    for obj in &staged {
        obj.check_unwind_sections();
    }

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
    // knowingly repeat them pass -no_warn_duplicate_libraries. Under -w
    // ld-prime leaves the warning out, -fatal_warnings or not.
    if ctx.args.warn_duplicate_libraries && !ctx.args.suppress_warnings {
        let mut seen = std::collections::HashSet::new();
        let mut dups = std::collections::BTreeSet::new();
        for arg in &ctx.args.inputs {
            let (option, name) = match arg {
                InputArg::Lib(name, false) => ("-l", name),
                InputArg::Lib(name, true) => ("-weak-l", name),
                InputArg::NeededLib(name) => ("-needed-l", name),
                InputArg::ReexportLib(name) => ("-reexport-l", name),
                InputArg::HiddenLib(name) => ("-hidden-l", name),
                InputArg::UpwardLib(name) => ("-upward-l", name),
                InputArg::LazyLib(name) => ("-lazy-l", name),
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
                | InputArg::NeededFile(path)
                | InputArg::UpwardFile(path)
                | InputArg::LazyFile(path) => consider(path, &mut stubs),
                InputArg::Lib(name, _)
                | InputArg::ReexportLib(name)
                | InputArg::NeededLib(name)
                | InputArg::LazyLib(name) => {
                    if let Some(path) = find_library(ctx, name) {
                        consider(&path, &mut stubs);
                    }
                }
                InputArg::UpwardLib(name) => {
                    if let Some(path) = find_dylib(ctx, name) {
                        consider(&path, &mut stubs);
                    }
                }
                InputArg::Framework(name, _)
                | InputArg::NeededFramework(name)
                | InputArg::ReexportFramework(name)
                | InputArg::UpwardFramework(name) => {
                    if let Some(path) = find_framework(ctx, name) {
                        consider(&path, &mut stubs);
                    }
                }
                _ => {}
            }
        }
        let wave1 = tapi::prefetch(&stubs, E::NAME, ctx.args.platform);
        let mut deps: Vec<&'static MappedFile> = Vec::new();
        for tbd in wave1.iter().flatten() {
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
        tapi::prefetch(&deps, E::NAME, ctx.args.platform);
    }

    let lib = |ctx: &Context<E>, name: &OsStr| {
        let path = find_library(ctx, name);
        if path.is_none() {
            error!("library '{}' not found", name.display());
        }
        path
    };
    let dylib = |ctx: &Context<E>, name: &OsStr| {
        let path = find_dylib(ctx, name);
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
            InputArg::UpwardFile(path) => {
                (Some(path.clone()), ReaderContext { upward: true, ..rc })
            }
            InputArg::LazyFile(path) => (Some(path.clone()), rc),
            InputArg::Lib(name, weak) => (lib(ctx, name), ReaderContext { weak: *weak, ..rc }),
            InputArg::ReexportLib(name) => (lib(ctx, name), ReaderContext { reexport: true, ..rc }),
            InputArg::HiddenLib(name) => (lib(ctx, name), ReaderContext { hidden: true, ..rc }),
            InputArg::NeededLib(name) => (lib(ctx, name), ReaderContext { needed: true, ..rc }),
            InputArg::UpwardLib(name) => (dylib(ctx, name), ReaderContext { upward: true, ..rc }),
            InputArg::LazyLib(name) => (lib(ctx, name), rc),
            InputArg::Framework(name, weak) => {
                (framework(ctx, name), ReaderContext { weak: *weak, ..rc })
            }
            InputArg::ReexportFramework(name) => {
                (framework(ctx, name), ReaderContext { reexport: true, ..rc })
            }
            InputArg::NeededFramework(name) => {
                (framework(ctx, name), ReaderContext { needed: true, ..rc })
            }
            InputArg::UpwardFramework(name) => {
                (framework(ctx, name), ReaderContext { upward: true, ..rc })
            }
            InputArg::BundleLoader(path) => {
                load_bundle_loader(ctx, path);
                continue;
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
    load_pending(ctx, queue);
}

/// -bundle_loader: the executable that will load this bundle. Its
/// exports resolve the bundle's remaining undefined symbols, bound at
/// run time to the main executable (XCTest bundles hosted by an app are
/// linked this way). It loads where the command line names it, which
/// places it among the dylibs as ld-prime does.
fn load_bundle_loader<E: Target>(ctx: &mut Context<E>, path: &Path) {
    if ctx.args.output_type != MH_BUNDLE {
        fatal!("-bundle_loader can only be used with -bundle");
    }
    let Ok(mf) = MappedFile::try_open(path) else {
        fatal!("library '{}' not found", path.display());
    };
    input_files::trace_file(ctx, path_bytes(&mf.name));
    input_files::parse_bundle_loader(ctx, mf);
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

/// Reads an object's auto-link options (LC_LINKER_OPTION) as ld-prime
/// does: their strings in a row, as a command line of library options.
/// Those naming a library, a framework or an archive to load are kept,
/// one to a command. The rest are dropped: unknown words and options
/// missing their argument with a warning, then with another the options
/// a command line may give for a library but an object may not (weak,
/// re-exported or upward); search paths and loading modes (-L,
/// -all_load, ...) silently. What is kept reads the same again.
fn read_linker_options(opts: &[Vec<Vec<u8>>], mf: &MappedFile) -> Vec<Vec<Vec<u8>>> {
    use crate::util::display;
    let words: Vec<&[u8]> = opts.iter().flatten().map(Vec::as_slice).collect();
    let warn = |kind: &str, what: &str| {
        let file = resolved_file_name(mf);
        crate::warn!("{kind} linker option from object file ignored: '{what}' in {file}");
    };
    let malformed = |opt: &str| {
        let (usage, file) = (crate::cmdline::missing_argument(opt), resolved_file_name(mf));
        crate::warn!("malformed linker option from object file ignored: '{usage}', in {file}");
    };
    let mut libs: Vec<Vec<Vec<u8>>> = Vec::new();
    let mut unexpected: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        let opt = display(word);
        i += 1;
        match word {
            b"-l"
            | b"-framework"
            | b"-needed_framework"
            | b"-lazy_framework"
            | b"-force_load"
            | b"-needed_library"
            | b"-lazy_library"
            | b"-weak_framework"
            | b"-reexport_framework"
            | b"-upward_framework"
            | b"-weak_library"
            | b"-reexport_library"
            | b"-upward_library"
            | b"-syslibroot"
            | b"-bundle_loader"
            | b"-L"
            | b"-F" => {
                // An empty argument is a missing one.
                let arg = words.get(i).filter(|arg| !arg.is_empty());
                i += 1;
                let Some(&arg) = arg else {
                    malformed(&opt);
                    continue;
                };
                match word {
                    b"-l" => libs.push(vec![[word, arg].concat()]),
                    b"-weak_framework" | b"-reexport_framework" | b"-upward_framework" => {
                        unexpected.push(format!("{opt} {}", display(arg)))
                    }
                    b"-weak_library" | b"-reexport_library" | b"-upward_library" => {
                        let kind = opt.strip_suffix("_library").unwrap();
                        unexpected.push(format!("{kind}-l{}", display(arg)));
                    }
                    b"-syslibroot" | b"-bundle_loader" | b"-L" | b"-F" => {}
                    _ => libs.push(vec![word.to_vec(), arg.to_vec()]),
                }
            }
            b"-all_load" | b"-ObjC" | b"-search_paths_first" | b"-search_dylibs_first" => {}
            // The library options that take the name joined to them.
            _ => match ["-needed-l", "-lazy-l", "-hidden-l", "-weak-l", "-reexport-l", "-upward-l"]
                .into_iter()
                .chain(["-l", "-L", "-F"])
                .find(|prefix| word.starts_with(prefix.as_bytes()))
            {
                Some(prefix) if word.len() == prefix.len() => malformed(prefix),
                Some("-weak-l" | "-reexport-l" | "-upward-l") => unexpected.push(opt.into_owned()),
                Some("-L" | "-F") => {}
                Some(_) => libs.push(vec![word.to_vec()]),
                None => warn("unknown", &opt),
            },
        }
    }
    for what in unexpected {
        warn("unexpected", &what);
    }
    libs
}

/// The file an auto-link option read by read_linker_options names, and
/// how to load it. A library or framework not found is remembered for
/// report_undef_errors.
fn autolinked_input<E: Target>(
    ctx: &mut Context<E>,
    opt: &[Vec<u8>],
) -> (Option<PathBuf>, ReaderContext) {
    let rc = ReaderContext { autolinked: true, ..Default::default() };
    let os_str = crate::util::os_str;
    match opt {
        [lib] => {
            let (name, rc) = match lib.strip_prefix(b"-hidden-l") {
                Some(name) => (name, ReaderContext { hidden: true, ..rc }),
                None => {
                    let name = ["-needed-l", "-lazy-l", "-l"]
                        .iter()
                        .find_map(|prefix| lib.strip_prefix(prefix.as_bytes()));
                    (name.unwrap(), rc)
                }
            };
            let path = find_library(ctx, os_str(name));
            if path.is_none() {
                ctx.autolink_misses.push(format!(
                    "Could not find or use auto-linked library '{0}': library '{0}' not found",
                    crate::util::display(name)
                ));
            }
            (path, rc)
        }
        [flag, name] if flag.ends_with(b"framework") => {
            let path = find_framework(ctx, os_str(name));
            if path.is_none() {
                // (The first name leaves out a ",suffix".)
                let base = name.split(|&c| c == b',').next().unwrap();
                ctx.autolink_misses.push(format!(
                    "Could not find or use auto-linked framework '{}': framework '{}' not found",
                    crate::util::display(base),
                    crate::util::display(name)
                ));
            }
            (path, rc)
        }
        [flag, file] if flag == b"-force_load" => {
            (Some(PathBuf::from(os_str(file))), ReaderContext { force_load: true, ..rc })
        }
        [_, file] => (Some(PathBuf::from(os_str(file))), rc),
        _ => unreachable!(),
    }
}

pub fn load_autolink_deps<E: Target>(ctx: &mut Context<E>) -> Autolinked {
    // Objects new to the link have their auto-link options read now,
    // which reports the ones ld-prime ignores.
    for obj in &mut ctx.objs {
        if obj.is_alive && !obj.linker_options_read {
            obj.linker_options = read_linker_options(&obj.linker_options, obj.mf);
            obj.linker_options_read = true;
        }
    }
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

    // An auto-link option is a hint, and ld-prime says nothing when it
    // finds no library or framework for one unless symbols are left
    // undefined (see report_undef_errors). Header-only SDK frameworks
    // make that routine: every Swift object importing CoreAudioTypes
    // carries `-framework CoreAudioTypes`, whose framework directory
    // holds headers and a module map but no binary (CotEditor's build
    // printed a warning 317 times).
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
        let (path, rc) = autolinked_input(ctx, &opt);
        if let Some(path) = path
            && let Some(mf) = MappedFile::open(&path)
        {
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

/// Makes `sym`, found in `dylibs[idx]`'s exports, an import from the
/// dylib that provides it, and returns that dylib's index.
fn import_from_dylib(
    sym: &mut crate::symbol::Symbol,
    dylibs: &[input_files::DylibFile],
    providers: &[Vec<usize>],
    idx: usize,
) -> usize {
    let owner = providing_dylib(dylibs, providers, idx, sym.name());
    sym.set_file(FileId::Dylib(owner as u32));
    sym.set_is_imported(true);
    sym.set_is_extern(true);
    sym.set_input_section(None);
    sym.set_is_common(false);
    owner
}

/// Lets newly auto-linked dylibs claim still-unresolved symbols. They
/// carry later priorities than every file already resolved, so they
/// can steal nothing - a full re-resolution would reach exactly this
/// outcome, at many times the cost.
pub fn claim_new_dylibs<E: Target>(ctx: &mut Context<E>, first: usize) {
    let dylibs = &ctx.dylibs;
    let providers = merged_providers(dylibs);
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if !sym.is_used() || sym.is_defined() {
            return;
        }
        for (dylib_idx, dylib) in dylibs.iter().enumerate().skip(first) {
            if dylib.exports.contains(sym.name()) {
                import_from_dylib(sym, dylibs, &providers, dylib_idx);
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
    intern_command_line_symbols(ctx);
    clear_claims(ctx);
    do_resolve(ctx, false);
    mark_live_objects(ctx);
    clear_claims(ctx);
    do_resolve(ctx, true);
    claim_locals(ctx);
}

/// Symbols the command line names (-e, -u) exist even when no object
/// mentions them, so that a dylib export can claim them: an app
/// extension's entry point, _NSExtensionMain, lives in Foundation and
/// nothing in the extension references it.
fn intern_command_line_symbols<E: Target>(ctx: &mut Context<E>) {
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

/// One resolution round over the objects - all of them, or with
/// `only_alive` just the live ones - like mold's resolve_symbols_pass:
/// definitions race for each symbol by rank and the winners claim it,
/// common symbols merge, and dylib exports claim what the objects
/// leave undefined.
fn do_resolve<E: Target>(ctx: &mut Context<E>, only_alive: bool) {
    use std::sync::atomic::Ordering;

    let refs = collect_references(ctx, only_alive);
    let best = race_definitions(ctx, only_alive);
    claim_definitions(ctx, only_alive, &best);
    merge_common_symbols(ctx, &best);

    // Record the references seen this round. A symbol is a weak import
    // only if every reference to it is weak: one strong reference
    // anywhere makes it strong (ld64's default, -weak_reference_
    // mismatches non-weak), and so binds it non-weakly and keeps its
    // dylib loaded non-weakly.
    for i in 0..ctx.symbols.syms.len() {
        if refs.strong[i].load(Ordering::Relaxed) {
            let sym = &mut ctx.symbols.syms[i];
            sym.set_is_strong_ref(true);
            sym.set_is_weak_ref(false);
        } else if refs.weak[i].load(Ordering::Relaxed) && !ctx.symbols.syms[i].is_strong_ref() {
            ctx.symbols.syms[i].set_is_weak_ref(true);
        }
    }

    // A relocatable link keeps every reference undefined rather than
    // binding it to a dylib.
    if !ctx.args.relocatable {
        claim_dylib_exports(ctx, &refs.used, &best);
    }

    // Record the final usage set for downstream passes.
    for (i, u) in refs.used.iter().enumerate() {
        ctx.symbols.syms[i].set_is_used(u.load(Ordering::Relaxed));
    }
}

/// Which symbols the objects considered in a round reference, and how.
struct References {
    used: Vec<std::sync::atomic::AtomicBool>,
    weak: Vec<std::sync::atomic::AtomicBool>,
    strong: Vec<std::sync::atomic::AtomicBool>,
}

/// Which symbols the files considered this round actually reference.
/// References from dead archive members must not count: they would
/// otherwise demand definitions nothing live needs. What the command
/// line names (-u, -e, -alias) counts as referenced.
fn collect_references<E: Target>(ctx: &Context<E>, only_alive: bool) -> References {
    use std::sync::atomic::{AtomicBool, Ordering};
    let n = ctx.symbols.syms.len();
    let refs = References {
        used: (0..n).map(|_| AtomicBool::new(false)).collect(),
        weak: (0..n).map(|_| AtomicBool::new(false)).collect(),
        strong: (0..n).map(|_| AtomicBool::new(false)).collect(),
    };
    ctx.objs.par_iter().filter(|obj| !only_alive || obj.is_alive).for_each(|obj| {
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab() && nlist.is_extern() && nlist.n_type() == N_UNDF {
                refs.used[sym_id as usize].store(true, Ordering::Relaxed);
                if nlist.n_desc & N_WEAK_REF != 0 {
                    refs.weak[sym_id as usize].store(true, Ordering::Relaxed);
                } else {
                    refs.strong[sym_id as usize].store(true, Ordering::Relaxed);
                }
            }
        }
    });

    let named = ctx
        .args
        .forced_undefined
        .iter()
        .chain(std::iter::once(&ctx.args.entry))
        .chain(ctx.args.aliases.iter().map(|(existing, _)| existing));
    for name in named {
        if let Some(id) = ctx.symbols.get(name) {
            refs.used[id as usize].store(true, Ordering::Relaxed);
        }
    }
    refs
}

/// The rank of a definition: (class << 40) | (alignment term << 32) |
/// priority, lower is better. Among weak definitions ld64 keeps the
/// copy with the greatest alignment (a Swift metadata record comes
/// 8-aligned from one object and 16-aligned from another; the first
/// copy wins only at equal alignment), so a live weak definition's rank
/// carries its subsection's alignment, inverted.
fn definition_rank(
    isecs: &[InputSection],
    obj: &crate::input_files::ObjectFile,
    nlist: &NList,
) -> Option<u64> {
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
        && let Some((isec, _)) =
            crate::input_files::find_symbol_subsec(isecs, &obj.subsecs, nlist.n_sect, nlist.n_value)
    {
        align_term = 63 - isecs[isec].p2align as u64;
    }
    Some((class << 40) | (align_term << 32) | obj.priority as u64)
}

/// The best definition rank of each symbol. Ranks race into it with an
/// atomic minimum, as in mold: the race is order-free because the
/// winner is the same whatever the interleaving, and since each object
/// has a unique priority, exactly one object ends up owning each
/// symbol.
fn race_definitions<E: Target>(
    ctx: &Context<E>,
    only_alive: bool,
) -> Vec<std::sync::atomic::AtomicU64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    let best: Vec<AtomicU64> =
        (0..ctx.symbols.syms.len()).map(|_| AtomicU64::new(u64::MAX)).collect();
    ctx.objs.par_iter().filter(|obj| !only_alive || obj.is_alive).for_each(|obj| {
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if let Some(rank) = definition_rank(&ctx.isecs, obj, nlist) {
                best[sym_id as usize].fetch_min(rank, Ordering::Relaxed);
            }
        }
    });
    best
}

/// Each object writes the symbols whose race it won. Ranks are unique
/// per object, so every symbol has exactly one writer and the parallel
/// writes are disjoint.
fn claim_definitions<E: Target>(
    ctx: &mut Context<E>,
    only_alive: bool,
    best: &[std::sync::atomic::AtomicU64],
) {
    use std::sync::atomic::Ordering;
    struct SymsPtr(*mut crate::symbol::Symbol);
    unsafe impl Sync for SymsPtr {}
    let syms_ptr = SymsPtr(ctx.symbols.syms.as_mut_ptr());
    let syms_ptr = &syms_ptr;
    let isecs = &ctx.isecs;

    ctx.objs.par_iter().enumerate().filter(|(_, obj)| !only_alive || obj.is_alive).for_each(
        |(obj_idx, obj)| {
            let r = obj.global_range();
            for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                let Some(rank) = definition_rank(isecs, obj, nlist) else {
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
}

/// Common symbols merge: the largest size and strictest alignment win
/// regardless of input order, gathered from every common claim once
/// the class-3 winners are known.
fn merge_common_symbols<E: Target>(ctx: &mut Context<E>, best: &[std::sync::atomic::AtomicU64]) {
    use std::sync::atomic::Ordering;
    let commons: Vec<(crate::symbol::SymbolId, u64, u8)> = ctx
        .objs
        .par_iter()
        .filter(|obj| obj.is_alive)
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
}

/// Dylib exports claim the referenced symbols that no object defines,
/// or that only a lazy archive member does; an earlier dylib beats a
/// later archive member and vice versa.
fn claim_dylib_exports<E: Target>(
    ctx: &mut Context<E>,
    used: &[std::sync::atomic::AtomicBool],
    best: &[std::sync::atomic::AtomicU64],
) {
    use std::sync::atomic::Ordering;
    let dylibs = &ctx.dylibs;
    let providers = merged_providers(dylibs);
    ctx.symbols.syms.par_iter_mut().enumerate().for_each(|(i, sym)| {
        if !used[i].load(Ordering::Relaxed) {
            return;
        }
        let won = best[i].load(Ordering::Relaxed);
        if sym.is_common() || won >> 40 < 2 {
            return;
        }
        for (dylib_idx, dylib) in dylibs.iter().enumerate() {
            let rank = (2u64 << 40) | dylib.priority as u64;
            if rank < won && dylib.exports.contains(sym.name()) {
                let owner = import_from_dylib(sym, dylibs, &providers, dylib_idx);
                // -weak_framework / -weak_library / -weak-l: every
                // import from the library is a weak import (ld64 binds
                // it weak-import and marks it N_WEAK_REF), whatever the
                // references say.
                if dylibs[owner].is_weak {
                    sym.set_is_weak_ref(true);
                }
                break;
            }
        }
    });
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
    use std::sync::atomic::{AtomicBool, Ordering};

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
        let native_refs: Vec<AtomicBool> =
            (0..ctx.symbols.syms.len()).map(|_| AtomicBool::new(false)).collect();
        ctx.objs.par_iter().filter(|obj| obj.is_alive && obj.lto_module.is_none()).for_each(
            |obj| {
                let r = obj.global_range();
                for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                    if !nlist.is_stab() && nlist.n_type() == N_UNDF {
                        native_refs[sym_id as usize].store(true, Ordering::Relaxed);
                    }
                }
            },
        );
        let mut preserve: Vec<std::ffi::CString> = Vec::new();
        for (i, sym) in ctx.symbols.syms.iter().enumerate() {
            if let Some(FileId::Obj(idx)) = sym.file()
                && ctx.objs[idx as usize].is_alive
                && ctx.objs[idx as usize].lto_module.is_some()
                && sym.is_extern()
            {
                if executable
                    && !ctx.args.export_dynamic
                    && (!sym.is_used() || !native_refs[i].load(Ordering::Relaxed))
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
        // a power of two, at most 2^15 (a 100000-byte array asks for
        // 32KB, which the page then caps with a warning).
        let p2align = if sym.common_p2align != 0 || size == 0 {
            sym.common_p2align
        } else {
            (size.next_power_of_two().trailing_zeros() as u8).min(15)
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
    // -no_fixup_chains keeps __mod_init_func. Not so for a -static
    // image, whose initializers dyld never runs (XNU runs the kernel's
    // __mod_init_func itself): it converts only with -init_offsets.
    let implied = !ctx.args.static_link
        && ctx.args.fixup_chains.unwrap_or_else(|| ctx.chained_fixups_by_default());
    if !ctx.args.init_offsets && !implied {
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

/// ld-prime's diagnostics for static initializers: a warning for each
/// in a dylib bound for the dyld shared cache, where every process
/// would run it, unless -no_warn_inits; with -no_inits, an error listing
/// them all.
pub fn check_initializers<E: Target>(ctx: &Context<E>) {
    let args = &ctx.args;
    let warn = args.shared_region && args.output_type == MH_DYLIB && !args.no_warn_inits;
    if !warn && !args.no_inits {
        return;
    }
    let inits = initializers(ctx);
    if args.no_inits {
        if !inits.is_empty() {
            let list: String =
                inits.iter().map(|(name, file)| format!("{name} in {file}\n")).collect();
            error!("Static initializers:\n{list}");
        }
        return;
    }
    for (name, file) in inits {
        crate::warn!(
            "static initializer '{name}' found in '{file}'. Use -no_inits to make this an \
             error.  Use -no_warn_inits to suppress warning"
        );
    }
}

/// The functions the inputs' __mod_init_func sections point at, by
/// name, with the files that hold the pointers.
fn initializers<E: Target>(ctx: &Context<E>) -> Vec<(&str, String)> {
    let mut vec = Vec::new();
    for (i, isec) in ctx.isecs.iter().enumerate() {
        if !isec.is_alive() || ctx.hdr_of(isec).section_type() != S_MOD_INIT_FUNC_POINTERS {
            continue;
        }
        let obj = &ctx.objs[isec.file as usize];
        let mut relocs = ctx.isec_relocs(i).to_vec();
        relocs.sort_by_key(|r| r.offset);
        for rel in relocs {
            let name = match rel.target() {
                RelocTarget::Sym(idx) => ctx.symbols[obj.symbols[idx as usize]].name(),
                RelocTarget::Section(target) => {
                    let target = ctx.resolve_isec(target as usize) as u32;
                    obj.symbols
                        .iter()
                        .map(|&id| &ctx.symbols[id])
                        .find(|s| s.input_section() == Some(target) && s.value == rel.addend as u64)
                        .map_or("", |s| s.name())
                }
            };
            vec.push((name, resolved_file_name(obj.mf)));
        }
    }
    vec
}

/// Validates only objects selected by resolution, including the LTO
/// output. Unused archive members must not cause errors or warnings.
pub fn check_input_versions<E: Target>(ctx: &Context<E>) {
    // A -r or -preload output for no platform takes any object.
    let (platform, minos) = (ctx.args.platform, ctx.args.platform_minos);
    if platform == 0 {
        return;
    }
    for (i, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        // An object may declare more than one platform; use the
        // deployment target for the platform being linked. ld-prime
        // takes one with no version command (an old one, or one
        // assembled for no OS) for macOS, with a warning in a macOS
        // link. The object the linker synthesizes has none either.
        let Some(first) = obj.platform_versions.first() else {
            if platform == crate::macho::PLATFORM_MACOS && !ctx.is_internal(i) {
                crate::warn!(
                    "no platform load command found in '{}', assuming: macOS",
                    resolved_file_name(obj.mf)
                );
            }
            continue;
        };
        let Some(version) = obj.platform_versions.iter().find(|v| v.platform == platform) else {
            // Firmware takes code built for any platform.
            if platform == crate::macho::PLATFORM_FIRMWARE {
                continue;
            }
            crate::error!(
                "building for '{}', but linking in object file ({}) built for '{}'",
                platform_name(platform),
                resolved_file_name(obj.mf),
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

/// Marks the literal records a symbol names, other than a temporary
/// (L-prefixed, which the arm64 assembler keeps for relocations to
/// name) or linker-private (l-prefixed, the assembler's ltmpN labels
/// included) one: ld-prime keeps each such record an atom of its own,
/// merged with no identical copy, and a -r output keeps its label
/// rather than naming it LC<n>/l<nnn>.
fn mark_labeled_literals<E: Target>(ctx: &Context<E>) {
    ctx.symbols.syms.par_iter().for_each(|sym| {
        if let Some(i) = sym.input_section()
            && !sym.name().is_empty()
            && !sym.name().starts_with(['l', 'L'])
        {
            let isec = &ctx.isecs[i as usize];
            if matches!(
                ctx.hdr_of(isec).section_type(),
                S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS
            ) {
                isec.mark_labeled();
            }
        }
    });
}

/// Merges identical literal elements across all live inputs: the first
/// live copy wins and the rest redirect to it. A labeled record stays
/// apart, and so do copies in sections of different names, as in
/// ld-prime: a class named "Foo" keeps its name in __objc_classname
/// though __cstring has a "Foo" too.
pub fn merge_literals<E: Target>(ctx: &mut Context<E>) {
    mark_labeled_literals(ctx);
    // Deduplication follows the symbol table's sharded shape: every
    // element's content hash is computed in parallel, elements bin by
    // hash, and the shards resolve independently - within a shard the
    // first occurrence in input order wins, which is exactly the
    // winner the old serial single-map walk picked.
    let hashed: Vec<(u64, &MachSection, u32)> = ctx
        .isecs
        .par_iter()
        .enumerate()
        .filter_map(|(i, isec)| {
            if !isec.is_alive()
                || isec.replacement != crate::input_sections::NO_REPLACEMENT
                || isec.is_labeled()
            {
                return None;
            }
            let hdr = ctx.hdr_of(isec);
            if !matches!(
                hdr.section_type(),
                S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS
            ) || is_unterminated_string(hdr, isec)
            {
                return None;
            }
            Some((xxhash_rust::xxh3::xxh3_64(isec.data()), hdr, i as u32))
        })
        .collect();

    const NUM_SHARDS: usize = 64;
    let mut bins: Vec<Vec<(u64, &MachSection, u32)>> = vec![Vec::new(); NUM_SHARDS];
    for &e in &hashed {
        bins[(e.0 % NUM_SHARDS as u64) as usize].push(e);
    }

    let isecs = &ctx.isecs;
    let folds: Vec<Vec<(u32, u32)>> = bins
        .into_par_iter()
        .map(|bin| {
            // Keyed by the content hash already computed; a match is
            // the same bytes in a section of the same name.
            let mut table: hashbrown::HashTable<(u64, &MachSection, u32)> =
                hashbrown::HashTable::new();
            let mut out = Vec::new();
            for (hash, hdr, i) in bin {
                let data = isecs[i as usize].data();
                let same = |&(h, other, j): &(u64, &MachSection, u32)| {
                    h == hash
                        && other.segname == hdr.segname
                        && other.sectname == hdr.sectname
                        && other.section_type() == hdr.section_type()
                        && isecs[j as usize].data() == data
                };
                match table.entry(hash, same, |e| e.0) {
                    hashbrown::hash_table::Entry::Occupied(e) => out.push((i, e.get().2)),
                    hashbrown::hash_table::Entry::Vacant(e) => {
                        e.insert((hash, hdr, i));
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

/// Whether a C-string literal is the unterminated string that ended its
/// input section, which the NUL it lacks completes past the section's
/// end (see initialize_sections). ld-prime merges it with no other
/// string, not even an identical one.
fn is_unterminated_string(hdr: &crate::macho::MachSection, isec: &InputSection) -> bool {
    isec.input_addr as u64 + isec.size as u64 > hdr.addr + hdr.size
}

/// Points every symbol defined in a merged-away subsection at the
/// surviving one - mold makes the merged section's fragment the
/// symbol's origin - so a symbol's address never follows a replacement
/// chain. The copies are identical, so the symbol's offset is
/// unchanged. (Section-relative relocations still resolve through the
/// chain in isec_addr.)
pub(crate) fn redirect_symbols_to_replacements<E: Target>(ctx: &mut Context<E>) {
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
/// copy. The kept copy stands for all of them whatever their sizes, as
/// in ld64: an inline function compiled at different optimization
/// levels, or a Swift __swift5_typeref string with or without a pad
/// byte, still has one definition, and a loser's bytes, relocations,
/// unwind info and data-in-code go with it. The defining symbol must
/// sit at the same offset in both copies, though.
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
    // walk in object order - the same order and the same resolve
    // checks as the original loop, over only the qualifying weak defs.
    for list in candidates {
        for (loser, winner, off, sym_value) in list {
            let winner = ctx.resolve_isec(winner);
            let loser = ctx.resolve_isec(loser);
            if loser == winner
                || off != sym_value
                || ctx.isecs[loser].replacement != crate::input_sections::NO_REPLACEMENT
            {
                continue;
            }
            ctx.isecs[loser].replacement = winner as u32;
        }
    }
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
            // A -static image has no dyld to look a symbol up at run
            // time, so ld-prime lets none stay undefined, whatever
            // -undefined or -U say.
            let allowed = !ctx.args.static_link
                && (ctx.args.undefined_dynamic_lookup
                    || ctx.args.allowed_undefined.iter().any(|n| n == sym.name()))
                && !initial.contains(&(i as crate::symbol::SymbolId));
            if allowed {
                let sym = &mut ctx.symbols[i];
                sym.set_file(FileId::Dylib((usize::MAX) as u32));
                sym.set_is_imported(true);
                sym.set_is_extern(true);
            } else {
                // ld-prime points at the auto-linked libraries it could
                // not find first.
                for msg in std::mem::take(&mut ctx.autolink_misses) {
                    crate::warn!("{msg}");
                }
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

/// -t lists the link's inputs, one line per file loaded: objects and
/// stubs by the path they were found at, every member of an archive
/// as archive(member), used or not, and each library a stub re-exports
/// (by its install name if the stub inlines it), once each, as ld-prime
/// does. ld-prime prints them in an order that varies from run to run.
pub fn print_trace<E: Target>(ctx: &Context<E>) {
    if !ctx.args.trace {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    for name in &ctx.traced_files {
        if seen.insert(name) {
            println!("{name}");
        }
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

/// An image bound for the dyld shared cache may link only libraries
/// that are in it too, since the cache builder binds every dependency
/// inside the cache. ld-prime rejects the first dylib in load-command
/// order installed anywhere else (@rpath, /usr/local, /Library, ...);
/// one that -dead_strip_dylibs drops doesn't count.
fn check_shared_cache_deps<E: Target>(ctx: &Context<E>) {
    if !ctx.args.shared_region {
        return;
    }
    if let Some(dylib) = ctx
        .dylibs
        .iter()
        .filter(|d| !d.is_bundle_loader && !crate::cmdline::in_shared_cache_path(&d.install_name))
        .min_by_key(|d| d.dylib_idx)
    {
        error!(
            "Shared cache eligible dylib cannot link to ineligible dylib '{}'.  Remove link to \
             ineligible dylib, fix its eligibility, or opt out of the shared cache using the \
             build setting 'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag \
             '-not_for_dyld_shared_cache')",
            crate::util::display(&dylib.install_name)
        );
    }
}

/// Warns about each dylib the command line links that nothing binds
/// to. ld-prime does so by default for a dylib bound for the dyld shared
/// cache, where each needless load costs every process, and for any
/// output under -warn_unused_dylibs. A -needed_* or
/// -reexport_* library is linked on purpose, and libSystem, libc++ and
/// Foundation, which compiler drivers and project templates link by
/// habit, are let off.
fn warn_unused_dylibs<E: Target>(ctx: &Context<E>) {
    let for_shared_cache = ctx.args.shared_region && ctx.args.output_type == MH_DYLIB;
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
        if !bound[i] && dylib.is_bundle_loader {
            let real = std::fs::canonicalize(&dylib.path).unwrap_or_else(|_| dylib.path.clone());
            crate::warn!(
                "linking with bundle loader ({}) but not using any symbols from it",
                real.display()
            );
        } else if !bound[i]
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
        } else if !dylib.is_implicit && !dylib.is_autolinked {
            let (priority, path) = dylib.named_at.unwrap_or((dylib.priority, dylib.path));
            ctx.stripped_dylibs.push((priority, path));
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
    // Only a dylib can have an upward dependency, one that depends on
    // it in turn: ld-prime loads the library as usual for anything else,
    // with a warning.
    if ctx.args.output_type != MH_DYLIB {
        for &i in &order {
            let dylib = &mut ctx.dylibs[i];
            if dylib.is_upward {
                let name = crate::util::display(&dylib.install_name);
                crate::warn!("ignoring upward dylib option for {name}");
                dylib.is_upward = false;
            }
        }
    }
    check_shared_cache_deps(ctx);
    check_libsystem_linked(ctx);
}

/// ld64 takes a dynamic image that would load no dylib at all for one
/// linked without libSystem by mistake (a stray -nostdlib) and refuses
/// it: an executable other than a -static one, or a dylib or bundle,
/// -static or not. Any dylib left after -dead_strip_dylibs, the bundle
/// loader included, will do, libSystem or not. ld-prime does the same
/// and, like ld64, lets off libsystem_kernel, which libSystem is built
/// on, and any link with an exit-asm.o (a stopgap for rdar://39514191).
/// Firmware has no libSystem to link.
fn check_libsystem_linked<E: Target>(ctx: &Context<E>) {
    if ctx.args.platform == crate::macho::PLATFORM_FIRMWARE {
        return;
    }
    let dynamic = match ctx.args.output_type {
        MH_EXECUTE => !ctx.args.static_link,
        MH_DYLIB | MH_BUNDLE => true,
        _ => false,
    };
    if !dynamic || !ctx.dylibs.is_empty() {
        return;
    }
    let is_exit_asm = |obj: &input_files::ObjectFile| {
        memchr::memmem::find(path_bytes(&obj.mf.name), b"exit-asm.o").is_some()
    };
    if ctx.args.install_name.as_deref() == Some(b"/usr/lib/system/libsystem_kernel.dylib")
        || ctx.objs.iter().any(is_exit_asm)
    {
        return;
    }
    fatal!("dynamic executables or dylibs must link with libSystem.dylib");
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
                // A GOT load of a local symbol needs no slot at all:
                // it relaxes, or ld-prime refuses the instruction.
                let class = E::classify_reloc(rel.r_type);
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
            // An x86-64 kext calls an import directly; kmutil fills in
            // the call by an external relocation.
            RelocClass::Branch if ctx.args.is_kext() && E::CPUTYPE == CPU_TYPE_X86_64 => {}
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

pub(crate) fn add_got<E: Target>(ctx: &mut Context<E>, id: crate::symbol::SymbolId) {
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
    // references share. In the shared region the weak-lookup slots,
    // which form __weak_got, go last.
    let got = std::mem::take(&mut ctx.got.got_syms);
    let objc = ctx.objc_stubs.msgsend_got_idx as usize;
    let in_weak_got = |id| ctx.args.shared_region && ctx.binds_weak_lookup(id);
    let mut order: Vec<usize> = (0..got.len()).collect();
    order.par_sort_by_key(|&i| {
        let id = got[i];
        let name = crate::util::name_sort_key(ctx.symbols[id].name());
        (in_weak_got(id), got_rank(ctx, id), name, i != objc)
    });
    ctx.got.weak_start = order.iter().position(|&i| in_weak_got(got[i])).unwrap_or(order.len());
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
    let header_addr = mach_header_addr(ctx);
    // A -preload image's mach header is in no segment, and nothing
    // names it.
    if ctx.args.output_type == MH_EXECUTE && !ctx.args.preload {
        let id = ctx.symbols.intern("__mh_execute_header");
        let sym = &mut ctx.symbols[id];
        if !sym.is_defined() {
            sym.set_file(FileId::Obj(internal));
            sym.value = header_addr;
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
        sym.value = header_addr;
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

    // A -preload image's mach header is in no segment; ___dso_handle
    // names the start of __TEXT, where the header would be, as in
    // ld-prime.
    if ctx.args.preload
        && let Some(text) = ctx.segments.iter().find(|s| s.name == "__TEXT")
        && let Some(id) = ctx.symbols.get("___dso_handle")
        && ctx.symbols[id].input_section().is_none()
    {
        ctx.symbols[id].value = text.cmd.vmaddr;
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
        ("__DATA_CONST", "__objc_catlist2") => 8,
        ("__DATA_CONST", "__objc_nlcatlist") => 9,
        ("__DATA_CONST", "__objc_protolist") => 10,
        ("__DATA_CONST", "__objc_imageinfo") => 11,
        ("__DATA_CONST", "__objc_protorefs") => 12,
        ("__DATA_CONST", "__objc_superrefs") => 13,
        // The GOT closes __DATA_CONST, after every input-derived
        // section (ld-prime: __cfstring, __objc_classlist,
        // __objc_imageinfo, then __got). In the shared region the lazy
        // pointers lead it, and the class data, __weak_got and the
        // selector references come before __got.
        ("__DATA_CONST", "__la_symbol_ptr") => 0,
        ("__DATA_CONST", "__objc_const") => 21,
        ("__DATA_CONST", "__weak_got") => 22,
        ("__DATA_CONST", "__objc_selrefs") => 23,
        ("__DATA_CONST", "__got") => 25,
        ("__DATA_CONST", _) => 20,
        // Without __DATA_CONST (-no_data_const, an x86-64 kext),
        // ld-prime's __DATA starts with the lazy pointers and the
        // initializer and terminator lists, and the GOT follows the
        // input sections.
        ("__DATA", "__la_symbol_ptr") => 0,
        ("__DATA", "__mod_init_func" | "__mod_term_func") => 1,
        ("__DATA", "__got") => 25,
        ("__DATA", "__objc_const") => 2,
        ("__DATA", "__objc_selrefs") => 3,
        ("__DATA", "__objc_protorefs") => 4,
        ("__DATA", "__objc_classrefs") => 5,
        ("__DATA", "__objc_superrefs") => 6,
        ("__DATA", "__objc_ivar") => 7,
        ("__DATA", "__objc_data") => 8,
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

/// Where ld-prime moves a __TEXT section of an image bound for the
/// shared region: the stubs and the Objective-C names, which the shared
/// cache builder bypasses and uniques, come after __unwind_info (100)
/// and __eh_frame (101), the names in a fixed order.
fn shared_region_text_rank<E: Target>(
    ctx: &Context<E>,
    hdr: &crate::chunks::ChunkHeader,
) -> Option<u32> {
    if !ctx.args.shared_region || hdr.segname != "__TEXT" {
        return None;
    }
    match hdr.sectname.as_str() {
        "__objc_stubs" => Some(102),
        "__stubs" => Some(103),
        "__objc_classname" => Some(104),
        "__objc_methname" => Some(105),
        "__objc_methtype" => Some(106),
        _ => None,
    }
}

/// The segment for read-only-after-fixup data: __DATA_CONST unless
/// -no_data_const.
pub(crate) fn data_seg<E: Target>(ctx: &Context<E>) -> &'static str {
    if ctx.args.data_const { "__DATA_CONST" } else { "__DATA" }
}

/// Sections a final link places in __DATA_CONST: data that needs no
/// writes after dyld's fixups. ld-prime's list - signed pointers
/// (__auth_ptr), CF and ObjC constant objects, the ObjC lists and the
/// initializer lists - but for the ones only a condition moves (see
/// SectionMap::const_name). A section not on it, such as
/// __objc_boolobj, stays in __DATA.
const DATA_CONST_SECTIONS: &[&str] = &[
    "__auth_ptr",
    "__cfstring",
    "__const",
    "__const_cfobj2",
    "__got",
    "__mod_init_func",
    "__mod_term_func",
    "__objc_arraydata",
    "__objc_arrayobj",
    "__objc_dateobj",
    "__objc_dictobj",
    "__objc_doubleobj",
    "__objc_floatobj",
    "__objc_intobj",
    "__objc_catlist",
    "__objc_catlist2",
    "__objc_classlist",
    "__objc_imageinfo",
    "__objc_nlcatlist",
    "__objc_nlclslist",
    "__objc_protolist",
];

/// Class, protocol and superclass references are written by the
/// Objective-C runtime on older systems, so they stay in __DATA unless
/// the deployment target is macOS 15 or later, where ld64 moves them to
/// __DATA_CONST (dyld fixes them up there; most class references fold
/// into __got).
pub(crate) fn objc_refs_are_const<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.platform == crate::macho::PLATFORM_MACOS
        && ctx.args.platform_minos >= crate::macho::encode_version(15, 0, 0)
}

/// dyld reads an image's interposing tuples (__DATA,__interpose) but
/// never writes them, so from macOS 15 on ld-prime makes them read-only
/// after fixups in any image dyld loads: they go to __DATA_CONST, even
/// with -no_data_const.
fn interpose_is_const<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.platform == crate::macho::PLATFORM_MACOS
        && ctx.args.platform_minos >= crate::macho::encode_version(15, 0, 0)
        && !ctx.args.without_dyld()
}

/// An output section's name: (segment, section).
type SectionName = (&'static str, &'static str);

/// The output section an input section with `flags` lands in, and the
/// name its flags follow; None for one the link consumes or drops.
/// `args` gives -rename_section and -rename_segment.
///
/// The __LLVM segment (bitcode, __swift_modhash, __cmdline, __asm) is
/// copied into no output, and __objc_clsrolist, a compiler-to-linker
/// list of the class_ro_t records of generic Swift classes (nothing
/// references it), into no image. ld-prime names the rest in three
/// steps. Its own moves come first (see SectionMap::builtin_name), so
/// -rename_section matches __DATA_CONST,__const, not __DATA,__const;
/// then -rename_section and -rename_segment rename that name (see
/// SectionMap::renamed for __interpose's move, which comes last); and a
/// section the renames leave in place then merges as in ld64 -
/// __StaticInit into __text, the fixed-size literal pools
/// (__literal4/8/16, already merged per element) into __const - under
/// that section's renamed name, if it is the standard section of its
/// name (see is_standard_section): a regular __literal8 holds no
/// literals, and a section renamed __literal8 is not one of the pools.
/// The flags follow the name before the renames: a renamed
/// __objc_classlist is still a list the runtime scans, a renamed
/// __literal8 still a literal pool. A -r output keeps every section
/// as it came, but for the renames.
fn output_section_for(
    args: &crate::cmdline::Args,
    map: SectionMap,
    segname: &str,
    sectname: &str,
    flags: u32,
) -> Option<(SectionName, SectionName)> {
    if segname == "__LLVM" {
        return None;
    }
    let name = (static_name(segname), static_name(sectname));
    if map.relocatable {
        return Some((renamed(args, name), name));
    }
    if name == ("__DATA", "__objc_clsrolist") {
        return None;
    }
    let name = map.builtin_name(name, flags);
    let out = map.renamed(args, name);
    Some(match merged_name(name) {
        Some(merged) if out == name && is_standard_section(segname, sectname, flags) => {
            (renamed(args, merged), merged)
        }
        _ => (out, name),
    })
}

/// The section a final link merges a __TEXT section into, like ld64:
/// __StaticInit joins __text, and the literal pools join __const.
fn merged_name(name: SectionName) -> Option<SectionName> {
    match name {
        ("__TEXT", "__StaticInit") => Some(("__TEXT", "__text")),
        ("__TEXT", "__literal4" | "__literal8" | "__literal16") => Some(("__TEXT", "__const")),
        _ => None,
    }
}

/// Applies -rename_section and then -rename_segment to a section's
/// name, as ld-prime does: the first -rename_section naming the
/// section renames it, and the first -rename_segment naming the
/// resulting segment then moves it - after a -rename_section too, so
/// a section renamed into a renamed segment moves on. Neither applies
/// twice: -rename_section chains A to B and B to C take A to B. A
/// section of a legacy name no -rename_section names takes its modern
/// name in its place (see modern_name).
pub(crate) fn renamed(args: &crate::cmdline::Args, name: SectionName) -> SectionName {
    let (seg, sect) = name;
    let (seg, sect) = match args.rename_sections.iter().find(|(s, t, _, _)| s == seg && t == sect) {
        Some((_, _, s, t)) => (static_name(s), static_name(t)),
        None => modern_name(name),
    };
    (renamed_segment(args, seg), sect)
}

/// The name ld-prime gives a section of a name old compilers used for
/// coalesced (weak) code and data, which lives in the usual sections
/// now: __textcoal_nt is __text, __const_coal __const and
/// __datacoal_nt __data, in the segments ld-prime knows the old names
/// in. It renames them in a final image and a -r output alike, and a
/// boundary symbol's section too, but the flags and the __DATA_CONST
/// move follow the old name: a __DATA,__const_coal stays in __DATA.
fn modern_name(name: SectionName) -> SectionName {
    match name {
        ("__TEXT", "__textcoal_nt") => ("__TEXT", "__text"),
        ("__TEXT" | "__DATA" | "__DATA_CONST", "__const_coal") => (name.0, "__const"),
        ("__DATA" | "__DATA_DIRTY", "__datacoal_nt") => (name.0, "__data"),
        _ => name,
    }
}

/// The segment -rename_segment moves a segment's sections to.
fn renamed_segment(args: &crate::cmdline::Args, seg: &'static str) -> &'static str {
    match args.rename_segments.iter().find(|(old, _)| old == seg) {
        Some((_, new)) => static_name(new),
        None => seg,
    }
}

/// A section or segment name that lives as long as the output's
/// headers: the usual segment names are literals, and the rest are
/// leaked (the callers name each distinct section once).
fn static_name(name: &str) -> &'static str {
    match name {
        "__TEXT" => "__TEXT",
        "__DATA_CONST" => "__DATA_CONST",
        "__DATA" => "__DATA",
        _ => String::leak(name.to_string()),
    }
}

/// What decides where output_section_for puts an input section.
#[derive(Clone, Copy)]
struct SectionMap {
    relocatable: bool,
    data_const: bool,
    objc_const_refs: bool,
    const_interpose: bool,
    shared_region: bool,
    relative_methods: bool,
    text_exec: bool,
}

impl SectionMap {
    /// The name ld-prime gives an input section of a final image, with
    /// the section's `flags`, before -rename_section and
    /// -rename_segment: with -text_exec (an arm64 kext) every section
    /// of code - pure instructions, in any segment - moves into
    /// __TEXT_EXEC,__text, and data that needs no writes after fixups
    /// to __DATA_CONST.
    fn builtin_name(self, name: SectionName, flags: u32) -> SectionName {
        if self.text_exec && flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
            return ("__TEXT_EXEC", "__text");
        }
        if !is_standard_section(name.0, name.1, flags) {
            return name;
        }
        self.const_name(name)
    }

    /// A standard __DATA section's name in a final image when it needs
    /// no writes after dyld's fixups: the same section in __DATA_CONST,
    /// unless -no_data_const - in the shared region, where dyld fixes
    /// them up for good, the selector references and the Objective-C
    /// runtime's class data too. ld-prime treats this move as a
    /// renaming, which boundary symbols follow as well (unlike
    /// -text_exec's: section$start$__TEXT$__text stays in __TEXT).
    fn const_name(self, name: SectionName) -> SectionName {
        let (seg, sect) = name;
        let is_const = match sect {
            "__objc_classrefs" | "__objc_protorefs" | "__objc_superrefs" => self.objc_const_refs,
            "__objc_selrefs" => self.shared_region,
            // Unless it holds absolute method lists, which the runtime
            // sorts in place.
            "__objc_const" => self.shared_region && self.relative_methods,
            _ => DATA_CONST_SECTIONS.contains(&sect),
        };
        if seg == "__DATA" && self.data_const && is_const { ("__DATA_CONST", sect) } else { name }
    }

    /// The section a section$start$ or section$end$ symbol names: the
    /// one an input section of that name lands in - or, for a pointer
    /// section only the linker makes, where ld-prime puts it: its GOTs
    /// in __DATA_CONST and, in the shared region, its lazy pointers
    /// too. (An input section of one of those names is data like any
    /// other to ld-prime, which rejects one typed as pointers.)
    fn boundary_name(self, name: SectionName) -> SectionName {
        let is_const = match name {
            ("__DATA", "__auth_got" | "__weak_got" | "__weak_auth_got") => true,
            ("__DATA", "__la_symbol_ptr" | "__lazy_load_got") => self.shared_region,
            _ => false,
        };
        if self.data_const && is_const { ("__DATA_CONST", name.1) } else { self.const_name(name) }
    }

    /// A final image's section name after -rename_section and
    /// -rename_segment (see renamed). ld-prime moves the interposing
    /// tuples to __DATA_CONST (see interpose_is_const) in place of a
    /// -rename_section: one naming __DATA,__interpose keeps the section
    /// out of __DATA_CONST, one naming __DATA_CONST,__interpose never
    /// applies, and -rename_segment moves the section on from there.
    /// The move takes any section of that name, -sectcreate's too, but
    /// not one a -rename_section gives the name.
    fn renamed(self, args: &crate::cmdline::Args, name: SectionName) -> SectionName {
        let is_renamed = args.rename_sections.iter().any(|(s, t, _, _)| s == name.0 && t == name.1);
        if name == ("__DATA", "__interpose") && self.const_interpose && !is_renamed {
            return (renamed_segment(args, "__DATA_CONST"), name.1);
        }
        renamed(args, name)
    }

    fn new<E: Target>(ctx: &Context<E>) -> Self {
        Self {
            relocatable: ctx.args.relocatable,
            data_const: ctx.args.data_const,
            objc_const_refs: objc_refs_are_const(ctx),
            const_interpose: interpose_is_const(ctx),
            shared_region: ctx.args.shared_region,
            relative_methods: objc_relative_method_lists(ctx),
            text_exec: ctx.args.text_exec,
        }
    }

    /// A final link's mapping, for the records the linker synthesizes.
    fn final_link<E: Target>(ctx: &Context<E>) -> Self {
        Self { relocatable: false, ..Self::new(ctx) }
    }
}

/// The flags an output section carries, from the flags ld-prime reads
/// its first member as having (see input_section_flags). In a final
/// image ld64 keeps the section type (a coalesced input section
/// becomes regular; a literal pool folded into __TEXT,__const is
/// regular), marks code (pure instructions) as having some
/// instructions, drops every other input attribute - no_dead_strip,
/// live_support, strip_static_syms and no_toc direct the linker, not
/// dyld, and some_instructions alone is but the assembler's note that
/// it emitted an instruction into the section - and marks just the
/// ObjC list sections the runtime scans as no-dead-strip. A -r output
/// is input to another link, so ld-prime copies the type and
/// attributes verbatim. __eh_frame carries the compiler's fixed flags
/// in both.
fn output_section_flags(segname: &str, sectname: &str, input: u32, relocatable: bool) -> u32 {
    if segname == "__TEXT" && sectname == "__eh_frame" {
        return S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT;
    }
    if relocatable {
        return input;
    }
    // The two reference lists the runtime may still write keep the
    // flags they came with (coalesced, no-dead-strip) while in __DATA
    // of a final image.
    if segname == "__DATA" && matches!(sectname, "__objc_protorefs" | "__objc_superrefs") {
        return input & (SECTION_TYPE | S_ATTR_NO_DEAD_STRIP);
    }
    // Selector references made constant (in the shared region) are
    // plain data to ld-prime.
    if segname == "__DATA_CONST" && sectname == "__objc_selrefs" {
        return S_REGULAR;
    }
    let mut ty = input & SECTION_TYPE;
    if ty == S_COALESCED || (segname == "__TEXT" && sectname == "__const") {
        ty = S_REGULAR;
    }
    let mut attrs = 0;
    if input & S_ATTR_PURE_INSTRUCTIONS != 0 {
        attrs = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
    }
    if matches!(
        sectname,
        "__objc_classlist"
            | "__objc_catlist"
            | "__objc_catlist2"
            | "__objc_nlclslist"
            | "__objc_nlcatlist"
            | "__objc_selrefs"
            | "__objc_classrefs"
    ) {
        attrs |= S_ATTR_NO_DEAD_STRIP;
    }
    ty | attrs
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

/// The output section named `seg`,`sect` - created if no input made
/// one - with a synthesized `tail` of `tail_size` bytes appended after
/// its input subsections, which are placed already.
fn tail_section<E: Target>(
    ctx: &mut Context<E>,
    seg: &'static str,
    sect: &str,
    flags: u32,
    p2align: u32,
    tail: Tail,
    tail_size: u64,
) -> OutputSectionId {
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
}

/// A record category merging rewrites in place of an input subsection,
/// such as a class's ro data, takes that subsection's position among
/// its output section's members, as ld-prime keeps
/// __OBJC_CLASS_RO_$_Foo where the input had it. Runs while the members
/// are still in input order; the other synthesized records go in the
/// section's tail.
fn place_replacing_blobs<E: Target>(ctx: &mut Context<E>) {
    let blobs: hashbrown::HashSet<u32> = ctx.data_blobs.iter().map(|b| b.isec).collect();
    let mut anchors: Vec<(u32, u32)> = (0..ctx.isecs.len())
        .filter(|&i| blobs.contains(&ctx.isecs[i].replacement))
        .map(|i| (i as u32, ctx.isecs[i].replacement))
        .collect();
    let mut seen = hashbrown::HashSet::new();
    anchors.retain(|&(_, blob)| seen.insert(blob));
    // Last first: a blob inserted (with its high index) then only ever
    // sits after the members the next, lower anchor is searched among.
    for (replaced, blob) in anchors.into_iter().rev() {
        let hdr = ctx.hdr_of(&ctx.isecs[replaced as usize]);
        let map = SectionMap::final_link(ctx);
        let out = output_section_for(&ctx.args, map, hdr.segname(), hdr.sectname(), hdr.flags);
        let Some(pos) = out.and_then(|((seg, sect), _)| {
            ctx.output_sections.iter().position(|o| o.hdr.segname == seg && o.hdr.sectname == sect)
        }) else {
            continue;
        };
        let p2align = ctx.isecs[blob as usize].p2align as u32;
        let osec = &mut ctx.output_sections[pos];
        let at = osec.members.partition_point(|&m| m < replaced);
        osec.members.insert(at, blob);
        osec.has_blobs = true;
        osec.hdr.p2align = osec.hdr.p2align.max(p2align);
        ctx.isecs[blob as usize]
            .set_output_section(ChunkId::Output(OutputSectionId::new(pos as u32)));
    }
}

/// The synthesized Objective-C records not placed among the inputs go
/// in the tail of the section they name.
fn place_tail_blobs<E: Target>(ctx: &mut Context<E>) {
    let unplaced =
        |ctx: &Context<E>, b: &DataBlob| ctx.isecs[b.isec as usize].output_section().is_none();
    let mut sects: Vec<&'static str> =
        ctx.data_blobs.iter().filter(|b| unplaced(ctx, b)).map(|b| b.sect).collect();
    sects.sort();
    sects.dedup();
    for sect in sects {
        let map = SectionMap::final_link(ctx);
        let ((seg, out), (flags_seg, flags_sect)) =
            output_section_for(&ctx.args, map, "__DATA", sect, 0).unwrap();
        let flags = output_section_flags(flags_seg, flags_sect, 0, false);
        let mut size = 0u64;
        let mut offs = Vec::new();
        for b in ctx.data_blobs.iter().filter(|b| b.sect == sect && unplaced(ctx, b)) {
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

/// Creates output section chunks and appends each input section to its
/// chunk, and groups chunks into segments.
pub fn create_output_sections<E: Target>(ctx: &mut Context<E>) {
    ctx.chunks.push(ChunkId::MachHeader);

    // Assign each input section to an output section, creating output
    // sections as needed. Keyed by the raw 16-byte name pairs, so the
    // hot loop does no allocation and no linear scans; chunks are
    // still created in first-encounter order.
    let relocatable = ctx.args.relocatable;
    let map = SectionMap::new(ctx);
    let text = text_section_name(ctx);
    // Each input section name's output section - by its flags too,
    // which say whether -text_exec moves it and whether it is the
    // standard section of its name (see is_standard_section).
    let mut by_name: hashbrown::HashMap<([u8; 16], [u8; 16], u32), Option<OutputSectionId>> =
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
    // Whether each output section has zero-fill (bit 0) and
    // file-backed (bit 1) input sections; renames can mix them.
    let mut fill_kinds: Vec<u8> = Vec::new();
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
            let key = (hdr.segname, hdr.sectname, hdr.flags);
            let id = match by_name.get(&key) {
                Some(&id) => id,
                None => {
                    let (seg, sect) = (hdr.segname(), hdr.sectname());
                    let out = output_section_for(&ctx.args, map, seg, sect, hdr.flags);
                    let id = out.map(|(out, flags_name)| match by_out.get(&out) {
                        Some(&id) => id,
                        None => {
                            // The first member decides the flags, as in
                            // ld-prime: code after data in a section
                            // doesn't make it code. An empty member
                            // counts if it names an atom (see
                            // bare_sections), in -r too.
                            let mut osec = OutputSection::new(out.0, out.1);
                            osec.hdr.flags = if !relocatable && out == text {
                                // ld-prime makes a final image's __text
                                // itself, as code, whatever its members
                                // (and under its -rename_section name).
                                S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS
                            } else {
                                let input = input_section_flags(seg, sect, hdr.flags);
                                output_section_flags(flags_name.0, flags_name.1, input, relocatable)
                            };
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
            if let Some(id) = id {
                if fill_kinds.len() <= id.index() {
                    fill_kinds.resize(id.index() + 1, 0);
                }
                fill_kinds[id.index()] |= if hdr.is_zerofill() { 1 } else { 2 };
            }
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
        osec.members.push(i as u32);
        ctx.isecs[i].set_output_section(ChunkId::Output(osec_id));
    }
    if !relocatable {
        resolve_zerofill_conflicts(ctx, &fill_kinds);
    }
    place_replacing_blobs(ctx);

    // A final image always has a __text section, empty if no code
    // reached it (a dylib of only data; ld-prime writes one of size 0,
    // byte-aligned).
    if !relocatable && !by_out.contains_key(&text) {
        let mut osec = OutputSection::new(text.0, text.1);
        osec.hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        let id = OutputSectionId::new(ctx.output_sections.len() as u32);
        ctx.output_sections.push(osec);
        ctx.chunks.push(ChunkId::Output(id));
        by_out.insert(text, id);
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

    // -sectalign sets an output section's alignment, e.g. to
    // page-align a blob that will be mapped or measured separately.
    // ld64 lowers it too, with a warning: its members keep their
    // offsets within the section, so one may end up misaligned (a
    // fixup that can't reach it then fails).
    for (seg, sect, p2align) in &ctx.args.sectalign {
        let p2align = *p2align as u32;
        for osec in &mut ctx.output_sections {
            if osec.hdr.segname == *seg && osec.hdr.sectname == *sect {
                if p2align < osec.hdr.p2align {
                    crate::warn!(
                        "-sectalign reduces alignment of {seg},{sect} from {} to {}",
                        1u64 << osec.hdr.p2align,
                        1u64 << p2align
                    );
                }
                osec.hdr.p2align = p2align;
            }
        }
    }

    // A section cannot be aligned beyond the segment's page: ld64
    // reduces the alignment with a warning (an x86-64 .align 16 asks
    // for 64KB). Not in a -static or -preload image, which no dyld
    // maps: ld-prime starts the section's segment on the alignment
    // there (see lay_out_segments).
    if !relocatable && !ctx.args.static_link {
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

    add_stub_and_got_chunks(ctx);

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
        let methname_size = ctx.objc_stubs.methname_data.len() as u64;
        let selrefs_size =
            (ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len()) as u64 * 8;
        let map = SectionMap::final_link(ctx);
        if methname_size > 0 {
            let ((seg, sect), _) =
                output_section_for(&ctx.args, map, "__TEXT", "__objc_methname", S_CSTRING_LITERALS)
                    .unwrap();
            let id = tail_section(
                ctx,
                seg,
                sect,
                S_CSTRING_LITERALS,
                0,
                Tail::ObjcMethname,
                methname_size,
            );
            ctx.objc_stubs.methname = Some(id);
        }
        if selrefs_size > 0 {
            let ((seg, sect), (flags_seg, flags_sect)) =
                output_section_for(&ctx.args, map, "__DATA", "__objc_selrefs", S_LITERAL_POINTERS)
                    .unwrap();
            // A slot keeps the alignment of the inputs it took over.
            let p2align = (ctx.objc_stubs.absorbed.iter())
                .map(|&(synth, _)| ctx.isecs[synth as usize].p2align as u32)
                .fold(3, u32::max);
            let id = tail_section(
                ctx,
                seg,
                sect,
                output_section_flags(flags_seg, flags_sect, S_LITERAL_POINTERS, false),
                p2align,
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
        place_tail_blobs(ctx);
    }

    if !ctx.objc_methlist.lists.is_empty() {
        // ld64 lays the lists out sorted by their symbol's name, each
        // 8-byte aligned; category merging also retires some after
        // their first placement.
        let mut name_of: hashbrown::HashMap<u32, &'static str> = hashbrown::HashMap::new();
        let syms =
            ctx.symbols.syms.iter().filter_map(|sym| Some((sym.name(), sym.input_section()?)));
        // (The lists category merging builds are named as extra locals.)
        for (name, isec) in syms.chain(ctx.extra_local_syms.iter().copied()) {
            let r = ctx.resolve_isec(isec as usize) as u32;
            let e = name_of.entry(r).or_insert(name);
            if name < *e {
                *e = name;
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

    if ctx.args.unwind_info() && chunks::unwind_info::is_needed(ctx) {
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

    add_linkedit_chunks(ctx);
    rename_synthetic_sections(ctx);
    add_boundary_sections(ctx);

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
        // Code in __TEXT_EXEC follows __TEXT. The __DATA_CONST of an
        // image no dyld loads (a -static one with -data_const or in the
        // shared region, a kext) comes after __DATA, as ld-prime
        // places it.
        let standard = match hdr.segname {
            "__TEXT" | "__TEXT_EXEC" => 0,
            "__DATA_CONST" if !ctx.args.without_dyld() => 1,
            "__DATA" => 2,
            "__DATA_CONST" => 3,
            _ => 4,
        };
        // -segment_order orders the rest: __TEXT, which holds the
        // mach header, stays first and __LINKEDIT last. A -preload
        // image's header precedes its segments but lies in none, and
        // its __TEXT goes where the list says.
        let seg_rank = match (id, hdr.segname) {
            (ChunkId::MachHeader, _) if ctx.args.preload => 0,
            (_, "__TEXT") if !ctx.args.preload => 0,
            (_, "__LINKEDIT") => usize::MAX,
            (_, name) => match segment_order.iter().position(|s| s == name) {
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
            _ => shared_region_text_rank(ctx, hdr)
                .unwrap_or_else(|| 1 + output_section_rank(hdr.segname, &hdr.sectname, hdr.flags)),
        };
        let seen = match id {
            ChunkId::Output(osec) => section_first_seen[osec.index()],
            _ => u64::MAX,
        };
        // Zero-fill sections go last in their segment so that they don't
        // occupy file space in the middle of it.
        (seg_rank, hdr.is_zerofill(), listed_section_rank(ctx, hdr), sect_rank, seen)
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
        if id == ChunkId::MachHeader && ctx.args.preload {
            continue;
        }
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
    add_boundary_segments(ctx);
    add_stack_segment(ctx);
    crate::chunks::indirect_symtab::assign_indices(ctx);
    check_segment_order(ctx);
    check_section_order(ctx);
    check_interposing(ctx);
    // The mach header's segment must come first after __PAGEZERO. A
    // -static image's header moves with -rename_segment __TEXT, and
    // only -segment_order can then put its segment there.
    if ctx.chunks.first() != Some(&ChunkId::MachHeader) {
        fatal!("Invalid -segment_order, __TEXT must be the first segment after zero page");
    }
    if ctx.args.no_zero_fill_sections && !ctx.args.relocatable {
        fill_zero_fill_sections(ctx);
    }
}

/// -no_zero_fill_sections gives every zero-fill section its bytes in
/// the file, for a loader that copies segments from the file without
/// zero-filling them (the x86-64 XNU kernel's booter): ld-prime makes
/// such a section regular, once it has taken its place at the end of
/// its segment. A thread-local one becomes S_THREAD_LOCAL_REGULAR, not
/// S_REGULAR as in ld-prime (which ld64 left thread-local): dyld finds
/// the thread-local template by those two types.
fn fill_zero_fill_sections<E: Target>(ctx: &mut Context<E>) {
    for osec in &mut ctx.output_sections {
        let hdr = &mut osec.hdr;
        let regular = match hdr.flags & SECTION_TYPE {
            S_ZEROFILL => S_REGULAR,
            S_THREAD_LOCAL_ZEROFILL => S_THREAD_LOCAL_REGULAR,
            _ => continue,
        };
        hdr.flags = (hdr.flags & !SECTION_TYPE) | regular;
    }
}

/// Where -section_order puts a section in its segment: the listed
/// sections lead in the list's order, after the mach header and after
/// __text unless the list places it; the rest follow as usual.
fn listed_section_rank<E: Target>(ctx: &Context<E>, hdr: &ChunkHeader) -> usize {
    let Some((_, list)) = ctx.args.section_order.iter().find(|(seg, _)| seg == hdr.segname) else {
        return 0;
    };
    match list.iter().position(|s| *s == hdr.sectname) {
        Some(i) => 1 + i,
        None if !hdr.is_sect || is_text_section(hdr) => 0,
        None => usize::MAX,
    }
}

fn is_text_section(hdr: &ChunkHeader) -> bool {
    hdr.segname == "__TEXT" && hdr.sectname == "__text"
}

/// ld-prime refuses a -section_order that puts a zero-fill section, which
/// has no file bytes, ahead of one with contents: the listed sections
/// lead their segment, so a listed zero-fill section must follow every
/// other section with contents, listed or not.
fn check_section_order<E: Target>(ctx: &Context<E>) {
    for (seg, list) in &ctx.args.section_order {
        let sects: Vec<&ChunkHeader> = ctx
            .chunks
            .iter()
            .map(|&id| ctx.chunk_header(id))
            .filter(|hdr| hdr.is_sect && hdr.segname == seg)
            .collect();
        // ld-prime's order: the listed sections, then the others but an
        // unlisted __text, which leads them all.
        let listed = list.iter().filter_map(|name| sects.iter().find(|hdr| hdr.sectname == *name));
        let others =
            sects.iter().filter(|hdr| !list.contains(&hdr.sectname) && !is_text_section(hdr));
        let order: Vec<&&ChunkHeader> = listed.chain(others).collect();
        if let Some(i) = order.iter().position(|hdr| hdr.is_zerofill())
            && order[i..].iter().any(|hdr| !hdr.is_zerofill())
        {
            fatal!(
                "{} is zero-fill, it should be ordered at the end of the segment {seg}, or alongside other zero-fill sections",
                order[i].sectname
            );
        }
    }
}

/// An image bound for the shared region (see resolve_shared_region)
/// may not carry interposing tuples, which the dyld shared cache
/// builder refuses. ld-prime finds them as dyld does - a section named
/// __interpose in a segment whose name starts with __DATA or __AUTH,
/// by its final name - and rejects even an empty one.
fn check_interposing<E: Target>(ctx: &Context<E>) {
    if !ctx.args.shared_region {
        return;
    }
    let is_interpose = |hdr: &&ChunkHeader| {
        hdr.is_sect
            && hdr.sectname == "__interpose"
            && (hdr.segname.starts_with("__DATA") || hdr.segname.starts_with("__AUTH"))
    };
    if let Some(hdr) = ctx.chunks.iter().map(|&id| ctx.chunk_header(id)).find(is_interpose) {
        error!(
            "Shared cache eligible dylib cannot use interposing tuples (found in '{} {}').  \
             Remove interposing tuples, or opt out of the shared cache using the build setting \
             'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')",
            hdr.segname, hdr.sectname
        );
    }
}

/// The segment of the mach header: __TEXT, which -rename_segment
/// moves only in a -static image. A dynamic image's header stays in
/// __TEXT, where dyld looks for it.
fn header_segment<E: Target>(ctx: &Context<E>) -> &'static str {
    if ctx.args.static_link { renamed_segment(&ctx.args, "__TEXT") } else { "__TEXT" }
}

/// The mach header's address, the start of its segment: where -segaddr
/// pins that segment, or else the image base.
fn mach_header_addr<E: Target>(ctx: &Context<E>) -> u64 {
    ctx.args.segaddr(header_segment(ctx)).unwrap_or(ctx.image_base())
}

/// The name of the __text section a final image always has: it moves
/// with -text_exec like the code, and -rename_section and
/// -rename_segment rename it like any section - but -rename_segment
/// __TEXT leaves it with the mach header.
fn text_section_name<E: Target>(ctx: &Context<E>) -> SectionName {
    let (seg, sect) =
        SectionMap::final_link(ctx).builtin_name(("__TEXT", "__text"), S_ATTR_PURE_INSTRUCTIONS);
    let is_renamed = ctx.args.rename_sections.iter().any(|(s, t, _, _)| s == seg && t == sect);
    if seg == "__TEXT" && !is_renamed {
        (header_segment(ctx), sect)
    } else {
        renamed(&ctx.args, (seg, sect))
    }
}

/// Applies -rename_section and -rename_segment to the sections the
/// linker synthesizes, as ld-prime does to all of them - the stubs and
/// their helper, the GOT and lazy pointers, __init_offsets,
/// __eh_frame, the Objective-C ones and -sectcreate's - but
/// __unwind_info, which stays in __TEXT; and moves the mach header to
/// its segment. (The output sections of input sections got their
/// renamed names when created.) A -sectcreate __DATA,__interpose moves
/// to __DATA_CONST like an input section (see SectionMap::renamed).
fn rename_synthetic_sections<E: Target>(ctx: &mut Context<E>) {
    let map = SectionMap::final_link(ctx);
    if ctx.args.rename_sections.is_empty()
        && ctx.args.rename_segments.is_empty()
        && !map.const_interpose
    {
        return;
    }
    ctx.mach_header.hdr.segname = header_segment(ctx);
    for i in 0..ctx.chunks.len() {
        let id = ctx.chunks[i];
        let hdr = ctx.chunk_header(id);
        if !hdr.is_sect || matches!(id, ChunkId::Output(_) | ChunkId::UnwindInfo) {
            continue;
        }
        let (seg, sect) = map.renamed(&ctx.args, (hdr.segname, static_name(&hdr.sectname)));
        let hdr = ctx.chunk_header_mut(id);
        hdr.segname = seg;
        hdr.sectname = sect.to_string();
    }
}

/// Resolves each section$start$/section$end$ and segment$start$/
/// segment$end$ symbol to the output section or segment it names, and
/// creates the sections nothing else does. ld-prime renames the name
/// as it does an input section's - __DATA,__const becomes
/// __DATA_CONST,__const, and -rename_section and -rename_segment
/// apply - or as its own section's (see SectionMap::boundary_name),
/// but merges and drops nothing: section$start$__TEXT$__literal8
/// names an empty __literal8 of its own.
fn add_boundary_sections<E: Target>(ctx: &mut Context<E>) {
    let map = SectionMap::final_link(ctx);
    for i in 0..ctx.boundary_syms.len() {
        let (_, _, seg, sect) = &ctx.boundary_syms[i];
        let Some(sect) = sect else {
            let seg = renamed_segment(&ctx.args, static_name(seg));
            ctx.boundary_syms[i].2 = seg.to_string();
            continue;
        };
        let flags = boundary_section_flags(seg, sect);
        let name = map.boundary_name((static_name(seg), static_name(sect)));
        let (seg, sect) = map.renamed(&ctx.args, name);
        ctx.boundary_syms[i].2 = seg.to_string();
        ctx.boundary_syms[i].3 = Some(sect.to_string());
        if !ctx.chunks.iter().any(|&id| {
            let hdr = ctx.chunk_header(id);
            hdr.is_sect && hdr.segname == seg && hdr.sectname == sect
        }) {
            let mut sec = SectCreateSection::new(seg, sect, &[], false);
            sec.hdr.flags = flags;
            add_sectcreate(ctx, sec);
        }
    }
}

/// Gives a segment$start$ or segment$end$ symbol naming a segment the
/// image lacks - no input has one, or -rename_section emptied it - an
/// empty segment (no sections, vmsize 0) to point at, as ld-prime
/// does: just before __LINKEDIT and at its address, in the order of
/// the symbols' names.
fn add_boundary_segments<E: Target>(ctx: &mut Context<E>) {
    let mut syms: Vec<(&str, &str)> = ctx
        .boundary_syms
        .iter()
        .filter(|(_, _, seg, sect)| sect.is_none() && !ctx.segments.iter().any(|s| s.name == seg))
        .map(|(id, _, seg, _)| (ctx.symbols[*id].name(), seg.as_str()))
        .collect();
    syms.sort_unstable();
    let mut missing: Vec<&'static str> = Vec::new();
    for (_, seg) in syms {
        if !missing.contains(&seg) {
            missing.push(static_name(seg));
        }
    }
    let linkedit = ctx.segments.len() - 1;
    ctx.segments.splice(linkedit..linkedit, missing.into_iter().map(OutputSegment::new));
}

/// A static executable's -stack_size stack: a segment of address space
/// alone before __LINKEDIT, pinned where resolve_stack_size says.
fn add_stack_segment<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.static_link && ctx.args.stack_size != 0 {
        let linkedit = ctx.segments.len() - 1;
        ctx.segments.insert(linkedit, OutputSegment::new("__UNIXSTACK"));
    }
}

/// The flags ld-prime gives a section only a section$start$ or
/// section$end$ symbol makes, by the name the symbol gives (before any
/// move or rename): a standard section's, though the initializer and
/// terminator lists are plain data then, and none for another name.
fn boundary_section_flags(segname: &str, sectname: &str) -> u32 {
    match (segname, sectname) {
        ("__DATA", "__mod_init_func" | "__mod_term_func") => S_REGULAR,
        _ => standard_section_flags(segname, sectname).unwrap_or(S_REGULAR),
    }
}

/// Whether ld-prime places an input section of a standard name (see
/// standard_section_flags) as that standard section: one of the
/// table's type, or of any type for the Objective-C runtime's sections
/// and __got, which it knows by name. Only such a section moves to
/// __DATA_CONST or merges into another section in a final image; any
/// other, such as a __mod_init_func or __literal8 assembled without
/// its type, stays where data of its name goes.
fn is_standard_section(segname: &str, sectname: &str, flags: u32) -> bool {
    let Some(table) = standard_section_flags(segname, sectname) else {
        return false;
    };
    table & SECTION_TYPE == flags & SECTION_TYPE
        || sectname.starts_with("__objc_")
        || (segname, sectname) == ("__DATA", "__got")
}

/// The flags ld-prime reads an input section as having: those its
/// table holds for the section's name (see standard_section_flags) if
/// the section has the table's type, or has any type and one of the
/// Objective-C runtime's names - a __TEXT,__const or __DATA,__data an
/// assembler nop landed in is plain data again, a regular __text code,
/// a regular __objc_methname C strings - and its own otherwise (a
/// regular __cstring holds no literals to merge). __objc_selrefs keeps
/// its own type, and so does __got (though ld-prime makes a regular
/// one's slots entries of its GOT, named in the indirect symbol table:
/// a -r output of it then has non-lazy pointers ld-prime refuses as
/// input).
fn input_section_flags(segname: &str, sectname: &str, flags: u32) -> u32 {
    match standard_section_flags(segname, sectname) {
        Some(table)
            if table & SECTION_TYPE == flags & SECTION_TYPE
                || (sectname.starts_with("__objc_") && sectname != "__objc_selrefs") =>
        {
            table
        }
        _ => flags,
    }
}

/// The flags of a section ld-prime's table of standard sections names:
/// those a compiler marks a section of that name with, or ld-prime its
/// own sections - code (the stubs and helpers too), literals, pointer
/// lists, the thread-local and zero-fill types, no-dead-strip for the
/// lists the Objective-C runtime scans, and none for the rest of the
/// data. None for another name, or in another segment.
fn standard_section_flags(segname: &str, sectname: &str) -> Option<u32> {
    let flags = match (segname, sectname) {
        (
            "__TEXT",
            "__text" | "__StaticInit" | "__stub_helper" | "__objc_stubs" | "__objc_clsstubs"
            | "__delay_stubs" | "__delay_helper" | "__lazy_helpers" | "__resolver_help",
        ) => S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS,
        (
            "__TEXT",
            "__cstring" | "__objc_classname" | "__objc_methname" | "__objc_methtype"
            | "__oslogstring",
        ) => S_CSTRING_LITERALS,
        ("__TEXT", "__literal4") => S_4BYTE_LITERALS,
        ("__TEXT", "__literal8") => S_8BYTE_LITERALS,
        ("__TEXT", "__literal16") => S_16BYTE_LITERALS,
        ("__TEXT", "__eh_frame") => output_section_flags(segname, sectname, 0, false),
        ("__TEXT", "__const" | "__ustring" | "__gcc_except_tab" | "__objc_methlist") => S_REGULAR,
        ("__DATA", "__got" | "__auth_got" | "__weak_got" | "__weak_auth_got") => {
            S_NON_LAZY_SYMBOL_POINTERS
        }
        ("__DATA", "__la_symbol_ptr" | "__la_resolver") => S_LAZY_SYMBOL_POINTERS,
        ("__DATA", "__mod_init_func") => S_MOD_INIT_FUNC_POINTERS,
        ("__DATA", "__mod_term_func") => S_MOD_TERM_FUNC_POINTERS,
        (
            "__DATA",
            "__objc_classlist" | "__objc_nlclslist" | "__objc_catlist" | "__objc_catlist2"
            | "__objc_nlcatlist" | "__objc_classrefs" | "__objc_superrefs" | "__objc_clsrolist",
        ) => S_ATTR_NO_DEAD_STRIP,
        ("__DATA", "__objc_protolist") => S_COALESCED,
        ("__DATA", "__objc_protorefs") => S_COALESCED | S_ATTR_NO_DEAD_STRIP,
        ("__DATA", "__objc_selrefs") => S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP,
        ("__DATA", "__thread_vars") => S_THREAD_LOCAL_VARIABLES,
        ("__DATA", "__thread_ptrs") => S_THREAD_LOCAL_VARIABLE_POINTERS,
        ("__DATA", "__thread_data") => S_THREAD_LOCAL_REGULAR,
        ("__DATA", "__thread_bss") => S_THREAD_LOCAL_ZEROFILL,
        ("__DATA", "__bss" | "__common") => S_ZEROFILL,
        (
            "__DATA",
            "__data" | "__const" | "__cfstring" | "__auth_ptr" | "__objc_data" | "__objc_const"
            | "__objc_ivar" | "__objc_imageinfo" | "__objc_intobj" | "__objc_floatobj"
            | "__objc_doubleobj" | "__objc_dateobj" | "__objc_dictobj" | "__objc_arrayobj"
            | "__objc_arraydata" | "__const_cfobj2",
        ) => S_REGULAR,
        // The compiler's records for the linker to encode into
        // __unwind_info, which no output carries (but a boundary
        // symbol's empty section).
        ("__LD", "__compact_unwind") => S_ATTR_DEBUG,
        _ => return None,
    };
    Some(flags)
}

/// Sizes the stubs, the lazy-binding helper and pointers, and the GOT
/// (with __weak_got split off, see GotSection), and adds the ones in
/// use to the output.
fn add_stub_and_got_chunks<E: Target>(ctx: &mut Context<E>) {
    if !ctx.stubs.symbols.is_empty() {
        if ctx.args.text_exec {
            ctx.stubs.hdr.segname = "__TEXT_EXEC";
        }
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
        // In the shared region, dyld binds them all at load, and the
        // section joins the read-only data.
        if ctx.args.shared_region {
            ctx.lazy_ptrs.hdr.segname = data_seg(ctx);
        }
        ctx.lazy_ptrs.hdr.size = ctx.stubs.lazy.len() as u64 * 8;
        ctx.chunks.push(ChunkId::LazyPtrs);
    }

    let got = &mut ctx.got;
    let weak = got.got_syms.len() - got.weak_start;
    got.hdr.size = got.weak_start as u64 * 8;
    got.weak_hdr.size = weak as u64 * 8;
    let seg = data_seg(ctx);
    // A kext's are plain data to ld-prime (indexed into the indirect
    // symbol table all the same).
    let flags = if ctx.args.is_kext() { S_REGULAR } else { S_NON_LAZY_SYMBOL_POINTERS };
    for (id, len) in [(ChunkId::Got, ctx.got.weak_start), (ChunkId::WeakGot, weak)] {
        if len > 0 {
            let hdr = ctx.chunk_header_mut(id);
            hdr.segname = seg;
            hdr.flags = flags;
            ctx.chunks.push(id);
        }
    }
    for i in 0..ctx.got.objc_classref_slots.len() {
        let (slot, class) = ctx.got.objc_classref_slots[i];
        let (chunk, off) = ctx.got.slot_place(ctx.sym_aux(class).got_idx as usize);
        ctx.isecs[slot as usize].offset = off as u32;
        ctx.isecs[slot as usize].set_output_section(chunk);
    }
}

/// Adds the __LINKEDIT tables, in ld-prime's order.
fn add_linkedit_chunks<E: Target>(ctx: &mut Context<E>) {
    // What dyld reads. A -static image or a kext has no dyld: a -static
    // one has only the fixups -fixup_chains or -no_fixup_chains asks
    // for (chains, or rebase and weak-bind opcodes, never an export
    // trie), or under -pie local relocations to slide by; a kext has
    // its relocations, by which kmutil links it.
    if !ctx.args.without_dyld() {
        ctx.chunks.push(ChunkId::ChainedFixups);
        ctx.chunks.push(ChunkId::RebaseInfo);
        ctx.chunks.push(ChunkId::BindInfo);
        ctx.chunks.push(ChunkId::WeakBindInfo);
        ctx.chunks.push(ChunkId::LazyBindInfo);
        ctx.chunks.push(ChunkId::ExportTrie);
    } else if ctx.use_chained_fixups() {
        ctx.chunks.push(ChunkId::ChainedFixups);
    } else if ctx.args.fixup_chains == Some(false) {
        ctx.chunks.push(ChunkId::RebaseInfo);
        ctx.chunks.push(ChunkId::WeakBindInfo);
    } else if ctx.args.pie || ctx.args.is_kext() {
        ctx.chunks.push(ChunkId::LocalRelocs);
    }
    if ctx.args.shared_region {
        ctx.chunks.push(ChunkId::SplitInfo);
    }
    ctx.chunks.push(ChunkId::FunctionStarts);
    if ctx.args.data_in_code_info {
        ctx.chunks.push(ChunkId::DataInCode);
    }
    ctx.chunks.push(ChunkId::Symtab);
    if ctx.args.is_kext() {
        ctx.chunks.push(ChunkId::ExternRelocs);
    }
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
}

/// Collects the relocations of an image no dyld loads (LC_DYSYMTAB's):
/// a -static -pie image's local ones, and a kext's local and external
/// ones.
fn collect_relocations<E: Target>(ctx: &mut Context<E>) {
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        ctx.local_relocs.locs = chunks::local_relocs::build(ctx);
        ctx.local_relocs.hdr.size = (ctx.local_relocs.locs.len() * size_of::<MachRel>()) as u64;
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) {
        ctx.extern_relocs.relocs = chunks::extern_relocs::build(ctx);
        ctx.extern_relocs.hdr.size = (ctx.extern_relocs.relocs.len() * size_of::<MachRel>()) as u64;
    }
}

/// Settles each output section that renames fill with both zero-fill
/// and file-backed input sections - bits 0 and 1 of `fill_kinds`, by
/// output section - as ld-prime does, with a warning. The section
/// takes the type of its first member in ld-prime's order: the
/// objects' in input order, a common symbol's __common after its
/// object's own sections. A zero-fill section drops the contents of
/// the others.
fn resolve_zerofill_conflicts<E: Target>(ctx: &mut Context<E>, fill_kinds: &[u8]) {
    if !fill_kinds.contains(&3) {
        return;
    }
    let owners = common_owners(ctx);
    for (i, &kinds) in fill_kinds.iter().enumerate() {
        if kinds == 3 {
            resolve_zerofill_conflict(ctx, OutputSectionId::new(i as u32), &owners);
        }
    }
}

fn resolve_zerofill_conflict<E: Target>(
    ctx: &mut Context<E>,
    id: OutputSectionId,
    common_owners: &hashbrown::HashMap<u32, u32>,
) {
    // The first file with a zero-fill member, whether that is a
    // common symbol, and the files with file-backed ones. A common
    // symbol's subsection, which mold makes in its internal object,
    // counts as its owner's; the others mold makes come from no file.
    let mut defined_in: Option<(u32, bool)> = None;
    let mut missing_in = Vec::new();
    for &member in &ctx.output_section(id).members {
        let file = ctx.isecs[member as usize].file;
        let file = if !ctx.is_internal(file as usize) {
            (file, false)
        } else if let Some(&owner) = common_owners.get(&member) {
            (owner, true)
        } else {
            continue;
        };
        if ctx.hdr_of(&ctx.isecs[member as usize]).is_zerofill() {
            defined_in = Some(defined_in.map_or(file, |first| first.min(file)));
        } else {
            missing_in.push(file.0);
        }
    }
    let Some((defined_in, is_common)) = defined_in else {
        return;
    };
    // mold makes common symbols' subsections last; one that comes
    // first to ld-prime makes the section zero-fill.
    if is_common && missing_in.iter().all(|&file| defined_in < file) {
        let hdr = &mut ctx.output_section_mut(id).hdr;
        hdr.flags = (hdr.flags & !SECTION_TYPE) | S_ZEROFILL;
    }
    missing_in.sort_unstable_by(|a, b| b.cmp(a));
    missing_in.dedup();
    let osec = ctx.output_section(id);
    let name = |file: u32| resolved_file_name(ctx.objs[file as usize].mf);
    let mut msg = format!(
        "section {},{} has a conflicting zerofill flag defined in {} but missing in:",
        osec.hdr.segname,
        osec.hdr.sectname,
        name(defined_in)
    );
    for file in missing_in {
        msg += &format!("\n  {}", name(file));
    }
    crate::warn!("{msg}");
}

/// The object each common symbol's subsection stands for the tentative
/// definition of, by subsection: the one declaring the largest size,
/// the first of equals, as in ld-prime.
pub(crate) fn common_owners<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<u32, u32> {
    let mut decls: hashbrown::HashMap<crate::symbol::SymbolId, (u64, u32)> =
        hashbrown::HashMap::new();
    for (i, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        let r = obj.global_range();
        for (nlist, &sym) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab() && nlist.n_type() == N_UNDF && nlist.is_common() {
                let decl = decls.entry(sym).or_insert((nlist.n_value, i as u32));
                if nlist.n_value > decl.0 {
                    *decl = (nlist.n_value, i as u32);
                }
            }
        }
    }
    decls
        .into_iter()
        .filter_map(|(sym, (_, obj))| Some((ctx.symbols[sym].input_section()?, obj)))
        .collect()
}

/// ld-prime's warnings for a -segment_order that places __TEXT or
/// __LINKEDIT where they cannot go, or leaves segments out (they follow
/// the listed ones in the usual order). The __TEXT of a -preload image
/// holds no mach header, and is ordered like any other segment.
fn check_segment_order<E: Target>(ctx: &Context<E>) {
    let order = &ctx.args.segment_order;
    if order.is_empty() {
        return;
    }
    let (text_pos, text_place) =
        if ctx.args.pagezero_size > 0 { (1, "second") } else { (0, "first") };
    let has_text = !ctx.args.preload && ctx.segments.iter().any(|s| s.name == "__TEXT");
    if has_text && order.iter().position(|s| s == "__TEXT").is_some_and(|i| i != text_pos) {
        crate::warn!(
            "-segment_order of __TEXT is ignored, the segment must be ordered {text_place}"
        );
    }
    if order.iter().position(|s| s == "__LINKEDIT").is_some_and(|i| i != order.len() - 1) {
        crate::warn!("-segment_order of __LINKEDIT is ignored, the segment must be ordered last");
    }
    for seg in &ctx.segments {
        let fixed = match seg.name {
            "__PAGEZERO" | "__LINKEDIT" => true,
            "__TEXT" => !ctx.args.preload,
            _ => false,
        };
        if !fixed && !order.iter().any(|s| s == seg.name) {
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
/// sections stays (clang's __OBJC_LABEL_PROTOCOL_$_X does), as does one
/// an earlier ld -r demoted, a local that kept N_PEXT (`demoted`).
fn keep_local_symbol_in<E: Target>(
    ctx: &Context<E>,
    name: &str,
    isec: Option<u32>,
    demoted: bool,
) -> bool {
    if !keep_local_symbol(name) {
        return false;
    }
    if demoted {
        return true;
    }
    match isec {
        Some(isec) => {
            let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
            if ctx.is_internal(isec.file as usize) {
                return true;
            }
            let hdr = ctx.hdr_of(isec);
            !(hdr.sectname.starts_with(b"__objc_")
                && [
                    "__objc_classlist",
                    "__objc_nlclslist",
                    "__objc_catlist",
                    "__objc_catlist2",
                    "__objc_nlcatlist",
                    "__objc_protolist",
                    "__objc_superrefs",
                    "__objc_protorefs",
                    "__objc_imageinfo",
                ]
                .iter()
                .any(|name| hdr.sectname_is(name))
                || has_unnamed_atoms(hdr))
        }
        None => true,
    }
}

/// Whether ld-prime makes a section's atoms by content and names none
/// of them: CFStrings, selector and class references, UTF-16 literals
/// and Objective-C constant literals (@42, @[...], @{...}). No label
/// of theirs is in an output's symbol table; a -r output names the
/// atoms itself on arm64 (see relocatable.rs).
pub(crate) fn has_unnamed_atoms(hdr: &MachSection) -> bool {
    if hdr.segname_is("__TEXT") {
        return hdr.sectname_is("__ustring");
    }
    hdr.segname_is("__DATA")
        && [
            "__cfstring",
            "__objc_selrefs",
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
        .any(|name| hdr.sectname_is(name))
}

/// One stab entry: its name and nlist, the symbol whose final address
/// fills in n_value, and the symbol the name is, if any - ld-prime
/// points the entry at that symbol's own string.
#[derive(Clone, Copy, Debug)]
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

    /// The string of the symbol the entry names, if it shares it.
    fn shared_strx(&self, strx_of: &[u32]) -> Option<u32> {
        let strx = strx_of[self.name_of? as usize];
        (strx != u32::MAX).then_some(strx)
    }
}

/// An object's planned stab entries: those written as they are (the
/// N_SO pair and N_OSO that open a run of its own, or every note a -r
/// input carries), then its symbols' notes and the N_SO closing its
/// run. A symbol's notes are kept as the symbol until written: they
/// are most of a large -g link's symbol table.
#[derive(Debug, Default)]
pub struct StabPlan {
    fixed: Vec<Stab>,
    syms: Vec<SymbolStabs>,
    closed: bool,
    len: usize,
}

impl StabPlan {
    pub fn len(&self) -> usize {
        self.len
    }

    /// The planned entries, in order.
    pub fn stabs<'a, E: Target>(&'a self, ctx: &'a Context<E>) -> impl Iterator<Item = Stab> + 'a {
        let close = self.closed.then_some(Stab::new(b"", STAB_END, None));
        self.fixed.iter().copied().chain(self.syms.iter().flat_map(|s| s.stabs(ctx))).chain(close)
    }

    /// The bytes of string table the entries' own names take: each named
    /// entry's but those that share the string of the symbol they name
    /// (`strx_of`) - a symbol's notes name it once.
    pub fn strtab_size<E: Target>(&self, ctx: &Context<E>, strx_of: &[u32]) -> usize {
        let fixed =
            self.fixed.iter().filter(|s| !s.name.is_empty() && s.shared_strx(strx_of).is_none());
        let syms = self.syms.iter().filter(|s| strx_of[s.sym as usize] == u32::MAX);
        fixed.map(|s| s.name.len() + 1).sum::<usize>()
            + syms.map(|s| ctx.symbols[s.sym].name().len() + 1).sum::<usize>()
    }

    /// Writes the entries, with their final addresses and their own names,
    /// into the object's block of the symbol table - mold-rust's
    /// populate_symtab.
    pub fn populate_symtab<E: Target>(
        &self,
        ctx: &Context<E>,
        strx_of: &[u32],
        block: &mut crate::chunks::symtab::SymtabBlock<'_>,
    ) {
        // A function's notes take its address three times in a row.
        let mut addr = (u32::MAX, 0);
        for stab in self.stabs(ctx) {
            let mut ent = stab.ent;
            if let Some(id) = stab.value_of {
                if addr.0 != id {
                    addr = (id, ctx.sym_addr(id));
                }
                ent.n_value = addr.1;
            }
            ent.n_strx = match stab.shared_strx(strx_of) {
                Some(strx) => strx,
                None if stab.name.is_empty() => 1,
                None => block.add_string(stab.name),
            };
            block.push(ent);
        }
    }
}

/// A symbol's debug notes: N_BNSYM, the N_FUN pair and N_ENSYM for a
/// function (`size` bytes long), an N_GSYM for global data, an N_STSYM
/// for a local's.
#[derive(Clone, Copy, Debug)]
struct SymbolStabs {
    sym: crate::symbol::SymbolId,
    size: u32,
    n_sect: u8,
    n_type: u8,
}

impl SymbolStabs {
    fn len(&self) -> usize {
        if self.n_type == N_FUN { 4 } else { 1 }
    }

    fn stabs<E: Target>(&self, ctx: &Context<E>) -> impl Iterator<Item = Stab> {
        let id = Some(self.sym);
        let name = ctx.symbols[self.sym].name().as_bytes();
        let sect = self.n_sect;
        // Named entries get their string offsets later; the rest keep 1,
        // the empty string.
        let stab = |n_type, n_sect| NList { n_strx: 1, n_type, n_sect, ..Default::default() };
        let mut out = [Stab::new(b"", stab(N_BNSYM, sect), id); 4];
        match self.n_type {
            N_FUN => {
                // ld64's shape: N_BNSYM, the N_FUN pair (the function's
                // address, then its size), N_ENSYM. Its stab reader takes
                // an N_FUN without the bracketing symbols badly (a crash
                // on a -r output that had only the pair).
                let fun = NList { n_strx: 0, ..stab(N_FUN, sect) };
                out[1] = Stab { name_of: id, ..Stab::new(name, fun, id) };
                out[2] =
                    Stab::new(b"", NList { n_value: self.size as u64, ..stab(N_FUN, 0) }, None);
                out[3] = Stab::new(b"", stab(N_ENSYM, sect), id);
            }
            // An N_GSYM names the global only, with no section or
            // address - the debugger looks the address up by name.
            N_GSYM => {
                let ent = NList { n_type: N_GSYM, ..Default::default() };
                out[0] = Stab { name, ent, value_of: None, name_of: id };
            }
            _ => {
                let ent = NList { n_strx: 0, ..stab(N_STSYM, sect) };
                out[0] = Stab { name_of: id, ..Stab::new(name, ent, id) };
            }
        }
        out.into_iter().take(self.len())
    }
}

/// Plans one object's debug-note stabs. An object with DWARF gets the
/// run ld64 writes: N_SO, N_OSO naming the object, N_FUN pairs and
/// N_GSYM/N_STSYM for its symbols, and a closing N_SO. An object that
/// already carries such a run (a -r output: ld64 does not merge DWARF,
/// it writes these notes) has it copied through, the address-bearing
/// entries rebased to their subsections' output addresses and those of
/// dead subsections dropped. Shared by the final link and -r.
pub fn plan_object_stabs<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    cwd: &Path,
    commons: &hashbrown::HashMap<crate::symbol::SymbolId, usize>,
) -> StabPlan {
    let obj = &ctx.objs[obj_idx];
    let mut plan = StabPlan::default();
    if !obj.is_alive {
        return plan;
    }
    let out = &mut plan.fixed;

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
        let mut in_unit = false;
        for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if !nlist.is_stab() {
                continue;
            }
            let mut ent = *nlist;
            let name = ctx.symbols[sym_id].name();
            // The closing N_SO that opens the input's stabs is not
            // copied: the output has its own.
            if nlist.n_type == N_SO && name.is_empty() {
                if !std::mem::replace(&mut in_unit, false) {
                    continue;
                }
            } else {
                in_unit = true;
            }
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
        plan.len = plan.fixed.len();
        return plan;
    }

    if !obj.has_debug_info {
        return plan;
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

    // The symbols' notes, in symbol-table order. ld-prime lists a
    // unit's notes by address instead, but no reader depends on that:
    // dsymutil and lldb map each unit's notes by name, and an N_FUN pair
    // stays together either way.
    for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
        let sym = &ctx.symbols[sym_id];
        // A tentative definition gets its note in the first object that
        // declares it.
        let common = nlist.is_common() && commons.get(&sym_id) == Some(&obj_idx);
        if nlist.is_stab()
            || (!common && !matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx))
            || (!nlist.is_extern()
                && !keep_local_symbol_in(
                    ctx,
                    sym.name(),
                    sym.input_section(),
                    nlist.n_type & N_PEXT != 0,
                ))
        {
            continue;
        }
        plan.syms.extend(symbol_stabs(ctx, sym_id, nlist.is_extern(), common));
    }
    plan.closed = true;
    plan.len = plan.fixed.len() + plan.syms.iter().map(|s| s.len()).sum::<usize>() + 1;
    plan
}

/// A symbol's debug notes, if it gets any.
fn symbol_stabs<E: Target>(
    ctx: &Context<E>,
    sym_id: crate::symbol::SymbolId,
    is_extern: bool,
    common: bool,
) -> Option<SymbolStabs> {
    let sym = &ctx.symbols[sym_id];
    let global = SymbolStabs { sym: sym_id, size: 0, n_sect: 0, n_type: N_GSYM };
    let Some(isec) = sym.input_section().map(|i| i as usize) else {
        // A -r output keeps a common undefined; it has no address.
        return common.then_some(global);
    };
    let isec = &ctx.isecs[ctx.resolve_isec(isec)];
    // ld-prime notes no exception tables' labels and no ivar offsets.
    let hdr = ctx.hdr_of(isec);
    let text = hdr.segname_is("__TEXT");
    if !isec.is_alive()
        || (text && hdr.sectname_is("__gcc_except_tab"))
        || (hdr.segname_is("__DATA") && hdr.sectname_is("__objc_ivar"))
    {
        return None;
    }
    let n_sect = ctx.isec_n_sect(isec);
    let is_text = text && hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0;
    Some(if is_text {
        SymbolStabs { size: isec.size, n_sect, n_type: N_FUN, ..global }
    } else if is_extern {
        global
    } else {
        SymbolStabs { n_sect, n_type: N_STSYM, ..global }
    })
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
) -> Vec<LocalEnt> {
    type Ent = LocalEnt;
    const PEXT: u8 = 0;
    const LOCAL: u8 = 1;
    const WEAK: u8 = 2;
    let local =
        |n_sect: u8, n_value: u64| NList { n_strx: 0, n_type: N_SECT, n_sect, n_desc: 0, n_value };

    // -non_global_symbols_no_strip_list / _strip_list filter local
    // symbols by name; stabs unaffected.
    let is_listed_out = |name: &[u8]| {
        ctx.args.local_keep_list.as_ref().is_some_and(|keep| keep.find(name) == -1)
            || ctx.args.local_strip_list.find(name) != -1
    };

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
                        || !keep_local_symbol_in(
                            ctx,
                            sym.name(),
                            sym.input_section(),
                            nlist.n_type & N_PEXT != 0,
                        )
                    {
                        continue;
                    }
                    if is_listed_out(sym.name().as_bytes()) {
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
        // The range-extension thunks' entries, named as ld-prime names
        // its branch islands.
        for (addr, n_sect, name) in crate::thunks::island_symbols(ctx) {
            if !is_listed_out(name) {
                ents.push((addr, LOCAL, name, local(n_sect, addr), None));
            }
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
    // external names the atom. Few atoms have aliases, so those are
    // found first, and only their addresses are looked for among the
    // externals.
    let aliased: Vec<usize> = (0..ents.len().saturating_sub(1))
        .into_par_iter()
        .filter(|&i| ents[i].0 == ents[i + 1].0 && (i == 0 || ents[i - 1].0 != ents[i].0))
        .collect();
    if !aliased.is_empty() {
        let addrs: Vec<u64> = aliased.iter().map(|&i| ents[i].0).collect();
        let named: Vec<std::sync::atomic::AtomicBool> =
            addrs.iter().map(|_| std::sync::atomic::AtomicBool::new(false)).collect();
        sorted_globals.par_iter().for_each(|&i| {
            let sym = &ctx.symbols[i];
            if !sym.is_weak_def()
                && sym.input_section().is_some()
                && let Ok(k) = addrs.binary_search(&ctx.sym_addr(i))
            {
                named[k].store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        for (&i, named) in aliased.iter().zip(named) {
            let n = ents[i..].iter().take_while(|e| e.0 == ents[i].0).count();
            if !named.into_inner() {
                ents[i..i + n].rotate_left(1);
            }
        }
    }
    ents
}

/// A local symbol table entry as plan_local_symbols sorts it: address,
/// rank, name, entry, and the symbol whose address fills n_value.
type LocalEnt = (u64, u8, &'static [u8], NList, Option<crate::symbol::SymbolId>);

/// Appends an entry and its name for each item, made by `f` on all cores
/// straight into the arrays' spare capacity, which the caller reserved.
fn par_push_entries<T: Sync>(
    names: &mut Vec<&'static [u8]>,
    entries: &mut Vec<(NList, Option<crate::symbol::SymbolId>)>,
    items: &[T],
    f: impl Fn(&T) -> (&'static [u8], NList, Option<crate::symbol::SymbolId>) + Sync,
) {
    let n = items.len();
    names.spare_capacity_mut()[..n]
        .par_iter_mut()
        .zip(&mut entries.spare_capacity_mut()[..n])
        .zip(items)
        .for_each(|((name, ent), item)| {
            let (n, e, sym) = f(item);
            name.write(n);
            ent.write((e, sym));
        });
    // SAFETY: the n slots past each array's end were written above.
    unsafe {
        names.set_len(names.len() + n);
        entries.set_len(entries.len() + n);
    }
}

/// Builds the output symbol table contents: local symbols in input order,
/// then defined globals and undefined symbols, each sorted by name.
/// Symbol values are filled in when the table is copied out, after
/// addresses are assigned.
pub fn create_output_symtab<E: Target>(
    ctx: &Context<E>,
    sorted_globals: &[crate::symbol::SymbolId],
) -> SymtabSection {
    use std::sync::atomic::{AtomicU32, Ordering};
    let mut data = SymtabSection::new();

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
        (0..classes.len()).into_par_iter().filter(|&i| classes[i] == Class::Pext).collect();
    let t = ctx.timer("symtab-locals");
    let locals = plan_local_symbols(ctx, &pexts, sorted_globals);
    drop(t);

    // Debug stabs. Mach-O binaries don't carry DWARF; instead, for each
    // object with debug info the symbol table gets stab entries telling
    // the debugger where the object file is (N_OSO) and where its
    // functions and globals ended up, and the debugger reads the DWARF
    // from the objects. Each object's run is independent.
    let t = ctx.timer("symtab-stabs");
    let planned: Vec<StabPlan> = if ctx.args.strip_debug {
        Vec::new()
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        let commons = common_stab_owners(ctx);
        ctx.objs
            .par_iter()
            .enumerate()
            .map(|(obj_idx, _)| plan_object_stabs(ctx, obj_idx, &cwd, &commons))
            .collect()
    };
    drop(t);

    let t = ctx.timer("symtab-entries");
    // Undefined (imported) symbols, sorted by name.
    let mut undefs: Vec<usize> =
        (0..classes.len()).into_par_iter().filter(|&i| classes[i] == Class::Undef).collect();
    undefs.par_sort_unstable_by_key(|&i| crate::util::name_sort_key(ctx.symbols[i].name()));

    // Every range's size is known now: the entries and their names are
    // allocated once, and each range is filled in parallel. The names
    // are the strings layout_strings lays out below. The debug notes
    // are not among them: copy_symtab writes them from their plans.
    let nstabs: usize = planned.iter().map(|plan| plan.len()).sum();
    let total = locals.len()
        + ctx.args.add_ast_paths.len()
        + usize::from(nstabs != 0)
        + sorted_globals.len()
        + undefs.len();
    let mut names: Vec<&'static [u8]> = Vec::with_capacity(total);
    data.entries.reserve_exact(total);

    par_push_entries(&mut names, &mut data.entries, &locals, |&(_, _, name, ent, sym)| {
        (name, ent, sym)
    });
    let nplain = data.entries.len();
    drop(locals);

    // Swift AST paths for the debugger (-add_ast_path), as N_AST stabs.
    for path in &ctx.args.add_ast_paths {
        names.push(leak_bytes(path_bytes(path).to_vec()));
        data.entries.push((NList { n_strx: 0, n_type: N_AST, ..Default::default() }, None));
    }

    // ld-prime opens the stabs with a closing N_SO of its own.
    if nstabs != 0 {
        names.push(b"");
        data.entries.push((STAB_END, None));
    }
    let stabs_start = data.entries.len();
    let nlocal = stabs_start + nstabs;
    data.nlocal = nlocal as u32;

    // Defined global symbols, sorted by name; the caller sorted them
    // once for this table and the export trie both.
    par_push_entries(&mut names, &mut data.entries, sorted_globals, |&i| {
        let sym = &ctx.symbols[i];
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
        let ent = NList { n_strx: 0, n_type, n_sect, n_desc, n_value: 0 };
        (sym.name().as_bytes(), ent, Some(i))
    });
    data.nextdef = sorted_globals.len() as u32;

    // The imports. The library ordinal lives in the high byte of n_desc.
    par_push_entries(&mut names, &mut data.entries, &undefs, |&i| {
        let sym = &ctx.symbols[i];
        let Some(FileId::Dylib(dylib)) = sym.file() else { unreachable!() };
        // A dynamic-lookup import records the DYNAMIC_LOOKUP ordinal, a
        // -bundle_loader import the EXECUTABLE ordinal.
        let ordinal = ctx.nlist_library_ordinal(dylib) as u16;
        let mut n_desc = ordinal << 8;
        if sym.is_weak_ref() {
            n_desc |= N_WEAK_REF;
        }
        let ent = NList { n_strx: 0, n_type: N_UNDF | N_EXT, n_sect: 0, n_desc, n_value: 0 };
        (sym.name().as_bytes(), ent, None)
    });
    data.nundef = undefs.len() as u32;
    debug_assert_eq!(data.entries.len(), total);
    drop(t);

    // The string table, in ld-prime's layout: the externals' names, the
    // locals', then each object's notes' in a block of its own. No note
    // is among the entries, so none shares a string there.
    let t = ctx.timer("symtab-strings");
    let strtab_end = crate::chunks::symtab::layout_strings(
        &mut data.entries,
        &mut names,
        stabs_start,
        (0, &[]),
        &[],
    );
    data.names = names;

    // Each symbol's index, for the indirect symbol table, and its string,
    // for the notes naming it - but for the first local's, whose notes
    // ld-prime gives a copy of their own.
    let nsyms = ctx.symbols.syms.len();
    let entry_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let strx_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let nglobals = sorted_globals.len();
    (0..nplain).into_par_iter().chain(stabs_start..stabs_start + nglobals).for_each(|i| {
        if let (ent, Some(id)) = data.entries[i] {
            let index = if i < stabs_start { i } else { i + nstabs };
            entry_of[id as usize].store(index as u32, Ordering::Relaxed);
            if index != 0 {
                strx_of[id as usize].store(ent.n_strx, Ordering::Relaxed);
            }
        }
    });
    undefs.par_iter().enumerate().for_each(|(k, &id)| {
        entry_of[id].store((nlocal + nglobals + k) as u32, Ordering::Relaxed);
    });
    data.output_sym_indices = entry_of.into_iter().map(AtomicU32::into_inner).collect();
    data.strx_of = strx_of.into_iter().map(AtomicU32::into_inner).collect();

    // The notes' strings, each object's after the previous one's.
    let sizes: Vec<usize> =
        planned.par_iter().map(|plan| plan.strtab_size(ctx, &data.strx_of)).collect();
    let mut strx = strtab_end;
    data.stab_strx = Vec::with_capacity(planned.len() + 1);
    for size in sizes {
        data.stab_strx.push(strx as u32);
        strx += size;
    }
    data.stab_strx.push(strx as u32);
    data.strtab_size = strx.next_multiple_of(8);
    data.stabs = planned;
    data.stabs_start = stabs_start;
    data.nstabs = nstabs;

    // An alias of an imported symbol is an N_INDR entry whose n_value
    // is the string-table offset of the name it stands for; that
    // name is in the table already as the import's own entry. The
    // slot is detached from the symbol so copy_symtab leaves n_value
    // alone.
    let entry = |index: u32| {
        let index = index as usize;
        if index < stabs_start { index } else { index - nstabs }
    };
    for &(alias, target) in &ctx.indirect_aliases {
        let a = data.output_sym_indices[alias as usize];
        let t = data.output_sym_indices[target as usize];
        if a == u32::MAX || t == u32::MAX {
            continue;
        }
        let strx = data.entries[entry(t)].0.n_strx;
        let ent = &mut data.entries[entry(a)];
        ent.0.n_type = N_INDR | N_EXT;
        ent.0.n_sect = 0;
        ent.0.n_desc = 0;
        ent.0.n_value = strx as u64;
        ent.1 = None;
    }

    drop(t);

    data
}

/// -pagezero_size, as ld-prime takes it: rounded up to a page (past the
/// top, to 0), and no more than 4 GiB in an executable with chained
/// fixups.
pub fn resolve_pagezero_size<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }
    let size = ctx.args.pagezero_size;
    if !size.is_multiple_of(E::PAGE_SIZE) {
        let aligned = size.wrapping_add(E::PAGE_SIZE - 1) & !(E::PAGE_SIZE - 1);
        // (As printf's %#llx spells it.)
        let shown = if aligned == 0 { "0".to_string() } else { format!("{aligned:#x}") };
        crate::warn!(
            "-pagezero_size not aligned, rounded up to: {shown}, use -segalign to change the alignment"
        );
        ctx.args.pagezero_size = aligned;
    }
    if ctx.args.output_type == MH_EXECUTE
        && ctx.use_chained_fixups()
        && ctx.args.pagezero_size > 0x1_0000_0000
    {
        crate::warn!("-pagezero_size is too large, setting it to 4GB");
        ctx.args.pagezero_size = 0x1_0000_0000;
    }
}

/// ld-prime's checks of the -segaddr pins, one pin at a time: a pin may
/// not lie in __PAGEZERO, share its address with another one, or be off
/// a page boundary - even one for a segment the image does not have.
pub fn check_segaddrs<E: Target>(ctx: &Context<E>) {
    if ctx.args.relocatable {
        return;
    }
    let segaddrs = &ctx.args.segaddrs;
    for (i, (name, addr)) in segaddrs.iter().enumerate() {
        if *addr < ctx.args.pagezero_size {
            fatal!("-segaddr {name} 0x{addr:X} conflicts with -pagezero_size");
        }
        if let Some((other, _)) = segaddrs[i + 1..].iter().find(|(_, a)| a == addr) {
            fatal!("duplicate -segaddr addresses for {name} and {other}");
        }
        if !addr.is_multiple_of(ctx.segment_align()) {
            fatal!(
                "-segaddr {name} 0x{addr:X} is not aligned to the page size ({:#x}), use -segalign to change it",
                ctx.segment_align()
            );
        }
    }
}

/// -image_base (or -seg1addr) sets __TEXT's address, for an image that
/// stays where it was linked. A -segaddr for __TEXT names the same
/// address, and the two must agree (a non-PIE -static image takes the
/// -segaddr's, with a warning). dyld slides a PIE executable wherever
/// it likes, and ld-prime ignores the base for one with a warning; it
/// ignores it too for any other image dyld loads with chained fixups (a
/// non-PIE executable only when -fixup_chains asks for them). A pinned
/// __TEXT stays where it is even then: in a dylib the other segments
/// still follow it, while in a PIE executable they go from __PAGEZERO's
/// end and so below it, out of order (place_segments).
pub fn resolve_image_base<E: Target>(ctx: &mut Context<E>) {
    // Before anything else looks at it, ld-prime rounds a base up to a
    // page (past the top, to 0): 4 KiB in an object file, which is
    // loaded nowhere.
    let align = if ctx.args.relocatable { 0x1000 } else { ctx.segment_align() };
    if let Some(base) = ctx.args.image_base
        && !base.is_multiple_of(align)
    {
        let aligned = base.wrapping_add(align - 1) & !(align - 1);
        crate::warn!(
            "base address 0x{base:X} is not properly aligned. Changing it to 0x{aligned:X}"
        );
        ctx.args.image_base = Some(aligned);
    }
    // It takes a zero base as none at all.
    if ctx.args.image_base == Some(0) {
        ctx.args.image_base = None;
    }
    if ctx.args.relocatable {
        ctx.args.image_base = None;
        return;
    }

    let text = ctx.args.segaddr("__TEXT");
    if let (Some(base), Some(text)) = (ctx.args.image_base, text)
        && base != text
    {
        if !ctx.args.static_link || ctx.args.pie {
            fatal!("-image_base and -segaddr __TEXT must match");
        }
        crate::warn!(
            "-image_base and -segaddr __TEXT must match, changing image base to {text:#x}"
        );
    }
    let Some(base) = text.or(ctx.args.image_base) else { return };
    ctx.args.image_base = Some(base);

    if ctx.args.output_type == MH_EXECUTE && ctx.args.pie && !ctx.args.static_link {
        crate::warn!("Linking with PIE, -image_base will be ignored");
        ctx.args.image_base = None;
    } else if !ctx.args.static_link && ctx.use_chained_fixups() {
        crate::warn!("prefered load addresses (-seg1addr) are disabled with chained fixups");
        ctx.args.image_base = text;
    }
}

/// Lays out the output: each segment's contents in file order, and the
/// segments in the address space. Where ld-prime puts a segment can
/// depend on the size of any other one (place_segments), so a segment
/// is laid out where it would go after the ones before it first and
/// moved once all are sized - all but the mach header's segment
/// (__TEXT), whose address is known up front (mach_header_addr) and
/// whose __unwind_info encodes the final addresses of its functions
/// (and of the others once they are placed: finish_unwind_info).
/// __LINKEDIT comes last: its tables read every other address.
pub fn set_osec_offsets<E: Target>(ctx: &mut Context<E>) {
    let linkedit = ctx.segments.len() - 1;
    debug_assert_eq!(ctx.segments[linkedit].name, "__LINKEDIT");

    let mut fileoff = lay_out_segments(ctx);
    while !finish_unwind_info(ctx) {
        fileoff = lay_out_segments(ctx);
    }

    // The output sections with range-extension thunks (executable
    // sections); their entries' addresses are recorded on the symbols
    // now that the sections are placed.
    let thunked: Vec<OutputSectionId> = ctx
        .chunks
        .iter()
        .filter_map(|&id| match id {
            ChunkId::Output(id) if !ctx.output_section(id).thunks.is_empty() => Some(id),
            _ => None,
        })
        .collect();
    if !thunked.is_empty() {
        crate::thunks::gather_thunk_addresses(ctx, &thunked);
    }

    // The fixup builders leave a text relocation's alignment alone.
    ctx.text_reloc_ranges = text_reloc_ranges(ctx);
    build_linkedit_tables(ctx);
    ctx.output_size = layout_segment(ctx, linkedit, fileoff, 0);
    place_linkedit(ctx);

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

/// The address ranges of the segments mapped without write permission,
/// where a pointer that needs a fixup (a rebase or a bind) is a text
/// relocation: the loader would have to make the segment writable to
/// apply it. None if the output may have text relocations
/// (Args::text_relocs) or has no fixups at all: an image nothing
/// slides has no rebases, and only a -static one has no binds either.
fn text_reloc_ranges<E: Target>(ctx: &Context<E>) -> Vec<Range<u64>> {
    if ctx.args.text_relocs || (ctx.args.static_link && !ctx.args.pie) {
        return Vec::new();
    }
    ctx.segments
        .iter()
        .filter(|seg| {
            seg.name != "__PAGEZERO"
                && seg.name != "__LINKEDIT"
                && chunks::segment_prots(ctx, seg.name).1 & VM_PROT_WRITE == 0
        })
        .map(|seg| seg.cmd.vmaddr..seg.cmd.vmaddr + seg.cmd.vmsize)
        .collect()
}

/// Fails the link on the text relocations applying relocations found,
/// listing every one as ld-prime does: output section by output
/// section, each atom's in address order where it encodes rebase
/// opcodes, else from the last to the first (the order an assembler
/// emits relocations in). An unaligned pointer in a chain fails the
/// link then instead.
fn report_text_relocs<E: Target>(ctx: &Context<E>) {
    let mut found = std::mem::take(&mut *ctx.text_relocs.lock().unwrap());
    if !ctx.use_chained_fixups() && ctx.chunks.contains(&ChunkId::RebaseInfo) {
        found.sort_unstable_by_key(|&(isec, i)| (ctx.isec_addr(isec as usize), i));
    } else {
        found.sort_unstable_by_key(|&(isec, i)| {
            (ctx.isec_addr(isec as usize), std::cmp::Reverse(i))
        });
    }
    let mut osec = None;
    for (id, i) in found {
        let isec = &ctx.isecs[id as usize];
        if osec != Some(isec.output_section) {
            eprintln!("Illegal text-relocations:");
            osec = Some(isec.output_section);
        }
        let rel = &ctx.isec_relocs(id as usize)[i as usize];
        let target = match rel.target() {
            RelocTarget::Sym(_) => {
                let sym = ctx.reloc_target_sym(isec.file as usize, rel).unwrap();
                ctx.symbols[sym].name().into()
            }
            RelocTarget::Section(target) => ctx.atom_name(target as usize),
        };
        eprintln!("  text-relocation in {} to '{target}'", ctx.atom_ref(id as usize, rel.offset));
    }
    if !chunks::chained_fixups::report_unaligned_chain_pointer(ctx) && osec.is_some() {
        error!("Found illegal text-relocations");
    }
}

/// Lays out every segment but __LINKEDIT and gives each its address.
/// Returns the file offset past them.
fn lay_out_segments<E: Target>(ctx: &mut Context<E>) -> u64 {
    let header_seg = in_place_segment(ctx);
    let header_addr = mach_header_addr(ctx);
    let mut fileoff = 0;
    // A -preload image's mach header and load commands fill the file's
    // first pages, ahead of the segments, whose base stands for the
    // image's address.
    if ctx.args.preload {
        ctx.mach_header.hdr.addr = ctx.image_base();
        ctx.mach_header.hdr.size = mach_header_size(ctx);
        fileoff = align_to(ctx.mach_header.hdr.size, ctx.segment_align());
    }
    // The other segments follow the header's (or the image base), each
    // on its first section's alignment where that exceeds a page (only
    // an image no dyld maps allows one); place_segments moves them where
    // they go. The file skips as many bytes as memory does, unless a
    // segment is pinned (ld-prime).
    let mirror_gaps = ctx.args.segaddrs.is_empty();
    let mut addr = ctx.image_base();
    for seg_idx in 0..ctx.segments.len() - 1 {
        let name = ctx.segments[seg_idx].name;
        if name == "__PAGEZERO" {
            fileoff = layout_segment(ctx, seg_idx, fileoff, 0);
            continue;
        }
        let vmaddr = if Some(name) == header_seg {
            fileoff = layout_segment(ctx, seg_idx, fileoff, header_addr);
            header_addr
        } else {
            let vmaddr = align_to(addr, segment_start_align(ctx, seg_idx));
            let gap = if mirror_gaps { vmaddr - addr } else { 0 };
            fileoff = layout_segment(ctx, seg_idx, fileoff + gap, vmaddr);
            vmaddr
        };
        addr = vmaddr + segment_span(ctx, &ctx.segments[seg_idx]);
    }
    place_segments(ctx);
    check_segment_addresses(ctx);
    crate::error::checkpoint();
    fileoff
}

/// The segment holding the mach header, laid out in place at the image
/// base or its -segaddr: none in a -preload image, whose header
/// precedes every segment in the file.
fn in_place_segment<E: Target>(ctx: &Context<E>) -> Option<&'static str> {
    (!ctx.args.preload).then(|| header_segment(ctx))
}

/// The boundary the segment after a segment starts on, in memory and in
/// the file: its -seg_page_size, else the page.
fn seg_page_size<E: Target>(ctx: &Context<E>, segname: &str) -> u64 {
    let sizes = &ctx.args.seg_page_sizes;
    sizes.iter().find(|(name, _)| name == segname).map_or(ctx.segment_align(), |&(_, size)| size)
}

/// The room a segment takes from the segments after it: its size up to
/// its -seg_page_size, which ld-prime leaves out of the size itself
/// (the XNU x86-64 kernel starts the segment after __TEXT on a 2 MiB
/// boundary that way).
fn segment_span<E: Target>(ctx: &Context<E>, seg: &OutputSegment) -> u64 {
    align_to(seg.cmd.vmsize, seg_page_size(ctx, seg.name))
}

/// The alignment of a segment's address: a page, or its first section's
/// alignment if greater.
fn segment_start_align<E: Target>(ctx: &Context<E>, seg_idx: usize) -> u64 {
    let first = ctx.segments[seg_idx].chunks.first().map_or(0, |&id| ctx.chunk_header(id).p2align);
    ctx.segment_align().max(1 << first)
}

/// __unwind_info is encoded as __TEXT is laid out, when only __TEXT's
/// addresses are final. If it covers code or LSDAs in other segments
/// too, this encodes it again now every segment has its address.
/// Returns false if that encoding needs more room than __TEXT left the
/// section; the layout is then done again with that much room (a
/// smaller one leaves zeros after it).
fn finish_unwind_info<E: Target>(ctx: &mut Context<E>) -> bool {
    if !ctx.chunks.contains(&ChunkId::UnwindInfo)
        || !chunks::unwind_info::covers_other_segments(ctx)
    {
        return true;
    }
    let (data, personalities) = {
        let _t = ctx.timer("unwind_encode");
        chunks::unwind_info::encode_unwind_info(ctx)
    };
    if data.len() as u64 > ctx.unwind_info.hdr.size {
        ctx.unwind_info.min_size = data.len() as u64;
        return false;
    }
    ctx.unwind_info.contents = data;
    ctx.unwind_info.personalities = personalities;
    true
}

/// Lays out a segment's chunks from file offset `fileoff` and address
/// `vmaddr`, and returns the file offset past the segment.
fn layout_segment<E: Target>(
    ctx: &mut Context<E>,
    seg_idx: usize,
    fileoff: u64,
    vmaddr: u64,
) -> u64 {
    let page = ctx.segment_align();
    if ctx.segments[seg_idx].name == "__PAGEZERO" {
        let seg = &mut ctx.segments[seg_idx];
        seg.cmd.vmaddr = 0;
        seg.cmd.vmsize = ctx.args.pagezero_size;
        return fileoff;
    }
    // The kernel maps a static executable's stack from nothing in the
    // file.
    if ctx.segments[seg_idx].name == "__UNIXSTACK" {
        let seg = &mut ctx.segments[seg_idx];
        seg.cmd.vmaddr = vmaddr;
        seg.cmd.vmsize = ctx.args.stack_size;
        return fileoff;
    }

    let seg_fileoff = fileoff;
    let mut cursor = fileoff;
    let chunk_ids = ctx.segments[seg_idx].chunks.clone();

    // Regular chunks, in file order
    for &id in &chunk_ids {
        if ctx.chunk_header(id).is_zerofill() {
            continue;
        }
        let size = match id {
            ChunkId::MachHeader => mach_header_size(ctx),
            // Encoded once its segment's addresses are known (the
            // __LINKEDIT tables are built ahead, but __unwind_info
            // embeds __TEXT offsets); the personality cells the
            // encoding cannot know yet (GOT addresses) come back as a
            // patch list for the copy phase.
            ChunkId::UnwindInfo => {
                let (data, personalities) = {
                    let _t = ctx.timer("unwind_encode");
                    chunks::unwind_info::encode_unwind_info(ctx)
                };
                let len = (data.len() as u64).max(ctx.unwind_info.min_size);
                ctx.unwind_info.contents = data;
                ctx.unwind_info.personalities = personalities;
                len
            }
            ChunkId::CodeSignature => {
                cursor = align_to(cursor, 16);
                chunks::code_signature::size(ctx, cursor)
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
            | ChunkId::LocalRelocs
            | ChunkId::ExternRelocs => 3,
            ChunkId::IndirectSymtab => 2,
            ChunkId::CodeSignature => 4,
            _ => ctx.chunk_header(id).p2align,
        };
        // Aligned is the address; the file offset keeps its distance
        // from it, which is no multiple of the alignment where a
        // -preload image's header pages shift the file.
        let addr = align_to(vmaddr + (cursor - seg_fileoff), 1 << p2align);
        cursor = seg_fileoff + (addr - vmaddr);
        let hdr = ctx.chunk_header_mut(id);
        hdr.fileoff = cursor;
        hdr.addr = addr;
        hdr.size = size;
        cursor += size;
    }

    let filesize = cursor - seg_fileoff;
    let mut vm_end = vmaddr + filesize;

    // Zero-fill chunks occupy address space after the file-backed part
    // of the segment.
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

    // A segment of zero-fill sections alone has nothing in the file,
    // and ld-prime gives it file offset 0, as it does those sections.
    let zerofill_only =
        !chunk_ids.is_empty() && chunk_ids.iter().all(|&id| ctx.chunk_header(id).is_zerofill());

    // __LINKEDIT's file contents end exactly at the code signature;
    // other segments are padded to a page boundary in the file, and the
    // next one starts on the segment's -seg_page_size boundary (which
    // ld-prime counts in __LINKEDIT's size, there being no next one).
    let seg_page = seg_page_size(ctx, ctx.segments[seg_idx].name);
    let seg = &mut ctx.segments[seg_idx];
    seg.cmd.vmaddr = vmaddr;
    seg.cmd.fileoff = if zerofill_only { 0 } else { seg_fileoff };
    if seg.name == "__LINKEDIT" {
        seg.cmd.filesize = filesize;
        seg.cmd.vmsize = align_to(vm_end - vmaddr, seg_page).max(filesize);
        return seg_fileoff + filesize;
    }
    seg.cmd.filesize = align_to(filesize, page);
    seg.cmd.vmsize = align_to(vm_end - vmaddr, page).max(seg.cmd.filesize);
    seg_fileoff + align_to(seg.cmd.filesize, seg_page)
}

/// Gives every segment but __LINKEDIT its address, as ld-prime does:
///
/// - A segment -segaddr pins goes there.
/// - With -segment_order, the segments listed after a pinned one
///   follow it, one after another (ld64's
///   segmentOrderAfterFixedAddressSegment).
/// - The others go from the image base, in segment order, each to the
///   lowest address where it runs into no segment placed before it.
///   The segments the two rules above place count as placed only from
///   the first of them (the mach header's segment aside) on, so the
///   segments ahead of it are simply laid out one after another - into
///   a pinned one, if it is in their way. A pinned __LINKEDIT, not
///   sized yet, counts from the start, as an empty segment.
fn place_segments<E: Target>(ctx: &mut Context<E>) {
    let base = ctx.image_base();
    let header_seg = in_place_segment(ctx);
    let segs = &ctx.segments[..ctx.segments.len() - 1];
    let range = |i: usize, addr: u64| addr..addr + segment_span(ctx, &segs[i]);

    // __PAGEZERO and the mach header's segment are laid out in place
    // already.
    let in_place: Vec<bool> =
        segs.iter().map(|seg| seg.name == "__PAGEZERO" || Some(seg.name) == header_seg).collect();
    let mut addrs: Vec<Option<u64>> = (0..segs.len())
        .map(
            |i| if in_place[i] { Some(segs[i].cmd.vmaddr) } else { ctx.args.segaddr(segs[i].name) },
        )
        .collect();
    for i in 1..segs.len() {
        if addrs[i].is_none()
            && follows_pinned_segment(ctx, segs[i].name)
            && let Some(prev) = addrs[i - 1]
        {
            let end = prev + segment_span(ctx, &segs[i - 1]);
            addrs[i] = Some(align_to(end, segment_start_align(ctx, i)));
        }
    }

    let fixed: Vec<usize> =
        (0..segs.len()).filter(|&i| !in_place[i] && addrs[i].is_some()).collect();
    let header = segs.iter().position(|seg| Some(seg.name) == header_seg);
    let mut used: Vec<Range<u64>> =
        header.map(|i| range(i, segs[i].cmd.vmaddr)).into_iter().collect();
    if let Some(addr) = ctx.args.segaddr("__LINKEDIT") {
        used.push(addr..addr);
    }
    for i in 0..segs.len() {
        if fixed.first() == Some(&i) {
            used.extend(fixed.iter().map(|&j| range(j, addrs[j].unwrap())));
        }
        if addrs[i].is_none() {
            let size = segment_span(ctx, &segs[i]);
            let span = lowest_free_span(base, size, segment_start_align(ctx, i), &used);
            addrs[i] = Some(span.end - size);
            used.push(span);
        }
    }

    for (i, addr) in addrs.into_iter().enumerate() {
        if !in_place[i] {
            move_segment(ctx, i, addr.unwrap());
        }
    }
}

/// Whether -segment_order lists a segment after one that -segaddr pins
/// (ld64's segmentOrderAfterFixedAddressSegment).
fn follows_pinned_segment<E: Target>(ctx: &Context<E>, segname: &str) -> bool {
    let mut pinned = false;
    for name in &ctx.args.segment_order {
        if name == segname {
            return pinned;
        }
        pinned |= ctx.args.segaddr(name).is_some();
    }
    false
}

/// Where `size` bytes go at the lowest address from `base` on where they
/// run into none of the `used` ranges: the base itself or the end of a
/// used range, rounded up to `align`. Returns the span from that address
/// before rounding to the end, which the rounding's padding is part of.
/// An empty segment is a point no other segment may straddle, and still
/// needs an address no segment covers.
fn lowest_free_span(base: u64, size: u64, align: u64, used: &[Range<u64>]) -> Range<u64> {
    let is_free = |span: &Range<u64>| {
        used.iter().all(|r| r.end <= span.start || span.end.max(span.start + 1) <= r.start)
    };
    std::iter::once(base)
        .chain(used.iter().map(|r| r.end).filter(|&end| end > base))
        .map(|start| start..align_to(start, align) + size)
        .filter(is_free)
        .min_by_key(|span| span.start)
        .unwrap()
}

/// Moves a laid-out segment to `addr`.
fn move_segment<E: Target>(ctx: &mut Context<E>, seg_idx: usize, addr: u64) {
    let delta = addr.wrapping_sub(ctx.segments[seg_idx].cmd.vmaddr);
    ctx.segments[seg_idx].cmd.vmaddr = addr;
    for i in 0..ctx.segments[seg_idx].chunks.len() {
        let id = ctx.segments[seg_idx].chunks[i];
        let hdr = ctx.chunk_header_mut(id);
        hdr.addr = hdr.addr.wrapping_add(delta);
    }
}

/// ld-prime refuses segments that overlap, which takes a -segaddr (or
/// an -image_base inside __PAGEZERO), and in an image dyld slides, a
/// segment below the one before it. It reports the first such segment.
/// Left out of the overlap check are empty segments and __LINKEDIT,
/// sized last.
fn check_segment_addresses<E: Target>(ctx: &Context<E>) {
    let (linkedit, segs) = ctx.segments.split_last().unwrap();
    let end = |seg: &OutputSegment| seg.cmd.vmaddr + seg.cmd.vmsize;
    for (i, a) in segs.iter().enumerate() {
        for b in &segs[i + 1..] {
            if a.cmd.vmsize > 0
                && b.cmd.vmsize > 0
                && a.cmd.vmaddr < end(b)
                && b.cmd.vmaddr < end(a)
            {
                error!(
                    "custom segments overlap: {}({:#x}-{:#x}) {}({:#x}-{:#x})",
                    a.name,
                    a.cmd.vmaddr,
                    end(a),
                    b.name,
                    b.cmd.vmaddr,
                    end(b)
                );
                return;
            }
        }
    }

    if !dyld_slides(ctx) {
        return;
    }
    for pair in segs.windows(2) {
        if pair[1].cmd.vmaddr < pair[0].cmd.vmaddr {
            error!("segment {} address is out of order", pair[1].name);
            return;
        }
    }
    if let (Some(addr), Some(last)) = (ctx.args.segaddr(linkedit.name), segs.last())
        && addr < last.cmd.vmaddr
    {
        error!("segment {} address is out of order", linkedit.name);
    }
}

/// __LINKEDIT goes where -segaddr pins it. Otherwise, in an image dyld
/// slides, it goes above every other segment, and in one that stays
/// where it was linked to the lowest address from the image base where
/// it fits, as any other segment would - which, with no segment pinned,
/// is above them too (a gap a segment's alignment left is no room).
fn place_linkedit<E: Target>(ctx: &mut Context<E>) {
    let linkedit = ctx.segments.len() - 1;
    let others = &ctx.segments[..linkedit];
    let addr = if let Some(addr) = ctx.args.segaddr("__LINKEDIT") {
        addr
    } else if dyld_slides(ctx) || ctx.args.segaddrs.is_empty() {
        others.iter().map(|seg| seg.cmd.vmaddr + segment_span(ctx, seg)).max().unwrap_or(0)
    } else {
        let used: Vec<Range<u64>> = others
            .iter()
            .map(|seg| seg.cmd.vmaddr..seg.cmd.vmaddr + segment_span(ctx, seg))
            .collect();
        let size = ctx.segments[linkedit].cmd.vmsize;
        lowest_free_span(ctx.image_base(), size, ctx.segment_align(), &used).start
    };
    move_segment(ctx, linkedit, addr);
}

/// Whether dyld loads the image wherever it likes: a PIE executable, a
/// dylib or a bundle, but not a -static image or a non-PIE executable.
fn dyld_slides<E: Target>(ctx: &Context<E>) -> bool {
    !ctx.args.static_link && (ctx.args.output_type != MH_EXECUTE || ctx.args.pie)
}

/// Builds the __LINKEDIT tables, once every other address is final.
fn build_linkedit_tables<E: Target>(ctx: &mut Context<E>) {
    // The LINKEDIT tables are independent of one another and
    // every address they read is final (the symbol table needs
    // none at all), so they build as one parallel task group;
    // layout_segment just consumes the cached bytes.
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
                        && sym
                            .input_section()
                            .map(|i| i as usize)
                            .is_none_or(|isec| shared.isecs[shared.resolve_isec(isec)].is_alive())
                })
                .map(|i| i as u32)
                .collect();
            v.par_sort_unstable_by_key(|&i| crate::util::name_sort_key(shared.symbols[i].name()));
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
                        if let Some(chained) = chunks::chained_fixups::build_chained_fixups(shared)
                        {
                            return Streams::Chained(chained);
                        }
                    } else {
                        chunks::chained_fixups::check_classic_pointers(shared);
                    }
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
    // Each table's size follows from its contents; layout_segment
    // places them.
    let (dice, split) = dice;
    ctx.symtab = symtab;
    ctx.symtab.hdr.size = (ctx.symtab.len() * size_of::<NList>()) as u64;
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
            // An image laid out for chains may fall back to these.
            ctx.chained_fixups.disabled = use_chained;
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
    collect_relocations(ctx);
}

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
        // An image that loads no dylib at all fails the libSystem
        // check that dead_strip_dylibs would make later, and ld-prime
        // says so first.
        check_libsystem_linked(ctx);
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
    // Relocations that failed to apply fail the link before the fixups
    // are written.
    report_text_relocs(ctx);
    crate::error::checkpoint();
    chunks::chained_fixups::warn_unaligned_pointers(ctx);

    if ctx.use_chained_fixups() {
        let _t = ctx.timer("write_fixup_chains");
        chunks::chained_fixups::write_fixup_chains(ctx, buf);
    }
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        chunks::local_relocs::write(ctx, buf);
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) {
        chunks::extern_relocs::write(ctx, buf);
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
