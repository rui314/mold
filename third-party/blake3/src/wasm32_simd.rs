/*
 * This code is based on rust_sse2.rs of the same distribution, and is subject to further improvements.
 * Some comments are left intact even if their applicability is questioned.
 *
 * Performance measurements with a primitive benchmark with ~16Kb of data:
 *
 * | M1 native     | 11,610 ns |
 * | M1 Wasm SIMD  | 13,355 ns |
 * | M1 Wasm       | 22,037 ns |
 * | x64 native    |  6,713 ns |
 * | x64 Wasm SIMD | 11,985 ns |
 * | x64 Wasm      | 25,978 ns |
 *
 * wasmtime v12.0.1 was used on both platforms.
 */

use core::arch::wasm32::*;

use crate::{
    counter_high, counter_low, CVBytes, CVWords, IncrementCounter, BLOCK_LEN, IV, MSG_SCHEDULE,
    OUT_LEN,
};
use arrayref::{array_mut_ref, array_ref, mut_array_refs};

pub const DEGREE: usize = 4;

#[inline(always)]
unsafe fn loadu(src: *const u8) -> v128 {
    // This is an unaligned load, so the pointer cast is allowed.
    v128_load(src as *const v128)
}

#[inline(always)]
unsafe fn storeu(src: v128, dest: *mut u8) {
    // This is an unaligned store, so the pointer cast is allowed.
    v128_store(dest as *mut v128, src)
}

#[inline(always)]
fn add(a: v128, b: v128) -> v128 {
    i32x4_add(a, b)
}

#[inline(always)]
fn xor(a: v128, b: v128) -> v128 {
    v128_xor(a, b)
}

#[inline(always)]
fn set1(x: u32) -> v128 {
    i32x4_splat(x as i32)
}

#[inline(always)]
fn set4(a: u32, b: u32, c: u32, d: u32) -> v128 {
    i32x4(a as i32, b as i32, c as i32, d as i32)
}

// These rotations are the "simple/shifts version". For the
// "complicated/shuffles version", see
// https://github.com/sneves/blake2-avx2/blob/b3723921f668df09ece52dcd225a36d4a4eea1d9/blake2s-common.h#L63-L66.
// For a discussion of the tradeoffs, see
// https://github.com/sneves/blake2-avx2/pull/5. Due to an LLVM bug
// (https://bugs.llvm.org/show_bug.cgi?id=44379), this version performs better
// on recent x86 chips.
#[inline(always)]
fn rot16(a: v128) -> v128 {
    v128_or(u32x4_shr(a, 16), u32x4_shl(a, 32 - 16))
}

#[inline(always)]
fn rot12(a: v128) -> v128 {
    v128_or(u32x4_shr(a, 12), u32x4_shl(a, 32 - 12))
}

#[inline(always)]
fn rot8(a: v128) -> v128 {
    v128_or(u32x4_shr(a, 8), u32x4_shl(a, 32 - 8))
}

#[inline(always)]
fn rot7(a: v128) -> v128 {
    v128_or(u32x4_shr(a, 7), u32x4_shl(a, 32 - 7))
}

#[inline(always)]
fn g1(row0: &mut v128, row1: &mut v128, row2: &mut v128, row3: &mut v128, m: v128) {
    *row0 = add(add(*row0, m), *row1);
    *row3 = xor(*row3, *row0);
    *row3 = rot16(*row3);
    *row2 = add(*row2, *row3);
    *row1 = xor(*row1, *row2);
    *row1 = rot12(*row1);
}

#[inline(always)]
fn g2(row0: &mut v128, row1: &mut v128, row2: &mut v128, row3: &mut v128, m: v128) {
    *row0 = add(add(*row0, m), *row1);
    *row3 = xor(*row3, *row0);
    *row3 = rot8(*row3);
    *row2 = add(*row2, *row3);
    *row1 = xor(*row1, *row2);
    *row1 = rot7(*row1);
}

// It could be a function, but artimetics in const generics is too limited yet.
macro_rules! shuffle {
    ($a: expr, $b: expr, $z:expr, $y:expr, $x:expr, $w:expr) => {
        i32x4_shuffle::<{ $w }, { $x }, { $y + 4 }, { $z + 4 }>($a, $b)
    };
}

