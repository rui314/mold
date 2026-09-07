//! Reading input files.
//!
//! Reading is I/O- and CPU-intensive and a large program has tens of
//! thousands of input files, so files are read in parallel. The command
//! line is inherently sequential, though: options such as `--as-needed`
//! apply to the files after them, and a file's priority for symbol
//! resolution is its position. The argument parser records the reader
//! state and position for each file, files are read in any order, and
//! the results are sorted back into command line order.

use std::cell::UnsafeCell;

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

/// A concurrent vector implemented as one bin per Rayon worker. Writers on
/// different workers never contend, and ordering is restored from ReaderJob
/// positions after all input has been read.
struct WorkerBins<T> {
    bins: Vec<UnsafeCell<Vec<T>>>,
}

// SAFETY: a Rayon worker has a unique index and executes at most one closure
// at a time. Callers do not invoke nested parallel work while pushing, and the
// bins are consumed only after their scoped traversals have joined.
unsafe impl<T: Send> Sync for WorkerBins<T> {}

impl<T> WorkerBins<T> {
    fn new() -> WorkerBins<T> {
        WorkerBins {
            bins: (0..=rayon::current_num_threads())
                .map(|_| UnsafeCell::new(Vec::new()))
                .collect(),
        }
    }

    fn push(&self, value: T) {
        let fallback = self.bins.len() - 1;
        let i = rayon::current_thread_index()
            .unwrap_or(fallback)
            .min(fallback);
        // SAFETY: the Sync invariant above gives this worker exclusive access
        // to its indexed vector for the duration of this non-nested closure.
        unsafe { &mut *self.bins[i].get() }.push(value);
    }

    fn into_vec(self) -> Vec<T> {
        self.bins
            .into_iter()
            .flat_map(UnsafeCell::into_inner)
            .collect()
    }
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
        _ => filetype::get_machine_type(&ctx.args.plugin, mf, || None),
    }
}

fn new_object_file<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    archive_name: &str,
) -> ObjectFile<E> {
    static COUNT: Counter = Counter::new("parsed_objs");
    COUNT.increment();

    let target = filetype::get_machine_type(&ctx.args.plugin, mf, || None);
    match target {
        None => fatal!("{}: unknown machine type", mf.name),
        Some(t) if t != ctx.args.emulation => {
            fatal!(
                "{}: incompatible file type: {} is expected but got {t}",
                mf.name,
                ctx.args.emulation
            )
        }
        _ => {}
    }
    let mut file = ObjectFile::<E>::new(mf, archive_name.to_string());
    file.base.as_needed = rctx.in_lib || (!archive_name.is_empty() && !rctx.whole_archive);
    file.register_global_symbols(&ctx.args, &mut ctx.symbol_bin());
    file
}

fn new_shared_file<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
) -> SharedFile<E> {
    if rctx.is_static {
        fatal!("{}: attempted static link of a dynamic object", mf.name);
    }
    let target = filetype::get_machine_type(&ctx.args.plugin, mf, || None);
    match target {
        None => fatal!("{}: unknown machine type", mf.name),
        Some(t) if t != ctx.args.emulation => {
            fatal!(
                "{}: incompatible file type: {} is expected but got {t}",
                mf.name,
                ctx.args.emulation
            )
        }
        _ => {}
    }
    let mut file = SharedFile::<E>::new(mf);
    file.base.as_needed = rctx.as_needed;
    file
}

// IR files for LTO are not read in place. We only record them here,
// and read_input_files() hands them to the LTO plugin once all input
// files have been found. We do this because LTO object file reading
// is order-dependent.
fn defer_lto_object<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    archive_name: &str,
) {
    let job = ReaderJob {
        rctx: rctx.clone(),
        mf: Some(mf),
        archive_name: archive_name.to_string(),
        ..ReaderJob::default()
    };
    ctx.lto_jobs.lock().unwrap().push(job);
}

