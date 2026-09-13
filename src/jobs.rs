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
use std::fs::{File, OpenOptions};
#[cfg(not(windows))]
use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
#[cfg(not(windows))]
use std::sync::Mutex;

#[cfg(not(windows))]
static LOCK_FILE: Mutex<Option<File>> = Mutex::new(None);

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
    let Ok(file) = OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .open(path)
    else {
        return;
    };
    // SAFETY: file owns a valid descriptor. Keep lockf for compatibility with
    // other mold processes; flock uses a different locking protocol.
    if unsafe { libc::lockf(file.as_raw_fd(), libc::F_LOCK, 0) } == -1 {
        return;
    }
    *LOCK_FILE.lock().unwrap() = Some(file);
}

#[cfg(windows)]
pub fn acquire_global_lock() {}

#[cfg(not(windows))]
pub fn release_global_lock() {
    drop(LOCK_FILE.lock().unwrap().take());
}

#[cfg(windows)]
pub fn release_global_lock() {}
