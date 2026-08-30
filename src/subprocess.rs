//! Process management: forking a child to hide exit latency, signal
//! handling for disk-full errors, and the `-run` subcommand.

use std::sync::atomic::{AtomicI32, Ordering};

use crate::diagnostics::Diagnostics;
use crate::fatal;

static PIPE_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

/// Exiting a process with a large heap is slow, so the parent forks a
/// child to do the actual work and exits as soon as the child reports
/// that the output has been written.
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

/// Tells the parent that the output is complete.
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

extern "C" fn on_signal(signo: libc::c_int) {
    // A write to a memory-mapped output file fails with SIGBUS if the
    // disk fills up; explain that rather than crashing silently.
    if signo == libc::SIGBUS {
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
    // SAFETY: restoring the default handler and re-raising.
    unsafe {
        libc::signal(signo, libc::SIG_DFL);
        libc::raise(signo);
    }
}

pub fn install_signal_handler() {
    // SAFETY: installing a handler that only calls async-signal-safe
    // functions.
    unsafe {
        libc::signal(libc::SIGSEGV, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGBUS, on_signal as *const () as libc::sighandler_t);
    }
}

/// `mold -run COMMAND ARGS...` runs a command with mold interposed as the
/// linker, which requires the `mold-wrapper.so` preload library.
pub fn process_run_subcommand(diag: &Diagnostics, argv: &[String]) -> ! {
    if argv.len() < 3 {
        fatal!(diag, "-run: argument missing");
    }
    let self_path = crate::util::self_path();
    let candidates = [
        self_path.parent().map(|p| p.join("mold-wrapper.so")),
        Some(std::path::PathBuf::from(
            "/usr/local/lib/mold/mold-wrapper.so",
        )),
        self_path
            .parent()
            .map(|p| p.join("../lib/mold/mold-wrapper.so")),
    ];
    let Some(dso) = candidates.into_iter().flatten().find(|p| p.is_file()) else {
        fatal!(diag, "mold-wrapper.so is missing");
    };

    std::env::set_var("LD_PRELOAD", &dso);
    std::env::set_var("MOLD_PATH", &self_path);

    use std::os::unix::process::CommandExt;
    let cmd = crate::util::path_filename(&argv[2]);
    let err = if cmd == "ld" || cmd == "ld.lld" || cmd == "ld.gold" {
        std::process::Command::new(&self_path)
            .args(&argv[3..])
            .exec()
    } else {
        std::process::Command::new(&argv[2]).args(&argv[3..]).exec()
    };
    fatal!(diag, "mold -run failed: {}: {err}", argv[2]);
}