/// Reads an IR object through the LTO plugin. An object listed by
/// `--:ignore-ir-file` is an archive member a previous pass found
/// unneeded.
fn new_lto_object<E: Arch>(
    ctx: &mut Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    archive_name: String,
) -> Option<ObjectFile<E>> {
    static COUNT: Counter = Counter::new("parsed_lto_objs");
    COUNT.increment();

    if ctx.args.ignore_ir_file.contains(&mf.identifier()) {
        return None;
    }
    let mut file = crate::lto::read_lto_object(ctx, mf, archive_name)?;
    file.base.as_needed = rctx.in_lib || (!file.archive_name.is_empty() && !rctx.whole_archive);
    file.register_global_symbols(&ctx.args, &mut ctx.symbol_bin());
    Some(file)
}

// Reads a file inside an archive.
fn read_archive_member<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    archive_name: &str,
) -> Option<Loaded<E>> {
    match get_file_type(ctx, mf) {
        FileType::ElfObj => {
            let file = new_object_file(ctx, rctx, mf, archive_name);
            Some(Loaded::Obj(rctx.pos.clone(), Box::new(file)))
        }
        FileType::GccLtoObj | FileType::LlvmBitcode => {
            defer_lto_object(ctx, rctx, mf, archive_name);
            None
        }
        FileType::ElfDso => {
            warn!(
                "{archive_name}({}): shared object file in an archive is ignored",
                mf.name
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
            let file = new_object_file(ctx, rctx, mf, "");
            push_loaded(ctx, Loaded::Obj(rctx.pos.clone(), Box::new(file)));
        }
        FileType::ElfDso => {
            let file = new_shared_file(ctx, rctx, mf);
            push_loaded(ctx, Loaded::Dso(rctx.pos.clone(), Box::new(file)));
        }
        FileType::Ar | FileType::ThinAr => {
            for child in archive_file::read_archive_members(mf) {
                let child_rctx = rctx.next_child();
                if let Some(loaded) = read_archive_member(ctx, &child_rctx, child, &mf.name) {
                    push_loaded(ctx, loaded);
                }
            }
        }
        FileType::Text => Script::new(ctx, rctx, mf).parse_linker_script(),
        FileType::GccLtoObj | FileType::LlvmBitcode => {
            defer_lto_object(ctx, rctx, mf, "");
        }
        _ => fatal!("{}: unknown file type", mf.name),
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
pub fn detect_machine_type<E: Arch>(ctx: &mut Context<E>, jobs: &[ReaderJob]) -> String {
    for job in jobs {
        if job.is_lib {
            continue;
        }
        if let Some(mf) = open_file(&ctx.args.chroot, &job.name) {
            if get_file_type(ctx, mf) != FileType::Text {
                if let Some(target) = filetype::get_machine_type(&ctx.args.plugin, mf, || None) {
                    return target.to_string();
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
                    return target.to_string();
                }
            }
        }
    }
    fatal!("-m option is missing");
}

fn open_library<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    path: &str,
) -> Option<&'static MappedFile> {
    let mf = open_file(&ctx.args.chroot, path)?;
    if let Some(target) = get_machine_type(ctx, rctx, mf) {
        if target != E::NAME {
            warn!(
                "{path}: skipping incompatible file: {target} (e_machine {})",
                E::E_MACHINE
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
    name: &str,
) -> &'static MappedFile {
    if let Some(name) = name.strip_prefix(':') {
        for dir in &ctx.args.library_paths {
            if let Some(mf) = open_library(ctx, rctx, &format!("{dir}/{name}")) {
                return mf;
            }
        }
        fatal!("library not found: :{name}");
    }

    for dir in &ctx.args.library_paths {
        let stem = format!("{dir}/lib{name}");
        if !rctx.is_static {
            if let Some(mf) = open_library(ctx, rctx, &format!("{stem}.so")) {
                return mf;
            }
        }
        if let Some(mf) = open_library(ctx, rctx, &format!("{stem}.a")) {
            return mf;
        }
    }
    fatal!("library not found: {name}");
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
    let scripts = WorkerBins::new();
    let loaded = WorkerBins::new();

    let ctx_ref: &Context<E> = ctx;
    rayon::scope(|scope| {
        jobs.into_par_iter().for_each(|job| {
            let mut rctx = job.rctx.clone();

            // Everything else is a command line argument that we need to open.
            let mf = if job.is_lib {
                let mf = find_library(ctx_ref, &rctx, &job.name);
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

            // An archive member is enqueued by the job that read its archive
            // file. A thin archive's members are opened here rather than by
            // that job, so that the files of a large archive are opened in
            // parallel.
            match get_file_type(ctx_ref, mf) {
                FileType::Ar => {
                    for child in archive_file::read_fat_archive_members(mf) {
                        let child_rctx = rctx.next_child();
                        let archive_name = mf.name.clone();
                        let loaded = &loaded;
                        scope.spawn(move |_| {
                            if let Some(file) =
                                read_archive_member(ctx_ref, &child_rctx, child, &archive_name)
                            {
                                loaded.push(file);
                            }
                        });
                    }
                }
                FileType::ThinAr => {
                    for path in archive_file::get_thin_archive_member_paths(mf) {
                        let child_rctx = rctx.next_child();
                        let archive_name = mf.name.clone();
                        let loaded = &loaded;
                        scope.spawn(move |_| {
                            let child = must_open_file(&ctx_ref.args.chroot, &path);
                            let child = crate::util::leak(MappedFile {
                                name: child.name.clone(),
                                data: child.data,
                                given_fullpath: true,
                                parent: None,
                                thin_parent: Some(mf),
                                is_dependency: std::sync::atomic::AtomicBool::new(true),
                            });
                            if let Some(file) =
                                read_archive_member(ctx_ref, &child_rctx, child, &archive_name)
                            {
                                loaded.push(file);
                            }
                        });
                    }
                }
                FileType::Text => {
                    let mut job = job;
                    job.mf = Some(mf);
                    scripts.push(job);
                }
                FileType::ElfObj => {
                    let file = new_object_file(ctx_ref, &rctx, mf, "");
                    loaded.push(Loaded::Obj(rctx.pos, Box::new(file)));
                }
                FileType::ElfDso => {
                    let file = new_shared_file(ctx_ref, &rctx, mf);
                    loaded.push(Loaded::Dso(rctx.pos, Box::new(file)));
                }
                FileType::GccLtoObj | FileType::LlvmBitcode => {
                    defer_lto_object(ctx_ref, &rctx, mf, "");
                }
                _ => fatal!("{}: unknown file type", mf.name),
            }
        });
    });

    for l in loaded.into_vec() {
        push_loaded(ctx, l);
    }

    // Parse linker scripts and read the files they name.
    let mut scripts = scripts.into_vec();
    scripts.sort_by(|a, b| a.rctx.pos.cmp(&b.rctx.pos));
    for job in scripts {
        let mut rctx = job.rctx.clone();
        Script::new(ctx, &mut rctx, job.mf.unwrap()).parse_linker_script();
    }

    // Hand IR files to the LTO plugin, in the command line order. The
    // plugin keeps global state, so claims can't run in parallel anyway,
    // and LLVM's LTO assumes that the module holding the prevailing copy
    // of a COMDAT group arrives before the modules holding the other
    // copies, as it does with a serial linker. Otherwise the losing
    // copies' internal symbols become undefined references in the LTO
    // result.
    let mut lto_jobs = std::mem::take(ctx.lto_jobs.get_mut().unwrap());
    lto_jobs.sort_by(|a, b| a.rctx.pos.cmp(&b.rctx.pos));
    for job in lto_jobs {
        if let Some(file) = new_lto_object(ctx, &job.rctx, job.mf.unwrap(), job.archive_name) {
            push_loaded(ctx, Loaded::Obj(job.rctx.pos, Box::new(file)));
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
