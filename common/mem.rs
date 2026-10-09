//! Memory: transparent huge pages for large tables, and allocations that
//! are never freed.

/// Requests transparent huge pages for a mapped byte range on Linux-based
/// targets.
///
/// # Safety
///
/// `data..data + size` must describe a live mapping.
#[cfg(any(target_os = "android", target_os = "linux"))]
pub(crate) unsafe fn madvise_hugepage(data: *mut u8, size: usize) {
    // SAFETY: guaranteed by the caller; MADV_HUGEPAGE is only a kernel hint.
    let _ = unsafe { libc::madvise(data.cast(), size, libc::MADV_HUGEPAGE) };
}

/// No-op on targets that do not support Linux's `MADV_HUGEPAGE` advice.
///
/// # Safety
///
/// Kept identical to the supported-target signature.
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) unsafe fn madvise_hugepage(_data: *mut u8, _size: usize) {}

/// Requests transparent huge pages for the whole pages strictly inside an
/// allocation, leaving possible allocator metadata in its boundary pages
/// untouched.
///
/// # Safety
///
/// `data..data + size` must describe a live allocation.
#[cfg(any(target_os = "android", target_os = "linux"))]
pub unsafe fn madvise_hugepage_interior(data: *const u8, size: usize) {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size == 0 || page_size <= 0 {
        return;
    }

    let page_size = page_size as usize;
    let start = data.addr();
    let Some(end) = start.checked_add(size) else {
        return;
    };
    let Some(rounded) = start.checked_add(page_size - 1) else {
        return;
    };
    let first_page = rounded / page_size * page_size;
    let last_page = end / page_size * page_size;
    if first_page < last_page {
        // SAFETY: these complete pages lie strictly inside the caller's
        // allocation.
        unsafe { madvise_hugepage(data.with_addr(first_page).cast_mut(), last_page - first_page) };
    }
}

/// No-op on targets that do not support Linux's `MADV_HUGEPAGE` advice.
///
/// # Safety
///
/// Kept identical to the supported-target signature.
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub unsafe fn madvise_hugepage_interior(_data: *const u8, _size: usize) {}

/// Leaks a value for the rest of the process's lifetime.
///
/// Input files, symbol names and a few other objects must outlive every
/// data structure of a link, and the process exits as soon as the link is
/// done, so never freeing them is both simplest and cheapest.
pub fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

/// Leaks a byte string for the rest of the process's lifetime.
pub fn leak_bytes(bytes: Vec<u8>) -> &'static [u8] {
    Vec::leak(bytes)
}