#[inline(always)]
fn unpacklo_epi64(a: v128, b: v128) -> v128 {
    i64x2_shuffle::<0, 2>(a, b)
}

#[inline(always)]
fn unpackhi_epi64(a: v128, b: v128) -> v128 {
    i64x2_shuffle::<1, 3>(a, b)
}

#[inline(always)]
fn unpacklo_epi32(a: v128, b: v128) -> v128 {
    i32x4_shuffle::<0, 4, 1, 5>(a, b)
}

#[inline(always)]
fn unpackhi_epi32(a: v128, b: v128) -> v128 {
    i32x4_shuffle::<2, 6, 3, 7>(a, b)
}

#[inline(always)]
fn shuffle_epi32<const I3: usize, const I2: usize, const I1: usize, const I0: usize>(
    a: v128,
) -> v128 {
    // Please note that generic arguments in delcaration and imlementation are in
    // different order.
    // second arg is actually ignored.
    i32x4_shuffle::<I0, I1, I2, I3>(a, a)
}

#[inline(always)]
fn blend_epi16(a: v128, b: v128, imm8: i32) -> v128 {
    // imm8 is always constant; it allows to implement this function with
    // i16x8_shuffle.  However, it is marginally slower on x64.
    let bits = i16x8(0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80);
    let mut mask = i16x8_splat(imm8 as i16);
    mask = v128_and(mask, bits);
    mask = i16x8_eq(mask, bits);
    // The swapped argument order is equivalent to mask negation.
    v128_bitselect(b, a, mask)
}

// Note the optimization here of leaving row1 as the unrotated row, rather than
// row0. All the message loads below are adjusted to compensate for this. See
// discussion at https://github.com/sneves/blake2-avx2/pull/4
#[inline(always)]
fn diagonalize(row0: &mut v128, row2: &mut v128, row3: &mut v128) {
    *row0 = shuffle_epi32::<2, 1, 0, 3>(*row0);
    *row3 = shuffle_epi32::<1, 0, 3, 2>(*row3);
    *row2 = shuffle_epi32::<0, 3, 2, 1>(*row2);
}

#[inline(always)]
fn undiagonalize(row0: &mut v128, row2: &mut v128, row3: &mut v128) {
    *row0 = shuffle_epi32::<0, 3, 2, 1>(*row0);
    *row3 = shuffle_epi32::<1, 0, 3, 2>(*row3);
    *row2 = shuffle_epi32::<2, 1, 0, 3>(*row2);
}

