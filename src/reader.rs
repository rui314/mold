//! Reading input files.
//!
//! Reading is I/O- and CPU-intensive and a large program has tens of
//! thousands of input files, so files are read in parallel. The command
//! line is inherently sequential, though: options such as `--as-needed`
//! apply to the files after them, and a file's priority for symbol
//! resolution is its position. The argument parser records the reader
//! state and position for each file, files are read in any order, and
//! the results are sorted back into command line order.

use std::path::Path;
use std::sync::Mutex;

use rayon::prelude::*;

use crate::arch::Arch;
use crate::archive_file;
use crate::cmdline::{ReaderContext, ReaderJob};
use crate::context::Context;
use crate::filetype::{self, FileType};
use crate::input_files::{DsoId, FileId, ObjId, ObjectFile, SharedFile};
use crate::linker_script::Script;
use crate::mapped_file::{must_open_file, open_file, MappedFile};
use crate::util::perf::Counter;
use crate::{fatal, out, warn};

/// A file that has been read, with its command line position.
enum Loaded<E: Arch> {
    Obj(Vec<u32>, Box<ObjectFile<E>>),
    Dso(Vec<u32>, Box<SharedFile<E>>),
}

/// Appends a result to the current worker's bin.
fn push_to_worker<T>(bins: &[Mutex<Vec<T>>], value: T) {
    let worker = rayon::current_thread_index().unwrap_or(bins.len() - 1);
    bins[worker].lock().unwrap().push(value);
}

fn get_file_type<E: Arch>(ctx: &Context<E>, mf: &MappedFile) -> FileType {
    filetype::get_file_type(&ctx.args.plugin, mf)
}

/// Returns the target a file was compiled for.
pub fn get_machine_type<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
) -> Option<&'static str> {
    match get_file_type(ctx, mf) {
        FileType::Text => crate::linker_script::output_target(ctx, rctx, mf),
        _ => filetype::get_machine_type(&ctx.args.plugin, &ctx.args.chroot, mf, || None),
    }
}

fn check_machine_type<E: Arch>(ctx: &Context<E>, mf: &'static MappedFile) {
    let target = filetype::get_machine_type(&ctx.args.plugin, &ctx.args.chroot, mf, || None);
    match target {
        None => fatal!("{}: unknown machine type", mf.name.display()),
        Some(t) if t != ctx.args.emulation => {
            fatal!(
                "{}: incompatible file type: {} is expected but got {t}",
                mf.name.display(),
                ctx.args.emulation
            )
        }
        _ => {}
    }
}

fn new_object_file<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    archive_name: &'static Path,
) -> ObjectFile<E> {
    static COUNT: Counter = Counter::new("parsed_objs");
    COUNT.increment();

    check_machine_type(ctx, mf);
    let mut file = ObjectFile::<E>::new(mf, archive_name);
    file.base.as_needed =
        rctx.in_lib || (!archive_name.as_os_str().is_empty() && !rctx.whole_archive);
    file.base.set_reachable(!file.base.as_needed);
    file.register_global_symbols(&ctx.args, &mut ctx.symbol_bin());
    file
}

fn new_shared_file<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
) -> SharedFile<E> {
    if rctx.is_static {
        fatal!("{}: attempted static link of a dynamic object", mf.name.display());
    }
    check_machine_type(ctx, mf);
    let mut file = SharedFile::<E>::new(mf, &mut ctx.symbol_bin());
    file.base.as_needed = rctx.as_needed;
    file.base.set_reachable(!file.base.as_needed);
    file
}

// IR files for LTO are not read in place. We only record them here,
// and read_input_files() hands them to the LTO plugin once all input
// files have been found. We do this because LTO object file reading
// is order-dependent.
fn defer_lto_object<E: Arch>(
    ctx: &Context<E>,
    rctx: ReaderContext,
    mf: &'static MappedFile,
    archive_name: &'static Path,
) {
    ctx.lto_jobs.lock().unwrap().push((rctx, mf, archive_name));
}

/// Reads an IR object through the LTO plugin. An object listed by
/// `--:ignore-ir-file` is an archive member a previous pass found
/// unneeded.
fn new_lto_object<E: Arch>(
    ctx: &mut Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    archive_name: &'static Path,
) -> Option<ObjectFile<E>> {
    static COUNT: Counter = Counter::new("parsed_lto_objs");
    COUNT.increment();

    if ctx.args.ignore_ir_file.contains(&mf.identifier()) {
        return None;
    }
    let mut file = crate::lto::read_lto_object(ctx, mf, archive_name)?;
    file.base.as_needed =
        rctx.in_lib || (!file.archive_name.as_os_str().is_empty() && !rctx.whole_archive);
    file.base.set_reachable(!file.base.as_needed);
    file.register_global_symbols(&ctx.args, &mut ctx.symbol_bin());
    Some(file)
}

