//! Small helpers shared across the linker.

pub use mold_common::bits::{align_to, bits, sign_extend};
pub use mold_common::bytes::{is_space, lines, os_str, split_once, trim_space};
pub use mold_common::leb128::{encode_sleb, encode_uleb, read_uleb, uleb_size, write_uleb};
pub use mold_common::mem::leak_bytes;

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

/// The bytes of a path, as the file system and Mach-O load commands
/// hold them.
pub fn path_bytes(path: &std::path::Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
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