#[inline(always)]
fn compress_pre(
    cv: &CVWords,
    block: &[u8; BLOCK_LEN],
    block_len: u8,
    counter: u64,
    flags: u8,
) -> [v128; 4] {
    // safe because CVWords is [u32; 8]
    let row0 = &mut unsafe { loadu(cv.as_ptr().add(0) as *const u8) };
    let row1 = &mut unsafe { loadu(cv.as_ptr().add(4) as *const u8) };
    let row2 = &mut set4(IV[0], IV[1], IV[2], IV[3]);
    let row3 = &mut set4(
        counter_low(counter),
        counter_high(counter),
        block_len as u32,
        flags as u32,
    );

    // safe because block is &[u8; 64]
    let mut m0 = unsafe { loadu(block.as_ptr().add(0 * 4 * DEGREE)) };
    let mut m1 = unsafe { loadu(block.as_ptr().add(1 * 4 * DEGREE)) };
    let mut m2 = unsafe { loadu(block.as_ptr().add(2 * 4 * DEGREE)) };
    let mut m3 = unsafe { loadu(block.as_ptr().add(3 * 4 * DEGREE)) };

    let mut t0;
    let mut t1;
    let mut t2;
    let mut t3;
    let mut tt;

    // Round 1. The first round permutes the message words from the original
    // input order, into the groups that get mixed in parallel.
    t0 = shuffle!(m0, m1, 2, 0, 2, 0); //  6  4  2  0
    g1(row0, row1, row2, row3, t0);
    t1 = shuffle!(m0, m1, 3, 1, 3, 1); //  7  5  3  1
    g2(row0, row1, row2, row3, t1);
    diagonalize(row0, row2, row3);
    t2 = shuffle!(m2, m3, 2, 0, 2, 0); // 14 12 10  8
    t2 = shuffle_epi32::<2, 1, 0, 3>(t2); // 12 10  8 14
    g1(row0, row1, row2, row3, t2);
    t3 = shuffle!(m2, m3, 3, 1, 3, 1); // 15 13 11  9
    t3 = shuffle_epi32::<2, 1, 0, 3>(t3); // 13 11  9 15
    g2(row0, row1, row2, row3, t3);
    undiagonalize(row0, row2, row3);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // Round 2. This round and all following rounds apply a fixed permutation
    // to the message words from the round before.
    t0 = shuffle!(m0, m1, 3, 1, 1, 2);
    t0 = shuffle_epi32::<0, 3, 2, 1>(t0);
    g1(row0, row1, row2, row3, t0);
    t1 = shuffle!(m2, m3, 3, 3, 2, 2);
    tt = shuffle_epi32::<0, 0, 3, 3>(m0);
    t1 = blend_epi16(tt, t1, 0xCC);
    g2(row0, row1, row2, row3, t1);
    diagonalize(row0, row2, row3);
    t2 = unpacklo_epi64(m3, m1);
    tt = blend_epi16(t2, m2, 0xC0);
    t2 = shuffle_epi32::<1, 3, 2, 0>(tt);
    g1(row0, row1, row2, row3, t2);
    t3 = unpackhi_epi32(m1, m3);
    tt = unpacklo_epi32(m2, t3);
    t3 = shuffle_epi32::<0, 1, 3, 2>(tt);
    g2(row0, row1, row2, row3, t3);
    undiagonalize(row0, row2, row3);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // Round 3
    t0 = shuffle!(m0, m1, 3, 1, 1, 2);
    t0 = shuffle_epi32::<0, 3, 2, 1>(t0);
    g1(row0, row1, row2, row3, t0);
    t1 = shuffle!(m2, m3, 3, 3, 2, 2);
    tt = shuffle_epi32::<0, 0, 3, 3>(m0);
    t1 = blend_epi16(tt, t1, 0xCC);
    g2(row0, row1, row2, row3, t1);
    diagonalize(row0, row2, row3);
    t2 = unpacklo_epi64(m3, m1);
    tt = blend_epi16(t2, m2, 0xC0);
    t2 = shuffle_epi32::<1, 3, 2, 0>(tt);
    g1(row0, row1, row2, row3, t2);
    t3 = unpackhi_epi32(m1, m3);
    tt = unpacklo_epi32(m2, t3);
    t3 = shuffle_epi32::<0, 1, 3, 2>(tt);
    g2(row0, row1, row2, row3, t3);
    undiagonalize(row0, row2, row3);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // Round 4
    t0 = shuffle!(m0, m1, 3, 1, 1, 2);
    t0 = shuffle_epi32::<0, 3, 2, 1>(t0);
    g1(row0, row1, row2, row3, t0);
    t1 = shuffle!(m2, m3, 3, 3, 2, 2);
    tt = shuffle_epi32::<0, 0, 3, 3>(m0);
    t1 = blend_epi16(tt, t1, 0xCC);
    g2(row0, row1, row2, row3, t1);
    diagonalize(row0, row2, row3);
    t2 = unpacklo_epi64(m3, m1);
    tt = blend_epi16(t2, m2, 0xC0);
    t2 = shuffle_epi32::<1, 3, 2, 0>(tt);
    g1(row0, row1, row2, row3, t2);
    t3 = unpackhi_epi32(m1, m3);
    tt = unpacklo_epi32(m2, t3);
    t3 = shuffle_epi32::<0, 1, 3, 2>(tt);
    g2(row0, row1, row2, row3, t3);
    undiagonalize(row0, row2, row3);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // Round 5
    t0 = shuffle!(m0, m1, 3, 1, 1, 2);
    t0 = shuffle_epi32::<0, 3, 2, 1>(t0);
    g1(row0, row1, row2, row3, t0);
    t1 = shuffle!(m2, m3, 3, 3, 2, 2);
    tt = shuffle_epi32::<0, 0, 3, 3>(m0);
    t1 = blend_epi16(tt, t1, 0xCC);
    g2(row0, row1, row2, row3, t1);
    diagonalize(row0, row2, row3);
    t2 = unpacklo_epi64(m3, m1);
    tt = blend_epi16(t2, m2, 0xC0);
    t2 = shuffle_epi32::<1, 3, 2, 0>(tt);
    g1(row0, row1, row2, row3, t2);
    t3 = unpackhi_epi32(m1, m3);
    tt = unpacklo_epi32(m2, t3);
    t3 = shuffle_epi32::<0, 1, 3, 2>(tt);
    g2(row0, row1, row2, row3, t3);
    undiagonalize(row0, row2, row3);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // Round 6
    t0 = shuffle!(m0, m1, 3, 1, 1, 2);
    t0 = shuffle_epi32::<0, 3, 2, 1>(t0);
    g1(row0, row1, row2, row3, t0);
    t1 = shuffle!(m2, m3, 3, 3, 2, 2);
    tt = shuffle_epi32::<0, 0, 3, 3>(m0);
    t1 = blend_epi16(tt, t1, 0xCC);
    g2(row0, row1, row2, row3, t1);
    diagonalize(row0, row2, row3);
    t2 = unpacklo_epi64(m3, m1);
    tt = blend_epi16(t2, m2, 0xC0);
    t2 = shuffle_epi32::<1, 3, 2, 0>(tt);
    g1(row0, row1, row2, row3, t2);
    t3 = unpackhi_epi32(m1, m3);
    tt = unpacklo_epi32(m2, t3);
    t3 = shuffle_epi32::<0, 1, 3, 2>(tt);
    g2(row0, row1, row2, row3, t3);
    undiagonalize(row0, row2, row3);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // Round 7
    t0 = shuffle!(m0, m1, 3, 1, 1, 2);
    t0 = shuffle_epi32::<0, 3, 2, 1>(t0);
    g1(row0, row1, row2, row3, t0);
    t1 = shuffle!(m2, m3, 3, 3, 2, 2);
    tt = shuffle_epi32::<0, 0, 3, 3>(m0);
    t1 = blend_epi16(tt, t1, 0xCC);
    g2(row0, row1, row2, row3, t1);
    diagonalize(row0, row2, row3);
    t2 = unpacklo_epi64(m3, m1);
    tt = blend_epi16(t2, m2, 0xC0);
    t2 = shuffle_epi32::<1, 3, 2, 0>(tt);
    g1(row0, row1, row2, row3, t2);
    t3 = unpackhi_epi32(m1, m3);
    tt = unpacklo_epi32(m2, t3);
    t3 = shuffle_epi32::<0, 1, 3, 2>(tt);
    g2(row0, row1, row2, row3, t3);
    undiagonalize(row0, row2, row3);

    [*row0, *row1, *row2, *row3]
}

