//! Reading input files: finding the files the command line names, and
//! the libraries the objects' auto-link options name, and loading them
//! as objects, archives and dylibs.
//!
//! Dylibs are loaded as they are named, in command line order, which
//! decides their load commands; objects and archive members are queued
//! and parsed in parallel, then added to the link in command line order,
//! which gives each its priority for symbol resolution.
//!
//! Before that, the option parser reads the first objects here for the
//! target and the platform the options don't name (detect_machine_type,
//! infer_platform).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use mold_common::archive_file::members;
use mold_common::bytes::display;
use mold_common::path::path_bytes;
use mold_common::{error, fatal, warn};
use rayon::prelude::*;

use crate::arch::Target;
use crate::cmdline::{Args, InputArg, LibraryKind, LibraryName, parse_triple, triple_arch};
use crate::context::Context;
use crate::filetype::{self, FileType, get_file_type};
use crate::input_files;
use crate::input_files::PlatformVersion;
use crate::macho::*;
use crate::mapped_file::{MappedFile, unreadable_file};
use crate::mergeable::MergedLibrary;

/// Without -arch, ld-prime links for the target of the first object
/// file named on the command line: a Mach-O object's CPU type, or a
/// bitcode file's target triple. Archives, dylibs and universal files
/// don't count, and without such an object there is no target.
pub fn detect_machine_type(args: &Args) -> &'static str {
    for input in &args.inputs {
        let (InputArg::File(path) | InputArg::Listed(path)) = input else { continue };
        let Some(mf) = open_for_target(path) else { continue };
        match get_file_type(mf) {
            FileType::Object => {
                if let Some(name) = crate::filetype::get_macho_target(mf.data()) {
                    return name;
                }
            }
            FileType::LlvmBitcode => {
                let plugin = crate::lto::load_plugin(args.lto_library.as_deref());
                let triple = crate::lto::target_triple(&plugin, mf.data(), &mf.name);
                return triple_arch(triple.split('-').next().unwrap_or_default(), &triple);
            }
            _ => {}
        }
    }
    fatal!("Missing -arch option");
}

/// An input file ld-prime reads before the link proper to work out the
/// target (see detect_machine_type and infer_platform): None for an
/// empty one, which says nothing. A file it can't map stops it, in
/// words that name no input.
fn open_for_target(path: &Path) -> Option<&'static MappedFile> {
    match MappedFile::try_open(path) {
        Ok(mf) => (mf.size() > 0).then_some(mf),
        Err(e) => fatal!("{}", unreadable_file(path, &e)),
    }
}

/// Without -platform_version (or -macos_version_min or -target),
/// ld-prime links for what the first object file named on the command
/// line that has a platform load command was built for: its platform,
/// minimum OS and SDK versions, whatever later objects say (one built
/// for a newer OS draws a warning, one for another platform an error).
/// Archive members, universal files and dylibs don't count, nor does a
/// bitcode file unless no Mach-O object does: then the first one's
/// target triple names the platform and OS version, and no SDK. A final
/// image must have a platform; a -r or -preload output may be for none.
pub fn infer_platform(args: &mut Args) {
    let mut bitcode = None;
    for input in &args.inputs {
        let (InputArg::File(path) | InputArg::Listed(path)) = input else { continue };
        let Some(mf) = open_for_target(path) else { continue };
        match get_file_type(mf) {
            FileType::Object => {
                let Some(v) = PlatformVersion::of_object(mf.data()) else {
                    continue;
                };
                if !is_supported_platform(v.platform) {
                    fatal!(
                        "{}: unsupported platform: {}",
                        mf.name.display(),
                        platform_name(v.platform)
                    );
                }
                args.platform = v.platform;
                args.platform_minos = v.minos;
                args.platform_sdk = v.sdk;
                return;
            }
            FileType::LlvmBitcode => {
                bitcode.get_or_insert(mf);
            }
            _ => {}
        }
    }
    if let Some(mf) = bitcode {
        let plugin = crate::lto::load_plugin(args.lto_library.as_deref());
        let triple = crate::lto::target_triple(&plugin, mf.data(), &mf.name);
        (_, args.platform, args.platform_minos) = parse_triple(&triple);
    } else if !args.relocatable && !args.preload {
        fatal!("Missing -platform_version option");
    }
}

