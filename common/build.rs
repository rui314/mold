//! Finds the git commit that mold is built from, for the linkers' version
//! strings.

use std::path::{Path, PathBuf};
use std::process::Command;

// Embed the current git commit hash into the mold executable. We ask git
// instead of parsing files under .git because the repository format varies
// (e.g. packed refs, reftable). When building from a source tarball, there's
// no .git, and the hash is simply omitted.
fn git_hash(source_dir: &Path) -> Option<String> {
    let dot_git = source_dir.join(".git");
    if !dot_git.exists() {
        // Source archives use the package version. If Git metadata is added
        // later, a clean rebuild is needed to embed the new commit hash.
        return None;
    }
    if dot_git.is_file() {
        println!("cargo:rerun-if-changed={}", dot_git.display());
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
        Some(if path.is_absolute() { path } else { source_dir.join(path) })
    };

    let reftable = git_path("reftable/tables.list").filter(|p| p.exists());
    if let Some(head) = git_path("HEAD").filter(|p| p.exists()) {
        println!("cargo:rerun-if-changed={}", head.display());
        if let Ok(contents) = std::fs::read_to_string(&head)
            && let Some(reference) =
                contents.strip_prefix("ref: ").map(str::trim).filter(|_| reftable.is_none())
            && let Some(path) = git_path(reference)
        {
            // A packed reference may acquire a loose file later.
            // Watch its nearest existing directory until that happens.
            if let Some(path) = path.ancestors().find(|p| p.exists()) {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    for path in git_path("packed-refs").into_iter().chain(reftable).filter(|p| p.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let output =
        Command::new("git").arg("-C").arg(source_dir).args(["rev-parse", "HEAD"]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // This crate is in common/ of the source tree.
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    if let Some(hash) = git_hash(manifest_dir.parent().unwrap()) {
        println!("cargo:rustc-env=MOLD_GIT_HASH={hash}");
    }
}
