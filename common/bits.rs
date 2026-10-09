//! Integer and bit manipulation: alignment, bit fields and sign extension.

/// Rounds `value` up to a multiple of `align`, which must be zero or a power
/// of two. Zero means "no alignment".
#[inline]
pub fn align_to(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// Rounds `val` up to the next value congruent to `modulus` modulo
/// `align`: the smallest x >= val with x % align == modulus. ld64 places
/// every subsection this way, keeping the offset it had within its
/// section modulo the section's alignment, not merely rounding up to the
/// section's alignment.
pub fn align_to_mod(val: u64, align: u64, modulus: u64) -> u64 {
    debug_assert!(align.is_power_of_two() && modulus < align);
    if val <= modulus { modulus } else { align_to(val - modulus, align) + modulus }
}

/// Rounds `value` down to a multiple of `align`, which must be a power of two.
pub fn align_down(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    value & !(align - 1)
}

/// Returns bit `pos` of `value`.
pub fn bit(value: u64, pos: u32) -> u64 {
    (value >> pos) & 1
}

// Returns bits [hi:lo] of `value`.
#[inline]
pub fn bits(value: u64, hi: u32, lo: u32) -> u64 {
    (value >> lo) & ((1u64 << (hi - lo + 1)) - 1)
}

// Casts `value` to a signed `n`-bit integer.
// For example, sign_extend(x, 32) == x as i32 as i64 for any x.
pub fn sign_extend(value: u64, n: u32) -> i64 {
    ((value << (64 - n)) as i64) >> (64 - n)
}

/// Whether `value` is representable as a signed `n`-bit integer.
pub fn is_int(value: i64, n: u32) -> bool {
    sign_extend(value as u64, n) == value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_extension() {
        assert_eq!(sign_extend(0xff, 8), -1);
        assert_eq!(sign_extend(0x7f, 8), 127);
        assert!(is_int(-128, 8));
        assert!(!is_int(128, 8));
    }
}