#[target_feature(enable = "simd128")]
pub fn compress_in_place(
    cv: &mut CVWords,
    block: &[u8; BLOCK_LEN],
    block_len: u8,
    counter: u64,
    flags: u8,
) {
    let [row0, row1, row2, row3] = compress_pre(cv, block, block_len, counter, flags);
    // it stores in reversed order...
    // safe because CVWords is [u32; 8]
    unsafe {
        storeu(xor(row0, row2), cv.as_mut_ptr().add(0) as *mut u8);
        storeu(xor(row1, row3), cv.as_mut_ptr().add(4) as *mut u8);
    }
}

#[target_feature(enable = "simd128")]
pub fn compress_xof(
    cv: &CVWords,
    block: &[u8; BLOCK_LEN],
    block_len: u8,
    counter: u64,
    flags: u8,
) -> [u8; 64] {
    let [mut row0, mut row1, mut row2, mut row3] =
        compress_pre(cv, block, block_len, counter, flags);
    row0 = xor(row0, row2);
    row1 = xor(row1, row3);
    // safe because CVWords is [u32; 8]
    row2 = xor(row2, unsafe { loadu(cv.as_ptr().add(0) as *const u8) });
    row3 = xor(row3, unsafe { loadu(cv.as_ptr().add(4) as *const u8) });
    // It seems to be architecture dependent, but works.
    // safe because sizes match, and every state of u8 is valid.
    unsafe { core::mem::transmute([row0, row1, row2, row3]) }
}