// Reads a file inside an archive.
fn read_archive_member<E: Arch>(
    ctx: &Context<E>,
    rctx: ReaderContext,
    mf: &'static MappedFile,
    archive_name: &'static Path,
) -> Option<Loaded<E>> {
    match get_file_type(ctx, mf) {
        FileType::ElfObj => {
            let file = new_object_file(ctx, &rctx, mf, archive_name);
            Some(Loaded::Obj(rctx.pos, Box::new(file)))
        }
        FileType::GccLtoObj | FileType::LlvmBitcode => {
            defer_lto_object(ctx, rctx, mf, archive_name);
            None
        }
        FileType::ElfDso => {
            warn!(
                "{}({}): shared object file in an archive is ignored",
                archive_name.display(),
                mf.name.display()
            );
            None
        }
        _ => None,
    }
}

// Reads the given file, which is located at rctx.pos in the command
// line. If the file is a container, i.e. an archive file or a linker
// script, the files in it are read in place, recursively. IR files
// for LTO are only recorded; see defer_lto_object().
//
// read_input_files() reads top-level files with this function too but
// overrides the container cases to read archive members in parallel.
pub fn read_file<E: Arch>(ctx: &mut Context<E>, rctx: &mut ReaderContext, mf: &'static MappedFile) {
    match get_file_type(ctx, mf) {
        FileType::ElfObj => {
            let file = new_object_file(ctx, rctx, mf, Path::new(""));
            push_loaded(ctx, Loaded::Obj(rctx.pos.clone(), Box::new(file)));
        }
        FileType::ElfDso => {
            let file = new_shared_file(ctx, rctx, mf);
            push_loaded(ctx, Loaded::Dso(rctx.pos.clone(), Box::new(file)));
        }
        FileType::Ar | FileType::ThinAr => {
            for child in archive_file::read_archive_members(&ctx.args.chroot, mf) {
                let child_rctx = rctx.next_child();
                if let Some(loaded) = read_archive_member(ctx, child_rctx, child, &mf.name) {
                    push_loaded(ctx, loaded);
                }
            }
        }
        FileType::Text => Script::new(ctx, rctx, mf).parse_linker_script(),
        FileType::GccLtoObj | FileType::LlvmBitcode => {
            defer_lto_object(ctx, rctx.clone(), mf, Path::new(""));
        }
        _ => fatal!("{}: unknown file type", mf.name.display()),
    }
}

fn push_loaded<E: Arch>(ctx: &mut Context<E>, loaded: Loaded<E>) {
    match loaded {
        Loaded::Obj(pos, file) => {
            let id = ObjId(ctx.objs.push(file));
            ctx.pending_files.push((pos, FileId::Obj(id)));
        }
        Loaded::Dso(pos, file) => {
            let id = DsoId(ctx.dsos.push(file));
            ctx.pending_files.push((pos, FileId::Dso(id)));
        }
    }
}

/// Deduces the target from the first recognizable input file.
pub fn detect_machine_type<E: Arch>(ctx: &mut Context<E>, jobs: &[ReaderJob]) -> &'static str {
    for job in jobs {
        if job.is_lib {
            continue;
        }
        if let Some(mf) = open_file(&ctx.args.chroot, &job.name) {
            if get_file_type(ctx, mf) != FileType::Text {
                if let Some(target) =
                    filetype::get_machine_type(&ctx.args.plugin, &ctx.args.chroot, mf, || None)
                {
                    return target;
                }
            }
        }
    }
    for job in jobs {
        if job.is_lib {
            continue;
        }
        if let Some(mf) = open_file(&ctx.args.chroot, &job.name) {
            if get_file_type(ctx, mf) == FileType::Text {
                if let Some(target) = crate::linker_script::output_target(ctx, &job.rctx, mf) {
                    return target;
                }
            }
        }
    }
    fatal!("-m option is missing");
}

fn open_library<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    path: &std::path::Path,
) -> Option<&'static MappedFile> {
    let mf = open_file(&ctx.args.chroot, path)?;
    if let Some(target) = get_machine_type(ctx, rctx, mf) {
        if target != E::NAME {
            warn!(
                "{}: skipping incompatible file: {target} (e_machine {})",
                path.display(), E::E_MACHINE
            );
            return None;
        }
    }
    Some(mf)
}

