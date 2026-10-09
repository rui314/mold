//! Runs the shell tests of the format the host builds programs for. The
//! Mach-O tests drive Apple's toolchain, which only macOS has, and the ELF
//! tests need a toolchain that makes ELF programs, which macOS lacks.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(target_os = "macos")]
    let (cases, run) = ("tests/macho", mold_tests::macho::run);
    #[cfg(not(target_os = "macos"))]
    let (cases, run) = ("tests/elf", mold_tests::elf::run);

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    run(&root.join(cases), Path::new(env!("CARGO_BIN_EXE_mold")))
}
