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
    println!("cargo:rerun-if-changed=csrc/mold-wrapper.c");
    println!("cargo:rerun-if-changed=csrc/lto-message.c");

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let object = out_dir.join("lto-message.o");
    let status = Command::new(&cc)
        .args(["-c", "-O2", "-fPIC", "-o"])
        .arg(&object)
        .arg("csrc/lto-message.c")
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        panic!("could not compile csrc/lto-message.c with {cc}");
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

    // OUT_DIR is <target>/<profile>/build/<pkg>-<hash>/out.
    let profile_dir = out_dir.ancestors().nth(3).unwrap().to_path_buf();
    let wrapper = profile_dir.join("mold-wrapper.so");
    let status = Command::new(&cc)
        .args(["-shared", "-fPIC", "-O2", "-o"])
        .arg(&wrapper)
        .arg("csrc/mold-wrapper.c")
        .arg("-ldl")
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        println!("cargo:warning=could not build mold-wrapper.so; `mold -run` will not work");
    }
}
