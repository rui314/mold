//! The linker driver: runs the passes in order.

use std::fmt;
use std::sync::mpsc;

use rayon::prelude::*;

use crate::arch::{self, Arch};
use crate::chunks::{self, ChunkId};
use crate::cmdline::{self, Args, TargetTraits};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::output_file::{split_ranges, OutputFile, Range};
use crate::{error, fatal, passes};

/// Runs the linker with the given command line. Returns the exit status.
///
/// `link_for_target` links for a named target, or reports the target the
/// inputs are actually for; the executable provides it, as the targets
/// are instantiated in crates of their own.
pub fn main(
    argv: Vec<String>,
    link_for_target: impl Fn(&str, &[String]) -> Result<i32, String>,
) -> i32 {
    // Process -run option first. process_run_subcommand() does not return.
    if argv.get(1).is_some_and(|a| a == "-run" || a == "--run") {
        crate::subprocess::process_run_subcommand(&argv);
    }

    // parse_nonpositional_args() may chdir(2) for -C. If we end up
    // restarting in redo_main(), we need to re-enter from the original
    // directory so relative paths (e.g. response files) still resolve.
    let orig_cwd = std::env::current_dir().ok();

    // Parse non-positional command line options
    let cmdline = cmdline::expand_response_files(&argv);

    // Parse with x86-64 defaults; if the target turns out to be different,
    // start over with the right one.
    let mut target = "x86_64".to_string();
    loop {
        if let Some(cwd) = &orig_cwd {
            let _ = std::env::set_current_dir(cwd);
        }
        match link_for_target(&target, &cmdline) {
            Ok(status) => return status,
            Err(actual) => target = actual,
        }
    }
}

fn target_traits<E: Arch>() -> TargetTraits {
    TargetTraits {
        name: E::NAME,
        is_rela: E::IS_RELA,
        is_sparc: E::IS_SPARC,
        is_riscv: E::IS_RISCV,
        is_sh4: E::FAMILY == arch::Family::Sh4,
        is_x86_64: E::FAMILY == arch::Family::X86_64,
        is_arm64: E::FAMILY == arch::Family::Arm64,
        page_size: E::PAGE_SIZE,
    }
}

fn thread_count(args: &Args) -> usize {
    // mold doesn't scale well with too many threads, so limit it to 32.
    args.thread_count.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(32)
    })
}

fn wait_for_background<T>(receiver: mpsc::Receiver<T>, name: &str) -> T {
    loop {
        match receiver.try_recv() {
            Ok(value) => return value,
            Err(mpsc::TryRecvError::Disconnected) => panic!("{name} task failed"),
            Err(mpsc::TryRecvError::Empty) => {
                if !matches!(rayon::yield_now(), Some(rayon::Yield::Executed)) {
                    std::thread::yield_now();
                }
            }
        }
    }
}

