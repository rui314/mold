//! Error, warning and fatal-error reporting.
//!
//! Errors don't abort the link immediately: the linker keeps going so that
//! all problems are reported in one run, then exits before writing the
//! output. Fatal errors are for conditions the linker can't continue past.

use std::fmt;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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

/// Diagnostic settings and state, shared by all threads.
#[derive(Debug, Default)]
pub struct Diagnostics {
    color: AtomicBool,
    fatal_warnings: AtomicBool,
    suppress_warnings: AtomicBool,
    noinhibit_exec: AtomicBool,
    has_error: AtomicBool,
    lock: Mutex<()>,
}

impl Diagnostics {
    pub fn new(color: bool) -> Self {
        let diag = Diagnostics::default();
        diag.color.store(color, Ordering::Relaxed);
        diag
    }

    pub fn set_color(&self, on: bool) {
        self.color.store(on, Ordering::Relaxed);
    }

    pub fn set_fatal_warnings(&self, on: bool) {
        self.fatal_warnings.store(on, Ordering::Relaxed);
    }

    pub fn set_suppress_warnings(&self, on: bool) {
        self.suppress_warnings.store(on, Ordering::Relaxed);
    }

    pub fn set_noinhibit_exec(&self, on: bool) {
        self.noinhibit_exec.store(on, Ordering::Relaxed);
    }

    pub fn has_error(&self) -> bool {
        self.has_error.load(Ordering::Relaxed)
    }

    fn color(&self) -> bool {
        self.color.load(Ordering::Relaxed)
    }

    // Some C++ stdlibs don't support std::osyncstream even though
    // it's is in the C++20 standard. So we implement it ourselves.
    fn emit(&self, prefix_mono: &str, prefix_color: &str, msg: fmt::Arguments) {
        let prefix = if self.color() {
            prefix_color
        } else {
            prefix_mono
        };
        let text = format!("{prefix}{msg}\n");
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = io::stderr().write_all(text.as_bytes());
    }

    /// Reports an unrecoverable error and exits.
    pub fn fatal(&self, msg: fmt::Arguments) -> ! {
        self.emit("mold: fatal: ", "mold: \x1b[0;1;31mfatal:\x1b[0m ", msg);
        exit_after_cleanup(1);
    }

    /// Reports an error. With `--noinhibit-exec` it is downgraded to a warning.
    pub fn error(&self, msg: fmt::Arguments) {
        if self.noinhibit_exec.load(Ordering::Relaxed) {
            self.emit("mold: warning: ", "mold: \x1b[0;1;35mwarning:\x1b[0m ", msg);
        } else {
            self.emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", msg);
            self.has_error.store(true, Ordering::Relaxed);
        }
    }

    /// Reports a warning. With `--fatal-warnings` it is promoted to an error.
    pub fn warn(&self, msg: fmt::Arguments) {
        if self.suppress_warnings.load(Ordering::Relaxed) {
            return;
        }
        if self.fatal_warnings.load(Ordering::Relaxed) {
            self.emit("mold: error: ", "mold: \x1b[0;1;31merror:\x1b[0m ", msg);
            self.has_error.store(true, Ordering::Relaxed);
        } else {
            self.emit("mold: warning: ", "mold: \x1b[0;1;35mwarning:\x1b[0m ", msg);
        }
    }

    /// Prints an informational message to stdout.
    pub fn out(&self, msg: fmt::Arguments) {
        let text = format!("{msg}\n");
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = io::stdout().write_all(text.as_bytes());
    }

    /// Exits with a failure status if any error has been reported.
    pub fn checkpoint(&self) {
        if self.has_error() {
            exit_after_cleanup(1);
        }
    }
}

/// Anything that can report diagnostics; implemented by the linker
/// context and by [`Diagnostics`] itself so that the macros below accept
/// either.
pub trait HasDiagnostics {
    fn diagnostics(&self) -> &Diagnostics;
}

impl HasDiagnostics for Diagnostics {
    fn diagnostics(&self) -> &Diagnostics {
        self
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
    ($ctx:expr, $($arg:tt)*) => {
        $crate::error::HasDiagnostics::diagnostics(&$ctx).fatal(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! error {
    ($ctx:expr, $($arg:tt)*) => {
        $crate::error::HasDiagnostics::diagnostics(&$ctx).error(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($ctx:expr, $($arg:tt)*) => {
        $crate::error::HasDiagnostics::diagnostics(&$ctx).warn(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! out {
    ($ctx:expr, $($arg:tt)*) => {
        $crate::error::HasDiagnostics::diagnostics(&$ctx).out(format_args!($($arg)*))
    };
}

// strerror is not thread-safe, so guard it with a lock.
//
// Rust's standard-library conversion owns the returned message.
pub fn errno_string() -> String {
    io::Error::last_os_error().to_string()
}

impl<T: HasDiagnostics + ?Sized> HasDiagnostics for &T {
    fn diagnostics(&self) -> &Diagnostics {
        (**self).diagnostics()
    }
}

impl<T: HasDiagnostics + ?Sized> HasDiagnostics for &mut T {
    fn diagnostics(&self) -> &Diagnostics {
        (**self).diagnostics()
    }
}

impl<T: HasDiagnostics + ?Sized> HasDiagnostics for Arc<T> {
    fn diagnostics(&self) -> &Diagnostics {
        (**self).diagnostics()
    }
}
