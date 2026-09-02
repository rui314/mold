//! Input file access.
//!
//! Every input file is read once and kept in memory for the whole link,
//! because sections, symbol names and relocation tables of object files
//! are used until the output is written. Files are therefore leaked with a
//! `'static` lifetime rather than tracked with reference counts, which
//! keeps lifetimes out of every data structure that refers to file
//! contents.

use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[cfg(not(windows))]
use rayon::prelude::*;

use crate::error::errno_string;
use crate::fatal;
use crate::util;

/// All files opened during this link.
static FILE_POOL: Mutex<Vec<&'static MappedFile>> = Mutex::new(Vec::new());

/// The files that are memory-mapped, for [`drop_mappings`].
static MMAPPED_FILES: Mutex<Vec<&'static MappedFile>> = Mutex::new(Vec::new());

/// Returns all files opened during this link.
pub fn file_pool() -> Vec<&'static MappedFile> {
    FILE_POOL.lock().unwrap().clone()
}

/// Drops the page table entries of the mapped input files, in parallel.
/// This makes process exit faster, as the kernel otherwise reclaims them
/// in a single thread on exit. File contents stay in the page cache.
pub fn drop_mappings() {
    let files = std::mem::take(&mut *MMAPPED_FILES.lock().unwrap());
    #[cfg(windows)]
    let _ = files;
    #[cfg(not(windows))]
    files.par_iter().for_each(|mf| {
        // SAFETY: the range is a whole mapping of the file that lives for
        // the rest of the process; MADV_DONTNEED only discards the pages,
        // which are faulted in again if they are ever read.
        unsafe {
            libc::madvise(
                mf.data().as_ptr() as *mut libc::c_void,
                mf.data().len(),
                libc::MADV_DONTNEED,
            )
        };
    });
}

// Files up to this size are read into malloc'ed memory rather than
// mmap'ed. mmap(2) takes the process's address space lock, so with tens
// of thousands of input files, the calls serialize at a few microseconds
// each no matter how many threads make them. read(2) takes no such lock,
// but copying costs memory bandwidth in proportion to the file size, so
// large files are still mmap'ed. On a Chromium link, files below this
// threshold are 64% of the inputs by count but 16% by size.
//
// The buffers add up to hundreds of megabytes on such a link, so they
// need to be backed by huge pages; with 4 KiB pages, faulting them in
// costs more than the mmap calls did. mimalloc does that by default.
const READ_THRESHOLD: u64 = 32 * 1024;

/// A byte range whose allocation is deliberately leaked with the input file.
/// Keeping the pointer rather than a permanent shared slice lets the linker
/// modify private relocation records in place, as C++ mold does.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MappedBytes {
    ptr: NonNull<u8>,
    len: usize,
}

impl MappedBytes {
    fn empty() -> MappedBytes {
        MappedBytes {
            ptr: NonNull::dangling(),
            len: 0,
        }
    }

    fn from_mut(data: &mut [u8]) -> MappedBytes {
        MappedBytes {
            ptr: NonNull::new(data.as_mut_ptr()).unwrap_or_else(NonNull::dangling),
            len: data.len(),
        }
    }

    fn slice(self, start: usize, size: usize) -> MappedBytes {
        assert!(start <= self.len && size <= self.len - start);
        MappedBytes {
            // SAFETY: the checked range lies in the same allocation.
            ptr: unsafe { NonNull::new_unchecked(self.ptr.as_ptr().add(start)) },
            len: size,
        }
    }
}

// MappedFile represents an input file that is either mmap'ed or read into
// memory. Either way, its contents are accessible through `data`.
#[derive(Debug)]
pub struct MappedFile {
    pub name: String,
    pub(crate) data: MappedBytes,

    /// False if the file was found by searching library paths (`-l`),
    /// which affects how a shared library's soname defaults.
    pub given_fullpath: bool,

    /// The archive this file is a member of.
    pub parent: Option<&'static MappedFile>,

    /// The thin archive this file is a member of.
    pub thin_parent: Option<&'static MappedFile>,

    // For --dependency-file
    pub is_dependency: AtomicBool,
}

// The bytes are normally read concurrently. Mutable access is restricted to
// disjoint relocation ranges owned by file-parallel passes.
unsafe impl Send for MappedFile {}
unsafe impl Sync for MappedFile {}