/// Links for the target `E`, or reports the target the inputs are actually
/// for.
pub fn link<E: Arch>(cmdline: &[String]) -> Result<i32, String> {
    let parsed = cmdline::parse_args(&target_traits::<E>(), cmdline);
    let cmdline::ParsedArgs { args, jobs, .. } = parsed;
    let mut ctx = Context::<E>::new(args, cmdline.to_vec());

    // If no -m option is given, deduce it from input files.
    if ctx.args.emulation.is_empty() {
        ctx.args.emulation = crate::reader::detect_machine_type(&mut ctx, &jobs);
    }

    // Redo if -m does not match with our speculation.
    if ctx.args.emulation != E::NAME {
        return Err(ctx.args.emulation.clone());
    }

    let t_all = ctx.timer("all");
    crate::subprocess::install_signal_handler();

    // Fork a subprocess unless --no-fork is given.
    if ctx.args.fork {
        crate::subprocess::fork_child();
    }

    crate::jobs::acquire_global_lock();

    let threads = thread_count(&ctx.args);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .use_current_thread()
        .build_global()
        .expect("failed to build linker thread pool");

    // Handle --wrap options if any.
    for name in ctx.args.wrap.clone() {
        let id = ctx.get_symbol(name.as_bytes());
        ctx.symbols[id].set_wrapped(true);
    }
    // Handle --retain-symbols-file options if any.
    if let Some(names) = ctx.args.retain_symbols_file.clone() {
        for name in names {
            let id = ctx.get_symbol(name.as_bytes());
            ctx.symbols[id].set_write_to_symtab();
        }
    }
    for name in ctx.args.trace_symbol.clone() {
        let id = ctx.get_symbol(name.as_bytes());
        ctx.symbols[id].set_traced(true);
    }

    // Version scripts and dynamic lists given on the command line.
    for path in ctx.args.version_scripts.clone() {
        let chroot = ctx.args.chroot.clone();
        let mf = crate::mapped_file::open_file(&chroot, &path).or_else(|| {
            ctx.args
                .library_paths
                .iter()
                .find_map(|dir| crate::mapped_file::open_file(&chroot, &format!("{dir}/{path}")))
        });
        let Some(mf) = mf else {
            fatal!("--version-script: file not found: {path}");
        };
        let mut rctx = cmdline::ReaderContext::default();
        crate::linker_script::Script::new(&mut ctx, &mut rctx, mf).parse_version_script();
    }
    for source in ctx.args.dynamic_list.clone() {
        match source {
            cmdline::DynamicListSource::File(path) => {
                let patterns = crate::linker_script::parse_dynamic_list(&mut ctx, &path);
                ctx.dynamic_list_patterns.extend(patterns);
            }
            cmdline::DynamicListSource::Pattern(pattern) => {
                ctx.dynamic_list_patterns
                    .push(crate::linker_script::DynamicPattern {
                        pattern: crate::util::leak_bytes(pattern.into_bytes()),
                        source: "<command line>".to_string(),
                        is_cpp: false,
                    });
            }
        }
    }

    // Parse input files
    crate::reader::read_input_files(&mut ctx, jobs);

    // Uniquify shared object files by soname
    {
        let mut dsos = std::mem::take(&mut ctx.dsos);
        let mut keep = vec![false; dsos.len()];
        for file in &dsos {
            keep[file.id().index()] = ctx.dso_sonames.insert(file.soname.clone());
        }
        dsos.retain(|file| keep[file.id().index()]);
        for slot in &mut ctx.file_by_priority {
            if let Some(FileId::Dso(d)) = *slot {
                if !keep[d.index()] {
                    *slot = None;
                }
            }
        }

        // Bins keep pointers to the files' SymbolId slots until gather.
        // Register only the DSOs retained above, so every slot outlives that
        // gather even when duplicate sonames were present on the command line.
        {
            let mut bin = ctx.symbol_bin();
            for file in &mut dsos {
                file.record_global_symbols(&mut bin);
            }
        }
        ctx.dsos = dsos;
    }

    // Handle -repro
    if ctx.args.repro {
        passes::write_repro_file(&ctx);
    }

    let mut t_before_copy = ctx.timer("before_copy");

    // Apply -exclude-libs
    passes::apply_exclude_libs(&mut ctx);

    // Create a dummy file containing linker-synthesized symbols.
    if !ctx.args.relocatable {
        let t = ctx.timer("create_internal_file");
        passes::create_internal_file(&mut ctx);
        drop(t);
    }

    // Resolve symbols by choosing the most appropriate file for each
    // symbol. This pass also removes redundant comdat sections (e.g.
    // duplicate inline functions).
    passes::resolve_symbols(&mut ctx);

    // If there's an object file compiled with -flto, do link-time
    // optimization.
    if passes::has_lto_obj(&ctx) {
        passes::do_lto(&mut ctx);
    }

    // Now that we know which object files are to be included to the
    // final output, we can remove unnecessary files.
    let t = ctx.timer("remove_unreachable_files");
    passes::remove_unreachable_files(&mut ctx);
    drop(t);

    // Building .gdb_index is split into three stages because the required data
    // becomes available at different points in the link. Compilation units and
    // public names depend only on input sections, so read them now in a
    // background task while foreground passes continue.
    let gdb_input_job = if ctx.args.gdb_index && !ctx.args.relocatable {
        let timer = t_before_copy.handle();
        let inputs = crate::gdb_index::prepare_inputs(&mut ctx);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let job = move || {
            let timer = timer.child("read_gdb_index_inputs");
            let data = crate::gdb_index::read_inputs::<E>(timer, inputs);
            let _ = sender.send(data);
        };
        // Rayon has no task priorities, so queue this as an ordinary
        // background task.
        rayon::spawn(job);
        Some(receiver)
    } else {
        None
    };

    // Parse .eh_frame section contents.
    passes::parse_eh_frame_sections(&mut ctx);

    // Parse .sframe section contents.
    passes::parse_sframe_sections(&mut ctx);

    // Split mergeable section contents into section pieces.
    passes::create_merged_sections(&mut ctx);

    // Handle --relocatable. Since the linker's behavior is quite different
    // from the normal one when the option is given, the logic is implemented
    // to a separate file.
    if ctx.args.relocatable {
        crate::relocatable::combine_objects(&mut ctx);
        return Ok(0);
    }

    // Non-allocated strings are independent of symbol and relocation passes.
    // Give the background task owned metadata and publish it before layout.
    let merge_input = chunks::merged::BackgroundMerge::prepare(&ctx);
    let merge_timers = ctx.timers.clone();
    let merge_comment = ctx.comment;
    let merge_cmdline = ctx.cmdline_args.clone();
    let (merge_sender, merge_receiver) = mpsc::sync_channel(1);
    rayon::spawn(move || {
        let result = merge_input.run(chunks::merged::ResolveOptions {
            allocated_only: false,
            gc_sections: false,
            comment: merge_comment,
            cmdline_args: &merge_cmdline,
            timers: &merge_timers,
        });
        let _ = merge_sender.send(result);
    });

    // Create .bss sections for common symbols.
    passes::convert_common_symbols(&mut ctx);

    // Apply version scripts.
    passes::apply_version_script(&mut ctx);

    // Parse symbol version suffixes (e.g. "foo@ver1").
    passes::parse_symbol_version(&mut ctx);

    // Set is_imported and is_exported bits for each symbol.
    passes::compute_import_export(&mut ctx);

    // Make sure that there's no duplicate symbol
    if !ctx.args.allow_multiple_definition {
        passes::check_duplicate_symbols(&ctx);
    }
    // Handle --zero-to-bss, which converts data sections containing only
    // zeros into BSS.
    if ctx.args.zero_to_bss {
        let t = ctx.timer("convert_zero_to_bss");
        passes::convert_zero_to_bss(&mut ctx);
        drop(t);
    }
    // Set "address-taken" bits for input sections.
    if ctx.args.icf {
        let t = ctx.timer("compute_address_significance");
        passes::compute_address_significance(&mut ctx);
        drop(t);
    }
    // Handle PPC64-specific .opd sections.
    E::rewrite_input_sections(&mut ctx);

    // Garbage-collect unreachable sections.
    if ctx.args.gc_sections {
        crate::gc_sections::gc_sections(&mut ctx);
    }
    // Merge identical read-only sections.
    if ctx.args.icf {
        crate::icf::icf_sections(&mut ctx);
    }

    // Create linker-synthesized sections such as .got or .plt.
    let t = ctx.timer("create_synthetic_sections");
    passes::create_synthetic_sections(&mut ctx);
    drop(t);

    // Handle --no-allow-shlib-undefined
    if !ctx.args.allow_shlib_undefined {
        passes::check_shlib_undefined(&mut ctx);
    }

    // Warn if symbols with different types are defined under the same name.
    passes::check_symbol_types(&ctx);

    // Bin input sections into output sections.
    passes::create_output_sections(&mut ctx);

    // Convert an .ARM.exidx to a synthetic section.
    if E::FAMILY == arch::Family::Arm32 {
        chunks::arm_exidx::create(&mut ctx);
    }

    // Handle --section-align options.
    if !ctx.args.section_align.is_empty() {
        passes::apply_section_align(&mut ctx);
    }
    // Add synthetic symbols such as __ehdr_start or __end.
    let t = ctx.timer("add_synthetic_symbols");
    passes::add_synthetic_symbols(&mut ctx);
    drop(t);

    // Beyond this point, no new files will be added to ctx.objs
    // or ctx.dsos.

    // Handle `-z cet-report`.
    if ctx.args.z_cet_report != cmdline::CetReportKind::None {
        passes::check_cet_errors(&ctx);
    }
    // Handle `-z execstack-if-needed`.
    if ctx.args.z_execstack_if_needed && ctx.objs.iter().any(|f| f.needs_executable_stack) {
        ctx.args.z_execstack = true;
    }

    // If we are linking a .so file, remaining undefined symbols does
    // not cause a linker error. Instead, they are treated as if they
    // were imported symbols.
    //
    // If we are linking an executable, weak undefs are converted to
    // weakly imported symbols so that they'll have another chance to be
    // resolved.
    passes::claim_unresolved_symbols(&mut ctx);

    // Beyond this point, no new symbols will be added to the result.

    // Handle --print-dependencies
    if ctx.args.print_dependencies {
        passes::print_dependencies(&ctx);
    }
    // Handle --require-defined
    for name in ctx.args.require_defined.clone() {
        let id = ctx.get_symbol(name.as_bytes());
        if ctx.symbols[id].file().is_none() {
            error!("--require-defined: undefined symbol: {}", ctx.symbols[id]);
        }
    }

    // .init_array and .fini_array contents have to be sorted by
    // a special rule. Sort them.
    passes::sort_init_fini(&mut ctx);

    // Likewise, .ctors and .dtors have to be sorted. They are rare
    // because they are superceded by .init_array/.fini_array, though.
    passes::sort_ctor_dtor(&mut ctx);

    // If .ctors/.dtors are to be placed to .init_array/.fini_array,
    // we need to reverse their contents.
    passes::fixup_ctors_in_init_array(&mut ctx);

    // Handle --shuffle-sections
    if ctx.args.shuffle_sections != cmdline::ShuffleSectionsKind::None {
        passes::shuffle_sections(&mut ctx);
    }
    // Copy string referred by .dynamic to .dynstr.
    let t = ctx.timer("add_dynamic_strings");
    passes::add_dynamic_strings(&mut ctx);
    drop(t);
    E::scan_symbols(&mut ctx);

    // Scan relocations to find symbols that need entries in .got, .plt,
    // .got.plt, .dynsym, .dynstr, etc.
    passes::scan_relocations(&mut ctx);

    // Now that we know all exported symbols, make sure that no versioned
    // name is defined twice.
    passes::check_symbol_version_conflicts(&ctx);

    // Compute the is_weak bit for each imported symbol.
    passes::compute_imported_symbol_weakness(&mut ctx);

    wait_for_background(merge_receiver, "non-allocated string merging").finish(&mut ctx);

    // Sort sections by section attributes so that we'll have to
    // create as few segments as possible.
    let t = ctx.timer("sort_output_sections");
    passes::sort_output_sections(&mut ctx);
    drop(t);

    // Handle --separate-debug-file.
    if ctx.gnu_debuglink.is_some() {
        passes::separate_debug_sections(&mut ctx);
    }
    // Compute sizes of output sections while assigning offsets
    // within an output section to input sections.
    passes::compute_section_sizes(&mut ctx);

    // RELR is encoded independently for each output chunk using offsets
    // relative to that chunk.
    if ctx.args.pack_dyn_relocs_relr {
        chunks::reldyn::construct_relr(&mut ctx);
    }
    // Reserve a space for dynamic symbol strings in .dynstr and sort
    // .dynsym contents if necessary. Beyond this point, no symbol will
    // be added to .dynsym.
    passes::sort_dynsyms(&mut ctx);
    // sort_debug_info_sections may uncompress the same .debug_info sections.
    if let Some(job) = gdb_input_job {
        ctx.gdb_index_data = Some(wait_for_background(job, ".gdb_index input"));
    }
    // Sort .debug_info contents so that DWARF32 debug info precedes that of
    // DWARF64. This is to mitigate the possibility of a relocation overflow.
    passes::sort_debug_info_sections(&mut ctx);

    // Type vectors identify compilation units by their order in the output
    // .debug_info section. That order is now fixed, so build the table while the
    // remaining layout passes continue. This stage stops scaling after about 12
    // workers, so limit its concurrency to avoid competing with foreground work.
    let gdb_table_workers = (threads * 3 / 8).clamp(1, 12);
    let mut gdb_table_job = if ctx.gdb_index.is_some() && ctx.gnu_debuglink.is_none() {
        let timer = t_all.handle();
        let mut data = ctx
            .gdb_index_data
            .take()
            .expect("missing .gdb_index input data");
        crate::gdb_index::prepare_tables(&ctx, &mut data);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let job = move || {
            let timer = timer.child("build_gdb_index_tables");
            let data = crate::gdb_index::build_tables(timer, data, gdb_table_workers);
            let _ = sender.send(data);
        };
        // Rayon has no task priorities, so queue this as an ordinary
        // background task.
        rayon::spawn(job);
        Some(receiver)
    } else {
        None
    };

    // Print reports about undefined symbols, if needed.
    if ctx.args.unresolved_symbols == cmdline::UnresolvedKind::Error {
        passes::report_undef_errors(&ctx);
    }

    // Fill .gnu.version_d section contents.
    if ctx.verdef.is_some() {
        chunks::verdef::construct(&mut ctx);
    }
    // Fill .gnu.version_r section contents.
    chunks::verneed::construct(&mut ctx);

    // .eh_frame is a special section from the linker's point of view,
    // as its contents are parsed and reconstructed by the linker,
    // unlike other sections that are regarded as opaque bytes.
    // Here, we construct output .eh_frame contents.
    chunks::eh_frame::construct(&mut ctx);

    // .sframe is likewise parsed and reconstructed by the linker. Build
    // the merged, PC-sorted output .sframe.
    chunks::sframe::construct(&mut ctx);

    // If --emit-relocs is given, we'll copy relocation sections from input
    // files to an output file.
    if ctx.args.emit_relocs {
        passes::create_reloc_sections(&mut ctx);
    }
    // Compute .symtab and .strtab sizes for each file.
    if !ctx.args.strip_all {
        passes::create_output_symtab(&mut ctx);
    }
    // Compute the section header values for all sections.
    let t = ctx.timer("compute_section_headers");
    passes::compute_section_headers(&mut ctx);
    drop(t);

    // Assign offsets to output sections
    let mut filesize = passes::set_osec_offsets(&mut ctx);

    // On RISC-V, branches are encode using multiple instructions so
    // that they can jump to anywhere in ±2 GiB by default. They may
    // be replaced with shorter instruction sequences if destinations
    // are close enough. Do this optimization.
    if E::IS_RISCV || E::IS_LOONGARCH {
        crate::shrink_sections::shrink_sections(&mut ctx);
        filesize = passes::set_osec_offsets(&mut ctx);
    }

    // We've created range extension thunks with a pessimistive assumption
    // that all out-of-section references are out of range. Now that we know
    // the addresses of all sections,, we can eliminate excessive thunks.
    if E::NEEDS_THUNK {
        crate::thunks::remove_redundant_thunks(&mut ctx);
        filesize = passes::set_osec_offsets(&mut ctx);
    }
    if ctx.arm_exidx.is_some() {
        chunks::arm_exidx::remove_duplicate_entries(&mut ctx);
        filesize = passes::set_osec_offsets(&mut ctx);
    }

    // At this point, memory layout is fixed.

    // Set actual addresses to linker-synthesized symbols.
    let t = ctx.timer("fix_synthetic_symbols");
    passes::fix_synthetic_symbols(&mut ctx);
    drop(t);
    chunks::sframe::sort(&mut ctx);

    // Beyond this, you can assume that symbol addresses including their
    // GOT or PLT addresses have a correct final value.

    // If --compress-debug-sections is given, compress .debug_* sections
    // using zlib or zstd.
    if ctx.args.compress_debug_sections != ELFCOMPRESS_NONE {
        passes::compress_debug_sections(&mut ctx);
        filesize = passes::set_osec_offsets(&mut ctx);
    }
    // Gather thunk symbols and attach them to themselves.
    if E::NEEDS_THUNK {
        crate::thunks::gather_thunk_addresses(&mut ctx);
    }
    // Re-finalize layout. fix_synthetic_symbols above may have changed
    // addends for dynamic relocations referencing synthetic symbols, which
    // can shift the encoded size of .rela.dyn under --pack-dyn-relocs=android
    // because Android's packed format encodes addends in variable-length
    // SLEB128. Other modes, including ordinary RELR, encode nothing whose
    // size depends on addends, so they do not need this pass.
    if ctx.args.pack_dyn_relocs_android {
        filesize = passes::set_osec_offsets(&mut ctx);
    }

    // At this point, both memory and file layouts are fixed.

    let t = ctx.timer("update_reldyn");
    chunks::reldyn::update_shdr(&mut ctx);
    drop(t);
    ctx.filesize = filesize;
    t_before_copy.stop();

    // Create an output file
    // Output buffer
    let t_open = ctx.timer("open_file");
    let mut output = OutputFile::open(
        &ctx.args.output,
        filesize,
        0o777,
        ctx.args.overwrite_output_file,
    );
    drop(t_open);
    {
        let mut t_copy = ctx.timer("copy");
        let buf = output.buf();

        // Copy input sections to the output file and apply relocations.
        copy_chunks(&ctx, buf);

        E::finish_output(&ctx, buf);

        if ctx.args.z_rewrite_endbr {
            passes::rewrite_endbr(&ctx, buf);
        }

        // Dynamic linker works better with sorted .rela.dyn section,
        // so we sort them.
        let reldyn = ctx.reldyn.hdr.shdr;
        if ctx.chunks.contains(&ChunkId::RelDyn) && reldyn.sh_size.get() != 0 {
            let start = reldyn.sh_offset.get() as usize;
            let end = (reldyn.sh_offset.get() + reldyn.sh_size.get()) as usize;
            chunks::reldyn::sort(&ctx, &mut buf[start..end]);
        }

        // The final stage reads address ranges, which requires relocated debug
        // sections. We have applied the relocations now, so finish the index.
        if ctx.gdb_index.is_some() && ctx.gnu_debuglink.is_none() {
            if let Some(job) = gdb_table_job.take() {
                ctx.gdb_index_data = Some(wait_for_background(job, ".gdb_index table"));
            }
            crate::gdb_index::write(&mut ctx, &mut output);
        }

        // .note.gnu.build-id section contains a cryptographic hash of the
        // entire output file. Now that we wrote everything except build-id,
        // we can compute it.
        if ctx.buildid.is_some() {
            let is_mmapped = output.is_mmapped();
            passes::write_build_id(&mut ctx, output.buf(), is_mmapped);
        }
        if ctx.gnu_debuglink.is_some() {
            passes::write_gnu_debuglink(&mut ctx, output.buf());
        }
        t_copy.stop();
    }
    error::checkpoint();

    // Close the output file. This is the end of the linker's main job.
    let t_close = ctx.timer("close_file");
    output.close();
    drop(t_close);

    // Handle --dependency-file
    if !ctx.args.dependency_file.is_empty() {
        passes::write_dependency_file(&ctx);
    }
    if !ctx.args.plugin.is_empty() {
        crate::lto::cleanup();
    }
    drop(t_all);

    if ctx.args.print_map {
        crate::mapfile::print_map(&ctx);
    }
    if ctx.gnu_debuglink.is_some() {
        passes::write_separate_debug_file(&mut ctx);
    }
    // Show stats numbers
    if ctx.args.stats {
        passes::show_stats(&ctx);
    }
    if ctx.args.perf {
        ctx.timers.print();
    }

    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    crate::subprocess::notify_parent();
    crate::jobs::release_global_lock();

    // Dropping page table entries here in parallel makes process exit
    // faster, as the kernel otherwise reclaims them in a single thread
    // on exit. File contents stay in the page cache.
    crate::mapped_file::drop_mappings();

    if ctx.args.quick_exit {
        error::exit_after_cleanup(0);
    }
    error::checkpoint();
    Ok(0)
}

