//! The `mold` executable: runs the linker instantiated for the target the
//! inputs are for. Each target is instantiated in a crate of its own so
//! that the compiler can build them in parallel, and a feature per target
//! decides which of them are built in.

// A Rust executable can define only one global allocator, so select mimalloc
// here rather than in the linker library.
#[cfg(not(feature = "system-allocator"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

type LinkFn = fn(&[std::ffi::OsString]) -> Result<i32, String>;

// Each target has its own monomorphized link function. Start with the first
// enabled target and switch to the matching function if the inputs differ.
const TARGETS: &[(&str, LinkFn)] = &[
    #[cfg(feature = "x86_64")]
    ("x86_64", mold_target_x86_64::link),
    #[cfg(feature = "i386")]
    ("i386", mold_target_i386::link),
    #[cfg(feature = "arm32")]
    ("arm32", mold_target_arm32::link),
    #[cfg(feature = "arm32be")]
    ("arm32be", mold_target_arm32be::link),
    #[cfg(feature = "arm64")]
    ("arm64", mold_target_arm64::link),
    #[cfg(feature = "arm64be")]
    ("arm64be", mold_target_arm64be::link),
    #[cfg(feature = "riscv64")]
    ("riscv64", mold_target_riscv64::link),
    #[cfg(feature = "riscv64be")]
    ("riscv64be", mold_target_riscv64be::link),
    #[cfg(feature = "riscv32")]
    ("riscv32", mold_target_riscv32::link),
    #[cfg(feature = "riscv32be")]
    ("riscv32be", mold_target_riscv32be::link),
    #[cfg(feature = "ppc64v2")]
    ("ppc64v2", mold_target_ppc64v2::link),
    #[cfg(feature = "ppc32")]
    ("ppc32", mold_target_ppc32::link),
    #[cfg(feature = "ppc64v1")]
    ("ppc64v1", mold_target_ppc64v1::link),
    #[cfg(feature = "s390x")]
    ("s390x", mold_target_s390x::link),
    #[cfg(feature = "sparc64")]
    ("sparc64", mold_target_sparc64::link),
    #[cfg(feature = "m68k")]
    ("m68k", mold_target_m68k::link),
    #[cfg(feature = "sh4")]
    ("sh4", mold_target_sh4::link),
    #[cfg(feature = "sh4be")]
    ("sh4be", mold_target_sh4be::link),
    #[cfg(feature = "loongarch64")]
    ("loongarch64", mold_target_loongarch64::link),
    #[cfg(feature = "loongarch32")]
    ("loongarch32", mold_target_loongarch32::link),
];

fn link_for_target(target: &str, cmdline: &[std::ffi::OsString]) -> Result<i32, String> {
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
    let status = mold::driver::main(args, initial_target, link_for_target);
    std::process::exit(status);
}