impl MappedFile {
    /// Opens a file, returning `None` if it doesn't exist.
    pub fn open(path: &str) -> Option<&'static MappedFile> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => fatal!("opening {path} failed: {e}"),
        };

        let metadata = file
            .metadata()
            .unwrap_or_else(|e| fatal!("{path}: fstat failed: {e}"));
        let size = metadata.len();

        // True if `data` is a memory mapping of the file rather than a copy of
        // its contents in anonymous memory. See open_file_impl().
        let mut is_mmapped = false;
        let data = if size == 0 {
            MappedBytes::empty()
        } else if size <= READ_THRESHOLD && metadata.is_file() {
            let mut buf = Vec::with_capacity(size as usize);
            (&file)
                .take(size)
                .read_to_end(&mut buf)
                .unwrap_or_else(|e| fatal!("{path}: read failed: {e}"));
            if buf.len() as u64 != size {
                fatal!("{path}: file is shorter than its reported size");
            }
            MappedBytes::from_mut(Vec::leak(buf))
        } else {
            // C++ mold maps inputs MAP_PRIVATE with write permission so it
            // can redirect ordinary relocation records in place without
            // changing the input file.
            //
            // SAFETY: the mapping is private and lives for the rest of the
            // process. Input files are not expected to change while the
            // linker runs.
            let map_len = usize::try_from(size)
                .unwrap_or_else(|_| fatal!("{path}: file is too large to map"));
            let map = unsafe { memmap2::MmapOptions::new().len(map_len).map_copy(&file) }
                .unwrap_or_else(|e| fatal!("{path}: mmap failed: {e}"));
            is_mmapped = true;
            MappedBytes::from_mut(Box::leak(Box::new(map)).as_mut())
        };

        let mf = util::leak(MappedFile {
            name: path.to_string(),
            data,
            given_fullpath: true,
            parent: None,
            thin_parent: None,
            is_dependency: AtomicBool::new(true),
        });
        FILE_POOL.lock().unwrap().push(mf);
        if is_mmapped {
            MMAPPED_FILES.lock().unwrap().push(mf);
        }
        Some(mf)
    }

    /// Opens a file that must exist.
    pub fn must_open(path: &str) -> &'static MappedFile {
        MappedFile::open(path).unwrap_or_else(|| fatal!("cannot open {path}: {}", errno_string()))
    }

    /// Returns a view of a member of this archive.
    pub fn slice(&'static self, name: String, start: usize, size: usize) -> &'static MappedFile {
        let mf = util::leak(MappedFile {
            name,
            data: self.data.slice(start, size),
            given_fullpath: true,
            parent: Some(self),
            thin_parent: None,
            is_dependency: AtomicBool::new(true),
        });
        FILE_POOL.lock().unwrap().push(mf);
        mf
    }

    pub fn size(&self) -> usize {
        self.data.len
    }

    #[inline]
    pub fn data(&self) -> &[u8] {
        // SAFETY: the allocation is leaked and the range was checked when
        // this file or archive member was created. Mutable accesses are to
        // relocation ranges during exclusive linker phases.
        unsafe { std::slice::from_raw_parts(self.data.ptr.as_ptr(), self.data.len) }
    }

    /// Returns a mutable raw pointer to a relocation range.
    ///
    /// # Safety
    ///
    /// The caller must have exclusive access to `range` while dereferencing
    /// the pointer, and no shared reference to an overlapping range may be
    /// used during that time.
    pub(crate) unsafe fn data_mut_ptr(&self, range: Range<usize>) -> *mut [u8] {
        assert!(
            range.start <= range.end && range.start <= self.data.len && range.end <= self.data.len
        );
        // SAFETY: the range is in bounds. Dereferencing remains the caller's
        // responsibility under the contract above.
        std::ptr::slice_from_raw_parts_mut(
            unsafe { self.data.ptr.as_ptr().add(range.start) },
            range.end - range.start,
        )
    }

    /// The offset of this slice within its top-level archive file.
    pub fn offset(&self) -> usize {
        match self.parent {
            Some(parent) => {
                let base = parent.data.ptr.as_ptr() as usize;
                self.data.ptr.as_ptr() as usize - base + parent.offset()
            }
            None => 0,
        }
    }

    // Returns a string that uniquely identify a file that is possibly
    // in an archive.
    pub fn identifier(&self) -> String {
        if let Some(parent) = self.parent {
            // We use the file offset within an archive as an identifier
            // because archive members may have the same name.
            return format!("{}:{}", parent.name, self.offset());
        }
        if let Some(thin_parent) = self.thin_parent {
            // If this is a thin archive member, the filename part is
            // guaranteed to be unique.
            return format!("{}:{}", thin_parent.name, self.name);
        }
        self.name.clone()
    }

    pub fn is_dependency(&self) -> bool {
        self.is_dependency.load(Ordering::Relaxed)
    }

    pub fn set_dependency(&self, value: bool) {
        self.is_dependency.store(value, Ordering::Relaxed);
    }
}

/// Opens an input file, applying `--chroot` to absolute paths.
pub fn open_file(chroot: &str, path: &str) -> Option<&'static MappedFile> {
    if path.starts_with('/') && !chroot.is_empty() {
        let path = format!("{chroot}/{}", util::path_clean(path));
        return MappedFile::open(&path);
    }
    MappedFile::open(path)
}

/// Opens an input file that must exist, applying `--chroot` to absolute paths.
pub fn must_open_file(chroot: &str, path: &str) -> &'static MappedFile {
    open_file(chroot, path).unwrap_or_else(|| fatal!("cannot open {path}: {}", errno_string()))
}

/// Whether a path refers to something that is not a directory.
pub fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}
