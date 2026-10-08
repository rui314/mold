//! Process management: forking a child to hide exit latency, and
//! signal handling that removes a partial output file.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::sync::Mutex;

static PIPE_WRITER: Mutex<Option<OwnedFd>> = Mutex::new(None);

// Exiting from a program with large memory usage is slow --
// it may take a few hundred milliseconds. To hide the latency,
// we fork a child and let it do the actual linking work.
pub fn fork_child() {
    let mut pipefd = [0i32; 2];
    // SAFETY: pipe initializes both descriptors on success; each then has
    // exactly one owner in this process.
    let (reader, writer) = unsafe {
        if libc::pipe(pipefd.as_mut_ptr()) == -1 {
            eprintln!("mold: pipe failed");
            std::process::exit(1);
        }
        (OwnedFd::from_raw_fd(pipefd[0]), OwnedFd::from_raw_fd(pipefd[1]))
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
            if File::from(reader).read_exact(&mut [0u8]).is_ok() {
                libc::_exit(0);
            }
            let mut status = 0;
            libc::waitpid(pid, &raw mut status, 0);
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

/// Tells the parent that the output is complete.
pub fn notify_parent() {
    let Some(writer) = PIPE_WRITER.lock().unwrap().take() else {
        return;
    };
    let _ = File::from(writer).write_all(&[1]);
}

extern "C" fn on_signal(
    signo: libc::c_int,
    _info: *mut libc::siginfo_t,
    _context: *mut libc::c_void,
) {
    // The output is written in place as its ranges are finished, so a
    // crash in the middle of a link would leave a truncated file at the
    // output path. Remove it before dying of the signal.
    crate::output_file::cleanup();
    // Re-throw the signal
    // SAFETY: restoring the default handlers and re-raising.
    unsafe {
        libc::signal(libc::SIGSEGV, libc::SIG_DFL);
        libc::signal(libc::SIGBUS, libc::SIG_DFL);
        libc::raise(signo);
    }
}

pub fn install_signal_handler() {
    // SAFETY: installing a signal handler with the three-argument SA_SIGINFO
    // calling convention.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
        libc::sigemptyset(&raw mut action.sa_mask);
        action.sa_flags = libc::SA_SIGINFO;
        libc::sigaction(libc::SIGSEGV, &raw const action, std::ptr::null_mut());
        libc::sigaction(libc::SIGBUS, &raw const action, std::ptr::null_mut());
    }
}
