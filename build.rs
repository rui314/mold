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

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut build = cc::Build::new();
    build.opt_level(2).pic(true);
    if let Ok(sanitizer) = std::env::var("CARGO_CFG_SANITIZE") {
        build.flag(format!("-fsanitize={sanitizer}"));
    }
    build.file("c/lto-message.c").compile("ltomessage");

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
    let mut command = build.get_compiler().to_command();
    command.args(["-shared", "-o"]);
    command.arg(&wrapper).arg("c/mold-wrapper.c");
    if target_os == "android" || target_os == "linux" {
        command.arg("-ldl");
    }
    let status = command.status();
    if !matches!(status, Ok(s) if s.success()) {
        println!("cargo:warning=could not build mold-wrapper.so; `mold -run` will not work");
    }
}
