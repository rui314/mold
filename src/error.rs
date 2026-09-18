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

/// Reports a warning. With `--fatal-warnings` it is promoted to an error.
pub fn warn(msg: fmt::Arguments) {
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

/// Prints an informational message to stdout.
pub fn out(msg: fmt::Arguments) {
    let text = format!("{msg}\n");
    let _guard = OUTPUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = io::stdout().write_all(text.as_bytes());
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
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::error::out(format_args!($($arg)*))
    };
}
