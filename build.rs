//! Builds the two small C parts of mold.
//!
//! `mold-wrapper.so` is the preload library behind `mold -run`. It
//! interposes the exec family, including the C-variadic `execl` functions
//! that stable Rust cannot define, so it is a C file. It is placed next to
//! the `mold` executable, where `-run` looks for it.
//!
//! `lto-message.c` adapts the LTO plugin's printf-like diagnostics
//! callback, which is C-variadic as well, and is linked into mold.

use std::path::{Path, PathBuf};
use std::process::Command;

// Embed the current git commit hash into the mold executable. We ask git
// instead of parsing files under .git because the repository format varies
// (e.g. packed refs, reftable). When building from a source tarball, there's
// no .git, and the hash is simply omitted.
fn git_hash(source_dir: &Path) -> Option<String> {
    let dot_git = source_dir.join(".git");
    println!("cargo:rerun-if-changed={}", dot_git.display());
    if !dot_git.exists() {
        return None;
    }

    let git_path = |name: &str| -> Option<PathBuf> {
        let output = Command::new("git")
            .arg("-C")
            .arg(source_dir)
            .args(["rev-parse", "--git-path", name])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
        Some(if path.is_absolute() {
            path
        } else {
            source_dir.join(path)
        })
    };

    if let Some(head) = git_path("HEAD") {
        println!("cargo:rerun-if-changed={}", head.display());
        if let Ok(contents) = std::fs::read_to_string(&head) {
            if let Some(reference) = contents.strip_prefix("ref: ").map(str::trim) {
                if let Some(path) = git_path(reference) {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }
    if let Some(path) = git_path("packed-refs") {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(source_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    let source_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let version = match git_hash(&source_dir) {
        Some(hash) => format!("mold {version} ({hash}; compatible with GNU ld)"),
        None => format!("mold {version} (compatible with GNU ld)"),
    };
    println!("cargo:rustc-env=MOLD_VERSION={version}");

    println!("cargo:rerun-if-changed=c/mold-wrapper.c");
    println!("cargo:rerun-if-changed=c/lto-message.c");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap();
    if target_os == "windows" && target_env == "msvc" {
        return;
    }

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

    if target_os == "windows" || target_os == "macos" {
        return;
    }

    // The build directory is inside the actual profile directory. PROFILE
    // only describes inheritance and need not be that directory's name.
    let profile_dir = out_dir
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "build"))
        .and_then(Path::parent)
        .expect("OUT_DIR must be inside Cargo's build directory");
    let wrapper = profile_dir.join("mold-wrapper.so");
    let mut command = Command::new(&cc);
    command.args(["-shared", "-fPIC", "-O2", "-o"]);
    command.arg(&wrapper).arg("c/mold-wrapper.c");
    if target_os == "android" || target_os == "linux" {
        command.arg("-ldl");
    }
    command.args(&sanitizer);
    let status = command.status();
    if !matches!(status, Ok(s) if s.success()) {
        println!("cargo:warning=could not build mold-wrapper.so; `mold -run` will not work");
    }
}
