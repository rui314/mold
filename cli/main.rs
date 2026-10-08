//! The `ld64.mold` executable: runs the linker instantiated for the
//! target the inputs are for. Each target is instantiated in a crate of
//! its own so that the compiler can build them in parallel, and a feature
//! per target decides which of them are built in.

use mold_macho::driver::{Cmdline, LinkResult};

// A Rust executable can define only one global allocator, so select mimalloc
// here rather than in the linker library.
#[cfg(not(feature = "system-allocator"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

type LinkFn = fn(Cmdline) -> LinkResult;

// Each target has its own monomorphized link function. Start with the first
// enabled target and switch to the matching function if the inputs differ.
const TARGETS: &[(&str, LinkFn)] = &[
    #[cfg(feature = "arm64")]
    ("arm64", mold_macho_arm64::link),
    #[cfg(feature = "x86_64")]
    ("x86_64", mold_macho_x86_64::link),
];

fn link_for_target(target: &str, cmdline: Cmdline) -> LinkResult {
    for &(name, link) in TARGETS {
        if name == target {
            return link(cmdline);
        }
    }
    eprintln!(
        "mold: unsupported target: {target}; rebuild mold with the appropriate target support"
    );
    std::process::exit(1);
}

fn main() {
    let Some(&(initial_target, _)) = TARGETS.first() else {
        eprintln!("mold: no targets enabled; rebuild mold with the appropriate target support");
        std::process::exit(1);
    };
    let args = std::env::args_os().collect();
    let status = mold_macho::driver::main(args, initial_target, link_for_target);
    std::process::exit(status);
}
