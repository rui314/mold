// Most of this code is duplicated from the root `blake3` crate. Perhaps we
// could share more of it in the future.

use crate::{BLOCK_LEN, CHUNK_LEN, OUT_LEN};
use arrayref::{array_mut_ref, array_ref};
use arrayvec::ArrayVec;
use core::usize;
use rand::prelude::*;

const CHUNK_START: u8 = 1 << 0;
const CHUNK_END: u8 = 1 << 1;
const PARENT: u8 = 1 << 2;
const ROOT: u8 = 1 << 3;
const KEYED_HASH: u8 = 1 << 4;
// const DERIVE_KEY_CONTEXT: u8 = 1 << 5;
// const DERIVE_KEY_MATERIAL: u8 = 1 << 6;

// Interesting input lengths to run tests on.
pub const TEST_CASES: &[usize] = &[
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7,
    8,
    BLOCK_LEN - 1,
    BLOCK_LEN,
    BLOCK_LEN + 1,
    2 * BLOCK_LEN - 1,
    2 * BLOCK_LEN,
    2 * BLOCK_LEN + 1,
    CHUNK_LEN - 1,
    CHUNK_LEN,
    CHUNK_LEN + 1,
    2 * CHUNK_LEN,
    2 * CHUNK_LEN + 1,
    3 * CHUNK_LEN,
    3 * CHUNK_LEN + 1,
    4 * CHUNK_LEN,
    4 * CHUNK_LEN + 1,
    5 * CHUNK_LEN,
    5 * CHUNK_LEN + 1,
    6 * CHUNK_LEN,
    6 * CHUNK_LEN + 1,
    7 * CHUNK_LEN,
    7 * CHUNK_LEN + 1,
    8 * CHUNK_LEN,
    8 * CHUNK_LEN + 1,
    16 * CHUNK_LEN,  // AVX512's bandwidth
    31 * CHUNK_LEN,  // 16 + 8 + 4 + 2 + 1
    100 * CHUNK_LEN, // subtrees larger than MAX_SIMD_DEGREE chunks
];

pub const TEST_CASES_MAX: usize = 100 * CHUNK_LEN;

// There's a test to make sure these two are equal below.
pub const TEST_KEY: [u8; 32] = *b"whats the Elvish word for friend";
pub const TEST_KEY_WORDS: [u32; 8] = [
    1952540791, 1752440947, 1816469605, 1752394102, 1919907616, 1868963940, 1919295602, 1684956521,
];

// Paint the input with a repeating byte pattern. We use a cycle length of 251,
// because that's the largest prime number less than 256. This makes it
// unlikely to swapping any two adjacent input blocks or chunks will give the
// same answer.
fn paint_test_input(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
}

#[inline(always)]
fn le_bytes_from_words_32(words: &[u32; 8]) -> [u8; 32] {
    let mut out = [0; 32];
    *array_mut_ref!(out, 0 * 4, 4) = words[0].to_le_bytes();
    *array_mut_ref!(out, 1 * 4, 4) = words[1].to_le_bytes();
    *array_mut_ref!(out, 2 * 4, 4) = words[2].to_le_bytes();
    *array_mut_ref!(out, 3 * 4, 4) = words[3].to_le_bytes();
    *array_mut_ref!(out, 4 * 4, 4) = words[4].to_le_bytes();
    *array_mut_ref!(out, 5 * 4, 4) = words[5].to_le_bytes();
    *array_mut_ref!(out, 6 * 4, 4) = words[6].to_le_bytes();
    *array_mut_ref!(out, 7 * 4, 4) = words[7].to_le_bytes();
    out
}

type CompressInPlaceFn =
    unsafe extern "C" fn(cv: *mut u32, block: *const u8, block_len: u8, counter: u64, flags: u8);

type CompressXofFn = unsafe extern "C" fn(
    cv: *const u32,
    block: *const u8,
    block_len: u8,
    counter: u64,
    flags: u8,
    out: *mut u8,
);

// A shared helper function for platform-specific tests.
pub fn test_compress_fn(compress_in_place_fn: CompressInPlaceFn, compress_xof_fn: CompressXofFn) {
    let initial_state = TEST_KEY_WORDS;
    let block_len: u8 = 61;
    let mut block = [0; BLOCK_LEN];
    paint_test_input(&mut block[..block_len as usize]);
    // Use a counter with set bits in both 32-bit words.
    let counter = (5u64 << 32) + 6;
    let flags = CHUNK_END | ROOT | KEYED_HASH;

    let mut portable_out = [0; 64];
    unsafe {
        crate::ffi::blake3_compress_xof_portable(
            initial_state.as_ptr(),
            block.as_ptr(),
            block_len,
            counter,
            flags,
            portable_out.as_mut_ptr(),
        );
    }

    let mut test_state = initial_state;
    unsafe {
        compress_in_place_fn(
            test_state.as_mut_ptr(),
            block.as_ptr(),
            block_len,
            counter,
            flags,
        )
    };
    let test_state_bytes = le_bytes_from_words_32(&test_state);
    let mut test_xof = [0; 64];
    unsafe {
        compress_xof_fn(
            initial_state.as_ptr(),
            block.as_ptr(),
            block_len,
            counter,
            flags,
            test_xof.as_mut_ptr(),
        )
    };

    assert_eq!(&portable_out[..32], &test_state_bytes[..]);
    assert_eq!(&portable_out[..], &test_xof[..]);
}

