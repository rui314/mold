use std::process::ExitCode;

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    use std::path::Path;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cases = [root.join("tests/macho")];
    mold_macho_tests::run(&cases, Path::new(env!("CARGO_BIN_EXE_mold")))
}

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    println!("skipped: the Mach-O tests run only on macOS");
    ExitCode::SUCCESS
}
