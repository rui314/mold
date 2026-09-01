//! Platform-independent virtual-memory reservations for linker arenas.

#[cfg(windows)]
use std::ffi::c_void;
use std::ptr::NonNull;

#[cfg(windows)]
const MEM_COMMIT: u32 = 0x1000;
#[cfg(windows)]
const MEM_RESERVE: u32 = 0x2000;
#[cfg(windows)]
const MEM_RELEASE: u32 = 0x8000;
#[cfg(windows)]
const PAGE_NOACCESS: u32 = 0x01;
#[cfg(windows)]
const PAGE_READWRITE: u32 = 0x04;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
}

/// Reserves a contiguous address range without committing physical memory.
pub fn reserve(size: usize) -> Option<NonNull<u8>> {
    #[cfg(windows)]
    // SAFETY: a null address asks Windows to choose an address for a new
    // reservation. No memory is accessible until [`commit`] succeeds.
    let data = unsafe { VirtualAlloc(std::ptr::null_mut(), size, MEM_RESERVE, PAGE_NOACCESS) };

    #[cfg(not(windows))]
    let data = {
        let flags = libc::MAP_ANONYMOUS | libc::MAP_PRIVATE;
        #[cfg(any(target_os = "android", target_os = "linux"))]
        let flags = flags | libc::MAP_NORESERVE;

        // SAFETY: this creates private anonymous storage.
        let data = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                -1,
                0,
            )
        };
        if data == libc::MAP_FAILED {
            return None;
        }

        #[cfg(any(target_os = "android", target_os = "linux"))]
        // SAFETY: the range is the fresh mapping; the advice is only a hint.
        unsafe {
            libc::madvise(data, size, libc::MADV_HUGEPAGE);
        }
        data
    };

    NonNull::new(data.cast())
}

/// Commits a portion of a reservation before it is written.
///
/// # Safety
///
/// The range must lie within a reservation returned by [`reserve`].
#[inline]
pub unsafe fn commit(address: *mut u8, size: usize) -> bool {
    #[cfg(windows)]
    {
        size == 0
            || !unsafe { VirtualAlloc(address.cast(), size, MEM_COMMIT, PAGE_READWRITE) }.is_null()
    }

    #[cfg(not(windows))]
    {
        let _ = (address, size);
        true
    }
}

/// Releases a complete reservation.
///
/// # Safety
///
/// `address` and `size` must describe a reservation returned by [`reserve`].
pub unsafe fn release(address: *mut u8, size: usize) {
    #[cfg(windows)]
    unsafe {
        let _ = size;
        VirtualFree(address.cast(), 0, MEM_RELEASE);
    }

    #[cfg(not(windows))]
    unsafe {
        libc::munmap(address.cast(), size);
    }
}
