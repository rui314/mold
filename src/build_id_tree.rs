use blake3::hazmat::{
    HasherExt, Mode, left_subtree_len, merge_subtrees_non_root, merge_subtrees_root,
};

pub(crate) fn granularity() -> usize {
    static SIZE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| match std::env::var("MOLD_INCREMENTAL_SUBTREE").ok().as_deref() {
        Some("262144") => 256 * 1024,
        Some("1048576") => 1024 * 1024,
        _ => 64 * 1024,
    })
}
pub(crate) fn subtree(bytes: &[u8], offset: usize) -> [u8; 32] {
    blake3::Hasher::new().set_input_offset(offset as u64).update(bytes).finalize_non_root()
}
pub(crate) fn virtual_zero_hash(
    bytes: &[u8],
    start: usize,
    zeros: std::ops::Range<usize>,
) -> [u8; 32] {
    *virtual_zero_hasher(bytes, start, zeros).finalize().as_bytes()
}
pub(crate) fn virtual_zero_subtree(
    bytes: &[u8],
    start: usize,
    zeros: std::ops::Range<usize>,
    offset: usize,
) -> [u8; 32] {
    virtual_zero_hasher_at(bytes, start, zeros, offset).finalize_non_root()
}
fn virtual_zero_hasher(
    bytes: &[u8],
    start: usize,
    zeros: std::ops::Range<usize>,
) -> blake3::Hasher {
    virtual_zero_hasher_at(bytes, start, zeros, 0)
}
fn virtual_zero_hasher_at(
    bytes: &[u8],
    start: usize,
    zeros: std::ops::Range<usize>,
    offset: usize,
) -> blake3::Hasher {
    let mut h = blake3::Hasher::new();
    h.set_input_offset(offset as u64);
    let begin = zeros.start.saturating_sub(start).min(bytes.len());
    let end = zeros.end.saturating_sub(start).min(bytes.len());
    h.update(&bytes[..begin]);
    let zero = [0; 64];
    let mut remaining = end.saturating_sub(begin);
    while remaining != 0 {
        let len = remaining.min(zero.len());
        h.update(&zero[..len]);
        remaining -= len;
    }
    h.update(&bytes[end.max(begin)..]);
    h
}
fn chain(values: &[[u8; 32]], length: usize, granularity: usize) -> [u8; 32] {
    if values.len() == 1 {
        return values[0];
    }
    let left = left_subtree_len(length as u64) as usize;
    let count = left / granularity;
    merge_subtrees_non_root(
        &chain(&values[..count], left, granularity),
        &chain(&values[count..], length - left, granularity),
        Mode::Hash,
    )
}
pub(crate) fn root(bytes: &[u8], values: &[[u8; 32]]) -> [u8; 32] {
    root_at(bytes, values, granularity())
}
fn root_at(bytes: &[u8], values: &[[u8; 32]], granularity: usize) -> [u8; 32] {
    if bytes.len() <= granularity {
        return *blake3::hash(bytes).as_bytes();
    }
    let left = left_subtree_len(bytes.len() as u64) as usize;
    let count = left / granularity;
    *merge_subtrees_root(
        &chain(&values[..count], left, granularity),
        &chain(&values[count..], bytes.len() - left, granularity),
        Mode::Hash,
    )
    .as_bytes()
}
pub(crate) fn hash(bytes: &[u8]) -> ([u8; 32], Vec<[u8; 32]>) {
    hash_at(bytes, granularity())
}
fn hash_at(bytes: &[u8], granularity: usize) -> ([u8; 32], Vec<[u8; 32]>) {
    let values: Vec<_> = bytes
        .chunks(granularity)
        .enumerate()
        .map(|(i, bytes)| subtree(bytes, i * granularity))
        .collect();
    (root_at(bytes, &values, granularity), values)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_zero_matches_physical_zero() {
        for length in [0, 19, 1025, 65536, 65557, 4 * 1024 * 1024] {
            let bytes = vec![91; length];
            for zeros in [0..20, 13..33, 65530..65550, length..length + 20] {
                let mut expected = bytes.clone();
                let begin = zeros.start.min(length);
                let end = zeros.end.min(length);
                expected[begin..end].fill(0);
                assert_eq!(
                    virtual_zero_hash(&bytes, 0, zeros.clone()),
                    *blake3::hash(&expected).as_bytes()
                );
                for (i, b) in bytes.chunks(65536).enumerate() {
                    let start = i * 65536;
                    assert_eq!(
                        virtual_zero_subtree(b, start, zeros.clone(), start),
                        subtree(&expected[start..start + b.len()], start)
                    );
                }
            }
        }
    }
    #[test]
    fn subtree_roots_match_blake3() {
        for granularity in [64usize * 1024, 256 * 1024, 1024 * 1024] {
            for length in [
                0,
                1,
                1024,
                1025,
                granularity - 1,
                granularity,
                granularity + 1,
                2 * granularity,
                2 * granularity + 19,
                4 * 1024 * 1024 - 1,
                4 * 1024 * 1024,
            ] {
                let mut bytes: Vec<_> =
                    (0..length).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
                let (digest, mut values) = hash_at(&bytes, granularity);
                assert_eq!(digest, *blake3::hash(&bytes).as_bytes());
                if length != 0 {
                    let position = length / 2;
                    bytes[position] ^= 83;
                    let index = position / granularity;
                    let start = index * granularity;
                    let end = (start + granularity).min(length);
                    values[index] = subtree(&bytes[start..end], start);
                    assert_eq!(
                        root_at(&bytes, &values, granularity),
                        *blake3::hash(&bytes).as_bytes()
                    );
                }
            }
        }
    }
}
