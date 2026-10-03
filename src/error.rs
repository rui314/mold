//! Error, warning and fatal-error reporting.
//!
//! Errors don't abort the link immediately: the linker keeps going so that
//! all problems are reported in one run, then exits before writing the
//! output. Fatal errors are for conditions the linker can't continue past.

use std::fmt;
use std::io::{self, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether to demangle symbol names in diagnostics. This is process-wide
/// state so that `Display` implementations, which have no access to the
/// linker context, can consult it.
static DEMANGLE: AtomicBool = AtomicBool::new(true);

pub fn set_demangle(enabled: bool) {
    DEMANGLE.store(enabled, Ordering::Relaxed);
}

pub fn demangle_enabled() -> bool {
    DEMANGLE.load(Ordering::Relaxed)
}

// Diagnostic settings, error state and output serialization are process-wide.
static COLOR: AtomicBool = AtomicBool::new(false);
static FATAL_WARNINGS: AtomicBool = AtomicBool::new(false);
static SUPPRESS_WARNINGS: AtomicBool = AtomicBool::new(false);
static HAS_ERROR: AtomicBool = AtomicBool::new(false);
static HAS_LAYOUT_ERROR: AtomicBool = AtomicBool::new(false);
static OUTPUT_LOCK: Mutex<()> = Mutex::new(());

/// Returns the text that C's strerror gives for an I/O error. Unlike
/// `io::Error`'s `Display`, it doesn't append " (os error N)". An error that
/// doesn't come from the OS is formatted as usual.
pub fn strerror(err: &io::Error) -> String {
    if let Some(errno) = err.raw_os_error() {
        let mut buf = [0u8; 256];
        // SAFETY: strerror_r writes at most `buf.len()` bytes to `buf`.
        let ret = unsafe { libc::strerror_r(errno, buf.as_mut_ptr().cast(), buf.len()) };
        if ret == 0
            && let Ok(msg) = std::ffi::CStr::from_bytes_until_nul(&buf)
        {
            return msg.to_string_lossy().into_owned();
        }
    }
    err.to_string()
}

pub fn set_color(on: bool) {
    COLOR.store(on, Ordering::Relaxed);
}

pub fn set_fatal_warnings(on: bool) {
    FATAL_WARNINGS.store(on, Ordering::Relaxed);
}

pub fn set_suppress_warnings(on: bool) {
    SUPPRESS_WARNINGS.store(on, Ordering::Relaxed);
}

/// A byte string that a diagnostic prints as it is (see raw).
pub struct Raw<'a>(&'a [u8]);

/// Prints a byte string in a diagnostic as it is, as ld-prime prints
/// the names it reads - a symbol's, a section's, a file's - which need
/// not be UTF-8, byte for byte. Formatted anywhere but into a
/// diagnostic (or error::render), each byte that isn't UTF-8 comes out
/// as U+FFFD.
pub fn raw(bytes: &[u8]) -> Raw<'_> {
    Raw(bytes)
}

/// An owned byte string a diagnostic prints as it is (see raw): a
/// file's name made up for the diagnostic, say.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawBuf(pub Vec<u8>);

impl From<&str> for RawBuf {
    fn from(s: &str) -> Self {
        Self(s.as_bytes().to_vec())
    }
}

impl fmt::Display for RawBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        raw(&self.0).fmt(f)
    }
}

/// Paths a diagnostic prints as they are, byte for byte (see raw), as
/// ld-prime prints them: path.raw() in place of Path::display, which
/// writes a U+FFFD for each byte that isn't UTF-8.
pub trait RawPath {
    fn raw(&self) -> Raw<'_>;
}

impl RawPath for std::path::Path {
    fn raw(&self) -> Raw<'_> {
        raw(std::os::unix::ffi::OsStrExt::as_bytes(self.as_os_str()))
    }
}

impl RawPath for std::ffi::OsStr {
    fn raw(&self) -> Raw<'_> {
        raw(std::os::unix::ffi::OsStrExt::as_bytes(self))
    }
}

