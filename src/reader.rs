//! Reading input files: finding the files the command line names, and
//! the libraries the objects' auto-link options name, and loading them
//! as objects, archives and dylibs.
//!
//! Dylibs are loaded as they are named, in command line order, which
//! decides their load commands; objects and archive members are queued
//! and parsed in parallel, then added to the link in command line order,
//! which gives each its priority for symbol resolution.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::cmdline::{Args, InputArg, LibraryKind, LibraryName};
use crate::context::Context;
use crate::error;
use crate::error::RawPath;
use crate::error::raw;
use crate::fatal;
use crate::filetype::{FileType, get_file_type};
use crate::input_files;
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::mergeable::MergedLibrary;
use crate::passes;
use crate::tapi;
use crate::target::Target;
use crate::util::path_bytes;

/// The default library search path: ld64's /usr/lib and /usr/local/lib,
/// and between them ld-prime's /usr/lib/swift, which it searches for
/// any library, not only Swift's.
const STANDARD_LIBRARY_DIRS: &[&str] = &["/usr/lib", "/usr/lib/swift", "/usr/local/lib"];

/// The default framework search path, as ld64's.
const STANDARD_FRAMEWORK_DIRS: &[&str] = &["/Library/Frameworks", "/System/Library/Frameworks"];

/// Settles the library and framework search paths, once the options
/// are checked, as ld-prime does: the -L (-F) directories, then, unless
/// -Z, the default ones (see search_dirs). -v prints the banner here and
/// -version_details its JSON; then either prints the paths on stderr,
/// as ld-prime does.
pub fn set_search_paths<E: Target>(ctx: &mut Context<E>) {
    let args = &mut ctx.args;
    if args.verbose {
        crate::cmdline::print_version();
    }
    if args.version_details {
        crate::cmdline::print_version_details();
    }
    args.library_paths = search_dirs(args, &args.library_paths, STANDARD_LIBRARY_DIRS);
    args.framework_paths = search_dirs(args, &args.framework_paths, STANDARD_FRAMEWORK_DIRS);
    if args.verbose || args.version_details {
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
/// (see push_search_dir). A directory given again, spelled the same, is
/// taken the first time only. A -syslibroot of / anywhere, which
/// configure scripts pass, puts none under a root (ld64 drops the roots
/// only for a last one); the roots still hold the files the options
/// naming a library's path look up (find_file) unless it is last.
fn search_dirs(args: &Args, dirs: &[PathBuf], standard: &[&str]) -> Vec<PathBuf> {
    let syslibroot: &[PathBuf] = if args.syslibroot.iter().any(|root| root.as_os_str() == "/") {
        &[]
    } else {
        &args.syslibroot
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs.iter().filter(|dir| seen.insert(dir.as_os_str())) {
        push_search_dir(syslibroot, &mut out, dir, true);
    }
    if !args.no_standard_dirs {
        for dir in standard {
            push_search_dir(syslibroot, &mut out, Path::new(dir), false);
        }
    }
    out
}

/// Adds a directory to a search path. ld64 looks an absolute directory
/// up under each syslibroot, keeping those that have it and falling
/// back to the directory itself - but a default directory missing from
/// the only SDK is not searched at all; one that climbs with "/.." is
/// first resolved (symbolic links too) where it can be. A relative
/// directory is never put under a syslibroot, which the compiler
/// driver always passes: `-L.` would otherwise search the SDK's root,
/// not the working directory. What is no directory is left out with a
/// warning, as is a directory the command line `given` that is not
/// there; a default directory that is not there goes without a word.
fn push_search_dir(syslibroot: &[PathBuf], dirs: &mut Vec<PathBuf>, dir: &Path, given: bool) {
    let mut dir = dir.to_path_buf();
    if dir.is_absolute() {
        if memchr::memmem::find(path_bytes(&dir), b"/..").is_some()
            && let Ok(real) = std::fs::canonicalize(&dir)
        {
            dir = real;
        }
        let len = dirs.len();
        for root in syslibroot {
            let path = under_root(root, &dir);
            match std::fs::metadata(&path) {
                Ok(md) if md.is_dir() => dirs.push(path),
                Ok(_) => crate::warn!(
                    "-syslibroot and combined search path '{}' is not a directory",
                    path.raw()
                ),
                Err(_) => {}
            }
        }
        if dirs.len() > len || (!given && syslibroot.len() == 1) {
            return;
        }
    }
    match std::fs::metadata(&dir) {
        Ok(md) if md.is_dir() => dirs.push(dir),
        Ok(_) => crate::warn!("search path '{}' is not a directory", dir.raw()),
        Err(_) if given => crate::warn!("search path '{}' not found", dir.raw()),
        Err(_) => {}
    }
}

/// An absolute path looked up under a syslibroot, as ld-prime joins
/// them: less its leading slash, the path goes below the root, but one
/// that starts with two slashes stays absolute and replaces the root.
pub(crate) fn under_root(root: &Path, path: &Path) -> PathBuf {
    let bytes = path_bytes(path);
    root.join(crate::util::os_str(bytes.strip_prefix(b"/").unwrap_or(bytes)))
}

/// Looks for files as ld-prime does in its searches for inputs, noting
/// each file it looks for and doesn't find: -dependency_info lists
/// them, so that a build system links again once one appears. A lookup
/// made ahead of time, or made again, is quiet, without a warning too.
pub struct Prober<'a> {
    missing: Option<&'a std::sync::Mutex<Vec<PathBuf>>>,
    quiet: bool,
    prefer_stubs: bool,
}

impl<'a> Prober<'a> {
    pub fn new<E: Target>(ctx: &'a Context<E>) -> Self {
        let missing = ctx.args.dependency_info.is_some().then_some(&ctx.missing_files);
        Self { missing, quiet: false, prefer_stubs: ctx.args.prefer_stubs }
    }

    pub fn quiet<E: Target>(ctx: &Context<E>) -> Self {
        Self { missing: None, quiet: true, prefer_stubs: ctx.args.prefer_stubs }
    }

    /// Whether there is a file at `path`.
    pub fn exists(&self, path: &Path) -> bool {
        let found = path.exists();
        if !found && let Some(missing) = self.missing {
            missing.lock().unwrap().push(path.to_path_buf());
        }
        found
    }

    /// The library at `path` or its stub, `path` with .tbd for its
    /// extension: ld-prime looks for both, the stub first, and takes
    /// the one there - where both are, the stub, but in an SDK, where a
    /// library has no business next to its stub: there it warns (but
    /// in Apple's internal SDK) and takes the library, unless
    /// $LD_PREFER_TAPI_FILE (Args::prefer_stubs).
    pub fn library(&self, path: &Path) -> Option<PathBuf> {
        let stub = path.with_extension("tbd");
        let has_stub = self.exists(&stub);
        let has_library = self.exists(path);
        if !has_stub {
            return has_library.then(|| path.to_path_buf());
        }
        let stub_bytes = path_bytes(&stub);
        let in_sdk = memchr::memmem::find(stub_bytes, b".sdk/").is_some();
        if !has_library || stub == path || !in_sdk || self.prefer_stubs {
            return Some(stub);
        }
        if !self.quiet && memchr::memmem::find(stub_bytes, b"/SDKs/Xcode.Internal").is_none() {
            crate::warn!(
                "text-based stub file {} and library file {} unexpectedly found. Falling back \
                 to library file for linking.",
                stub.raw(),
                path.raw()
            );
        }
        Some(path.to_path_buf())
    }
}

/// Finds -framework Name[,suffix]: Name.framework/Name (its stub
/// first, unless `stubs` is false, as for a framework to merge) in each
/// framework directory, as ld64 does. A suffix names a variant of the
/// binary the framework's Name symlink points to (Name in Versions/A
/// when Name links there) with the suffix appended, which is looked for
/// in every directory before the framework itself. -image_suffix's
/// suffixes are tried before the name without. A sparse framework, one
/// that has only its Versions/Current without the symlinks at its top,
/// is found there in a second pass over the directories if
/// -search_in_sparse_frameworks asks.
fn find_framework<E: Target>(
    ctx: &Context<E>,
    prober: &Prober,
    arg: &OsStr,
    stubs: bool,
) -> Option<PathBuf> {
    let (name, suffix) = match memchr::memchr(b',', arg.as_bytes()) {
        Some(comma) => (&arg.as_bytes()[..comma], Some(&arg.as_bytes()[comma + 1..])),
        None => (arg.as_bytes(), None),
    };
    let name = crate::util::os_str(name);
    let mut framework = name.to_os_string();
    framework.push(".framework");
    let search = |subdir: &str| {
        for suffix in [suffix, None].into_iter().take(1 + suffix.is_some() as usize) {
            for dir in &ctx.args.framework_paths {
                let mut path = dir.join(&framework).join(subdir).join(name);
                if let Some(suffix) = suffix {
                    path = std::fs::canonicalize(&path).unwrap_or(path);
                    path.as_mut_os_string().push(crate::util::os_str(suffix));
                }
                for path in with_image_suffixes(ctx, &path) {
                    let found = match stubs {
                        true => prober.library(&path),
                        false => prober.exists(&path).then_some(path),
                    };
                    if found.is_some() {
                        return found;
                    }
                }
            }
        }
        None
    };
    search("").or_else(|| {
        ctx.args.search_in_sparse_frameworks.then(|| search("Versions/Current")).flatten()
    })
}

/// A library path with each -image_suffix suffix (put before its
/// extension: libfoo_debug.dylib), then as it is.
fn with_image_suffixes<E: Target>(ctx: &Context<E>, path: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = ctx
        .args
        .image_suffixes
        .iter()
        .map(|suffix| {
            let mut stem = path.file_stem().unwrap_or_default().to_os_string();
            stem.push(suffix);
            if let Some(ext) = path.extension() {
                stem.push(".");
                stem.push(ext);
            }
            path.with_file_name(stem)
        })
        .collect();
    paths.push(path.to_path_buf());
    paths
}

fn find_library<E: Target>(ctx: &Context<E>, prober: &Prober, name: &OsStr) -> Option<PathBuf> {
    // By default each directory is tried for a dylib and then an
    // archive before moving on (-search_paths_first, ld64's default
    // since Xcode 4). -search_dylibs_first restores the older ld64
    // behavior: a dylib anywhere on the path beats an archive
    // anywhere.
    // An image that links no dylib (Args::links_dylibs) looks for
    // archives only. A relocatable output looks for dylibs too, only to
    // ignore them (collect_file).
    use LibFile::*;
    let passes: &[&[LibFile]] = if !ctx.args.links_dylibs() {
        &[&[Archive]]
    } else if ctx.args.search_dylibs_first {
        &[&[Dylib, So], &[Archive]]
    } else {
        &[&[Dylib, So, Archive]]
    };
    search_library(ctx, prober, name, passes)
}

/// A file a library search looks for in a directory, as lib<name> and
/// an extension: a dylib - or its stub, a .tbd of the name, which
/// ld-prime looks for with it (see Prober::library), unless the dylib
/// itself is wanted -, a .so or an archive.
#[derive(Clone, Copy, PartialEq)]
enum LibFile {
    Dylib,
    DylibItself,
    So,
    Archive,
}

/// Looks for a dylib only, as -upward-l and -reexport-l do: an archive
/// can be neither an upward dependency nor a re-exported library.
fn find_dylib<E: Target>(ctx: &Context<E>, prober: &Prober, name: &OsStr) -> Option<PathBuf> {
    search_library(ctx, prober, name, &[&[LibFile::Dylib, LibFile::So]])
}

/// Looks for a dylib to merge (-merge-l): a dylib itself, which may
/// carry its mergeable record, never a stub.
fn find_mergeable_dylib<E: Target>(
    ctx: &Context<E>,
    prober: &Prober,
    name: &OsStr,
) -> Option<PathBuf> {
    search_library(ctx, prober, name, &[&[LibFile::DylibItself, LibFile::So]])
}

/// Looks for lib<name> in the library search path, for each pass of
/// files in turn. What is there counts, as for ld-prime, which fails
/// on a directory it finds. A name ending in .o
/// is a file name to look up as it is, whatever the option: clang
/// links crt1.o for an old deployment target as -lcrt1.10.6.o.
fn search_library<E: Target>(
    ctx: &Context<E>,
    prober: &Prober,
    name: &OsStr,
    passes: &[&[LibFile]],
) -> Option<PathBuf> {
    if name.as_bytes().ends_with(b".o") {
        return ctx.args.library_paths.iter().map(|dir| dir.join(name)).find(|p| prober.exists(p));
    }
    // In a directory, ld-prime looks for each -image_suffix variant
    // of the library, of any extension, before the library itself.
    for files in passes {
        for dir in &ctx.args.library_paths {
            let suffixes = ctx.args.image_suffixes.iter().map(OsString::as_os_str);
            for suffix in suffixes.chain([OsStr::new("")]) {
                for &file in *files {
                    let ext = match file {
                        LibFile::Dylib | LibFile::DylibItself => "dylib",
                        LibFile::So => "so",
                        LibFile::Archive => "a",
                    };
                    let mut leaf = OsString::from("lib");
                    leaf.push(name);
                    leaf.push(suffix);
                    leaf.push(format!(".{ext}"));
                    let path = dir.join(leaf);
                    let found = match file {
                        LibFile::Dylib => prober.library(&path),
                        _ => prober.exists(&path).then_some(path),
                    };
                    if found.is_some() {
                        return found;
                    }
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
    /// -merge_library, -merge-l, -merge_framework: the dylib's content
    /// goes into the image.
    merge: bool,
    /// Named by an object's auto-link option, or only by -possible-l
    /// and the like: a hint.
    autolinked: bool,
    /// Found in the SDK (see found_in_sdk).
    sdk: bool,
    /// -assert-weak-l, -assert_weak_library, -assert_weak_framework.
    assert_weak: bool,
    /// -delay-l, -delay_library, -delay_framework: the dylib's
    /// initializers run at the first use of one of its symbols.
    delay: bool,
    /// Matched by -sub_library or -sub_umbrella: re-exported, and not
    /// weak whatever the naming says (see sub_reexport).
    sub_reexport: bool,
}

impl ReaderContext {
    /// What no naming says, which any naming's union with is itself.
    const NO_NAMING: Self = Self {
        force_load: false,
        weak: false,
        reexport: false,
        hidden: false,
        needed: false,
        upward: false,
        lazy: false,
        merge: false,
        autolinked: true,
        sdk: false,
        assert_weak: false,
        delay: false,
        sub_reexport: false,
    };

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
            merge: self.merge || other.merge,
            autolinked: self.autolinked && other.autolinked,
            sdk: self.sdk || other.sdk,
            assert_weak: self.assert_weak || other.assert_weak,
            delay: self.delay || other.delay,
            sub_reexport: self.sub_reexport || other.sub_reexport,
        }
    }
}

/// Reports a dylib that does not let this link name it directly (see
/// input_files::is_allowed_client): an error on the command line, while
/// the library an auto-link option names is left out, as a hint not
/// found is (see missing_hint).
fn refuses_client<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    rc: ReaderContext,
) -> bool {
    let id = input_files::dylib_identity(ctx, mf);
    if input_files::is_allowed_client(ctx, &id) {
        return false;
    }
    let leaf = id.install_name.rsplit(|&b| b == b'/').next().unwrap_or(&[]);
    let msg = format_args!(
        "cannot link directly with '{}' because product being built is not an allowed client of it",
        raw(leaf)
    );
    if !rc.autolinked {
        error!("{msg}");
    } else {
        let msg = format_args!("Could not parse or use implicit file '{}': {msg}", mf.name.raw());
        ctx.autolink_misses.push(error::render(msg));
    }
    true
}

/// Whether an object or dylib is for an architecture the link doesn't
/// take (see input_files::takes_arch), which ld-prime ignores with a
/// warning - an archive member too, whether the link needs it or not.
/// -allow_sub_type_mismatches has it take one of another subtype with
/// a warning instead.
pub(crate) fn is_foreign<E: Target>(ctx: &Context<E>, mf: &MappedFile) -> bool {
    let Some(arch) = input_files::foreign_arch::<E>(mf) else { return false };
    if ctx.args.allow_sub_type_mismatches && input_files::is_subtype_mismatch::<E>(mf) {
        let name = input_files::without_fat_arch(path_bytes(&mf.name));
        crate::warn!("linking {arch} file '{}' into {} link", crate::error::raw(&name), E::NAME);
        return false;
    }
    let why = format!("found architecture '{arch}', required architecture '{}'", E::NAME);
    input_files::ignore_foreign_file(ctx, mf, &why);
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
    // auto-link options; load each file once, as its first naming says
    // (the first that isn't a public re-export's, for a dylib). An
    // object file, though, loads as often as it is named, as in
    // ld-prime: twice over, its globals are duplicate definitions.
    let ty = get_file_type(mf);
    let object = matches!(ty, FileType::Object | FileType::LlvmBitcode);
    if !ctx.visited_files.insert(mf.name.clone()) && !object {
        name_again(ctx, mf, rc, out);
        return;
    }
    if !matches!(ty, FileType::Archive | FileType::Fat) {
        input_files::trace_file(ctx, path_bytes(&mf.name));
    }
    if matches!(ty, FileType::Object | FileType::Dylib) && is_foreign(ctx, mf) {
        return;
    }
    match ty {
        FileType::Object => {
            let priority = ctx.next_priority();
            out.push(PendingObject { mf, alive: true, hidden: rc.hidden, priority });
        }
        // Only a dylib -make_mergeable made can be merged, which
        // ld-prime checks first, in an image that would ignore the dylib
        // too. (It takes a stub for one, then binds the stub's symbols
        // to the image itself, which dyld then fails to find.)
        FileType::Tapi | FileType::Dylib if rc.merge && !input_files::is_mergeable(mf) => {
            error!("dylib cannot be merged, not built with -make_mergeable in '{}'", mf.name.raw());
        }
        // A relocatable output keeps every reference undefined for the
        // final link, and the other images that link no dylib have
        // nothing to load one with (Args::links_dylibs): ld-prime reads
        // a dylib on their command lines (and ignores a stub without
        // the architecture as ever), then ignores it with a warning.
        FileType::Tapi | FileType::Dylib if ctx.args.relocatable || !ctx.args.links_dylibs() => {
            if ty == FileType::Dylib || input_files::load_tbd(ctx, mf).is_some() {
                crate::warn!("ignoring unexpected dylib '{}'", mf.name.raw());
            }
        }
        FileType::Dylib if rc.merge => merge_dylib(ctx, mf, out),
        FileType::Tapi | FileType::Dylib if refuses_client(ctx, mf, rc) => {}
        FileType::Dylib if !input_files::has_uuid(mf.data()) => refuse_without_uuid(mf),
        FileType::Tapi | FileType::Dylib => load_dylib(ctx, mf, rc),
        FileType::Archive => collect_archive_members(ctx, mf, rc, out),
        FileType::Fat => match input_files::fat_slice::<E>(&ctx.args, mf) {
            Some(slice) => collect_file(ctx, slice, rc, out),
            None => input_files::warn_fat_missing_arch(ctx, mf),
        },
        FileType::LlvmBitcode => {
            input_files::parse_bitcode(ctx, mf, true);
        }
        FileType::Empty => {}
        _ => refuse_file(mf),
    }
}

/// Loads a dylib or its stub, and the public libraries it re-exports,
/// and gives it what its naming says.
fn load_dylib<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile, rc: ReaderContext) {
    let first = ctx.dylibs.len();
    let idx = if get_file_type(mf) == FileType::Tapi {
        input_files::parse_dylib(ctx, mf)
    } else {
        Some(input_files::parse_dylib_binary(ctx, mf))
    };
    let Some(idx) = idx else { return };
    // The dylibs loaded during the parse beyond this one are the public
    // libraries it re-exports; a weak parent's are weak, a lazy one's
    // lazy, and a delayed one's initialized when it is. (Those standing
    // for libraries its exports moved to load weakly only as their
    // imports say; see passes::weaken_moved_imports.)
    let lazy = rc.lazy && ctx.args.lazy_load;
    let delay_init = rc.delay.then(|| ctx.dylibs[idx].install_name.clone());
    for d in &mut ctx.dylibs[first..] {
        d.is_weak |= rc.weak && d.name_source != input_files::NameSource::Moved;
        d.is_lazy |= lazy;
        if d.delay_init.is_none() {
            d.delay_init.clone_from(&delay_init);
        }
    }
    // One named before by another path keeps what that said.
    if idx >= first || ctx.dylibs[idx].is_implicit {
        name_dylib(ctx, idx, rc);
    }
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
fn name_dylib<E: Target>(ctx: &mut Context<E>, idx: usize, rc: ReaderContext) {
    if ctx.dylibs[idx].is_implicit {
        ctx.dylibs[idx].named_at = Some(ctx.next_priority());
    }
    let lazy = rc.lazy && ctx.args.lazy_load;
    let dylib = &mut ctx.dylibs[idx];
    let delay_init = rc.delay.then(|| dylib.install_name.clone());
    if dylib.is_implicit && !rc.autolinked {
        dylib.is_weak = rc.weak;
        dylib.is_lazy = lazy;
        dylib.delay_init = delay_init;
    } else {
        dylib.is_weak |= rc.weak;
        dylib.is_lazy |= lazy;
        if dylib.delay_init.is_none() {
            dylib.delay_init = delay_init;
        }
    }
    if rc.delay && dylib.has_weak_defs {
        crate::warn!(
            "delay-init link with '{}' will be ignored because it has weak-def exports",
            crate::error::raw(&dylib.install_name)
        );
    }
    dylib.is_reexported |= rc.reexport;
    dylib.is_weak_asserted |= rc.assert_weak;
    dylib.is_needed |= rc.needed;
    dylib.is_upward |= rc.upward;
    dylib.in_sdk = rc.sdk;
    dylib.is_implicit = false;
    // A library -sub_library or -sub_umbrella re-exports loads strongly.
    if rc.sub_reexport {
        if dylib.is_weak {
            let name = crate::error::raw(&dylib.install_name);
            crate::warn!("re-exported dylibs cannot be weak-linked: {name}");
            dylib.is_weak = false;
        }
        dylib.is_reexported = true;
    }
}

/// Queues an archive's members. Every member is parsed eagerly; whether
/// it is *live* - whether its content reaches the output - is decided
/// by symbol resolution and the liveness walk. -all_load and -force_load
/// make every member live up front; -ObjC does so for members with
/// Objective-C metadata, which register classes by their mere presence.
/// ld64 exempts clang's runtime library (libclang_rt.*.a, which the
/// compiler driver adds to every link) from -all_load: its members are
/// wanted only when referenced.
fn collect_archive_members<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    rc: ReaderContext,
    out: &mut Vec<PendingObject>,
) {
    let all_load = ctx.args.all_load
        && !mf.name.file_name().is_some_and(|f| f.as_bytes().starts_with(b"libclang_rt"));
    if rc.force_load {
        ctx.force_loaded.insert(mf.name.clone());
    }
    for member in crate::archive_file::read_archive_members(mf) {
        input_files::trace_file(ctx, path_bytes(&member.name));
        let alive = rc.force_load
            || all_load
            || (ctx.args.load_objc && input_files::has_objc_sections(member));
        match get_file_type(member) {
            FileType::LlvmBitcode => {
                input_files::parse_bitcode(ctx, member, alive);
            }
            FileType::Object if is_foreign(ctx, member) => {}
            _ => {
                let priority = ctx.next_priority();
                out.push(PendingObject { mf: member, alive, hidden: rc.hidden, priority });
            }
        }
    }
}

/// A file named again: a library (or a universal file) is loaded once,
/// but -force_load of an archive named before loads its members all the
/// same.
fn name_again<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    rc: ReaderContext,
    out: &mut [PendingObject],
) {
    if rc.force_load && ctx.force_loaded.insert(mf.name.clone()) {
        for p in out.iter_mut().filter(|p| p.mf.parent.is_some_and(|a| a.name == mf.name)) {
            p.alive = true;
        }
    }
}

/// Refuses an image the link reads that has no LC_UUID (see
/// input_files::has_uuid).
fn refuse_without_uuid(mf: &MappedFile) {
    error!("missing LC_UUID load command in '{}'", mf.name.raw());
}

/// Refuses a file the link can't take, by what it is.
fn refuse_file(mf: &MappedFile) {
    let name = input_files::without_fat_arch(path_bytes(&mf.name));
    let name = raw(&name);
    if crate::filetype::get_macho_filetype(mf.data()).is_some() {
        error!(
            "unsupported mach-o filetype (only MH_OBJECT and MH_DYLIB can be linked) in '{name}'"
        );
    } else {
        error!("unknown file type in '{name}'");
    }
}

/// Merges a mergeable dylib into the image: the entries of its record
/// (LC_ATOM_INFO), read back into the object file they stand for, link
/// as that object would (see mergeable::synthesize_object), and the
/// dylibs it links stand by their recorded identities (see
/// add_merged_dependencies). The image gets the hook for the classes of
/// mergeable libraries for the classes it defines (see bundle_hook).
fn merge_dylib<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    out: &mut Vec<PendingObject>,
) {
    let record = crate::mergeable::MergeableRecord::read(mf);
    let obj = crate::mergeable::synthesize_object::<E>(&record, &mf.name);
    let synth = MappedFile::synthesized(mf.name.clone(), obj);
    if record.defines_classes() {
        crate::bundle_hook::note_merged_library(ctx, &record.own.install_name, synth);
    }
    let priority = ctx.next_priority();
    out.push(PendingObject { mf: synth, alive: true, hidden: false, priority });
    let deps = record.dependencies(&mf.name);
    ctx.merged_dependencies.extend(deps);
    let lib =
        MergedLibrary { install_name: record.own.install_name, minos: record.minos, obj: synth };
    ctx.merged_libraries.push(lib);
}