fn file_range<E: Arch>(ctx: &Context<E>, id: ChunkId) -> Range {
    let hdr = ctx.chunk_header(id);
    let size = if hdr.shdr.sh_type.get() == SHT_NOBITS {
        0
    } else {
        hdr.shdr.sh_size.get()
    };
    Range {
        offset: hdr.shdr.sh_offset.get(),
        size,
    }
}

/// A chunk together with the other chunks whose bytes it writes.
struct Task {
    chunk: ChunkId,
    extra: Vec<ChunkId>,
}

// Copy chunks to an output file
//
// Each chunk owns its own byte range, and a few also write into another
// chunk's range (`.eh_frame` fills the `.eh_frame_hdr` table, `.symtab`
// writes `.strtab`, and relocation sections may patch addends into their
// target sections). The buffer is split into the disjoint ranges each
// task needs so that all tasks can run in parallel safely.
pub fn copy_chunks<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let t = ctx.timer("copy_chunks");

    let mut first: Vec<Task> = Vec::new();
    let mut last: Vec<Task> = Vec::new();
    let is_reloc_sec = |id: ChunkId| {
        let ty = ctx.chunk_header(id).shdr.sh_type.get();
        matches!(
            id,
            ChunkId::Reloc(_) | ChunkId::EhFrameReloc | ChunkId::SFrameReloc
        ) || ty == SHT_REL
            || (E::FAMILY == arch::Family::Sh4 && ty == SHT_RELA)
    };

    for &id in &ctx.chunks {
        match id {
            ChunkId::EhFrameHdr | ChunkId::Strtab | ChunkId::SymtabShndx => {}
            ChunkId::EhFrame => {
                let extra = ctx
                    .eh_frame_hdr
                    .as_ref()
                    .map(|_| ChunkId::EhFrameHdr)
                    .into_iter()
                    .collect();
                first.push(Task { chunk: id, extra });
            }
            ChunkId::Symtab => {
                let mut extra = vec![ChunkId::Strtab];
                if ctx.symtab_shndx.is_some() {
                    extra.push(ChunkId::SymtabShndx);
                }
                first.push(Task { chunk: id, extra });
            }
            ChunkId::Reloc(i) => {
                let osec = ctx.reloc_sections[i as usize].output_section;
                last.push(Task {
                    chunk: id,
                    extra: vec![ChunkId::Output(osec)],
                });
            }
            ChunkId::EhFrameReloc => last.push(Task {
                chunk: id,
                extra: vec![ChunkId::EhFrame],
            }),
            _ if is_reloc_sec(id) => last.push(Task {
                chunk: id,
                extra: Vec::new(),
            }),
            _ => first.push(Task {
                chunk: id,
                extra: Vec::new(),
            }),
        }
    }

    // For --relocatable and --emit-relocs, we want to copy non-relocation
    // sections first, for two reasons. First, REL-type relocation sections (as
    // opposed to RELA-type) store relocation addends to target sections, so the
    // targets must be written first. Second, relaxation may retype an emitted
    // relocation in place while applying relocations (e.g. AArch64 GOT/TLS
    // relaxations), and RelocSection has to observe the updated type, so it
    // must run after the target sections.
    //
    // We also do that for SH4 because despite being RELA, we always need
    // to write addends to relocated places for SH4.
    run_tasks(ctx, buf, &first, &t);
    run_tasks(ctx, buf, &last, &t);

    // Undefined symbols in SHF_ALLOC sections are found by scan_relocations(),
    // but those in non-SHF_ALLOC sections cannot be found until we copy section
    // contents. So we need to call this function again to report possible
    // undefined errors.
    passes::report_undef_errors(ctx);

    // Zero-clear paddings between chunks
    let mut ranges: Vec<Range> = ctx
        .chunks
        .iter()
        .map(|&id| file_range(ctx, id))
        .filter(|r| r.size != 0)
        .collect();
    ranges.sort_by_key(|r| r.offset);
    let mut pos = 0usize;
    for r in ranges {
        let start = r.offset as usize;
        if start > pos {
            buf[pos..start].fill(0);
        }
        pos = pos.max(start + r.size as usize);
    }
    buf[pos..].fill(0);
}

