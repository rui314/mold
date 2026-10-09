//! Input file access.
//!
//! Every input file is read once and kept in memory for the whole link,
//! because sections, symbol names and relocation tables of object files
//! are used until the output is written. Files are therefore leaked with a
//! `'static` lifetime rather than tracked with reference counts, which
//! keeps lifetimes out of every data structure that refers to file
//! contents.

use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::error;
use crate::error::RawPath;
use crate::fatal;

/// Opens are memoized by path: a file named twice (a library on the
/// command line and in an auto-link option, an archive listed
/// repeatedly) gets one mapping, which also lets downstream caches key
/// by data address.
static FILE_CACHE: Mutex<Option<HashMap<PathBuf, &'static MappedFile>>> = Mutex::new(None);

/// The contents of the files opened so far, by file_id: a file reached
/// by another path shares them, and so do the caches keyed by data
/// address. An SDK framework's stub is reached by two: `-framework
/// Foundation` finds Foundation.framework/Foundation.tbd, a symbolic
/// link to the Versions/C/Foundation.tbd that a re-exported install
/// name finds, and tapi::parse_cached parses the 6 MB file once. A file
/// is read by the first to open it while those opening it by another
/// path wait.
type Contents = HashMap<FileId, &'static OnceLock<&'static [u8]>>;
static CONTENTS: Mutex<Option<Contents>> = Mutex::new(None);

/// A file is known by its device and inode, size and modification time
/// (the inode of a file deleted since may be another's).
#[cfg(not(windows))]
type FileId = (u64, u64, u64, i64, i64);

#[cfg(not(windows))]
fn file_id(_path: &Path, md: &Metadata) -> io::Result<FileId> {
    use std::os::unix::fs::MetadataExt;
    Ok((md.dev(), md.ino(), md.size(), md.mtime(), md.mtime_nsec()))
}

/// std gives no file IDs on Windows, so a file is known by its path with
/// symbolic links resolved.
#[cfg(windows)]
type FileId = PathBuf;

#[cfg(windows)]
fn file_id(path: &Path, _md: &Metadata) -> io::Result<FileId> {
    std::fs::canonicalize(path)
}

// Files up to this size are read into malloc'ed memory rather than
// mmap'ed. mmap(2) takes the process's address space lock, so with tens
// of thousands of input files, the calls serialize at a few microseconds
// each no matter how many threads make them. read(2) takes no such lock,
// but copying costs memory bandwidth in proportion to the file size, so
// large files are still mmap'ed.
const READ_THRESHOLD: u64 = 32 * 1024;

/// The `size` bytes of an open file, read or mapped.
fn read_contents(file: &File, size: u64, path: &Path) -> &'static [u8] {
    let display = path.raw();
    if size == 0 {
        &[]
    } else if size <= READ_THRESHOLD {
        let mut buf = Vec::with_capacity(size as usize);
        file.take(size)
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
        let map = unsafe { memmap2::Mmap::map(file) }
            .unwrap_or_else(|e| fatal!("{display}: mmap failed: {e}"));
        Box::leak(Box::new(map))
    }
}

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
        let id = file_id(path, &metadata)?;
        let cell: &'static OnceLock<&'static [u8]> = CONTENTS
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .entry(id)
            .or_insert_with(|| Box::leak(Box::new(OnceLock::new())));
        let data = *cell.get_or_init(|| read_contents(&file, metadata.len(), path));
        let mf: &'static Self =
            Box::leak(Box::new(Self { name: path.to_path_buf(), data, parent: None, mtime: None }));
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
            Err(e) => fatal!("cannot open {}: {e}", path.raw()),
        }
    }

    /// Opens a file, handing any failure to the caller to word.
    pub fn try_open(path: impl AsRef<Path>) -> io::Result<&'static Self> {
        Self::open_impl(path.as_ref())
    }

    /// Opens a file that must exist.
    pub fn must_open(path: impl AsRef<Path>) -> &'static Self {
        let path = path.as_ref();
        Self::open_impl(path).unwrap_or_else(|e| fatal!("cannot open {}: {e}", path.raw()))
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
        }))
    }

    /// A file the linker made in memory, under the name of the input it
    /// stands for (see mergeable::synthesize_object).
    pub fn synthesized(name: PathBuf, data: Vec<u8>) -> &'static Self {
        let data = Vec::leak(data);
        Box::leak(Box::new(Self { name, data, parent: None, mtime: None }))
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    #[inline]
    pub fn data(&self) -> &'static [u8] {
        self.data
    }
}

/// The words for a file MappedFile::try_open failed on with `e`. Every
/// input is read whole, and an empty one refused, so a file that is
/// there but no regular one (which MappedFile takes for none) is one
/// that can't be mapped - a directory - or an empty one.
pub fn unreadable_file(path: &Path, e: &std::io::Error) -> error::Message {
    let p = path.raw();
    let found = std::fs::metadata(path).ok().filter(|_| e.kind() == std::io::ErrorKind::NotFound);
    match found {
        Some(md) if md.len() == 0 => b"file is empty".to_vec(),
        Some(_) => {
            let e = std::io::Error::from_raw_os_error(libc::EINVAL);
            let errno = crate::error::strerror(&e);
            error::render(format_args!("cannot map {p}: {errno}"))
        }
        None => {
            let errno = crate::error::strerror(e);
            error::render(format_args!("cannot open {p}: {errno}"))
        }
    }
}
