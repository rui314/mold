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
static HAS_WARNING: AtomicBool = AtomicBool::new(false);
static OUTPUT_LOCK: Mutex<()> = Mutex::new(());

/// An I/O error as ld64 words it: "errno=2 (No such file or directory)".
pub fn errno_text(e: &io::Error) -> String {
    let text = e.to_string();
    match e.raw_os_error() {
        Some(n) => {
            let text = text.strip_suffix(&format!(" (os error {n})")).unwrap_or(&text);
            format!("errno={n} ({text})")
        }
        None => text,
    }
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

// Format each message before taking the lock so diagnostics from different
// threads cannot interleave.
fn emit(prefix_mono: &str, prefix_color: &str, msg: fmt::Arguments) {
    let prefix = if COLOR.load(Ordering::Relaxed) { prefix_color } else { prefix_mono };
    let text = format!("{prefix}{msg}\n");
    let _guard = OUTPUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = io::stderr().write_all(text.as_bytes());
}

/// Reports an unrecoverable error and exits, giving the messages held
/// back first.
pub fn fatal(msg: fmt::Arguments) -> ! {
    release_held();
    emit("mold: fatal: ", "mold: \x1b[0;1;31mfatal:\x1b[0m ", msg);
    exit_after_cleanup(1);
}

/// A message held back: a warning, or a notice printed bare.
pub enum Held {
    Warning(String),
    Notice(String),
}

/// The messages ld-prime gives as it reads the options, which wait
/// until the options are known to be read for the target (see
/// cmdline's OptionWarnings), but come out before the error an option
/// runs into.
static HELD: Mutex<Vec<Held>> = Mutex::new(Vec::new());

pub fn hold(msg: Held) {
    HELD.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
}

/// Gives the messages held back.
pub fn release_held() {
    let held = std::mem::take(&mut *HELD.lock().unwrap_or_else(|e| e.into_inner()));
    for msg in held {
        match msg {
            Held::Warning(msg) => warn(format_args!("{msg}")),
            Held::Notice(msg) => notice(format_args!("{msg}")),
        }
    }
}

/// Forgets the messages held back, of options read for another target
/// that are read again.
pub fn drop_held() {
    HELD.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Reports an error.
pub fn error(msg: fmt::Arguments) {
    emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", msg);
    HAS_ERROR.store(true, Ordering::Relaxed);
}

/// Reports an error in the output's layout, or in writing it: a
/// thread-local section it can't place, or a fixup that doesn't fit.
/// ld-prime lays the output out to the end all the same, and prints
/// the layout as it fails the link (see passes::print_final_layout).
pub fn layout_error(msg: fmt::Arguments) {
    emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", msg);
    HAS_LAYOUT_ERROR.store(true, Ordering::Relaxed);
}

/// Whether a layout_error has been reported.
pub fn has_layout_error() -> bool {
    HAS_LAYOUT_ERROR.load(Ordering::Relaxed)
}

/// Reports a warning. -w hides it, but -fatal_warnings still counts it
/// (see check_fatal_warnings).
pub fn warn(msg: fmt::Arguments) {
    HAS_WARNING.store(true, Ordering::Relaxed);
    if !SUPPRESS_WARNINGS.load(Ordering::Relaxed) {
        emit("mold: warning: ", "mold: \x1b[0;1;35mwarning:\x1b[0m ", msg);
    }
}

/// Prints a message with no prefix: ld-prime gives a few notices that
/// are neither warnings nor errors (a renamed option).
pub fn notice(msg: fmt::Arguments) {
    emit("", "", msg);
}

/// Counts a warning that -w hid before it could be given.
pub fn hidden_warning() {
    HAS_WARNING.store(true, Ordering::Relaxed);
}

/// Fails a link that has given a warning, shown or hidden by -w, under
/// -fatal_warnings. ld-prime links to the end first, reporting each
/// warning as a warning, then fails with the output in place; this is
/// called once the output is written.
pub fn check_fatal_warnings() {
    if FATAL_WARNINGS.load(Ordering::Relaxed) && HAS_WARNING.load(Ordering::Relaxed) {
        error(format_args!("fatal warning(s) induced error (-fatal_warnings)"));
        exit_after_cleanup(1);
    }
}

/// Exits with a failure status if any error has been reported.
pub fn checkpoint() {
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
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::error::warn(format_args!($($arg)*))
    };
}
