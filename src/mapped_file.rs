//! Input file access.
//!
//! Every input file is read once and kept in memory for the whole link,
//! because sections, symbol names and relocation tables of object files
//! are used until the output is written. Files are therefore leaked with a
//! `'static` lifetime rather than tracked with reference counts, which
//! keeps lifetimes out of every data structure that refers to file
//! contents.

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(windows))]
use rayon::prelude::*;

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

// MappedFile represents an input file that is either mmap'ed or read into
// memory. Either way, its contents are accessible through `data`.
#[derive(Debug)]
pub struct MappedFile {
    pub name: PathBuf,
    /// A byte range whose allocation is deliberately leaked with the input file.
    /// Keeping a raw slice pointer lets the linker modify private relocation
    /// records in place, as C++ mold does.
    pub(crate) data: NonNull<[u8]>,

    /// False if the file was found by searching library paths (`-l`),
    /// which affects how a shared library's soname defaults.
    pub given_fullpath: bool,

    /// The archive this file is a member of.
    pub parent: Option<&'static Self>,

    /// The thin archive this file is a member of.
    pub thin_parent: Option<&'static Self>,

    // For --dependency-file
    pub is_dependency: AtomicBool,
}

// The bytes are normally read concurrently. Mutable access is restricted to
// disjoint relocation ranges owned by file-parallel passes.
unsafe impl Send for MappedFile {}
unsafe impl Sync for MappedFile {}

impl MappedFile {
    fn open_impl(path: &Path) -> io::Result<&'static Self> {
        let file = File::open(path)?;

        let display = path.display();
        let metadata = file.metadata().unwrap_or_else(|e| fatal!("{display}: fstat failed: {e}"));
        let size = metadata.len();

        // True if `data` is a memory mapping of the file rather than a copy of
        // its contents in anonymous memory. See open_file_impl().
        let mut is_mmapped = false;
        let data = if size == 0 {
            NonNull::slice_from_raw_parts(NonNull::dangling(), 0)
        } else if size <= READ_THRESHOLD && metadata.is_file() {
            let mut buf = Vec::with_capacity(size as usize);
            (&file)
                .take(size)
                .read_to_end(&mut buf)
                .unwrap_or_else(|e| fatal!("{display}: read failed: {e}"));
            if buf.len() as u64 != size {
                fatal!("{display}: file is shorter than its reported size");
            }
            NonNull::from(Vec::leak(buf))
        } else {
            // C++ mold maps inputs MAP_PRIVATE with write permission so it
            // can redirect ordinary relocation records in place without
            // changing the input file.
            //
            // SAFETY: the mapping is private and lives for the rest of the
            // process. Input files are not expected to change while the
            // linker runs.
            let map_len = usize::try_from(size)
                .unwrap_or_else(|_| fatal!("{display}: file is too large to map"));
            let map = unsafe { memmap2::MmapOptions::new().len(map_len).map_copy(&file) }
                .unwrap_or_else(|e| fatal!("{display}: mmap failed: {e}"));
            is_mmapped = true;
            NonNull::from(Box::leak(Box::new(map)).as_mut())
        };

        let mf = util::leak(Self {
            name: path.to_path_buf(),
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
        Ok(mf)
    }

    /// Opens a file, returning `None` if it doesn't exist.
    pub fn open(path: impl AsRef<Path>) -> Option<&'static Self> {
        let path = path.as_ref();
        match Self::open_impl(path) {
            Ok(mf) => Some(mf),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => fatal!("opening {} failed: {e}", path.display()),
        }
    }

