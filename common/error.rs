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
static NOINHIBIT_EXEC: AtomicBool = AtomicBool::new(false);
static HAS_ERROR: AtomicBool = AtomicBool::new(false);
static OUTPUT_LOCK: Mutex<()> = Mutex::new(());

pub fn set_color(on: bool) {
    COLOR.store(on, Ordering::Relaxed);
}

pub fn set_fatal_warnings(on: bool) {
    FATAL_WARNINGS.store(on, Ordering::Relaxed);
}

pub fn set_suppress_warnings(on: bool) {
    SUPPRESS_WARNINGS.store(on, Ordering::Relaxed);
}

pub fn set_noinhibit_exec(on: bool) {
    NOINHIBIT_EXEC.store(on, Ordering::Relaxed);
}

pub fn noinhibit_exec() -> bool {
    NOINHIBIT_EXEC.load(Ordering::Relaxed)
}

// Format each message before taking the lock so diagnostics from different
// threads cannot interleave.
fn emit(prefix_mono: &str, prefix_color: &str, msg: fmt::Arguments) {
    let prefix = if COLOR.load(Ordering::Relaxed) { prefix_color } else { prefix_mono };
    let text = format!("{prefix}{msg}\n");
    let _guard = OUTPUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = io::stderr().write_all(text.as_bytes());
}

/// Reports an unrecoverable error and exits.
pub fn fatal(msg: fmt::Arguments) -> ! {
    emit("mold: fatal: ", "mold: \x1b[0;1;31mfatal:\x1b[0m ", msg);
    exit_after_cleanup(1);
}

/// Reports an error. With `--noinhibit-exec` it is downgraded to a warning.
pub fn error(msg: fmt::Arguments) {
    if NOINHIBIT_EXEC.load(Ordering::Relaxed) {
        emit("mold: warning: ", "mold: \x1b[0;1;35mwarning:\x1b[0m ", msg);
    } else {
        emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", msg);
        HAS_ERROR.store(true, Ordering::Relaxed);
    }
}

/// Reports a warning, or holds it back (see hold_warnings). With
/// `--fatal-warnings` it is promoted to an error.
pub fn warn(msg: fmt::Arguments) {
    if let Some(held) = HELD.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        held.push(msg.to_string());
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

/// The warnings held back while the options are read (Some while they
/// are), until the parse is known to be the one that counts: the Mach-O
/// linker parses once per target it guesses, and gives a warning once.
static HELD: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Holds back the warnings from here on (see HELD).
pub fn hold_warnings() {
    *HELD.lock().unwrap_or_else(|e| e.into_inner()) = Some(Vec::new());
}

/// Gives the warnings held back, and holds back no more.
pub fn release_held() {
    let held = HELD.lock().unwrap_or_else(|e| e.into_inner()).take();
    for msg in held.into_iter().flatten() {
        warn(format_args!("{msg}"));
    }
}

/// Forgets the warnings held back, of options read for another target
/// that are read again, and holds back no more.
pub fn drop_held() {
    *HELD.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Prints a message with no prefix to stderr: some reports of what the
/// link did (the Mach-O linker's -why_live, -why_load and text
/// relocations) come in lines of their own.
pub fn notice(msg: fmt::Arguments) {
    emit("", "", msg);
}

/// Prints an informational message to stdout.
pub fn out(msg: fmt::Arguments) {
    let text = format!("{msg}\n");
    let _guard = OUTPUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = io::stdout().write_all(text.as_bytes());
}

/// Returns the text that C's strerror gives for an I/O error. Unlike
/// `io::Error`'s `Display`, it doesn't append " (os error N)". An error that
/// doesn't come from the OS is formatted as usual.
pub fn strerror(err: &io::Error) -> String {
    #[cfg(not(windows))]
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

/// Exits with a failure status if any error has been reported.
pub fn checkpoint() {
    if HAS_ERROR.load(Ordering::Relaxed) {
        exit_after_cleanup(1);
    }
}

/// Removes a partially-written output file, then terminates the process
/// without running destructors. Input files are mapped for the process's
/// lifetime, so there is nothing else to release.
pub fn exit_after_cleanup(status: i32) -> ! {
    crate::output_file::cleanup();
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    // SAFETY: `_exit` only terminates the process.
    #[cfg(not(windows))]
    unsafe {
        libc::_exit(status)
    }
    #[cfg(windows)]
    std::process::exit(status)
}

/// Makes a panic remove a partially-written output file, as a fatal error
/// does, before the default hook reports the panic. Nothing recovers from
/// a panic, so it always terminates the process.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::output_file::cleanup();
        default_hook(info);
    }));
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
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::error::warn(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! notice {
    ($($arg:tt)*) => {
        $crate::error::notice(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::error::out(format_args!($($arg)*))
    };
}
