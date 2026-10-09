//! Small helpers shared across the linker.

pub use mold_common::util::{
    align_to, bits, encode_sleb, encode_uleb, is_space, leak_bytes, os_str, read_uleb, sign_extend,
};

/// Rounds `val` up to the next value congruent to `modulus` modulo
/// `align`: the smallest x >= val with x % align == modulus. ld64 places
/// every subsection this way, keeping the offset it had within its
/// section modulo the section's alignment, not merely rounding up to the
/// section's alignment.
pub fn align_to_mod(val: u64, align: u64, modulus: u64) -> u64 {
    debug_assert!(align.is_power_of_two() && modulus < align);
    if val <= modulus { modulus } else { align_to(val - modulus, align) + modulus }
}

// Little-endian reads and writes of the integer at the start of a
// slice: the targets' instructions and relocated fields, and the
// fields of a mergeable dylib's record.
pub use mold_common::endian::{
    read_ul16 as read16, read_ul32 as read32, read_ul64 as read64, write_ul32 as write32,
    write_ul64 as write64,
};

// Little-endian appends of an integer to a buffer: the fields of the
// tables the linker builds in full before writing them out (the chained
// fixups, the unwind info, a DOF section).
pub fn push16(buf: &mut Vec<u8>, val: u16) {
    buf.extend_from_slice(&val.to_le_bytes());
}

pub fn push32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
}

pub fn push64(buf: &mut Vec<u8>, val: u64) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// A sort key that orders byte strings like the strings themselves but
/// settles most comparisons on one integer: the first eight bytes,
/// big-endian, zero-padded. Symbol names cannot contain NULs, so
/// (prefix, name) order equals plain name order. Mach-O sorts its
/// global symbols and export-trie input by name (ELF mold never
/// name-sorts), and mangled names share long prefixes, which makes
/// plain slice comparison the sort's bottleneck.
pub fn name_sort_key(name: &[u8]) -> (u64, &[u8]) {
    let mut p = [0u8; 8];
    let n = name.len().min(8);
    p[..n].copy_from_slice(&name[..n]);
    (u64::from_be_bytes(p), name)
}

/// Writes `value` in unsigned LEB128 encoding at the start of `buf`,
/// returning its length.
pub fn write_uleb(buf: &mut [u8], mut value: u64) -> usize {
    let mut i = 0;
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf[i] = byte;
        i += 1;
        if value == 0 {
            return i;
        }
    }
}

/// The length of `value` in unsigned LEB128 encoding.
pub fn uleb_size(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

/// The bytes of a path, as the file system and Mach-O load commands
/// hold them.
pub fn path_bytes(path: &std::path::Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// A byte string without the white space (see is_space) it starts and
/// ends with.
pub fn trim_space(mut bytes: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = bytes
        && is_space(*first)
    {
        bytes = rest;
    }
    while let [rest @ .., last] = bytes
        && is_space(*last)
    {
        bytes = rest;
    }
    bytes
}

/// The lines of a text file, as str::lines splits a string: at each
/// '\n', a '\r' before it dropped, the last line ended or not. Lists
/// of names (symbols, files) are bytes, whatever their encoding.
pub fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    let n = if text.is_empty() { 0 } else { usize::MAX };
    let text = text.strip_suffix(b"\n").unwrap_or(text);
    let lines = text.split(|&c| c == b'\n').take(n);
    lines.map(|line| line.strip_suffix(b"\r").unwrap_or(line))
}

/// Splits a byte string at the first `sep`, as str::split_once does a
/// string.
pub fn split_once(bytes: &[u8], sep: u8) -> Option<(&[u8], &[u8])> {
    let i = memchr::memchr(sep, bytes)?;
    Some((&bytes[..i], &bytes[i + 1..]))
}

/// Makes room in one of the link's arenas (the symbols, the subsections,
/// ...) for `additional` more elements, with an eighth of the whole to
/// spare when it grows: the auto-link rounds append a few objects' worth
/// to arenas the command line's objects filled exactly, which they would
/// take by copying themselves whole (the 57 MB of symbols and 82 MB of
/// subsections of ASan iTerm2, 10 ms). The room left over costs address
/// space until it is written.
pub fn reserve_arena<T>(v: &mut Vec<T>, additional: usize) {
    if v.capacity() - v.len() < additional {
        v.reserve_exact(additional + (v.len() + additional) / 8);
    }
}

/// Fills `buf` with random bytes from the operating system.
pub fn random_bytes(buf: &mut [u8]) {
    getrandom::fill(buf).unwrap_or_else(|err| crate::fatal!("cannot get random bytes: {err}"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uleb_roundtrip() {
        for &value in &[0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut buf = Vec::new();
            encode_uleb(&mut buf, value);
            assert_eq!(uleb_size(value), buf.len());
            let mut written = [0xff; 10];
            assert_eq!(write_uleb(&mut written, value), buf.len());
            assert_eq!(&written[..buf.len()], buf);
            let mut slice = buf.as_slice();
            assert_eq!(read_uleb(&mut slice), value);
            assert!(slice.is_empty());
        }
    }
}
