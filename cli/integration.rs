use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let cases = [root.join("tests/cases"), root.join("test")];
    mold_tests::run(&cases, Path::new(env!("CARGO_BIN_EXE_mold")))
}
