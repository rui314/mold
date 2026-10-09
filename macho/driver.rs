//! The linker driver: runs the passes in order.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::sync::Arc;

use rayon::prelude::*;

use crate::arch::Target;
use crate::bundle_hook;
use crate::chunks::{self, ChunkId};
use crate::cmdline;
use crate::context::Context;
use crate::dead_strip;
use crate::objc;
use crate::output_file;
use crate::passes;
use crate::reader;
use crate::symbol_moves;

/// The fully expanded command line.
pub type Cmdline = Arc<[Cow<'static, OsStr>]>;

/// The exit status of a link, or the name of the target the inputs are
/// actually for.
pub type LinkResult = Result<i32, &'static str>;

/// Runs the linker with the given command line. Returns the exit status.
///
/// `initial_target` is an enabled target used for the initial argument
/// parsing. `link_for_target` links for a named target, or reports the
/// target the inputs are actually for; the executable provides both, as
/// the targets are instantiated in crates of their own.
pub fn main(
    argv: Vec<OsString>,
    initial_target: &str,
    link_for_target: impl Fn(&str, Cmdline) -> LinkResult,
) -> i32 {
    let cmdline: Cmdline = cmdline::expand_response_files(argv).into();

    // Parse with an enabled target's defaults; if the target turns out to
    // be different, start over with the right one.
    let mut target = initial_target;
    loop {
        match link_for_target(target, Arc::clone(&cmdline)) {
            Ok(status) => return status,
            Err(actual) => target = actual,
        }
    }
}

fn target_traits<E: Target>() -> cmdline::TargetTraits {
    cmdline::TargetTraits { name: E::NAME, page_size: E::PAGE_SIZE }
}

/// Links for the target `E`, or reports the target the inputs are
/// actually for.
pub fn link<E: Target>(cmdline: Cmdline) -> LinkResult {
    let args = cmdline::parse_args(&target_traits::<E>(), &cmdline);

    // Redo if the target does not match with our speculation.
    if args.arch != Some(E::NAME) {
        return Err(args.arch.unwrap());
    }

    // Fork so exit latency (unmapping every input) hides behind the
    // parent's return, as in mold; MOLD_NO_FORK=1 keeps one process
    // for debuggers and profilers.
    if std::env::var_os("MOLD_NO_FORK").is_none() {
        crate::subprocess::fork_child();
    }

    let mut ctx: Context<E> = Context::new(args);
    crate::error::set_demangle(ctx.args.demangle);
    cmdline::set_search_paths(&mut ctx);

    let t_all = ctx.timer("all");
    crate::subprocess::install_signal_handler();
    crate::error::install_panic_hook();

    // Runs a pass under a -print_statistics timer.
    macro_rules! timed {
        ($name:literal, $e:expr) => {{
            let t = ctx.timer($name);
            $e;
            drop(t);
        }};
    }

    // Parse input files
    timed!("read_input_files", reader::read_input_files(&mut ctx));

    // Create a dummy file containing linker-synthesized symbols.
    passes::create_internal_file(&mut ctx);
    crate::error::checkpoint();

    // Resolve symbols by choosing the most appropriate file for each
    // symbol, loading the libraries the live objects' auto-link options
    // name as they become live.
    let mut t = ctx.timer("resolve_symbols");
    passes::resolve_symbols(&mut ctx);
    let mut checked = passes::CheckedInputs::default();
    passes::check_input_versions(&ctx, &mut checked);
    passes::check_bitcode_duplicates(&ctx);
    // A link that has failed so far compiles no bitcode: say, one that
    // has a bitcode file built for another platform.
    crate::error::checkpoint();

    // A -r link of bitcode alone writes the bitcode merged.
    if ctx.args.relocatable && passes::links_only_bitcode(&ctx) {
        t.stop();
        passes::print_why_load(&ctx);
        crate::lto::write_merged_bitcode(&ctx);
        crate::error::checkpoint();
        crate::mapfile::write_dependency_info(&ctx);
        crate::error::checkpoint();
        crate::subprocess::notify_parent();
        drop(t_all);
        return Ok(0);
    }

    // If there's a bitcode file, do link-time optimization.
    if passes::has_lto_obj(&ctx) {
        passes::do_lto(&mut ctx);
        passes::check_input_versions(&ctx, &mut checked);
    }
    passes::print_why_load(&ctx);
    passes::warn_newer_dylibs(&ctx);
    crate::error::checkpoint();
    t.stop();
    bundle_hook::create_class_table(&mut ctx);
    passes::check_common_conflicts(&ctx);

    // Now that we know which object files are to be included to the
    // final output, we can remove unnecessary files.
    timed!("remove_unreachable_files", passes::remove_unreachable_files(&mut ctx));
    passes::remove_swift_reflection_metadata(&mut ctx);

    // Handle -r. Since the linker's behavior is quite different from the
    // normal one when the option is given, the logic is implemented to a
    // separate file.
    if ctx.args.relocatable {
        passes::print_implicit_trace(&ctx);
        passes::check_duplicate_symbols(&ctx);
        passes::check_removed_swift_metadata_refs(&ctx);
        crate::error::checkpoint();
        passes::check_poisoned_symbols(&ctx);
        crate::error::checkpoint();
        passes::hide_all_exports(&mut ctx);
        passes::handle_exported_symbols_list(&mut ctx);
        passes::handle_unexported_symbols_list(&mut ctx);
        passes::check_weak_exports(&ctx);
        // Literals are left for the final link to merge: every input
        // label and relocation then carries through as it is.
        // ld64 -r keeps one copy of each weak definition (the marker
        // stays on it for the final link to auto-hide); Swift's
        // per-object conformance and metadata records doubled
        // NetNewsWire's RSCore prelink's __DATA,__const otherwise.
        timed!("coalesce_weak_defs", passes::coalesce_weak_defs(&mut ctx));
        let moves = symbol_moves::find_moves(&ctx);
        timed!("create_output_sections", passes::create_output_sections(&mut ctx, &moves));
        passes::set_section_alignments(&mut ctx);
        passes::sort_section_members(&mut ctx);
        timed!("compute_section_sizes", passes::compute_section_sizes(&mut ctx));
        timed!("create_synthetic_sections", passes::create_synthetic_sections(&mut ctx, &moves));
        passes::rename_synthetic_sections(&mut ctx);
        passes::add_boundary_sections(&mut ctx);
        timed!("sort_output_sections", passes::sort_output_sections(&mut ctx));
        passes::create_segments(&mut ctx);
        passes::add_boundary_segments(&mut ctx);
        passes::add_stack_segment(&mut ctx);
        passes::finish_section_alignments(&mut ctx);
        chunks::indirect_symtab::assign_indices(&mut ctx);
        passes::check_segment_order(&ctx);
        passes::check_section_order(&ctx);
        passes::check_interposing(&ctx);
        passes::check_header_segment(&ctx);
        timed!("relocatable", ctx.output_size = crate::relocatable::combine_objects(&mut ctx));
        crate::error::checkpoint();
        crate::subprocess::notify_parent();
        drop(t_all);
        passes::show_stats(&ctx);
        return Ok(0);
    }

    // Merge the initializers, literals and Objective-C references, and
    // add the symbols and sections the linker synthesizes.
    passes::check_initializers(&ctx);
    passes::convert_init_offsets(&mut ctx);
    timed!("merge_literals", {
        passes::merge_literals(&mut ctx);
        objc::coalesce_objc_refs(&mut ctx);
    });
    timed!("add_synthetic_symbols", passes::add_synthetic_symbols(&mut ctx));
    timed!("convert_common_symbols", passes::convert_common_symbols(&mut ctx));
    timed!("create_objc_msgsend_stubs", objc::create_objc_msgsend_stubs(&mut ctx));

    // Decide what the image exports.
    timed!("auto_hide_weak_defs", passes::auto_hide_weak_defs(&mut ctx));
    timed!("hide_all_exports", passes::hide_all_exports(&mut ctx));
    timed!("handle_exported_symbols_list", passes::handle_exported_symbols_list(&mut ctx));
    timed!("handle_unexported_symbols_list", passes::handle_unexported_symbols_list(&mut ctx));
    passes::force_symbol_weakness(&mut ctx);
    timed!("create_symbol_reexports", passes::create_symbol_reexports(&mut ctx));
    timed!("coalesce_weak_defs", passes::coalesce_weak_defs(&mut ctx));
    passes::print_dependencies(&ctx);
    passes::print_trace(&ctx);
    passes::print_implicit_trace(&ctx);

    // Garbage-collect unreachable subsections.
    if dead_strip::strips_dead_code(&ctx) {
        timed!("dead_strip", dead_strip::strip_dead_code(&mut ctx));
    }
    timed!("create_dof_sections", crate::dtrace::create_dof_sections(&mut ctx));

    // Report the errors resolution and dead stripping leave.
    passes::check_removed_swift_metadata_refs(&ctx);
    passes::claim_unresolved_symbols(&mut ctx);
    timed!("report_undef_errors", passes::report_undef_errors(&mut ctx));
    crate::error::checkpoint();
    passes::check_weak_imports(&ctx);
    crate::error::checkpoint();
    passes::check_duplicate_symbols(&ctx);
    crate::error::checkpoint();
    passes::check_poisoned_symbols(&ctx);
    crate::error::checkpoint();
    passes::warn_unused_dylibs(&ctx);
    passes::warn_redundant_reexports(&ctx);
    passes::check_weak_exports(&ctx);
    crate::error::checkpoint();

    // Fold identical functions.
    if ctx.args.deduplicate {
        timed!("compute_address_significance", passes::compute_address_significance(&mut ctx));
        crate::icf::icf_sections(&mut ctx);
    }

    // Scan relocations to find symbols that need stubs, GOT slots and
    // the like. The passes before the scan flag what else needs them,
    // and rewrite the class loads it is to see as GOT loads.
    passes::add_entry_stub(&mut ctx);
    objc::scan_objc_stubs(&mut ctx);
    objc::fold_objc_classrefs(&mut ctx);
    timed!("scan_relocations", passes::scan_relocations(&mut ctx));
    objc::convert_objc_method_lists(&mut ctx);
    objc::merge_objc_categories(&mut ctx);
    // Synthetic stubs and unwind data can introduce library references
    // (notably dyld_stub_binder). Establish them before pruning dylibs.
    chunks::stub_helper::resolve_stub_binder(&mut ctx);
    crate::lazy_load::bind_dyld_lazy_load(&mut ctx);
    timed!("dead_strip_dylibs", passes::dead_strip_dylibs(&mut ctx));
    passes::check_shared_cache_deps(&ctx);
    passes::check_libsystem_linked(&ctx);
    passes::bind_private_reexports_to_image(&mut ctx);
    passes::check_weak_assertions(&ctx);
    crate::error::checkpoint();
    crate::lazy_load::create_lazy_loads(&mut ctx);
    crate::delay_init::create_delay_init(&mut ctx);
    passes::finish_stubs(&mut ctx);

    // Bin input sections into output sections. -move_to_rw_segment and
    // the like take the subsections they name to other segments.
    let moves = symbol_moves::find_moves(&ctx);
    timed!("create_output_sections", passes::create_output_sections(&mut ctx, &moves));
    passes::set_section_alignments(&mut ctx);

    // Order each output section's members: -order_file's first, cold
    // code last.
    passes::sort_section_members(&mut ctx);

    // Compute sizes of output sections while assigning offsets within
    // an output section to input sections.
    timed!("compute_section_sizes", passes::compute_section_sizes(&mut ctx));

    // Create linker-synthesized sections such as __stubs or __got, and
    // give them their final names.
    timed!("create_synthetic_sections", passes::create_synthetic_sections(&mut ctx, &moves));
    passes::rename_synthetic_sections(&mut ctx);

    // Create the sections section$start$ and section$end$ symbols name.
    passes::add_boundary_sections(&mut ctx);
    crate::mapfile::trace_symbol_layout(&ctx);

    // Sort the sections into file order, and group them into segments.
    timed!("sort_output_sections", passes::sort_output_sections(&mut ctx));
    passes::create_segments(&mut ctx);
    passes::add_boundary_segments(&mut ctx);
    passes::add_stack_segment(&mut ctx);
    passes::finish_section_alignments(&mut ctx);
    chunks::indirect_symtab::assign_indices(&mut ctx);
    passes::check_segment_order(&ctx);
    passes::check_section_order(&ctx);
    passes::check_interposing(&ctx);
    passes::check_header_segment(&ctx);

    // Handle -no_zero_fill_sections.
    if ctx.args.no_zero_fill_sections {
        passes::fill_zero_fill_sections(&mut ctx);
    }

    // The output symbol table builds inside set_osec_offsets, as part
    // of the parallel __LINKEDIT task group.
    timed!("set_osec_offsets", passes::set_osec_offsets(&mut ctx));
    passes::fix_synthetic_symbols(&mut ctx);
    passes::check_entry_point(&ctx);
    crate::error::checkpoint();
    crate::mapfile::write_dependency_info(&ctx);
    crate::mapfile::print_map(&ctx);
    crate::mapfile::write_sdk_imports(&ctx);

    // Write the output. The file is created up front, executable, and
    // each range of the buffer is queued to `out` the moment it is
    // final, so the file is written from background threads while the
    // rest is produced: everything between the header and the symbol
    // table after the copy and its fix-ups, the symbol and string
    // tables after copy_symtab, the header after the UUID, the
    // signature last. finish() waits for the last one.
    let t_copy = ctx.timer("copy");
    let mut buf = vec![0; output_file::buffer_len(&ctx.args.output, ctx.output_size)];
    let out = output_file::OutputFile::open(&ctx.args.output, 0o777, buf.as_ptr(), buf.len());

    // Copy input sections to the output file and apply relocations.
    copy_chunks(&ctx, &mut buf);

    // Relocations that failed to apply fail the link before the fixups
    // are written.
    passes::report_text_relocs(&ctx);
    crate::error::checkpoint();

    // The fixups, the symbol table (which also fills the string table),
    // the mach header, the UUID and the code signature follow serially,
    // in that order, since each depends on the bytes before it.
    if ctx.use_chained_fixups() {
        timed!("write_fixup_chains", chunks::chained_fixups::write_fixup_chains(&ctx, &mut buf));
    }
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        chunks::local_relocs::write(&ctx, &mut buf);
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) {
        chunks::extern_relocs::write(&ctx, &mut buf);
    }
    timed!("apply_optimization_hints", E::apply_optimization_hints(&ctx, &mut buf));

    let hdr_end = ctx.mach_header.hdr.size as usize;
    let sig_start = if ctx.chunks.contains(&ChunkId::CodeSignature) {
        ctx.code_signature.hdr.fileoff as usize
    } else {
        buf.len()
    };
    let symtab_start = (ctx.symtab.hdr.fileoff as usize).min(ctx.strtab.hdr.fileoff as usize);

    // Nothing below writes between the header and the symbol table.
    out.queue(hdr_end, symtab_start - hdr_end);
    timed!("copy_symtab", chunks::symtab::copy_symtab(&ctx, &mut buf));
    out.queue(symtab_start, sig_start - symtab_start);
    chunks::write_mach_header(&ctx, &mut buf);

    let hashes = passes::compute_uuid(&ctx, &mut buf, sig_start);
    out.queue(0, hdr_end);

    if ctx.args.adhoc_codesign {
        timed!("write_code_signature", chunks::code_signature::write(&ctx, &mut buf, &hashes));
    }
    out.queue(sig_start, buf.len() - sig_start);
    crate::error::checkpoint();
    // The traces name the output by its UUID; one that can't be written
    // fails the link, which leaves no output.
    crate::mapfile::write_trace_files(&ctx);
    crate::error::checkpoint();
    timed!("close_file", out.close());
    drop(t_copy);
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    crate::subprocess::notify_parent();
    drop(t_all);
    passes::show_stats(&ctx);
    Ok(0)
}

/// Copies all chunks to the output buffer and applies relocations, in
/// parallel: the buffer is carved into disjoint per-chunk slices, and
/// every chunk writes only within its own.
pub(crate) fn copy_chunks<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
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
    let slices = output_file::split_ranges(buf, &ranges);

    let t = ctx.timer("copy_chunks");
    jobs.par_iter().zip(slices).for_each(|(&(id, _), slice)| chunks::copy_buf(ctx, id, slice));
    drop(t);
}