// Testing the portable implementation against itself is circular, but why not.
#[test]
fn test_compress_portable() {
    test_compress_fn(
        crate::ffi::blake3_compress_in_place_portable,
        crate::ffi::blake3_compress_xof_portable,
    );
}

#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn test_compress_sse2() {
    if !crate::sse2_detected() {
        return;
    }
    test_compress_fn(
        crate::ffi::x86::blake3_compress_in_place_sse2,
        crate::ffi::x86::blake3_compress_xof_sse2,
    );
}

#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn test_compress_sse41() {
    if !crate::sse41_detected() {
        return;
    }
    test_compress_fn(
        crate::ffi::x86::blake3_compress_in_place_sse41,
        crate::ffi::x86::blake3_compress_xof_sse41,
    );
}

#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn test_compress_avx512() {
    if !crate::avx512_detected() {
        return;
    }
    test_compress_fn(
        crate::ffi::x86::blake3_compress_in_place_avx512,
        crate::ffi::x86::blake3_compress_xof_avx512,
    );
}

type HashManyFn = unsafe extern "C" fn(
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

// A shared helper function for platform-specific tests.
pub fn test_hash_many_fn(hash_many_fn: HashManyFn) {
    // Test a few different initial counter values.
    // - 0: The base case.
    // - u32::MAX: The low word of the counter overflows for all inputs except the first.
    // - i32::MAX: *No* overflow. But carry bugs in tricky SIMD code can screw this up, if you XOR
    //   when you're supposed to ANDNOT...
    let initial_counters = [0, u32::MAX as u64, i32::MAX as u64];
    for counter in initial_counters {
        dbg!(counter);

        // 31 (16 + 8 + 4 + 2 + 1) inputs
        const NUM_INPUTS: usize = 31;
        let mut input_buf = [0; CHUNK_LEN * NUM_INPUTS];
        crate::test::paint_test_input(&mut input_buf);

        // First hash chunks.
        let mut chunks = ArrayVec::<&[u8; CHUNK_LEN], NUM_INPUTS>::new();
        for i in 0..NUM_INPUTS {
            chunks.push(array_ref!(input_buf, i * CHUNK_LEN, CHUNK_LEN));
        }
        let mut portable_chunks_out = [0; NUM_INPUTS * OUT_LEN];
        unsafe {
            crate::ffi::blake3_hash_many_portable(
                chunks.as_ptr() as _,
                chunks.len(),
                CHUNK_LEN / BLOCK_LEN,
                TEST_KEY_WORDS.as_ptr(),
                counter,
                true,
                KEYED_HASH,
                CHUNK_START,
                CHUNK_END,
                portable_chunks_out.as_mut_ptr(),
            );
        }

        let mut test_chunks_out = [0; NUM_INPUTS * OUT_LEN];
        unsafe {
            hash_many_fn(
                chunks.as_ptr() as _,
                chunks.len(),
                CHUNK_LEN / BLOCK_LEN,
                TEST_KEY_WORDS.as_ptr(),
                counter,
                true,
                KEYED_HASH,
                CHUNK_START,
                CHUNK_END,
                test_chunks_out.as_mut_ptr(),
            );
        }
        for n in 0..NUM_INPUTS {
            dbg!(n);
            assert_eq!(
                &portable_chunks_out[n * OUT_LEN..][..OUT_LEN],
                &test_chunks_out[n * OUT_LEN..][..OUT_LEN]
            );
        }

        // Then hash parents.
        let mut parents = ArrayVec::<&[u8; 2 * OUT_LEN], NUM_INPUTS>::new();
        for i in 0..NUM_INPUTS {
            parents.push(array_ref!(input_buf, i * 2 * OUT_LEN, 2 * OUT_LEN));
        }
        let mut portable_parents_out = [0; NUM_INPUTS * OUT_LEN];
        unsafe {
            crate::ffi::blake3_hash_many_portable(
                parents.as_ptr() as _,
                parents.len(),
                1,
                TEST_KEY_WORDS.as_ptr(),
                counter,
                false,
                KEYED_HASH | PARENT,
                0,
                0,
                portable_parents_out.as_mut_ptr(),
            );
        }

        let mut test_parents_out = [0; NUM_INPUTS * OUT_LEN];
        unsafe {
            hash_many_fn(
                parents.as_ptr() as _,
                parents.len(),
                1,
                TEST_KEY_WORDS.as_ptr(),
                counter,
                false,
                KEYED_HASH | PARENT,
                0,
                0,
                test_parents_out.as_mut_ptr(),
            );
        }
        for n in 0..NUM_INPUTS {
            dbg!(n);
            assert_eq!(
                &portable_parents_out[n * OUT_LEN..][..OUT_LEN],
                &test_parents_out[n * OUT_LEN..][..OUT_LEN]
            );
        }
    }
}

// Testing the portable implementation against itself is circular, but why not.
#[test]
fn test_hash_many_portable() {
    test_hash_many_fn(crate::ffi::blake3_hash_many_portable);
}

#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn test_hash_many_sse2() {
    if !crate::sse2_detected() {
        return;
    }
    test_hash_many_fn(crate::ffi::x86::blake3_hash_many_sse2);
}

