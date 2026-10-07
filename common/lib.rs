//! Code that mold's linkers share. Nothing here depends on the format of
//! object files.

pub mod archive_file;
pub mod compress;
pub mod concurrent_map;
pub mod demangle;
pub mod endian;
pub mod error;
pub mod glob;
pub mod hyperloglog;
pub mod jobs;
pub mod mapped_file;
pub mod output_file;
pub mod parallel;
pub mod perf;
mod prefetch;
pub mod siphash;
pub mod subprocess;
pub mod tar;
pub mod util;
pub mod worker_local;

pub use prefetch::prefetch;