fn run_tasks<E: Arch>(
    ctx: &Context<E>,
    buf: &mut [u8],
    tasks: &[Task],
    timer: &crate::util::perf::Timer,
) {
    let mut ranges: Vec<Range> = Vec::new();
    let mut task_ranges: Vec<Vec<usize>> = Vec::new();
    for task in tasks {
        let mut idx = Vec::new();
        for id in std::iter::once(task.chunk).chain(task.extra.iter().copied()) {
            let r = file_range(ctx, id);
            idx.push(ranges.len());
            ranges.push(r);
        }
        task_ranges.push(idx);
    }

    let mut slices: Vec<Option<&mut [u8]>> =
        split_ranges(buf, &ranges).into_iter().map(Some).collect();
    let mut work: Vec<(&Task, Vec<&mut [u8]>)> = Vec::new();
    for (task, idx) in tasks.iter().zip(task_ranges) {
        let bufs = idx.into_iter().map(|i| slices[i].take().unwrap()).collect();
        work.push((task, bufs));
    }

    work.into_par_iter().for_each(|(task, mut bufs)| {
        let name = ctx.chunk_header(task.chunk).name;
        let _t = timer.child(&if name.is_empty() {
            "(header)".to_string()
        } else {
            name.to_string()
        });
        let mut bufs = bufs.drain(..);
        let own = bufs.next().unwrap();
        match task.chunk {
            ChunkId::EhFrame => chunks::eh_frame::copy_buf(ctx, own, bufs.next()),
            ChunkId::Symtab => {
                let strtab = bufs.next().unwrap();
                chunks::symtab::copy_buf(ctx, own, strtab, bufs.next());
            }
            ChunkId::Reloc(i) => chunks::reloc::copy_buf(ctx, i, own, bufs.next()),
            ChunkId::EhFrameReloc => chunks::eh_frame_reloc::copy_buf(ctx, own, bufs.next()),
            id => chunks::copy_buf(ctx, id, own),
        }
    });

    // .eh_frame_hdr's header, whose table .eh_frame wrote.
    if tasks.iter().any(|t| t.chunk == ChunkId::EhFrame) && ctx.eh_frame_hdr.is_some() {
        let r = file_range(ctx, ChunkId::EhFrameHdr);
        chunks::eh_frame_hdr::write_header(
            ctx,
            &mut buf[r.offset as usize..(r.offset + r.size) as usize],
        );
    }
}

impl<E: Arch> fmt::Debug for Context<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Context<{}>", E::NAME)
    }
}
