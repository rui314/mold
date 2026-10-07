//! The `-run` subcommand.

use mold_common::fatal;

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
    use std::os::unix::process::CommandExt;

    use mold_common::error::strerror;

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
    let mut command = std::process::Command::new(program);
    command.args(&argv[3..]).env("MOLD_PATH", &self_path).env("MOLD_WRAPPER_FD", &fd);
    if cfg!(target_os = "freebsd") {
        command.env("LD_PRELOAD_FDS", &fd);
    } else {
        command.env("LD_PRELOAD", format!("/proc/self/fd/{fd}"));
    }
    let err = command.exec();
    fatal!("mold -run failed: {}: {}", argv[2].to_string_lossy(), strerror(&err));
}

/// Copies `contents` to a sealed memfd and returns its descriptor, which
/// programs executed by this process inherit.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
fn create_sealed_memfd(contents: &[u8]) -> i32 {
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::io::FromRawFd;

    use mold_common::error::strerror;

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
        fatal!("-run: memfd_create failed: {}", strerror(&err));
    }

    // SAFETY: `fd` is a new descriptor that nothing else owns.
    let mut file = unsafe { File::from_raw_fd(fd) };
    if let Err(err) = file.write_all(contents) {
        fatal!("-run: cannot write mold-wrapper.so: {}", strerror(&err));
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
        fatal!("-run: cannot seal mold-wrapper.so: {}", strerror(&err));
    }
    new_fd
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
pub fn process_run_subcommand(_argv: &[std::ffi::OsString]) -> ! {
    fatal!("-run is supported only on Linux and FreeBSD");
}
