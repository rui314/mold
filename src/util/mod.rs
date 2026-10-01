//! Small helpers shared across the linker.

pub mod demangle;
pub mod glob;
pub mod perf;
pub(crate) mod siphash;

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

/// Rounds `value` up to a multiple of the page size `page` as ld-prime
/// does, which rounds anything to 0 under a -segalign of 0.
pub fn page_align(value: u64, page: u64) -> u64 {
    let mask = page.wrapping_sub(1);
    value.wrapping_add(mask) & !mask
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

// Returns [hi:lo] bits of val.
#[inline]
pub fn bits(value: u64, hi: u32, lo: u32) -> u64 {
    (value >> lo) & ((1u64 << (hi - lo + 1)) - 1)
}

// Cast val to a signed N bit integer.
// For example, sign_extend(x, 32) == (i32)x for any integer x.
pub fn sign_extend(value: u64, n: u32) -> i64 {
    ((value << (64 - n)) as i64) >> (64 - n)
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

/// Appends `value` in unsigned LEB128 encoding.
pub fn encode_uleb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends `value` in signed LEB128 encoding.
pub fn encode_sleb(out: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let negative = byte & 0x40 != 0;
        if (value == 0 && !negative) || (value == -1 && negative) {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Converts bytes from a response file, a load command or a file list
/// to an OS string. Unix paths can contain arbitrary non-NUL bytes.
pub fn os_str(bytes: &[u8]) -> &std::ffi::OsStr {
    std::os::unix::ffi::OsStrExt::from_bytes(bytes)
}

/// The bytes of a path, as the file system and Mach-O load commands
/// hold them.
pub fn path_bytes(path: &std::path::Path) -> &[u8] {
    std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str())
}

/// Leaks a byte string for the rest of the process's lifetime: names
/// in the output string table outlive every data structure of a link,
/// and the process exits as soon as the link is done.
pub fn leak_bytes(bytes: Vec<u8>) -> &'static [u8] {
    Vec::leak(bytes)
}

/// Fills `buf` with random bytes from the operating system.
pub fn random_bytes(buf: &mut [u8]) {
    getrandom::fill(buf).unwrap_or_else(|err| crate::fatal!("cannot get random bytes: {err}"));
}

/// Formats a byte string for diagnostics, replacing invalid UTF-8.
pub fn display(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// Computes the SHA-256 hash of `data` into `out`.
pub fn sha256(data: &[u8], out: &mut [u8; 32]) {
    use sha2::Digest;
    out.copy_from_slice(&sha2::Sha256::digest(data));
}

/// Computes the SHA-1 hash of `data` into `out`.
pub fn sha1(data: &[u8], out: &mut [u8; 20]) {
    use sha1::Digest;
    out.copy_from_slice(&sha1::Sha1::digest(data));
}