/// Loads the dylibs the merged mergeable dylibs link, after the command
/// line's: ld-prime gives each a load command by the install name and
/// versions the mergeable dylib recorded, without reading it, and binds
/// the merged code's imports to it - unless the command line loaded the
/// library already, whose naming then decides (a -weak-l makes it weak).
/// A library merged as well is none: what one merged library imports
/// from another, the other's merged code defines.
fn add_merged_dependencies<E: Target>(ctx: &mut Context<E>) {
    for dep in std::mem::take(&mut ctx.merged_dependencies) {
        if !ctx.merged_libraries.iter().any(|lib| lib.install_name == dep.info.install_name) {
            input_files::add_merged_dependency(ctx, dep);
        }
    }
}

/// ld-prime warns of some sections of every object it parses - archive
/// members the link doesn't use included: it drops each __LD section it
/// doesn't know, and aligns the constants of a __DATA,__cfstring to a
/// pointer whatever the section says. Staging runs in parallel, so the
/// diagnostics come here, in input order.
fn warn_about_sections(staged: &[input_files::StagedObject]) {
    for obj in staged {
        for (i, hdr) in obj.sect_hdrs.iter().enumerate() {
            if input_files::is_unknown_ld_section(hdr) {
                crate::warn!(
                    "unknown section: __LD/{} in {}",
                    raw(hdr.sectname()),
                    obj.mf.name.raw()
                );
            } else if hdr.segname() == b"__DATA"
                && hdr.sectname() == b"__cfstring"
                && hdr.p2align != 3
                && obj.isecs.iter().any(|isec| isec.shndx == i as u32 && isec.is_alive())
            {
                crate::warn!(
                    "section __DATA/__cfstring is not pointer aligned in {}",
                    obj.mf.name.raw()
                );
            }
        }
    }
}

