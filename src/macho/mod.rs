//! The Mach-O linker: mold for macOS.
//!
//! This tree mirrors the ELF linker one level up: `driver` runs the
//! passes in order, `passes` implements them, `arch` isolates the
//! target-dependent relocation handling, and `output_chunks` builds every
//! piece of the output file. The diagnostics, input-file mapping, archive
//! reading, file-type detection, forking and small helpers are the ELF
//! linker's own, shared through `crate::`.

pub mod arch;
pub mod cmdline;
pub mod context;
pub mod dead_strip;
pub mod driver;
pub mod dwarf;
pub mod files;
pub mod format;
pub mod icf;
pub mod input_files;
pub mod input_sections;
pub mod lto;
pub mod mapfile;
pub mod output_chunks;
pub mod output_file;
pub mod passes;
pub mod relocatable;
pub mod symbol;
pub mod tapi;
pub mod thunks;
pub mod util;