#[inline(always)]
fn round(v: &mut [v128; 16], m: &[v128; 16], r: usize) {
    v[0] = add(v[0], m[MSG_SCHEDULE[r][0] as usize]);
    v[1] = add(v[1], m[MSG_SCHEDULE[r][2] as usize]);
    v[2] = add(v[2], m[MSG_SCHEDULE[r][4] as usize]);
    v[3] = add(v[3], m[MSG_SCHEDULE[r][6] as usize]);
    v[0] = add(v[0], v[4]);
    v[1] = add(v[1], v[5]);
    v[2] = add(v[2], v[6]);
    v[3] = add(v[3], v[7]);
    v[12] = xor(v[12], v[0]);
    v[13] = xor(v[13], v[1]);
    v[14] = xor(v[14], v[2]);
    v[15] = xor(v[15], v[3]);
    v[12] = rot16(v[12]);
    v[13] = rot16(v[13]);
    v[14] = rot16(v[14]);
    v[15] = rot16(v[15]);
    v[8] = add(v[8], v[12]);
    v[9] = add(v[9], v[13]);
    v[10] = add(v[10], v[14]);
    v[11] = add(v[11], v[15]);
    v[4] = xor(v[4], v[8]);
    v[5] = xor(v[5], v[9]);
    v[6] = xor(v[6], v[10]);
    v[7] = xor(v[7], v[11]);
    v[4] = rot12(v[4]);
    v[5] = rot12(v[5]);
    v[6] = rot12(v[6]);
    v[7] = rot12(v[7]);
    v[0] = add(v[0], m[MSG_SCHEDULE[r][1] as usize]);
    v[1] = add(v[1], m[MSG_SCHEDULE[r][3] as usize]);
    v[2] = add(v[2], m[MSG_SCHEDULE[r][5] as usize]);
    v[3] = add(v[3], m[MSG_SCHEDULE[r][7] as usize]);
    v[0] = add(v[0], v[4]);
    v[1] = add(v[1], v[5]);
    v[2] = add(v[2], v[6]);
    v[3] = add(v[3], v[7]);
    v[12] = xor(v[12], v[0]);
    v[13] = xor(v[13], v[1]);
    v[14] = xor(v[14], v[2]);
    v[15] = xor(v[15], v[3]);
    v[12] = rot8(v[12]);
    v[13] = rot8(v[13]);
    v[14] = rot8(v[14]);
    v[15] = rot8(v[15]);
    v[8] = add(v[8], v[12]);
    v[9] = add(v[9], v[13]);
    v[10] = add(v[10], v[14]);
    v[11] = add(v[11], v[15]);
    v[4] = xor(v[4], v[8]);
    v[5] = xor(v[5], v[9]);
    v[6] = xor(v[6], v[10]);
    v[7] = xor(v[7], v[11]);
    v[4] = rot7(v[4]);
    v[5] = rot7(v[5]);
    v[6] = rot7(v[6]);
    v[7] = rot7(v[7]);

    v[0] = add(v[0], m[MSG_SCHEDULE[r][8] as usize]);
    v[1] = add(v[1], m[MSG_SCHEDULE[r][10] as usize]);
    v[2] = add(v[2], m[MSG_SCHEDULE[r][12] as usize]);
    v[3] = add(v[3], m[MSG_SCHEDULE[r][14] as usize]);
    v[0] = add(v[0], v[5]);
    v[1] = add(v[1], v[6]);
    v[2] = add(v[2], v[7]);
    v[3] = add(v[3], v[4]);
    v[15] = xor(v[15], v[0]);
    v[12] = xor(v[12], v[1]);
    v[13] = xor(v[13], v[2]);
    v[14] = xor(v[14], v[3]);
    v[15] = rot16(v[15]);
    v[12] = rot16(v[12]);
    v[13] = rot16(v[13]);
    v[14] = rot16(v[14]);
    v[10] = add(v[10], v[15]);
    v[11] = add(v[11], v[12]);
    v[8] = add(v[8], v[13]);
    v[9] = add(v[9], v[14]);
    v[5] = xor(v[5], v[10]);
    v[6] = xor(v[6], v[11]);
    v[7] = xor(v[7], v[8]);
    v[4] = xor(v[4], v[9]);
    v[5] = rot12(v[5]);
    v[6] = rot12(v[6]);
    v[7] = rot12(v[7]);
    v[4] = rot12(v[4]);
    v[0] = add(v[0], m[MSG_SCHEDULE[r][9] as usize]);
    v[1] = add(v[1], m[MSG_SCHEDULE[r][11] as usize]);
    v[2] = add(v[2], m[MSG_SCHEDULE[r][13] as usize]);
    v[3] = add(v[3], m[MSG_SCHEDULE[r][15] as usize]);
    v[0] = add(v[0], v[5]);
    v[1] = add(v[1], v[6]);
    v[2] = add(v[2], v[7]);
    v[3] = add(v[3], v[4]);
    v[15] = xor(v[15], v[0]);
    v[12] = xor(v[12], v[1]);
    v[13] = xor(v[13], v[2]);
    v[14] = xor(v[14], v[3]);
    v[15] = rot8(v[15]);
    v[12] = rot8(v[12]);
    v[13] = rot8(v[13]);
    v[14] = rot8(v[14]);
    v[10] = add(v[10], v[15]);
    v[11] = add(v[11], v[12]);
    v[8] = add(v[8], v[13]);
    v[9] = add(v[9], v[14]);
    v[5] = xor(v[5], v[10]);
    v[6] = xor(v[6], v[11]);
    v[7] = xor(v[7], v[8]);
    v[4] = xor(v[4], v[9]);
    v[5] = rot7(v[5]);
    v[6] = rot7(v[6]);
    v[7] = rot7(v[7]);
    v[4] = rot7(v[4]);
}

