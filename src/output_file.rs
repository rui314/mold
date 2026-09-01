//! The output file and helpers for writing to it from many threads.

// The C++ Windows output-file counterpart records:
// TODO: use intermediate temporary file for output.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
#[cfg(not(windows))]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(not(windows))]
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use memmap2::MmapMut;
#[cfg(not(windows))]
use memmap2::MmapOptions;

use crate::error::Diagnostics;
use crate::fatal;

/// The temporary file being written, removed on a fatal error.
static TMPFILE: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(not(windows))]
static OUTPUT_BUFFER_START: AtomicUsize = AtomicUsize::new(0);
#[cfg(not(windows))]
static OUTPUT_BUFFER_END: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(windows))]
fn set_output_buffer_range(start: usize, len: usize) {
    OUTPUT_BUFFER_END.store(0, Ordering::SeqCst);
    OUTPUT_BUFFER_START.store(start, Ordering::SeqCst);
    OUTPUT_BUFFER_END.store(start + len, Ordering::SeqCst);
}

#[cfg(not(windows))]
pub(crate) fn output_buffer_contains(addr: usize) -> bool {
    let start = OUTPUT_BUFFER_START.load(Ordering::SeqCst);
    let end = OUTPUT_BUFFER_END.load(Ordering::SeqCst);
    start != 0 && start <= addr && addr < end
}

#[cfg(not(windows))]
fn open_options(mode: u32) -> OpenOptions {
    let mut options = OpenOptions::new();
    options.mode(mode);
    options
}

#[cfg(windows)]
fn open_options(_mode: u32) -> OpenOptions {
    OpenOptions::new()
}

fn set_permissions(file: &File, perm: u32) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        file.set_permissions(std::fs::Permissions::from_mode(perm & !umask()))
    }

    #[cfg(windows)]
    {
        let mut permissions = file.metadata()?.permissions();
        permissions.set_readonly(perm & 0o200 == 0);
        file.set_permissions(permissions)
    }
}

