//! CityHash64, as libc++'s std::hash of a string computes it: a
//! mergeable record hashes its names with that.
//!
//! This implementation is based on CityHash v1.0.3 at
//! https://github.com/google/cityhash. libc++ differs from it in two
//! places, marked "libc++:" below.

// The license of https://github.com/google/cityhash, from its COPYING:
//
// Copyright (c) 2011 Google, Inc.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

// Some primes between 2^63 and 2^64 for various uses.
const K0: u64 = 0xc3a5_c85c_97cb_3127;
const K1: u64 = 0xb492_b66f_be98_f273;
const K2: u64 = 0x9ae1_6a3b_2f90_404f;
const K3: u64 = 0xc949_d7c7_509e_6557;

fn fetch64(s: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(s[at..at + 8].try_into().unwrap())
}

fn fetch32(s: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(s[at..at + 4].try_into().unwrap())
}

fn shift_mix(val: u64) -> u64 {
    val ^ (val >> 47)
}

/// Hash128to64() of the 128-bit value with `u` as its low half.
fn hash_len_16(u: u64, v: u64) -> u64 {
    // Murmur-inspired hashing.
    const MUL: u64 = 0x9ddf_ea08_eb38_2d69;
    let mut a = (u ^ v).wrapping_mul(MUL);
    a ^= a >> 47;
    let mut b = (v ^ a).wrapping_mul(MUL);
    b ^= b >> 47;
    b.wrapping_mul(MUL)
}

fn hash_len_0_to_16(s: &[u8]) -> u64 {
    let len = s.len();
    if len > 8 {
        let a = fetch64(s, 0);
        let b = fetch64(s, len - 8);
        return hash_len_16(a, b.wrapping_add(len as u64).rotate_right(len as u32)) ^ b;
    }
    if len >= 4 {
        // libc++: `a << 3` is in 32 bits, not 64.
        let a = fetch32(s, 0);
        return hash_len_16((len as u64).wrapping_add((a << 3) as u64), fetch32(s, len - 4) as u64);
    }
    if len > 0 {
        let (a, b, c) = (s[0] as u32, s[len >> 1] as u32, s[len - 1] as u32);
        let y = a.wrapping_add(b << 8);
        let z = (len as u32).wrapping_add(c << 2);
        return shift_mix((y as u64).wrapping_mul(K2) ^ (z as u64).wrapping_mul(K3))
            .wrapping_mul(K2);
    }
    K2
}

// This probably works well for 16-byte strings as well, but it may be overkill
// in that case.
fn hash_len_17_to_32(s: &[u8]) -> u64 {
    let len = s.len();
    let a = fetch64(s, 0).wrapping_mul(K1);
    let b = fetch64(s, 8);
    let c = fetch64(s, len - 8).wrapping_mul(K2);
    let d = fetch64(s, len - 16).wrapping_mul(K0);
    hash_len_16(
        a.wrapping_sub(b).rotate_right(43).wrapping_add(c.rotate_right(30)).wrapping_add(d),
        a.wrapping_add((b ^ K3).rotate_right(20)).wrapping_sub(c).wrapping_add(len as u64),
    )
}