/// An absolute path looked up under a syslibroot, as ld-prime joins
/// them: less its leading slash, the path goes below the root, but one
/// that starts with two slashes stays absolute and replaces the root.
pub(crate) fn under_root(root: &Path, path: &Path) -> PathBuf {
    let bytes = path_bytes(path);
    root.join(mold_common::bytes::os_str(bytes.strip_prefix(b"/").unwrap_or(bytes)))
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
        let (opts, warnings) = read_linker_options(words, || "command line");
        for msg in warnings {
            warn!("{msg}");
        }
        ctx.cmdline_linker_options = Some(opts);
    }
    let inputs = std::mem::take(&mut ctx.args.inputs);
    let paths: Vec<Option<PathBuf>> = inputs.iter().map(|arg| find_input(ctx, arg)).collect();
    let namings = library_namings(&ctx.args, &inputs, &paths);

    // Read the stubs among the inputs ahead, in parallel (see
    // prefetch_stubs).
    let stubs: Vec<&'static MappedFile> = inputs
        .iter()
        .zip(&paths)
        .filter(|(arg, _)| !matches!(arg, InputArg::Library(LibraryKind::Force, _)))
        .filter_map(|(_, path)| MappedFile::open(path.as_ref()?))
        .filter(|mf| get_file_type(mf) == FileType::Tapi)
        .collect();
    prefetch_stubs(ctx, &stubs);

    // A library only -possible-l and the like name is a hint: one not
    // found is mentioned only if symbols stay undefined.
    for (arg, path) in inputs.iter().zip(&paths) {
        if let (InputArg::Library(LibraryKind::Possible, name), None) = (arg, path) {
            let framework = matches!(name, LibraryName::Framework(_));
            ctx.autolink_misses.push(missing_hint(framework, name.as_os_str().as_encoded_bytes()));
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
            Ok(mf) if mf.size() == 0 => error!("file is empty in '{}'", path.display()),
            Ok(mf) if matches!(arg, InputArg::BundleLoader(_)) => {
                load_bundle_loader(ctx, mf, rc, &mut queue)
            }
            Ok(mf) => read_file(ctx, mf, rc, &mut queue),
            Err(e) => error!("{}", unreadable_file(&path, &e)),
        }
    }
    ctx.args.inputs = inputs;
    add_merged_dependencies(ctx);
    collect_indirect_files(ctx, &mut queue);
    add_bundle_hook(ctx, &mut queue);
    load_pending(ctx, queue);
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
        dups.insert(format!("'{}'", name.display()));
    }
    if !dups.is_empty() {
        let list: Vec<String> = dups.into_iter().collect();
        warn!("ignoring duplicate libraries: {}", list.join(", "));
    }
}

/// Looks for files as ld-prime does in its searches for inputs, noting
/// each file it looks for and doesn't find: -dependency_info lists
/// them, so that a build system links again once one appears. A lookup
/// made again is quiet, without a warning too; one made ahead of time
/// keeps its notes and warnings for the search it stands for to give
/// (see ProbeLog).
pub struct Prober<'a> {
    missing: Option<&'a std::sync::Mutex<Vec<PathBuf>>>,
    quiet: bool,
    prefer_stubs: bool,
    warnings: Option<&'a std::sync::Mutex<Vec<String>>>,
}

impl<'a> Prober<'a> {
    pub fn new<E: Target>(ctx: &'a Context<E>) -> Self {
        let missing = ctx.args.dependency_info.is_some().then_some(&ctx.missing_files);
        Self { missing, quiet: false, prefer_stubs: ctx.args.prefer_stubs, warnings: None }
    }

    pub fn quiet<E: Target>(ctx: &Context<E>) -> Self {
        Self { missing: None, quiet: true, prefer_stubs: ctx.args.prefer_stubs, warnings: None }
    }

    /// A prober for a search made ahead of time, which keeps what it
    /// notes and warns of in `log`.
    fn recording<E: Target>(ctx: &Context<E>, log: &'a ProbeLog) -> Self {
        let (missing, warnings) = (Some(&log.missing), Some(&log.warnings));
        Self { missing, quiet: false, prefer_stubs: ctx.args.prefer_stubs, warnings }
    }

    /// Whether there is a file at `path`.
    pub fn exists(&self, path: &Path) -> bool {
        let found = file_exists(path);
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
            let msg = format_args!(
                "text-based stub file {} and library file {} unexpectedly found. Falling back \
                 to library file for linking.",
                stub.display(),
                path.display()
            );
            match self.warnings {
                Some(warnings) => warnings.lock().unwrap().push(msg.to_string()),
                None => warn!("{msg}"),
            }
        }
        Some(path.to_path_buf())
    }
}

