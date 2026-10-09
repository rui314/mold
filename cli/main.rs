//! The `mold` executable: runs the ELF linker, or the Mach-O linker if
//! invoked as `ld64.mold`, instantiated for the target the inputs are for.
//! Each target is instantiated in a crate of its own so that the compiler
//! can build them in parallel, and a feature per target decides which of
//! them are built in.

use mold_elf::driver::{Cmdline, LinkResult};
use std::ffi::OsStr;
use std::path::Path;

// A Rust executable can define only one global allocator, so select mimalloc
// here rather than in the linker library.
#[cfg(not(feature = "system-allocator"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

type LinkFn = fn(Cmdline) -> LinkResult;

// Each target has its own monomorphized link function. Start with the first
// enabled target and switch to the matching function if the inputs are for
// another target.
const ELF_TARGETS: &[(&str, LinkFn)] = &[
    #[cfg(feature = "x86_64")]
    ("x86_64", mold_elf_x86_64::link),
    #[cfg(feature = "i386")]
    ("i386", mold_elf_i386::link),
    #[cfg(feature = "arm32")]
    ("arm32", mold_elf_arm32::link),
    #[cfg(feature = "arm32be")]
    ("arm32be", mold_elf_arm32be::link),
    #[cfg(feature = "arm64")]
    ("arm64", mold_elf_arm64::link),
    #[cfg(feature = "arm64be")]
    ("arm64be", mold_elf_arm64be::link),
    #[cfg(feature = "riscv64")]
    ("riscv64", mold_elf_riscv64::link),
    #[cfg(feature = "riscv64be")]
    ("riscv64be", mold_elf_riscv64be::link),
    #[cfg(feature = "riscv32")]
    ("riscv32", mold_elf_riscv32::link),
    #[cfg(feature = "riscv32be")]
    ("riscv32be", mold_elf_riscv32be::link),
    #[cfg(feature = "ppc64v2")]
    ("ppc64v2", mold_elf_ppc64v2::link),
    #[cfg(feature = "ppc32")]
    ("ppc32", mold_elf_ppc32::link),
    #[cfg(feature = "ppc64v1")]
    ("ppc64v1", mold_elf_ppc64v1::link),
    #[cfg(feature = "s390x")]
    ("s390x", mold_elf_s390x::link),
    #[cfg(feature = "sparc64")]
    ("sparc64", mold_elf_sparc64::link),
    #[cfg(feature = "m68k")]
    ("m68k", mold_elf_m68k::link),
    #[cfg(feature = "sh4")]
    ("sh4", mold_elf_sh4::link),
    #[cfg(feature = "sh4be")]
    ("sh4be", mold_elf_sh4be::link),
    #[cfg(feature = "loongarch64")]
    ("loongarch64", mold_elf_loongarch64::link),
    #[cfg(feature = "loongarch32")]
    ("loongarch32", mold_elf_loongarch32::link),
];

const MACHO_TARGETS: &[(&str, LinkFn)] = &[
    #[cfg(feature = "macho-arm64")]
    ("arm64", mold_macho_arm64::link),
    #[cfg(feature = "macho-x86_64")]
    ("x86_64", mold_macho_x86_64::link),
];

fn link_for_target(targets: &[(&str, LinkFn)], target: &str, cmdline: Cmdline) -> LinkResult {
    for &(name, link) in targets {
        if name == target {
            return link(cmdline);
        }
    }
    eprintln!(
        "mold: unsupported target: {target}; rebuild mold with the appropriate target support"
    );
    std::process::exit(1);
}

// As with lld's ld64.lld, the name tells mold to act as Apple's linker,
// ld64. clang on macOS looks for ld64.mold when given -fuse-ld=mold.
fn is_ld64(argv0: &OsStr) -> bool {
    let name = Path::new(argv0).file_name().unwrap_or_default().as_encoded_bytes();
    name == b"ld64" || name.starts_with(b"ld64.")
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let macho = args.first().is_some_and(|argv0| is_ld64(argv0));
    let targets = if macho { MACHO_TARGETS } else { ELF_TARGETS };

    let Some(&(initial_target, _)) = targets.first() else {
        eprintln!("mold: no targets enabled; rebuild mold with the appropriate target support");
        std::process::exit(1);
    };
    let link = |target: &str, cmdline| link_for_target(targets, target, cmdline);
    let status = if macho {
        mold_macho::driver::main(args, initial_target, link)
    } else {
        mold_elf::driver::main(args, initial_target, link)
    };
    std::process::exit(status);
}
