#![feature(test)]

extern crate test;

use arrayref::array_ref;
use arrayvec::ArrayVec;
use blake3::platform::{Platform, MAX_SIMD_DEGREE};
use blake3::OUT_LEN;
use blake3::{BLOCK_LEN, CHUNK_LEN};
use rand::prelude::*;
use test::Bencher;

const KIB: usize = 1024;

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
        let mut rng = rand::rng();
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

fn bench_single_compression_fn(b: &mut Bencher, platform: Platform) {
    let mut state = [1u32; 8];
    let mut r = RandomInput::new(b, 64);
    let input = array_ref!(r.get(), 0, 64);
    b.iter(|| platform.compress_in_place(&mut state, input, 64 as u8, 0, 0));
}

#[bench]
fn bench_single_compression_portable(b: &mut Bencher) {
    bench_single_compression_fn(b, Platform::portable());
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_single_compression_sse2(b: &mut Bencher) {
    if let Some(platform) = Platform::sse2() {
        bench_single_compression_fn(b, platform);
    }
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_single_compression_sse41(b: &mut Bencher) {
    if let Some(platform) = Platform::sse41() {
        bench_single_compression_fn(b, platform);
    }
}

#[bench]
#[cfg(blake3_avx512_ffi)]
fn bench_single_compression_avx512(b: &mut Bencher) {
    if let Some(platform) = Platform::avx512() {
        bench_single_compression_fn(b, platform);
    }
}

fn bench_many_chunks_fn(b: &mut Bencher, platform: Platform) {
    let degree = platform.simd_degree();
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
        platform.hash_many(
            &input_arrays[..],
            &[0; 8],
            0,
            blake3::IncrementCounter::Yes,
            0,
            0,
            0,
            &mut out,
        );
    });
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_chunks_sse2(b: &mut Bencher) {
    if let Some(platform) = Platform::sse2() {
        bench_many_chunks_fn(b, platform);
    }
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_chunks_sse41(b: &mut Bencher) {
    if let Some(platform) = Platform::sse41() {
        bench_many_chunks_fn(b, platform);
    }
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_chunks_avx2(b: &mut Bencher) {
    if let Some(platform) = Platform::avx2() {
        bench_many_chunks_fn(b, platform);
    }
}

#[bench]
#[cfg(blake3_avx512_ffi)]
fn bench_many_chunks_avx512(b: &mut Bencher) {
    if let Some(platform) = Platform::avx512() {
        bench_many_chunks_fn(b, platform);
    }
}

#[bench]
#[cfg(blake3_neon)]
fn bench_many_chunks_neon(b: &mut Bencher) {
    bench_many_chunks_fn(b, Platform::neon().unwrap());
}

#[bench]
#[cfg(blake3_wasm32_simd)]
fn bench_many_chunks_wasm(b: &mut Bencher) {
    bench_many_chunks_fn(b, Platform::wasm32_simd().unwrap());
}

// TODO: When we get const generics we can unify this with the chunks code.
fn bench_many_parents_fn(b: &mut Bencher, platform: Platform) {
    let degree = platform.simd_degree();
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
        platform.hash_many(
            &input_arrays[..],
            &[0; 8],
            0,
            blake3::IncrementCounter::No,
            0,
            0,
            0,
            &mut out,
        );
    });
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_parents_sse2(b: &mut Bencher) {
    if let Some(platform) = Platform::sse2() {
        bench_many_parents_fn(b, platform);
    }
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_parents_sse41(b: &mut Bencher) {
    if let Some(platform) = Platform::sse41() {
        bench_many_parents_fn(b, platform);
    }
}

#[bench]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn bench_many_parents_avx2(b: &mut Bencher) {
    if let Some(platform) = Platform::avx2() {
        bench_many_parents_fn(b, platform);
    }
}

#[bench]
#[cfg(blake3_avx512_ffi)]
fn bench_many_parents_avx512(b: &mut Bencher) {
    if let Some(platform) = Platform::avx512() {
        bench_many_parents_fn(b, platform);
    }
}

#[bench]
#[cfg(blake3_neon)]
fn bench_many_parents_neon(b: &mut Bencher) {
    bench_many_parents_fn(b, Platform::neon().unwrap());
}

#[bench]
#[cfg(blake3_wasm32_simd)]
fn bench_many_parents_wasm(b: &mut Bencher) {
    bench_many_parents_fn(b, Platform::wasm32_simd().unwrap());
}

fn bench_atonce(b: &mut Bencher, len: usize) {
    let mut input = RandomInput::new(b, len);
    b.iter(|| blake3::hash(input.get()));
}

#[bench]
fn bench_atonce_0001_block(b: &mut Bencher) {
    bench_atonce(b, BLOCK_LEN);
}

#[bench]
fn bench_atonce_0001_kib(b: &mut Bencher) {
    bench_atonce(b, 1 * KIB);
}

#[bench]
fn bench_atonce_0002_kib(b: &mut Bencher) {
    bench_atonce(b, 2 * KIB);
}

#[bench]
fn bench_atonce_0004_kib(b: &mut Bencher) {
    bench_atonce(b, 4 * KIB);
}

#[bench]
fn bench_atonce_0008_kib(b: &mut Bencher) {
    bench_atonce(b, 8 * KIB);
}

#[bench]
fn bench_atonce_0016_kib(b: &mut Bencher) {
    bench_atonce(b, 16 * KIB);
}

#[bench]
fn bench_atonce_0032_kib(b: &mut Bencher) {
    bench_atonce(b, 32 * KIB);
}

#[bench]
fn bench_atonce_0064_kib(b: &mut Bencher) {
    bench_atonce(b, 64 * KIB);
}

#[bench]
fn bench_atonce_0128_kib(b: &mut Bencher) {
    bench_atonce(b, 128 * KIB);
}

#[bench]
fn bench_atonce_0256_kib(b: &mut Bencher) {
    bench_atonce(b, 256 * KIB);
}

#[bench]
fn bench_atonce_0512_kib(b: &mut Bencher) {
    bench_atonce(b, 512 * KIB);
}

#[bench]
fn bench_atonce_1024_kib(b: &mut Bencher) {
    bench_atonce(b, 1024 * KIB);
}

fn bench_incremental(b: &mut Bencher, len: usize) {
    let mut input = RandomInput::new(b, len);
    b.iter(|| blake3::Hasher::new().update(input.get()).finalize());
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

fn bench_reference(b: &mut Bencher, len: usize) {
    let mut input = RandomInput::new(b, len);
    b.iter(|| {
        let mut hasher = reference_impl::Hasher::new();
        hasher.update(input.get());
        let mut out = [0; 32];
        hasher.finalize(&mut out);
        out
    });
}

#[bench]
fn bench_reference_0001_block(b: &mut Bencher) {
    bench_reference(b, BLOCK_LEN);
}

#[bench]
fn bench_reference_0001_kib(b: &mut Bencher) {
    bench_reference(b, 1 * KIB);
}

#[bench]
fn bench_reference_0002_kib(b: &mut Bencher) {
    bench_reference(b, 2 * KIB);
}

#[bench]
fn bench_reference_0004_kib(b: &mut Bencher) {
    bench_reference(b, 4 * KIB);
}

#[bench]
fn bench_reference_0008_kib(b: &mut Bencher) {
    bench_reference(b, 8 * KIB);
}

#[bench]
fn bench_reference_0016_kib(b: &mut Bencher) {
    bench_reference(b, 16 * KIB);
}

#[bench]
fn bench_reference_0032_kib(b: &mut Bencher) {
    bench_reference(b, 32 * KIB);
}

#[bench]
fn bench_reference_0064_kib(b: &mut Bencher) {
    bench_reference(b, 64 * KIB);
}

#[bench]
fn bench_reference_0128_kib(b: &mut Bencher) {
    bench_reference(b, 128 * KIB);
}

#[bench]
fn bench_reference_0256_kib(b: &mut Bencher) {
    bench_reference(b, 256 * KIB);
}

#[bench]
fn bench_reference_0512_kib(b: &mut Bencher) {
    bench_reference(b, 512 * KIB);
}

#[bench]
fn bench_reference_1024_kib(b: &mut Bencher) {
    bench_reference(b, 1024 * KIB);
}

#[cfg(feature = "rayon")]
fn bench_rayon(b: &mut Bencher, len: usize) {
    let mut input = RandomInput::new(b, len);
    b.iter(|| blake3::Hasher::new().update_rayon(input.get()).finalize());
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0001_block(b: &mut Bencher) {
    bench_rayon(b, BLOCK_LEN);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0001_kib(b: &mut Bencher) {
    bench_rayon(b, 1 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0002_kib(b: &mut Bencher) {
    bench_rayon(b, 2 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0004_kib(b: &mut Bencher) {
    bench_rayon(b, 4 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0008_kib(b: &mut Bencher) {
    bench_rayon(b, 8 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0016_kib(b: &mut Bencher) {
    bench_rayon(b, 16 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0032_kib(b: &mut Bencher) {
    bench_rayon(b, 32 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0064_kib(b: &mut Bencher) {
    bench_rayon(b, 64 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0128_kib(b: &mut Bencher) {
    bench_rayon(b, 128 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0256_kib(b: &mut Bencher) {
    bench_rayon(b, 256 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_0512_kib(b: &mut Bencher) {
    bench_rayon(b, 512 * KIB);
}

#[bench]
#[cfg(feature = "rayon")]
fn bench_rayon_1024_kib(b: &mut Bencher) {
    bench_rayon(b, 1024 * KIB);
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
        let mut hasher = blake3::Hasher::new();
        let input = input.get();
        hasher.update(&input[..1]);
        hasher.update(&input[1..]);
        hasher.finalize()
    });
}

fn bench_xof(b: &mut Bencher, len: usize) {
    b.bytes = len as u64;
    let mut output = [0u8; 64 * BLOCK_LEN];
    let output_slice = &mut output[..len];
    let mut xof = blake3::Hasher::new().finalize_xof();
    b.iter(|| xof.fill(output_slice));
}

#[bench]
fn bench_xof_01_block(b: &mut Bencher) {
    bench_xof(b, 1 * BLOCK_LEN);
}

#[bench]
fn bench_xof_02_blocks(b: &mut Bencher) {
    bench_xof(b, 2 * BLOCK_LEN);
}

#[bench]
fn bench_xof_03_blocks(b: &mut Bencher) {
    bench_xof(b, 3 * BLOCK_LEN);
}

#[bench]
fn bench_xof_04_blocks(b: &mut Bencher) {
    bench_xof(b, 4 * BLOCK_LEN);
}

#[bench]
fn bench_xof_05_blocks(b: &mut Bencher) {
    bench_xof(b, 5 * BLOCK_LEN);
}

#[bench]
fn bench_xof_06_blocks(b: &mut Bencher) {
    bench_xof(b, 6 * BLOCK_LEN);
}

#[bench]
fn bench_xof_07_blocks(b: &mut Bencher) {
    bench_xof(b, 7 * BLOCK_LEN);
}

#[bench]
fn bench_xof_08_blocks(b: &mut Bencher) {
    bench_xof(b, 8 * BLOCK_LEN);
}

#[bench]
fn bench_xof_09_blocks(b: &mut Bencher) {
    bench_xof(b, 9 * BLOCK_LEN);
}

#[bench]
fn bench_xof_10_blocks(b: &mut Bencher) {
    bench_xof(b, 10 * BLOCK_LEN);
}

#[bench]
fn bench_xof_11_blocks(b: &mut Bencher) {
    bench_xof(b, 11 * BLOCK_LEN);
}

#[bench]
fn bench_xof_12_blocks(b: &mut Bencher) {
    bench_xof(b, 12 * BLOCK_LEN);
}

#[bench]
fn bench_xof_13_blocks(b: &mut Bencher) {
    bench_xof(b, 13 * BLOCK_LEN);
}

#[bench]
fn bench_xof_14_blocks(b: &mut Bencher) {
    bench_xof(b, 14 * BLOCK_LEN);
}

#[bench]
fn bench_xof_15_blocks(b: &mut Bencher) {
    bench_xof(b, 15 * BLOCK_LEN);
}

#[bench]
fn bench_xof_16_blocks(b: &mut Bencher) {
    bench_xof(b, 16 * BLOCK_LEN);
}

#[bench]
fn bench_xof_32_blocks(b: &mut Bencher) {
    bench_xof(b, 32 * BLOCK_LEN);
}

#[bench]
fn bench_xof_64_blocks(b: &mut Bencher) {
    bench_xof(b, 64 * BLOCK_LEN);
}
