//! Low-level tree manipulations and other sharp tools
//!
//! The target audience for this module is projects like [Bao](https://github.com/oconnor663/bao),
//! which work directly with the interior hashes ("chaining values") of BLAKE3 chunks and subtrees.
//! For example, you could use these functions to implement a BitTorrent-like protocol using the
//! BLAKE3 tree structure, or to hash an input that's distributed across different machines. These
//! use cases are advanced, and most applications don't need this module. Also:
//!
//! <div class="warning">
//!
//! **Warning:** This module is *hazardous material*. If you've heard folks say *don't roll your
//! own crypto,* this is the sort of thing they're talking about. These functions have complicated
//! requirements, and any mistakes will give you garbage output and/or break the security
//! properties that BLAKE3 is supposed to have. Read section 2.1 of [the BLAKE3
//! paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf) to understand the
//! tree structure you need to maintain. Test your code against [`blake3::hash`](../fn.hash.html)
//! and make sure you can get the same outputs for [lots of different
//! inputs](https://github.com/BLAKE3-team/BLAKE3/blob/master/test_vectors/test_vectors.json).
//!
//! </div>
//!
//! On the other hand:
//!
//! <div class="warning">
//!
//! **Encouragement:** Playing with these functions is a great way to learn how BLAKE3 works on the
//! inside. Have fun!
//!
//! </div>
//!
//! The main entrypoint for this module is the [`HasherExt`] trait, particularly the
//! [`set_input_offset`](HasherExt::set_input_offset) and
//! [`finalize_non_root`](HasherExt::finalize_non_root) methods. These let you compute the chaining
//! values of individual chunks or subtrees. You then combine these chaining values into larger
//! subtrees using [`merge_subtrees_non_root`] and finally (once at the very top)
//! [`merge_subtrees_root`] or [`merge_subtrees_root_xof`].
//!
//! # Examples
//!
//! Here's an example of computing all the interior hashes in a 3-chunk tree:
//!
//! ```text
//!            root
//!          /      \
//!      parent      \
//!    /       \      \
//! chunk0  chunk1  chunk2
//! ```
//!
//! ```
//! # fn main() {
//! use blake3::{Hasher, CHUNK_LEN};
//! use blake3::hazmat::{merge_subtrees_non_root, merge_subtrees_root, Mode};
//! use blake3::hazmat::HasherExt; // an extension trait for Hasher
//!
//! let chunk0 = [b'a'; CHUNK_LEN];
//! let chunk1 = [b'b'; CHUNK_LEN];
//! let chunk2 = [b'c'; 42]; // The final chunk can be short.
//!
//! // Compute the non-root hashes ("chaining values") of all three chunks. Chunks or subtrees
//! // that don't begin at the start of the input use `set_input_offset` to say where they begin.
//! let chunk0_cv = Hasher::new()
//!     // .set_input_offset(0) is the default.
//!     .update(&chunk0)
//!     .finalize_non_root();
//! let chunk1_cv = Hasher::new()
//!     .set_input_offset(CHUNK_LEN as u64)
//!     .update(&chunk1)
//!     .finalize_non_root();
//! let chunk2_cv = Hasher::new()
//!     .set_input_offset(2 * CHUNK_LEN as u64)
//!     .update(&chunk2)
//!     .finalize_non_root();
//!
//! // Join the first two chunks with a non-root parent node and compute its chaining value.
//! let parent_cv = merge_subtrees_non_root(&chunk0_cv, &chunk1_cv, Mode::Hash);
//!
//! // Join that parent node and the third chunk with a root parent node and compute the hash.
//! let root_hash = merge_subtrees_root(&parent_cv, &chunk2_cv, Mode::Hash);
//!
//! // Double check that we got the right answer.
//! let mut combined_input = Vec::new();
//! combined_input.extend_from_slice(&chunk0);
//! combined_input.extend_from_slice(&chunk1);
//! combined_input.extend_from_slice(&chunk2);
//! assert_eq!(root_hash, blake3::hash(&combined_input));
//! # }
//! ```
//!
//! Hashing many chunks together is important for performance, because it allows the implementation
//! to use SIMD parallelism internally. ([AVX-512](https://en.wikipedia.org/wiki/AVX-512) for
//! example needs 16 chunks to really get going.) We can reproduce `parent_cv` by hashing `chunk0`
//! and `chunk1` at the same time:
//!
//! ```
//! # fn main() {
//! # use blake3::{Hasher, CHUNK_LEN};
//! # use blake3::hazmat::{Mode, HasherExt, merge_subtrees_non_root, merge_subtrees_root};
//! # let chunk0 = [b'a'; CHUNK_LEN];
//! # let chunk1 = [b'b'; CHUNK_LEN];
//! # let chunk0_cv = Hasher::new().update(&chunk0).finalize_non_root();
//! # let chunk1_cv = Hasher::new().set_input_offset(CHUNK_LEN as u64).update(&chunk1).finalize_non_root();
//! # let parent_cv = merge_subtrees_non_root(&chunk0_cv, &chunk1_cv, Mode::Hash);
//! # let mut combined_input = Vec::new();
//! # combined_input.extend_from_slice(&chunk0);
//! # combined_input.extend_from_slice(&chunk1);
//! let left_subtree_cv = Hasher::new()
//!     // .set_input_offset(0) is the default.
//!     .update(&combined_input[..2 * CHUNK_LEN])
//!     .finalize_non_root();
//! assert_eq!(left_subtree_cv, parent_cv);
//!
//! // Using multiple updates gives the same answer, though it's not as efficient.
//! let mut subtree_hasher = Hasher::new();
//! // Again, .set_input_offset(0) is the default.
//! subtree_hasher.update(&chunk0);
//! subtree_hasher.update(&chunk1);
//! assert_eq!(left_subtree_cv, subtree_hasher.finalize_non_root());
//! # }
//! ```
//!
//! However, hashing multiple chunks together **must** respect the overall tree structure. Hashing
//! `chunk0` and `chunk1` together is valid, but hashing `chunk1` and `chunk2` together is
//! incorrect and gives a garbage result that will never match a standard BLAKE3 hash. The
//! implementation includes a few best-effort asserts to catch some of these mistakes, but these
//! checks aren't guaranteed. For example, this second call to `update` currently panics:
//!
//! ```should_panic
//! # fn main() {
//! # use blake3::{Hasher, CHUNK_LEN};
//! # use blake3::hazmat::HasherExt;
//! # let chunk0 = [b'a'; CHUNK_LEN];
//! # let chunk1 = [b'b'; CHUNK_LEN];
//! # let chunk2 = [b'c'; 42];
//! let oops = Hasher::new()
//!     .set_input_offset(CHUNK_LEN as u64)
//!     .update(&chunk1)
//!     // PANIC: "the subtree starting at 1024 contains at most 1024 bytes"
//!     .update(&chunk2)
//!     .finalize_non_root();
//! # }
//! ```
//!
//! For more on valid tree structures, see the docs for and [`left_subtree_len`] and
//! [`max_subtree_len`], and see section 2.1 of [the BLAKE3
//! paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf). Note that the
//! merging functions ([`merge_subtrees_root`] and friends) don't know the shape of the left and
//! right subtrees you're giving them, and they can't help you catch mistakes. The best way to
//! catch mistakes with these is to compare your root output to the [`blake3::hash`](crate::hash)
//! of the same input.

