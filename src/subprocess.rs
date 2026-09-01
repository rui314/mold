//! Process management: forking a child to hide exit latency, signal
//! handling for disk-full errors, and the `-run` subcommand.

#[cfg(not(windows))]
use std::sync::atomic::{AtomicI32, Ordering};

use crate::error::Diagnostics;
use crate::fatal;

#[cfg(not(windows))]
static PIPE_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

// Exiting from a program with large memory usage is slow --
// it may take a few hundred milliseconds. To hide the latency,
// we fork a child and let it do the actual linking work.
#[cfg(not(windows))]
pub fn fork_child() {
    let mut pipefd = [0i32; 2];
    // SAFETY: plain libc calls with valid arguments.
    unsafe {
        if libc::pipe(pipefd.as_mut_ptr()) == -1 {
            eprintln!("mold: pipe failed");
            std::process::exit(1);
        }
        let pid = libc::fork();
        if pid == -1 {
            eprintln!("mold: fork failed");
            std::process::exit(1);
        }
        if pid > 0 {
            // Parent
            libc::close(pipefd[1]);
            let mut buf = [0u8; 1];
            if libc::read(pipefd[0], buf.as_mut_ptr() as *mut libc::c_void, 1) == 1 {
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
        // Child
        libc::close(pipefd[0]);
    }
    PIPE_WRITE_FD.store(pipefd[1], Ordering::Relaxed);
}

#[cfg(windows)]
pub fn fork_child() {}

/// Tells the parent that the output is complete.
#[cfg(not(windows))]
pub fn notify_parent() {
    let fd = PIPE_WRITE_FD.swap(-1, Ordering::Relaxed);
    if fd == -1 {
        return;
    }
    let buf = [1u8];
    // SAFETY: fd is a valid pipe write end.
    unsafe {
        libc::write(fd, buf.as_ptr() as *const libc::c_void, 1);
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
    // The C++ handler has an additional OneTBB compatibility condition:
    // OneTBB 2021.9.0 has the interface version 12090.
    // Rust does not install OneTBB's signal handler.
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
pub fn process_run_subcommand(diag: &Diagnostics, argv: &[String]) -> ! {
    if argv.len() < 3 {
        fatal!(diag, "-run: argument missing");
    }
    let self_path = crate::util::self_path();
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
        fatal!(diag, "mold-wrapper.so is missing");
    };

    // Set environment variables
    std::env::set_var("LD_PRELOAD", &dso);
    std::env::set_var("MOLD_PATH", &self_path);

    use std::os::unix::process::CommandExt;
    let cmd = crate::util::path_filename(&argv[2]);
    // If ld, ld.lld or ld.gold is specified, run mold instead
    let err = if cmd == "ld" || cmd == "ld.lld" || cmd == "ld.gold" {
        std::process::Command::new(&self_path)
            .args(&argv[3..])
            .exec()
    } else {
        std::process::Command::new(&argv[2]).args(&argv[3..]).exec()
    };
    // Execute a given command
    fatal!(diag, "mold -run failed: {}: {err}", argv[2]);
}

#[cfg(windows)]
pub fn process_run_subcommand(diag: &Diagnostics, _argv: &[String]) -> ! {
    fatal!(diag, "-run is supported only on Unix");
}
