#![feature(test)]

extern crate test;

use arrayref::array_ref;
use arrayvec::ArrayVec;
use rand::prelude::*;
use test::Bencher;

const KIB: usize = 1024;
const MAX_SIMD_DEGREE: usize = 16;

const BLOCK_LEN: usize = 64;
const CHUNK_LEN: usize = 1024;
const OUT_LEN: usize = 32;

// This struct randomizes two things:
// 1. The actual bytes of input.
// 2. The page offset the input starts at.
pub struct RandomInput {
    buf: Vec<u8>,
    len: usize,
    offsets: Vec<usize>,
    offset_index: usize,
}

impl RandomInput {
    pub fn new(b: &mut Bencher, len: usize) -> Self {
        b.bytes += len as u64;
        let page_size: usize = page_size::get();
        let mut buf = vec![0u8; len + page_size];
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut buf);
        let mut offsets: Vec<usize> = (0..page_size).collect();
        offsets.shuffle(&mut rng);
        Self {
            buf,
            len,
            offsets,
            offset_index: 0,
        }
    }

    pub fn get(&mut self) -> &[u8] {
        let offset = self.offsets[self.offset_index];
        self.offset_index += 1;
        if self.offset_index >= self.offsets.len() {
            self.offset_index = 0;
        }
        &self.buf[offset..][..self.len]
    }
}

type CompressInPlaceFn =
    unsafe extern "C" fn(cv: *mut u32, block: *const u8, block_len: u8, counter: u64, flags: u8);

fn bench_single_compression_fn(b: &mut Bencher, f: CompressInPlaceFn) {
    let mut state = [1u32; 8];
    let mut r = RandomInput::new(b, 64);
    let input = array_ref!(r.get(), 0, 64);
    b.iter(|| unsafe { f(state.as_mut_ptr(), input.as_ptr(), 64, 0, 0) });
}

#[bench]
fn bench_single_compression_portable(b: &mut Bencher) {
    bench_single_compression_fn(
        b,
        blake3_c_rust_bindings::ffi::blake3_compress_in_place_portable,
    );
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_single_compression_sse2(b: &mut Bencher) {
    if !blake3_c_rust_bindings::sse2_detected() {
        return;
    }
    bench_single_compression_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_compress_in_place_sse2,
    );
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_single_compression_sse41(b: &mut Bencher) {
    if !blake3_c_rust_bindings::sse41_detected() {
        return;
    }
    bench_single_compression_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_compress_in_place_sse41,
    );
}

#[bench]
fn bench_single_compression_avx512(b: &mut Bencher) {
    if !blake3_c_rust_bindings::avx512_detected() {
        return;
    }
    bench_single_compression_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_compress_in_place_avx512,
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

fn bench_many_chunks_fn(b: &mut Bencher, f: HashManyFn, degree: usize) {
    let mut inputs = Vec::new();
    for _ in 0..degree {
        inputs.push(RandomInput::new(b, CHUNK_LEN));
    }
    b.iter(|| {
        let input_arrays: ArrayVec<&[u8; CHUNK_LEN], MAX_SIMD_DEGREE> = inputs
            .iter_mut()
            .take(degree)
            .map(|i| array_ref!(i.get(), 0, CHUNK_LEN))
            .collect();
        let mut out = [0; MAX_SIMD_DEGREE * OUT_LEN];
        unsafe {
            f(
                input_arrays.as_ptr() as _,
                input_arrays.len(),
                CHUNK_LEN / BLOCK_LEN,
                [0u32; 8].as_ptr(),
                0,
                true,
                0,
                0,
                0,
                out.as_mut_ptr(),
            )
        }
    });
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_chunks_sse2(b: &mut Bencher) {
    if !blake3_c_rust_bindings::sse2_detected() {
        return;
    }
    bench_many_chunks_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_sse2,
        4,
    );
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_chunks_sse41(b: &mut Bencher) {
    if !blake3_c_rust_bindings::sse41_detected() {
        return;
    }
    bench_many_chunks_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_sse41,
        4,
    );
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_chunks_avx2(b: &mut Bencher) {
    if !blake3_c_rust_bindings::avx2_detected() {
        return;
    }
    bench_many_chunks_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_avx2,
        8,
    );
}

#[bench]
fn bench_many_chunks_avx512(b: &mut Bencher) {
    if !blake3_c_rust_bindings::avx512_detected() {
        return;
    }
    bench_many_chunks_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_avx512,
        16,
    );
}

#[bench]
#[cfg(feature = "neon")]
fn bench_many_chunks_neon(b: &mut Bencher) {
    // When "neon" is on, NEON support is assumed.
    bench_many_chunks_fn(
        b,
        blake3_c_rust_bindings::ffi::neon::blake3_hash_many_neon,
        4,
    );
}