impl fmt::Display for Raw<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for chunk in self.0.utf8_chunks() {
            f.write_str(chunk.valid())?;
            for &byte in chunk.invalid() {
                f.write_str(byte_mark(byte))?;
            }
        }
        Ok(())
    }
}

/// A U+FFFD for each byte from 0x80 up, the bytes that can be invalid
/// in UTF-8: a Raw writes such a byte as the copy for it, which render
/// knows by its address and writes the byte for. (No character could
/// mark the byte in the text: a name may hold any.)
static BYTE_MARKS: [u8; 3 * 128] = {
    let mut marks = [0; 3 * 128];
    let mut i = 0;
    while i < marks.len() {
        (marks[i], marks[i + 1], marks[i + 2]) = (0xef, 0xbf, 0xbd);
        i += 3;
    }
    marks
};

fn byte_mark(byte: u8) -> &'static str {
    let i = usize::from(byte - 0x80) * 3;
    std::str::from_utf8(&BYTE_MARKS[i..i + 3]).unwrap()
}

/// A diagnostic as the bytes to print.
pub type Message = Vec<u8>;

/// Formats a message into the bytes to print, with the bytes a Raw
/// marked put back.
pub(crate) fn render(msg: fmt::Arguments) -> Message {
    struct Sink(Message);
    impl fmt::Write for Sink {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let marks = BYTE_MARKS.as_ptr_range();
            if marks.contains(&s.as_ptr()) {
                let offset = s.as_ptr() as usize - marks.start as usize;
                self.0.push(0x80 + (offset / 3) as u8);
            } else {
                self.0.extend_from_slice(s.as_bytes());
            }
            Ok(())
        }
    }
    let mut sink = Sink(Vec::new());
    let _ = fmt::write(&mut sink, msg);
    sink.0
}

/// The messages given from the worker threads of a parallel pass, which
/// would come out in whatever order the threads happened to run. They
/// wait here and come out sorted, the same in every run, before the
/// next message the link's own thread gives, or as the link ends.
static PARALLEL: Mutex<Vec<Message>> = Mutex::new(Vec::new());

// Format each message before taking the lock so diagnostics from different
// threads cannot interleave.
fn emit(prefix_mono: &str, prefix_color: &str, msg: fmt::Arguments) {
    let prefix = if COLOR.load(Ordering::Relaxed) { prefix_color } else { prefix_mono };
    let text = render(format_args!("{prefix}{msg}\n"));
    if rayon::current_thread_index().is_some() {
        PARALLEL.lock().unwrap_or_else(|e| e.into_inner()).push(text);
        return;
    }
    release_parallel();
    let _guard = OUTPUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = io::stderr().write_all(&text);
}

/// Gives the messages of parallel passes held back so far, sorted.
fn release_parallel() {
    let mut msgs = std::mem::take(&mut *PARALLEL.lock().unwrap_or_else(|e| e.into_inner()));
    if msgs.is_empty() {
        return;
    }
    msgs.sort_unstable();
    let _guard = OUTPUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for text in msgs {
        let _ = io::stderr().write_all(&text);
    }
}

/// Reports an unrecoverable error and exits.
pub fn fatal(msg: fmt::Arguments) -> ! {
    release_layout_error();
    emit("mold: fatal: ", "mold: \x1b[0;1;31mfatal:\x1b[0m ", msg);
    exit_after_cleanup(1);
}

/// The warnings held back while the options are read (Some while they
/// are), until the parse is known to be for the target: the driver
/// parses once per speculated target, and a warning is given once.
static HELD: Mutex<Option<Vec<Message>>> = Mutex::new(None);

/// Holds back the warnings from here on (see HELD).
pub fn hold_warnings() {
    *HELD.lock().unwrap_or_else(|e| e.into_inner()) = Some(Vec::new());
}

/// Gives the warnings held back, and holds back no more.
pub fn release_held() {
    let held = HELD.lock().unwrap_or_else(|e| e.into_inner()).take();
    for msg in held.into_iter().flatten() {
        warn(format_args!("{}", raw(&msg)));
    }
}