/// Stages the queued object files in parallel and integrates them in
/// input order - the parallel front end of the mold design.
fn load_pending<E: Target>(ctx: &mut Context<E>, pending: Vec<PendingObject>) {
    let relocatable = ctx.args.relocatable;
    let kept_fdes = input_files::KeptFdes::of(&ctx.args);
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
                kept_fdes,
            )
        })
        .collect();
    drop(t);
    warn_about_sections(&staged);

    // Intern every staged object's global names in one parallel batch
    // (mold's sharded symbol table), so the serial integration loop
    // below does no hashing. Names carry the xxh3 hashes staging
    // computed alongside them, so nothing here touches their bytes.
    // Each object's extern (name, hash) list is filtered in parallel -
    // a debug link scans millions of nlists here - then concatenated in
    // object order into the batch the sharded intern resolves at once.
    let per_obj: Vec<Vec<(&'static [u8], u64)>> = staged
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
    let batch = per_obj.concat();
    let t = ctx.timer("gather");
    let ids = ctx.symbols.gather(&batch);
    drop(t);

    let t = ctx.timer("integrate");
    input_files::integrate_objects(ctx, staged, ids, counts);
    drop(t);
}

/// ld64 warns, once, about the libraries given more than once by the
/// same kind of option (-weak-lz repeats no -lz): the -l options, those
/// naming a library by path, and an archive's bare path, but not a
/// dylib's or a framework option. Build systems that knowingly repeat
/// them pass -no_warn_duplicate_libraries.
fn warn_duplicate_libraries<E: Target>(ctx: &Context<E>) {
    if !ctx.args.warn_duplicate_libraries {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    let mut seen_files = std::collections::HashSet::new();
    let mut dups = std::collections::BTreeSet::new();
    for arg in &ctx.args.inputs {
        let name = match arg {
            InputArg::Library(_, LibraryName::Framework(_)) => continue,
            InputArg::Library(kind, name) if !seen.insert((kind, name)) => name.as_os_str(),
            InputArg::File(path) | InputArg::Listed(path)
                if !seen_files.insert(path)
                    && MappedFile::open(path)
                        .is_some_and(|mf| get_file_type(mf) == FileType::Archive) =>
            {
                path.as_os_str()
            }
            _ => continue,
        };
        dups.insert(error::render(format_args!("'{}'", name.raw())));
    }
    if !dups.is_empty() {
        let list: Vec<Vec<u8>> = dups.into_iter().collect();
        crate::warn!("ignoring duplicate libraries: {}", raw(&list.join(&b", "[..])));
    }
}

/// Reads all input files: finds the file each input names, loads the
/// dylibs in command line order and parses the objects and archive
/// members in parallel (see load_pending).
pub fn read_input_files<E: Target>(ctx: &mut Context<E>) {
    warn_duplicate_libraries(ctx);
    // -add_linker_option's options are read first, as the command
    // line's.
    if !ctx.args.ignore_auto_link {
        let words = std::slice::from_ref(&ctx.args.linker_options);
        ctx.cmdline_linker_options = Some(read_linker_options(words, || "command line"));
    }
    let inputs = std::mem::take(&mut ctx.args.inputs);
    let paths: Vec<Option<PathBuf>> = inputs.iter().map(|arg| find_input(ctx, arg)).collect();
    let namings = library_namings(&ctx.args, &inputs, &paths);

    let stubs: Vec<&'static MappedFile> = inputs
        .iter()
        .zip(&paths)
        .filter(|(arg, _)| !matches!(arg, InputArg::Library(LibraryKind::Force, _)))
        .filter_map(|(_, path)| MappedFile::open(path.as_ref()?))
        .filter(|mf| get_file_type(mf) == FileType::Tapi)
        .collect();
    prefetch_stubs(ctx, &stubs);

    for (arg, path) in inputs.iter().zip(&paths) {
        if let (InputArg::Library(LibraryKind::Possible, name), None) = (arg, path) {
            let framework = matches!(name, LibraryName::Framework(_));
            ctx.autolink_misses.push(missing_hint(framework, name.as_os_str().as_bytes()));
        }
    }

    let mut queue: Vec<PendingObject> = Vec::new();
    for ((arg, path), rc) in inputs.iter().zip(paths).zip(namings) {
        let (Some(path), Some(mut rc)) = (path, rc) else { continue };
        // A library only -possible-l and the like name is a hint, which
        // loads with the auto-linked ones.
        if rc.autolinked {
            ctx.possible_files.push(path);
            continue;
        }
        rc.sub_reexport = sub_reexport(ctx, arg, &path);
        match MappedFile::try_open(&path) {
            Ok(mf) if mf.size() == 0 => error!("file is empty in '{}'", path.raw()),
            Ok(mf) if matches!(arg, InputArg::BundleLoader(_)) => {
                load_bundle_loader(ctx, mf, rc, &mut queue)
            }
            Ok(mf) => collect_file(ctx, mf, rc, &mut queue),
            Err(e) => error!("{}", raw(&unreadable_file(&path, &e))),
        }
    }
    ctx.args.inputs = inputs;
    add_merged_dependencies(ctx);
    collect_indirect_files(ctx, &mut queue);
    add_bundle_hook(ctx, &mut queue);
    load_pending(ctx, queue);
}

/// Warms the .tbd parse cache: parses the `stubs` on all cores, then
/// the stubs they re-export likewise, wave after wave down an SDK's
/// umbrella trees, so that the serial loop loading them finds every
/// parse done.
fn prefetch_stubs<E: Target>(ctx: &Context<E>, stubs: &[&'static MappedFile]) {
    let mut seen: hashbrown::HashSet<&Path> = stubs.iter().map(|mf| mf.name.as_path()).collect();
    let mut wave = stubs.to_vec();
    while !wave.is_empty() {
        let tbds = tapi::prefetch(&wave, E::NAME, ctx.args.platform);
        wave.clear();
        for tbd in tbds.iter().flatten() {
            for name in &tbd.reexports {
                if tbd.document(name).is_some() {
                    continue;
                }
                if let Some(dep) = crate::input_files::find_reexport(ctx, name)
                    && get_file_type(dep) == FileType::Tapi
                    && seen.insert(dep.name.as_path())
                {
                    wave.push(dep);
                }
            }
        }
    }
}

/// Parses the stubs of the libraries the auto-link options `opts` name
/// ahead of load_autolink_deps' serial loop (see prefetch_stubs), which
/// looks for them again, noting the files it doesn't find: this quiet
/// search notes nothing. Each file is opened by one thread.
fn prefetch_autolinked_stubs<E: Target>(ctx: &Context<E>, opts: &[Vec<Vec<u8>>]) {
    let prober = Prober::quiet(ctx);
    let mut paths: Vec<PathBuf> =
        opts.par_iter().filter_map(|opt| find_autolinked(ctx, &prober, opt)).collect();
    paths.sort_unstable();
    paths.dedup();
    let stubs: Vec<&'static MappedFile> = paths
        .par_iter()
        .filter_map(|path| MappedFile::try_open(path).ok())
        .filter(|mf| get_file_type(mf) == FileType::Tapi)
        .collect();
    prefetch_stubs(ctx, &stubs);
}

/// Adds the hook for the classes of mergeable libraries (see
/// bundle_hook), if the link may need it, as the first object, for the
/// classes of a library a -no_merge_* option names whether or not the
/// option is the one to load it: `-lfoo -no_merge_library ./libfoo.dylib`
/// adds the hook to an image that doesn't re-export libfoo.
fn add_bundle_hook<E: Target>(ctx: &mut Context<E>, queue: &mut Vec<PendingObject>) {
    let reexported: Vec<&'static MappedFile> = (ctx.args.inputs.iter())
        .filter(|arg| matches!(arg, InputArg::Library(LibraryKind::NoMerge, _)))
        .filter_map(|arg| find_input(ctx, arg).and_then(MappedFile::open))
        .collect();
    for mf in reexported {
        crate::bundle_hook::note_reexported_library(ctx, mf);
    }
    if let Some(mf) = crate::bundle_hook::hook_object(ctx) {
        ctx.bundle_hook.obj = Some(ctx.objs.len());
        queue.insert(0, PendingObject { mf, alive: true, hidden: false, priority: 0 });
    }
}

/// Whether -sub_umbrella or -sub_library re-exports an input: the
/// former a framework a -framework option names so, the latter a
/// library whose file is so named less its extension ("libfoo" for
/// libfoo.dylib or libfoo.tbd, whatever its install name), a
/// framework's included, which ld-prime deprecates.
fn sub_reexport<E: Target>(ctx: &Context<E>, arg: &InputArg, path: &Path) -> bool {
    use LibraryKind::*;
    let (umbrellas, libraries) = (&ctx.args.sub_umbrellas, &ctx.args.sub_libraries);
    if umbrellas.is_empty() && libraries.is_empty() {
        return false;
    }
    let framework = match arg {
        InputArg::Library(
            Plain | Weak | Reexport | Needed | Upward | Lazy,
            LibraryName::Framework(name),
        ) => Some(name.as_bytes()),
        _ => None,
    };
    // (Less the suffix a -framework option may give after a comma.)
    let framework = framework.map(|name| name.split(|&c| c == b',').next().unwrap_or(name));
    if framework.is_some_and(|name| umbrellas.iter().any(|u| u == name)) {
        return true;
    }
    let stem = path.file_stem().map_or(&b""[..], |stem| stem.as_bytes());
    if !libraries.iter().any(|l| l == stem) {
        return false;
    }
    if framework.is_some() {
        crate::warn!(
            "using -sub_library to re-export a framework is deprecated.  Use -reexport_framework instead"
        );
    }
    true
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

/// The words for a file MappedFile::try_open failed on with `e`. Every
/// input is read whole, and an empty one refused, so a file that is
/// there but no regular one (which MappedFile takes for none) is one
/// that can't be mapped - a directory - or an empty one.
pub fn unreadable_file(path: &Path, e: &std::io::Error) -> error::Message {
    let p = path.raw();
    let found = std::fs::metadata(path).ok().filter(|_| e.kind() == std::io::ErrorKind::NotFound);
    match found {
        Some(md) if md.len() == 0 => b"file is empty".to_vec(),
        Some(_) => {
            let e = std::io::Error::from_raw_os_error(libc::EINVAL);
            let errno = crate::error::strerror(&e);
            error::render(format_args!("cannot map {p}: {errno}"))
        }
        None => {
            let errno = crate::error::strerror(e);
            error::render(format_args!("cannot open {p}: {errno}"))
        }
    }
}

/// Finds the file an input names: None for a library or framework not
/// found, or a file a library option, -force_load, -bundle_loader or a
/// path of an archive names that isn't there. ld-prime takes a path of
/// an archive on the command line - one its name ends in .a - for a
/// library's: an absolute one is looked for under each -syslibroot
/// first (see find_file), and one missing is a library not found. Any
/// other path, and any a -filelist gives, is the file's.
fn find_input<E: Target>(ctx: &Context<E>, arg: &InputArg) -> Option<PathBuf> {
    use LibraryKind::*;
    let prober = &Prober::new(ctx);
    match arg {
        InputArg::File(path) if path_bytes(path).ends_with(b".a") => {
            find_file(ctx, prober, path, true)
        }
        InputArg::File(path) | InputArg::Listed(path) => Some(path.clone()),
        InputArg::Library(Merge, LibraryName::Path(path)) => find_file(ctx, prober, path, false),
        InputArg::BundleLoader(path) | InputArg::Library(_, LibraryName::Path(path)) => {
            find_file(ctx, prober, path, true)
        }
        InputArg::Library(Upward | Reexport | NoMerge | Delay, LibraryName::Lib(name)) => {
            find_dylib(ctx, prober, name)
        }
        InputArg::Library(Merge, LibraryName::Lib(name)) => find_mergeable_dylib(ctx, prober, name),
        InputArg::Library(_, LibraryName::Lib(name)) => find_library(ctx, prober, name),
        InputArg::Library(Merge, LibraryName::Framework(name)) => {
            find_framework(ctx, prober, name, false)
        }
        InputArg::Library(_, LibraryName::Framework(name)) => {
            find_framework(ctx, prober, name, true)
        }
    }
}

/// The file an option that takes a library's path names: an absolute
/// path under each -syslibroot first, as ld64's findFile looks it up,
/// a stub in place of the library where there is one there -
/// `-weak_library /usr/lib/libz.dylib` links the SDK's
/// usr/lib/libz.tbd - then the path as it is, itself only. An object
/// is taken as it is. A library to merge is no stub (`stubs`). A last
/// -syslibroot of / drops the roots (see sdk_roots).
fn find_file<E: Target>(
    ctx: &Context<E>,
    prober: &Prober,
    path: &Path,
    stubs: bool,
) -> Option<PathBuf> {
    let object = path.extension() == Some(OsStr::new("o"));
    if path.is_absolute() && !object {
        for root in sdk_roots(&ctx.args) {
            let path = under_root(root, path);
            let found = match stubs {
                true => prober.library(&path),
                false => prober.exists(&path).then_some(path),
            };
            if found.is_some() {
                return found;
            }
        }
    }
    prober.exists(path).then(|| path.to_path_buf())
}

/// Whether the library an option names was `found` in the SDK, whose
/// libraries ld-prime trusts to suit the deployment target: for an
/// option naming the library's path, under a -syslibroot joined to it
/// (see find_file); otherwise by a search (see searched_in_sdk).
fn found_in_sdk(args: &Args, arg: &InputArg, found: &Path) -> bool {
    use LibraryKind::*;
    match arg {
        InputArg::Library(Weak | Reexport | Needed | Upward | Lazy, LibraryName::Path(path)) => {
            found != path && found != path.with_extension("tbd")
        }
        _ => searched_in_sdk(args, found),
    }
}

/// Whether a library search `found` a file in the SDK: in a directory
/// under a -syslibroot as their paths spell them, the root joined to the
/// directory or not (-L$SDK/usr/lib, say).
fn searched_in_sdk(args: &Args, found: &Path) -> bool {
    sdk_roots(args).iter().any(|root| path_bytes(found).starts_with(path_bytes(root)))
}

/// The -syslibroots the files an option names by path are looked up
/// under, and those of the SDK: none when the last one is /, as ld64
/// drops them then.
fn sdk_roots(args: &Args) -> &[PathBuf] {
    match args.syslibroot.last() {
        Some(root) if root.as_os_str() == "/" => &[],
        _ => &args.syslibroot,
    }
}

/// What a library option says of the library it names: the flags it
/// gives the file, whether the library is a framework, and its name as
/// the option gives it (a path, for the options that take one).
fn library_option(arg: &InputArg) -> Option<(ReaderContext, bool, &OsStr)> {
    use LibraryKind::*;
    let InputArg::Library(kind, name) = arg else { return None };
    let kind = *kind;
    let rc = ReaderContext {
        force_load: kind == Force,
        weak: kind == Weak,
        reexport: matches!(kind, Reexport | NoMerge),
        hidden: kind == Hidden,
        needed: kind == Needed,
        upward: kind == Upward,
        lazy: kind == Lazy,
        merge: kind == Merge,
        autolinked: kind == Possible,
        assert_weak: kind == AssertWeak,
        delay: kind == Delay,
        ..Default::default()
    };
    Some((rc, matches!(name, LibraryName::Framework(_)), name.as_os_str()))
}

/// How each input is named: the flags its option gives the file, None
/// for a library an earlier option named. ld-prime reads the library
/// options before it reads a file, and merges what those naming one
/// library say - those naming one framework, or finding one file: under
/// -L., `-lfoo` and `-upward_library ./libfoo.dylib` both load an upward
/// libfoo. A file also given by bare path, or named by options that
/// match no other way (`-upward_library libfoo.dylib`), takes nothing
/// from the other namings: the first to load the file decides (see
/// collect_file). The first library or framework not found, -force_load's
/// among them, stops the link, as does a naming check_naming refuses.
fn library_namings(
    args: &Args,
    inputs: &[InputArg],
    paths: &[Option<PathBuf>],
) -> Vec<Option<ReaderContext>> {
    let mut merged: hashbrown::HashMap<(bool, &OsStr), ReaderContext> = hashbrown::HashMap::new();
    let mut keys = Vec::with_capacity(inputs.len());
    for (arg, path) in inputs.iter().zip(paths) {
        let key = match (library_option(arg), path) {
            // A hint (see missing_hint).
            (Some((rc, _, _)), None) if rc.autolinked => None,
            (Some((_, true, name)), None) => fatal!("framework '{}' not found", name.raw()),
            (Some((_, false, name)), None) => fatal!("library '{}' not found", name.raw()),
            (None, None) => match arg {
                InputArg::BundleLoader(path) | InputArg::File(path) => {
                    fatal!("library '{}' not found", path.raw())
                }
                _ => None,
            },
            (Some((rc, framework, name)), Some(path)) => {
                // A framework by its name, any other library by the
                // file found.
                let key = (framework, if framework { name } else { path.as_os_str() });
                let all = merged.entry(key).or_insert(ReaderContext::NO_NAMING);
                // An archive has no imports to make weak; ld-prime warns
                // of a -weak-l that finds one, once for the library.
                if let InputArg::Library(LibraryKind::Weak, LibraryName::Lib(_)) = arg
                    && !all.weak
                    && path.extension() == Some(OsStr::new("a"))
                {
                    crate::warn!(
                        "-weak-l{0} resolved to a static library '{1}', but only dynamic libraries can be weak linked. Use -l{0} when linking static libraries, or make sure .dylib/.tbd library is located in -L search paths.",
                        name.raw(),
                        path.raw()
                    );
                }
                *all = all.union(ReaderContext { sdk: found_in_sdk(args, arg, path), ..rc });
                check_naming(*all, framework, name);
                Some(key)
            }
            (None, _) => None,
        };
        keys.push(key);
    }
    // The options naming one library make one input, where it is first
    // named; each other input is one of its own.
    let naming = |key: Option<_>| match key {
        Some(key) => merged.remove(&key),
        None => Some(ReaderContext::default()),
    };
    keys.into_iter().map(naming).collect()
}

/// ld-prime refuses to re-export a library that it links weakly or
/// lazily, and to merge one that it re-exports, links weakly, upward
/// or lazily, all of which want a load command, naming the pair as the
/// option that makes it spells the library (a path as `-weak-l<path>`).
fn check_naming(rc: ReaderContext, framework: bool, name: &OsStr) {
    let spell = |opt: &str| match framework {
        true => error::RawBuf(error::render(format_args!("'-{opt}_framework {}'", name.raw()))),
        false => error::RawBuf(error::render(format_args!("'-{opt}-l{}'", name.raw()))),
    };
    for (on, opt) in [(rc.weak, "weak"), (rc.lazy, "lazy"), (rc.delay, "delay")] {
        if on && rc.reexport {
            fatal!("{} and {} cannot be used together", spell(opt), spell("reexport"));
        }
    }
    if rc.delay && rc.lazy {
        fatal!("{} and {} cannot be used together", spell("delay"), spell("lazy"));
    }
    let load_command =
        [(rc.reexport, "reexport"), (rc.weak, "weak"), (rc.upward, "upward"), (rc.lazy, "lazy")];
    for (on, opt) in load_command {
        if on && rc.merge {
            fatal!("{} and {} cannot be used together", spell(opt), spell("merge"));
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
        FileType::Fat => input_files::fat_slice::<E>(&ctx.args, mf),
        _ => Some(mf),
    };
    match exe.filter(|exe| crate::filetype::get_macho_filetype(exe.data()) == Some(MH_EXECUTE)) {
        Some(exe) => {
            input_files::trace_file(ctx, path_bytes(&mf.name));
            match input_files::has_uuid(exe.data()) {
                true => _ = input_files::parse_bundle_loader(ctx, exe),
                false => refuse_without_uuid(exe),
            }
        }
        None => collect_file(ctx, mf, rc, out),
    }
}

/// Reads an object's auto-link options (LC_LINKER_OPTION) as ld-prime
/// does: their strings in a row, as a command line of library options.
/// Those naming a library, a framework or an archive to load are kept,
/// one to a command. The rest are dropped: unknown words, options
/// missing their argument and the options a command line may give for
/// a library but an object may not (weak, re-exported or upward) with a
/// warning; search paths and loading modes (-L, -all_load, ...)
/// silently. What is kept reads the same again. `file` names the object
/// in the warnings ("command line" for -add_linker_option's, which
/// ld-prime reads the same way). Returns what is kept.
fn read_linker_options<F: std::fmt::Display>(
    opts: &[Vec<Vec<u8>>],
    file: impl Fn() -> F,
) -> Vec<Vec<Vec<u8>>> {
    let words: Vec<&[u8]> = opts.iter().flatten().map(Vec::as_slice).collect();
    let warn = |kind: &str, what: &[u8]| {
        let (what, file) = (raw(what), file());
        crate::warn!("{kind} linker option from object file ignored: '{what}' in {file}");
    };
    let malformed = |opt: &str| {
        let file = file();
        crate::warn!(
            "malformed linker option from object file ignored: '{opt}' missing argument, in {file}"
        );
    };
    let mut libs: Vec<Vec<Vec<u8>>> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
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
                // An empty argument is a missing one. (These options
                // are ASCII.)
                let arg = words.get(i).filter(|arg| !arg.is_empty());
                i += 1;
                let Some(&arg) = arg else {
                    malformed(std::str::from_utf8(word).unwrap());
                    continue;
                };
                match word {
                    b"-l" => libs.push(vec![[word, arg].concat()]),
                    b"-weak_framework" | b"-reexport_framework" | b"-upward_framework" => {
                        warn("unexpected", &[word, b" ", arg].concat())
                    }
                    b"-weak_library" | b"-reexport_library" | b"-upward_library" => {
                        let kind = word.strip_suffix(b"_library").unwrap();
                        warn("unexpected", &[kind, b"-l", arg].concat());
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
                Some("-weak-l" | "-reexport-l" | "-upward-l") => warn("unexpected", word),
                Some("-L" | "-F") => {}
                Some(_) => libs.push(vec![word.to_vec()]),
                None => warn("unknown", word),
            },
        }
    }
    libs
}

/// The file an auto-link option read by read_linker_options names, and
/// how to load it. A library or framework not found is remembered for
/// passes::report_undef_errors.
fn autolinked_input<E: Target>(
    ctx: &mut Context<E>,
    opt: &[Vec<u8>],
) -> (Option<PathBuf>, ReaderContext) {
    let rc = ReaderContext { autolinked: true, ..Default::default() };
    let path = find_autolinked(ctx, &Prober::new(ctx), opt);
    match opt {
        [lib] => {
            if path.is_none() {
                ctx.autolink_misses.push(missing_hint(false, autolinked_library(lib)));
            }
            let sdk = path.as_ref().is_some_and(|path| searched_in_sdk(&ctx.args, path));
            // -force_load_swift_libs loads a Swift library's archive
            // whole, by its file name.
            let is_swift = |path: &PathBuf| {
                path.file_name().is_some_and(|f| f.as_bytes().starts_with(b"libswift"))
            };
            let force_load = ctx.args.force_load_swift_libs && path.as_ref().is_some_and(is_swift);
            let hidden = lib.starts_with(b"-hidden-l");
            (path, ReaderContext { force_load, hidden, sdk, ..rc })
        }
        [flag, name] if flag.ends_with(b"framework") => {
            if path.is_none() {
                ctx.autolink_misses.push(missing_hint(true, name));
            }
            let sdk = path.as_ref().is_some_and(|path| searched_in_sdk(&ctx.args, path));
            (path, ReaderContext { sdk, ..rc })
        }
        [flag, _] if flag == b"-force_load" => (path, ReaderContext { force_load: true, ..rc }),
        _ => (path, rc),
    }
}

/// Looks with `prober` for the file an auto-link option names: the
/// library or framework it names, or the file itself.
fn find_autolinked<E: Target>(
    ctx: &Context<E>,
    prober: &Prober,
    opt: &[Vec<u8>],
) -> Option<PathBuf> {
    let os_str = crate::util::os_str;
    match opt {
        [lib] => find_library(ctx, prober, os_str(autolinked_library(lib))),
        [flag, name] if flag.ends_with(b"framework") => {
            find_framework(ctx, prober, os_str(name), true)
        }
        [_, file] => Some(PathBuf::from(os_str(file))),
        _ => unreachable!(),
    }
}

/// The name of the library an auto-link option -l<name> (or -needed-l,
/// -lazy-l, -hidden-l) names.
fn autolinked_library(lib: &[u8]) -> &[u8] {
    let prefixes = ["-hidden-l", "-needed-l", "-lazy-l", "-l"];
    prefixes.iter().find_map(|prefix| lib.strip_prefix(prefix.as_bytes())).unwrap()
}

/// What ld-prime says, if symbols stay undefined, of a library or
/// framework an auto-link option or a -possible-l and the like names
/// that it didn't find. (A framework's first name leaves out a
/// ",suffix".)
fn missing_hint(framework: bool, name: &[u8]) -> error::Message {
    if framework {
        let base = name.split(|&c| c == b',').next().unwrap();
        error::render(format_args!(
            "Could not find or use auto-linked framework '{}': framework '{}' not found",
            raw(base),
            raw(name)
        ))
    } else {
        error::render(format_args!(
            "Could not find or use auto-linked library '{0}': library '{0}' not found",
            raw(name)
        ))
    }
}

/// Acts on the auto-link options (LC_LINKER_OPTION) of the live
/// objects, once symbols are resolved: each names a library or
/// framework the object needs, as if it had been on the command line.
/// Swift objects rely on this entirely. What the options load may make
/// more objects live, with options of their own, so symbols resolve
/// again and the options are read again until they load nothing new.
pub fn load_autolink_deps<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.ignore_auto_link {
        return;
    }
    loop {
        // Those of objects new to the link are read.
        for obj in ctx.objs.iter_mut().filter(|obj| obj.is_alive && !obj.linker_options_read) {
            let mf = obj.mf;
            obj.linker_options = read_linker_options(&obj.linker_options, || mf.name.raw());
            obj.linker_options_read = true;
        }
        // ld64 does not act on auto-link options in a -r link: the
        // LC_LINKER_OPTION commands are copied into the output object
        // and the final link resolves them. Loading them here would let
        // the libraries claim symbols that the output must leave
        // undefined (Xcode's prelink of a Swift package auto-linked
        // libc++ this way and the -r symbol table then lacked operator
        // new).
        if ctx.args.relocatable {
            return;
        }

        let (num_objs, num_dylibs) = (ctx.objs.len(), ctx.dylibs.len());
        load_autolinked_libraries(ctx);

        // New objects change what resolution chose, and so does a new
        // dylib that an earlier one merged as a private re-export, which
        // takes its symbols from that one: resolution runs again. Other
        // new dylibs only claim what is still undefined (see
        // passes::claim_new_dylibs).
        let (old, new) = ctx.dylibs.split_at(num_dylibs);
        let rebinds =
            new.iter().any(|d| old.iter().any(|o| o.merged_reexports.contains(&d.install_name)));
        if ctx.objs.len() == num_objs && !rebinds {
            if ctx.dylibs.len() != num_dylibs {
                passes::claim_new_dylibs(ctx, num_dylibs);
            }
            return;
        }
        passes::resolve_symbols(ctx);
    }
}

/// Loads the libraries the auto-link options not acted on yet name, and
/// the first time, those only -possible-l and the like name.
fn load_autolinked_libraries<E: Target>(ctx: &mut Context<E>) {
    // ld64 acts on the auto-link options as a sorted set, not in the
    // order the objects mention them: its load commands list the
    // auto-linked libraries alphabetically ("-framework AppKit" ...
    // "-lswiftCore", "-lswiftCoreFoundation" ...), which fixes their
    // ordinals too.
    // (Objects mostly repeat each other's options, which are deduplicated
    // before they are sorted.)
    let objs = ctx.objs.iter().filter(|obj| obj.is_alive);
    let opts = ctx.cmdline_linker_options.iter().flatten();
    let opts: hashbrown::HashSet<&Vec<Vec<u8>>> =
        opts.chain(objs.flat_map(|obj| &obj.linker_options)).collect();
    let mut pending: Vec<Vec<Vec<u8>>> = (opts.into_iter())
        .filter(|opt| !ctx.processed_linker_options.contains(*opt))
        .cloned()
        .collect();
    pending.sort();
    prefetch_autolinked_stubs(ctx, &pending);
    let dylibs_before = ctx.dylibs.len();
    ctx.autolink_priority = ctx.autolink_priority.min(ctx.priority_counter + 1);

    // A library already in the link as a public re-export (Foundation's
    // stub brings CoreFoundation) that an auto-link option now names
    // is a hint like any other auto-linked library: listed only if
    // something binds to it (ld-prime drops CoreFoundation from a
    // Swift program that never binds to it).
    let implicit_before: Vec<bool> = ctx.dylibs.iter().map(|d| d.is_implicit).collect();
    // An auto-link option is a hint, and ld-prime says nothing when it
    // finds no library or framework for one unless symbols are left
    // undefined (see passes::report_undef_errors). Header-only SDK
    // frameworks make that routine: every Swift object importing
    // CoreAudioTypes carries `-framework CoreAudioTypes`, whose
    // framework directory holds headers and a module map but no binary
    // (CotEditor's build printed a warning 317 times).
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
    // The libraries only -possible-l and the like name come next, once,
    // in command line order: ld-prime binds a symbol both they and an
    // auto-linked library define to the auto-linked one.
    for path in std::mem::take(&mut ctx.possible_files) {
        match MappedFile::try_open(&path) {
            Ok(mf) if mf.size() == 0 => error!("file is empty in '{}'", path.raw()),
            Ok(mf) => {
                let sdk = searched_in_sdk(&ctx.args, &path);
                let rc = ReaderContext { autolinked: true, sdk, ..Default::default() };
                collect_file(ctx, mf, rc, &mut queue);
            }
            Err(e) => error!("{}", raw(&unreadable_file(&path, &e))),
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
}
