//! The linker passes, in the order the driver runs them.

use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::chunks::init_offsets::InitFunc;
use crate::chunks::{self, ChunkId, OutputSectionId, OutputSegment, mach_header_size};
use crate::cmdline::{Args, InputArg};
use crate::context::Context;
use crate::error;
use crate::fatal;
use crate::filetype::{FileType, get_file_type};
use crate::input_files;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, RelocTarget};
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::objc::{DataBlob, DataField};
use crate::output_sections::{data_seg, header_segment};
use crate::tapi;
use crate::target::RelocClass;
use crate::target::Target;
use crate::util::{align_to, path_bytes};

/// The default library search path: ld64's /usr/lib and /usr/local/lib,
/// and between them ld-prime's /usr/lib/swift, which it searches for
/// any library, not only Swift's.
const STANDARD_LIBRARY_DIRS: &[&str] = &["/usr/lib", "/usr/lib/swift", "/usr/local/lib"];

/// The default framework search path, as ld64's.
const STANDARD_FRAMEWORK_DIRS: &[&str] = &["/Library/Frameworks", "/System/Library/Frameworks"];

/// Settles the library and framework search paths, once the options
/// are checked, as ld-prime does: the -L (-F) directories, then, unless
/// -Z, the default ones (see search_dirs). -v prints the banner here,
/// then the paths on stderr, as ld-prime does.
pub fn set_search_paths<E: Target>(ctx: &mut Context<E>) {
    let args = &mut ctx.args;
    if args.verbose {
        crate::cmdline::print_version();
    }
    args.library_paths = search_dirs(args, &args.library_paths, STANDARD_LIBRARY_DIRS);
    args.framework_paths = search_dirs(args, &args.framework_paths, STANDARD_FRAMEWORK_DIRS);
    if args.verbose {
        let mut out = Vec::new();
        for (title, dirs) in
            [("Library", &args.library_paths), ("Framework", &args.framework_paths)]
        {
            out.extend_from_slice(format!("{title} search paths:\n").as_bytes());
            for dir in dirs {
                out.push(b'\t');
                out.extend_from_slice(path_bytes(dir));
                out.push(b'\n');
            }
        }
        let _ = std::io::Write::write_all(&mut std::io::stderr(), &out);
    }
}