/// A 16-byte hash for s[0] ... s[31], `a` and `b`. Quick and dirty.
/// Callers do best to use "random-looking" values for `a` and `b`.
fn weak_hash_len_32_with_seeds(s: &[u8], a: u64, b: u64) -> (u64, u64) {
    let (w, x, y, z) = (fetch64(s, 0), fetch64(s, 8), fetch64(s, 16), fetch64(s, 24));
    let mut a = a.wrapping_add(w);
    let mut b = b.wrapping_add(a).wrapping_add(z).rotate_right(21);
    let c = a;
    a = a.wrapping_add(x).wrapping_add(y);
    b = b.wrapping_add(a.rotate_right(44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

/// An 8-byte hash for 33 to 64 bytes.
fn hash_len_33_to_64(s: &[u8]) -> u64 {
    let len = s.len();
    let mut z = fetch64(s, 24);
    let mut a = fetch64(s, 0)
        .wrapping_add((len as u64).wrapping_add(fetch64(s, len - 16)).wrapping_mul(K0));
    let mut b = a.wrapping_add(z).rotate_right(52);
    let mut c = a.rotate_right(37);
    a = a.wrapping_add(fetch64(s, 8));
    c = c.wrapping_add(a.rotate_right(7));
    a = a.wrapping_add(fetch64(s, 16));
    let vf = a.wrapping_add(z);
    let vs = b.wrapping_add(a.rotate_right(31)).wrapping_add(c);
    a = fetch64(s, 16).wrapping_add(fetch64(s, len - 32));
    // libc++: z is added to, not set.
    z = z.wrapping_add(fetch64(s, len - 8));
    b = a.wrapping_add(z).rotate_right(52);
    c = a.rotate_right(37);
    a = a.wrapping_add(fetch64(s, len - 24));
    c = c.wrapping_add(a.rotate_right(7));
    a = a.wrapping_add(fetch64(s, len - 16));
    let wf = a.wrapping_add(z);
    let ws = b.wrapping_add(a.rotate_right(31)).wrapping_add(c);
    let r = shift_mix(
        vf.wrapping_add(ws).wrapping_mul(K2).wrapping_add(wf.wrapping_add(vs).wrapping_mul(K0)),
    );
    shift_mix(r.wrapping_mul(K0).wrapping_add(vs)).wrapping_mul(K2)
}

pub(crate) fn hash(s: &[u8]) -> u64 {
    let len = s.len();
    if len <= 16 {
        return hash_len_0_to_16(s);
    }
    if len <= 32 {
        return hash_len_17_to_32(s);
    }
    if len <= 64 {
        return hash_len_33_to_64(s);
    }

    // For strings over 64 bytes we hash the end first, and then as we
    // loop we keep 56 bytes of state: v, w, x, y, and z.
    let n = len as u64;
    let mut x = fetch64(s, len - 40);
    let mut y = fetch64(s, len - 16).wrapping_add(fetch64(s, len - 56));
    let mut z = hash_len_16(fetch64(s, len - 48).wrapping_add(n), fetch64(s, len - 24));
    let mut v = weak_hash_len_32_with_seeds(&s[len - 64..], n, z);
    let mut w = weak_hash_len_32_with_seeds(&s[len - 32..], y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(fetch64(s, 0));

    // Decrease len to the nearest multiple of 64, and operate on 64-byte chunks.
    for c in s[..(len - 1) & !63].as_chunks::<64>().0 {
        x = (x.wrapping_add(y).wrapping_add(v.0).wrapping_add(fetch64(c, 8)))
            .rotate_right(37)
            .wrapping_mul(K1);
        y = (y.wrapping_add(v.1).wrapping_add(fetch64(c, 48))).rotate_right(42).wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(fetch64(c, 40));
        z = z.wrapping_add(w.0).rotate_right(33).wrapping_mul(K1);
        v = weak_hash_len_32_with_seeds(c, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak_hash_len_32_with_seeds(
            &c[32..],
            z.wrapping_add(w.1),
            y.wrapping_add(fetch64(c, 16)),
        );
        std::mem::swap(&mut z, &mut x);
    }
    hash_len_16(
        hash_len_16(v.0, w.0).wrapping_add(shift_mix(y).wrapping_mul(K1)).wrapping_add(z),
        hash_len_16(v.1, w.1).wrapping_add(x),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // std::hash<std::string_view> from Xcode's libc++, of each length's
    // prefix of `data`. Lengths 4 to 8 and 33 to 64 are where libc++
    // differs from CityHash v1.0.3.
    #[test]
    fn matches_libcxx() {
        let data: Vec<u8> =
            (0..400).map(|i: usize| (i * 131 + (i >> 3) * 7 + 0x5a) as u8).collect();
        for (len, expected) in [
            (0, 0x9ae1_6a3b_2f90_404f),
            (3, 0x51d1_83ca_b178_69e7),
            (5, 0x06ab_c9b6_c811_7ca8),
            (8, 0xed1f_6592_0960_3ce9),
            (12, 0x48b7_91b0_fe9b_e040),
            (17, 0x027a_8eab_ea05_2a3f),
            (32, 0x1c21_a4ef_fa7b_3f54),
            (33, 0x6583_3651_60f0_a2e0),
            (50, 0x1e81_10d2_800d_5748),
            (64, 0xd021_ebba_7723_3653),
            (65, 0x73c3_2c4f_c042_52a2),
            (128, 0xe173_4283_984b_218f),
            (129, 0xd017_1b7c_8ed1_4630),
            (400, 0x3683_4df1_0afa_73c8),
        ] {
            assert_eq!(hash(&data[..len]), expected, "len={len}");
        }
    }
}
