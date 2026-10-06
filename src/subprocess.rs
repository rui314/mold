//! Process management: forking a child to hide exit latency, signal
//! handling for disk-full errors, and the `-run` subcommand.

#[cfg(not(windows))]
use std::fs::File;
#[cfg(not(windows))]
use std::io::{Read, Write};
#[cfg(not(windows))]
use std::os::unix::io::{FromRawFd, OwnedFd};
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

            // If SIGCHLD is ignored, which is inherited across exec, the
            // child is reaped automatically and waitpid fails. Its exit
            // status is lost then, so report a failure.
            let mut status = 0;
            if libc::waitpid(pid, &raw mut status, 0) == -1 {
                libc::_exit(1);
            }
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
    let _ = File::from(writer).write_all(&[1]);
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
    // not enough space left on the filesystem. The actual disk blocks are
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
            libc::write(libc::STDERR_FILENO, msg.as_ptr().cast(), msg.len());
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
        libc::sigemptyset(&raw mut action.sa_mask);
        action.sa_flags = libc::SA_SIGINFO;
        libc::sigaction(libc::SIGSEGV, &raw const action, std::ptr::null_mut());
        libc::sigaction(libc::SIGBUS, &raw const action, std::ptr::null_mut());
    }
}

#[cfg(windows)]
pub fn install_signal_handler() {}

/// `mold -run COMMAND ARGS...` runs a command with mold interposed as the
/// linker. The preload library `mold-wrapper.so`, which is embedded in the
/// executable, intercepts the exec family of functions in the processes
/// that the command starts and replaces `ld` with mold.
///
/// The library is passed to the command in a sealed memfd. Child processes
/// inherit the descriptor and preload the library from it, so no file is
/// installed or left behind, and the kernel frees the memfd when the last
/// process using it exits. c/mold-wrapper.c describes how the library
/// keeps the descriptor valid in descendant processes.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
pub fn process_run_subcommand(argv: &[std::ffi::OsString]) -> ! {
    static WRAPPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mold-wrapper.so"));

    if argv.len() < 3 {
        fatal!("-run: argument missing");
    }
    if WRAPPER.is_empty() {
        fatal!("-run: mold was built without mold-wrapper.so");
    }
    let self_path = std::env::current_exe().expect("cannot get current executable path");
    let fd = create_sealed_memfd(WRAPPER).to_string();

    // If ld, ld.lld or ld.gold is specified, run mold instead
    let cmd = std::path::Path::new(&argv[2]).file_name().unwrap_or_default();
    let program = if cmd == "ld" || cmd == "ld.lld" || cmd == "ld.gold" {
        self_path.as_os_str()
    } else {
        &argv[2]
    };

    // Execute a given command with the wrapper preloaded
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(program);
    command.args(&argv[3..]).env("MOLD_PATH", &self_path).env("MOLD_WRAPPER_FD", &fd);
    if cfg!(target_os = "freebsd") {
        command.env("LD_PRELOAD_FDS", &fd);
    } else {
        command.env("LD_PRELOAD", format!("/proc/self/fd/{fd}"));
    }
    let err = command.exec();
    fatal!("mold -run failed: {}: {}", argv[2].to_string_lossy(), crate::error::strerror(&err));
}

/// Copies `contents` to a sealed memfd and returns its descriptor, which
/// programs executed by this process inherit.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
fn create_sealed_memfd(contents: &[u8]) -> i32 {
    let flags = libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING;
    // SAFETY: the name is a NUL-terminated string.
    #[cfg(target_os = "freebsd")]
    let fd = unsafe { libc::memfd_create(c"mold-wrapper.so".as_ptr(), flags) };
    // glibc provides memfd_create() only since 2.27.
    // SAFETY: the name is a NUL-terminated string.
    #[cfg(not(target_os = "freebsd"))]
    let fd =
        unsafe { libc::syscall(libc::SYS_memfd_create, c"mold-wrapper.so".as_ptr(), flags) } as i32;
    if fd == -1 {
        let err = std::io::Error::last_os_error();
        fatal!("-run: memfd_create failed: {}", crate::error::strerror(&err));
    }

    // SAFETY: `fd` is a new descriptor that nothing else owns.
    let mut file = unsafe { File::from_raw_fd(fd) };
    if let Err(err) = file.write_all(contents) {
        fatal!("-run: cannot write mold-wrapper.so: {}", crate::error::strerror(&err));
    }

    // Seal the memfd so that its contents never change, and duplicate it to
    // a descriptor number above those that shells and build tools usually
    // pick. Unlike the original, the duplicate is inherited across exec.
    let seals = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    // SAFETY: fcntl only changes the state of a descriptor we own.
    let new_fd = unsafe {
        if libc::fcntl(fd, libc::F_ADD_SEALS, seals) == -1 {
            -1
        } else {
            libc::fcntl(fd, libc::F_DUPFD, 100)
        }
    };
    if new_fd == -1 {
        let err = std::io::Error::last_os_error();
        fatal!("-run: cannot seal mold-wrapper.so: {}", crate::error::strerror(&err));
    }
    new_fd
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
pub fn process_run_subcommand(_argv: &[std::ffi::OsString]) -> ! {
    fatal!("-run is supported only on Linux and FreeBSD");
}
