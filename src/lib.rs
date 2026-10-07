//! A high-performance ELF linker.

pub mod arch;
pub(crate) mod archive_file;
pub(crate) mod build_id_tree;
mod change_set;
pub(crate) mod chunks;
pub(crate) mod cmdline;
pub(crate) mod context;
pub mod driver;
pub mod elf;
mod elf_consts;
pub(crate) mod error;
pub(crate) mod filetype;
pub(crate) mod gc_sections;
pub(crate) mod gdb_index;
pub(crate) mod icf;
pub(crate) mod incremental;
pub(crate) mod input_files;
pub(crate) mod input_sections;
pub(crate) mod jobs;
pub(crate) mod linker_script;
#[cfg(not(all(target_os = "windows", target_env = "msvc")))]
pub(crate) mod lto;
#[cfg(all(target_os = "windows", target_env = "msvc"))]
#[path = "lto_win32.rs"]
pub(crate) mod lto;
pub(crate) mod mapfile;
pub(crate) mod mapped_file;
mod merge_image;
pub(crate) mod micro_link;
pub(crate) mod output_file;
pub(crate) mod passes;
pub(crate) mod reader;
mod reloc_env;
pub(crate) mod relocatable;
mod semantic_cells;
pub(crate) mod shrink_sections;
pub(crate) mod subprocess;
pub(crate) mod symbol;
pub(crate) mod thunks;
pub(crate) mod tls;
pub mod util;