#[inline(always)]
fn transpose_vecs(vecs: &mut [v128; DEGREE]) {
    // Interleave 32-bit lanes. The low unpack is lanes 00/11 and the high is
    // 22/33. Note that this doesn't split the vector into two lanes, as the
    // AVX2 counterparts do.
    let ab_01 = unpacklo_epi32(vecs[0], vecs[1]);
    let ab_23 = unpackhi_epi32(vecs[0], vecs[1]);
    let cd_01 = unpacklo_epi32(vecs[2], vecs[3]);
    let cd_23 = unpackhi_epi32(vecs[2], vecs[3]);

    // Interleave 64-bit lanes.
    let abcd_0 = unpacklo_epi64(ab_01, cd_01);
    let abcd_1 = unpackhi_epi64(ab_01, cd_01);
    let abcd_2 = unpacklo_epi64(ab_23, cd_23);
    let abcd_3 = unpackhi_epi64(ab_23, cd_23);

    vecs[0] = abcd_0;
    vecs[1] = abcd_1;
    vecs[2] = abcd_2;
    vecs[3] = abcd_3;
}

#[inline(always)]
unsafe fn transpose_msg_vecs(inputs: &[*const u8; DEGREE], block_offset: usize) -> [v128; 16] {
    let mut vecs = [
        loadu(inputs[0].add(block_offset + 0 * 4 * DEGREE)),
        loadu(inputs[1].add(block_offset + 0 * 4 * DEGREE)),
        loadu(inputs[2].add(block_offset + 0 * 4 * DEGREE)),
        loadu(inputs[3].add(block_offset + 0 * 4 * DEGREE)),
        loadu(inputs[0].add(block_offset + 1 * 4 * DEGREE)),
        loadu(inputs[1].add(block_offset + 1 * 4 * DEGREE)),
        loadu(inputs[2].add(block_offset + 1 * 4 * DEGREE)),
        loadu(inputs[3].add(block_offset + 1 * 4 * DEGREE)),
        loadu(inputs[0].add(block_offset + 2 * 4 * DEGREE)),
        loadu(inputs[1].add(block_offset + 2 * 4 * DEGREE)),
        loadu(inputs[2].add(block_offset + 2 * 4 * DEGREE)),
        loadu(inputs[3].add(block_offset + 2 * 4 * DEGREE)),
        loadu(inputs[0].add(block_offset + 3 * 4 * DEGREE)),
        loadu(inputs[1].add(block_offset + 3 * 4 * DEGREE)),
        loadu(inputs[2].add(block_offset + 3 * 4 * DEGREE)),
        loadu(inputs[3].add(block_offset + 3 * 4 * DEGREE)),
    ];
    let squares = mut_array_refs!(&mut vecs, DEGREE, DEGREE, DEGREE, DEGREE);
    transpose_vecs(squares.0);
    transpose_vecs(squares.1);
    transpose_vecs(squares.2);
    transpose_vecs(squares.3);
    vecs
}

