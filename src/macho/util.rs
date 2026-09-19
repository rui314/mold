//! Helpers specific to the Mach-O linker. The ones shared with the ELF
//! linker (alignment, bit fields, LEB128 encoding) live in `crate::util`.

/// Rounds `val` up to the next value congruent to `modulus` modulo
/// `align`: the smallest x >= val with x % align == modulus. ld64
/// places every atom this way, keeping the offset it had within its
/// section modulo the section's alignment, not merely rounding up to
/// the section's alignment.
pub fn align_to_mod(val: u64, align: u64, modulus: u64) -> u64 {
    debug_assert!(align.is_power_of_two() && modulus < align);
    if val <= modulus { modulus } else { crate::util::align_to(val - modulus, align) + modulus }
}

/// Sign-extends a value whose sign bit is bit `n`. Unlike
/// `crate::util::sign_extend`, which takes a field width, this takes the
/// index of the sign bit, as the Mach-O relocation formats describe it.
pub fn sign_extend(val: u64, n: u32) -> i64 {
    ((val << (63 - n)) as i64) >> (63 - n)
}

/// A sort key that orders strings like the strings themselves but
/// settles most comparisons on one integer: the first eight bytes,
/// big-endian, zero-padded. Symbol names cannot contain NULs, so
/// (prefix, name) order equals plain name order. Mach-O sorts its
/// global symbols and export-trie input by name (ELF mold never
/// name-sorts), and mangled names share long prefixes, which makes
/// plain str comparison the sort's bottleneck.
pub fn name_sort_key(name: &str) -> (u64, &str) {
    let b = name.as_bytes();
    let mut p = [0u8; 8];
    let n = b.len().min(8);
    p[..n].copy_from_slice(&b[..n]);
    (u64::from_be_bytes(p), name)
}

/// Matches a symbol-list pattern: literal text with `*` wildcards,
/// the dialect ld64 uses in its various symbol list files.
pub fn glob_match(pat: &str, name: &str) -> bool {
    let mut parts = pat.split('*');
    let first = parts.next().unwrap_or("");
    if !name.starts_with(first) {
        return false;
    }
    let mut pos = first.len();
    let mut rest: Vec<&str> = parts.collect();
    let last = rest.pop();
    for part in rest {
        match name[pos..].find(part) {
            Some(i) => pos = pos + i + part.len(),
            None => return false,
        }
    }
    match last {
        Some(l) => name.len() >= pos + l.len() && name.ends_with(l),
        None => pos == name.len(),
    }
}

/// Computes the SHA-256 hash of `data` into `out`, for code signatures.
pub fn sha256(data: &[u8], out: &mut [u8; 32]) {
    use sha2::Digest;
    out.copy_from_slice(&sha2::Sha256::digest(data));
}

/// A diagnostic spelling only: symbol lookup and output use the
/// original name. Mach-O adds an underscore to the Itanium ABI name.
pub fn demangle(name: &str) -> std::borrow::Cow<'_, str> {
    if crate::error::demangle_enabled()
        && name.starts_with("__Z")
        && let Some(text) = crate::util::demangle::demangle_cpp(&name.as_bytes()[1..])
    {
        return text.into();
    }
    name.into()
}
