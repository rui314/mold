use crate::{CVWords, IncrementCounter, BLOCK_LEN, OUT_LEN};

// Note that there is no AVX2 implementation of compress_in_place or
// compress_xof.

// Unsafe because this may only be called on platforms supporting AVX2.
pub unsafe fn hash_many<const N: usize>(
    inputs: &[&[u8; N]],
    key: &CVWords,
    counter: u64,
    increment_counter: IncrementCounter,
    flags: u8,
    flags_start: u8,
    flags_end: u8,
    out: &mut [u8],
) {
    // The Rust hash_many implementations do bounds checking on the `out`
    // array, but the C implementations don't. Even though this is an unsafe
    // function, assert the bounds here.
    assert!(out.len() >= inputs.len() * OUT_LEN);
    ffi::blake3_hash_many_avx2(
        inputs.as_ptr() as *const *const u8,
        inputs.len(),
        N / BLOCK_LEN,
        key.as_ptr(),
        counter,
        increment_counter.yes(),
        flags,
        flags_start,
        flags_end,
        out.as_mut_ptr(),
    )
}

pub mod ffi {
    extern "C" {
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
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_hash_many() {
        if !crate::platform::avx2_detected() {
            return;
        }
        crate::test::test_hash_many_fn(hash_many, hash_many);
    }
}
