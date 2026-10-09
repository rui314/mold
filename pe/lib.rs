//! The PE/COFF linker of mold, for x86_64 images such as UEFI applications.
//!
//! This crate reads the lld-link command line that rustc passes to linkers for
//! MSVC-style targets, and COFF object files.

pub mod args;
pub mod coff;
