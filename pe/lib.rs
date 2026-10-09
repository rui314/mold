//! The PE/COFF linker of mold, for x86_64 images such as UEFI applications.
//!
//! It reads COFF objects and archives, and accepts the command line of
//! lld-link, which rustc passes to linkers for its MSVC-style targets.
//!
//! - [`args`] parses the command line.
//! - [`coff`] reads object files.
//! - `link` resolves symbols, selects COMDAT sections and garbage-collects.
//! - `image` lays out the sections and writes the PE file.

pub mod args;
pub mod coff;
mod image;
mod link;

use std::ffi::OsString;
use std::path::Path;

/// Returns true if the command line is one that rustc uses for an MSVC-style
/// linker: `-flavor link ...`, or a program named `lld-link`.
pub fn is_invocation(argv: &[OsString]) -> bool {
    if argv.get(1).is_some_and(|a| a == "-flavor") && argv.get(2).is_some_and(|a| a == "link") {
        return true;
    }
    argv.first()
        .and_then(|a| Path::new(a).file_name())
        .is_some_and(|name| name.to_string_lossy().starts_with("lld-link"))
}

/// Links the program described by `argv` and returns the exit status.
pub fn main(argv: Vec<OsString>) -> i32 {
    let opts = args::parse(&argv);
    link::link(opts);
    0
}
