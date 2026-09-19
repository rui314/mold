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

use std::os::unix::fs::{FileExt, PermissionsExt};
use std::path::Path;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::fatal;

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
    path: String,
    len: usize,
    tx: Option<Sender<(usize, usize)>>,
    threads: Vec<JoinHandle<Result<(), String>>>,
}

impl OutputFile {
    /// Creates the output file for the `len`-byte buffer at `buf` and
    /// starts the writers. The buffer must outlive the OutputFile, and a
    /// range must not be modified after it has been queued.
    pub fn create(path: &str, buf: *const u8, len: usize) -> OutputFile {
        // Remove an existing file first. Overwriting a running
        // executable is an error on some systems, and on macOS the
        // kernel caches code signature state per vnode, so a fresh file
        // avoids stale-signature kills.
        let _ = std::fs::remove_file(path);
        // A fatal error or a crash signal removes the partial output
        // through the linker's shared registry.
        crate::output_file::set_tmpfile(Some(Path::new(path)));

        let file =
            std::fs::File::create(path).unwrap_or_else(|e| fatal!("cannot write {path}: {e}"));
        let _ = file.set_len(len as u64);
        let file = Arc::new(file);

        let (tx, rx) = channel::<(usize, usize)>();
        let rx = Arc::new(Mutex::new(rx));
        let shared = SharedBuf { ptr: buf, len };
        let threads = (0..WRITERS)
            .map(|_| {
                let file = Arc::clone(&file);
                let rx = Arc::clone(&rx);
                std::thread::spawn(move || -> Result<(), String> {
                    loop {
                        let job = rx.lock().unwrap().recv();
                        let Ok((off, n)) = job else {
                            return Ok(());
                        };
                        // A method call captures the whole SharedBuf
                        // (a field alone would capture the bare pointer,
                        // which is not Send).
                        file.write_all_at(shared.block(off, n), off as u64)
                            .map_err(|e| e.to_string())?;
                    }
                })
            })
            .collect();

        OutputFile { path: path.to_string(), len, tx: Some(tx), threads }
    }

    /// Queues the buffer's bytes at `off..off + len` for writing. They
    /// must be final.
    pub fn queue(&self, off: usize, len: usize) {
        debug_assert!(off + len <= self.len);
        let tx = self.tx.as_ref().unwrap();
        let end = off + len;
        let mut pos = off;
        while pos < end {
            let n = BLOCK.min(end - pos);
            // A closed channel means a writer failed; finish() reports it.
            let _ = tx.send((pos, n));
            pos += n;
        }
    }

    /// Waits for every queued range to reach the file and makes it
    /// executable.
    pub fn finish(mut self) {
        drop(self.tx.take());
        for thread in self.threads.drain(..) {
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => fatal!("cannot write {}: {e}", self.path),
                Err(_) => fatal!("cannot write {}: writer thread panicked", self.path),
            }
        }
        if let Err(e) = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o755))
        {
            fatal!("cannot chmod {}: {e}", self.path);
        }
        crate::output_file::set_tmpfile(None);
    }
}

/// Writes a complete buffer: for output that is built in full before
/// anything can be written (-r).
pub fn write(path: &str, buf: &[u8]) {
    let out = OutputFile::create(path, buf.as_ptr(), buf.len());
    out.queue(0, buf.len());
    out.finish();
}
