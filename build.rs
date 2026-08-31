//! Builds the two small C parts of mold.
//!
//! `mold-wrapper.so` is the preload library behind `mold -run`. It
//! interposes the exec family, including the C-variadic `execl` functions
//! that stable Rust cannot define, so it is a C file. It is placed next to
//! the `mold` executable, where `-run` looks for it.
//!
//! `lto-message.c` adapts the LTO plugin's printf-like diagnostics
//! callback, which is C-variadic as well, and is linked into mold.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=c/mold-wrapper.c");
    println!("cargo:rerun-if-changed=c/lto-message.c");

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let sanitizer = std::env::var("CARGO_CFG_SANITIZE")
        .ok()
        .map(|value| format!("-fsanitize={value}"));

    let object = out_dir.join("lto-message.o");
    let mut command = Command::new(&cc);
    command.args(["-c", "-O2", "-fPIC", "-o"]);
    command.arg(&object).arg("c/lto-message.c");
    command.args(&sanitizer);
    let status = command.status();
    if !matches!(status, Ok(s) if s.success()) {
        panic!("could not compile c/lto-message.c with {cc}");
    }
    let archive = out_dir.join("libltomessage.a");
    let status = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&object)
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        panic!("could not create {}", archive.display());
    }
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=ltomessage");

    // Cargo uses different build directory layouts with and without
    // -Zbuild-std, so find the profile directory by name.
    let profile = std::env::var("PROFILE").unwrap();
    let profile_dir = out_dir
        .ancestors()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(profile.as_str()))
        .unwrap();
    let wrapper = profile_dir.join("mold-wrapper.so");
    let mut command = Command::new(&cc);
    command.args(["-shared", "-fPIC", "-O2", "-o"]);
    command.arg(&wrapper).arg("c/mold-wrapper.c").arg("-ldl");
    command.args(&sanitizer);
    let status = command.status();
    if !matches!(status, Ok(s) if s.success()) {
        println!("cargo:warning=could not build mold-wrapper.so; `mold -run` will not work");
    }
}
