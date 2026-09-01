//! Limits the number of concurrent mold processes.

// Many build systems attempt to invoke as many linker processes as there
// are cores, based on the assumption that the linker is single-threaded.
// However, since mold is multi-threaded, such build systems' behavior is
// not beneficial and just increases the overall peak memory usage.
// On machines with limited memory, this could lead to an out-of-memory
// error.
//
// This file implements a feature that limits the number of concurrent
// mold processes to just 1 for each user. It is intended to be used as
// `MOLD_JOBS=1 ninja` or `MOLD_JOBS=1 make -j$(nproc)`.

#[cfg(not(windows))]
use std::ffi::CStr;
#[cfg(not(windows))]
use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(not(windows))]
static LOCK_FD: AtomicI32 = AtomicI32::new(-1);

#[cfg(not(windows))]
pub fn acquire_global_lock() {
    if std::env::var_os("MOLD_JOBS").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }

    let path = if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(dir).join("mold-lock")
    } else {
        // SAFETY: getpwuid returns either null or a pointer to process-global
        // storage. We copy the name before making another libc call.
        let name = unsafe {
            let pwd = libc::getpwuid(libc::getuid());
            if pwd.is_null() || (*pwd).pw_name.is_null() {
                return;
            }
            CStr::from_ptr((*pwd).pw_name).to_bytes().to_vec()
        };
        let name = String::from_utf8_lossy(&name);
        std::path::PathBuf::from(format!("/tmp/mold-lock-{name}"))
    };
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return;
    };

    // SAFETY: path is a valid C string and the remaining arguments have the
    // types required by open and lockf.
    unsafe {
        let fd = libc::open(
            path.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_CLOEXEC,
            0o600,
        );
        if fd == -1 {
            return;
        }
        if libc::lockf(fd, libc::F_LOCK, 0) == -1 {
            libc::close(fd);
            return;
        }
        LOCK_FD.store(fd, Ordering::Relaxed);
    }
}

#[cfg(windows)]
pub fn acquire_global_lock() {}

#[cfg(not(windows))]
pub fn release_global_lock() {
    let fd = LOCK_FD.swap(-1, Ordering::Relaxed);
    if fd != -1 {
        // SAFETY: fd is the lock file opened by acquire_global_lock.
        unsafe { libc::close(fd) };
    }
}

#[cfg(windows)]
pub fn release_global_lock() {}
