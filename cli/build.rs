//! Passes host-dependent options to the linker for the mold executable.

use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // mimalloc uses 64-bit atomics, which C compilers implement as calls
    // into libatomic on hosts without them, such as 32-bit PowerPC.
    let widths = env::var("CARGO_CFG_TARGET_HAS_ATOMIC").unwrap_or_default();
    if !widths.split(',').any(|w| w == "64") {
        // rustc places libraries named by this crate before its dependencies
        // on the linker command line, where --as-needed would drop libatomic.
        // Link arguments come after all libraries.
        println!("cargo:rustc-link-arg-bins=-latomic");
    }

    // The linker is instantiated for each target, and many functions compile
    // to the same machine code for several targets. Identical Code Folding
    // merges them, which roughly halves the text of a release build. GNU ld
    // doesn't support it, so we pass the option only if the linker takes it.
    if env::var("PROFILE").unwrap() == "release" && link_succeeds("-Wl,--icf=all") {
        println!("cargo:rustc-link-arg-bins=-Wl,--icf=all");
    }
}

/// Links an empty program with the given linker argument, using the same
/// compiler, linker and flags as the mold executable.
fn link_succeeds(arg: &str) -> bool {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let mut cmd = Command::new(env::var_os("RUSTC").unwrap());
    cmd.arg("--target").arg(env::var("TARGET").unwrap());
    cmd.arg("-o").arg(Path::new(&out_dir).join("link-probe"));
    cmd.arg(format!("-Clink-arg={arg}"));
    if let Some(linker) = env::var_os("RUSTC_LINKER") {
        let mut opt = OsString::from("-Clinker=");
        opt.push(linker);
        cmd.arg(opt);
    }
    // Flags are separated by 0x1f.
    let flags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    cmd.args(flags.split('\x1f').filter(|flag| !flag.is_empty()));
    cmd.arg("-");
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());

    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    child.stdin.take().unwrap().write_all(b"fn main() {}").unwrap();
    child.wait().is_ok_and(|status| status.success())
}