use crate::platform::Platform;
use crate::{CVWords, Hasher, CHUNK_LEN, IV, KEY_LEN, OUT_LEN};

/// Extension methods for [`Hasher`]. This is the main entrypoint to the `hazmat` module.
pub trait HasherExt {
    /// Similar to [`Hasher::new_derive_key`] but using a pre-hashed [`ContextKey`] from
    /// [`hash_derive_key_context`].
    ///
    /// The [`hash_derive_key_context`] function is _only_ valid source of the [`ContextKey`]
    ///
    /// # Example
    ///
    /// ```
    /// use blake3::Hasher;
    /// use blake3::hazmat::HasherExt;
    ///
    /// let context_key = blake3::hazmat::hash_derive_key_context("foo");
    /// let mut hasher = Hasher::new_from_context_key(&context_key);
    /// hasher.update(b"bar");
    /// let derived_key = *hasher.finalize().as_bytes();
    ///
    /// assert_eq!(derived_key, blake3::derive_key("foo", b"bar"));
    /// ```
    fn new_from_context_key(context_key: &ContextKey) -> Self;

    /// Configure the `Hasher` to process a chunk or subtree starting at `offset` bytes into the
    /// whole input.
    ///
    /// You must call this function before processing any input with [`update`](Hasher::update) or
    /// similar. This step isn't required for the first chunk, or for a subtree that includes the
    /// first chunk (i.e. when the `offset` is zero), but it's required for all other chunks and
    /// subtrees.
    ///
    /// The starting input offset of a subtree implies a maximum possible length for that subtree.
    /// See [`max_subtree_len`] and section 2.1 of [the BLAKE3
    /// paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf). Note that only
    /// subtrees along the right edge of the whole tree can have a length less than their maximum
    /// possible length.
    ///
    /// See the [module level examples](index.html#examples).
    ///
    /// # Panics
    ///
    /// This function panics if the `Hasher` has already accepted any input with
    /// [`update`](Hasher::update) or similar.
    ///
    /// This should always be paired with [`finalize_non_root`](HasherExt::finalize_non_root). It's
    /// never correct to use a non-zero input offset with [`finalize`](Hasher::finalize) or
    /// [`finalize_xof`](Hasher::finalize_xof). The `offset` must also be a multiple of
    /// `CHUNK_LEN`. Violating either of these rules will currently fail an assertion and panic,
    /// but this is not guaranteed.
    fn set_input_offset(&mut self, offset: u64) -> &mut Self;

