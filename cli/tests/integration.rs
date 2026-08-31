use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cases = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/cases");
    mold_tests::run(&cases, Path::new(env!("CARGO_BIN_EXE_mold")))
}
