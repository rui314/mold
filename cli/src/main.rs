//! The `mold` executable: runs the linker instantiated for the target the
//! inputs are for. Each target is instantiated in a crate of its own so
//! that the compiler can build them in parallel, and a feature per target
//! decides which of them are built in.

// A Rust executable can define only one global allocator, so select mimalloc
// here rather than in the linker library.
#[cfg(not(feature = "system-allocator"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// C++ mold also suppresses ignorable mimalloc warnings and re-enables
// transparent huge pages when a parent disabled them. The setting is inherited,
// and huge pages improve large links. The Rust entry point currently does
// neither.

// Each target has its own monomorphized link function. Start with x86-64 and,
// if the inputs use another machine type, run the matching function instead.
fn link_for_target(target: &str, cmdline: &[std::ffi::OsString]) -> Result<i32, String> {
    match target {
        #[cfg(feature = "x86_64")]
        "x86_64" => mold_target_x86_64::link(cmdline),
        #[cfg(feature = "i386")]
        "i386" => mold_target_i386::link(cmdline),
        #[cfg(feature = "arm32")]
        "arm32" => mold_target_arm32::link(cmdline),
        #[cfg(feature = "arm32be")]
        "arm32be" => mold_target_arm32be::link(cmdline),
        #[cfg(feature = "arm64")]
        "arm64" => mold_target_arm64::link(cmdline),
        #[cfg(feature = "arm64be")]
        "arm64be" => mold_target_arm64be::link(cmdline),
        #[cfg(feature = "riscv64")]
        "riscv64" => mold_target_riscv64::link(cmdline),
        #[cfg(feature = "riscv64be")]
        "riscv64be" => mold_target_riscv64be::link(cmdline),
        #[cfg(feature = "riscv32")]
        "riscv32" => mold_target_riscv32::link(cmdline),
        #[cfg(feature = "riscv32be")]
        "riscv32be" => mold_target_riscv32be::link(cmdline),
        #[cfg(feature = "ppc64v2")]
        "ppc64v2" => mold_target_ppc64v2::link(cmdline),
        #[cfg(feature = "ppc32")]
        "ppc32" => mold_target_ppc32::link(cmdline),
        #[cfg(feature = "ppc64v1")]
        "ppc64v1" => mold_target_ppc64v1::link(cmdline),
        #[cfg(feature = "s390x")]
        "s390x" => mold_target_s390x::link(cmdline),
        #[cfg(feature = "sparc64")]
        "sparc64" => mold_target_sparc64::link(cmdline),
        #[cfg(feature = "m68k")]
        "m68k" => mold_target_m68k::link(cmdline),
        #[cfg(feature = "sh4")]
        "sh4" => mold_target_sh4::link(cmdline),
        #[cfg(feature = "sh4be")]
        "sh4be" => mold_target_sh4be::link(cmdline),
        #[cfg(feature = "loongarch64")]
        "loongarch64" => mold_target_loongarch64::link(cmdline),
        #[cfg(feature = "loongarch32")]
        "loongarch32" => mold_target_loongarch32::link(cmdline),
        _ => {
            eprintln!("mold: unsupported target: {target}; rebuild mold with the appropriate target support");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args = std::env::args_os().collect();
    let status = mold::driver::main(args, link_for_target);
    std::process::exit(status);
}
