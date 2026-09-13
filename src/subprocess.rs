//! Process management: forking a child to hide exit latency, signal
//! handling for disk-full errors, and the `-run` subcommand.

#[cfg(not(windows))]
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(not(windows))]
use std::sync::Mutex;

use crate::fatal;

#[cfg(not(windows))]
static PIPE_WRITER: Mutex<Option<OwnedFd>> = Mutex::new(None);

// Exiting from a program with large memory usage is slow --
// it may take a few hundred milliseconds. To hide the latency,
// we fork a child and let it do the actual linking work.
#[cfg(not(windows))]
pub fn fork_child() {
    let mut pipefd = [0i32; 2];
    // Preserve pipe's descriptor inheritance across the LTO restart.
    // SAFETY: pipe initializes both descriptors on success; each then has
    // exactly one owner in this process.
    let (reader, writer) = unsafe {
        if libc::pipe(pipefd.as_mut_ptr()) == -1 {
            eprintln!("mold: pipe failed");
            std::process::exit(1);
        }
        (
            OwnedFd::from_raw_fd(pipefd[0]),
            OwnedFd::from_raw_fd(pipefd[1]),
        )
    };
    // SAFETY: this runs before the linker starts its worker threads. The
    // parent only waits for completion and exits; the child continues linking.
    unsafe {
        let pid = libc::fork();
        if pid == -1 {
            eprintln!("mold: fork failed");
            std::process::exit(1);
        }
        if pid > 0 {
            // Parent
            drop(writer);
            let mut buf = [0u8; 1];
            if libc::read(reader.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, 1) == 1 {
                libc::_exit(0);
            }
            let mut status = 0;
            libc::waitpid(pid, &mut status, 0);
            if libc::WIFEXITED(status) {
                libc::_exit(libc::WEXITSTATUS(status));
            }
            if libc::WIFSIGNALED(status) {
                libc::raise(libc::WTERMSIG(status));
            }
            libc::_exit(1);
        }
    }
    // Child
    drop(reader);
    *PIPE_WRITER.lock().unwrap() = Some(writer);
}

#[cfg(windows)]
pub fn fork_child() {}

/// Tells the parent that the output is complete.
#[cfg(not(windows))]
pub fn notify_parent() {
    let Some(writer) = PIPE_WRITER.lock().unwrap().take() else {
        return;
    };
    let buf = [1u8];
    // SAFETY: writer owns a valid pipe write end.
    unsafe {
        libc::write(writer.as_raw_fd(), buf.as_ptr() as *const libc::c_void, 1);
    }
}

#[cfg(windows)]
pub fn notify_parent() {}

#[cfg(not(windows))]
extern "C" fn on_signal(
    signo: libc::c_int,
    info: *mut libc::siginfo_t,
    _context: *mut libc::c_void,
) {
    // mold mmap's an output file, and the mmap succeeds even if there's
    // no enough space left on the filesystem. The actual disk blocks are
    // not allocated on the mmap call but when the program writes to it
    // for the first time.
    //
    // If a disk becomes full as a result of a write to an mmap'ed memory
    // region, the failure of the write is reported as a SIGBUS. This
    // signal handler catches that signal and prints out a user-friendly
    // error message. Without this, it is very hard to realize that the
    // disk might be full.
    let addr = if info.is_null() {
        0
    } else {
        // SAFETY: SA_SIGINFO gives the handler a valid siginfo_t pointer.
        unsafe { (*info).si_addr() as usize }
    };
    if (signo == libc::SIGSEGV || signo == libc::SIGBUS)
        && crate::output_file::output_buffer_contains(addr)
    {
        // Handle disk full error
        let msg = b"mold: failed to write to an output file. Disk full?\n";
        // SAFETY: write is async-signal-safe.
        unsafe {
            libc::write(
                libc::STDERR_FILENO,
                msg.as_ptr() as *const libc::c_void,
                msg.len(),
            );
        }
    }
    crate::output_file::cleanup();
    // Re-throw the signal
    // SAFETY: restoring the default handlers and re-raising.
    unsafe {
        libc::signal(libc::SIGSEGV, libc::SIG_DFL);
        libc::signal(libc::SIGBUS, libc::SIG_DFL);
        libc::raise(signo);
    }
}

#[cfg(not(windows))]
pub fn install_signal_handler() {
    // OneTBB 2021.9.0 (interface version 12090) installs its own signal
    // handler. This binary does not link OneTBB, so no compatibility condition
    // is needed.
    // SAFETY: installing a signal handler with the three-argument SA_SIGINFO
    // calling convention.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = libc::SA_SIGINFO;
        libc::sigaction(libc::SIGSEGV, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGBUS, &action, std::ptr::null_mut());
    }
}

#[cfg(windows)]
pub fn install_signal_handler() {}

/// `mold -run COMMAND ARGS...` runs a command with mold interposed as the
/// linker, which requires the `mold-wrapper.so` preload library.
#[cfg(not(windows))]
pub fn process_run_subcommand(argv: &[std::ffi::OsString]) -> ! {
    if argv.len() < 3 {
        fatal!("-run: argument missing");
    }
    let self_path = std::env::current_exe().expect("cannot get current executable path");
    let candidates = [
        // Look for mold-wrapper.so from the same directory as the executable is.
        self_path.parent().map(|p| p.join("mold-wrapper.so")),
        // If not found, search $(MOLD_LIBDIR)/mold, which is /usr/local/lib/mold
        // by default.
        Some(std::path::PathBuf::from(
            "/usr/local/lib/mold/mold-wrapper.so",
        )),
        // Look for ../lib/mold/mold-wrapper.so
        self_path
            .parent()
            .map(|p| p.join("../lib/mold/mold-wrapper.so")),
    ];
    // Get the mold-wrapper.so path
    let Some(dso) = candidates.into_iter().flatten().find(|p| p.is_file()) else {
        fatal!("mold-wrapper.so is missing");
    };

    // Set environment variables
    std::env::set_var("LD_PRELOAD", &dso);
    std::env::set_var("MOLD_PATH", &self_path);

    use std::os::unix::process::CommandExt;
    let cmd = std::path::Path::new(&argv[2])
        .file_name()
        .unwrap_or_default();
    // If ld, ld.lld or ld.gold is specified, run mold instead
    let err = if cmd == "ld" || cmd == "ld.lld" || cmd == "ld.gold" {
        std::process::Command::new(&self_path)
            .args(&argv[3..])
            .exec()
    } else {
        std::process::Command::new(&argv[2]).args(&argv[3..]).exec()
    };
    // Execute a given command
    fatal!("mold -run failed: {}: {err}", argv[2].to_string_lossy());
}

#[cfg(windows)]
pub fn process_run_subcommand(_argv: &[std::ffi::OsString]) -> ! {
    fatal!("-run is supported only on Unix");
}