#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn test_hash_many_sse41() {
    if !crate::sse41_detected() {
        return;
    }
    test_hash_many_fn(crate::ffi::x86::blake3_hash_many_sse41);
}

#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn test_hash_many_avx2() {
    if !crate::avx2_detected() {
        return;
    }
    test_hash_many_fn(crate::ffi::x86::blake3_hash_many_avx2);
}

#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn test_hash_many_avx512() {
    if !crate::avx512_detected() {
        return;
    }
    test_hash_many_fn(crate::ffi::x86::blake3_hash_many_avx512);
}

#[test]
#[cfg(feature = "neon")]
fn test_hash_many_neon() {
    test_hash_many_fn(crate::ffi::neon::blake3_hash_many_neon);
}

#[test]
fn test_compare_reference_impl() {
    const OUT: usize = 303; // more than 64, not a multiple of 4
    let mut input_buf = [0; TEST_CASES_MAX];
    paint_test_input(&mut input_buf);
    for &case in TEST_CASES {
        let input = &input_buf[..case];
        dbg!(case);

        // regular
        {
            let mut reference_hasher = reference_impl::Hasher::new();
            reference_hasher.update(input);
            let mut expected_out = [0; OUT];
            reference_hasher.finalize(&mut expected_out);

            let mut test_hasher = crate::Hasher::new();
            test_hasher.update(input);
            let mut test_out = [0; OUT];
            test_hasher.finalize(&mut test_out);
            assert_eq!(test_out[..], expected_out[..]);
        }

        // keyed
        {
            let mut reference_hasher = reference_impl::Hasher::new_keyed(&TEST_KEY);
            reference_hasher.update(input);
            let mut expected_out = [0; OUT];
            reference_hasher.finalize(&mut expected_out);

            let mut test_hasher = crate::Hasher::new_keyed(&TEST_KEY);
            test_hasher.update(input);
            let mut test_out = [0; OUT];
            test_hasher.finalize(&mut test_out);
            assert_eq!(test_out[..], expected_out[..]);
        }

        // derive_key
        {
            let context = "BLAKE3 2019-12-27 16:13:59 example context (not the test vector one)";
            let mut reference_hasher = reference_impl::Hasher::new_derive_key(context);
            reference_hasher.update(input);
            let mut expected_out = [0; OUT];
            reference_hasher.finalize(&mut expected_out);

            // the regular C string API
            let mut test_hasher = crate::Hasher::new_derive_key(context);
            test_hasher.update(input);
            let mut test_out = [0; OUT];
            test_hasher.finalize(&mut test_out);
            assert_eq!(test_out[..], expected_out[..]);

            // the raw bytes API
            let mut test_hasher_raw = crate::Hasher::new_derive_key_raw(context.as_bytes());
            test_hasher_raw.update(input);
            let mut test_out_raw = [0; OUT];
            test_hasher_raw.finalize(&mut test_out_raw);
            assert_eq!(test_out_raw[..], expected_out[..]);
        }
    }
}

fn reference_hash(input: &[u8]) -> [u8; OUT_LEN] {
    let mut hasher = reference_impl::Hasher::new();
    hasher.update(input);
    let mut bytes = [0; OUT_LEN];
    hasher.finalize(&mut bytes);
    bytes.into()
}

