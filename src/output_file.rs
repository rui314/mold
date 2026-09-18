//! The output file and helpers for writing to it from many threads.

// TODO: use an intermediate temporary file for output on Windows.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::ops::Range;
#[cfg(not(windows))]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(not(windows))]
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
#[cfg(windows)]
use std::sync::Mutex;

use memmap2::MmapMut;
#[cfg(not(windows))]
use memmap2::MmapOptions;

use crate::fatal;

/// The temporary file being written, removed on a fatal error.
#[cfg(windows)]
static TMPFILE: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(not(windows))]
static TMPFILE: AtomicPtr<libc::c_char> = AtomicPtr::new(std::ptr::null_mut());

fn set_tmpfile(path: Option<&Path>) {
    #[cfg(not(windows))]
    {
        // Published paths live until process exit: a signal on another thread
        // may still be using the old pointer when this registration changes.
        let ptr = path.map_or(std::ptr::null_mut(), |path| {
            std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                .expect("temporary path contains NUL")
                .into_raw()
        });
        TMPFILE.store(ptr, Ordering::Release);
    }
    #[cfg(windows)]
    {
        *TMPFILE.lock().unwrap() = path.map(Path::to_path_buf);
    }
}

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
    #[cfg(not(windows))]
    {
        let path = TMPFILE.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !path.is_null() {
            // SAFETY: path is a published, NUL-terminated string that is never
            // freed. This path is also called from a signal handler, so it must
            // not lock, allocate or drop owned storage. unlink is signal-safe.
            unsafe { libc::unlink(path) };
        }
    }
    #[cfg(windows)]
    if let Ok(mut guard) = TMPFILE.lock() {
        if let Some(path) = guard.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

enum Storage {
    File {
        file: File,
        // Empty files and a locked file awaiting resize have no mapping.
        // A mapping may reserve more address space than the file's `len`.
        map: Option<MmapMut>,
        len: usize,
    },
    Memory(Vec<u8>),
}

/// An output file, either memory-mapped or buffered in memory.
pub struct OutputFile {
    path: PathBuf,
    tmp_path: Option<PathBuf>,
    storage: Storage,
    perm: u32,
}

/// Calling fallocate speeds up later linking passes on ext4 by taking disk
/// block allocation out of the page-fault handler. On tmpfs, it instead
/// makes things much slower: the kernel allocates and zeroes every page of
/// the range inside the syscall, on one thread, which takes ~1.8 s for a
/// 5 GiB file.
#[cfg(any(target_os = "android", target_os = "linux"))]
fn preallocate(file: &File, offset: u64, size: u64) {
    // SAFETY: fstatfs and fallocate only inspect and act on a valid open
    // descriptor; the statfs buffer is fully written before it is read.
    unsafe {
        let mut fs: libc::statfs = std::mem::zeroed();
        if libc::fstatfs(file.as_raw_fd(), &mut fs) != 0 || fs.f_type != libc::TMPFS_MAGIC as _ {
            libc::fallocate(
                file.as_raw_fd(),
                0,
                offset as libc::off_t,
                size as libc::off_t,
            );
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn preallocate(_file: &File, _offset: u64, _size: u64) {}

fn map_file(file: &File, size: u64) -> io::Result<Option<MmapMut>> {
    if size == 0 {
        return Ok(None);
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
    let mut map = map?;
    // Enable transparent huge pages for an output memory-mapped file
    // when the target provides the required advice.
    // Linking a Chromium debug build is ~20% faster with this madvise call.
    //
    // Without this, every 4 KiB page of the output takes its own
    // page fault when it is first written, and the faults of the
    // many copying threads serialize on the file's page cache. With
    // it, the kernel backs the mapping with large folios and the
    // number of faults drops by an order of magnitude.
    // SAFETY: the range is the mapping; the advice is only a hint.
    unsafe { crate::util::madvise_hugepage(map.as_mut_ptr(), map.len()) };
    Ok(Some(map))
}

impl OutputFile {
    #[cfg(not(windows))]
    fn publish_output_buffer(&self) {
        match &self.storage {
            Storage::File {
                map: Some(map),
                len,
                ..
            } => {
                set_output_buffer_range(map.as_ptr() as usize, *len);
            }
            _ => set_output_buffer_range(0, 0),
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
        args: &crate::cmdline::Args,
        size: u64,
        perm: u32,
        overwrite_in_place: bool,
    ) -> OutputFile {
        let path = crate::mapped_file::apply_chroot(&args.chroot, &args.output);
        let mut output = Self::open_impl(&path, size, perm, overwrite_in_place);
        if let Some(filler) = args.filler {
            output.buf().fill(filler);
        }
        output
    }

    fn open_impl(path: &Path, size: u64, perm: u32, overwrite_in_place: bool) -> OutputFile {
        let is_special =
            path == Path::new("-") || std::fs::metadata(path).is_ok_and(|m| !m.is_file());
        if is_special {
            return OutputFile {
                path: path.to_path_buf(),
                tmp_path: None,
                storage: Storage::Memory(vec![0; size as usize]),
                perm,
            };
        }

        let dir = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut name = std::ffi::OsString::from(".");
        name.push(path.file_name().unwrap_or_default());
        name.push(format!(".{}", std::process::id()));
        let tmp = dir.join(name);

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
                .unwrap_or_else(|e| fatal!("cannot open {}: {e}", tmp.display()))
        });
        set_tmpfile(Some(&tmp));

        set_permissions(&file, perm)
            .unwrap_or_else(|e| fatal!("{}: fchmod failed: {e}", tmp.display()));
        file.set_len(size)
            .unwrap_or_else(|e| fatal!("{}: ftruncate failed: {e}", tmp.display()));
        preallocate(&file, 0, size);

        let map = map_file(&file, size)
            .unwrap_or_else(|e| fatal!("{}: mmap failed: {e}", path.display()));
        let output = OutputFile {
            path: path.to_path_buf(),
            tmp_path: Some(tmp),
            storage: Storage::File {
                file,
                map,
                len: size as usize,
            },
            perm,
        };
        #[cfg(not(windows))]
        output.publish_output_buffer();
        output
    }

    /// Opens a file that is written in place rather than renamed and holds an
    /// exclusive `flock`. This is used for a separate debug file that a
    /// debugger may wait on. The file is made unusable right away so that a
    /// stale one is not picked up by accident; [`Self::resize`] gives it its
    /// size.
    #[cfg(not(windows))]
    pub fn open_locked(path: &Path, perm: u32) -> OutputFile {
        let mut file = open_options(perm)
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .unwrap_or_else(|e| fatal!("cannot open {}: {e}", path.display()));
        // SAFETY: flock on a valid descriptor.
        unsafe {
            libc::flock(file.as_raw_fd(), libc::LOCK_EX);
        }
        // We may be overwriting to an existing debug info file. We want to
        // make the file unusable so that gdb won't use it by accident until
        // it's ready.
        file.write_all(&[0; 256])
            .unwrap_or_else(|e| fatal!("{}: write failed: {e}", path.display()));
        OutputFile {
            path: path.to_path_buf(),
            tmp_path: None,
            storage: Storage::File {
                file,
                map: None,
                len: 0,
            },
            perm,
        }
    }

    #[cfg(windows)]
    pub fn open_locked(_path: &Path, _perm: u32) -> OutputFile {
        fatal!("LockingOutputFile is not supported on Windows");
    }

    /// Sets the size of a file opened with [`Self::open_locked`].
    pub fn resize(&mut self, size: u64) {
        let Storage::File { file, map, len } = &mut self.storage else {
            panic!("resizing an output file that isn't a file");
        };
        file.set_len(size)
            .unwrap_or_else(|e| fatal!("{}: ftruncate failed: {e}", self.path.display()));
        *map = map_file(file, size)
            .unwrap_or_else(|e| fatal!("{}: mmap failed: {e}", self.path.display()));
        *len = size as usize;
        #[cfg(not(windows))]
        self.publish_output_buffer();
    }

    /// Whether the buffer is a mapping of the file rather than memory.
    pub fn is_mmapped(&self) -> bool {
        matches!(self.storage, Storage::File { map: Some(_), .. })
    }

    pub fn buf(&mut self) -> &mut [u8] {
        match &mut self.storage {
            Storage::File {
                map: Some(map),
                len,
                ..
            } => &mut map[..*len],
            Storage::File { map: None, .. } => &mut [],
            Storage::Memory(vec) => &mut vec[..],
        }
    }

    pub(crate) fn len(&self) -> usize {
        match &self.storage {
            Storage::File { len, .. } => *len,
            Storage::Memory(vec) => vec.len(),
        }
    }

    // Extend the file so the caller can fill the appended data through the tail
    // of the mapping. This is called at most once per output file, for
    // .gdb_index, whose size is not known until the other sections have been
    // written. The new space is zero-initialized, and the output buffer may
    // move.
    pub fn extend(&mut self, size: usize) {
        let new_len = self.len() + size;
        match &mut self.storage {
            Storage::Memory(vec) => vec.resize(new_len, 0),
            Storage::File { file, map, len } => {
                file.set_len(new_len as u64)
                    .unwrap_or_else(|e| fatal!("{}: ftruncate failed: {e}", self.path.display()));
                // Allocate only the appended range. Reallocating the already
                // written prefix can flush dirty extents on filesystems such
                // as btrfs and serialize a large part of .gdb_index output.
                preallocate(file, *len as u64, size as u64);
                if map.as_ref().is_none_or(|map| new_len > map.len()) {
                    // The appended data does not fit in the existing mapping, so map
                    // the grown file again.
                    *map = map_file(file, new_len as u64)
                        .unwrap_or_else(|e| fatal!("{}: mmap failed: {e}", self.path.display()));
                }
                *len = new_len;
            }
        }
        #[cfg(not(windows))]
        self.publish_output_buffer();
    }

    /// Finishes writing and moves the file into place. The mapping is
    /// released without waiting for the data to reach the disk.
    pub fn close(self) {
        #[cfg(not(windows))]
        set_output_buffer_range(0, 0);
        // Hold the file (and any lock) through publication of the output.
        let _file = match self.storage {
            Storage::File { file, map, .. } => {
                drop(map);
                Some(file)
            }
            Storage::Memory(vec) => {
                if self.path == Path::new("-") {
                    let mut stdout = std::io::stdout().lock();
                    stdout
                        .write_all(&vec)
                        .and_then(|()| stdout.flush())
                        .unwrap_or_else(|e| fatal!("write failed: {e}"));
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
                let mut file = open_options(self.perm)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&self.path)
                    .unwrap_or_else(|e| fatal!("cannot open {}: {e}", self.path.display()));
                file.write_all(&vec)
                    .unwrap_or_else(|e| fatal!("{}: write failed: {e}", self.path.display()));
                None
            }
        };
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
                    "cannot rename {} to {}: {e}",
                    tmp.display(),
                    self.path.display()
                )
            });
            set_tmpfile(None);
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
        rest.split_off_mut(..start - pos).unwrap();
        let slice = rest.split_off_mut(..end - start).unwrap();
        slices.push(slice);

        pos = end;
    }
    slices
}

/// Borrows several disjoint ranges of a buffer mutably at once.
pub fn split_ranges<'a>(buf: &'a mut [u8], ranges: &[Range<u64>]) -> Vec<&'a mut [u8]> {
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_by_key(|&i| ranges[i].start);

    let mut result: Vec<Option<&'a mut [u8]>> = (0..ranges.len()).map(|_| None).collect();
    let mut rest = buf;
    let mut pos = 0u64;
    for i in order {
        let r = &ranges[i];
        assert!(r.start >= pos, "overlapping output ranges");
        rest.split_off_mut(..(r.start - pos) as usize).unwrap();
        let slice = rest.split_off_mut(..(r.end - r.start) as usize).unwrap();
        result[i] = Some(slice);

        pos = r.end;
    }
    result.into_iter().map(|s| s.unwrap()).collect()
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn initializes_output_buffer_and_tracks_its_addresses() {
        set_output_buffer_range(0x1000, 0x100);
        assert!(!output_buffer_contains(0x0fff));
        assert!(output_buffer_contains(0x1000));
        assert!(output_buffer_contains(0x10ff));
        assert!(!output_buffer_contains(0x1100));
        set_output_buffer_range(0, 0);

        // Finished ELF files should have no filler left. Check initialization
        // here so the end-to-end filler test cannot pass by ignoring the flag.
        let path = std::env::temp_dir().join(format!("mold-filler-{}", std::process::id()));
        for output in [PathBuf::from("-"), path.clone()] {
            for filler in [0xfe, 0x00] {
                let args = crate::cmdline::Args {
                    output: output.clone(),
                    filler: Some(filler),
                    ..Default::default()
                };
                let mut file = OutputFile::open(&args, 8192, 0o600, true);
                assert!(file.buf().iter().all(|&byte| byte == filler));
                if output != Path::new("-") {
                    let start = file.buf().as_ptr() as usize;
                    assert!(output_buffer_contains(start));
                    assert!(output_buffer_contains(start + 8191));
                    assert!(!output_buffer_contains(start + 8192));
                    file.close();
                    assert!(!output_buffer_contains(start));
                }
            }
        }
        // Cover empty regular files, remapping on growth, and the locked
        // staging state used by separate debug output. Keep this in the same
        // test because output publication uses process-global state.
        for initial in [0, 8] {
            let mut file = OutputFile::open_impl(&path, initial, 0o600, true);
            file.buf().fill(7);
            file.extend(65536);
            assert!(file.buf()[..initial as usize].iter().all(|&b| b == 7));
            assert!(file.buf()[initial as usize..].iter().all(|&b| b == 0));
            file.close();
            assert_eq!(std::fs::metadata(&path).unwrap().len(), initial + 65536);
        }
        let mut file = OutputFile::open_locked(&path, 0o600);
        assert!(file.buf().is_empty());
        file.resize(0);
        assert!(!file.is_mmapped());
        file.resize(4);
        file.buf().copy_from_slice(b"test");
        file.close();
        assert_eq!(std::fs::read(&path).unwrap(), b"test");
        std::fs::remove_file(path).unwrap();
    }
}