    /// Finalize the non-root hash ("chaining value") of the current chunk or subtree.
    ///
    /// Afterwards you can merge subtree chaining values into parent nodes using
    /// [`merge_subtrees_non_root`] and ultimately into the root node with either
    /// [`merge_subtrees_root`] (similar to [`Hasher::finalize`]) or [`merge_subtrees_root_xof`]
    /// (similar to [`Hasher::finalize_xof`]).
    ///
    /// See the [module level examples](index.html#examples), particularly the discussion of valid
    /// tree structures.
    fn finalize_non_root(&self) -> ChainingValue;
}

impl HasherExt for Hasher {
    fn new_from_context_key(context_key: &[u8; KEY_LEN]) -> Hasher {
        let context_key_words = crate::platform::words_from_le_bytes_32(context_key);
        Hasher::new_internal(&context_key_words, crate::DERIVE_KEY_MATERIAL)
    }

    fn set_input_offset(&mut self, offset: u64) -> &mut Hasher {
        assert_eq!(self.count(), 0, "hasher has already accepted input");
        assert_eq!(
            offset % CHUNK_LEN as u64,
            0,
            "offset ({offset}) must be a chunk boundary (divisible by {CHUNK_LEN})",
        );
        let counter = offset / CHUNK_LEN as u64;
        self.chunk_state.chunk_counter = counter;
        self.initial_chunk_counter = counter;
        self
    }

    fn finalize_non_root(&self) -> ChainingValue {
        assert_ne!(self.count(), 0, "empty subtrees are never valid");
        self.final_output().chaining_value()
    }
}