/// Whether there is a file at `path`, memoized: a search made ahead of
/// time looks for the files that the one it stands for looks for again
/// (a library a stub re-exports, as the stub is prefetched and as it
/// loads), and a stat(2) of a path in an SDK walks its 15 or so
/// components each time. The answers are kept in shards by the path's
/// hash, as the threads of the parallel searches ask at once.
fn file_exists(path: &Path) -> bool {
    type Found = hashbrown::HashMap<PathBuf, bool>;
    const SHARDS: usize = 64;
    static FOUND: [std::sync::Mutex<Option<Found>>; SHARDS] =
        [const { std::sync::Mutex::new(None) }; SHARDS];
    let shard = &FOUND[xxhash_rust::xxh3::xxh3_64(path_bytes(path)) as usize % SHARDS];
    if let Some(&found) = shard.lock().unwrap().get_or_insert_with(Found::new).get(path) {
        return found;
    }
    let found = path.exists();
    shard.lock().unwrap().get_or_insert_with(Found::new).insert(path.to_path_buf(), found);
    found
}

/// What a search made ahead of time, in parallel with others, noted and
/// warned of, which the search it stands for gives in its turn.
#[derive(Default)]
struct ProbeLog {
    missing: std::sync::Mutex<Vec<PathBuf>>,
    warnings: std::sync::Mutex<Vec<String>>,
}

