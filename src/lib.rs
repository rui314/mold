//! A high-performance ELF linker.

pub mod arch;
pub mod archive_file;
pub mod cmdline;
pub mod context;
pub mod driver;
pub mod elf;
pub mod error;
pub mod filetype;
pub mod gc_sections;
pub mod gdb_index;
pub mod icf;
pub mod input_files;
pub mod input_sections;
pub mod jobs;
pub mod linker_script;
#[cfg(not(all(target_os = "windows", target_env = "msvc")))]
pub mod lto;
#[cfg(all(target_os = "windows", target_env = "msvc"))]
#[path = "lto_win32.rs"]
pub mod lto;
pub mod mapfile;
pub mod mapped_file;
pub mod output_chunks;
pub mod output_file;
pub mod passes;
pub mod reader;
pub mod relocatable;
pub mod shrink_sections;
pub mod subprocess;
pub mod symbol;
pub mod thunks;
pub mod tls;
pub mod util;
