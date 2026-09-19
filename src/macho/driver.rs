//! The linker driver: runs the passes in order.

use crate::macho::arch::Arch;
use crate::macho::cmdline;
use crate::macho::context::Context;
use crate::macho::dead_strip;
use crate::macho::output_file;
use crate::macho::passes;

/// Runs the linker with the given command line. Returns the exit status.
///
/// `link_for_target` links for a named target, or reports the target the
/// inputs are actually for; the executable provides it, as the targets
/// are instantiated in crates of their own.
pub fn main(
    argv: Vec<String>,
    link_for_target: impl Fn(&str, &[String]) -> Result<i32, String>,
) -> i32 {
    // Guess the target from -arch, then from the first Mach-O input
    // file, falling back to the host. If the guess turns out wrong,
    // start over with the right one.
    let mut target = argv
        .windows(2)
        .find(|w| w[0] == "-arch")
        .map(|w| w[1].clone())
        .or_else(|| sniff_target(&argv))
        .unwrap_or_else(|| host_target().to_string());

    loop {
        match link_for_target(&target, &argv) {
            Ok(status) => return status,
            Err(actual) => target = actual,
        }
    }
}

/// Reads the CPU type of the first Mach-O file named on the command
/// line, if any.
fn sniff_target(argv: &[String]) -> Option<String> {
    use crate::macho::format::*;
    for arg in &argv[1..] {
        if arg.starts_with('-') {
            continue;
        }
        let Ok(data) = std::fs::read(arg) else { continue };
        if data.len() < 8 {
            continue;
        }
        let magic = u32::from_le_bytes(data[..4].try_into().unwrap());
        if magic == MH_MAGIC_64 {
            let cputype = u32::from_le_bytes(data[4..8].try_into().unwrap());
            if let Some(name) = crate::macho::arch::cputype_name(cputype) {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn host_target() -> &'static str {
    if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
}

/// Links for the target `E`, or reports the target the inputs are
/// actually for.
pub fn link<E: Arch>(cmdline: &[String]) -> Result<i32, String> {
    let cmdline = cmdline::expand_response_files(cmdline);
    let args = cmdline::parse_args(&cmdline);

    if let Some(arch) = &args.arch {
        if arch != E::NAME {
            return Err(arch.clone());
        }
    }

    // Fork so exit latency (unmapping every input) hides behind the
    // parent's return, as in mold; MOLD_NO_FORK=1 keeps one process
    // for debuggers and profilers.
    if std::env::var_os("MOLD_NO_FORK").is_none() {
        crate::subprocess::fork_child();
    }

    let mut ctx: Context<E> = Context::new(args);
    crate::error::set_suppress_warnings(ctx.args.suppress_warnings);
    crate::error::set_fatal_warnings(ctx.args.fatal_warnings);
    crate::error::set_demangle(ctx.args.demangle);

    // -print_statistics phase timer, in the spirit of mold's --perf.
    let t0 = std::time::Instant::now();
    let mut phases: Vec<(&str, std::time::Duration)> = Vec::new();
    let mut last = t0;
    let mut lap = |phases: &mut Vec<(&str, std::time::Duration)>, name: &'static str| {
        let now = std::time::Instant::now();
        phases.push((name, now - last));
        last = now;
    };

    // Read every input eagerly, then resolve; loading auto-linked
    // libraries or the LTO output adds inputs, so resolution repeats
    // until the input set is stable.
    passes::read_input_files(&mut ctx);
    passes::create_internal_file(&mut ctx);
    crate::error::checkpoint();
    lap(&mut phases, "parse");
    loop {
        passes::resolve_symbols(&mut ctx);
        match passes::load_autolink_deps(&mut ctx) {
            passes::Autolinked::Nothing => break,
            passes::Autolinked::DylibsOnly(first) => {
                passes::claim_new_dylibs(&mut ctx, first);
                break;
            }
            passes::Autolinked::Objects => {}
        }
    }
    if passes::do_lto(&mut ctx) {
        loop {
            passes::resolve_symbols(&mut ctx);
            match passes::load_autolink_deps(&mut ctx) {
                passes::Autolinked::Nothing => break,
                passes::Autolinked::DylibsOnly(first) => {
                    passes::claim_new_dylibs(&mut ctx, first);
                    break;
                }
                passes::Autolinked::Objects => {}
            }
        }
    }
    lap(&mut phases, "resolve");
    passes::check_input_versions(&ctx);
    passes::remove_unreachable_files(&mut ctx);
    passes::check_duplicate_symbols(&ctx);
    crate::error::checkpoint();
    if ctx.args.relocatable {
        passes::hide_all_exports(&mut ctx);
        passes::merge_literals(&mut ctx);
        passes::coalesce_objc_refs(&mut ctx);
        // ld64 -r keeps one copy of each weak definition (the marker
        // stays on it for the final link to auto-hide); Swift's
        // per-object conformance and metadata records doubled
        // NetNewsWire's RSCore prelink's __DATA,__const otherwise.
        passes::coalesce_weak_defs(&mut ctx);
        passes::create_output_sections(&mut ctx);
        crate::macho::relocatable::link(&mut ctx);
        crate::error::checkpoint();
        // Xcode asks every link, its single-object prelinks included,
        // for -dependency_info and fails the build if the file is
        // missing.
        crate::macho::mapfile::write_dependency_info(&ctx);
        crate::subprocess::notify_parent();
        return Ok(0);
    }
    passes::convert_init_offsets(&mut ctx);
    {
        let tt = std::time::Instant::now();
        passes::merge_literals(&mut ctx);
        passes::coalesce_objc_refs(&mut ctx);
        if std::env::var_os("MOLD_TIMING").is_some() {
            eprintln!("    merge_literals {:?}", tt.elapsed());
        }
    }
    macro_rules! tp {
        ($name:expr, $e:expr) => {{
            let tt = std::time::Instant::now();
            $e;
            if std::env::var_os("MOLD_TIMING").is_some() {
                eprintln!("    {} {:?}", $name, tt.elapsed());
            }
        }};
    }
    tp!("add_synthetic_symbols", passes::add_synthetic_symbols(&mut ctx));
    tp!("convert_common_symbols", passes::convert_common_symbols(&mut ctx));
    tp!("create_objc_msgsend_stubs", passes::create_objc_msgsend_stubs(&mut ctx));
    tp!("auto_hide_weak_defs", passes::auto_hide_weak_defs(&mut ctx));
    tp!("hide_all_exports", passes::hide_all_exports(&mut ctx));
    tp!("create_symbol_reexports", passes::create_symbol_reexports(&mut ctx));
    tp!("coalesce_weak_defs", passes::coalesce_weak_defs(&mut ctx));
    passes::print_dependencies(&ctx);
    passes::print_why_load(&ctx);
    passes::print_trace(&ctx);
    crate::error::checkpoint();
    if ctx.args.dead_strip {
        let tt = std::time::Instant::now();
        dead_strip::dead_strip(&mut ctx);
        dead_strip::mark_live_references(&mut ctx);
        if std::env::var_os("MOLD_TIMING").is_some() {
            eprintln!("    dead_strip {:?}", tt.elapsed());
        }
    }
    tp!("report_undef_errors", passes::report_undef_errors(&mut ctx));
    crate::error::checkpoint();
    if ctx.args.deduplicate {
        let tt = std::time::Instant::now();
        crate::macho::icf::icf_sections(&mut ctx);
        if std::env::var_os("MOLD_TIMING").is_some() {
            eprintln!("    icf {:?}", tt.elapsed());
        }
    }
    lap(&mut phases, "passes");
    {
        let tt = std::time::Instant::now();
        passes::scan_relocations(&mut ctx);
        if std::env::var_os("MOLD_TIMING").is_some() {
            eprintln!("    scan_relocations {:?}", tt.elapsed());
        }
    }
    passes::add_entry_stub(&mut ctx);
    passes::scan_unwind_personalities(&mut ctx);
    passes::scan_objc_stubs(&mut ctx);
    passes::fold_objc_classrefs(&mut ctx);
    passes::convert_objc_method_lists(&mut ctx);
    passes::merge_objc_categories(&mut ctx);
    // Synthetic stubs and unwind data can introduce library references
    // (notably dyld_stub_binder). Establish them before pruning dylibs.
    tp!("dead_strip_dylibs", passes::dead_strip_dylibs(&mut ctx));

    // Decide the output layout
    let tt = std::time::Instant::now();
    passes::create_output_sections(&mut ctx);
    let t_sections = tt.elapsed();
    // The output symbol table builds inside set_osec_offsets, as part
    // of the parallel __LINKEDIT task group.
    let tt = std::time::Instant::now();
    passes::set_osec_offsets(&mut ctx);
    let t_offsets = tt.elapsed();
    if std::env::var_os("MOLD_TIMING").is_some() {
        eprintln!("    sections {t_sections:?} offsets {t_offsets:?}");
    }
    passes::fix_synthetic_symbols(&mut ctx);
    passes::resolve_entry(&mut ctx);
    crate::error::checkpoint();
    crate::macho::mapfile::print_map(&ctx);
    crate::macho::mapfile::write_dependency_info(&ctx);
    crate::macho::mapfile::write_sdk_imports(&ctx);
    lap(&mut phases, "layout");

    // Write the output. The file is created up front and its ranges are
    // written from background threads as copy_chunks finishes them;
    // finish() waits for the last one.
    let mut buf = vec![0; ctx.output_size as usize];
    let out = output_file::OutputFile::create(&ctx.args.output, buf.as_ptr(), buf.len());
    passes::copy_chunks(&ctx, &mut buf, &out);
    crate::error::checkpoint();
    {
        let tt = std::time::Instant::now();
        out.finish();
        if std::env::var_os("MOLD_TIMING").is_some() {
            eprintln!("    write-wait {:?}", tt.elapsed());
        }
    }
    crate::subprocess::notify_parent();
    lap(&mut phases, "copy+write");

    // ld64's -print_statistics reports its phase times and memory to
    // stderr; ours reports phases and the sizes that drive them.
    if ctx.args.print_statistics {
        eprintln!("ld total time: {:>8.1?}", t0.elapsed());
        for (name, dur) in &phases {
            eprintln!("  {name:<10} {dur:>8.1?}");
        }
        eprintln!(
            "  objects: {} alive of {}; dylibs: {}; output: {} bytes",
            ctx.objs.iter().enumerate().filter(|(i, o)| o.is_alive && !ctx.is_internal(*i)).count(),
            ctx.objs.len() - 1,
            ctx.dylibs.len(),
            ctx.output_size,
        );
    }

    Ok(0)
}
