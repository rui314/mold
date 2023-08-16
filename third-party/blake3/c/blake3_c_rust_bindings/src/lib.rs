//! These are Rust bindings for the C implementation of BLAKE3. As there is a
//! native (and faster) Rust implementation of BLAKE3 provided in this same
//! repo, these bindings are not expected to be used in production. They're
//! intended for testing and benchmarking.

use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;

#[cfg(test)]
mod test;

pub const BLOCK_LEN: usize = 64;
pub const CHUNK_LEN: usize = 1024;
pub const OUT_LEN: usize = 32;

// Feature detection functions for tests and benchmarks. Note that the C code
// does its own feature detection in blake3_dispatch.c.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn sse2_detected() -> bool {
    is_x86_feature_detected!("sse2")
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn sse41_detected() -> bool {
    is_x86_feature_detected!("sse4.1")
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn avx2_detected() -> bool {
    is_x86_feature_detected!("avx2")
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn avx512_detected() -> bool {
    is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512vl")
}

#[derive(Clone)]
pub struct Hasher(ffi::blake3_hasher);

impl Hasher {
    pub fn new() -> Self {
        let mut c_state = MaybeUninit::uninit();
        unsafe {
            ffi::blake3_hasher_init(c_state.as_mut_ptr());
            Self(c_state.assume_init())
        }
    }

    pub fn new_keyed(key: &[u8; 32]) -> Self {
        let mut c_state = MaybeUninit::uninit();
        unsafe {
            ffi::blake3_hasher_init_keyed(c_state.as_mut_ptr(), key.as_ptr());
            Self(c_state.assume_init())
        }
    }

    pub fn new_derive_key(context: &str) -> Self {
        let mut c_state = MaybeUninit::uninit();
        let context_c_string = CString::new(context).expect("valid C string, no null bytes");
        unsafe {
            ffi::blake3_hasher_init_derive_key(c_state.as_mut_ptr(), context_c_string.as_ptr());
            Self(c_state.assume_init())
        }
    }

    pub fn new_derive_key_raw(context: &[u8]) -> Self {
        let mut c_state = MaybeUninit::uninit();
        unsafe {
            ffi::blake3_hasher_init_derive_key_raw(
                c_state.as_mut_ptr(),
                context.as_ptr() as *const _,
                context.len(),
            );
            Self(c_state.assume_init())
        }
    }

    pub fn update(&mut self, input: &[u8]) {
        unsafe {
            ffi::blake3_hasher_update(&mut self.0, input.as_ptr() as *const c_void, input.len());
        }
    }

    pub fn finalize(&self, output: &mut [u8]) {
        unsafe {
            ffi::blake3_hasher_finalize(&self.0, output.as_mut_ptr(), output.len());
        }
    }

    pub fn finalize_seek(&self, seek: u64, output: &mut [u8]) {
        unsafe {
            ffi::blake3_hasher_finalize_seek(&self.0, seek, output.as_mut_ptr(), output.len());
        }
    }

    pub fn reset(&mut self) {
        unsafe {
            ffi::blake3_hasher_reset(&mut self.0);
        }
    }
}

pub mod ffi {
    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct blake3_chunk_state {
        pub cv: [u32; 8usize],
        pub chunk_counter: u64,
        pub buf: [u8; 64usize],
        pub buf_len: u8,
        pub blocks_compressed: u8,
        pub flags: u8,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct blake3_hasher {
        pub key: [u32; 8usize],
        pub chunk: blake3_chunk_state,
        pub cv_stack_len: u8,
        pub cv_stack: [u8; 1728usize],
    }

    extern "C" {
        // public interface
        pub fn blake3_hasher_init(self_: *mut blake3_hasher);
        pub fn blake3_hasher_init_keyed(self_: *mut blake3_hasher, key: *const u8);
        pub fn blake3_hasher_init_derive_key(
            self_: *mut blake3_hasher,
            context: *const ::std::os::raw::c_char,
        );
        pub fn blake3_hasher_init_derive_key_raw(
            self_: *mut blake3_hasher,
            context: *const ::std::os::raw::c_void,
            context_len: usize,
        );
        pub fn blake3_hasher_update(
            self_: *mut blake3_hasher,
            input: *const ::std::os::raw::c_void,
            input_len: usize,
        );
        pub fn blake3_hasher_finalize(self_: *const blake3_hasher, out: *mut u8, out_len: usize);
        pub fn blake3_hasher_finalize_seek(
            self_: *const blake3_hasher,
            seek: u64,
            out: *mut u8,
            out_len: usize,
        );
        pub fn blake3_hasher_reset(self_: *mut blake3_hasher);

        // portable low-level functions
        pub fn blake3_compress_in_place_portable(
            cv: *mut u32,
            block: *const u8,
            block_len: u8,
            counter: u64,
            flags: u8,
        );
        pub fn blake3_compress_xof_portable(
            cv: *const u32,
            block: *const u8,
            block_len: u8,
            counter: u64,
            flags: u8,
            out: *mut u8,
        );
        pub fn blake3_hash_many_portable(
            inputs: *const *const u8,
            num_inputs: usize,
            blocks: usize,
            key: *const u32,
            counter: u64,
            increment_counter: bool,
            flags: u8,
            flags_start: u8,
            flags_end: u8,
            out: *mut u8,
        );
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    pub mod x86 {
        extern "C" {
            // SSE2 low level functions
            pub fn blake3_compress_in_place_sse2(
                cv: *mut u32,
                block: *const u8,
                block_len: u8,
                counter: u64,
                flags: u8,
            );
            pub fn blake3_compress_xof_sse2(
                cv: *const u32,
                block: *const u8,
                block_len: u8,
                counter: u64,
                flags: u8,
                out: *mut u8,
            );
            pub fn blake3_hash_many_sse2(
                inputs: *const *const u8,
                num_inputs: usize,
                blocks: usize,
                key: *const u32,
                counter: u64,
                increment_counter: bool,
                flags: u8,
                flags_start: u8,
                flags_end: u8,
                out: *mut u8,
            );

            // SSE4.1 low level functions
            pub fn blake3_compress_in_place_sse41(
                cv: *mut u32,
                block: *const u8,
                block_len: u8,
                counter: u64,
                flags: u8,
            );
            pub fn blake3_compress_xof_sse41(
                cv: *const u32,
                block: *const u8,
                block_len: u8,
                counter: u64,
                flags: u8,
                out: *mut u8,
            );
            pub fn blake3_hash_many_sse41(
                inputs: *const *const u8,
                num_inputs: usize,
                blocks: usize,
                key: *const u32,
                counter: u64,
                increment_counter: bool,
                flags: u8,
                flags_start: u8,
                flags_end: u8,
                out: *mut u8,
            );

            // AVX2 low level functions
            pub fn blake3_hash_many_avx2(
                inputs: *const *const u8,
                num_inputs: usize,
                blocks: usize,
                key: *const u32,
                counter: u64,
                increment_counter: bool,
                flags: u8,
                flags_start: u8,
                flags_end: u8,
                out: *mut u8,
            );

            // AVX-512 low level functions
            pub fn blake3_compress_xof_avx512(
                cv: *const u32,
                block: *const u8,
                block_len: u8,
                counter: u64,
                flags: u8,
                out: *mut u8,
            );
            pub fn blake3_compress_in_place_avx512(
                cv: *mut u32,
                block: *const u8,
                block_len: u8,
                counter: u64,
                flags: u8,
            );
            pub fn blake3_hash_many_avx512(
                inputs: *const *const u8,
                num_inputs: usize,
                blocks: usize,
                key: *const u32,
                counter: u64,
                increment_counter: bool,
                flags: u8,
                flags_start: u8,
                flags_end: u8,
                out: *mut u8,
            );
        }
    }

    #[cfg(feature = "neon")]
    pub mod neon {
        extern "C" {
            // NEON low level functions
            pub fn blake3_hash_many_neon(
                inputs: *const *const u8,
                num_inputs: usize,
                blocks: usize,
                key: *const u32,
                counter: u64,
                increment_counter: bool,
                flags: u8,
                flags_start: u8,
                flags_end: u8,
                out: *mut u8,
            );
        }
    }
}