/// Forgets the warnings held back, of options read for another target
/// that are read again, and holds back no more.
pub fn drop_held() {
    *HELD.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Reports an error.
pub fn error(msg: fmt::Arguments) {
    emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", msg);
    HAS_ERROR.store(true, Ordering::Relaxed);
}

/// The layout error to give, with where in the output file it is (see
/// layout_error_at), until it is given.
static LAYOUT_ERROR: Mutex<Option<(u64, Message)>> = Mutex::new(None);

/// Reports an error in the output's layout: a thread-local section it
/// can't place, say. ld-prime lays the output out to the end all the
/// same, and prints the layout as it fails the link (see
/// passes::print_final_layout), but gives only the first error it
/// finds: one found later is dropped.
pub fn layout_error(msg: fmt::Arguments) {
    layout_error_at(0, msg);
}

/// Reports an error in writing the output, `fileoff` bytes into the
/// file: a fixup that doesn't fit. Of the errors found as the sections
/// are written, in parallel, ld-prime gives the first in the file,
/// whatever its kind or the order of the inputs - and none of them
/// after an error in the layout (see layout_error).
pub fn layout_error_at(fileoff: u64, msg: fmt::Arguments) {
    let mut first = LAYOUT_ERROR.lock().unwrap_or_else(|e| e.into_inner());
    if first.as_ref().is_none_or(|&(at, _)| fileoff < at) {
        *first = Some((fileoff, render(msg)));
    }
    HAS_LAYOUT_ERROR.store(true, Ordering::Relaxed);
}

/// Gives the layout error reported, unless it has been given.
pub fn release_layout_error() {
    let first = LAYOUT_ERROR.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some((_, msg)) = first {
        emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", format_args!("{}", raw(&msg)));
    }
}

/// Whether a layout_error has been reported.
pub fn has_layout_error() -> bool {
    HAS_LAYOUT_ERROR.load(Ordering::Relaxed)
}

/// Reports a warning, or holds it back (see hold_warnings). With -w it
/// is dropped; with -fatal_warnings it is promoted to an error.
pub fn warn(msg: fmt::Arguments) {
    if let Some(held) = HELD.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        held.push(render(msg));
        return;
    }
    if SUPPRESS_WARNINGS.load(Ordering::Relaxed) {
        return;
    }
    if FATAL_WARNINGS.load(Ordering::Relaxed) {
        emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", msg);
        HAS_ERROR.store(true, Ordering::Relaxed);
    } else {
        emit("mold: warning: ", "mold: \x1b[0;1;35mwarning:\x1b[0m ", msg);
    }
}

/// Prints a message with no prefix: ld-prime reports some of what it
/// did (-why_live, -why_load, the text relocations and the final
/// layout it fails on) in lines of its own.
pub fn notice(msg: fmt::Arguments) {
    emit("", "", msg);
}

/// Exits with a failure status if any error has been reported, giving
/// the messages of the parallel passes before it first.
pub fn checkpoint() {
    release_parallel();
    if HAS_ERROR.load(Ordering::Relaxed) || HAS_LAYOUT_ERROR.load(Ordering::Relaxed) {
        exit_after_cleanup(1);
    }
}

/// Exits with a failure status if an error has been reported that
/// stops the layout: any but a layout_error.
pub fn checkpoint_in_layout() {
    if HAS_ERROR.load(Ordering::Relaxed) {
        exit_after_cleanup(1);
    }
}

/// Removes a partially-written output file, then terminates the process
/// without running destructors. Input files are mapped for the process's
/// lifetime, so there is nothing else to release.
pub fn exit_after_cleanup(status: i32) -> ! {
    release_layout_error();
    release_parallel();
    crate::output_file::cleanup();
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    // SAFETY: `_exit` only terminates the process.
    unsafe { libc::_exit(status) }
}

#[macro_export]
macro_rules! fatal {
    ($($arg:tt)*) => {
        $crate::error::fatal(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::error::error(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! layout_error {
    ($($arg:tt)*) => {
        $crate::error::layout_error(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! layout_error_at {
    ($fileoff:expr, $($arg:tt)*) => {
        $crate::error::layout_error_at($fileoff, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::error::warn(format_args!($($arg)*))
    };
}
