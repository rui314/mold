//! Input file access.
//!
//! Every input file is read once and kept in memory for the whole link,
//! because sections, symbol names and relocation tables of object files
//! are used until the output is written. Files are therefore leaked with a
//! `'static` lifetime rather than tracked with reference counts, which
//! keeps lifetimes out of every data structure that refers to file
//! contents.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::fatal;

/// Opens are memoized by path: a file named twice (a library on the
/// command line and in an auto-link option, an archive listed
/// repeatedly) gets one mapping, which also lets downstream caches key
/// by data address.
static FILE_CACHE: Mutex<Option<HashMap<PathBuf, &'static MappedFile>>> = Mutex::new(None);

// Files up to this size are read into malloc'ed memory rather than
// mmap'ed. mmap(2) takes the process's address space lock, so with tens
// of thousands of input files, the calls serialize at a few microseconds
// each no matter how many threads make them. read(2) takes no such lock,
// but copying costs memory bandwidth in proportion to the file size, so
// large files are still mmap'ed.
const READ_THRESHOLD: u64 = 32 * 1024;

// MappedFile represents an input file that is either mmap'ed or read into
// memory. Either way, its contents are accessible through `data()`.
#[derive(Debug)]
pub struct MappedFile {
    pub name: PathBuf,
    /// The bytes, deliberately leaked with the input file.
    pub(crate) data: &'static [u8],
    /// The archive this file is a member of.
    pub parent: Option<&'static Self>,
    /// The modification time to note for the file in debug stabs when
    /// it is not the one of a file on disk: an archive member's, as
    /// its header records it, or 0 for the object LTO compiled in
    /// memory.
    pub mtime: Option<u64>,
    /// An object LTO compiled, which ld-prime names in diagnostics as
    /// it was written (if at all), not by its real path.
    pub is_lto_output: bool,
}

impl MappedFile {
    /// A path that is not a regular file (a framework directory, say)
    /// reads as not found.
    fn open_impl(path: &Path) -> io::Result<&'static Self> {
        if let Some(&mf) = FILE_CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(path) {
            return Ok(mf);
        }
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let display = path.display();
        let size = metadata.len();

        let data: &'static [u8] = if size == 0 {
            &[]
        } else if size <= READ_THRESHOLD {
            let mut buf = Vec::with_capacity(size as usize);
            (&file)
                .take(size)
                .read_to_end(&mut buf)
                .unwrap_or_else(|e| fatal!("{display}: read failed: {e}"));
            if buf.len() as u64 != size {
                fatal!("{display}: file is shorter than its reported size");
            }
            Vec::leak(buf)
        } else {
            // SAFETY: the mapping outlives every reference (it is
            // leaked), and linkers conventionally assume inputs are
            // not modified during the link.
            let map = unsafe { memmap2::Mmap::map(&file) }
                .unwrap_or_else(|e| fatal!("{display}: mmap failed: {e}"));
            Box::leak(Box::new(map))
        };
        let mf: &'static Self = Box::leak(Box::new(Self {
            name: path.to_path_buf(),
            data,
            parent: None,
            mtime: None,
            is_lto_output: false,
        }));
        FILE_CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(path.to_path_buf(), mf);
        Ok(mf)
    }

    /// Opens a file, returning `None` if it doesn't exist. Any other
    /// failure - permission denied, an unmappable file - is reported
    /// with the operating system's own words rather than as "not
    /// found".
    pub fn open(path: impl AsRef<Path>) -> Option<&'static Self> {
        let path = path.as_ref();
        match Self::open_impl(path) {
            Ok(mf) => Some(mf),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => fatal!("cannot open {}: {e}", path.display()),
        }
    }

    /// Opens a file, handing any failure to the caller to word.
    pub fn try_open(path: impl AsRef<Path>) -> io::Result<&'static Self> {
        Self::open_impl(path.as_ref())
    }

    /// Opens a file that must exist.
    pub fn must_open(path: impl AsRef<Path>) -> &'static Self {
        let path = path.as_ref();
        Self::open_impl(path).unwrap_or_else(|e| fatal!("cannot open {}: {e}", path.display()))
    }

    /// Returns a view of a member of this archive (or of a fat file).
    pub fn slice(&'static self, name: PathBuf, start: usize, size: usize) -> &'static Self {
        self.part(name, start, size, None)
    }

    /// An archive member, with the modification time its header records.
    pub fn member(
        &'static self,
        name: PathBuf,
        start: usize,
        size: usize,
        date: u64,
    ) -> &'static Self {
        self.part(name, start, size, Some(date))
    }

    fn part(
        &'static self,
        name: PathBuf,
        start: usize,
        size: usize,
        mtime: Option<u64>,
    ) -> &'static Self {
        assert!(start <= self.size() && size <= self.size() - start);
        Box::leak(Box::new(Self {
            name,
            data: &self.data[start..start + size],
            parent: Some(self),
            mtime,
            is_lto_output: false,
        }))
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    #[inline]
    pub fn data(&self) -> &'static [u8] {
        self.data
    }
}
