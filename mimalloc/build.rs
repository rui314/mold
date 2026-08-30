//! Builds the vendored mimalloc (csrc/mimalloc, the same copy the C++
//! mold links) into a static library, from its single-file build.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../csrc/mimalloc");
    println!("cargo:rerun-if-changed={}", root.join("src").display());
    println!("cargo:rerun-if-changed={}", root.join("include").display());

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let object = out_dir.join("mimalloc.o");

    // MI_USE_ENVIRON=0 as in mold's build: the allocator's behavior does
    // not depend on the environment.
    let status = Command::new(&cc)
        .args([
            "-c",
            "-O3",
            "-DNDEBUG",
            "-DMI_USE_ENVIRON=0",
            "-fPIC",
            "-fvisibility=hidden",
            "-I",
        ])
        .arg(root.join("include"))
        .arg("-o")
        .arg(&object)
        .arg(root.join("src/static.c"))
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        panic!("could not compile csrc/mimalloc/src/static.c with {cc}");
    }

    let archive = out_dir.join("libmimalloc.a");
    let status = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&object)
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        panic!("could not create {}", archive.display());
    }
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=mimalloc");
}
