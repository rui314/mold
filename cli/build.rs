//! Links libatomic into mold on hosts without 64-bit atomics, such as
//! 32-bit PowerPC. mimalloc uses 64-bit atomics, which C compilers
//! implement as calls into libatomic on those hosts.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let widths = std::env::var("CARGO_CFG_TARGET_HAS_ATOMIC").unwrap_or_default();
    if !widths.split(',').any(|w| w == "64") {
        // rustc places libraries named by this crate before its dependencies
        // on the linker command line, where --as-needed would drop libatomic.
        // Link arguments come after all libraries.
        println!("cargo:rustc-link-arg-bins=-latomic");
    }
}
