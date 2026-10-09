//! Output file writing.
//!
//! Unlike mold for ELF, the output is built in an anonymous buffer and
//! written with pwrite, not through a shared mapping of the file. That
//! is deliberate: on macOS, a vnode that has ever had a writable shared
//! mapping fails ad-hoc code-signature validation at exec time - the
//! binary is killed with SIGKILL even though codesign reports it valid
//! on disk, msync changes nothing, and rename()ing a mapped-written temp
//! file over the destination does not help because the taint follows
//! the vnode. clonefile() of the temp file does yield a fresh vnode
//! that executes, but every way of making mapped-written pages visible
//! to it - clonefile, msync, munmap - is a synchronous per-page push
//! about half as fast as pwrite's cluster write, which has reached the
//! drive by the time it returns (a following fsync is ~2ms for 195MB).
//! LLVM's FileOutputBuffer falls back to in-memory buffers for
//! executables on Darwin for the same reason.
//!
//! What can be had instead is overlap. The write is bound by the
//! kernel's page-cache and drive path, not by our CPU work, so a byte
//! range is handed to writer threads as soon as nothing will modify it
//! again, while the symbol table, the page hashes and the signature are
//! still being produced on the other cores; finish() waits for the last
//! block.

use std::fs::{File, OpenOptions};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use mold_common::output_file::set_tmpfile;

use mold_common::error::strerror;
use mold_common::fatal;

/// Opens the output file, returning it and whether this link created
/// it. An existing file is removed first, so that a fresh file takes
/// `mode` (less the umask: 0777 for an image, 0644 for an object), but
/// a character device (/dev/null), which is written in place, as is a
/// file that can't be removed (in a directory the link may not write),
/// which keeps its mode.
fn open(path: &Path, mode: u32) -> (File, bool) {
    if !is_char_device(path) {
        let _ = std::fs::remove_file(path);
    }
    let created = std::fs::symlink_metadata(path).is_err();
    let file = open_options(mode)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap_or_else(|e| fatal!("cannot open {}: {}", path.display(), strerror(&e)));
    (file, created)
}

#[cfg(not(windows))]
fn is_char_device(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path).is_ok_and(|m| m.file_type().is_char_device())
}

#[cfg(windows)]
fn is_char_device(_path: &Path) -> bool {
    false
}

#[cfg(not(windows))]
fn open_options(mode: u32) -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.mode(mode);
    options
}

#[cfg(windows)]
fn open_options(_mode: u32) -> OpenOptions {
    OpenOptions::new()
}

#[cfg(not(windows))]
fn write_all_at(file: &File, buf: &[u8], off: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, off)
}