// TODO: When we get const generics we can unify this with the chunks code.
fn bench_many_parents_fn(b: &mut Bencher, f: HashManyFn, degree: usize) {
    let mut inputs = Vec::new();
    for _ in 0..degree {
        inputs.push(RandomInput::new(b, BLOCK_LEN));
    }
    b.iter(|| {
        let input_arrays: ArrayVec<&[u8; BLOCK_LEN], MAX_SIMD_DEGREE> = inputs
            .iter_mut()
            .take(degree)
            .map(|i| array_ref!(i.get(), 0, BLOCK_LEN))
            .collect();
        let mut out = [0; MAX_SIMD_DEGREE * OUT_LEN];
        unsafe {
            f(
                input_arrays.as_ptr() as _,
                input_arrays.len(),
                1,
                [0u32; 8].as_ptr(),
                0,
                false,
                0,
                0,
                0,
                out.as_mut_ptr(),
            )
        }
    });
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_parents_sse2(b: &mut Bencher) {
    if !blake3_c_rust_bindings::sse2_detected() {
        return;
    }
    bench_many_parents_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_sse2,
        4,
    );
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_parents_sse41(b: &mut Bencher) {
    if !blake3_c_rust_bindings::sse41_detected() {
        return;
    }
    bench_many_parents_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_sse41,
        4,
    );
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_parents_avx2(b: &mut Bencher) {
    if !blake3_c_rust_bindings::avx2_detected() {
        return;
    }
    bench_many_parents_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_avx2,
        8,
    );
}

#[bench]
fn bench_many_parents_avx512(b: &mut Bencher) {
    if !blake3_c_rust_bindings::avx512_detected() {
        return;
    }
    bench_many_parents_fn(
        b,
        blake3_c_rust_bindings::ffi::x86::blake3_hash_many_avx512,
        16,
    );
}

#[bench]
#[cfg(feature = "neon")]
fn bench_many_parents_neon(b: &mut Bencher) {
    // When "neon" is on, NEON support is assumed.
    bench_many_parents_fn(
        b,
        blake3_c_rust_bindings::ffi::neon::blake3_hash_many_neon,
        4,
    );
}

fn bench_incremental(b: &mut Bencher, len: usize) {
    let mut input = RandomInput::new(b, len);
    b.iter(|| {
        let mut hasher = blake3_c_rust_bindings::Hasher::new();
        hasher.update(input.get());
        let mut out = [0; 32];
        hasher.finalize(&mut out);
        out
    });
}

#[bench]
fn bench_incremental_0001_block(b: &mut Bencher) {
    bench_incremental(b, BLOCK_LEN);
}

#[bench]
fn bench_incremental_0001_kib(b: &mut Bencher) {
    bench_incremental(b, 1 * KIB);
}

#[bench]
fn bench_incremental_0002_kib(b: &mut Bencher) {
    bench_incremental(b, 2 * KIB);
}

#[bench]
fn bench_incremental_0004_kib(b: &mut Bencher) {
    bench_incremental(b, 4 * KIB);
}

#[bench]
fn bench_incremental_0008_kib(b: &mut Bencher) {
    bench_incremental(b, 8 * KIB);
}

#[bench]
fn bench_incremental_0016_kib(b: &mut Bencher) {
    bench_incremental(b, 16 * KIB);
}

#[bench]
fn bench_incremental_0032_kib(b: &mut Bencher) {
    bench_incremental(b, 32 * KIB);
}

#[bench]
fn bench_incremental_0064_kib(b: &mut Bencher) {
    bench_incremental(b, 64 * KIB);
}

#[bench]
fn bench_incremental_0128_kib(b: &mut Bencher) {
    bench_incremental(b, 128 * KIB);
}

#[bench]
fn bench_incremental_0256_kib(b: &mut Bencher) {
    bench_incremental(b, 256 * KIB);
}

#[bench]
fn bench_incremental_0512_kib(b: &mut Bencher) {
    bench_incremental(b, 512 * KIB);
}

#[bench]
fn bench_incremental_1024_kib(b: &mut Bencher) {
    bench_incremental(b, 1024 * KIB);
}

// This checks that update() splits up its input in increasing powers of 2, so
// that it can recover a high degree of parallelism when the number of bytes
// hashed so far is uneven. The performance of this benchmark should be
// reasonably close to bench_incremental_0064_kib, within 80% or so. When we
// had a bug in this logic (https://github.com/BLAKE3-team/BLAKE3/issues/69),
// performance was less than half.
#[bench]
fn bench_two_updates(b: &mut Bencher) {
    let len = 65536;
    let mut input = RandomInput::new(b, len);
    b.iter(|| {
        let mut hasher = blake3_c_rust_bindings::Hasher::new();
        let input = input.get();
        hasher.update(&input[..1]);
        hasher.update(&input[1..]);
        let mut out = [0; 32];
        hasher.finalize(&mut out);
        out
    });
}
