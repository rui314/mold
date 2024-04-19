use crate::{
    le_bytes_from_words_32, le_bytes_from_words_64, words_from_le_bytes_32, words_from_le_bytes_64,
    BlockBytes, BlockWords, CVBytes, CVWords, Implementation, IV, MAX_SIMD_DEGREE, MSG_SCHEDULE,
};

const DEGREE: usize = MAX_SIMD_DEGREE;

unsafe extern "C" fn degree() -> usize {
    DEGREE
}

#[inline(always)]
fn g(state: &mut BlockWords, a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(x);
    state[d] = (state[d] ^ state[a]).rotate_right(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(12);
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(y);
    state[d] = (state[d] ^ state[a]).rotate_right(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(7);
}

#[inline(always)]
fn round(state: &mut [u32; 16], msg: &BlockWords, round: usize) {
    // Select the message schedule based on the round.
    let schedule = MSG_SCHEDULE[round];

    // Mix the columns.
    g(state, 0, 4, 8, 12, msg[schedule[0]], msg[schedule[1]]);
    g(state, 1, 5, 9, 13, msg[schedule[2]], msg[schedule[3]]);
    g(state, 2, 6, 10, 14, msg[schedule[4]], msg[schedule[5]]);
    g(state, 3, 7, 11, 15, msg[schedule[6]], msg[schedule[7]]);

    // Mix the diagonals.
    g(state, 0, 5, 10, 15, msg[schedule[8]], msg[schedule[9]]);
    g(state, 1, 6, 11, 12, msg[schedule[10]], msg[schedule[11]]);
    g(state, 2, 7, 8, 13, msg[schedule[12]], msg[schedule[13]]);
    g(state, 3, 4, 9, 14, msg[schedule[14]], msg[schedule[15]]);
}

#[inline(always)]
fn compress_inner(
    block_words: &BlockWords,
    block_len: u32,
    cv_words: &CVWords,
    counter: u64,
    flags: u32,
) -> [u32; 16] {
    let mut state = [
        cv_words[0],
        cv_words[1],
        cv_words[2],
        cv_words[3],
        cv_words[4],
        cv_words[5],
        cv_words[6],
        cv_words[7],
        IV[0],
        IV[1],
        IV[2],
        IV[3],
        counter as u32,
        (counter >> 32) as u32,
        block_len as u32,
        flags as u32,
    ];
    for round_number in 0..7 {
        round(&mut state, &block_words, round_number);
    }
    state
}

pub(crate) unsafe extern "C" fn compress(
    block: *const BlockBytes,
    block_len: u32,
    cv: *const CVBytes,
    counter: u64,
    flags: u32,
    out: *mut CVBytes,
) {
    let block_words = words_from_le_bytes_64(&*block);
    let cv_words = words_from_le_bytes_32(&*cv);
    let mut state = compress_inner(&block_words, block_len, &cv_words, counter, flags);
    for word_index in 0..8 {
        state[word_index] ^= state[word_index + 8];
    }
    *out = le_bytes_from_words_32(state[..8].try_into().unwrap());
}

pub(crate) unsafe extern "C" fn compress_xof(
    block: *const BlockBytes,
    block_len: u32,
    cv: *const CVBytes,
    counter: u64,
    flags: u32,
    out: *mut BlockBytes,
) {
    let block_words = words_from_le_bytes_64(&*block);
    let cv_words = words_from_le_bytes_32(&*cv);
    let mut state = compress_inner(&block_words, block_len, &cv_words, counter, flags);
    for word_index in 0..8 {
        state[word_index] ^= state[word_index + 8];
        state[word_index + 8] ^= cv_words[word_index];
    }
    *out = le_bytes_from_words_64(&state);
}

pub(crate) unsafe extern "C" fn hash_chunks(
    input: *const u8,
    input_len: usize,
    key: *const CVBytes,
    counter: u64,
    flags: u32,
    transposed_output: *mut u32,
) {
    crate::hash_chunks_using_compress(
        compress,
        input,
        input_len,
        key,
        counter,
        flags,
        transposed_output,
    )
}

pub(crate) unsafe extern "C" fn hash_parents(
    transposed_input: *const u32,
    num_parents: usize,
    key: *const CVBytes,
    flags: u32,
    transposed_output: *mut u32, // may overlap the input
) {
    crate::hash_parents_using_compress(
        compress,
        transposed_input,
        num_parents,
        key,
        flags,
        transposed_output,
    )
}

pub(crate) unsafe extern "C" fn xof(
    block: *const BlockBytes,
    block_len: u32,
    cv: *const CVBytes,
    counter: u64,
    flags: u32,
    out: *mut u8,
    out_len: usize,
) {
    crate::xof_using_compress_xof(
        compress_xof,
        block,
        block_len,
        cv,
        counter,
        flags,
        out,
        out_len,
    )
}

pub(crate) unsafe extern "C" fn xof_xor(
    block: *const BlockBytes,
    block_len: u32,
    cv: *const CVBytes,
    counter: u64,
    flags: u32,
    out: *mut u8,
    out_len: usize,
) {
    crate::xof_xor_using_compress_xof(
        compress_xof,
        block,
        block_len,
        cv,
        counter,
        flags,
        out,
        out_len,
    )
}

pub(crate) unsafe extern "C" fn universal_hash(
    input: *const u8,
    input_len: usize,
    key: *const CVBytes,
    counter: u64,
    out: *mut [u8; 16],
) {
    crate::universal_hash_using_compress(compress, input, input_len, key, counter, out)
}

pub fn implementation() -> Implementation {
    Implementation::new(
        degree,
        compress,
        hash_chunks,
        hash_parents,
        xof,
        xof_xor,
        universal_hash,
    )
}

#[cfg(test)]
mod test {
    use super::*;

    // This is circular but do it anyway.
    #[test]
    fn test_compress_vs_portable() {
        crate::test::test_compress_vs_portable(&implementation());
    }

    #[test]
    fn test_compress_vs_reference() {
        crate::test::test_compress_vs_reference(&implementation());
    }

    // This is circular but do it anyway.
    #[test]
    fn test_hash_chunks_vs_portable() {
        crate::test::test_hash_chunks_vs_portable(&implementation());
    }

    // This is circular but do it anyway.
    #[test]
    fn test_hash_parents_vs_portable() {
        crate::test::test_hash_parents_vs_portable(&implementation());
    }

    #[test]
    fn test_chunks_and_parents_vs_reference() {
        crate::test::test_chunks_and_parents_vs_reference(&implementation());
    }

    // This is circular but do it anyway.
    #[test]
    fn test_xof_vs_portable() {
        crate::test::test_xof_vs_portable(&implementation());
    }

    #[test]
    fn test_xof_vs_reference() {
        crate::test::test_xof_vs_reference(&implementation());
    }

    // This is circular but do it anyway.
    #[test]
    fn test_universal_hash_vs_portable() {
        crate::test::test_universal_hash_vs_portable(&implementation());
    }

    #[test]
    fn test_universal_hash_vs_reference() {
        crate::test::test_universal_hash_vs_reference(&implementation());
    }
}
