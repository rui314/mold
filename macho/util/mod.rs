//! Small helpers shared across the linker.

pub(crate) mod cityhash;
pub mod demangle;
pub mod glob;
pub mod perf;
pub(crate) mod siphash;
pub(crate) mod worker_local;

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

// Little-endian reads and writes of the integer at the start of a
// slice: the targets' instructions and relocated fields, and the
// fields of a mergeable dylib's record.
pub fn read16(loc: &[u8]) -> u16 {
    u16::from_le_bytes(loc[..2].try_into().unwrap())
}

pub fn read32(loc: &[u8]) -> u32 {
    u32::from_le_bytes(loc[..4].try_into().unwrap())
}

pub fn read64(loc: &[u8]) -> u64 {
    u64::from_le_bytes(loc[..8].try_into().unwrap())
}

pub fn write32(loc: &mut [u8], val: u32) {
    loc[..4].copy_from_slice(&val.to_le_bytes());
}

pub fn write64(loc: &mut [u8], val: u64) {
    loc[..8].copy_from_slice(&val.to_le_bytes());
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

/// Reads an unsigned LEB128 value, advancing `bytes` past it.
#[inline]
pub fn read_uleb(bytes: &mut &[u8]) -> u64 {
    let mut value = 0;
    let mut shift = 0;
    loop {
        let (&byte, rest) = bytes.split_first().expect("truncated LEB128");
        *bytes = rest;
        if shift < 64 {
            value |= ((byte & 0x7f) as u64) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            return value;
        }
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

/// Whether a byte is white space as isspace() takes it in the C locale.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
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
