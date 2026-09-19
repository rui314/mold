use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    if !cfg!(target_os = "macos") {
        println!("Mach-O tests need Apple's toolchain; skipped on this host");
        return ExitCode::SUCCESS;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    mold_tests::run_macho(&root.join("tests/macho"), Path::new(env!("CARGO_BIN_EXE_mold")))
}