#[test]
fn test_compare_update_multiple() {
    // Don't use all the long test cases here, since that's unnecessarily slow
    // in debug mode.
    let mut short_test_cases = TEST_CASES;
    while *short_test_cases.last().unwrap() > 4 * CHUNK_LEN {
        short_test_cases = &short_test_cases[..short_test_cases.len() - 1];
    }
    assert_eq!(*short_test_cases.last().unwrap(), 4 * CHUNK_LEN);

    let mut input_buf = [0; 2 * TEST_CASES_MAX];
    paint_test_input(&mut input_buf);

    for &first_update in short_test_cases {
        dbg!(first_update);
        let first_input = &input_buf[..first_update];
        let mut test_hasher = crate::Hasher::new();
        test_hasher.update(first_input);

        for &second_update in short_test_cases {
            dbg!(second_update);
            let second_input = &input_buf[first_update..][..second_update];
            let total_input = &input_buf[..first_update + second_update];

            // Clone the hasher with first_update bytes already written, so
            // that the next iteration can reuse it.
            let mut test_hasher = test_hasher.clone();
            test_hasher.update(second_input);
            let mut test_out = [0; OUT_LEN];
            test_hasher.finalize(&mut test_out);

            let expected = reference_hash(total_input);
            assert_eq!(expected, test_out);
        }
    }
}

#[test]
fn test_fuzz_hasher() {
    const INPUT_MAX: usize = 4 * CHUNK_LEN;
    let mut input_buf = [0; 3 * INPUT_MAX];
    paint_test_input(&mut input_buf);

    // Don't do too many iterations in debug mode, to keep the tests under a
    // second or so. CI should run tests in release mode also. Provide an
    // environment variable for specifying a larger number of fuzz iterations.
    let num_tests = if cfg!(debug_assertions) { 100 } else { 10_000 };

    // Use a fixed RNG seed for reproducibility.
    let mut rng = rand_chacha::ChaCha8Rng::from_seed([1; 32]);
    for _num_test in 0..num_tests {
        dbg!(_num_test);
        let mut hasher = crate::Hasher::new();
        let mut total_input = 0;
        // For each test, write 3 inputs of random length.
        for _ in 0..3 {
            let input_len = rng.gen_range(0..INPUT_MAX + 1);
            dbg!(input_len);
            let input = &input_buf[total_input..][..input_len];
            hasher.update(input);
            total_input += input_len;
        }
        let expected = reference_hash(&input_buf[..total_input]);
        let mut test_out = [0; 32];
        hasher.finalize(&mut test_out);
        assert_eq!(expected, test_out);
    }
}

#[test]
fn test_finalize_seek() {
    let mut expected = [0; 1000];
    {
        let mut reference_hasher = reference_impl::Hasher::new();
        reference_hasher.update(b"foobarbaz");
        reference_hasher.finalize(&mut expected);
    }

    let mut test_hasher = crate::Hasher::new();
    test_hasher.update(b"foobarbaz");

    let mut out = [0; 103];
    for &seek in &[0, 1, 7, 59, 63, 64, 65, 501, expected.len() - out.len()] {
        dbg!(seek);
        test_hasher.finalize_seek(seek as u64, &mut out);
        assert_eq!(&expected[seek..][..out.len()], &out[..]);
    }
}

#[test]
fn test_reset() {
    {
        let mut hasher = crate::Hasher::new();
        hasher.update(&[42; 3 * CHUNK_LEN + 7]);
        hasher.reset();
        hasher.update(&[42; CHUNK_LEN + 3]);
        let mut output = [0; 32];
        hasher.finalize(&mut output);

        let mut reference_hasher = reference_impl::Hasher::new();
        reference_hasher.update(&[42; CHUNK_LEN + 3]);
        let mut reference_hash = [0; 32];
        reference_hasher.finalize(&mut reference_hash);

        assert_eq!(reference_hash, output);
    }
    {
        let key = &[99; 32];
        let mut hasher = crate::Hasher::new_keyed(key);
        hasher.update(&[42; 3 * CHUNK_LEN + 7]);
        hasher.reset();
        hasher.update(&[42; CHUNK_LEN + 3]);
        let mut output = [0; 32];
        hasher.finalize(&mut output);

        let mut reference_hasher = reference_impl::Hasher::new_keyed(key);
        reference_hasher.update(&[42; CHUNK_LEN + 3]);
        let mut reference_hash = [0; 32];
        reference_hasher.finalize(&mut reference_hash);

        assert_eq!(reference_hash, output);
    }
    {
        let context = "BLAKE3 2020-02-12 10:20:58 reset test";
        let mut hasher = crate::Hasher::new_derive_key(context);
        hasher.update(&[42; 3 * CHUNK_LEN + 7]);
        hasher.reset();
        hasher.update(&[42; CHUNK_LEN + 3]);
        let mut output = [0; 32];
        hasher.finalize(&mut output);

        let mut reference_hasher = reference_impl::Hasher::new_derive_key(context);
        reference_hasher.update(&[42; CHUNK_LEN + 3]);
        let mut reference_hash = [0; 32];
        reference_hasher.finalize(&mut reference_hash);

        assert_eq!(reference_hash, output);
    }
}