/// Removes a partially written output file.
pub fn cleanup() {
    if let Ok(mut guard) = TMPFILE.lock() {
        if let Some(path) = guard.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

enum Storage {
    /// A mapping of the file, possibly larger than the file itself; `len`
    /// is the file's size.
    Mmap {
        // Size of the file mapping, which may extend past the end of the file.
        // The C++ locking-output counterpart tracks the same capacity:
        // Size of the file mapping, which may extend past the end of the file.
        map: MmapMut,
        len: usize,
    },
    Memory(Vec<u8>),
}

/// An output file, either memory-mapped or buffered in memory.
pub struct OutputFile {
    path: String,
    tmp_path: Option<PathBuf>,
    /// The file being written, if it is a real file.
    file: Option<File>,
    storage: Storage,
    perm: u32,
}

/// Calling fallocate speeds up later linking passes on ext4 by taking disk
/// block allocation out of the page-fault handler. On tmpfs, it instead
/// makes things much slower: the kernel allocates and zeroes every page of
/// the range inside the syscall, on one thread, which takes ~1.8 s for a
/// 5 GiB file.
#[cfg(any(target_os = "android", target_os = "linux"))]
fn preallocate(file: &File, size: u64) {
    // SAFETY: fstatfs and fallocate only inspect and act on a valid open
    // descriptor; the statfs buffer is fully written before it is read.
    unsafe {
        let mut fs: libc::statfs = std::mem::zeroed();
        if libc::fstatfs(file.as_raw_fd(), &mut fs) != 0 || fs.f_type != libc::TMPFS_MAGIC as _ {
            libc::fallocate(file.as_raw_fd(), 0, 0, size as libc::off_t);
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn preallocate(_file: &File, _size: u64) {}

fn map_file(file: &File, size: u64) -> Storage {
    if size == 0 {
        return Storage::Memory(Vec::new());
    }
    // We map the file with twice as much address space as its size, so
    // that extend() can grow the file into the mapping in place. Touching
    // the mapping beyond the end of the file is not allowed, but growing
    // the file with ftruncate makes the tail of the mapping accessible
    // without any further mmap call.
    //
    // SAFETY: the file is private to this process until it's closed.
    #[cfg(not(windows))]
    let map = unsafe { MmapOptions::new().len(size as usize * 2).map_mut(file) }
        // If the address space is too tight, map just the file.
        .or_else(|_| unsafe { MmapMut::map_mut(file) });
    #[cfg(windows)]
    let map = unsafe { MmapMut::map_mut(file) };
    match map {
        Ok(map) => {
            #[cfg(any(target_os = "android", target_os = "linux"))]
            let mut map = map;
            // Enable transparent huge page for an output memory-mapped file.
            // Linking a Chromium debug build is ~20% faster with this madvise call.
            //
            // Without this, every 4 KiB page of the output takes its own
            // page fault when it is first written, and the faults of the
            // many copying threads serialize on the file's page cache. With
            // it, the kernel backs the mapping with large folios and the
            // number of faults drops by an order of magnitude.
            #[cfg(any(target_os = "android", target_os = "linux"))]
            // SAFETY: the range is the mapping; the advice is only a hint.
            unsafe {
                libc::madvise(
                    map.as_mut_ptr() as *mut libc::c_void,
                    map.len(),
                    libc::MADV_HUGEPAGE,
                )
            };
            Storage::Mmap {
                map,
                len: size as usize,
            }
        }
        Err(_) => Storage::Memory(vec![0; size as usize]),
    }
}

impl OutputFile {
    #[cfg(not(windows))]
    fn publish_output_buffer(&self) {
        match &self.storage {
            Storage::Mmap { map, len } => {
                set_output_buffer_range(map.as_ptr() as usize, *len);
            }
            Storage::Memory(_) => set_output_buffer_range(0, 0),
        }
    }

    /// Opens an output file of the given size.
    ///
    /// A regular file is written through a memory mapping of a temporary
    /// file that is renamed into place on close, so that a failed link
    /// doesn't leave a truncated output behind and a running executable
    /// isn't modified underneath the kernel. Anything else — a device, a
    /// pipe, or standard output — is assembled in memory and written out
    /// at the end.
    pub fn open(
        diag: &Diagnostics,
        path: &str,
        size: u64,
        perm: u32,
        overwrite_in_place: bool,
    ) -> OutputFile {
        let is_special = path == "-" || std::fs::metadata(path).is_ok_and(|m| !m.is_file());
        if is_special {
            return OutputFile {
                path: path.to_string(),
                tmp_path: None,
                file: None,
                storage: Storage::Memory(vec![0; size as usize]),
                perm,
            };
        }

        let dir = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = Path::new(path)
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned());
        let tmp = dir.join(format!(".{name}.{}", std::process::id()));

        // Reuse an existing file if exists and writable because on Linux,
        // writing to an existing file is much faster than creating a fresh
        // file and writing to it.
        let reuse_existing = || -> Option<File> {
            if !overwrite_in_place || std::fs::rename(path, &tmp).is_err() {
                return None;
            }
            match open_options(perm)
                .read(true)
                .write(true)
                .create(true)
                .open(&tmp)
            {
                Ok(file) => Some(file),
                Err(_) => {
                    let _ = std::fs::remove_file(&tmp);
                    None
                }
            }
        };
        let file = reuse_existing().unwrap_or_else(|| {
            open_options(perm)
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)
                .unwrap_or_else(|e| fatal!(diag, "cannot open {}: {e}", tmp.display()))
        });
        *TMPFILE.lock().unwrap() = Some(tmp.clone());

        set_permissions(&file, perm)
            .unwrap_or_else(|e| fatal!(diag, "{}: fchmod failed: {e}", tmp.display()));
        file.set_len(size)
            .unwrap_or_else(|e| fatal!(diag, "{}: ftruncate failed: {e}", tmp.display()));
        preallocate(&file, size);

        let storage = map_file(&file, size);
        let output = OutputFile {
            path: path.to_string(),
            tmp_path: Some(tmp),
            file: Some(file),
            storage,
            perm,
        };
        #[cfg(not(windows))]
        output.publish_output_buffer();
        output
    }

    // LockingOutputFile is similar to MemoryMappedOutputFile, but it doesn't
    // rename output files and instead acquires file lock using flock().
    /// Opens a file that is written in place under an exclusive lock, for
    /// a separate debug file that a debugger may wait on. The file is
    /// made unusable right away so that a stale one isn't picked up by
    /// accident; [`Self::resize`] gives it its size.
    #[cfg(not(windows))]
    pub fn open_locked(diag: &Diagnostics, path: &str, perm: u32) -> OutputFile {
        let mut file = open_options(perm)
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .unwrap_or_else(|e| fatal!(diag, "cannot open {path}: {e}"));
        // SAFETY: flock on a valid descriptor.
        unsafe {
            libc::flock(file.as_raw_fd(), libc::LOCK_EX);
        }
        // We may be overwriting to an existing debug info file. We want to
        // make the file unusable so that gdb won't use it by accident until
        // it's ready.
        file.write_all(&[0; 256])
            .unwrap_or_else(|e| fatal!(diag, "{path}: write failed: {e}"));
        OutputFile {
            path: path.to_string(),
            tmp_path: None,
            file: Some(file),
            storage: Storage::Memory(Vec::new()),
            perm,
        }
    }

    #[cfg(windows)]
    pub fn open_locked(diag: &Diagnostics, _path: &str, _perm: u32) -> OutputFile {
        fatal!(diag, "LockingOutputFile is not supported on Windows");
    }

    /// Sets the size of a file opened with [`Self::open_locked`].
    pub fn resize(&mut self, diag: &Diagnostics, size: u64) {
        let file = self
            .file
            .as_ref()
            .expect("resizing an output file that isn't a file");
        file.set_len(size)
            .unwrap_or_else(|e| fatal!(diag, "{}: ftruncate failed: {e}", self.path));
        // As in MemoryMappedOutputFile, we map the file with twice as much
        // address space as its size so that extend() can grow the file into
        // the mapping in place.
        self.storage = map_file(file, size);
        #[cfg(not(windows))]
        self.publish_output_buffer();
    }

    /// Whether the buffer is a mapping of the file rather than memory.
    pub fn is_mmapped(&self) -> bool {
        matches!(self.storage, Storage::Mmap { .. })
    }

    pub fn buf(&mut self) -> &mut [u8] {
        match &mut self.storage {
            Storage::Mmap { map, len } => &mut map[..*len],
            Storage::Memory(vec) => &mut vec[..],
        }
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            Storage::Mmap { len, .. } => *len,
            Storage::Memory(vec) => vec.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // Extend the file so that the caller can fill the appended data
    // through the tail of the mapping. This is called at most once per
    // output file.
    //
    // C++ OutputFile::extend has this pointer-returning contract; Rust grows
    // the same storage and lets the caller borrow the new tail afterwards.
    //
    // Appends `size` bytes to the output file and returns a pointer to
    // the newly-allocated space, bumping `filesize` accordingly. We use
    // it for .gdb_index, whose size is not known until all other
    // sections have been written. The new space is zero-initialized.
    // `buf` and `ctx.buf` may move as a result of this call.
    pub fn extend(&mut self, diag: &Diagnostics, size: usize) {
        let new_len = self.len() + size;
        match (&mut self.storage, &self.file) {
            (Storage::Memory(vec), _) => vec.resize(new_len, 0),
            (Storage::Mmap { map, len }, Some(file)) => {
                file.set_len(new_len as u64)
                    .unwrap_or_else(|e| fatal!(diag, "{}: ftruncate failed: {e}", self.path));
                preallocate(file, new_len as u64);
                if new_len <= map.len() {
                    *len = new_len;
                } else {
                    // MemoryMappedOutputFile counterpart:
                    // The appended data does not fit in the mapping. Map the grown
                    // file again, moving the buffer.
                    // LockingOutputFile counterpart:
                    // The appended data does not fit in the mapping. Map the
                    // grown file again, moving the buffer.
                    self.storage = map_file(file, new_len as u64);
                }
            }
            (Storage::Mmap { .. }, None) => unreachable!("a mapping always has a file"),
        }
        #[cfg(not(windows))]
        self.publish_output_buffer();
    }

    /// Finishes writing and moves the file into place. The mapping is
    /// released without waiting for the data to reach the disk.
    pub fn close(self, diag: &Diagnostics) {
        #[cfg(not(windows))]
        set_output_buffer_range(0, 0);
        match self.storage {
            Storage::Mmap { map, .. } => drop(map),
            Storage::Memory(vec) => {
                if self.path == "-" {
                    let mut stdout = std::io::stdout().lock();
                    stdout
                        .write_all(&vec)
                        .and_then(|()| stdout.flush())
                        .unwrap_or_else(|e| fatal!(diag, "write failed: {e}"));
                    // Close the descriptor too: the parent process may
                    // already have exited, and a program that then runs
                    // the output would fail with ETXTBSY while this
                    // process still holds it open for writing.
                    // SAFETY: nothing else writes to stdout after this.
                    #[cfg(not(windows))]
                    unsafe {
                        libc::close(libc::STDOUT_FILENO);
                    }
                    return;
                }
                let mut file = match self.file {
                    Some(file) => file,
                    None => open_options(self.perm)
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&self.path)
                        .unwrap_or_else(|e| fatal!(diag, "cannot open {}: {e}", self.path)),
                };
                file.write_all(&vec)
                    .unwrap_or_else(|e| fatal!(diag, "{}: write failed: {e}", self.path));
            }
        }
        if let Some(tmp) = self.tmp_path {
            // If an output file already exists, open a file and then remove it.
            // This is the fastest way to unlink a file, as it does not make the
            // system to immediately release disk blocks occupied by the file.
            // The descriptor is kept until the process exits.
            if let Ok(old) = File::open(&self.path) {
                let _ = std::fs::remove_file(&self.path);
                std::mem::forget(old);
            }
            std::fs::rename(&tmp, &self.path).unwrap_or_else(|e| {
                fatal!(
                    diag,
                    "cannot rename {} to {}: {e}",
                    tmp.display(),
                    self.path
                )
            });
            *TMPFILE.lock().unwrap() = None;
        }
    }
}

#[cfg(not(windows))]
fn umask() -> u32 {
    // SAFETY: umask is thread-safe in the sense that it just swaps a
    // process-wide value; restoring it immediately keeps it unchanged.
    unsafe {
        let mask = libc::umask(0);
        libc::umask(mask);
        mask as u32
    }
}

/// Splits a buffer into consecutive slices at the given ascending
/// offsets. The first slice starts at `offsets[0]`; bytes before it are
/// not returned. The last slice extends to the end of the buffer.
pub fn split_at_offsets<'a, T>(buf: &'a mut [T], offsets: &[u64]) -> Vec<&'a mut [T]> {
    let mut slices = Vec::with_capacity(offsets.len());
    let mut rest = buf;
    let mut pos = 0usize;
    for (i, &start) in offsets.iter().enumerate() {
        let start = start as usize;
        let end = offsets.get(i + 1).map_or(rest.len() + pos, |&e| e as usize);
        debug_assert!(pos <= start && start <= end);
        let (_, tail) = std::mem::take(&mut rest).split_at_mut(start - pos);
        let (slice, tail) = tail.split_at_mut(end - start);
        slices.push(slice);
        rest = tail;
        pos = end;
    }
    slices
}

/// A byte range of the output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub offset: u64,
    pub size: u64,
}