#[inline(always)]
fn load_counters(counter: u64, increment_counter: IncrementCounter) -> (v128, v128) {
    let mask = if increment_counter.yes() { !0 } else { 0 };
    (
        set4(
            counter_low(counter + (mask & 0)),
            counter_low(counter + (mask & 1)),
            counter_low(counter + (mask & 2)),
            counter_low(counter + (mask & 3)),
        ),
        set4(
            counter_high(counter + (mask & 0)),
            counter_high(counter + (mask & 1)),
            counter_high(counter + (mask & 2)),
            counter_high(counter + (mask & 3)),
        ),
    )
}

#[target_feature(enable = "simd128")]
pub unsafe fn hash4(
    inputs: &[*const u8; DEGREE],
    blocks: usize,
    key: &CVWords,
    counter: u64,
    increment_counter: IncrementCounter,
    flags: u8,
    flags_start: u8,
    flags_end: u8,
    out: &mut [u8; DEGREE * OUT_LEN],
) {
    let mut h_vecs = [
        set1(key[0]),
        set1(key[1]),
        set1(key[2]),
        set1(key[3]),
        set1(key[4]),
        set1(key[5]),
        set1(key[6]),
        set1(key[7]),
    ];
    let (counter_low_vec, counter_high_vec) = load_counters(counter, increment_counter);
    let mut block_flags = flags | flags_start;

    for block in 0..blocks {
        if block + 1 == blocks {
            block_flags |= flags_end;
        }
        let block_len_vec = set1(BLOCK_LEN as u32); // full blocks only
        let block_flags_vec = set1(block_flags as u32);
        let msg_vecs = transpose_msg_vecs(inputs, block * BLOCK_LEN);

        // The transposed compression function. Note that inlining this
        // manually here improves compile times by a lot, compared to factoring
        // it out into its own function and making it #[inline(always)]. Just
        // guessing, it might have something to do with loop unrolling.
        let mut v = [
            h_vecs[0],
            h_vecs[1],
            h_vecs[2],
            h_vecs[3],
            h_vecs[4],
            h_vecs[5],
            h_vecs[6],
            h_vecs[7],
            set1(IV[0]),
            set1(IV[1]),
            set1(IV[2]),
            set1(IV[3]),
            counter_low_vec,
            counter_high_vec,
            block_len_vec,
            block_flags_vec,
        ];
        round(&mut v, &msg_vecs, 0);
        round(&mut v, &msg_vecs, 1);
        round(&mut v, &msg_vecs, 2);
        round(&mut v, &msg_vecs, 3);
        round(&mut v, &msg_vecs, 4);
        round(&mut v, &msg_vecs, 5);
        round(&mut v, &msg_vecs, 6);
        h_vecs[0] = xor(v[0], v[8]);
        h_vecs[1] = xor(v[1], v[9]);
        h_vecs[2] = xor(v[2], v[10]);
        h_vecs[3] = xor(v[3], v[11]);
        h_vecs[4] = xor(v[4], v[12]);
        h_vecs[5] = xor(v[5], v[13]);
        h_vecs[6] = xor(v[6], v[14]);
        h_vecs[7] = xor(v[7], v[15]);

        block_flags = flags;
    }

    let squares = mut_array_refs!(&mut h_vecs, DEGREE, DEGREE);
    transpose_vecs(squares.0);
    transpose_vecs(squares.1);
    // The first four vecs now contain the first half of each output, and the
    // second four vecs contain the second half of each output.
    storeu(h_vecs[0], out.as_mut_ptr().add(0 * 4 * DEGREE));
    storeu(h_vecs[4], out.as_mut_ptr().add(1 * 4 * DEGREE));
    storeu(h_vecs[1], out.as_mut_ptr().add(2 * 4 * DEGREE));
    storeu(h_vecs[5], out.as_mut_ptr().add(3 * 4 * DEGREE));
    storeu(h_vecs[2], out.as_mut_ptr().add(4 * 4 * DEGREE));
    storeu(h_vecs[6], out.as_mut_ptr().add(5 * 4 * DEGREE));
    storeu(h_vecs[3], out.as_mut_ptr().add(6 * 4 * DEGREE));
    storeu(h_vecs[7], out.as_mut_ptr().add(7 * 4 * DEGREE));
}

