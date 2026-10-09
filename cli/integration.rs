use std::process::ExitCode;

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    use std::path::Path;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cases = [root.join("tests/elf")];
    mold_elf_tests::run(&cases, Path::new(env!("CARGO_BIN_EXE_mold")))
}

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    println!("skipped: the ELF tests don't run on macOS");
    ExitCode::SUCCESS
}
