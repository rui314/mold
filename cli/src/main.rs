//! The `mold` executable: runs the linker instantiated for the target the
//! inputs are for. Each target is instantiated in a crate of its own so
//! that the compiler can build them in parallel, and a feature per target
//! decides which of them are built in.

use mold::error::Diagnostics;

// Including mimalloc-new-delete.h overrides the new/delete operators.
// We need it only when using mimalloc as a dynamic library.
// This header should be included in only one source file, so we do
// it in this file.
//
// Rust selects the allocator in this executable for the same one-place rule.
#[cfg(not(feature = "system-allocator"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Silence mimalloc warnings that users can ignore

// A parent process may have disabled transparent huge pages, and the
// flag is inherited. Huge pages make mold considerably faster on
// large links, so re-enable them for this process.
//
// These two C++ entry-point operations are owned by mimalloc/C++-specific
// APIs; the Rust entry point currently has no corresponding calls.

// Since mold_main is a template, we can't run it without a type parameter.
// We speculatively run mold_main with X86_64, and if the speculation was
// wrong, re-run it with an actual machine type.
fn link_for_target(target: &str, cmdline: &[String], diag: &Diagnostics) -> Result<i32, String> {
    match target {
        #[cfg(feature = "x86_64")]
        "x86_64" => mold_target_x86_64::link(cmdline, diag),
        #[cfg(feature = "i386")]
        "i386" => mold_target_i386::link(cmdline, diag),
        #[cfg(feature = "arm32")]
        "arm32" => mold_target_arm32::link(cmdline, diag),
        #[cfg(feature = "arm32be")]
        "arm32be" => mold_target_arm32be::link(cmdline, diag),
        #[cfg(feature = "arm64")]
        "arm64" => mold_target_arm64::link(cmdline, diag),
        #[cfg(feature = "arm64be")]
        "arm64be" => mold_target_arm64be::link(cmdline, diag),
        #[cfg(feature = "riscv64")]
        "riscv64" => mold_target_riscv64::link(cmdline, diag),
        #[cfg(feature = "riscv64be")]
        "riscv64be" => mold_target_riscv64be::link(cmdline, diag),
        #[cfg(feature = "riscv32")]
        "riscv32" => mold_target_riscv32::link(cmdline, diag),
        #[cfg(feature = "riscv32be")]
        "riscv32be" => mold_target_riscv32be::link(cmdline, diag),
        #[cfg(feature = "ppc64v2")]
        "ppc64v2" => mold_target_ppc64v2::link(cmdline, diag),
        #[cfg(feature = "ppc32")]
        "ppc32" => mold_target_ppc32::link(cmdline, diag),
        #[cfg(feature = "ppc64v1")]
        "ppc64v1" => mold_target_ppc64v1::link(cmdline, diag),
        #[cfg(feature = "s390x")]
        "s390x" => mold_target_s390x::link(cmdline, diag),
        #[cfg(feature = "sparc64")]
        "sparc64" => mold_target_sparc64::link(cmdline, diag),
        #[cfg(feature = "m68k")]
        "m68k" => mold_target_m68k::link(cmdline, diag),
        #[cfg(feature = "sh4")]
        "sh4" => mold_target_sh4::link(cmdline, diag),
        #[cfg(feature = "sh4be")]
        "sh4be" => mold_target_sh4be::link(cmdline, diag),
        #[cfg(feature = "loongarch64")]
        "loongarch64" => mold_target_loongarch64::link(cmdline, diag),
        #[cfg(feature = "loongarch32")]
        "loongarch32" => mold_target_loongarch32::link(cmdline, diag),
        _ => {
            eprintln!("mold: unsupported target: {target}; rebuild mold with the appropriate target support");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let status = mold::driver::main(args, link_for_target);
    std::process::exit(status);
}
