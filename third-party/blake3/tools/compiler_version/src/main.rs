use std::process::Command;

fn main() {
    // Print the rustc version.
    Command::new(env!("CARGO"))
        .args(&["rustc", "--quiet", "--", "--version"])
        .status()
        .unwrap();
    println!();

    // Print the Cargo version.
    Command::new(env!("CARGO"))
        .args(&["--version"])
        .status()
        .unwrap();
    println!();

    // Print the C compiler version. This relies on C compiler detection done
    // in build.rs, which sets the COMPILER_PATH variable.
    let compiler_path = env!("COMPILER_PATH");
    let mut compiler_command = Command::new(compiler_path);
    // Use the --version flag on everything other than MSVC.
    if !cfg!(target_env = "msvc") {
        compiler_command.arg("--version");
    }
    let _ = compiler_command.status().unwrap();
}