/// Windows has no positioned write that doesn't move the file's cursor,
/// but no write here uses the cursor.
#[cfg(windows)]
fn write_all_at(file: &File, mut buf: &[u8], mut off: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = file.seek_write(buf, off)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

/// Returns the length of the buffer that holds a file of the given size.
/// No Rust object can be larger than isize::MAX bytes, which limits the
/// output to 2 GiB on 32-bit hosts.
pub fn buffer_len(path: &Path, size: u64) -> usize {
    if isize::try_from(size).is_err() {
        fatal!("{}: output file is too large for this host: {size} bytes", path.display());
    }
    size as usize
}

fn write_error(path: &Path, e: &io::Error) -> ! {
    fatal!("cannot write {}: {}", path.display(), strerror(e))
}

/// The output buffer as the writer threads see it: a bare pointer,
/// because the linking thread keeps its &mut to the buffer while ranges
/// of it are being written. A range is queued only once nothing will
/// write it again, so a writer's reads never overlap a write.
#[derive(Clone, Copy)]
struct SharedBuf {
    ptr: *const u8,
    len: usize,
}

// SAFETY: see the invariant above; the pointee outlives the OutputFile.
unsafe impl Send for SharedBuf {}
unsafe impl Sync for SharedBuf {}

impl SharedBuf {
    /// The queued block at `off..off + n`.
    fn block(&self, off: usize, n: usize) -> &[u8] {
        assert!(off + n <= self.len);
        // SAFETY: within the buffer, which outlives the writers, and
        // not modified once queued.
        unsafe { std::slice::from_raw_parts(self.ptr.add(off), n) }
    }
}

/// Queued ranges are cut into blocks of this size so the writers share
/// them.
const BLOCK: usize = 8 << 20;

/// The page size of the file cache: 16 KiB on Apple silicon, and a
/// multiple of the 4 KiB pages elsewhere. Two pwrites to different bytes of one page must
/// not run at once: when neither covers the whole page, the kernel may
/// zero-fill the part of a newly cached page its write doesn't cover,
/// over the other's bytes. So the writers get only whole pages, and a
/// range's partial first and last pages are written once they are done.
const PAGE: usize = 1 << 14;

/// The writers mostly sit in the kernel, and the copy into the page
/// cache scales only a little with threads, but the exposed tail after
/// the last range is queued shrinks with more of them (debug clang:
/// 18ms with one writer, 5ms with four) and that outweighs the cores
/// they take from the work they overlap: four measured best on the
/// debug clang and nushell links, one a percent behind.
const WRITERS: usize = 4;

/// An output file being written by background threads while the buffer
/// is finished.
pub struct OutputFile {
    path: PathBuf,
    file: Arc<File>,
    buf: SharedBuf,
    len: usize,
    tx: Option<Sender<(usize, usize)>>,
    threads: Vec<JoinHandle<io::Result<()>>>,
    /// The partial pages at queued ranges' ends, written by finish().
    edges: Mutex<Vec<(usize, usize)>>,
}

impl OutputFile {
    /// Creates the output file for the `len`-byte buffer at `buf`, with
    /// permissions `mode` less the umask (see open), and starts the
    /// writers. The buffer must outlive the OutputFile, and a range must
    /// not be modified after it has been queued.
    pub fn open(path: &Path, mode: u32, buf: *const u8, len: usize) -> Self {
        // An existing file is removed first (see open). Overwriting a
        // running executable is an error on some systems, and on macOS
        // the kernel caches code signature state per vnode, so a fresh
        // file avoids stale-signature kills. Only a file this link
        // created is removed again if it fails or crashes.
        let (file, created) = open(path, mode);
        if created {
            set_tmpfile(Some(path));
        }
        if let Err(e) = file.set_len(len as u64) {
            fatal!("cannot set the size of {}: {}", path.display(), strerror(&e));
        }
        let file = Arc::new(file);

        let (tx, rx) = channel::<(usize, usize)>();
        let rx = Arc::new(Mutex::new(rx));
        let shared = SharedBuf { ptr: buf, len };
        let threads = (0..WRITERS)
            .map(|_| {
                let file = Arc::clone(&file);
                let rx = Arc::clone(&rx);
                std::thread::spawn(move || -> io::Result<()> {
                    loop {
                        let job = rx.lock().unwrap().recv();
                        let Ok((off, n)) = job else {
                            return Ok(());
                        };
                        // A method call captures the whole SharedBuf
                        // (a field alone would capture the bare pointer,
                        // which is not Send).
                        write_all_at(&file, shared.block(off, n), off as u64)?;
                    }
                })
            })
            .collect();

        Self {
            path: path.to_path_buf(),
            file,
            buf: shared,
            len,
            tx: Some(tx),
            threads,
            edges: Mutex::new(Vec::new()),
        }
    }

    /// Queues the buffer's bytes at `off..off + len` for writing. They
    /// must be final.
    pub fn queue(&self, off: usize, len: usize) {
        debug_assert!(off + len <= self.len);
        let tx = self.tx.as_ref().unwrap();
        let end = off + len;
        // The whole pages go to the writers; the partial ones at either
        // end wait for finish(), since a neighboring range may still be
        // written into the same pages.
        let lo = off.next_multiple_of(PAGE).min(end);
        let hi = (end / PAGE * PAGE).max(lo);
        let mut edges = self.edges.lock().unwrap();
        if off < lo {
            edges.push((off, lo - off));
        }
        if hi < end {
            edges.push((hi, end - hi));
        }
        drop(edges);
        let mut pos = lo;
        while pos < hi {
            let n = BLOCK.min(hi - pos);
            // A closed channel means a writer failed; finish() reports it.
            let _ = tx.send((pos, n));
            pos += n;
        }
    }

    /// Waits for every queued range to reach the file.
    pub fn close(mut self) {
        drop(self.tx.take());
        for thread in self.threads.drain(..) {
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => write_error(&self.path, &e),
                Err(_) => fatal!("cannot write {}: writer thread panicked", self.path.display()),
            }
        }
        for &(off, n) in self.edges.get_mut().unwrap().iter() {
            if let Err(e) = write_all_at(&self.file, self.buf.block(off, n), off as u64) {
                write_error(&self.path, &e);
            }
        }
        set_tmpfile(None);
    }
}

/// Writes a complete buffer: for output that is built in full before
/// anything can be written (-r, an object, which is not executable).
pub fn write(path: &Path, buf: &[u8]) {
    let out = OutputFile::open(path, 0o644, buf.as_ptr(), buf.len());
    out.queue(0, buf.len());
    out.close();
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