/// Finds a library given to `-l`.
pub fn find_library<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    name: &std::ffi::OsStr,
) -> &'static MappedFile {
    if let Some(exact) = name.as_encoded_bytes().strip_prefix(b":") {
        let exact = std::path::Path::new(crate::util::os_str(exact));
        let exact = exact.strip_prefix("/").unwrap_or(exact);
        for dir in &ctx.args.library_paths {
            if let Some(mf) = open_library(ctx, rctx, &dir.join(exact)) {
                return mf;
            }
        }
    } else {
        let mut stem = std::ffi::OsString::from("lib");
        stem.push(name);
        let shared = (!rctx.is_static).then(|| {
            let mut filename = stem.clone();
            filename.push(".so");
            filename
        });
        let mut archive = stem;
        archive.push(".a");
        for dir in &ctx.args.library_paths {
            for filename in shared.iter().chain(std::iter::once(&archive)) {
                if let Some(mf) = open_library(ctx, rctx, &dir.join(filename)) {
                    return mf;
                }
            }
        }
    }
    fatal!("library not found: {}", name.to_string_lossy());
}

// Reads all input files.
//
// Reading input files is I/O- and CPU-intensive, and a large program
// can easily consist of tens of thousands of them, so we want to read
// files in parallel. The command line, on the other hand, is
// inherently sequential: options such as --as-needed or
// --whole-archive apply to the files after them, and a file's
// priority for symbol resolution is its position in the command line.
//
// We reconcile the two as follows: the command line parser has
// already recorded the reader state and the position for each file
// argument in its ReaderJob. We open and read files in parallel and
// then sort the files we've found back into the command line order to
// assign priorities.
pub fn read_input_files<E: Arch>(ctx: &mut Context<E>, jobs: Vec<ReaderJob>) {
    let _t = ctx.timer("read_input_files");

    // Open and read files in parallel. Archive files are expanded into
    // one job per member so that members are read in parallel too.
    //
    // Linker scripts are one exception to the parallelism: they can
    // modify the context, e.g. by defining symbol versions, so we only
    // collect them here and parse them after this loop, one at a time
    // and in the command line order, to keep their effects
    // deterministic. Scripts given as input files are rare and small,
    // such as the GROUP file that glibc installs as libc.so, so the
    // lost parallelism doesn't matter.
    //
    // IR files for LTO are the other exception. This loop only records
    // them, and we hand them to the LTO plugin after the scripts have
    // been parsed, also in the command line order.
    //
    // Parsing scripts late assumes that no script directive affects how
    // command line arguments after the script are read. That holds for
    // the directives we currently support: a script can only add input
    // files, whose positions order them correctly, and define symbol
    // versions or symbols, which are not used until after this
    // function. If we add a directive that doesn't satisfy this, such
    // as SEARCH_DIR, which affects how -l arguments after it are
    // resolved, this scheme needs to be revisited.

    // Keep one bin per worker, locking only to append a result after I/O and
    // parsing. Command line positions restore the order after the jobs join.
    let workers = rayon::current_num_threads();
    let scripts: Vec<_> = (0..=workers).map(|_| Mutex::new(Vec::new())).collect();
    let loaded: Vec<_> = (0..=workers).map(|_| Mutex::new(Vec::new())).collect();

    let ctx_ref: &Context<E> = ctx;
    rayon::scope(|scope| {
        jobs.into_par_iter().for_each(|job| {
            let mut rctx = job.rctx;

            // Open the input named by this command line argument.
            let mf = if job.is_lib {
                let mf = find_library(ctx_ref, &rctx, job.name.as_os_str());
                crate::util::leak(MappedFile {
                    name: mf.name.clone(),
                    data: mf.data,
                    given_fullpath: false,
                    parent: mf.parent,
                    thin_parent: mf.thin_parent,
                    is_dependency: std::sync::atomic::AtomicBool::new(true),
                })
            } else {
                must_open_file(&ctx_ref.args.chroot, &job.name)
            };

            // Read each archive member in its own task, including opening
            // thin archive members, so that file I/O runs in parallel too.
            match get_file_type(ctx_ref, mf) {
                FileType::Ar => {
                    for child in archive_file::read_fat_archive_members(mf) {
                        let child_rctx = rctx.next_child();
                        let archive_name = mf.name.as_path();
                        let loaded = &loaded;
                        scope.spawn(move |_| {
                            if let Some(file) =
                                read_archive_member(ctx_ref, child_rctx, child, archive_name)
                            {
                                push_to_worker(loaded, file);
                            }
                        });
                    }
                }
                FileType::ThinAr => {
                    for path in archive_file::get_thin_archive_member_paths(mf) {
                        let child_rctx = rctx.next_child();
                        let archive_name = mf.name.as_path();
                        let loaded = &loaded;
                        scope.spawn(move |_| {
                            let child = mf.open_thin_member(&ctx_ref.args.chroot, &path);
                            if let Some(file) =
                                read_archive_member(ctx_ref, child_rctx, child, archive_name)
                            {
                                push_to_worker(loaded, file);
                            }
                        });
                    }
                }
                FileType::Text => push_to_worker(&scripts, (rctx, mf)),
                FileType::ElfObj => {
                    let file = new_object_file(ctx_ref, &rctx, mf, Path::new(""));
                    push_to_worker(&loaded, Loaded::Obj(rctx.pos, Box::new(file)));
                }
                FileType::ElfDso => {
                    let file = new_shared_file(ctx_ref, &rctx, mf);
                    push_to_worker(&loaded, Loaded::Dso(rctx.pos, Box::new(file)));
                }
                FileType::GccLtoObj | FileType::LlvmBitcode => {
                    defer_lto_object(ctx_ref, rctx, mf, Path::new(""));
                }
                _ => fatal!("{}: unknown file type", mf.name.display()),
            }
        });
    });

    for l in loaded.into_iter().flat_map(|bin| bin.into_inner().unwrap()) {
        push_loaded(ctx, l);
    }

    // Parse linker scripts and read the files they name.
    let mut scripts: Vec<_> = scripts
        .into_iter()
        .flat_map(|bin| bin.into_inner().unwrap())
        .collect();
    scripts.sort_by(|(a, _), (b, _)| a.pos.cmp(&b.pos));
    for (mut rctx, mf) in scripts {
        Script::new(ctx, &mut rctx, mf).parse_linker_script();
    }

    // Hand IR files to the LTO plugin, in the command line order. The
    // plugin keeps global state, so claims can't run in parallel anyway,
    // and LLVM's LTO assumes that the module holding the prevailing copy
    // of a COMDAT group arrives before the modules holding the other
    // copies, as it does with a serial linker. Otherwise the losing
    // copies' internal symbols become undefined references in the LTO
    // result.
    let mut lto_jobs = std::mem::take(ctx.lto_jobs.get_mut().unwrap());
    lto_jobs.sort_by(|(a, ..), (b, ..)| a.pos.cmp(&b.pos));
    for (rctx, mf, archive_name) in lto_jobs {
        if let Some(file) = new_lto_object(ctx, &rctx, mf, archive_name) {
            push_loaded(ctx, Loaded::Obj(rctx.pos, Box::new(file)));
        }
    }

    // Sort the files into the command line order and assign priorities.
    let mut pending = std::mem::take(&mut ctx.pending_files);
    pending.sort_by(|a, b| a.0.cmp(&b.0));

    let objs = std::mem::take(&mut ctx.objs);
    let dsos = std::mem::take(&mut ctx.dsos);
    let mut objs: Vec<Option<Box<ObjectFile<E>>>> = objs.into_iter().map(Some).collect();
    let mut dsos: Vec<Option<Box<SharedFile<E>>>> = dsos.into_iter().map(Some).collect();

    // Priority 0 is reserved for the internal object file. LTO-generated
    // files use priorities beginning at 100, so regular files begin at 10000.
    ctx.file_by_priority.push(None);
    for (_, id) in pending {
        let priority = 10000 + ctx.file_by_priority.len() as u32 - 1;
        match id {
            FileId::Obj(idx) => {
                let mut file = objs[idx.index()].take().unwrap();
                file.base.priority = priority;
                if ctx.args.trace {
                    out!("trace: {file}");
                }
                let id = ObjId(ctx.objs.push(file));
                ctx.file_by_priority.push(Some(FileId::Obj(id)));
            }
            FileId::Dso(idx) => {
                let mut file = dsos[idx.index()].take().unwrap();
                file.base.priority = priority;
                if ctx.args.trace {
                    out!("trace: {file}");
                }
                let id = DsoId(ctx.dsos.push(file));
                ctx.file_by_priority.push(Some(FileId::Dso(id)));
            }
        }
    }

    if ctx.objs.is_empty() && ctx.dsos.is_empty() {
        fatal!("no input files");
    }
}