/// The maximum length of a subtree in bytes, given its starting offset in bytes
///
/// If you try to hash more than this many bytes as one subtree, you'll end up merging parent nodes
/// that shouldn't be merged, and your output will be garbage. [`Hasher::update`] will currently
/// panic in this case, but this is not guaranteed.
///
/// For input offset zero (the default), there is no maximum length, and this function returns
/// `None`. For all other offsets it returns `Some`. Note that valid offsets must be a multiple of
/// [`CHUNK_LEN`] (1024); it's not possible to start hashing a chunk in the middle.
///
/// In the example tree below, chunks are numbered by their _0-based index_. The subtree that
/// _starts_ with chunk 3, i.e. `input_offset = 3 * CHUNK_LEN`, includes only that one chunk, so
/// its max length is `Some(CHUNK_LEN)`. The subtree that starts with chunk 6 includes chunk 7 but
/// not chunk 8, so its max length is `Some(2 * CHUNK_LEN)`. The subtree that starts with chunk 12
/// includes chunks 13, 14, and 15, but if the tree were bigger it would not include chunk 16, so
/// its max length is `Some(4 * CHUNK_LEN)`. One way to think about the rule here is that, if you
/// go beyond the max subtree length from a given starting offset, you start dealing with subtrees
/// that include chunks _to the left_ of where you started.
///
/// ```text
///                           root
///                 /                       \
///              .                             .
///        /           \                 /           \
///       .             .               .             .
///    /    \         /    \         /    \         /    \
///   .      .       .      .       .      .       .      .
///  / \    / \     / \    / \     / \    / \     / \    / \
/// 0  1   2  3    4  5   6  7    8  9   10 11   12 13  14 15
/// ```
///
/// The general rule turns out to be that for a subtree starting at a 0-based chunk index N greater
/// than zero, the maximum number of chunks in that subtree is the largest power-of-two that
/// divides N, which is given by `1 << N.trailing_zeros()`.
///
/// This function can be useful for writing tests or debug assertions, but it's actually rare to
/// use this for real control flow. Callers who split their input recursively using
/// [`left_subtree_len`] will automatically satisfy the `max_subtree_len` bound and don't
/// necessarily need to check. It's also common to choose some fixed power-of-two subtree size, say
/// 64 chunks, and divide your input up into slices of that fixed length (with the final slice
/// possibly short). This approach also automatically satisfies the `max_subtree_len` bound and
/// doesn't need to check. Proving that this is true can be an interesting exercise. Note that
/// chunks 0, 4, 8, and 12 all begin subtrees of at least 4 chunks in the example tree above.
///
/// # Panics
///
/// This function currently panics if `input_offset` is not a multiple of `CHUNK_LEN`. This is not
/// guaranteed.
#[inline(always)]
pub fn max_subtree_len(input_offset: u64) -> Option<u64> {
    if input_offset == 0 {
        return None;
    }
    assert_eq!(input_offset % CHUNK_LEN as u64, 0);
    let counter = input_offset / CHUNK_LEN as u64;
    let max_chunks = 1 << counter.trailing_zeros();
    Some(max_chunks * CHUNK_LEN as u64)
}

#[test]
fn test_max_subtree_len() {
    assert_eq!(max_subtree_len(0), None);
    // (chunk index, max chunks)
    let cases = [
        (1, 1),
        (2, 2),
        (3, 1),
        (4, 4),
        (5, 1),
        (6, 2),
        (7, 1),
        (8, 8),
    ];
    for (chunk_index, max_chunks) in cases {
        let input_offset = chunk_index * CHUNK_LEN as u64;
        assert_eq!(
            max_subtree_len(input_offset),
            Some(max_chunks * CHUNK_LEN as u64),
        );
    }
}

/// Given the length in bytes of either a complete input or a subtree input, return the number of
/// bytes that belong to its left child subtree. The rest belong to its right child subtree.
///
/// Concretely, this function returns the largest power-of-two number of bytes that's strictly less
/// than `input_len`. This leads to a tree where all left subtrees are "complete" and at least as
/// large as their sibling right subtrees, as specified in section 2.1 of [the BLAKE3
/// paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf). For example, if an
/// input is exactly two chunks, its left and right subtrees both get one chunk. But if an input is
/// two chunks plus one more byte, then its left subtree gets two chunks, and its right subtree
/// only gets one byte.
///
/// This function isn't meaningful for one chunk of input, because chunks don't have children. It
/// currently panics in debug mode if `input_len <= CHUNK_LEN`.
///
/// # Example
///
/// Hash a input of random length as two subtrees:
///
/// ```
/// # #[cfg(feature = "std")] {
/// use blake3::hazmat::{left_subtree_len, merge_subtrees_root, HasherExt, Mode};
/// use blake3::{Hasher, CHUNK_LEN};
///
/// // Generate a random-length input. Note that to be split into two subtrees, the input length
/// // must be greater than CHUNK_LEN.
/// let input_len = rand::random_range(CHUNK_LEN + 1..1_000_000);
/// let mut input = vec![0; input_len];
/// rand::fill(&mut input[..]);
///
/// // Compute the left and right subtree hashes and then the root hash. left_subtree_len() tells
/// // us exactly where to split the input. Any other split would either panic (if we're lucky) or
/// // lead to an incorrect root hash.
/// let left_len = left_subtree_len(input_len as u64) as usize;
/// let left_subtree_cv = Hasher::new()
///     .update(&input[..left_len])
///     .finalize_non_root();
/// let right_subtree_cv = Hasher::new()
///     .set_input_offset(left_len as u64)
///     .update(&input[left_len..])
///     .finalize_non_root();
/// let root_hash = merge_subtrees_root(&left_subtree_cv, &right_subtree_cv, Mode::Hash);
///
/// // Double check the answer.
/// assert_eq!(root_hash, blake3::hash(&input));
/// # }
/// ```
#[inline(always)]
pub fn left_subtree_len(input_len: u64) -> u64 {
    debug_assert!(input_len > CHUNK_LEN as u64);
    // Note that .next_power_of_two() is greater than *or equal*.
    ((input_len + 1) / 2).next_power_of_two()
}

