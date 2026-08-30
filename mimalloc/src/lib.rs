//! The mimalloc allocator as mold's global allocator, built from the same
//! vendored source as the C++ mold (see csrc/mimalloc).
//!
//! mold allocates heavily from many threads at once; mimalloc avoids the
//! heap-growth syscalls and lock contention that glibc's malloc suffers.

use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;

extern "C" {
    fn mi_malloc_aligned(size: usize, alignment: usize) -> *mut c_void;
    fn mi_zalloc_aligned(size: usize, alignment: usize) -> *mut c_void;
    fn mi_realloc_aligned(p: *mut c_void, newsize: usize, alignment: usize) -> *mut c_void;
    fn mi_free(p: *mut c_void);
}

/// The allocator; declare it as the `#[global_allocator]`.
pub struct MiMalloc;

// SAFETY: mimalloc's functions are thread-safe, and every allocation is
// requested and released with its own size and alignment as GlobalAlloc
// demands.
unsafe impl GlobalAlloc for MiMalloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        mi_malloc_aligned(layout.size(), layout.align()) as *mut u8
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        mi_zalloc_aligned(layout.size(), layout.align()) as *mut u8
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        mi_realloc_aligned(ptr as *mut c_void, new_size, layout.align()) as *mut u8
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        mi_free(ptr as *mut c_void);
    }
}