#[target_feature(enable = "simd128")]
unsafe fn hash1<const N: usize>(
    input: &[u8; N],
    key: &CVWords,
    counter: u64,
    flags: u8,
    flags_start: u8,
    flags_end: u8,
    out: &mut CVBytes,
) {
    debug_assert_eq!(N % BLOCK_LEN, 0, "uneven blocks");
    let mut cv = *key;
    let mut block_flags = flags | flags_start;
    let mut slice = &input[..];
    while slice.len() >= BLOCK_LEN {
        if slice.len() == BLOCK_LEN {
            block_flags |= flags_end;
        }
        compress_in_place(
            &mut cv,
            array_ref!(slice, 0, BLOCK_LEN),
            BLOCK_LEN as u8,
            counter,
            block_flags,
        );
        block_flags = flags;
        slice = &slice[BLOCK_LEN..];
    }
    *out = core::mem::transmute(cv);
}

#[target_feature(enable = "simd128")]
pub unsafe fn hash_many<const N: usize>(
    mut inputs: &[&[u8; N]],
    key: &CVWords,
    mut counter: u64,
    increment_counter: IncrementCounter,
    flags: u8,
    flags_start: u8,
    flags_end: u8,
    mut out: &mut [u8],
) {
    debug_assert!(out.len() >= inputs.len() * OUT_LEN, "out too short");
    while inputs.len() >= DEGREE && out.len() >= DEGREE * OUT_LEN {
        // Safe because the layout of arrays is guaranteed, and because the
        // `blocks` count is determined statically from the argument type.
        let input_ptrs: &[*const u8; DEGREE] = &*(inputs.as_ptr() as *const [*const u8; DEGREE]);
        let blocks = N / BLOCK_LEN;
        hash4(
            input_ptrs,
            blocks,
            key,
            counter,
            increment_counter,
            flags,
            flags_start,
            flags_end,
            array_mut_ref!(out, 0, DEGREE * OUT_LEN),
        );
        if increment_counter.yes() {
            counter += DEGREE as u64;
        }
        inputs = &inputs[DEGREE..];
        out = &mut out[DEGREE * OUT_LEN..];
    }
    for (&input, output) in inputs.iter().zip(out.chunks_exact_mut(OUT_LEN)) {
        hash1(
            input,
            key,
            counter,
            flags,
            flags_start,
            flags_end,
            array_mut_ref!(output, 0, OUT_LEN),
        );
        if increment_counter.yes() {
            counter += 1;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_transpose() {
        #[target_feature(enable = "simd128")]
        fn transpose_wrapper(vecs: &mut [v128; DEGREE]) {
            transpose_vecs(vecs);
        }

        let mut matrix = [[0 as u32; DEGREE]; DEGREE];
        for i in 0..DEGREE {
            for j in 0..DEGREE {
                matrix[i][j] = (i * DEGREE + j) as u32;
            }
        }

        unsafe {
            let mut vecs: [v128; DEGREE] = core::mem::transmute(matrix);
            transpose_wrapper(&mut vecs);
            matrix = core::mem::transmute(vecs);
        }

        for i in 0..DEGREE {
            for j in 0..DEGREE {
                // Reversed indexes from above.
                assert_eq!(matrix[j][i], (i * DEGREE + j) as u32);
            }
        }
    }

    #[test]
    fn test_compress() {
        crate::test::test_compress_fn(compress_in_place, compress_xof);
    }

    #[test]
    fn test_hash_many() {
        crate::test::test_hash_many_fn(hash_many, hash_many);
    }
}