/// The directories `dirs` given on the command line, then, unless -Z,
/// the default ones, each looked up under the syslibroots as ld64 does
/// (see push_search_dir) - but a default directory missing from the
/// only SDK is not searched at all, not even outside it.
fn search_dirs(args: &Args, dirs: &[PathBuf], standard: &[&str]) -> Vec<PathBuf> {
    let syslibroot = &args.syslibroot;
    let mut out = Vec::new();
    for dir in dirs {
        push_search_dir(syslibroot, &mut out, dir);
    }
    if !args.no_standard_dirs {
        for dir in standard {
            if let [root] = syslibroot.as_slice() {
                let dir = under_root(root, Path::new(dir));
                if dir.is_dir() {
                    out.push(dir);
                }
            } else {
                push_search_dir(syslibroot, &mut out, Path::new(dir));
            }
        }
    }
    out
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

fn find_framework<E: Target>(ctx: &Context<E>, name: &OsStr) -> Option<PathBuf> {
    let with_suffix = |suffix: &str| {
        let mut file = name.to_os_string();
        file.push(suffix);
        file
    };
    for dir in &ctx.args.framework_paths {
        let fw = dir.join(with_suffix(".framework"));
        for file in [with_suffix(".tbd"), name.to_os_string()] {
            let path = fw.join(file);
            if path.exists() {
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
    // An image that links no dylib (Args::links_dylibs) looks for
    // archives only. A relocatable output looks for dylibs too, only to
    // ignore them (collect_file).
    let passes: &[&[&str]] = if !ctx.args.links_dylibs() {
        &[&["a"]]
    } else if ctx.args.search_dylibs_first {
        &[&["tbd", "dylib"], &["a"]]
    } else {
        &[&["tbd", "dylib", "a"]]
    };
    search_library(ctx, name, passes)
}

/// Looks for a dylib only, as -upward-l and -reexport-l do: an archive
/// can be neither an upward dependency nor a re-exported library.
fn find_dylib<E: Target>(ctx: &Context<E>, name: &OsStr) -> Option<PathBuf> {
    search_library(ctx, name, &[&["tbd", "dylib"]])
}

/// Looks for lib<name>.<ext> in the library search path, for each pass
/// of extensions in turn. What is there counts, as for ld-prime, which
/// fails on a directory it finds (see unreadable_input).
fn search_library<E: Target>(
    ctx: &Context<E>,
    name: &OsStr,
    passes: &[&[&str]],
) -> Option<PathBuf> {
    for exts in passes {
        for dir in &ctx.args.library_paths {
            for ext in *exts {
                let mut file = OsString::from("lib");
                file.push(name);
                file.push(format!(".{ext}"));
                let path = dir.join(file);
                if path.exists() {
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

/// Gives a dylib what its first naming says (library_namings has merged
/// what the options naming one library say): -needed_* keeps the load
/// command under -dead_strip_dylibs, -reexport_* re-exports it, -weak_*
/// makes every import from it weak and -upward_* makes it an upward
/// dependency. A library so far loaded only as a public re-export of
/// another (Foundation's stub brings CoreFoundation) got its weakness
/// from that parent; its first command-line naming decides it instead:
/// `-weak_framework Foundation -framework CoreFoundation` imports from
/// CoreFoundation strongly, while an auto-link option, a hint, changes
/// nothing. -needed_* covers the named library only; the ones its stub
/// re-exports get a load command only if something binds to them.
fn name_dylib<E: Target>(ctx: &mut Context<E>, idx: usize, mf: &MappedFile, rc: ReaderContext) {
    if ctx.dylibs[idx].is_implicit {
        ctx.dylibs[idx].named_at = Some((ctx.next_priority(), mf.name.clone()));
    }
    let lazy = rc.lazy && ctx.args.lazy_load;
    let dylib = &mut ctx.dylibs[idx];
    if dylib.is_implicit && !rc.autolinked {
        dylib.is_weak = rc.weak;
        dylib.is_lazy = lazy;
    } else {
        dylib.is_weak |= rc.weak;
        dylib.is_lazy |= lazy;
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
    /// -lazy_library, -lazy-l, -lazy_framework: dyld loads the dylib
    /// at its first use (from macOS 27 on; as any other before).
    lazy: bool,
    /// Named by an object's auto-link option: a hint.
    autolinked: bool,
}

impl ReaderContext {
    /// What two namings of one library say together.
    fn union(self, other: Self) -> Self {
        Self {
            force_load: self.force_load || other.force_load,
            weak: self.weak || other.weak,
            reexport: self.reexport || other.reexport,
            hidden: self.hidden || other.hidden,
            needed: self.needed || other.needed,
            upward: self.upward || other.upward,
            lazy: self.lazy || other.lazy,
            autolinked: self.autolinked && other.autolinked,
        }
    }
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

/// Whether a dylib (or a universal file) a naming loaded before was
/// ignored then: for lacking the architecture, or in a link that takes
/// no dylib.
fn was_ignored<E: Target>(ctx: &Context<E>, mf: &'static MappedFile) -> bool {
    match get_file_type(mf) {
        FileType::Tapi | FileType::Dylib => !ctx.dylibs.iter().any(|d| d.path == mf.name),
        FileType::Fat => input_files::fat_slice::<E>(mf).is_none(),
        _ => false,
    }
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
    // auto-link options; load each file once, as its first naming says
    // (the first that isn't a public re-export's, for a dylib). An
    // object file, though, loads as often as it is named, as in
    // ld-prime: twice over, its globals are duplicate definitions. So
    // does a dylib ld-prime ignored, with its warning each time.
    let object = matches!(get_file_type(mf), FileType::Object | FileType::LlvmBitcode);
    if !ctx.visited_files.insert(mf.name.clone()) && !object && !was_ignored(ctx, mf) {
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
        // final link, and the other images that link no dylib have
        // nothing to load one with (Args::links_dylibs): ld-prime reads
        // a dylib on their command lines (and ignores a stub without
        // the architecture as ever), then ignores it with a warning.
        FileType::Tapi | FileType::Dylib if ctx.args.relocatable || !ctx.args.links_dylibs() => {
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
            // public libraries it re-exports; a weak parent's are weak,
            // and a lazy one's lazy. (Those standing for libraries its
            // exports moved to load weakly only as their imports say;
            // see weaken_moved_imports.)
            let lazy = rc.lazy && ctx.args.lazy_load;
            for d in &mut ctx.dylibs[first..] {
                d.is_weak |= rc.weak && d.name_source != input_files::NameSource::Moved;
                d.is_lazy |= lazy;
            }
            // One named before by another path keeps what that said.
            if idx >= first || ctx.dylibs[idx].is_implicit {
                if ctx.dylibs[idx].path != mf.name {
                    let path = ctx.dylibs[idx].path.clone();
                    input_files::untrace_file(ctx, path_bytes(&path));
                }
                name_dylib(ctx, idx, mf, rc);
            }
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
            None => input_files::warn_fat_missing_arch::<E>(mf),
        },
        FileType::LlvmBitcode => {
            input_files::parse_bitcode(ctx, mf, true);
        }
        FileType::Empty => {}
        _ => {
            let name = input_files::trace_name(path_bytes(&mf.name));
            if crate::filetype::get_macho_filetype(mf.data()).is_some() {
                fatal!(
                    "unsupported mach-o filetype (only MH_OBJECT and MH_DYLIB can be linked) in '{name}'"
                );
            }
            fatal!("unknown file type in '{name}'");
        }
    }
}

/// ld-prime warns of some sections of every object it parses - archive
/// members the link doesn't use included: it drops each __LD section it
/// doesn't know, aligns the constants of a __DATA,__cfstring to a
/// pointer whatever the section says, reads an __objc_imageinfo record
/// only if it has its 8 bytes and no more than their worth, and ignores
/// a label at the end of a section of fixed-size records. It fails the
/// link on an initializer, terminator or __objc_clsrolist pointer with
/// no relocation.
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
        if let Some(what) = obj.pointer_without_target {
            error!("{what} has no target in '{}'", resolved_file_name(obj.mf));
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

/// ld64 warns, once, about the libraries given more than once, each as
/// spelled (-weak-lz repeats no -lz): the -l options, those naming a
/// library by path, and an archive's bare path, but not a dylib's or a
/// framework option. Build systems that knowingly repeat them pass
/// -no_warn_duplicate_libraries. Under -w ld-prime leaves the warning
/// out, -fatal_warnings or not.
fn warn_duplicate_libraries<E: Target>(ctx: &Context<E>) {
    if !ctx.args.warn_duplicate_libraries || ctx.args.suppress_warnings {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    let mut seen_files = std::collections::HashSet::new();
    let mut dups = std::collections::BTreeSet::new();
    for arg in &ctx.args.inputs {
        let (option, name) = match arg {
            InputArg::Lib(name, false) => ("-l", name.as_os_str()),
            InputArg::Lib(name, true) => ("-weak-l", name.as_os_str()),
            InputArg::NeededLib(name) => ("-needed-l", name.as_os_str()),
            InputArg::ReexportLib(name) => ("-reexport-l", name.as_os_str()),
            InputArg::HiddenLib(name) => ("-hidden-l", name.as_os_str()),
            InputArg::UpwardLib(name) => ("-upward-l", name.as_os_str()),
            InputArg::LazyLib(name) => ("-lazy-l", name.as_os_str()),
            InputArg::ForceLoad(path) => ("-force_load ", path.as_os_str()),
            InputArg::WeakFile(path) => ("-weak_library ", path.as_os_str()),
            InputArg::ReexportFile(path) => ("-reexport_library ", path.as_os_str()),
            InputArg::NeededFile(path) => ("-needed_library ", path.as_os_str()),
            InputArg::UpwardFile(path) => ("-upward_library ", path.as_os_str()),
            InputArg::LazyFile(path) => ("-lazy_library ", path.as_os_str()),
            // Objects, which are many, go without a string.
            InputArg::File(path) => {
                if !seen_files.insert(path)
                    && MappedFile::open(path)
                        .is_some_and(|mf| get_file_type(mf) == FileType::Archive)
                {
                    dups.insert(format!("'{}'", path.display()));
                }
                continue;
            }
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

pub fn read_input_files<E: Target>(ctx: &mut Context<E>) {
    warn_duplicate_libraries(ctx);
    let inputs = std::mem::take(&mut ctx.args.inputs);
    let paths = find_inputs(ctx, &inputs);
    let namings = library_namings(&inputs, &paths);

    // Warm the .tbd parse cache: parse every input that is a stub
    // library on all cores, then do the same for the stubs they
    // reexport - two waves cover an SDK's umbrella trees. The serial
    // loop below then finds every parse already done.
    {
        let stubs: Vec<&'static MappedFile> = inputs
            .iter()
            .zip(&paths)
            .filter(|(arg, _)| !matches!(arg, InputArg::ForceLoad(_)))
            .filter_map(|(_, path)| MappedFile::open(path.as_ref()?))
            .filter(|mf| get_file_type(mf) == FileType::Tapi)
            .collect();
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

    let mut queue: Vec<PendingObject> = Vec::new();
    for ((arg, path), rc) in inputs.iter().zip(paths).zip(namings) {
        let (Some(path), Some(rc)) = (path, rc) else { continue };
        match MappedFile::try_open(&path) {
            Ok(mf) if mf.size() == 0 => error!("file is empty in '{}'", path.display()),
            Ok(mf) if matches!(arg, InputArg::BundleLoader(_)) => {
                load_bundle_loader(ctx, mf, rc, &mut queue)
            }
            Ok(mf) => collect_file(ctx, mf, rc, &mut queue),
            Err(e) => error!("{}", unreadable_input(&path, &e)),
        }
    }
    ctx.args.inputs = inputs;
    collect_indirect_files(ctx, &mut queue);
    load_pending(ctx, queue);
}

/// Loads the files that -dylib_file names for re-exported libraries
/// but that are no libraries (see load_reexports) as any input, after
/// the command line's: ld-prime links an object named so into the
/// output.
fn collect_indirect_files<E: Target>(ctx: &mut Context<E>, out: &mut Vec<PendingObject>) {
    for mf in std::mem::take(&mut ctx.indirect_files) {
        collect_file(ctx, mf, ReaderContext::default(), out);
    }
}

/// ld-prime's words for an input file MappedFile::try_open failed on
/// with `e`. ld-prime maps every input whole, and refuses an empty one,
/// so a file that is there but no regular one (which MappedFile takes
/// for none) is one it can't map - a directory - or an empty one.
pub fn unreadable_input(path: &Path, e: &std::io::Error) -> String {
    let p = path.display();
    let found = std::fs::metadata(path).ok().filter(|_| e.kind() == std::io::ErrorKind::NotFound);
    match found {
        Some(md) if md.len() == 0 => format!("file is empty in '{p}'"),
        Some(_) => {
            let e = std::io::Error::from_raw_os_error(libc::EINVAL);
            format!("file cannot be mmap()ed, {} path={p} in '{p}'", crate::error::errno_text(&e))
        }
        None => {
            format!("file cannot be open()ed, {} path={p} in '{p}'", crate::error::errno_text(e))
        }
    }
}

/// Finds the file each input names: None for a library or framework
/// not found, or a file a library option, -force_load or -bundle_loader
/// names that isn't there.
fn find_inputs<E: Target>(ctx: &Context<E>, inputs: &[InputArg]) -> Vec<Option<PathBuf>> {
    let find = |arg: &InputArg| match arg {
        InputArg::File(path) => Some(path.clone()),
        InputArg::ForceLoad(path)
        | InputArg::BundleLoader(path)
        | InputArg::WeakFile(path)
        | InputArg::ReexportFile(path)
        | InputArg::NeededFile(path)
        | InputArg::UpwardFile(path)
        | InputArg::LazyFile(path) => find_file(ctx, path),
        InputArg::Lib(name, _)
        | InputArg::HiddenLib(name)
        | InputArg::NeededLib(name)
        | InputArg::LazyLib(name) => find_library(ctx, name),
        InputArg::UpwardLib(name) | InputArg::ReexportLib(name) => find_dylib(ctx, name),
        InputArg::Framework(name, _)
        | InputArg::ReexportFramework(name)
        | InputArg::NeededFramework(name)
        | InputArg::UpwardFramework(name)
        | InputArg::LazyFramework(name) => find_framework(ctx, name),
    };
    inputs.iter().map(find).collect()
}

/// The file an option that takes a library's path names: an absolute
/// path under each -syslibroot first, as ld64's findFile looks it up,
/// then as it is, each time a stub in place of the library where there
/// is one - `-weak_library /usr/lib/libz.dylib` links the SDK's
/// usr/lib/libz.tbd. An object is taken as it is.
fn find_file<E: Target>(ctx: &Context<E>, path: &Path) -> Option<PathBuf> {
    let ext = path.extension();
    let object = ext == Some(OsStr::new("o"));
    let archive = ext == Some(OsStr::new("a"));
    let mut candidates: Vec<PathBuf> = Vec::new();
    if path.is_absolute() && !object {
        for root in &ctx.args.syslibroot {
            let path = under_root(root, path);
            candidates.extend([path.with_extension("tbd"), path]);
        }
    }
    if !object && !archive {
        candidates.push(path.with_extension("tbd"));
    }
    candidates.push(path.to_path_buf());
    candidates.into_iter().find(|path| path.exists())
}

/// What a library option says of the library it names: the flags it
/// gives the file, whether the library is a framework, and its name as
/// the option gives it (a path, for the options that take one).
fn library_option(arg: &InputArg) -> Option<(ReaderContext, bool, &OsStr)> {
    use InputArg::*;
    let name = match arg {
        Lib(name, _)
        | ReexportLib(name)
        | HiddenLib(name)
        | NeededLib(name)
        | UpwardLib(name)
        | LazyLib(name)
        | Framework(name, _)
        | ReexportFramework(name)
        | NeededFramework(name)
        | UpwardFramework(name)
        | LazyFramework(name) => name.as_os_str(),
        WeakFile(path) | ReexportFile(path) | NeededFile(path) | UpwardFile(path)
        | LazyFile(path) => path.as_os_str(),
        File(_) | ForceLoad(_) | BundleLoader(_) => return None,
    };
    let rc = ReaderContext {
        weak: matches!(arg, Lib(_, true) | Framework(_, true) | WeakFile(_)),
        reexport: matches!(arg, ReexportLib(_) | ReexportFramework(_) | ReexportFile(_)),
        hidden: matches!(arg, HiddenLib(_)),
        needed: matches!(arg, NeededLib(_) | NeededFramework(_) | NeededFile(_)),
        upward: matches!(arg, UpwardLib(_) | UpwardFramework(_) | UpwardFile(_)),
        lazy: matches!(arg, LazyLib(_) | LazyFramework(_) | LazyFile(_)),
        ..Default::default()
    };
    let framework = matches!(
        arg,
        Framework(..)
            | ReexportFramework(_)
            | NeededFramework(_)
            | UpwardFramework(_)
            | LazyFramework(_)
    );
    Some((rc, framework, name))
}

/// How each input is named: the flags its option gives the file, None
/// for a library an earlier option named. ld-prime reads the library
/// options before it reads a file, and merges what those naming one
/// library say - those naming one framework, or finding one file: under
/// -L., `-lfoo` and `-upward_library ./libfoo.dylib` both load an upward
/// libfoo. A file also given by bare path, or named by options that
/// match no other way (`-upward_library libfoo.dylib`), takes nothing
/// from the other namings: the first to load the file decides (see
/// collect_file). ld-prime stops at the first library it doesn't find,
/// -force_load's among them, and at a naming check_naming refuses; it
/// looks the frameworks up only after all of the libraries.
fn library_namings(inputs: &[InputArg], paths: &[Option<PathBuf>]) -> Vec<Option<ReaderContext>> {
    let mut merged: hashbrown::HashMap<(bool, &OsStr), ReaderContext> = hashbrown::HashMap::new();
    let mut keys = Vec::with_capacity(inputs.len());
    let mut missing_framework = None;
    for (arg, path) in inputs.iter().zip(paths) {
        let key = match (library_option(arg), path) {
            (Some((_, true, name)), None) => {
                missing_framework.get_or_insert(name);
                None
            }
            (Some((_, false, name)), None) => fatal!("library '{}' not found", name.display()),
            (None, None) => match arg {
                InputArg::ForceLoad(path) | InputArg::BundleLoader(path) => {
                    fatal!("library '{}' not found", path.display())
                }
                _ => None,
            },
            (Some((rc, framework, name)), Some(path)) => {
                // A framework by its name, any other library by the
                // file found.
                let key = (framework, if framework { name } else { path.as_os_str() });
                let all = merged.entry(key).or_default();
                // An archive has no imports to make weak; ld-prime warns
                // of a -weak-l that finds one, once for the library.
                if let InputArg::Lib(_, true) = arg
                    && !all.weak
                    && path.extension() == Some(OsStr::new("a"))
                {
                    crate::warn!(
                        "-weak-l{0} resolved to a static library '{1}', but only dynamic libraries can be weak linked. Use -l{0} when linking static libraries, or make sure .dylib/.tbd library is located in -L search paths.",
                        name.display(),
                        path.display()
                    );
                }
                *all = all.union(rc);
                if missing_framework.is_none() {
                    check_naming(*all, framework, name);
                }
                Some(key)
            }
            (None, _) => None,
        };
        keys.push(key);
    }
    if let Some(name) = missing_framework {
        fatal!("framework '{}' not found", name.display());
    }
    // The options naming one library make one input, where it is first
    // named; each other input is one of its own.
    let naming = |(arg, key): (&InputArg, Option<_>)| match key {
        Some(key) => merged.remove(&key),
        None => Some(ReaderContext {
            force_load: matches!(arg, InputArg::ForceLoad(_)),
            ..Default::default()
        }),
    };
    inputs.iter().zip(keys).map(naming).collect()
}

/// ld-prime refuses to re-export a library that it links weakly or
/// lazily, naming the pair as the option that makes it spells the
/// library (a path as `-weak-l<path>`).
fn check_naming(rc: ReaderContext, framework: bool, name: &OsStr) {
    let spell = |opt: &str| match framework {
        true => format!("'-{opt}_framework {}'", name.display()),
        false => format!("'-{opt}-l{}'", name.display()),
    };
    for (on, opt) in [(rc.weak, "weak"), (rc.lazy, "lazy")] {
        if on && rc.reexport {
            fatal!("{} and {} cannot be used together", spell(opt), spell("reexport"));
        }
    }
}

/// -bundle_loader: the executable that will load this bundle. Its
/// exports resolve the bundle's remaining undefined symbols, bound at
/// run time to the main executable (XCTest bundles hosted by an app are
/// linked this way). It loads where the command line names it, which
/// places it among the dylibs as ld-prime does. A file of another kind
/// loads as any input would: ld-prime links an object named so into
/// the bundle, binds to a dylib as usual and refuses another bundle.
fn load_bundle_loader<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    rc: ReaderContext,
    out: &mut Vec<PendingObject>,
) {
    let exe = match get_file_type(mf) {
        FileType::Fat => input_files::fat_slice::<E>(mf),
        _ => Some(mf),
    };
    match exe.filter(|exe| crate::filetype::get_macho_filetype(exe.data()) == Some(MH_EXECUTE)) {
        Some(exe) => {
            input_files::trace_file(ctx, path_bytes(&mf.name));
            input_files::parse_bundle_loader(ctx, exe);
        }
        None => collect_file(ctx, mf, rc, out),
    }
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
    collect_indirect_files(ctx, &mut queue);
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
/// An export that an $ld$previous directive moves to an older library
/// for the target binds to that one.
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
    dylibs[idx].moved_exports.get(name).copied().unwrap_or(idx)
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
    if ctx.args.has_entry_point() {
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

/// The rank of a definition: (class << 40) | (weak term << 32) |
/// priority, lower is better. A live weak definition's rank carries
/// the order in which ld-prime, like ld64, prefers the copies of one
/// (see weak_definition_rank); the first copy wins only among equals.
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
    let mut weak_term = 0u64;
    if class == 1
        && nlist.n_type() == N_SECT
        && let Some((isec, _)) =
            crate::input_files::find_symbol_subsec(isecs, &obj.subsecs, nlist.n_sect, nlist.n_value)
    {
        weak_term = weak_definition_rank(&isecs[isec], nlist, obj.hidden);
    }
    Some((class << 40) | (weak_term << 32) | obj.priority as u64)
}

/// How ld-prime, like ld64, orders the copies of a weak definition,
/// lower first: a copy that can't be auto-hidden before one that can
/// (.weak_def_can_be_hidden, a global's N_WEAK_DEF | N_WEAK_REF), then
/// a global before a private extern (unless both can be hidden), then
/// the more aligned. An atom's alignment is its section's with the
/// atom's address as the modulus, so a copy at 8 mod 16 is 8-aligned:
/// a Swift metadata record comes at 16 from one object and at 8 from
/// another, and the first copy wins only if equally aligned.
fn weak_definition_rank(isec: &InputSection, nlist: &NList, hidden: bool) -> u64 {
    let private = nlist.n_type & N_PEXT != 0 || hidden;
    let auto_hide = !private && nlist.n_desc & N_WEAK_REF != 0;
    let p2align = isec.p2align_at(nlist.n_value) as u64;
    ((auto_hide as u64) << 7) | ((private as u64) << 6) | (63 - p2align)
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
    let init = init_function(ctx);
    if !ctx.args.init_offsets && !implied {
        // ld-prime runs an -init function only from __init_offsets and
        // drops it here. ld64 named it in LC_ROUTINES_64, which dyld
        // runs before the image's other initializers; so do we, for an
        // image dyld loads, if the function is its own.
        if !ctx.args.without_dyld() {
            ctx.init_routine = init.filter(|&id| ctx.symbols[id].input_section().is_some());
        }
        return;
    }
    // ld-prime makes it the first of the initializer offsets.
    if let Some(id) = init {
        let func = init_func(ctx, id);
        ctx.init_offsets.init_funcs.push(func);
    }
    for i in 0..ctx.isecs.len() {
        if ctx.hdr_of(&ctx.isecs[i]).section_type() != S_MOD_INIT_FUNC_POINTERS
            || !ctx.isecs[i].is_alive()
        {
            continue;
        }
        let obj = ctx.isecs[i].file as usize;
        for rel in initializer_relocs(ctx, i) {
            let func = match rel.target() {
                RelocTarget::Sym(idx) => init_func(ctx, ctx.objs[obj].symbols[idx as usize]),
                RelocTarget::Section(isec) => {
                    InitFunc::Local(ctx.resolve_isec(isec as usize), rel.addend as u64)
                }
            };
            ctx.init_offsets.init_funcs.push(func);
        }
        ctx.isecs[i].set_alive(false);
    }
}

/// The initializer symbol `id` is. One dyld binds has no offset in the
/// image: ld-prime fails the link as it writes it (see
/// init_offsets::copy_buf). An absolute symbol's value is the offset.
fn init_func<E: Target>(ctx: &Context<E>, id: crate::symbol::SymbolId) -> InitFunc {
    let sym = &ctx.symbols[id];
    match sym.input_section() {
        Some(isec) => InitFunc::Local(ctx.resolve_isec(isec as usize), sym.value),
        None if ctx.is_absolute_symbol(id) => InitFunc::Absolute(sym.value),
        None => InitFunc::Imported(id),
    }
}

/// The function -init names, if it is defined. An undefined one is
/// reported with the other initial undefines.
fn init_function<E: Target>(ctx: &Context<E>) -> Option<crate::symbol::SymbolId> {
    let id = ctx.symbols.get(ctx.args.init.as_deref()?)?;
    ctx.symbols[id].is_defined().then_some(id)
}

/// The relocations naming the functions of the initializer pointers
/// subsection `i` holds, in slot order. A pointer the difference of two
/// symbols makes (a SUBTRACTOR and an UNSIGNED relocation) names the
/// function it adds, as in ld-prime, not the one it subtracts too.
fn initializer_relocs<E: Target>(ctx: &Context<E>, i: usize) -> Vec<crate::input_sections::Reloc> {
    let mut relocs: Vec<_> =
        ctx.isec_relocs(i).iter().filter(|r| r.r_type != E::RELOC_SUBTRACTOR).copied().collect();
    relocs.sort_by_key(|r| r.offset);
    relocs
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
        for rel in initializer_relocs(ctx, i) {
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
/// rather than naming it LC<n>/l<nnn>. So it keeps an __objc_superrefs
/// or __objc_protorefs entry any symbol names, even the ltmpN label of
/// its section's start (see coalesce_objc_refs), unless the section
/// is of the literal-pointer type (see is_class_or_protocol_ref).
fn mark_labeled_literals<E: Target>(ctx: &Context<E>) {
    ctx.symbols.syms.par_iter().for_each(|sym| {
        if let Some(i) = sym.input_section()
            && !sym.name().is_empty()
        {
            let isec = &ctx.isecs[i as usize];
            let hdr = ctx.hdr_of(isec);
            let labeled = match hdr.section_type() {
                S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS => {
                    !sym.name().starts_with(['l', 'L'])
                }
                S_LITERAL_POINTERS => false,
                _ => is_class_or_protocol_ref(hdr),
            };
            if labeled {
                isec.mark_labeled();
            }
        }
    });
}

/// Merges identical literal elements across all live inputs: the most
/// aligned live copy wins, the first of equals, and the rest redirect
/// to it. Only the elements ld-prime merges take part (see
/// is_mergeable_literal); a labeled record stays apart, and so do
/// copies in sections of different names, as in ld-prime: a class
/// named "Foo" keeps its name in __objc_classname though __cstring has
/// a "Foo" too.
///
/// A C string keeps its input offset modulo its section's alignment,
/// as any atom does (see InputSection::align_offset), and like ld64
/// ld-prime keeps the copy that alignment favors most (see
/// InputSection::p2align_at): Swift pads the strings of its 16-aligned
/// __objc_methname so that many start at a multiple of 16, and a copy
/// from Swift then wins over clang's, which has no alignment.
pub fn merge_literals<E: Target>(ctx: &mut Context<E>) {
    mark_labeled_literals(ctx);
    // Deduplication follows the symbol table's sharded shape: every
    // element's content hash is computed in parallel, elements bin by
    // hash, and the shards resolve independently - within a shard the
    // copies meet in input order, as in the old serial single-map
    // walk.
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
            if !is_mergeable_literal(hdr, isec) {
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
            // the same bytes in a section of the same name. An entry
            // holds its first copy and its group, whose winner so far
            // is in `best`.
            let mut table: hashbrown::HashTable<(u64, &MachSection, u32, u32)> =
                hashbrown::HashTable::new();
            let mut best: Vec<u32> = Vec::new();
            let mut losers: Vec<(u32, u32)> = Vec::new();
            let p2align =
                |i: u32| isecs[i as usize].p2align_at(isecs[i as usize].input_addr as u64);
            for (hash, hdr, i) in bin {
                let data = isecs[i as usize].data();
                let same = |&(h, other, j, _): &(u64, &MachSection, u32, u32)| {
                    h == hash
                        && other.segname == hdr.segname
                        && other.sectname == hdr.sectname
                        && other.section_type() == hdr.section_type()
                        && isecs[j as usize].data() == data
                };
                match table.entry(hash, same, |e| e.0) {
                    hashbrown::hash_table::Entry::Occupied(e) => {
                        let group = e.get().3;
                        let winner = &mut best[group as usize];
                        if p2align(i) > p2align(*winner) {
                            losers.push((*winner, group));
                            *winner = i;
                        } else {
                            losers.push((i, group));
                        }
                    }
                    hashbrown::hash_table::Entry::Vacant(e) => {
                        e.insert((hash, hdr, i, best.len() as u32));
                        best.push(i);
                    }
                }
            }
            losers.into_iter().map(|(i, group)| (i, best[group as usize])).collect::<Vec<_>>()
        })
        .collect();

    // The winner keeps its own alignment: a loser's is no stricter.
    for fold in folds {
        for (loser, winner) in fold {
            ctx.isecs[loser as usize].replacement = winner;
        }
    }

    redirect_symbols_to_replacements(ctx);
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
/// cuts at its symbols, like ld64, but merges each atom with identical
/// ones whatever labels it: every object that spells @"é" has its own
/// copy, and so its own CFString, which merges only once the strings
/// have (iTerm2's debug dylib had 67 CFStrings too many).
fn is_mergeable_literal(hdr: &MachSection, isec: &InputSection) -> bool {
    if isec.nrels != 0 {
        return false;
    }
    match hdr.section_type() {
        S_CSTRING_LITERALS => !is_unterminated_string(hdr, isec),
        S_4BYTE_LITERALS => hdr.segname_is("__TEXT") && hdr.sectname_is("__literal4"),
        S_8BYTE_LITERALS => hdr.segname_is("__TEXT") && hdr.sectname_is("__literal8"),
        S_16BYTE_LITERALS => hdr.segname_is("__TEXT") && hdr.sectname_is("__literal16"),
        S_REGULAR => hdr.segname_is("__TEXT") && hdr.sectname_is("__ustring"),
        _ => false,
    }
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
/// check_duplicate_symbols. As in ld-prime, a final link reports them
/// only if no symbol is undefined, and only of the symbols -dead_strip
/// leaves live.
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
                let sym = &ctx.symbols[sym_id];
                match sym.file() {
                    Some(FileId::Obj(owner))
                        if owner as usize != obj_idx
                            && sym
                                .input_section()
                                .is_none_or(|i| ctx.isecs[i as usize].is_alive()) =>
                    {
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
    let mut undef: Vec<usize> = (0..ctx.symbols.syms.len())
        .into_par_iter()
        .filter(|&i| ctx.symbols.syms[i].is_used() && !ctx.symbols.syms[i].is_defined())
        .collect();
    if undef.is_empty() {
        return;
    }
    // ld-prime reports them by name.
    undef.par_sort_unstable_by_key(|&i| ctx.symbols.syms[i].name().as_bytes());
    let referenced = referenced_symbols(ctx);
    // A name the command line insists on must resolve: ld-prime reports
    // one even under -undefined dynamic_lookup or -U, as wanted by its
    // "<initial-undefines>" (-u, the entry point, a name an export list
    // gives without wildcards) or by the alias in its
    // "command-line-aliases-file" (an -alias base) - unless -dead_strip
    // strips the alias, which only an export root survives. The alias
    // itself counts as defined. The lazy dylibs' __dyld_lazy_load is
    // none: ld-prime wants it from its "<lazy-load-undefs>", as an
    // ordinary reference, which a kext's dynamic lookup lets stay.
    let lazy_load = ctx.symbols.get("__dyld_lazy_load").filter(|_| ctx.args.lazy_load);
    let mut initial: hashbrown::HashMap<crate::symbol::SymbolId, &str> = hashbrown::HashMap::new();
    let entry = ctx.args.has_entry_point().then_some(&ctx.args.entry);
    for name in ctx.args.forced_undefined.iter().chain(entry) {
        if let Some(id) = ctx.symbols.get(name)
            && Some(id) != lazy_load
        {
            initial.insert(id, "<initial-undefines>");
        }
    }
    let mut aliases = hashbrown::HashSet::new();
    for (base, alias) in &ctx.args.aliases {
        let live = !ctx.args.dead_strip
            || (crate::dead_strip::keeps_export(ctx, alias)
                && ctx.args.unexported_symbols.find(alias.as_bytes()) == -1);
        if let Some(id) = ctx.symbols.get(base) {
            let place = if live { "command-line-aliases-file" } else { "<initial-undefines>" };
            initial.entry(id).or_insert(place);
        }
        aliases.extend(ctx.symbols.get(alias));
    }

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
            None if Some(id) == lazy_load => "<lazy-load-undefs>".to_string(),
            None => initial.get(&id).copied().unwrap_or("<synthesized>").to_string(),
        }
    };

    for i in undef {
        let sym = &ctx.symbols[i];
        if referenced[i].load(Ordering::Relaxed)
            && !aliases.contains(&(i as crate::symbol::SymbolId))
        {
            // A -static image has no dyld to look a symbol up at run
            // time, so ld-prime lets none stay undefined, whatever
            // -undefined or -U say.
            let allowed = !ctx.args.static_link
                && (ctx.args.undefined_dynamic_lookup
                    || ctx.args.allowed_undefined.iter().any(|n| n == sym.name()))
                && !initial.contains_key(&(i as crate::symbol::SymbolId));
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
/// relocation, the personality of a live function's unwind record or of
/// its FDE's CIE, an initializer __init_offsets names (whose pointer is
/// gone), or a name -u, -e or -alias insists on.
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
    // A personality is named by an unwind record or a CIE rather than by
    // a relocation of a subsection, and an undefined one is looked up at
    // run time under -undefined dynamic_lookup like any other import.
    let alive = |isec: u32| ctx.isecs[isec as usize].is_alive();
    let personalities = ctx
        .unwind_records
        .iter()
        .filter(|rec| alive(rec.isec))
        .filter_map(|rec| rec.personality())
        .chain(
            ctx.fdes
                .iter()
                .filter(|fde| alive(fde.isec))
                .filter_map(|fde| ctx.cies[fde.cie as usize].personality),
        );
    for id in personalities {
        referenced[id as usize].store(true, Ordering::Relaxed);
    }
    for &func in &ctx.init_offsets.init_funcs {
        if let InitFunc::Imported(id) = func {
            referenced[id as usize].store(true, Ordering::Relaxed);
        }
    }
    for name in ctx
        .args
        .forced_undefined
        .iter()
        .chain(ctx.args.has_entry_point().then_some(&ctx.args.entry))
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
/// (by its install name if the stub inlines it), once each - but an
/// object as often as it is loaded - as ld-prime does. ld-prime prints
/// them in an order that varies from run to run.
pub fn print_trace<E: Target>(ctx: &Context<E>) {
    if !ctx.args.trace {
        return;
    }
    let objects: std::collections::HashSet<String> = ctx
        .objs
        .iter()
        .filter(|obj| obj.mf.parent.is_none())
        .map(|obj| input_files::trace_name(path_bytes(&obj.mf.name)))
        .collect();
    let mut seen = std::collections::HashSet::new();
    for name in &ctx.traced_files {
        if objects.contains(name) || seen.insert(name) {
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
            crate::error::notice(format_args!(
                "{option} caused load of {}",
                resolved_file_name(obj.mf)
            ));
        }
    }
    for (idx, obj) in members() {
        if let Some(name) = ctx.why_load.get(&idx) {
            crate::error::notice(format_args!(
                "'{name}' caused load of {}",
                resolved_file_name(obj.mf)
            ));
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
        .filter(|d| !d.is_bundle_loader && !d.is_lazy)
        .filter(|d| !crate::cmdline::in_shared_cache_path(&d.install_name))
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

/// True if a dylib's exports bound here moved to older libraries
/// ($ld$previous): ld-prime lists a library under the install names
/// bound to it, so one all of whose bound exports moved loses its load
/// command, named or not; libc++ does to libc++abi for macOS 13 if only
/// char8_t's type_info binds. (It drops a -needed_* or -reexport_*
/// library alike, not what the option asks for; those stay.)
fn exports_moved_away<E: Target>(ctx: &Context<E>, dylib: &input_files::DylibFile) -> bool {
    dylib.moved_exports.iter().any(|(&name, &target)| {
        let file = ctx.symbols.get(name).and_then(|id| ctx.symbols[id].file());
        file == Some(FileId::Dylib(target as u32))
    })
}

/// Makes the exports that moved from a weakly loaded dylib to an older
/// library weak imports, though the older one loads as its own imports
/// say: ld-prime weak-imports the 39 symbols iTerm2 binds to
/// libswiftNetwork, since Network loads weakly (its two direct imports
/// are weak), yet loads libswiftNetwork strongly.
fn weaken_moved_imports<E: Target>(ctx: &mut Context<E>) {
    for dylib in ctx.dylibs.iter().filter(|d| d.is_weak) {
        for (&name, &target) in &dylib.moved_exports {
            if let Some(id) = ctx.symbols.get(name)
                && ctx.symbols[id].file() == Some(FileId::Dylib(target as u32))
            {
                ctx.symbols[id].set_is_weak_ref(true);
            }
        }
    }
}

/// For each dylib with a load command that stands for a library exports
/// moved to (see add_moved_dylibs), the library of the link with its
/// install name and a load command too, which then speaks for both:
/// ld-prime binds AppKit's moved exports to libswiftAppKit 1.0.0 for
/// macOS 13, but to 2775.10.103 if -lswiftAppKit names the SDK's stub
/// as well, while an auto-link option's stub with nothing bound to it
/// has no load command and changes nothing.
fn moved_dylib_twins<E: Target>(ctx: &Context<E>, used: &[bool]) -> Vec<Option<usize>> {
    use crate::input_files::NameSource;
    let dylibs = &ctx.dylibs;
    (0..dylibs.len())
        .map(|i| {
            if !used[i] || dylibs[i].name_source != NameSource::Moved {
                return None;
            }
            (0..dylibs.len()).find(|&j| {
                used[j]
                    && dylibs[j].name_source != NameSource::Moved
                    && dylibs[j].install_name == dylibs[i].install_name
            })
        })
        .collect()
}

/// Drops load commands for dylibs no symbol binds to
/// (-dead_strip_dylibs). Bind records name dylibs by their 1-based
/// load-command ordinal, so surviving dylibs are renumbered and symbol
/// origins remapped.
pub fn dead_strip_dylibs<E: Target>(ctx: &mut Context<E>) {
    warn_unused_dylibs(ctx);
    // An auto-linked dylib is stripped even without -dead_strip_dylibs,
    // as ld64 treats its option as a hint: NetNewsWire's auto-link
    // options name 43 frameworks and Swift overlays nothing in it binds
    // to, and ld-prime lists none of them. (ld-prime ignores
    // MH_DEAD_STRIPPABLE_DYLIB, with which ld64 stripped a dylib too.)
    let strippable = |dylib: &crate::input_files::DylibFile| {
        ctx.args.dead_strip_dylibs || dylib.is_autolinked || dylib.is_implicit
    };

    // libSystem stays whether or not anything binds to it: ld-prime
    // keeps it under -dead_strip_dylibs in an image that binds nothing
    // from it (dyld needs it to run anything), so a dylib exporting
    // only its own functions still lists it.
    let mut used = vec![false; ctx.dylibs.len()];
    for (i, dylib) in ctx.dylibs.iter().enumerate() {
        used[i] = dylib.is_needed
            || dylib.install_name == b"/usr/lib/libSystem.B.dylib"
            || !strippable(dylib) && (dylib.is_reexported || !exports_moved_away(ctx, dylib));
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
    let twins = moved_dylib_twins(ctx, &used);
    for (i, &twin) in twins.iter().enumerate() {
        if let Some(twin) = twin {
            used[i] = false;
            bound[twin] += bound[i];
            weak[twin] += weak[i];
        }
    }
    for (i, dylib) in ctx.dylibs.iter_mut().enumerate() {
        if bound[i] > 0 && weak[i] == bound[i] {
            dylib.is_weak = true;
        }
    }
    weaken_moved_imports(ctx);

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
    for (i, &twin) in twins.iter().enumerate() {
        if let Some(twin) = twin {
            remap[i] = remap[twin];
        }
    }

    for sym in &mut ctx.symbols.syms {
        if let Some(FileId::Dylib(idx)) = sym.file()
            && idx != u32::MAX
        {
            sym.set_file(FileId::Dylib(remap[idx as usize] as u32));
        }
    }
    for dylib in &mut ctx.dylibs {
        for target in dylib.moved_exports.values_mut() {
            *target = remap[*target];
        }
    }

    // Ordinals (and so the load commands) in ld64's order: the
    // libraries named on the command line or by auto-link options in
    // naming order, then the implicitly loaded ones by install name. A
    // lazy dylib has neither: its imports' n_desc names the image
    // itself (ordinal 0), as ld-prime writes it.
    for dylib in ctx.dylibs.iter_mut().filter(|d| d.is_lazy) {
        dylib.dylib_idx = 0;
    }
    let mut order: Vec<usize> = (0..ctx.dylibs.len())
        .filter(|&i| !ctx.dylibs[i].is_bundle_loader && !ctx.dylibs[i].is_lazy)
        .collect();
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
/// loader included, will do, libSystem or not, but a lazy one, which
/// has no load command, won't. ld-prime does the same
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
    if !dynamic || ctx.dylibs.iter().any(|d| !d.is_lazy) {
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

/// Sets the address-taken bit of every subsection whose address the
/// image may observe, which identical code folding must then keep
/// apart - mold's --icf=safe. Mach-O objects carry no address
/// significance table, so, as mold infers it without one, anything but
/// a branch takes the address of what it refers to: an adrp/add pair,
/// a GOT load, a pointer or a difference in data. ld-prime counts only
/// the references the output keeps, after dead stripping. An exported
/// symbol's address is observable by any image that imports it.
pub fn compute_address_significance<E: Target>(ctx: &mut Context<E>) {
    let ctx_ref: &Context<E> = ctx;
    ctx_ref
        .isecs
        .par_iter()
        .filter(|isec| isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT)
        .for_each(|isec| {
            for r in input_files::isec_relocs_of(&ctx_ref.objs, isec) {
                if E::classify_reloc(r.r_type) != RelocClass::Branch
                    && let Some(dst) = ctx_ref.reloc_target_isec(isec.file as usize, r)
                {
                    ctx_ref.isecs[ctx_ref.resolve_isec(dst)].set_address_taken();
                }
            }
        });

    ctx_ref.symbols.syms.par_iter().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && !sym.is_private_extern()
            && let Some(isec) = sym.input_section()
        {
            ctx_ref.isecs[ctx_ref.resolve_isec(isec as usize)].set_address_taken();
        }
    });
}

/// Decides which symbols need a stub or a GOT slot, from how relocations
/// refer to them. Only the relocations of subsections the output keeps
/// count: not those of a copy merged into another, such as a losing
/// weak definition. Swift's symbolic type references are weak, and the
/// copy in the object defining the type refers to its descriptor
/// directly while every other object's goes through a GOT slot.
pub fn scan_relocations<E: Target>(ctx: &mut Context<E>) {
    // Classification reads only; collect it on all cores. The apply
    // loop below stays serial so GOT and stub slots keep their
    // deterministic first-seen order.
    let ctx_ref: &Context<E> = ctx;
    let classes: Vec<(crate::symbol::SymbolId, RelocClass)> = ctx_ref
        .isecs
        .par_iter()
        .filter(|isec| isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT)
        .flat_map_iter(|isec| {
            crate::input_files::isec_relocs_of(&ctx_ref.objs, isec).iter().filter_map(move |rel| {
                let id = ctx_ref.reloc_target_sym(isec.file as usize, rel)?;
                // A GOT load of a local symbol needs no slot at all:
                // it relaxes, or ld-prime refuses the instruction.
                let mut class = E::classify_reloc(rel.r_type);
                // A one-byte branch (x86-64's jmp rel8) reaches only
                // code near it, so it takes no stub: one to an import
                // is a fixup error, as in ld-prime.
                if class == RelocClass::Branch && rel.size == 1 {
                    class = RelocClass::Plain;
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

    // A lazily loaded dylib's symbols take no stub or GOT slot; the
    // image reaches them through create_lazy_loads's helpers.
    let has_lazy = ctx.dylibs.iter().any(|d| d.is_lazy);
    for (id, class) in classes {
        if has_lazy && ctx.is_lazy_import(id) {
            continue;
        }
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
            RelocClass::Branch if ctx.binds_as_import(id) => {
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

/// Replaces input subsections, each an 8-byte pointer to a symbol, by
/// the symbol's GOT entry: each by a synthetic subsection standing for
/// the entry, placed once __got is, so what refers to the input slot
/// reads the entry. A stand-in is not alive: the __got chunk writes the
/// slot, and the input slot's local symbol is not emitted. `slots`
/// pairs an input slot with its symbol.
pub(crate) fn absorb_got_slots<E: Target>(
    ctx: &mut Context<E>,
    slots: Vec<(u32, crate::symbol::SymbolId)>,
) {
    if slots.is_empty() {
        return;
    }
    let sect = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__got"),
        segname: str_to_name(data_seg(ctx)),
        p2align: 3,
        flags: S_NON_LAZY_SYMBOL_POINTERS,
        ..Default::default()
    });
    for (slot, id) in slots {
        add_got(ctx, id);
        let synth = crate::objc::add_slot_stand_in(ctx, sect);
        ctx.isecs[slot as usize].replacement = synth;
        ctx.got.stand_ins.push((synth, id));
    }
}

/// Moves the slots of each input __DATA,__got into the GOT. ld-prime
/// reads such a section, whatever its type, as non-lazy pointers, and
/// makes each slot an entry of its own __got, named in the indirect
/// symbol table; mold makes each the entry of the symbol it points at,
/// which a load through the GOT may share. A slot that is no plain
/// pointer to a symbol (one with an addend, or to a place in a
/// section) keeps its bytes and relocation in a slot of its own after
/// the symbols' (see GotSection::input_slots), as ld-prime's does; so
/// does a constant, on which ld-prime crashes.
pub fn fold_input_got<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }
    let is_got = |hdr: &MachSection| hdr.segname() == "__DATA" && hdr.sectname() == "__got";
    let mut slots = Vec::new();
    let mut input_slots = Vec::new();
    for obj in ctx.objs.iter().filter(|obj| obj.is_alive) {
        if !obj.sect_hdrs.iter().any(is_got) {
            continue;
        }
        for &i in &obj.subsecs {
            let isec = &ctx.isecs[i];
            if !isec.is_alive()
                || isec.replacement != crate::input_sections::NO_REPLACEMENT
                || !is_got(ctx.hdr_of(isec))
            {
                continue;
            }
            match pointer_target(ctx, i as usize) {
                Some(idx) => slots.push((i, obj.symbols[idx as usize])),
                None => input_slots.push(i),
            }
        }
    }
    absorb_got_slots(ctx, slots);
    for &i in &input_slots {
        ctx.isecs[i as usize].set_placed();
    }
    ctx.got.input_slots = input_slots;
}

/// The symbol, by its index in the object, that subsection `i` is a
/// pointer to: 8 bytes an 8-byte absolute relocation of the symbol
/// fills, with no addend.
pub(crate) fn pointer_target<E: Target>(ctx: &Context<E>, i: usize) -> Option<u32> {
    let [rel] = ctx.isec_relocs(i) else { return None };
    let RelocTarget::Sym(idx) = rel.target() else { return None };
    let plain = E::classify_reloc(rel.r_type) == RelocClass::Plain
        && ctx.isecs[i].size == 8
        && rel.size == 8
        && !rel.is_pcrel
        && !rel.is_subtracted
        && rel.addend == 0;
    plain.then_some(idx)
}

/// Makes what the image reaches the symbols of the dylibs dyld loads
/// lazily through (-lazy-l and the like, from macOS 27 on), as
/// ld-prime does. Such a dylib has no LC_LOAD_DYLIB: it has an
/// LC_LAZY_LOAD_DYLIB_INFO record (see chunks::lazy_load_info) naming
/// it, a flag word and the symbols the image uses from it, each with a
/// __lazy_load_got slot. Calls go to a helper per symbol that jumps
/// through the slot once the flag says the dylib is loaded, and first
/// has __dyld_lazy_load (libdyld's, called through a stub as any
/// import) load it and bind the slots; a GOT load calls a helper that
/// goes through the slot likewise (see LazyUse). ld-prime refuses any
/// other reference, such as a pointer in data, which dyld would have
/// to bind at launch; the error names the fixup as it does.
pub fn create_lazy_loads<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.lazy_load {
        ctx.lazy_helpers.keep_alive = add_keep_alive_atom(ctx);
    }
    if !ctx.dylibs.iter().any(|d| d.is_lazy) {
        return;
    }
    // (The keep-alive atom's reference, as ld-prime names it.)
    if let Some(id) = ctx.symbols.get("__dyld_lazy_load")
        && ctx.is_lazy_import(id)
    {
        error!("keepAlive use of '__dyld_lazy_load' in 'anon' cannot be lazy loaded.");
    }
    let uses = lazy_uses(ctx);
    let (flags, slots) = create_lazy_load_slots(ctx, &uses);
    create_lazy_helpers(ctx, &uses, &flags, &slots);

    // The helpers call __dyld_lazy_load through its stub.
    if !ctx.lazy_helpers.helpers.is_empty()
        && let Some(id) = ctx.symbols.get("__dyld_lazy_load")
    {
        add_stub(ctx, id);
        if ctx.lazy_binding() {
            ensure_stub_binder(ctx);
        } else {
            add_got(ctx, id);
        }
        ctx.lazy_helpers.dyld_lazy_load = Some(id);
    }
}

/// ld-prime keeps __dyld_lazy_load alive, in any link that names a
/// lazy dylib, by a reference from an empty atom it appends to __text:
/// it has an entry of its own in __unwind_info (encoding 0), and in
/// -map. Returns its subsection.
fn add_keep_alive_atom<E: Target>(ctx: &mut Context<E>) -> u32 {
    let flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__text"),
        segname: str_to_name("__TEXT"),
        flags,
        ..Default::default()
    });
    ctx.isecs.push(InputSection {
        file,
        shndx,
        p2align: 0,
        input_addr: 0,
        size: 0,
        contents: 0,
        rel_offset: 0,
        nrels: 0,
        output_section: u32::MAX,
        offset: 0,
        flags: InputSection::flags_alive_no_modulus(),
        replacement: crate::input_sections::NO_REPLACEMENT,
        unwind_offset: 0,
        nunwind: 0,
    });
    (ctx.isecs.len() - 1) as u32
}

/// A reference to a lazy dylib's symbol: the subsection, the
/// relocation's offset in it, the symbol, and how it refers to it.
type LazyUseSite = (u32, u32, crate::symbol::SymbolId, crate::target::LazyRef);

/// A __lazy_load_got slot: its symbol, and whether it is the one the
/// symbol's call helper has to itself (see Target::LAZY_CALL_OWN_SLOT).
type LazySlot = (crate::symbol::SymbolId, bool);

/// The references to lazy dylibs' symbols from live subsections, in
/// input order, once the ones ld-prime refuses are reported.
fn lazy_uses<E: Target>(ctx: &Context<E>) -> Vec<LazyUseSite> {
    let uses: Vec<LazyUseSite> = (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| {
            let isec = &ctx.isecs[i];
            isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT
        })
        .flat_map_iter(|i| {
            let (file, data) = (ctx.isecs[i].file as usize, ctx.isecs[i].data());
            ctx.isec_relocs(i).iter().filter_map(move |r| {
                let id = ctx.reloc_target_sym(file, r)?;
                ctx.is_lazy_import(id).then(|| (i as u32, r.offset, id, E::lazy_ref(r, data)))
            })
        })
        .collect();
    for &(isec, _, id, how) in &uses {
        if let crate::target::LazyRef::Unsupported(kind) = how {
            let sym = &ctx.symbols[id];
            let sec = &ctx.isecs[isec as usize];
            let split = ctx.objs[sec.file as usize].subsections_via_symbols;
            let atom = if input_files::is_record_list(ctx.hdr_of(sec), split) {
                "anon".into()
            } else {
                ctx.atom_name(isec as usize)
            };
            error!("{kind} use of '{sym}' in '{atom}' cannot be lazy loaded.");
        }
    }
    // A stub or GOT slot another pass made for one (an unwind
    // personality's, an input __got's) would be a pointer dyld binds at
    // launch, and is refused alike. (ld-prime leaves a personality's
    // slot zero.)
    for &id in ctx.stubs.symbols.iter().chain(&ctx.got.got_syms) {
        if ctx.is_lazy_import(id) {
            error!("ptr64 use of '{}' in 'anon' cannot be lazy loaded.", ctx.symbols[id]);
        }
    }
    crate::error::checkpoint();
    uses
}

/// The slot a use of a symbol goes through.
fn lazy_slot<E: Target>(id: crate::symbol::SymbolId, how: crate::target::LazyRef) -> LazySlot {
    (id, E::LAZY_CALL_OWN_SLOT && how == crate::target::LazyRef::Call)
}

/// Gives each lazy dylib the image uses its flag word, its symbols
/// their __lazy_load_got slots and it its record. Returns each dylib's
/// flag word's subsection, and each slot's index.
fn create_lazy_load_slots<E: Target>(
    ctx: &mut Context<E>,
    uses: &[LazyUseSite],
) -> (Vec<u32>, hashbrown::HashMap<LazySlot, u32>) {
    use crate::chunks::lazy_load_info::{LazyDylib, record_size};

    // Each dylib's slots by name, the dylibs in load order: the order of
    // the slots.
    let mut by_dylib: Vec<Vec<LazySlot>> = vec![Vec::new(); ctx.dylibs.len()];
    for &(_, _, id, how) in uses {
        if let Some(FileId::Dylib(d)) = ctx.symbols[id].file() {
            by_dylib[d as usize].push(lazy_slot::<E>(id, how));
        }
    }
    for list in &mut by_dylib {
        list.sort_unstable_by_key(|&(id, own)| (ctx.symbols[id].name(), own));
        list.dedup();
    }
    let mut used: Vec<usize> = (0..ctx.dylibs.len()).filter(|&d| !by_dylib[d].is_empty()).collect();
    used.sort_by(|&a, &b| {
        let (da, db) = (&ctx.dylibs[a], &ctx.dylibs[b]);
        da.load_order.cmp(&db.load_order).then_with(|| da.install_name.cmp(&db.install_name))
    });

    // The flag words, by install name, ahead of __dyld_private.
    let mut flags = vec![u32::MAX; ctx.dylibs.len()];
    let mut by_name = used.clone();
    by_name.sort_by(|&a, &b| ctx.dylibs[a].install_name.cmp(&ctx.dylibs[b].install_name));
    for d in by_name {
        let isec = add_data_word(ctx, 4);
        let install_name = &ctx.dylibs[d].install_name;
        let leaf = install_name.rsplit(|&c| c == b'/').next().unwrap_or(install_name);
        let name = format!("_lazyLoadFlag${}", String::from_utf8_lossy(leaf));
        ctx.extra_local_syms.push((String::leak(name), isec));
        flags[d] = isec;
    }
    let private = ctx.stub_helper.dyld_private_isec;
    if let Some(i) = ctx.data_blobs.iter().position(|b| b.isec == private) {
        let blob = ctx.data_blobs.remove(i);
        ctx.data_blobs.push(blob);
    }

    let mut index = hashbrown::HashMap::new();
    let mut slots = Vec::new();
    for &d in &used {
        let got_start = slots.len() as u32;
        let list = std::mem::take(&mut by_dylib[d]);
        for &(id, own) in &list {
            index.insert((id, own), slots.len() as u32);
            // The slot an arm64 ldr loads (see LazyRef::Slot).
            if !own {
                ctx.sym_aux_mut(id).lazy_got_idx = slots.len() as u32;
            }
            let name: &str = String::leak(format!("{}$lazyGOT", ctx.symbols[id].name()));
            slots.push((id, name));
        }
        let syms: Vec<_> = list.into_iter().map(|(id, _)| id).collect();
        let (flag, size) = (flags[d], record_size(ctx, &ctx.dylibs[d].install_name, &syms));
        let dylib = d as u32;
        ctx.lazy_load_info.dylibs.push(LazyDylib { dylib, flag, syms, got_start, offset: 0, size });
    }
    ctx.lazy_load_got.slots = slots;
    // The records go in the reverse order.
    let mut offset = 0;
    for d in ctx.lazy_load_info.dylibs.iter_mut().rev() {
        d.offset = offset;
        offset += d.size;
    }
    ctx.lazy_load_info.hdr.size = offset as u64;
    (flags, index)
}

/// Makes the helpers, laid out by name: one per symbol for calls, and
/// one per symbol and register for GOT loads, or per load in arm64
/// frameless code.
fn create_lazy_helpers<E: Target>(
    ctx: &mut Context<E>,
    uses: &[LazyUseSite],
    flags: &[u32],
    slots: &hashbrown::HashMap<LazySlot, u32>,
) {
    use crate::chunks::lazy_helpers::{LazyHelper, LazyUse};
    use crate::target::LazyRef;

    let mut helpers: Vec<LazyHelper> = Vec::new();
    let mut index: hashbrown::HashMap<(crate::symbol::SymbolId, LazyUse), usize> =
        hashbrown::HashMap::new();
    let mut sites = Vec::new();
    for &(isec, offset, id, how) in uses {
        let kind = match how {
            LazyRef::Call => LazyUse::Call,
            LazyRef::Cmp => LazyUse::Cmp,
            LazyRef::Load => {
                let (reg, own) = E::lazy_load_site(ctx.isecs[isec as usize].data(), offset);
                LazyUse::Load { reg, site: own.then_some((isec, offset)) }
            }
            LazyRef::Slot | LazyRef::Unsupported(_) => continue,
        };
        let i = *index.entry((id, kind)).or_insert_with(|| {
            let sym = ctx.symbols[id].name();
            let name = match kind {
                LazyUse::Call => format!("{sym}$lazyLoadStub"),
                LazyUse::Cmp => format!("{sym}$lazyGOT$cmpHelper"),
                LazyUse::Load { reg, site: None } => {
                    format!("{sym}$lazyGOT$loadHelper_{}", E::lazy_register_name(reg))
                }
                LazyUse::Load { reg, site: Some(_) } => format!(
                    "{sym}$lazyGOT$loadHelper_{}$for${}+{offset}",
                    E::lazy_register_name(reg),
                    ctx.atom_name(isec as usize)
                ),
            };
            let Some(FileId::Dylib(d)) = ctx.symbols[id].file() else { unreachable!() };
            let (flag, slot) = (flags[d as usize], slots[&lazy_slot::<E>(id, how)]);
            let name = String::leak(name);
            helpers.push(LazyHelper { sym: id, kind, name, flag, slot, offset: 0 });
            helpers.len() - 1
        });
        if how != LazyRef::Call {
            sites.push(((isec, offset), i));
        }
    }

    let mut sorted: Vec<(usize, LazyHelper)> = helpers.into_iter().enumerate().collect();
    sorted.sort_by_key(|(_, h)| h.name);
    let mut rank = vec![0; sorted.len()];
    let mut offset = 0;
    for (r, (i, h)) in sorted.iter_mut().enumerate() {
        rank[*i] = r as u32;
        h.offset = offset;
        offset += E::lazy_helper_size(h.kind);
        if h.kind == LazyUse::Call {
            ctx.sym_aux_mut(h.sym).lazy_stub_idx = r as u32;
        }
    }
    ctx.lazy_helpers.sites = sites.into_iter().map(|(site, i)| (site, rank[i])).collect();
    ctx.lazy_helpers.helpers = sorted.into_iter().map(|(_, h)| h).collect();
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

    // A dylib, a bundle or dyld may find its own mach header by a name
    // for its kind, which ld-prime defines as it does ___dso_handle
    // below, out of the symbol table.
    let header_name = match ctx.args.output_type {
        MH_DYLIB => Some("__mh_dylib_header"),
        MH_BUNDLE => Some("__mh_bundle_header"),
        MH_DYLINKER => Some("__mh_dylinker_header"),
        _ => None,
    };
    if let Some(name) = header_name {
        define_header_alias(ctx, name, internal, header_addr);
    }

    // ___dso_handle identifies the image; C++ static destructors pass it
    // to __cxa_atexit. It resolves to the mach header but is never
    // exported.
    define_header_alias(ctx, "___dso_handle", internal, header_addr);

    // -alias gives an existing definition a second name: the new
    // symbol shares the original's subsection and offset, so it lands
    // at the same address and is exported alongside it. Apple uses
    // aliases to publish compatibility names (e.g. libSystem's dozens
    // of $VARIANT names) without touching the source. An undefined
    // base is reported with the other undefined symbols.
    let aliases = std::mem::take(&mut ctx.args.aliases);
    for (existing, new) in &aliases {
        let Some(src) = ctx.symbols.get(existing).filter(|&id| ctx.symbols[id].is_defined()) else {
            continue;
        };
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

/// Defines `name` at the mach header unless an input does, as a local
/// symbol of the internal object, which the symbol table leaves out.
fn define_header_alias<E: Target>(
    ctx: &mut Context<E>,
    name: &'static str,
    internal: u32,
    addr: u64,
) {
    let id = ctx.symbols.intern(name);
    let sym = &mut ctx.symbols[id];
    if !sym.is_defined() {
        sym.set_file(FileId::Obj(internal));
        sym.value = addr;
        sym.set_is_extern(false);
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

/// The mach header's address, the start of its segment: where -segaddr
/// pins that segment, or else the image base.
fn mach_header_addr<E: Target>(ctx: &Context<E>) -> u64 {
    ctx.args.segaddr(header_segment(ctx)).unwrap_or(ctx.image_base())
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

/// Whether a section is an Objective-C list whose entries ld-prime
/// names no symbol for: __DATA's __objc_classlist, __objc_nlclslist,
/// __objc_catlist, __objc_catlist2 and __objc_nlcatlist, and
/// __objc_clsrolist, which only a -r output keeps.
pub(crate) fn is_unnamed_objc_list(hdr: &MachSection) -> bool {
    hdr.segname_is("__DATA")
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
        .any(|name| hdr.sectname_is(name))
}

/// The symbols of an object, by index, that name an entry of an
/// Objective-C list ld-prime names no symbol for (see
/// is_unnamed_objc_list) and survive all the same. ld-prime takes one
/// of an entry's symbols for the name of its atom, which is lost, and
/// keeps the others as aliases: an external one (a global or a private
/// external, both demoted by then, see demote_unnamed_atom_names) is
/// the name, else the greatest name; the arm64
/// assembler's ltmpN labels don't count. So an entry one label names
/// has no symbol in the output (clang's l_OBJC_LABEL_CLASS_$, Swift's
/// _objc_classes_...), but where a private external names it too, the
/// label stays and the private external goes.
pub(crate) fn objc_list_aliases<E: Target>(
    ctx: &Context<E>,
    obj: &crate::input_files::ObjectFile,
) -> hashbrown::HashSet<usize> {
    let mut aliases = hashbrown::HashSet::new();
    let lists: Vec<bool> = obj.sect_hdrs.iter().map(is_unnamed_objc_list).collect();
    if !lists.contains(&true) {
        return aliases;
    }
    // (place, is external, name, index), sorted so that each place's
    // name comes last.
    let mut syms: Vec<((u8, u64), bool, &str, usize)> = obj
        .nlists
        .iter()
        .zip(&obj.symbols)
        .enumerate()
        .filter_map(|(i, (nlist, &id))| {
            if nlist.is_stab() || nlist.n_type() != N_SECT || !lists[nlist.n_sect as usize - 1] {
                return None;
            }
            let name = ctx.symbols[id].name();
            let place = (nlist.n_sect, nlist.n_value);
            let external = nlist.n_type & (N_EXT | N_PEXT) != 0;
            (!name.starts_with("ltmp")).then_some((place, external, name, i))
        })
        .collect();
    syms.sort_unstable();
    for w in syms.windows(2) {
        if w[0].0 == w[1].0 {
            aliases.insert(w[0].3);
        }
    }
    aliases
}

/// Whether ld-prime makes a section's atoms by content and names none
/// of them: CFStrings, selector and class references, UTF-16 literals
/// and Objective-C constant literals (@42, @[...], @{...}). No label
/// of theirs is in an output's symbol table; a -r output names the
/// atoms itself on arm64 (see relocatable.rs). Superclass and protocol
/// references of the literal-pointer type are taken for class
/// references too, which merge whatever labels them (see
/// is_class_or_protocol_ref). In an object without subsections
/// (`split` false) the UTF-16 literals' section is one atom, whose
/// labels ld-prime keeps as any other's.
pub(crate) fn has_unnamed_atoms(hdr: &MachSection, split: bool) -> bool {
    if hdr.segname_is("__TEXT") {
        return split && hdr.sectname_is("__ustring");
    }
    hdr.segname_is("__DATA")
        && ([
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
            || (hdr.section_type() == S_LITERAL_POINTERS && is_class_or_protocol_ref(hdr)))
}

/// Whether a section holds superclass or protocol references,
/// __DATA,__objc_superrefs or __objc_protorefs. ld-prime cuts them one
/// per pointer and merges the unlabeled ones of one target; one a
/// symbol names stays apart and keeps its label (see
/// mark_labeled_literals), unless the section has the literal-pointer
/// type, which merges them all (see has_unnamed_atoms).
pub(crate) fn is_class_or_protocol_ref(hdr: &MachSection) -> bool {
    hdr.segname_is("__DATA") && is_class_or_protocol_ref_name(hdr.sectname())
}

pub(crate) fn is_class_or_protocol_ref_name(sectname: &str) -> bool {
    matches!(sectname, "__objc_superrefs" | "__objc_protorefs")
}

/// -pagezero_size, as ld-prime takes it: rounded up to a page (past the
/// top, to 0), and no more than 4 GiB in an executable with chained
/// fixups.
pub fn resolve_pagezero_size<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }
    let size = ctx.args.pagezero_size;
    let page = ctx.segment_align();
    if !size.is_multiple_of(page) {
        let aligned = page_align(size, page);
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
        let aligned = page_align(base, align);
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

    // Range-extension thunks go in before the first placement, unless
    // only the placement can tell whether a branch may be out of reach;
    // the code is then placed again with them if it turns out so.
    crate::thunks::warn_large_atoms(ctx);
    let need_thunks = crate::thunks::need_thunks(ctx);
    if need_thunks == Some(true) {
        crate::thunks::create_range_extension_thunks(ctx);
    }
    let mut fileoff = lay_out_segments(ctx);
    while !finish_unwind_info(ctx) {
        fileoff = lay_out_segments(ctx);
    }
    if need_thunks.is_none() && crate::thunks::code_span(ctx) > E::BRANCH_RANGE / 2 {
        crate::thunks::create_range_extension_thunks(ctx);
        fileoff = lay_out_segments(ctx);
        while !finish_unwind_info(ctx) {
            fileoff = lay_out_segments(ctx);
        }
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

    // An error ld-prime finds before it lays out __LINKEDIT ends the
    // link there: it prints the layout with __LINKEDIT unsized (see
    // unsized_linkedit_addr) - twice for one in the segments' layout.
    let misplaced = check_section_file_ends(ctx) || check_segments_in_order(ctx);
    if crate::error::has_early_layout_error() {
        ctx.segments[linkedit].cmd.vmaddr = unsized_linkedit_addr(ctx);
        ctx.segments[linkedit].cmd.fileoff = fileoff;
        print_final_layout(ctx);
        if misplaced {
            print_final_layout(ctx);
        }
        crate::error::checkpoint();
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
/// listing them as ld-prime does: output section by output section,
/// each atom's from the last to the first (the order an assembler emits
/// relocations in). Where it encodes rebase opcodes, it lists each
/// atom's in address order, and those of the first section only. An
/// unaligned pointer in a chain fails the link then instead.
fn report_text_relocs<E: Target>(ctx: &Context<E>) {
    let mut found = std::mem::take(&mut *ctx.text_relocs.lock().unwrap());
    let rebase_opcodes = !ctx.use_chained_fixups() && ctx.chunks.contains(&ChunkId::RebaseInfo);
    if rebase_opcodes {
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
            if rebase_opcodes && osec.is_some() {
                break;
            }
            crate::error::notice(format_args!("Illegal text-relocations:"));
            osec = Some(isec.output_section);
        }
        let rel = &ctx.isec_relocs(id as usize)[i as usize];
        let target = ctx.text_reloc_target_name(isec.file as usize, rel);
        crate::error::notice(format_args!(
            "  text-relocation in {} to '{target}'",
            ctx.atom_ref(id as usize, rel.offset)
        ));
    }
    if !chunks::chained_fixups::report_unaligned_chain_pointer(ctx) && osec.is_some() {
        error!("Found illegal text-relocations");
    }
}

/// Prints the image's segments and sections, in load command order, if
/// an error in its layout ends the link (see error::layout_error), as
/// ld-prime does - with its own layout's addresses, sizes and file
/// offsets, the last in 32 bits.
pub fn print_final_layout<E: Target>(ctx: &Context<E>) {
    use std::fmt::Write;
    if !crate::error::has_layout_error() {
        return;
    }
    crate::error::release_layout_errors();
    let mut out = String::from("final section layout:\n");
    for seg in &ctx.segments {
        let cmd = &seg.cmd;
        let _ = writeln!(
            out,
            "    {:<20} addr=0x{:09x}, size=0x{:09x}, fileOffset=0x{:08x}, fileSize=0x{:08x}",
            seg.name, cmd.vmaddr, cmd.vmsize, cmd.fileoff as u32, cmd.filesize as u32
        );
        for hdr in seg.chunks.iter().map(|&id| ctx.chunk_header(id)).filter(|hdr| hdr.is_sect) {
            let zerofill = hdr.is_zerofill();
            let fileoff = if zerofill { 0 } else { hdr.fileoff };
            let _ = writeln!(
                out,
                "        {:<16} addr=0x{:09x}, size=0x{:09x}, fileOffset=0x{:08x} (zerofill={})",
                hdr.sectname, hdr.addr, hdr.size, fileoff as u32, zerofill as u8
            );
        }
    }
    crate::error::notice(format_args!("{}", out.trim_end_matches('\n')));
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
    check_segment_overlaps(ctx);
    crate::error::checkpoint_in_layout();
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
    page_align(seg.cmd.vmsize, seg_page_size(ctx, seg.name))
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
    seg.cmd.filesize = page_align(filesize, page);
    seg.cmd.vmsize = page_align(vm_end - vmaddr, page).max(seg.cmd.filesize);
    seg_fileoff + page_align(seg.cmd.filesize, seg_page)
}

/// Rounds `value` up to a multiple of the page size `page` as ld-prime
/// does, which rounds anything to 0 under a -segalign of 0.
fn page_align(value: u64, page: u64) -> u64 {
    let mask = page.wrapping_sub(1);
    value.wrapping_add(mask) & !mask
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
/// - In an image dyld slides, a pinned __TEXT is no base the others
///   float from (ld-prime ignores it as a PIE's image base): every
///   pinned segment then counts as placed from the start, and a
///   segment may follow one below the base.
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
    let detached =
        dyld_slides(ctx) && header_seg.is_some_and(|seg| ctx.args.segaddr(seg).is_some());
    let first_pin = if detached { Some(0) } else { fixed.first().copied() };
    let floor = if detached { 0 } else { base };
    let header = segs.iter().position(|seg| Some(seg.name) == header_seg);
    let mut used: Vec<Range<u64>> =
        header.map(|i| range(i, segs[i].cmd.vmaddr)).into_iter().collect();
    if let Some(addr) = ctx.args.segaddr("__LINKEDIT") {
        used.push(addr..addr);
    }
    for i in 0..segs.len() {
        if first_pin == Some(i) {
            used.extend(fixed.iter().map(|&j| range(j, addrs[j].unwrap())));
        }
        if addrs[i].is_none() {
            let size = segment_span(ctx, &segs[i]);
            let align = segment_start_align(ctx, i);
            let span = lowest_free_span(base, floor, size, align, &used);
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

/// Where `size` bytes go at the lowest address where they run into none
/// of the `used` ranges: `base` itself or the end of a used range above
/// `floor`, rounded up to `align`. Returns the span from that address
/// before rounding to the end, which the rounding's padding is part of.
/// An empty segment is a point no other segment may straddle, and still
/// needs an address no segment covers.
fn lowest_free_span(
    base: u64,
    floor: u64,
    size: u64,
    align: u64,
    used: &[Range<u64>],
) -> Range<u64> {
    let is_free = |span: &Range<u64>| {
        used.iter().all(|r| r.end <= span.start || span.end.max(span.start + 1) <= r.start)
    };
    std::iter::once(base)
        .chain(used.iter().map(|r| r.end).filter(|&end| end > floor))
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
/// an -image_base inside __PAGEZERO). It reports the first such pair.
/// Left out are empty segments and __LINKEDIT, sized last.
fn check_segment_overlaps<E: Target>(ctx: &Context<E>) {
    let segs = &ctx.segments[..ctx.segments.len() - 1];
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
}

/// ld-prime keeps a section's file offset in 32 bits, and so its
/// segment's end, and refuses a section that ends past that: in a
/// segment that ends at 4 GiB, which a -segalign of 2 GiB gives one,
/// and in any with a -segalign of 0, which leaves every segment empty.
/// It is an error in the layout it finds before __LINKEDIT (see
/// error::early_layout_error), of the first such section. Returns
/// whether it found one.
fn check_section_file_ends<E: Target>(ctx: &Context<E>) -> bool {
    for seg in &ctx.segments[..ctx.segments.len() - 1] {
        let seg_end = (seg.cmd.fileoff + seg.cmd.filesize) as u32;
        for &id in &seg.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.is_sect && !hdr.is_zerofill() && hdr.fileoff + hdr.size > seg_end as u64 {
                crate::early_layout_error!(
                    "section {},{} file end ({}) goes past the segment end ({seg_end}) ",
                    hdr.segname,
                    hdr.sectname,
                    hdr.fileoff + hdr.size
                );
                return true;
            }
        }
    }
    false
}

/// In an image dyld slides, ld-prime refuses a segment below the one
/// before it - an error in the layout it finds before __LINKEDIT (see
/// error::early_layout_error). It reports the first such segment.
/// Returns whether it found one.
fn check_segments_in_order<E: Target>(ctx: &Context<E>) -> bool {
    if !dyld_slides(ctx) {
        return false;
    }
    let (linkedit, segs) = ctx.segments.split_last().unwrap();
    for pair in segs.windows(2) {
        if pair[1].cmd.vmaddr < pair[0].cmd.vmaddr {
            crate::early_layout_error!("segment {} address is out of order", pair[1].name);
            return true;
        }
    }
    if let (Some(addr), Some(last)) = (ctx.args.segaddr(linkedit.name), segs.last())
        && addr < last.cmd.vmaddr
    {
        crate::early_layout_error!("segment {} address is out of order", linkedit.name);
        return true;
    }
    false
}

/// Where ld-prime has __LINKEDIT when an error in the layout stops it
/// before sizing it: where -segaddr pins it, or else after the last
/// segment as it places them first - each one neither in place nor
/// pinned above all the ones before it, before place_segments moves it.
fn unsized_linkedit_addr<E: Target>(ctx: &Context<E>) -> u64 {
    let (linkedit, segs) = ctx.segments.split_last().unwrap();
    if let Some(addr) = ctx.args.segaddr(linkedit.name) {
        return addr;
    }
    let header_seg = in_place_segment(ctx);
    let (mut top, mut end) = (0, 0);
    for seg in segs {
        let in_place = seg.name == "__PAGEZERO" || Some(seg.name) == header_seg;
        let pinned = ctx.args.segaddr(seg.name).is_some();
        let start = if in_place || pinned { seg.cmd.vmaddr } else { top };
        end = start + segment_span(ctx, seg);
        top = top.max(end);
    }
    end
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
        let base = ctx.image_base();
        lowest_free_span(base, base, size, ctx.segment_align(), &used).start
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
                    chunks::symtab::create_output_symtab(shared, sorted_globals)
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
                            let dice = chunks::data_in_code::build(shared, |hdr| hdr.fileoff);
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

/// Resolves the entry point symbol.
pub fn resolve_entry<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.has_entry_point() {
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
    if !ctx.args.has_entry_point() {
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
    let Some(id) = bind_stub_binder(ctx) else {
        // An image that loads no dylib at all fails the libSystem
        // check that dead_strip_dylibs would make later, and ld-prime
        // says so first.
        check_libsystem_linked(ctx);
        fatal!("lazy binding needs dyld_stub_binder, which no loaded dylib exports");
    };
    ctx.symbols[id].set_is_used(true);
    add_got(ctx, id);
    ctx.stub_helper.dyld_stub_binder = Some(id);
    let isec = add_data_word(ctx, 8);
    ctx.stub_helper.dyld_private_isec = isec;
    ctx.extra_local_syms.push(("__dyld_private", isec));
}

/// Synthesizes a zero word of `size` bytes, aligned to its size, in
/// __DATA,__data (after the inputs'), and returns its subsection.
fn add_data_word<E: Target>(ctx: &mut Context<E>, size: u32) -> u32 {
    let p2align = size.trailing_zeros() as u8;
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__data"),
        segname: str_to_name("__DATA"),
        p2align: p2align as u32,
        flags: 0,
        ..Default::default()
    });
    ctx.isecs.push(InputSection {
        file,
        shndx,
        p2align,
        input_addr: 0,
        size,
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
    let fields = vec![DataField::Bytes(vec![0; size as usize])];
    ctx.data_blobs.push(DataBlob { sect: "__data", isec, fields });
    isec
}

/// Binds dyld_stub_binder to the first loaded dylib that exports it,
/// unless something in the link defines it.
fn bind_stub_binder<E: Target>(ctx: &mut Context<E>) -> Option<crate::symbol::SymbolId> {
    let name = "dyld_stub_binder";
    let dylib = ctx.dylibs.iter().position(|d| d.exports.contains(name))?;
    let id = ctx.symbols.intern(name);
    let sym = &mut ctx.symbols[id];
    if !sym.is_defined() {
        sym.set_file(FileId::Dylib(dylib as u32));
        sym.set_is_imported(true);
        sym.set_is_extern(true);
        sym.set_input_section(None);
    }
    Some(id)
}

/// ld-prime makes dyld_stub_binder an initial undefine of an image with
/// lazy binding, stubs or not, which may stay undefined: the library
/// exporting it (libSystem's libdyld) counts as used then, and -map
/// lists it. Nothing refers to the symbol until a stub does.
pub fn resolve_stub_binder<E: Target>(ctx: &mut Context<E>) {
    if ctx.lazy_binding() {
        bind_stub_binder(ctx);
    }
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
    print_final_layout(ctx);
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