impl ProbeLog {
    /// Notes the files the search didn't find, and gives its warnings,
    /// as the search would have if made now.
    fn replay<E: Target>(self, ctx: &Context<E>) {
        if ctx.args.dependency_info.is_some() {
            ctx.missing_files.lock().unwrap().extend(self.missing.into_inner().unwrap());
        }
        for msg in self.warnings.into_inner().unwrap() {
            warn!("{msg}");
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
    let arg = arg.as_encoded_bytes();
    let (name, suffix) = match memchr::memchr(b',', arg) {
        Some(comma) => (&arg[..comma], Some(&arg[comma + 1..])),
        None => (arg, None),
    };
    let name = mold_common::bytes::os_str(name);
    let mut framework = name.to_os_string();
    framework.push(".framework");
    let search = |subdir: &str| {
        for suffix in [suffix, None].into_iter().take(1 + suffix.is_some() as usize) {
            for dir in &ctx.args.framework_paths {
                let mut path = dir.join(&framework).join(subdir).join(name);
                if let Some(suffix) = suffix {
                    path = std::fs::canonicalize(&path).unwrap_or(path);
                    path.as_mut_os_string().push(mold_common::bytes::os_str(suffix));
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
    // ignore them (read_file).
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
    if name.as_encoded_bytes().ends_with(b".o") {
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

/// An LC_RPATH entry as a search directory: @loader_path stands for the
/// directory of the dylib that carries the entry.
pub(crate) fn loader_rpath(dylib: &Path, rpath: &[u8]) -> PathBuf {
    match rpath.strip_prefix(b"@loader_path/") {
        Some(rest) => dir_of(dylib).join(mold_common::bytes::os_str(rest)),
        None => PathBuf::from(mold_common::bytes::os_str(rpath)),
    }
}

/// The directory dyld would use for a dylib's @loader_path: that of
/// the real file, symlinks resolved. A framework's X.framework/X is a
/// symlink to Versions/A/X, and its LC_RPATH entries are written for
/// that location (XCTest's `@loader_path/../../../../PrivateFrameworks`
/// reaches XCTestCore only from Versions/A). A fat file's name may
/// carry the "(for architecture ...)" suffix the loader adds.
fn dir_of(path: &Path) -> PathBuf {
    let bytes = mold_common::path::path_bytes(path);
    let end = memchr::memmem::find(bytes, b"(for architecture").unwrap_or(bytes.len());
    let path = Path::new(mold_common::bytes::os_str(&bytes[..end]));
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
/// unless its file isn't there; one that is ld-prime reads as any input.
/// Then the name is looked for as
/// find_dylib_ref does, the files not found noted for -dependency_info
/// - but for the name itself if a stub has the library `inlined`.
pub(crate) fn resolve_dylib_ref<E: Target>(
    ctx: &Context<E>,
    name: &[u8],
    loader: &Path,
    loader_rpaths: &[PathBuf],
    inlined: bool,
) -> Option<&'static MappedFile> {
    let dylib_files = ctx.args.dylib_files.iter().filter(|(install_name, _)| install_name == name);
    for (_, file) in dylib_files {
        match MappedFile::try_open(file) {
            Ok(mf) if mf.size() > 0 => return Some(mf),
            Ok(_) => fatal!("file is empty in '{}'", file.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !file.exists() => {}
            Err(e) => fatal!("{}", unreadable_file(file, &e)),
        }
    }
    let prober = Prober::new(ctx);
    let loader = Some((loader, loader_rpaths));
    MappedFile::open(&find_dylib_ref(ctx, &prober, name, loader, inlined)?)
}

/// Locates a re-exported library by its install name as load_reexports
/// does, but quietly: ahead of the link, or again after it.
fn find_reexport<E: Target>(ctx: &Context<E>, name: &[u8]) -> Option<&'static MappedFile> {
    let prober = Prober::quiet(ctx);
    MappedFile::open(&find_dylib_ref(ctx, &prober, name, None, false)?)
}

/// Finds the file of a dependent dylib's install name with `prober`, as
/// ld-prime looks for it: a name relative to its `loader` - the file of
/// the dylib that names it, whose directory @loader_path stands for
/// (looked up only for such a name: it resolves symbolic links), and
/// that dylib's rpaths - in its place there first; then by the name's end in the search paths
/// (see find_by_leaf); then the name itself, an absolute one under each
/// -syslibroot first (reexports between freshly built dylibs use
/// absolute install names outside any SDK) - but not where a stub has
/// the library `inlined`, which ld-prime takes in its place. Each place
/// is looked in for a stub too (see Prober::library).
fn find_dylib_ref<E: Target>(
    ctx: &Context<E>,
    prober: &Prober,
    name: &[u8],
    loader: Option<(&Path, &[PathBuf])>,
    inlined: bool,
) -> Option<PathBuf> {
    use mold_common::bytes::os_str;
    if let Some((loader, loader_rpaths)) = loader {
        if let Some(rest) = name.strip_prefix(b"@loader_path/") {
            if let Some(path) = prober.library(&dir_of(loader).join(os_str(rest))) {
                return Some(path);
            }
        } else if let Some(rest) = name.strip_prefix(b"@rpath/") {
            let mut rpaths = loader_rpaths.iter();
            if let Some(path) = rpaths.find_map(|rpath| prober.library(&rpath.join(os_str(rest)))) {
                return Some(path);
            }
        }
    }
    if let Some(path) = find_by_leaf(ctx, prober, name) {
        return Some(path);
    }
    let path = Path::new(os_str(name));
    if path.is_absolute() {
        for root in &ctx.args.syslibroot {
            if let Some(path) = prober.library(&under_root(root, path)) {
                return Some(path);
            }
        }
    }
    if inlined {
        return None;
    }
    prober.library(path)
}

/// Looks a dependent dylib up by the end of its install name in the
/// search paths, as ld-prime does before the name itself: a framework's
/// path from its .framework directory (/Foo.framework/Versions/A/Foo)
/// in each framework directory, another library's leaf
/// (libfoo.1.dylib) in each library directory - but for a library
/// inside a framework, which is looked up by its name alone.
fn find_by_leaf<E: Target>(ctx: &Context<E>, prober: &Prober, name: &[u8]) -> Option<PathBuf> {
    use memchr::{memmem, memrchr};
    use mold_common::bytes::os_str;
    use mold_common::path::path_bytes;
    let leaf = memrchr(b'/', name).map_or(name, |slash| &name[slash + 1..]);
    let framework_dir = [b"/", leaf, b".framework/"].concat();
    if leaf.len() < name.len() && memmem::rfind(name, &framework_dir).is_some() {
        let end = memmem::rfind(name, b".framework").unwrap();
        let from = &name[memrchr(b'/', &name[..end]).unwrap()..];
        return (ctx.args.framework_paths.iter())
            .find_map(|dir| prober.library(Path::new(os_str(&[path_bytes(dir), from].concat()))));
    }
    if leaf.ends_with(b".dylib") && memmem::find(name, b".framework/").is_some() {
        return None;
    }
    let leaf = Path::new(os_str(leaf));
    ctx.args.library_paths.iter().find_map(|dir| prober.library(&dir.join(leaf)))
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
/// read_file). The first library or framework not found, -force_load's
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
            (Some((_, true, name)), None) => fatal!("framework '{}' not found", name.display()),
            (Some((_, false, name)), None) => fatal!("library '{}' not found", name.display()),
            (None, None) => match arg {
                InputArg::BundleLoader(path) | InputArg::File(path) => {
                    fatal!("library '{}' not found", path.display())
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
                    warn!(
                        "-weak-l{0} resolved to a static library '{1}', but only dynamic libraries can be weak linked. Use -l{0} when linking static libraries, or make sure .dylib/.tbd library is located in -L search paths.",
                        name.display(),
                        path.display()
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
        true => format!("'-{opt}_framework {}'", name.display()),
        false => format!("'-{opt}-l{}'", name.display()),
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

/// Reads the `stubs` on all cores (see input_files::read_stub), and the
/// stubs of the libraries they re-export, down an SDK's umbrella trees,
/// so that the serial loop loading them finds every stub read.
fn prefetch_stubs<E: Target>(ctx: &Context<E>, stubs: &[&'static MappedFile]) {
    let seen = Prefetched::default();
    rayon::scope(|scope| {
        for &mf in stubs {
            prefetch_stub(ctx, scope, &seen, mf);
        }
    });
}

/// The stubs prefetch_stub has taken up, by the address of their
/// contents - a stub reached by two paths is one (see MappedFile) - and
/// the re-exported install names it has looked for.
#[derive(Default)]
struct Prefetched {
    files: std::sync::Mutex<hashbrown::HashSet<usize>>,
    names: std::sync::Mutex<hashbrown::HashSet<&'static [u8]>>,
}

/// Has `scope` read `mf` if it is a stub not taken up before, and then
/// likewise the stubs of the libraries it re-exports. Each stub is a
/// task of its own, which starts as soon as the stub re-exporting it is
/// read, as mold's mark_live_files visits each file it finds: the pool
/// stays busy with the small stubs while the large ones are read (an
/// SDK's SwiftUICore stub is 10 MB).
fn prefetch_stub<'s, E: Target>(
    ctx: &'s Context<E>,
    scope: &rayon::Scope<'s>,
    seen: &'s Prefetched,
    mf: &'static MappedFile,
) {
    let data = mf.data().as_ptr() as usize;
    if get_file_type(mf) != FileType::Tapi || !seen.files.lock().unwrap().insert(data) {
        return;
    }
    scope.spawn(move |scope| {
        let Some(stub) = input_files::read_stub(ctx, mf) else { return };
        for &name in stub.reexports() {
            if !stub.inlines(name)
                && seen.names.lock().unwrap().insert(name)
                && let Some(dep) = find_reexport(ctx, name)
            {
                prefetch_stub(ctx, scope, seen, dep);
            }
        }
    });
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
        ) => Some(name.as_encoded_bytes()),
        _ => None,
    };
    // (Less the suffix a -framework option may give after a comma.)
    let framework = framework.map(|name| name.split(|&c| c == b',').next().unwrap_or(name));
    if framework.is_some_and(|name| umbrellas.iter().any(|u| u == name)) {
        return true;
    }
    let stem = path.file_stem().map_or(&b""[..], |stem| stem.as_encoded_bytes());
    if !libraries.iter().any(|l| l == stem) {
        return false;
    }
    if framework.is_some() {
        warn!(
            "using -sub_library to re-export a framework is deprecated.  Use -reexport_framework instead"
        );
    }
    true
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
        FileType::Fat => filetype::fat_slice::<E>(&ctx.args, mf),
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
        None => read_file(ctx, mf, rc, out),
    }
}

/// Loads the files that -dylib_file names for re-exported libraries
/// but that are no libraries (see load_reexports) as any input, after
/// the command line's: ld-prime links an object named so into the
/// output.
fn collect_indirect_files<E: Target>(ctx: &mut Context<E>, out: &mut Vec<PendingObject>) {
    for mf in std::mem::take(&mut ctx.indirect_files) {
        read_file(ctx, mf, ReaderContext::default(), out);
    }
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

/// A parsed-input request: a file to stage as an object, with its
/// liveness and input-order priority.
struct PendingObject {
    mf: &'static MappedFile,
    alive: bool,
    hidden: bool,
    priority: u32,
}

/// Classifies one input file. Dylib stubs and binaries are registered
/// immediately (they are cheap and order-sensitive); objects and
/// archive members are queued for parallel staging; bitcode is
/// registered immediately since libLTO calls are kept on one thread.
fn read_file<E: Target>(
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
            error!(
                "dylib cannot be merged, not built with -make_mergeable in '{}'",
                mf.name.display()
            );
        }
        // A relocatable output keeps every reference undefined for the
        // final link, and the other images that link no dylib have
        // nothing to load one with (Args::links_dylibs): ld-prime reads
        // a dylib on their command lines (and ignores a stub without
        // the architecture as ever), then ignores it with a warning.
        FileType::Tapi | FileType::Dylib if ctx.args.relocatable || !ctx.args.links_dylibs() => {
            if ty == FileType::Dylib || input_files::load_tbd(ctx, mf).is_some() {
                warn!("ignoring unexpected dylib '{}'", mf.name.display());
            }
        }
        FileType::Dylib if rc.merge => merge_dylib(ctx, mf, out),
        FileType::Tapi | FileType::Dylib => load_dylib(ctx, mf, rc),
        FileType::Archive => collect_archive_members(ctx, mf, rc, out),
        FileType::Fat => match filetype::fat_slice::<E>(&ctx.args, mf) {
            Some(slice) => read_file(ctx, slice, rc, out),
            None => input_files::warn_fat_missing_arch(ctx, mf),
        },
        FileType::LlvmBitcode => {
            crate::lto::read_lto_object(ctx, mf, true);
        }
        FileType::Empty => {}
        _ => refuse_file(mf),
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

/// Whether an object or dylib is for an architecture the link doesn't
/// take (see filetype::takes_arch), which ld-prime ignores with a
/// warning - an archive member too, whether the link needs it or not.
/// -allow_sub_type_mismatches has it take one of another subtype with
/// a warning instead.
pub(crate) fn is_foreign<E: Target>(ctx: &Context<E>, mf: &MappedFile) -> bool {
    let Some(arch) = filetype::foreign_arch::<E>(mf) else { return false };
    if ctx.args.allow_sub_type_mismatches && filetype::is_subtype_mismatch::<E>(mf) {
        let name = filetype::without_fat_arch(path_bytes(&mf.name));
        warn!("linking {arch} file '{}' into {} link", display(&name), E::NAME);
        return false;
    }
    let why = format!("found architecture '{arch}', required architecture '{}'", E::NAME);
    input_files::ignore_foreign_file(ctx, mf, &why);
    true
}

/// Refuses a file the link can't take, by what it is.
fn refuse_file(mf: &MappedFile) {
    let name = filetype::without_fat_arch(path_bytes(&mf.name));
    let name = display(&name);
    if crate::filetype::get_macho_filetype(mf.data()).is_some() {
        error!(
            "unsupported mach-o filetype (only MH_OBJECT and MH_DYLIB can be linked) in '{name}'"
        );
    } else {
        error!("unknown file type in '{name}'");
    }
}

/// Refuses an image the link reads that has no LC_UUID (see
/// input_files::has_uuid).
fn refuse_without_uuid(mf: &MappedFile) {
    error!("missing LC_UUID load command in '{}'", mf.name.display());
}

/// Loads a dylib or its stub, and the public libraries it re-exports,
/// and gives it what its naming says - unless it refuses this link.
fn load_dylib<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile, rc: ReaderContext) {
    if refuses_client(ctx, mf, rc) {
        return;
    }
    let is_stub = get_file_type(mf) == FileType::Tapi;
    if !is_stub && !input_files::has_uuid(mf.data()) {
        refuse_without_uuid(mf);
        return;
    }
    let first = ctx.dylibs.len();
    let idx = if is_stub {
        input_files::parse_tbd(ctx, mf)
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
        display(leaf)
    );
    if !rc.autolinked {
        error!("{msg}");
    } else {
        let msg =
            format_args!("Could not parse or use implicit file '{}': {msg}", mf.name.display());
        ctx.autolink_misses.push(msg.to_string());
    }
    true
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
        warn!(
            "delay-init link with '{}' will be ignored because it has weak-def exports",
            display(&dylib.install_name)
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
            let name = display(&dylib.install_name);
            warn!("re-exported dylibs cannot be weak-linked: {name}");
            dylib.is_weak = false;
        }
        dylib.is_reexported = true;
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

/// Queues an archive's members. Every member is parsed eagerly; whether
/// it is *live* - whether its content reaches the output - is decided
/// by symbol resolution and the liveness walk. -all_load and -force_load
/// make every member live up front; -ObjC does so for members with
/// Objective-C metadata, which register classes by their mere presence,
/// but only in the archives the command line names: ld-prime loads an
/// archive that only an auto-link option or -possible-l names (a Swift
/// object's `-framework X` for a static framework) for the symbols its
/// members resolve, as without -ObjC. ld64 exempts clang's runtime
/// library (libclang_rt.*.a, which the compiler driver adds to every
/// link) from -all_load: its members are wanted only when referenced.
fn collect_archive_members<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    rc: ReaderContext,
    out: &mut Vec<PendingObject>,
) {
    let all_load = ctx.args.all_load
        && !mf.name.file_name().is_some_and(|f| f.as_encoded_bytes().starts_with(b"libclang_rt"));
    if rc.force_load {
        ctx.force_loaded.insert(mf.name.clone());
    }
    for member in members(&mf.name, mf.data()).map(|member| mf.member(&member)) {
        input_files::trace_file(ctx, path_bytes(&member.name));
        let alive = rc.force_load
            || all_load
            || (ctx.args.load_objc && !rc.autolinked && input_files::has_objc_sections(member));
        match get_file_type(member) {
            FileType::LlvmBitcode => {
                crate::lto::read_lto_object(ctx, member, alive);
            }
            FileType::Object if is_foreign(ctx, member) => {}
            _ => {
                let priority = ctx.next_priority();
                out.push(PendingObject { mf: member, alive, hidden: rc.hidden, priority });
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
    for obj in &staged {
        obj.warn_about_sections();
    }

    // Intern every staged object's global names in one parallel batch
    // (mold's sharded symbol table), so the serial integration loop
    // below does no hashing. Names carry the xxh3 hashes staging
    // computed alongside them, so nothing here touches their bytes.
    // Each object's extern (name, hash) list is filtered in parallel -
    // a debug link scans millions of MachSyms here - then concatenated in
    // object order into the batch the sharded intern resolves at once.
    let per_obj: Vec<Vec<(&'static [u8], u64)>> = staged
        .par_iter()
        .map(|st| {
            let r = st.global_range();
            st.sym_names[r.clone()]
                .iter()
                .zip(&st.sym_hashes[r.clone()])
                .zip(st.mach_syms[r].iter())
                .filter(|((_, _), msym)| !msym.is_stab() && msym.is_extern())
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

/// Acts on the auto-link options (LC_LINKER_OPTION) of the live
/// objects, once symbols are resolved: each names a library or
/// framework the object needs, as if it had been on the command line.
/// Swift objects rely on this entirely. Returns whether resolution must
/// start over (see passes::resolve_symbols): new objects change what it
/// chose and may make more objects live, with options of their own, and
/// so does a new dylib that an earlier one merged as a private
/// re-export, which takes its symbols from that one. Other new dylibs
/// only claim what is still undefined (see passes::claim_new_dylibs).
pub fn load_autolink_deps<E: Target>(ctx: &mut Context<E>) -> bool {
    if ctx.args.ignore_auto_link {
        return false;
    }
    let _t = ctx.timer("load_autolink_deps");
    // Those of objects new to the link are read, in parallel (a Swift
    // object has dozens), and warned of in object order.
    let new: Vec<&mut input_files::ObjectFile> =
        ctx.objs.iter_mut().filter(|obj| obj.is_reachable && !obj.linker_options_read).collect();
    let warnings: Vec<Vec<String>> = new
        .into_par_iter()
        .map(|obj| {
            let mf = obj.mf;
            let (opts, warnings) = read_linker_options(&obj.linker_options, || mf.name.display());
            obj.linker_options = opts;
            obj.linker_options_read = true;
            warnings
        })
        .collect();
    for msg in warnings.iter().flatten() {
        warn!("{msg}");
    }
    // ld64 does not act on auto-link options in a -r link: the
    // LC_LINKER_OPTION commands are copied into the output object and
    // the final link resolves them. Loading them here would let the
    // libraries claim symbols that the output must leave undefined
    // (Xcode's prelink of a Swift package auto-linked libc++ this way
    // and the -r symbol table then lacked operator new).
    if ctx.args.relocatable {
        return false;
    }

    let (num_objs, num_dylibs) = (ctx.objs.len(), ctx.dylibs.len());
    load_autolinked_libraries(ctx);
    let (old, new) = ctx.dylibs.split_at(num_dylibs);
    let rebinds =
        new.iter().any(|d| old.iter().any(|o| o.merged_reexports.contains(&d.install_name)));
    ctx.objs.len() != num_objs || rebinds
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
    let objs = ctx.objs.iter().filter(|obj| obj.is_reachable);
    let opts = ctx.cmdline_linker_options.iter().flatten();
    let opts: hashbrown::HashSet<&Vec<Vec<u8>>> =
        opts.chain(objs.flat_map(|obj| &obj.linker_options)).collect();
    let mut pending: Vec<Vec<Vec<u8>>> = (opts.into_iter())
        .filter(|opt| !ctx.processed_linker_options.contains(*opt))
        .cloned()
        .collect();
    pending.sort();
    let found = prefetch_autolinked_stubs(ctx, &pending);
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
    for (opt, (path, log)) in pending.into_iter().zip(found) {
        ctx.processed_linker_options.insert(opt.clone());
        log.replay(ctx);
        let rc = autolinked_input(ctx, &opt, path.as_deref());
        if let Some(path) = path
            && let Some(mf) = MappedFile::open(&path)
        {
            read_file(ctx, mf, rc, &mut queue);
        }
    }
    // The libraries only -possible-l and the like name come next, once,
    // in command line order: ld-prime binds a symbol both they and an
    // auto-linked library define to the auto-linked one.
    for path in std::mem::take(&mut ctx.possible_files) {
        match MappedFile::try_open(&path) {
            Ok(mf) if mf.size() == 0 => error!("file is empty in '{}'", path.display()),
            Ok(mf) => {
                let sdk = searched_in_sdk(&ctx.args, &path);
                let rc = ReaderContext { autolinked: true, sdk, ..Default::default() };
                read_file(ctx, mf, rc, &mut queue);
            }
            Err(e) => error!("{}", unreadable_file(&path, &e)),
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

/// Looks for the libraries the auto-link options `opts` name, in
/// parallel, and reads their stubs as they are found (see
/// prefetch_stub), ahead of load_autolinked_libraries' serial loop,
/// which takes the file found for each option and gives what its
/// search noted and warned of in its turn. (mold's read_input_files
/// looks for each library in the task that reads it.)
fn prefetch_autolinked_stubs<E: Target>(
    ctx: &Context<E>,
    opts: &[Vec<Vec<u8>>],
) -> Vec<(Option<PathBuf>, ProbeLog)> {
    let seen = Prefetched::default();
    rayon::scope(|scope| {
        let find = |opt: &Vec<Vec<u8>>| {
            let log = ProbeLog::default();
            let path = find_autolinked(ctx, &Prober::recording(ctx, &log), opt);
            if let Some(mf) = path.as_ref().and_then(|path| MappedFile::try_open(path).ok()) {
                prefetch_stub(ctx, scope, &seen, mf);
            }
            (path, log)
        };
        opts.par_iter().map(find).collect()
    })
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
/// ld-prime reads the same way). Returns what is kept, and the warnings
/// for the caller to give.
fn read_linker_options<F: std::fmt::Display>(
    opts: &[Vec<Vec<u8>>],
    file: impl Fn() -> F,
) -> (Vec<Vec<Vec<u8>>>, Vec<String>) {
    let words: Vec<&[u8]> = opts.iter().flatten().map(Vec::as_slice).collect();
    let mut warnings = Vec::new();
    let ignored = |kind: &str, what: &[u8]| {
        let (what, file) = (display(what), file());
        format!("{kind} linker option from object file ignored: '{what}' in {file}")
    };
    let malformed = |opt: &str| {
        let file = file();
        format!(
            "malformed linker option from object file ignored: '{opt}' missing argument, in {file}"
        )
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
                    warnings.push(malformed(std::str::from_utf8(word).unwrap()));
                    continue;
                };
                match word {
                    b"-l" => libs.push(vec![[word, arg].concat()]),
                    b"-weak_framework" | b"-reexport_framework" | b"-upward_framework" => {
                        warnings.push(ignored("unexpected", &[word, b" ", arg].concat()))
                    }
                    b"-weak_library" | b"-reexport_library" | b"-upward_library" => {
                        let kind = word.strip_suffix(b"_library").unwrap();
                        warnings.push(ignored("unexpected", &[kind, b"-l", arg].concat()));
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
                Some(prefix) if word.len() == prefix.len() => warnings.push(malformed(prefix)),
                Some("-weak-l" | "-reexport-l" | "-upward-l") => {
                    warnings.push(ignored("unexpected", word))
                }
                Some("-L" | "-F") => {}
                Some(_) => libs.push(vec![word.to_vec()]),
                None => warnings.push(ignored("unknown", word)),
            },
        }
    }
    (libs, warnings)
}

/// How to load `path`, the file found for an auto-link option read by
/// read_linker_options. A library or framework not found is remembered
/// for passes::report_undef_errors.
fn autolinked_input<E: Target>(
    ctx: &mut Context<E>,
    opt: &[Vec<u8>],
    path: Option<&Path>,
) -> ReaderContext {
    let rc = ReaderContext { autolinked: true, ..Default::default() };
    match opt {
        [lib] => {
            if path.is_none() {
                ctx.autolink_misses.push(missing_hint(false, autolinked_library(lib)));
            }
            let sdk = path.is_some_and(|path| searched_in_sdk(&ctx.args, path));
            // -force_load_swift_libs loads a Swift library's archive
            // whole, by its file name.
            let is_swift = |path: &Path| {
                path.file_name().is_some_and(|f| f.as_encoded_bytes().starts_with(b"libswift"))
            };
            let force_load = ctx.args.force_load_swift_libs && path.is_some_and(is_swift);
            let hidden = lib.starts_with(b"-hidden-l");
            ReaderContext { force_load, hidden, sdk, ..rc }
        }
        [flag, name] if flag.ends_with(b"framework") => {
            if path.is_none() {
                ctx.autolink_misses.push(missing_hint(true, name));
            }
            let sdk = path.is_some_and(|path| searched_in_sdk(&ctx.args, path));
            ReaderContext { sdk, ..rc }
        }
        [flag, _] if flag == b"-force_load" => ReaderContext { force_load: true, ..rc },
        _ => rc,
    }
}

/// Looks with `prober` for the file an auto-link option names: the
/// library or framework it names, or the file itself.
fn find_autolinked<E: Target>(
    ctx: &Context<E>,
    prober: &Prober,
    opt: &[Vec<u8>],
) -> Option<PathBuf> {
    let os_str = mold_common::bytes::os_str;
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
fn missing_hint(framework: bool, name: &[u8]) -> String {
    if framework {
        let base = name.split(|&c| c == b',').next().unwrap();
        format!(
            "Could not find or use auto-linked framework '{}': framework '{}' not found",
            display(base),
            display(name)
        )
    } else {
        format!(
            "Could not find or use auto-linked library '{0}': library '{0}' not found",
            display(name)
        )
    }
}
