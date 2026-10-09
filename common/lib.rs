//! Code that mold's linkers share. Nothing here depends on the format of
//! object files.

pub mod archive_file;
pub mod bits;
pub mod bytes;
pub mod cityhash;
pub mod compress;
pub mod concurrent_map;
pub mod demangle;
pub mod endian;
pub mod error;
pub mod glob;
pub mod hyperloglog;
pub mod jobs;
pub mod leb128;
pub mod mapped_file;
pub mod mem;
pub mod output_file;
pub mod parallel;
pub mod path;
pub mod perf;
mod prefetch;
pub mod record;
pub mod response_file;
pub mod siphash;
pub mod subprocess;
pub mod tar;
pub mod worker_local;

pub use prefetch::prefetch;

/// The git commit that mold is built from, or None if the source tree has
/// no git metadata, as in a source tarball.
pub const GIT_HASH: Option<&str> = option_env!("MOLD_GIT_HASH");
