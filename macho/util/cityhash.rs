//! libc++'s std::hash of a string, which a mergeable record hashes the
//! names with: its CityHash64, __murmur2_or_cityhash<size_t, 64>, in
//! the ABI that hashes 4 to 8 bytes with a 32-bit shift. The functions
//! are libc++'s, by name.

const K0: u64 = 0xc3a5_c85c_97cb_3127;
const K1: u64 = 0xb492_b66f_be98_f273;
const K2: u64 = 0x9ae1_6a3b_2f90_404f;
const K3: u64 = 0xc949_d7c7_509e_6557;

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

    // A longer string's end first, then its 64-byte chunks, with 56
    // bytes of state: v, w, x, y and z.
    let n = len as u64;
    let mut x = load64(s, len - 40);
    let mut y = load64(s, len - 16).wrapping_add(load64(s, len - 56));
    let mut z = hash_len_16(load64(s, len - 48).wrapping_add(n), load64(s, len - 24));
    let mut v = weak_hash_len_32_with_seeds(&s[len - 64..], n, z);
    let mut w = weak_hash_len_32_with_seeds(&s[len - 32..], y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(load64(s, 0));
    for c in s[..(len - 1) & !63].as_chunks::<64>().0 {
        x = (x.wrapping_add(y).wrapping_add(v.0).wrapping_add(load64(c, 8)))
            .rotate_right(37)
            .wrapping_mul(K1);
        y = (y.wrapping_add(v.1).wrapping_add(load64(c, 48))).rotate_right(42).wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(load64(c, 40));
        z = z.wrapping_add(w.0).rotate_right(33).wrapping_mul(K1);
        v = weak_hash_len_32_with_seeds(c, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        let seed = y.wrapping_add(load64(c, 16));
        w = weak_hash_len_32_with_seeds(&c[32..], z.wrapping_add(w.1), seed);
        std::mem::swap(&mut z, &mut x);
    }
    hash_len_16(
        hash_len_16(v.0, w.0).wrapping_add(shift_mix(y).wrapping_mul(K1)).wrapping_add(z),
        hash_len_16(v.1, w.1).wrapping_add(x),
    )
}

fn load64(s: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(s[at..at + 8].try_into().unwrap())
}

fn load32(s: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(s[at..at + 4].try_into().unwrap())
}

fn shift_mix(val: u64) -> u64 {
    val ^ (val >> 47)
}

fn hash_len_16(u: u64, v: u64) -> u64 {
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
        let a = load64(s, 0);
        let b = load64(s, len - 8);
        return hash_len_16(a, b.wrapping_add(len as u64).rotate_right(len as u32)) ^ b;
    }
    if len >= 4 {
        let a = load32(s, 0);
        let b = load32(s, len - 4);
        // `a << 3` in 32 bits: this ABI's.
        return hash_len_16((len as u64).wrapping_add((a << 3) as u64), b as u64);
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

fn hash_len_17_to_32(s: &[u8]) -> u64 {
    let len = s.len();
    let a = load64(s, 0).wrapping_mul(K1);
    let b = load64(s, 8);
    let c = load64(s, len - 8).wrapping_mul(K2);
    let d = load64(s, len - 16).wrapping_mul(K0);
    hash_len_16(
        a.wrapping_sub(b).rotate_right(43).wrapping_add(c.rotate_right(30)).wrapping_add(d),
        a.wrapping_add((b ^ K3).rotate_right(20)).wrapping_sub(c).wrapping_add(len as u64),
    )
}

/// A 16-byte hash of the 32 bytes at `s` and the seeds `a` and `b`.
fn weak_hash_len_32_with_seeds(s: &[u8], a: u64, b: u64) -> (u64, u64) {
    let (w, x, y, z) = (load64(s, 0), load64(s, 8), load64(s, 16), load64(s, 24));
    let mut a = a.wrapping_add(w);
    let mut b = b.wrapping_add(a).wrapping_add(z).rotate_right(21);
    let c = a;
    a = a.wrapping_add(x).wrapping_add(y);
    b = b.wrapping_add(a.rotate_right(44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

fn hash_len_33_to_64(s: &[u8]) -> u64 {
    let len = s.len();
    let mut z = load64(s, 24);
    let mut a =
        load64(s, 0).wrapping_add((len as u64).wrapping_add(load64(s, len - 16)).wrapping_mul(K0));
    let mut b = a.wrapping_add(z).rotate_right(52);
    let mut c = a.rotate_right(37);
    a = a.wrapping_add(load64(s, 8));
    c = c.wrapping_add(a.rotate_right(7));
    a = a.wrapping_add(load64(s, 16));
    let vf = a.wrapping_add(z);
    let vs = b.wrapping_add(a.rotate_right(31)).wrapping_add(c);
    a = load64(s, 16).wrapping_add(load64(s, len - 32));
    z = z.wrapping_add(load64(s, len - 8));
    b = a.wrapping_add(z).rotate_right(52);
    c = a.rotate_right(37);
    a = a.wrapping_add(load64(s, len - 24));
    c = c.wrapping_add(a.rotate_right(7));
    a = a.wrapping_add(load64(s, len - 16));
    let wf = a.wrapping_add(z);
    let ws = b.wrapping_add(a.rotate_right(31)).wrapping_add(c);
    let r = shift_mix(
        vf.wrapping_add(ws).wrapping_mul(K2).wrapping_add(wf.wrapping_add(vs).wrapping_mul(K0)),
    );
    shift_mix(r.wrapping_mul(K0).wrapping_add(vs)).wrapping_mul(K2)
}
