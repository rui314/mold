//! Input file access for the Mach-O linker, on top of the shared
//! [`MappedFile`].
//!
//! Opens are memoized by path: a file named twice (a library on the
//! command line and in a prefetch, an archive listed repeatedly) gets one
//! mapping, which also lets downstream caches key by data address.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::fatal;
use crate::mapped_file::MappedFile;

static CACHE: Mutex<Option<HashMap<PathBuf, Option<&'static MappedFile>>>> = Mutex::new(None);

fn cached(
    path: &Path,
    open: impl FnOnce() -> Option<&'static MappedFile>,
) -> Option<&'static MappedFile> {
    if let Some(&mf) = CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(path) {
        return mf;
    }
    let mf = open();
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(path.to_path_buf(), mf);
    mf
}

/// Maps a file, or returns None if it doesn't exist. A path that is not a
/// regular file (a framework directory, say) reads as not found.
pub fn open(path: &Path) -> Option<&'static MappedFile> {
    cached(path, || {
        if path.exists() && !path.is_file() {
            return None;
        }
        MappedFile::open(path)
    })
}

/// Maps a file that must exist.
pub fn must_open(path: &Path) -> &'static MappedFile {
    open(path)
        .unwrap_or_else(|| fatal!("cannot open {}: No such file or directory", path.display()))
}

/// Splits an archive into its members, skipping the `__.SYMDEF` index.
/// A member is named `archive(member)`, the spelling ld64 uses in its
/// diagnostics and in the debug-note stabs of a relocatable output.
pub fn read_archive_members(mf: &'static MappedFile) -> Vec<&'static MappedFile> {
    let base = mf.data().as_ptr() as usize;
    crate::archive_file::archive_members(mf, false)
        .map(|(name, body)| {
            let full = format!("{}({})", mf.name_str(), name.display());
            mf.slice(PathBuf::from(full), body.as_ptr() as usize - base, body.len())
        })
        .collect()
}

/// The file name as text, which is how the Mach-O linker reports and
/// compares it.
pub(crate) trait FileName {
    fn name_str(&self) -> &str;
}

impl FileName for MappedFile {
    fn name_str(&self) -> &str {
        self.name.to_str().unwrap_or("<non-UTF-8 path>")
    }
}