/// Borrows several disjoint ranges of a buffer mutably at once.
pub fn split_ranges<'a>(buf: &'a mut [u8], ranges: &[Range]) -> Vec<&'a mut [u8]> {
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_by_key(|&i| ranges[i].offset);

    let mut result: Vec<Option<&'a mut [u8]>> = (0..ranges.len()).map(|_| None).collect();
    let mut rest = buf;
    let mut pos = 0u64;
    for i in order {
        let r = ranges[i];
        assert!(r.offset >= pos, "overlapping output ranges");
        let (_, tail) = std::mem::take(&mut rest).split_at_mut((r.offset - pos) as usize);
        let (slice, tail) = tail.split_at_mut(r.size as usize);
        result[i] = Some(slice);
        rest = tail;
        pos = r.offset + r.size;
    }
    result.into_iter().map(|s| s.unwrap()).collect()
}

/// Creates a file with the given contents.
pub fn write_file(diag: &Diagnostics, path: &str, contents: &[u8]) {
    let mut file = File::create(path).unwrap_or_else(|e| fatal!(diag, "cannot open {path}: {e}"));
    file.write_all(contents)
        .unwrap_or_else(|e| fatal!(diag, "{path}: write failed: {e}"));
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn recognizes_output_buffer_addresses() {
        set_output_buffer_range(0x1000, 0x100);
        assert!(!output_buffer_contains(0x0fff));
        assert!(output_buffer_contains(0x1000));
        assert!(output_buffer_contains(0x10ff));
        assert!(!output_buffer_contains(0x1100));
        set_output_buffer_range(0, 0);
    }
}