    /// Opens a file that must exist.
    pub fn must_open(path: impl AsRef<Path>) -> &'static Self {
        let path = path.as_ref();
        Self::open_impl(path).unwrap_or_else(|e| fatal!("cannot open {}: {e}", path.display()))
    }

    /// Returns a view of a member of this archive.
    pub fn slice(&'static self, name: PathBuf, start: usize, size: usize) -> &'static Self {
        assert!(start <= self.size() && size <= self.size() - start);
        // SAFETY: the checked range lies in the same allocation.
        let data = unsafe { self.data.cast::<u8>().add(start) };
        let mf = util::leak(Self {
            name,
            data: NonNull::slice_from_raw_parts(data, size),
            given_fullpath: true,
            parent: Some(self),
            thin_parent: None,
            is_dependency: AtomicBool::new(true),
        });
        FILE_POOL.lock().unwrap().push(mf);
        mf
    }

    /// Opens a member whose bytes are stored outside this thin archive.
    pub fn open_thin_member(&'static self, chroot: &Path, path: &Path) -> &'static Self {
        let member = must_open_file(chroot, path);
        util::leak(Self {
            name: member.name.clone(),
            data: member.data,
            given_fullpath: true,
            parent: None,
            thin_parent: Some(self),
            is_dependency: AtomicBool::new(true),
        })
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    #[inline]
    pub fn data(&self) -> &[u8] {
        // SAFETY: the allocation is leaked and the range was checked when
        // this file or archive member was created. Mutable accesses are to
        // relocation ranges during exclusive linker phases.
        unsafe { self.data.as_ref() }
    }

    /// Returns a mutable raw pointer to a relocation range.
    ///
    /// # Safety
    ///
    /// The caller must have exclusive access to `range` while dereferencing
    /// the pointer, and no shared reference to an overlapping range may be
    /// used during that time.
    pub(crate) unsafe fn data_mut_ptr(&self, range: Range<usize>) -> *mut [u8] {
        assert!(range.start <= range.end && range.end <= self.size());
        // SAFETY: the range is in bounds. Dereferencing remains the caller's
        // responsibility under the contract above.
        std::ptr::slice_from_raw_parts_mut(
            unsafe { self.data.cast::<u8>().as_ptr().add(range.start) },
            range.end - range.start,
        )
    }

    /// The offset of this slice within its top-level archive file.
    pub fn offset(&self) -> usize {
        match self.parent {
            Some(parent) => {
                let base = parent.data.cast::<u8>().as_ptr() as usize;
                self.data.cast::<u8>().as_ptr() as usize - base + parent.offset()
            }
            None => 0,
        }
    }

    // Returns a string that uniquely identify a file that is possibly
    // in an archive.
    pub fn identifier(&self) -> std::ffi::OsString {
        if let Some(parent) = self.parent {
            // Archive members may have the same name, so use the file offset.
            let mut name = parent.name.as_os_str().to_os_string();
            name.push(format!(":{}", self.offset()));
            return name;
        }
        if let Some(parent) = self.thin_parent {
            // Thin archive members have unique filenames.
            let mut name = parent.name.as_os_str().to_os_string();
            name.push(":");
            name.push(&self.name);
            return name;
        }
        self.name.as_os_str().to_os_string()
    }

    pub fn is_dependency(&self) -> bool {
        self.is_dependency.load(Ordering::Relaxed)
    }

    pub fn set_dependency(&self, value: bool) {
        self.is_dependency.store(value, Ordering::Relaxed);
    }
}

pub fn apply_chroot<'a>(chroot: &Path, path: &'a Path) -> Cow<'a, Path> {
    if path.is_absolute() && !chroot.as_os_str().is_empty() {
        Cow::Owned(chroot.join(util::clean_path(path).strip_prefix("/").unwrap_or(path)))
    } else {
        Cow::Borrowed(path)
    }
}

/// Opens an input file, applying `--chroot` to absolute paths.
pub fn open_file(chroot: &Path, path: impl AsRef<Path>) -> Option<&'static MappedFile> {
    MappedFile::open(apply_chroot(chroot, path.as_ref()))
}

/// Opens an input file that must exist, applying `--chroot` to absolute paths.
pub fn must_open_file(chroot: &Path, path: impl AsRef<Path>) -> &'static MappedFile {
    let path = path.as_ref();
    MappedFile::open_impl(&apply_chroot(chroot, path))
        .unwrap_or_else(|e| fatal!("cannot open {}: {e}", path.display()))
}