#[test]
fn test_left_subtree_len() {
    assert_eq!(left_subtree_len(1025), 1024);
    for boundary_case in [2, 4, 8, 16, 32, 64] {
        let input_len = boundary_case * CHUNK_LEN as u64;
        assert_eq!(left_subtree_len(input_len - 1), input_len / 2);
        assert_eq!(left_subtree_len(input_len), input_len / 2);
        assert_eq!(left_subtree_len(input_len + 1), input_len);
    }
}

/// The `mode` argument to [`merge_subtrees_root`] and friends
///
/// See the [module level examples](index.html#examples).
#[derive(Copy, Clone, Debug)]
pub enum Mode<'a> {
    /// Corresponding to [`hash`](crate::hash)
    Hash,

    /// Corresponding to [`keyed_hash`](crate::hash)
    KeyedHash(&'a [u8; KEY_LEN]),

    /// Corresponding to [`derive_key`](crate::hash)
    ///
    /// The [`ContextKey`] comes from [`hash_derive_key_context`].
    DeriveKeyMaterial(&'a ContextKey),
}

impl<'a> Mode<'a> {
    fn key_words(&self) -> CVWords {
        match self {
            Mode::Hash => *IV,
            Mode::KeyedHash(key) => crate::platform::words_from_le_bytes_32(key),
            Mode::DeriveKeyMaterial(cx_key) => crate::platform::words_from_le_bytes_32(cx_key),
        }
    }

    fn flags_byte(&self) -> u8 {
        match self {
            Mode::Hash => 0,
            Mode::KeyedHash(_) => crate::KEYED_HASH,
            Mode::DeriveKeyMaterial(_) => crate::DERIVE_KEY_MATERIAL,
        }
    }
}

/// "Chaining value" is the academic term for a non-root or non-final hash.
///
/// Besides just sounding fancy, it turns out there are [security
/// reasons](https://jacko.io/tree_hashing.html) to be careful about the difference between
/// (root/final) hashes and (non-root/non-final) chaining values.
pub type ChainingValue = [u8; OUT_LEN];

fn merge_subtrees_inner(
    left_child: &ChainingValue,
    right_child: &ChainingValue,
    mode: Mode,
) -> crate::Output {
    crate::parent_node_output(
        &left_child,
        &right_child,
        &mode.key_words(),
        mode.flags_byte(),
        Platform::detect(),
    )
}

/// Compute a non-root parent node chaining value from two child chaining values.
///
/// See the [module level examples](index.html#examples), particularly the discussion of valid tree
/// structures. The left and right child chaining values can come from either
/// [`Hasher::finalize_non_root`](HasherExt::finalize_non_root) or other calls to
/// `merge_subtrees_non_root`. "Chaining value" is the academic term for a non-root or non-final
/// hash.
pub fn merge_subtrees_non_root(
    left_child: &ChainingValue,
    right_child: &ChainingValue,
    mode: Mode,
) -> ChainingValue {
    merge_subtrees_inner(left_child, right_child, mode).chaining_value()
}

/// Compute a root hash from two child chaining values.
///
/// See the [module level examples](index.html#examples), particularly the discussion of valid tree
/// structures. The left and right child chaining values can come from either
/// [`Hasher::finalize_non_root`](HasherExt::finalize_non_root) or [`merge_subtrees_non_root`].
/// "Chaining value" is the academic term for a non-root or non-final hash.
///
/// Note that inputs of [`CHUNK_LEN`] or less don't produce any parent nodes and can't be hashed
/// using this function. In that case you must get the root hash from [`Hasher::finalize`] (or just
/// [`blake3::hash`](crate::hash)).
pub fn merge_subtrees_root(
    left_child: &ChainingValue,
    right_child: &ChainingValue,
    mode: Mode,
) -> crate::Hash {
    merge_subtrees_inner(left_child, right_child, mode).root_hash()
}

/// Build a root [`OutputReader`](crate::OutputReader) from two child chaining values.
///
/// See also the [module level examples](index.html#examples), particularly the discussion of valid
/// tree structures. The left and right child chaining values can come from either
/// [`Hasher::finalize_non_root`](HasherExt::finalize_non_root) or [`merge_subtrees_non_root`].
/// "Chaining value" is the academic term for a non-root or non-final hash.
///
/// Note that inputs of [`CHUNK_LEN`] or less don't produce any parent nodes and can't be hashed
/// using this function. In that case you must get the `OutputReader` from
/// [`Hasher::finalize_xof`].
///
/// # Example
///
/// ```
/// use blake3::hazmat::{merge_subtrees_root_xof, HasherExt, Mode};
/// use blake3::{Hasher, CHUNK_LEN};
///
/// // Hash a 2-chunk subtree in steps. Note that only
/// // the final chunk can be shorter than CHUNK_LEN.
/// let chunk0 = &[42; CHUNK_LEN];
/// let chunk1 = b"hello world";
/// let chunk0_cv = Hasher::new()
///     .update(chunk0)
///     .finalize_non_root();
/// let chunk1_cv = Hasher::new()
///     .set_input_offset(CHUNK_LEN as u64)
///     .update(chunk1)
///     .finalize_non_root();
///
/// // Obtain a blake3::OutputReader at the root and extract 1000 bytes.
/// let mut output_reader = merge_subtrees_root_xof(&chunk0_cv, &chunk1_cv, Mode::Hash);
/// let mut output_bytes = [0; 1_000];
/// output_reader.fill(&mut output_bytes);
///
/// // Double check the answer.
/// let mut hasher = Hasher::new();
/// hasher.update(chunk0);
/// hasher.update(chunk1);
/// let mut expected = [0; 1_000];
/// hasher.finalize_xof().fill(&mut expected);
/// assert_eq!(output_bytes, expected);
/// ```
pub fn merge_subtrees_root_xof(
    left_child: &ChainingValue,
    right_child: &ChainingValue,
    mode: Mode,
) -> crate::OutputReader {
    crate::OutputReader::new(merge_subtrees_inner(left_child, right_child, mode))
}

/// An alias to distinguish [`hash_derive_key_context`] outputs from other keys.
pub type ContextKey = [u8; KEY_LEN];

/// Hash a [`derive_key`](crate::derive_key) context string and return a [`ContextKey`].
///
/// The _only_ valid uses for the returned [`ContextKey`] are [`Hasher::new_from_context_key`] and
/// [`Mode::DeriveKeyMaterial`] (together with the merge subtree functions).
///
/// # Example
///
/// ```
/// use blake3::Hasher;
/// use blake3::hazmat::HasherExt;
///
/// let context_key = blake3::hazmat::hash_derive_key_context("foo");
/// let mut hasher = Hasher::new_from_context_key(&context_key);
/// hasher.update(b"bar");
/// let derived_key = *hasher.finalize().as_bytes();
///
/// assert_eq!(derived_key, blake3::derive_key("foo", b"bar"));
/// ```
pub fn hash_derive_key_context(context: &str) -> ContextKey {
    crate::hash_all_at_once::<crate::join::SerialJoin>(
        context.as_bytes(),
        IV,
        crate::DERIVE_KEY_CONTEXT,
    )
    .root_hash()
    .0
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[should_panic]
    fn test_empty_subtree_should_panic() {
        Hasher::new().finalize_non_root();
    }

    #[test]
    #[should_panic]
    fn test_unaligned_offset_should_panic() {
        Hasher::new().set_input_offset(1);
    }

    #[test]
    #[should_panic]
    fn test_hasher_already_accepted_input_should_panic() {
        Hasher::new().update(b"x").set_input_offset(0);
    }

    #[test]
    #[should_panic]
    fn test_too_much_input_should_panic() {
        Hasher::new()
            .set_input_offset(CHUNK_LEN as u64)
            .update(&[0; CHUNK_LEN + 1]);
    }

    #[test]
    #[should_panic]
    fn test_set_input_offset_cant_finalize() {
        Hasher::new().set_input_offset(CHUNK_LEN as u64).finalize();
    }

    #[test]
    #[should_panic]
    fn test_set_input_offset_cant_finalize_xof() {
        Hasher::new()
            .set_input_offset(CHUNK_LEN as u64)
            .finalize_xof();
    }

    #[test]
    fn test_grouped_hash() {
        const MAX_CHUNKS: usize = (crate::test::TEST_CASES_MAX + 1) / CHUNK_LEN;
        let mut input_buf = [0; crate::test::TEST_CASES_MAX];
        crate::test::paint_test_input(&mut input_buf);
        for subtree_chunks in [1, 2, 4, 8, 16, 32] {
            #[cfg(feature = "std")]
            dbg!(subtree_chunks);
            let subtree_len = subtree_chunks * CHUNK_LEN;
            for &case in crate::test::TEST_CASES {
                if case <= subtree_len {
                    continue;
                }
                #[cfg(feature = "std")]
                dbg!(case);
                let input = &input_buf[..case];
                let expected_hash = crate::hash(input);

                // Collect all the group chaining values.
                let mut chaining_values = arrayvec::ArrayVec::<ChainingValue, MAX_CHUNKS>::new();
                let mut subtree_offset = 0;
                while subtree_offset < input.len() {
                    let take = core::cmp::min(subtree_len, input.len() - subtree_offset);
                    let subtree_input = &input[subtree_offset..][..take];
                    let subtree_cv = Hasher::new()
                        .set_input_offset(subtree_offset as u64)
                        .update(subtree_input)
                        .finalize_non_root();
                    chaining_values.push(subtree_cv);
                    subtree_offset += take;
                }

                // Compress all the chaining_values together, layer by layer.
                assert!(chaining_values.len() >= 2);
                while chaining_values.len() > 2 {
                    let n = chaining_values.len();
                    // Merge each side-by-side pair in place, overwriting the front half of the
                    // array with the merged results. This moves us "up one level" in the tree.
                    for i in 0..(n / 2) {
                        chaining_values[i] = merge_subtrees_non_root(
                            &chaining_values[2 * i],
                            &chaining_values[2 * i + 1],
                            Mode::Hash,
                        );
                    }
                    // If there's an odd CV out, it moves up.
                    if n % 2 == 1 {
                        chaining_values[n / 2] = chaining_values[n - 1];
                    }
                    chaining_values.truncate(n / 2 + n % 2);
                }
                assert_eq!(chaining_values.len(), 2);
                let root_hash =
                    merge_subtrees_root(&chaining_values[0], &chaining_values[1], Mode::Hash);
                assert_eq!(expected_hash, root_hash);
            }
        }
    }

    #[test]
    fn test_keyed_hash_xof() {
        let group0 = &[42; 4096];
        let group1 = &[43; 4095];
        let mut input = [0; 8191];
        input[..4096].copy_from_slice(group0);
        input[4096..].copy_from_slice(group1);
        let key = &[44; 32];

        let mut expected_output = [0; 100];
        Hasher::new_keyed(&key)
            .update(&input)
            .finalize_xof()
            .fill(&mut expected_output);

        let mut hazmat_output = [0; 100];
        let left = Hasher::new_keyed(key).update(group0).finalize_non_root();
        let right = Hasher::new_keyed(key)
            .set_input_offset(group0.len() as u64)
            .update(group1)
            .finalize_non_root();
        merge_subtrees_root_xof(&left, &right, Mode::KeyedHash(&key)).fill(&mut hazmat_output);
        assert_eq!(expected_output, hazmat_output);
    }

    #[test]
    fn test_derive_key() {
        let context = "foo";
        let mut input = [0; 1025];
        crate::test::paint_test_input(&mut input);
        let expected = crate::derive_key(context, &input);

        let cx_key = hash_derive_key_context(context);
        let left = Hasher::new_from_context_key(&cx_key)
            .update(&input[..1024])
            .finalize_non_root();
        let right = Hasher::new_from_context_key(&cx_key)
            .set_input_offset(1024)
            .update(&input[1024..])
            .finalize_non_root();
        let derived_key = merge_subtrees_root(&left, &right, Mode::DeriveKeyMaterial(&cx_key)).0;
        assert_eq!(expected, derived_key);
    }
}
