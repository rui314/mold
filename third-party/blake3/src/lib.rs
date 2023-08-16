//! The official Rust implementation of the [BLAKE3] cryptographic hash
//! function.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Hash an input all at once.
//! let hash1 = blake3::hash(b"foobarbaz");
//!
//! // Hash an input incrementally.
//! let mut hasher = blake3::Hasher::new();
//! hasher.update(b"foo");
//! hasher.update(b"bar");
//! hasher.update(b"baz");
//! let hash2 = hasher.finalize();
//! assert_eq!(hash1, hash2);
//!
//! // Extended output. OutputReader also implements Read and Seek.
//! # #[cfg(feature = "std")] {
//! let mut output = [0; 1000];
//! let mut output_reader = hasher.finalize_xof();
//! output_reader.fill(&mut output);
//! assert_eq!(hash1, output[..32]);
//! # }
//!
//! // Print a hash as hex.
//! println!("{}", hash1);
//! # Ok(())
//! # }
//! ```
//!
//! # Cargo Features
//!
//! The `std` feature (the only feature enabled by default) is required for
//! implementations of the [`Write`] and [`Seek`] traits, and also for runtime
//! CPU feature detection on x86. If this feature is disabled, the only way to
//! use the x86 SIMD implementations is to enable the corresponding instruction
//! sets globally, with e.g. `RUSTFLAGS="-C target-cpu=native"`. The resulting
//! binary will not be portable to other machines.
//!
//! The `rayon` feature (disabled by default, but enabled for [docs.rs]) adds
//! the [`Hasher::update_rayon`] method, for multithreaded hashing. However,
//! even if this feature is enabled, all other APIs remain single-threaded.
//!
//! The NEON implementation is enabled by default for AArch64 but requires the
//! `neon` feature for other ARM targets. Not all ARMv7 CPUs support NEON, and
//! enabling this feature will produce a binary that's not portable to CPUs
//! without NEON support.
//!
//! The `traits-preview` feature enables implementations of traits from the
//! RustCrypto [`digest`] crate, and re-exports that crate as
//! `traits::digest`. However, the traits aren't stable, and they're expected to
//! change in incompatible ways before that crate reaches 1.0. For that reason,
//! this crate makes no SemVer guarantees for this feature, and callers who use
//! it should expect breaking changes between patch versions. (The "-preview"
//! feature name follows the conventions of the RustCrypto [`signature`] crate.)
//!
//! [`Hasher::update_rayon`]: struct.Hasher.html#method.update_rayon
//! [BLAKE3]: https://blake3.io
//! [Rayon]: https://github.com/rayon-rs/rayon
//! [docs.rs]: https://docs.rs/
//! [`Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
//! [`Seek`]: https://doc.rust-lang.org/std/io/trait.Seek.html
//! [`digest`]: https://crates.io/crates/digest
//! [`signature`]: https://crates.io/crates/signature

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod test;

// The guts module is for incremental use cases like the `bao` crate that need
// to explicitly compute chunk and parent chaining values. It is semi-stable
// and likely to keep working, but largely undocumented and not intended for
// widespread use.
#[doc(hidden)]
pub mod guts;

/// Undocumented and unstable, for benchmarks only.
#[doc(hidden)]
pub mod platform;

// Platform-specific implementations of the compression function. These
// BLAKE3-specific cfg flags are set in build.rs.
#[cfg(blake3_avx2_rust)]
#[path = "rust_avx2.rs"]
mod avx2;
#[cfg(blake3_avx2_ffi)]
#[path = "ffi_avx2.rs"]
mod avx2;
#[cfg(blake3_avx512_ffi)]
#[path = "ffi_avx512.rs"]
mod avx512;
#[cfg(blake3_neon)]
#[path = "ffi_neon.rs"]
mod neon;
mod portable;
#[cfg(blake3_sse2_rust)]
#[path = "rust_sse2.rs"]
mod sse2;
#[cfg(blake3_sse2_ffi)]
#[path = "ffi_sse2.rs"]
mod sse2;
#[cfg(blake3_sse41_rust)]
#[path = "rust_sse41.rs"]
mod sse41;
#[cfg(blake3_sse41_ffi)]
#[path = "ffi_sse41.rs"]
mod sse41;

#[cfg(feature = "traits-preview")]
pub mod traits;

mod join;

use arrayref::{array_mut_ref, array_ref};
use arrayvec::{ArrayString, ArrayVec};
use core::cmp;
use core::fmt;
use platform::{Platform, MAX_SIMD_DEGREE, MAX_SIMD_DEGREE_OR_2};

/// The number of bytes in a [`Hash`](struct.Hash.html), 32.
pub const OUT_LEN: usize = 32;

/// The number of bytes in a key, 32.
pub const KEY_LEN: usize = 32;

const MAX_DEPTH: usize = 54; // 2^54 * CHUNK_LEN = 2^64
use guts::{BLOCK_LEN, CHUNK_LEN};

// While iterating the compression function within a chunk, the CV is
// represented as words, to avoid doing two extra endianness conversions for
// each compression in the portable implementation. But the hash_many interface
// needs to hash both input bytes and parent nodes, so its better for its
// output CVs to be represented as bytes.
type CVWords = [u32; 8];
type CVBytes = [u8; 32]; // little-endian

const IV: &CVWords = &[
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

const MSG_SCHEDULE: [[usize; 16]; 7] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8],
    [3, 4, 10, 12, 13, 2, 7, 14, 6, 5, 9, 0, 11, 15, 8, 1],
    [10, 7, 12, 9, 14, 3, 13, 15, 4, 0, 11, 2, 5, 8, 1, 6],
    [12, 13, 9, 11, 15, 10, 14, 8, 7, 2, 5, 3, 0, 1, 6, 4],
    [9, 14, 11, 5, 8, 12, 15, 1, 13, 3, 0, 10, 2, 6, 4, 7],
    [11, 15, 5, 0, 1, 9, 8, 6, 14, 10, 2, 12, 3, 4, 7, 13],
];

// These are the internal flags that we use to domain separate root/non-root,
// chunk/parent, and chunk beginning/middle/end. These get set at the high end
// of the block flags word in the compression function, so their values start
// high and go down.
const CHUNK_START: u8 = 1 << 0;
const CHUNK_END: u8 = 1 << 1;
const PARENT: u8 = 1 << 2;
const ROOT: u8 = 1 << 3;
const KEYED_HASH: u8 = 1 << 4;
const DERIVE_KEY_CONTEXT: u8 = 1 << 5;
const DERIVE_KEY_MATERIAL: u8 = 1 << 6;

#[inline]
fn counter_low(counter: u64) -> u32 {
    counter as u32
}

#[inline]
fn counter_high(counter: u64) -> u32 {
    (counter >> 32) as u32
}

/// An output of the default size, 32 bytes, which provides constant-time
/// equality checking.
///
/// `Hash` implements [`From`] and [`Into`] for `[u8; 32]`, and it provides
/// [`from_bytes`] and [`as_bytes`] for explicit conversions between itself and
/// `[u8; 32]`. However, byte arrays and slices don't provide constant-time
/// equality checking, which is often a security requirement in software that
/// handles private data. `Hash` doesn't implement [`Deref`] or [`AsRef`], to
/// avoid situations where a type conversion happens implicitly and the
/// constant-time property is accidentally lost.
///
/// `Hash` provides the [`to_hex`] and [`from_hex`] methods for converting to
/// and from hexadecimal. It also implements [`Display`] and [`FromStr`].
///
/// [`From`]: https://doc.rust-lang.org/std/convert/trait.From.html
/// [`Into`]: https://doc.rust-lang.org/std/convert/trait.Into.html
/// [`as_bytes`]: #method.as_bytes
/// [`from_bytes`]: #method.from_bytes
/// [`Deref`]: https://doc.rust-lang.org/stable/std/ops/trait.Deref.html
/// [`AsRef`]: https://doc.rust-lang.org/std/convert/trait.AsRef.html
/// [`to_hex`]: #method.to_hex
/// [`from_hex`]: #method.from_hex
/// [`Display`]: https://doc.rust-lang.org/std/fmt/trait.Display.html
/// [`FromStr`]: https://doc.rust-lang.org/std/str/trait.FromStr.html
#[derive(Clone, Copy, Hash)]
pub struct Hash([u8; OUT_LEN]);

impl Hash {
    /// The raw bytes of the `Hash`. Note that byte arrays don't provide
    /// constant-time equality checking, so if  you need to compare hashes,
    /// prefer the `Hash` type.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; OUT_LEN] {
        &self.0
    }

    /// Create a `Hash` from its raw bytes representation.
    pub const fn from_bytes(bytes: [u8; OUT_LEN]) -> Self {
        Self(bytes)
    }

    /// Encode a `Hash` in lowercase hexadecimal.
    ///
    /// The returned [`ArrayString`] is a fixed size and doesn't allocate memory
    /// on the heap. Note that [`ArrayString`] doesn't provide constant-time
    /// equality checking, so if you need to compare hashes, prefer the `Hash`
    /// type.
    ///
    /// [`ArrayString`]: https://docs.rs/arrayvec/0.5.1/arrayvec/struct.ArrayString.html
    pub fn to_hex(&self) -> ArrayString<{ 2 * OUT_LEN }> {
        let mut s = ArrayString::new();
        let table = b"0123456789abcdef";
        for &b in self.0.iter() {
            s.push(table[(b >> 4) as usize] as char);
            s.push(table[(b & 0xf) as usize] as char);
        }
        s
    }

    /// Decode a `Hash` from hexadecimal. Both uppercase and lowercase ASCII
    /// bytes are supported.
    ///
    /// Any byte outside the ranges `'0'...'9'`, `'a'...'f'`, and `'A'...'F'`
    /// results in an error. An input length other than 64 also results in an
    /// error.
    ///
    /// Note that `Hash` also implements `FromStr`, so `Hash::from_hex("...")`
    /// is equivalent to `"...".parse()`.
    pub fn from_hex(hex: impl AsRef<[u8]>) -> Result<Self, HexError> {
        fn hex_val(byte: u8) -> Result<u8, HexError> {
            match byte {
                b'A'..=b'F' => Ok(byte - b'A' + 10),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                b'0'..=b'9' => Ok(byte - b'0'),
                _ => Err(HexError(HexErrorInner::InvalidByte(byte))),
            }
        }
        let hex_bytes: &[u8] = hex.as_ref();
        if hex_bytes.len() != OUT_LEN * 2 {
            return Err(HexError(HexErrorInner::InvalidLen(hex_bytes.len())));
        }
        let mut hash_bytes: [u8; OUT_LEN] = [0; OUT_LEN];
        for i in 0..OUT_LEN {
            hash_bytes[i] = 16 * hex_val(hex_bytes[2 * i])? + hex_val(hex_bytes[2 * i + 1])?;
        }
        Ok(Hash::from(hash_bytes))
    }
}

impl From<[u8; OUT_LEN]> for Hash {
    #[inline]
    fn from(bytes: [u8; OUT_LEN]) -> Self {
        Self::from_bytes(bytes)
    }
}

impl From<Hash> for [u8; OUT_LEN] {
    #[inline]
    fn from(hash: Hash) -> Self {
        hash.0
    }
}

impl core::str::FromStr for Hash {
    type Err = HexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hash::from_hex(s)
    }
}

/// This implementation is constant-time.
impl PartialEq for Hash {
    #[inline]
    fn eq(&self, other: &Hash) -> bool {
        constant_time_eq::constant_time_eq_32(&self.0, &other.0)
    }
}

/// This implementation is constant-time.
impl PartialEq<[u8; OUT_LEN]> for Hash {
    #[inline]
    fn eq(&self, other: &[u8; OUT_LEN]) -> bool {
        constant_time_eq::constant_time_eq_32(&self.0, other)
    }
}

/// This implementation is constant-time if the target is 32 bytes long.
impl PartialEq<[u8]> for Hash {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        constant_time_eq::constant_time_eq(&self.0, other)
    }
}

impl Eq for Hash {}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Formatting field as `&str` to reduce code size since the `Debug`
        // dynamic dispatch table for `&str` is likely needed elsewhere already,
        // but that for `ArrayString<[u8; 64]>` is not.
        let hex = self.to_hex();
        let hex: &str = hex.as_str();

        f.write_str(hex)
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Formatting field as `&str` to reduce code size since the `Debug`
        // dynamic dispatch table for `&str` is likely needed elsewhere already,
        // but that for `ArrayString<[u8; 64]>` is not.
        let hex = self.to_hex();
        let hex: &str = hex.as_str();

        f.debug_tuple("Hash").field(&hex).finish()
    }
}

/// The error type for [`Hash::from_hex`].
///
/// The `.to_string()` representation of this error currently distinguishes between bad length
/// errors and bad character errors. This is to help with logging and debugging, but it isn't a
/// stable API detail, and it may change at any time.
#[derive(Clone, Debug)]
pub struct HexError(HexErrorInner);

#[derive(Clone, Debug)]
enum HexErrorInner {
    InvalidByte(u8),
    InvalidLen(usize),
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            HexErrorInner::InvalidByte(byte) => {
                if byte < 128 {
                    write!(f, "invalid hex character: {:?}", byte as char)
                } else {
                    write!(f, "invalid hex character: 0x{:x}", byte)
                }
            }
            HexErrorInner::InvalidLen(len) => {
                write!(f, "expected 64 hex bytes, received {}", len)
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for HexError {}

// Each chunk or parent node can produce either a 32-byte chaining value or, by
// setting the ROOT flag, any number of final output bytes. The Output struct
// captures the state just prior to choosing between those two possibilities.
#[derive(Clone)]
struct Output {
    input_chaining_value: CVWords,
    block: [u8; 64],
    block_len: u8,
    counter: u64,
    flags: u8,
    platform: Platform,
}

impl Output {
    fn chaining_value(&self) -> CVBytes {
        let mut cv = self.input_chaining_value;
        self.platform.compress_in_place(
            &mut cv,
            &self.block,
            self.block_len,
            self.counter,
            self.flags,
        );
        platform::le_bytes_from_words_32(&cv)
    }

    fn root_hash(&self) -> Hash {
        debug_assert_eq!(self.counter, 0);
        let mut cv = self.input_chaining_value;
        self.platform
            .compress_in_place(&mut cv, &self.block, self.block_len, 0, self.flags | ROOT);
        Hash(platform::le_bytes_from_words_32(&cv))
    }

    fn root_output_block(&self) -> [u8; 2 * OUT_LEN] {
        self.platform.compress_xof(
            &self.input_chaining_value,
            &self.block,
            self.block_len,
            self.counter,
            self.flags | ROOT,
        )
    }
}

#[derive(Clone)]
struct ChunkState {
    cv: CVWords,
    chunk_counter: u64,
    buf: [u8; BLOCK_LEN],
    buf_len: u8,
    blocks_compressed: u8,
    flags: u8,
    platform: Platform,
}

impl ChunkState {
    fn new(key: &CVWords, chunk_counter: u64, flags: u8, platform: Platform) -> Self {
        Self {
            cv: *key,
            chunk_counter,
            buf: [0; BLOCK_LEN],
            buf_len: 0,
            blocks_compressed: 0,
            flags,
            platform,
        }
    }

    fn len(&self) -> usize {
        BLOCK_LEN * self.blocks_compressed as usize + self.buf_len as usize
    }

    fn fill_buf(&mut self, input: &mut &[u8]) {
        let want = BLOCK_LEN - self.buf_len as usize;
        let take = cmp::min(want, input.len());
        self.buf[self.buf_len as usize..][..take].copy_from_slice(&input[..take]);
        self.buf_len += take as u8;
        *input = &input[take..];
    }

    fn start_flag(&self) -> u8 {
        if self.blocks_compressed == 0 {
            CHUNK_START
        } else {
            0
        }
    }

    // Try to avoid buffering as much as possible, by compressing directly from
    // the input slice when full blocks are available.
    fn update(&mut self, mut input: &[u8]) -> &mut Self {
        if self.buf_len > 0 {
            self.fill_buf(&mut input);
            if !input.is_empty() {
                debug_assert_eq!(self.buf_len as usize, BLOCK_LEN);
                let block_flags = self.flags | self.start_flag(); // borrowck
                self.platform.compress_in_place(
                    &mut self.cv,
                    &self.buf,
                    BLOCK_LEN as u8,
                    self.chunk_counter,
                    block_flags,
                );
                self.buf_len = 0;
                self.buf = [0; BLOCK_LEN];
                self.blocks_compressed += 1;
            }
        }

        while input.len() > BLOCK_LEN {
            debug_assert_eq!(self.buf_len, 0);
            let block_flags = self.flags | self.start_flag(); // borrowck
            self.platform.compress_in_place(
                &mut self.cv,
                array_ref!(input, 0, BLOCK_LEN),
                BLOCK_LEN as u8,
                self.chunk_counter,
                block_flags,
            );
            self.blocks_compressed += 1;
            input = &input[BLOCK_LEN..];
        }

        self.fill_buf(&mut input);
        debug_assert!(input.is_empty());
        debug_assert!(self.len() <= CHUNK_LEN);
        self
    }

    fn output(&self) -> Output {
        let block_flags = self.flags | self.start_flag() | CHUNK_END;
        Output {
            input_chaining_value: self.cv,
            block: self.buf,
            block_len: self.buf_len,
            counter: self.chunk_counter,
            flags: block_flags,
            platform: self.platform,
        }
    }
}

// Don't derive(Debug), because the state may be secret.
impl fmt::Debug for ChunkState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ChunkState")
            .field("len", &self.len())
            .field("chunk_counter", &self.chunk_counter)
            .field("flags", &self.flags)
            .field("platform", &self.platform)
            .finish()
    }
}

// IMPLEMENTATION NOTE
// ===================
// The recursive function compress_subtree_wide(), implemented below, is the
// basis of high-performance BLAKE3. We use it both for all-at-once hashing,
// and for the incremental input with Hasher (though we have to be careful with
// subtree boundaries in the incremental case). compress_subtree_wide() applies
// several optimizations at the same time:
// - Multithreading with Rayon.
// - Parallel chunk hashing with SIMD.
// - Parallel parent hashing with SIMD. Note that while SIMD chunk hashing
//   maxes out at MAX_SIMD_DEGREE*CHUNK_LEN, parallel parent hashing continues
//   to benefit from larger inputs, because more levels of the tree benefit can
//   use full-width SIMD vectors for parent hashing. Without parallel parent
//   hashing, we lose about 10% of overall throughput on AVX2 and AVX-512.

/// Undocumented and unstable, for benchmarks only.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum IncrementCounter {
    Yes,
    No,
}

impl IncrementCounter {
    #[inline]
    fn yes(&self) -> bool {
        match self {
            IncrementCounter::Yes => true,
            IncrementCounter::No => false,
        }
    }
}

// The largest power of two less than or equal to `n`, used for left_len()
// immediately below, and also directly in Hasher::update().
fn largest_power_of_two_leq(n: usize) -> usize {
    ((n / 2) + 1).next_power_of_two()
}

// Given some input larger than one chunk, return the number of bytes that
// should go in the left subtree. This is the largest power-of-2 number of
// chunks that leaves at least 1 byte for the right subtree.
fn left_len(content_len: usize) -> usize {
    debug_assert!(content_len > CHUNK_LEN);
    // Subtract 1 to reserve at least one byte for the right side.
    let full_chunks = (content_len - 1) / CHUNK_LEN;
    largest_power_of_two_leq(full_chunks) * CHUNK_LEN
}

// Use SIMD parallelism to hash up to MAX_SIMD_DEGREE chunks at the same time
// on a single thread. Write out the chunk chaining values and return the
// number of chunks hashed. These chunks are never the root and never empty;
// those cases use a different codepath.
fn compress_chunks_parallel(
    input: &[u8],
    key: &CVWords,
    chunk_counter: u64,
    flags: u8,
    platform: Platform,
    out: &mut [u8],
) -> usize {
    debug_assert!(!input.is_empty(), "empty chunks below the root");
    debug_assert!(input.len() <= MAX_SIMD_DEGREE * CHUNK_LEN);

    let mut chunks_exact = input.chunks_exact(CHUNK_LEN);
    let mut chunks_array = ArrayVec::<&[u8; CHUNK_LEN], MAX_SIMD_DEGREE>::new();
    for chunk in &mut chunks_exact {
        chunks_array.push(array_ref!(chunk, 0, CHUNK_LEN));
    }
    platform.hash_many(
        &chunks_array,
        key,
        chunk_counter,
        IncrementCounter::Yes,
        flags,
        CHUNK_START,
        CHUNK_END,
        out,
    );

    // Hash the remaining partial chunk, if there is one. Note that the empty
    // chunk (meaning the empty message) is a different codepath.
    let chunks_so_far = chunks_array.len();
    if !chunks_exact.remainder().is_empty() {
        let counter = chunk_counter + chunks_so_far as u64;
        let mut chunk_state = ChunkState::new(key, counter, flags, platform);
        chunk_state.update(chunks_exact.remainder());
        *array_mut_ref!(out, chunks_so_far * OUT_LEN, OUT_LEN) =
            chunk_state.output().chaining_value();
        chunks_so_far + 1
    } else {
        chunks_so_far
    }
}

// Use SIMD parallelism to hash up to MAX_SIMD_DEGREE parents at the same time
// on a single thread. Write out the parent chaining values and return the
// number of parents hashed. (If there's an odd input chaining value left over,
// return it as an additional output.) These parents are never the root and
// never empty; those cases use a different codepath.
fn compress_parents_parallel(
    child_chaining_values: &[u8],
    key: &CVWords,
    flags: u8,
    platform: Platform,
    out: &mut [u8],
) -> usize {
    debug_assert_eq!(child_chaining_values.len() % OUT_LEN, 0, "wacky hash bytes");
    let num_children = child_chaining_values.len() / OUT_LEN;
    debug_assert!(num_children >= 2, "not enough children");
    debug_assert!(num_children <= 2 * MAX_SIMD_DEGREE_OR_2, "too many");

    let mut parents_exact = child_chaining_values.chunks_exact(BLOCK_LEN);
    // Use MAX_SIMD_DEGREE_OR_2 rather than MAX_SIMD_DEGREE here, because of
    // the requirements of compress_subtree_wide().
    let mut parents_array = ArrayVec::<&[u8; BLOCK_LEN], MAX_SIMD_DEGREE_OR_2>::new();
    for parent in &mut parents_exact {
        parents_array.push(array_ref!(parent, 0, BLOCK_LEN));
    }
    platform.hash_many(
        &parents_array,
        key,
        0, // Parents always use counter 0.
        IncrementCounter::No,
        flags | PARENT,
        0, // Parents have no start flags.
        0, // Parents have no end flags.
        out,
    );

    // If there's an odd child left over, it becomes an output.
    let parents_so_far = parents_array.len();
    if !parents_exact.remainder().is_empty() {
        out[parents_so_far * OUT_LEN..][..OUT_LEN].copy_from_slice(parents_exact.remainder());
        parents_so_far + 1
    } else {
        parents_so_far
    }
}

// The wide helper function returns (writes out) an array of chaining values
// and returns the length of that array. The number of chaining values returned
// is the dynamically detected SIMD degree, at most MAX_SIMD_DEGREE. Or fewer,
// if the input is shorter than that many chunks. The reason for maintaining a
// wide array of chaining values going back up the tree, is to allow the
// implementation to hash as many parents in parallel as possible.
//
// As a special case when the SIMD degree is 1, this function will still return
// at least 2 outputs. This guarantees that this function doesn't perform the
// root compression. (If it did, it would use the wrong flags, and also we
// wouldn't be able to implement extendable output.) Note that this function is
// not used when the whole input is only 1 chunk long; that's a different
// codepath.
//
// Why not just have the caller split the input on the first update(), instead
// of implementing this special rule? Because we don't want to limit SIMD or
// multithreading parallelism for that update().
fn compress_subtree_wide<J: join::Join>(
    input: &[u8],
    key: &CVWords,
    chunk_counter: u64,
    flags: u8,
    platform: Platform,
    out: &mut [u8],
) -> usize {
    // Note that the single chunk case does *not* bump the SIMD degree up to 2
    // when it is 1. This allows Rayon the option of multithreading even the
    // 2-chunk case, which can help performance on smaller platforms.
    if input.len() <= platform.simd_degree() * CHUNK_LEN {
        return compress_chunks_parallel(input, key, chunk_counter, flags, platform, out);
    }

    // With more than simd_degree chunks, we need to recurse. Start by dividing
    // the input into left and right subtrees. (Note that this is only optimal
    // as long as the SIMD degree is a power of 2. If we ever get a SIMD degree
    // of 3 or something, we'll need a more complicated strategy.)
    debug_assert_eq!(platform.simd_degree().count_ones(), 1, "power of 2");
    let (left, right) = input.split_at(left_len(input.len()));
    let right_chunk_counter = chunk_counter + (left.len() / CHUNK_LEN) as u64;

    // Make space for the child outputs. Here we use MAX_SIMD_DEGREE_OR_2 to
    // account for the special case of returning 2 outputs when the SIMD degree
    // is 1.
    let mut cv_array = [0; 2 * MAX_SIMD_DEGREE_OR_2 * OUT_LEN];
    let degree = if left.len() == CHUNK_LEN {
        // The "simd_degree=1 and we're at the leaf nodes" case.
        debug_assert_eq!(platform.simd_degree(), 1);
        1
    } else {
        cmp::max(platform.simd_degree(), 2)
    };
    let (left_out, right_out) = cv_array.split_at_mut(degree * OUT_LEN);

    // Recurse! For update_rayon(), this is where we take advantage of RayonJoin and use multiple
    // threads.
    let (left_n, right_n) = J::join(
        || compress_subtree_wide::<J>(left, key, chunk_counter, flags, platform, left_out),
        || compress_subtree_wide::<J>(right, key, right_chunk_counter, flags, platform, right_out),
    );

    // The special case again. If simd_degree=1, then we'll have left_n=1 and
    // right_n=1. Rather than compressing them into a single output, return
    // them directly, to make sure we always have at least two outputs.
    debug_assert_eq!(left_n, degree);
    debug_assert!(right_n >= 1 && right_n <= left_n);
    if left_n == 1 {
        out[..2 * OUT_LEN].copy_from_slice(&cv_array[..2 * OUT_LEN]);
        return 2;
    }

    // Otherwise, do one layer of parent node compression.
    let num_children = left_n + right_n;
    compress_parents_parallel(
        &cv_array[..num_children * OUT_LEN],
        key,
        flags,
        platform,
        out,
    )
}

// Hash a subtree with compress_subtree_wide(), and then condense the resulting
// list of chaining values down to a single parent node. Don't compress that
// last parent node, however. Instead, return its message bytes (the
// concatenated chaining values of its children). This is necessary when the
// first call to update() supplies a complete subtree, because the topmost
// parent node of that subtree could end up being the root. It's also necessary
// for extended output in the general case.
//
// As with compress_subtree_wide(), this function is not used on inputs of 1
// chunk or less. That's a different codepath.
fn compress_subtree_to_parent_node<J: join::Join>(
    input: &[u8],
    key: &CVWords,
    chunk_counter: u64,
    flags: u8,
    platform: Platform,
) -> [u8; BLOCK_LEN] {
    debug_assert!(input.len() > CHUNK_LEN);
    let mut cv_array = [0; MAX_SIMD_DEGREE_OR_2 * OUT_LEN];
    let mut num_cvs =
        compress_subtree_wide::<J>(input, &key, chunk_counter, flags, platform, &mut cv_array);
    debug_assert!(num_cvs >= 2);

    // If MAX_SIMD_DEGREE is greater than 2 and there's enough input,
    // compress_subtree_wide() returns more than 2 chaining values. Condense
    // them into 2 by forming parent nodes repeatedly.
    let mut out_array = [0; MAX_SIMD_DEGREE_OR_2 * OUT_LEN / 2];
    while num_cvs > 2 {
        let cv_slice = &cv_array[..num_cvs * OUT_LEN];
        num_cvs = compress_parents_parallel(cv_slice, key, flags, platform, &mut out_array);
        cv_array[..num_cvs * OUT_LEN].copy_from_slice(&out_array[..num_cvs * OUT_LEN]);
    }
    *array_ref!(cv_array, 0, 2 * OUT_LEN)
}

// Hash a complete input all at once. Unlike compress_subtree_wide() and
// compress_subtree_to_parent_node(), this function handles the 1 chunk case.
fn hash_all_at_once<J: join::Join>(input: &[u8], key: &CVWords, flags: u8) -> Output {
    let platform = Platform::detect();

    // If the whole subtree is one chunk, hash it directly with a ChunkState.
    if input.len() <= CHUNK_LEN {
        return ChunkState::new(key, 0, flags, platform)
            .update(input)
            .output();
    }

    // Otherwise construct an Output object from the parent node returned by
    // compress_subtree_to_parent_node().
    Output {
        input_chaining_value: *key,
        block: compress_subtree_to_parent_node::<J>(input, key, 0, flags, platform),
        block_len: BLOCK_LEN as u8,
        counter: 0,
        flags: flags | PARENT,
        platform,
    }
}

/// The default hash function.
///
/// For an incremental version that accepts multiple writes, see
/// [`Hasher::update`].
///
/// For output sizes other than 32 bytes, see [`Hasher::finalize_xof`] and
/// [`OutputReader`].
///
/// This function is always single-threaded. For multithreading support, see
/// [`Hasher::update_rayon`](struct.Hasher.html#method.update_rayon).
pub fn hash(input: &[u8]) -> Hash {
    hash_all_at_once::<join::SerialJoin>(input, IV, 0).root_hash()
}

/// The keyed hash function.
///
/// This is suitable for use as a message authentication code, for example to
/// replace an HMAC instance. In that use case, the constant-time equality
/// checking provided by [`Hash`](struct.Hash.html) is almost always a security
/// requirement, and callers need to be careful not to compare MACs as raw
/// bytes.
///
/// For output sizes other than 32 bytes, see [`Hasher::new_keyed`],
/// [`Hasher::finalize_xof`], and [`OutputReader`].
///
/// This function is always single-threaded. For multithreading support, see
/// [`Hasher::new_keyed`] and
/// [`Hasher::update_rayon`](struct.Hasher.html#method.update_rayon).
pub fn keyed_hash(key: &[u8; KEY_LEN], input: &[u8]) -> Hash {
    let key_words = platform::words_from_le_bytes_32(key);
    hash_all_at_once::<join::SerialJoin>(input, &key_words, KEYED_HASH).root_hash()
}

/// The key derivation function.
///
/// Given cryptographic key material of any length and a context string of any
/// length, this function outputs a 32-byte derived subkey. **The context string
/// should be hardcoded, globally unique, and application-specific.** A good
/// default format for such strings is `"[application] [commit timestamp]
/// [purpose]"`, e.g., `"example.com 2019-12-25 16:18:03 session tokens v1"`.
///
/// Key derivation is important when you want to use the same key in multiple
/// algorithms or use cases. Using the same key with different cryptographic
/// algorithms is generally forbidden, and deriving a separate subkey for each
/// use case protects you from bad interactions. Derived keys also mitigate the
/// damage from one part of your application accidentally leaking its key.
///
/// As a rare exception to that general rule, however, it is possible to use
/// `derive_key` itself with key material that you are already using with
/// another algorithm. You might need to do this if you're adding features to
/// an existing application, which does not yet use key derivation internally.
/// However, you still must not share key material with algorithms that forbid
/// key reuse entirely, like a one-time pad. For more on this, see sections 6.2
/// and 7.8 of the [BLAKE3 paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf).
///
/// Note that BLAKE3 is not a password hash, and **`derive_key` should never be
/// used with passwords.** Instead, use a dedicated password hash like
/// [Argon2]. Password hashes are entirely different from generic hash
/// functions, with opposite design requirements.
///
/// For output sizes other than 32 bytes, see [`Hasher::new_derive_key`],
/// [`Hasher::finalize_xof`], and [`OutputReader`].
///
/// This function is always single-threaded. For multithreading support, see
/// [`Hasher::new_derive_key`] and
/// [`Hasher::update_rayon`](struct.Hasher.html#method.update_rayon).
///
/// [Argon2]: https://en.wikipedia.org/wiki/Argon2
pub fn derive_key(context: &str, key_material: &[u8]) -> [u8; OUT_LEN] {
    let context_key =
        hash_all_at_once::<join::SerialJoin>(context.as_bytes(), IV, DERIVE_KEY_CONTEXT)
            .root_hash();
    let context_key_words = platform::words_from_le_bytes_32(context_key.as_bytes());
    hash_all_at_once::<join::SerialJoin>(key_material, &context_key_words, DERIVE_KEY_MATERIAL)
        .root_hash()
        .0
}

fn parent_node_output(
    left_child: &CVBytes,
    right_child: &CVBytes,
    key: &CVWords,
    flags: u8,
    platform: Platform,
) -> Output {
    let mut block = [0; BLOCK_LEN];
    block[..32].copy_from_slice(left_child);
    block[32..].copy_from_slice(right_child);
    Output {
        input_chaining_value: *key,
        block,
        block_len: BLOCK_LEN as u8,
        counter: 0,
        flags: flags | PARENT,
        platform,
    }
}

/// An incremental hash state that can accept any number of writes.
///
/// When the `traits-preview` Cargo feature is enabled, this type implements
/// several commonly used traits from the
/// [`digest`](https://crates.io/crates/digest) crate. However, those
/// traits aren't stable, and they're expected to change in incompatible ways
/// before that crate reaches 1.0. For that reason, this crate makes no SemVer
/// guarantees for this feature, and callers who use it should expect breaking
/// changes between patch versions.
///
/// When the `rayon` Cargo feature is enabled, the
/// [`update_rayon`](#method.update_rayon) method is available for multithreaded
/// hashing.
///
/// **Performance note:** The [`update`](#method.update) method can't take full
/// advantage of SIMD optimizations if its input buffer is too small or oddly
/// sized. Using a 16 KiB buffer, or any multiple of that, enables all currently
/// supported SIMD instruction sets.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Hash an input incrementally.
/// let mut hasher = blake3::Hasher::new();
/// hasher.update(b"foo");
/// hasher.update(b"bar");
/// hasher.update(b"baz");
/// assert_eq!(hasher.finalize(), blake3::hash(b"foobarbaz"));
///
/// // Extended output. OutputReader also implements Read and Seek.
/// # #[cfg(feature = "std")] {
/// let mut output = [0; 1000];
/// let mut output_reader = hasher.finalize_xof();
/// output_reader.fill(&mut output);
/// assert_eq!(&output[..32], blake3::hash(b"foobarbaz").as_bytes());
/// # }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Hasher {
    key: CVWords,
    chunk_state: ChunkState,
    // The stack size is MAX_DEPTH + 1 because we do lazy merging. For example,
    // with 7 chunks, we have 3 entries in the stack. Adding an 8th chunk
    // requires a 4th entry, rather than merging everything down to 1, because
    // we don't know whether more input is coming. This is different from how
    // the reference implementation does things.
    cv_stack: ArrayVec<CVBytes, { MAX_DEPTH + 1 }>,
}

impl Hasher {
    fn new_internal(key: &CVWords, flags: u8) -> Self {
        Self {
            key: *key,
            chunk_state: ChunkState::new(key, 0, flags, Platform::detect()),
            cv_stack: ArrayVec::new(),
        }
    }

    /// Construct a new `Hasher` for the regular hash function.
    pub fn new() -> Self {
        Self::new_internal(IV, 0)
    }

    /// Construct a new `Hasher` for the keyed hash function. See
    /// [`keyed_hash`].
    ///
    /// [`keyed_hash`]: fn.keyed_hash.html
    pub fn new_keyed(key: &[u8; KEY_LEN]) -> Self {
        let key_words = platform::words_from_le_bytes_32(key);
        Self::new_internal(&key_words, KEYED_HASH)
    }

    /// Construct a new `Hasher` for the key derivation function. See
    /// [`derive_key`]. The context string should be hardcoded, globally
    /// unique, and application-specific.
    ///
    /// [`derive_key`]: fn.derive_key.html
    pub fn new_derive_key(context: &str) -> Self {
        let context_key =
            hash_all_at_once::<join::SerialJoin>(context.as_bytes(), IV, DERIVE_KEY_CONTEXT)
                .root_hash();
        let context_key_words = platform::words_from_le_bytes_32(context_key.as_bytes());
        Self::new_internal(&context_key_words, DERIVE_KEY_MATERIAL)
    }

    /// Reset the `Hasher` to its initial state.
    ///
    /// This is functionally the same as overwriting the `Hasher` with a new
    /// one, using the same key or context string if any.
    pub fn reset(&mut self) -> &mut Self {
        self.chunk_state = ChunkState::new(
            &self.key,
            0,
            self.chunk_state.flags,
            self.chunk_state.platform,
        );
        self.cv_stack.clear();
        self
    }

    // As described in push_cv() below, we do "lazy merging", delaying merges
    // until right before the next CV is about to be added. This is different
    // from the reference implementation. Another difference is that we aren't
    // always merging 1 chunk at a time. Instead, each CV might represent any
    // power-of-two number of chunks, as long as the smaller-above-larger stack
    // order is maintained. Instead of the "count the trailing 0-bits"
    // algorithm described in the spec, we use a "count the total number of
    // 1-bits" variant that doesn't require us to retain the subtree size of
    // the CV on top of the stack. The principle is the same: each CV that
    // should remain in the stack is represented by a 1-bit in the total number
    // of chunks (or bytes) so far.
    fn merge_cv_stack(&mut self, total_len: u64) {
        let post_merge_stack_len = total_len.count_ones() as usize;
        while self.cv_stack.len() > post_merge_stack_len {
            let right_child = self.cv_stack.pop().unwrap();
            let left_child = self.cv_stack.pop().unwrap();
            let parent_output = parent_node_output(
                &left_child,
                &right_child,
                &self.key,
                self.chunk_state.flags,
                self.chunk_state.platform,
            );
            self.cv_stack.push(parent_output.chaining_value());
        }
    }

    // In reference_impl.rs, we merge the new CV with existing CVs from the
    // stack before pushing it. We can do that because we know more input is
    // coming, so we know none of the merges are root.
    //
    // This setting is different. We want to feed as much input as possible to
    // compress_subtree_wide(), without setting aside anything for the
    // chunk_state. If the user gives us 64 KiB, we want to parallelize over
    // all 64 KiB at once as a single subtree, if at all possible.
    //
    // This leads to two problems:
    // 1) This 64 KiB input might be the only call that ever gets made to
    //    update. In this case, the root node of the 64 KiB subtree would be
    //    the root node of the whole tree, and it would need to be ROOT
    //    finalized. We can't compress it until we know.
    // 2) This 64 KiB input might complete a larger tree, whose root node is
    //    similarly going to be the the root of the whole tree. For example,
    //    maybe we have 196 KiB (that is, 128 + 64) hashed so far. We can't
    //    compress the node at the root of the 256 KiB subtree until we know
    //    how to finalize it.
    //
    // The second problem is solved with "lazy merging". That is, when we're
    // about to add a CV to the stack, we don't merge it with anything first,
    // as the reference impl does. Instead we do merges using the *previous* CV
    // that was added, which is sitting on top of the stack, and we put the new
    // CV (unmerged) on top of the stack afterwards. This guarantees that we
    // never merge the root node until finalize().
    //
    // Solving the first problem requires an additional tool,
    // compress_subtree_to_parent_node(). That function always returns the top
    // *two* chaining values of the subtree it's compressing. We then do lazy
    // merging with each of them separately, so that the second CV will always
    // remain unmerged. (That also helps us support extendable output when
    // we're hashing an input all-at-once.)
    fn push_cv(&mut self, new_cv: &CVBytes, chunk_counter: u64) {
        self.merge_cv_stack(chunk_counter);
        self.cv_stack.push(*new_cv);
    }

    /// Add input bytes to the hash state. You can call this any number of
    /// times.
    ///
    /// This method is always single-threaded. For multithreading support, see
    /// [`update_rayon`](#method.update_rayon) below (enabled with the `rayon`
    /// Cargo feature).
    ///
    /// Note that the degree of SIMD parallelism that `update` can use is
    /// limited by the size of this input buffer. The 8 KiB buffer currently
    /// used by [`std::io::copy`] is enough to leverage AVX2, for example, but
    /// not enough to leverage AVX-512. A 16 KiB buffer is large enough to
    /// leverage all currently supported SIMD instruction sets.
    ///
    /// [`std::io::copy`]: https://doc.rust-lang.org/std/io/fn.copy.html
    pub fn update(&mut self, input: &[u8]) -> &mut Self {
        self.update_with_join::<join::SerialJoin>(input)
    }

    /// Identical to [`update`](Hasher::update), but using Rayon-based
    /// multithreading internally.
    ///
    /// This method is gated by the `rayon` Cargo feature, which is disabled by
    /// default but enabled on [docs.rs](https://docs.rs).
    ///
    /// To get any performance benefit from multithreading, the input buffer
    /// needs to be large. As a rule of thumb on x86_64, `update_rayon` is
    /// _slower_ than `update` for inputs under 128 KiB. That threshold varies
    /// quite a lot across different processors, and it's important to benchmark
    /// your specific use case.
    ///
    /// Memory mapping an entire input file is a simple way to take advantage of
    /// multithreading without needing to carefully tune your buffer size or
    /// offload IO. However, on spinning disks where random access is expensive,
    /// that approach can lead to disk thrashing and terrible IO performance.
    /// Note that OS page caching can mask this problem, in which case it might
    /// only appear for files larger than available RAM. Again, benchmarking
    /// your specific use case is important.
    #[cfg(feature = "rayon")]
    pub fn update_rayon(&mut self, input: &[u8]) -> &mut Self {
        self.update_with_join::<join::RayonJoin>(input)
    }

    fn update_with_join<J: join::Join>(&mut self, mut input: &[u8]) -> &mut Self {
        // If we have some partial chunk bytes in the internal chunk_state, we
        // need to finish that chunk first.
        if self.chunk_state.len() > 0 {
            let want = CHUNK_LEN - self.chunk_state.len();
            let take = cmp::min(want, input.len());
            self.chunk_state.update(&input[..take]);
            input = &input[take..];
            if !input.is_empty() {
                // We've filled the current chunk, and there's more input
                // coming, so we know it's not the root and we can finalize it.
                // Then we'll proceed to hashing whole chunks below.
                debug_assert_eq!(self.chunk_state.len(), CHUNK_LEN);
                let chunk_cv = self.chunk_state.output().chaining_value();
                self.push_cv(&chunk_cv, self.chunk_state.chunk_counter);
                self.chunk_state = ChunkState::new(
                    &self.key,
                    self.chunk_state.chunk_counter + 1,
                    self.chunk_state.flags,
                    self.chunk_state.platform,
                );
            } else {
                return self;
            }
        }

        // Now the chunk_state is clear, and we have more input. If there's
        // more than a single chunk (so, definitely not the root chunk), hash
        // the largest whole subtree we can, with the full benefits of SIMD and
        // multithreading parallelism. Two restrictions:
        // - The subtree has to be a power-of-2 number of chunks. Only subtrees
        //   along the right edge can be incomplete, and we don't know where
        //   the right edge is going to be until we get to finalize().
        // - The subtree must evenly divide the total number of chunks up until
        //   this point (if total is not 0). If the current incomplete subtree
        //   is only waiting for 1 more chunk, we can't hash a subtree of 4
        //   chunks. We have to complete the current subtree first.
        // Because we might need to break up the input to form powers of 2, or
        // to evenly divide what we already have, this part runs in a loop.
        while input.len() > CHUNK_LEN {
            debug_assert_eq!(self.chunk_state.len(), 0, "no partial chunk data");
            debug_assert_eq!(CHUNK_LEN.count_ones(), 1, "power of 2 chunk len");
            let mut subtree_len = largest_power_of_two_leq(input.len());
            let count_so_far = self.chunk_state.chunk_counter * CHUNK_LEN as u64;
            // Shrink the subtree_len until it evenly divides the count so far.
            // We know that subtree_len itself is a power of 2, so we can use a
            // bitmasking trick instead of an actual remainder operation. (Note
            // that if the caller consistently passes power-of-2 inputs of the
            // same size, as is hopefully typical, this loop condition will
            // always fail, and subtree_len will always be the full length of
            // the input.)
            //
            // An aside: We don't have to shrink subtree_len quite this much.
            // For example, if count_so_far is 1, we could pass 2 chunks to
            // compress_subtree_to_parent_node. Since we'll get 2 CVs back,
            // we'll still get the right answer in the end, and we might get to
            // use 2-way SIMD parallelism. The problem with this optimization,
            // is that it gets us stuck always hashing 2 chunks. The total
            // number of chunks will remain odd, and we'll never graduate to
            // higher degrees of parallelism. See
            // https://github.com/BLAKE3-team/BLAKE3/issues/69.
            while (subtree_len - 1) as u64 & count_so_far != 0 {
                subtree_len /= 2;
            }
            // The shrunken subtree_len might now be 1 chunk long. If so, hash
            // that one chunk by itself. Otherwise, compress the subtree into a
            // pair of CVs.
            let subtree_chunks = (subtree_len / CHUNK_LEN) as u64;
            if subtree_len <= CHUNK_LEN {
                debug_assert_eq!(subtree_len, CHUNK_LEN);
                self.push_cv(
                    &ChunkState::new(
                        &self.key,
                        self.chunk_state.chunk_counter,
                        self.chunk_state.flags,
                        self.chunk_state.platform,
                    )
                    .update(&input[..subtree_len])
                    .output()
                    .chaining_value(),
                    self.chunk_state.chunk_counter,
                );
            } else {
                // This is the high-performance happy path, though getting here
                // depends on the caller giving us a long enough input.
                let cv_pair = compress_subtree_to_parent_node::<J>(
                    &input[..subtree_len],
                    &self.key,
                    self.chunk_state.chunk_counter,
                    self.chunk_state.flags,
                    self.chunk_state.platform,
                );
                let left_cv = array_ref!(cv_pair, 0, 32);
                let right_cv = array_ref!(cv_pair, 32, 32);
                // Push the two CVs we received into the CV stack in order. Because
                // the stack merges lazily, this guarantees we aren't merging the
                // root.
                self.push_cv(left_cv, self.chunk_state.chunk_counter);
                self.push_cv(
                    right_cv,
                    self.chunk_state.chunk_counter + (subtree_chunks / 2),
                );
            }
            self.chunk_state.chunk_counter += subtree_chunks;
            input = &input[subtree_len..];
        }

        // What remains is 1 chunk or less. Add it to the chunk state.
        debug_assert!(input.len() <= CHUNK_LEN);
        if !input.is_empty() {
            self.chunk_state.update(input);
            // Having added some input to the chunk_state, we know what's in
            // the CV stack won't become the root node, and we can do an extra
            // merge. This simplifies finalize().
            self.merge_cv_stack(self.chunk_state.chunk_counter);
        }

        self
    }

    fn final_output(&self) -> Output {
        // If the current chunk is the only chunk, that makes it the root node
        // also. Convert it directly into an Output. Otherwise, we need to
        // merge subtrees below.
        if self.cv_stack.is_empty() {
            debug_assert_eq!(self.chunk_state.chunk_counter, 0);
            return self.chunk_state.output();
        }

        // If there are any bytes in the ChunkState, finalize that chunk and
        // merge its CV with everything in the CV stack. In that case, the work
        // we did at the end of update() above guarantees that the stack
        // doesn't contain any unmerged subtrees that need to be merged first.
        // (This is important, because if there were two chunk hashes sitting
        // on top of the stack, they would need to merge with each other, and
        // merging a new chunk hash into them would be incorrect.)
        //
        // If there are no bytes in the ChunkState, we'll merge what's already
        // in the stack. In this case it's fine if there are unmerged chunks on
        // top, because we'll merge them with each other. Note that the case of
        // the empty chunk is taken care of above.
        let mut output: Output;
        let mut num_cvs_remaining = self.cv_stack.len();
        if self.chunk_state.len() > 0 {
            debug_assert_eq!(
                self.cv_stack.len(),
                self.chunk_state.chunk_counter.count_ones() as usize,
                "cv stack does not need a merge"
            );
            output = self.chunk_state.output();
        } else {
            debug_assert!(self.cv_stack.len() >= 2);
            output = parent_node_output(
                &self.cv_stack[num_cvs_remaining - 2],
                &self.cv_stack[num_cvs_remaining - 1],
                &self.key,
                self.chunk_state.flags,
                self.chunk_state.platform,
            );
            num_cvs_remaining -= 2;
        }
        while num_cvs_remaining > 0 {
            output = parent_node_output(
                &self.cv_stack[num_cvs_remaining - 1],
                &output.chaining_value(),
                &self.key,
                self.chunk_state.flags,
                self.chunk_state.platform,
            );
            num_cvs_remaining -= 1;
        }
        output
    }

    /// Finalize the hash state and return the [`Hash`](struct.Hash.html) of
    /// the input.
    ///
    /// This method is idempotent. Calling it twice will give the same result.
    /// You can also add more input and finalize again.
    pub fn finalize(&self) -> Hash {
        self.final_output().root_hash()
    }

    /// Finalize the hash state and return an [`OutputReader`], which can
    /// supply any number of output bytes.
    ///
    /// This method is idempotent. Calling it twice will give the same result.
    /// You can also add more input and finalize again.
    ///
    /// [`OutputReader`]: struct.OutputReader.html
    pub fn finalize_xof(&self) -> OutputReader {
        OutputReader::new(self.final_output())
    }

    /// Return the total number of bytes hashed so far.
    pub fn count(&self) -> u64 {
        self.chunk_state.chunk_counter * CHUNK_LEN as u64 + self.chunk_state.len() as u64
    }
}

// Don't derive(Debug), because the state may be secret.
impl fmt::Debug for Hasher {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Hasher")
            .field("flags", &self.chunk_state.flags)
            .field("platform", &self.chunk_state.platform)
            .finish()
    }
}

impl Default for Hasher {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl std::io::Write for Hasher {
    /// This is equivalent to [`update`](#method.update).
    #[inline]
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        self.update(input);
        Ok(input.len())
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An incremental reader for extended output, returned by
/// [`Hasher::finalize_xof`](struct.Hasher.html#method.finalize_xof).
///
/// Shorter BLAKE3 outputs are prefixes of longer ones, and explicitly requesting a short output is
/// equivalent to truncating the default-length output. Note that this is a difference between
/// BLAKE2 and BLAKE3.
///
/// # Security notes
///
/// Outputs shorter than the default length of 32 bytes (256 bits) provide less security. An N-bit
/// BLAKE3 output is intended to provide N bits of first and second preimage resistance and N/2
/// bits of collision resistance, for any N up to 256. Longer outputs don't provide any additional
/// security.
///
/// Avoid relying on the secrecy of the output offset, that is, the number of output bytes read or
/// the arguments to [`seek`](struct.OutputReader.html#method.seek) or
/// [`set_position`](struct.OutputReader.html#method.set_position). [_Block-Cipher-Based Tree
/// Hashing_ by Aldo Gunsing](https://eprint.iacr.org/2022/283) shows that an attacker who knows
/// both the message and the key (if any) can easily determine the offset of an extended output.
/// For comparison, AES-CTR has a similar property: if you know the key, you can decrypt a block
/// from an unknown position in the output stream to recover its block index. Callers with strong
/// secret keys aren't affected in practice, but secret offsets are a [design
/// smell](https://en.wikipedia.org/wiki/Design_smell) in any case.
#[derive(Clone)]
pub struct OutputReader {
    inner: Output,
    position_within_block: u8,
}

impl OutputReader {
    fn new(inner: Output) -> Self {
        Self {
            inner,
            position_within_block: 0,
        }
    }

    /// Fill a buffer with output bytes and advance the position of the
    /// `OutputReader`. This is equivalent to [`Read::read`], except that it
    /// doesn't return a `Result`. Both methods always fill the entire buffer.
    ///
    /// Note that `OutputReader` doesn't buffer output bytes internally, so
    /// calling `fill` repeatedly with a short-length or odd-length slice will
    /// end up performing the same compression multiple times. If you're
    /// reading output in a loop, prefer a slice length that's a multiple of
    /// 64.
    ///
    /// The maximum output size of BLAKE3 is 2<sup>64</sup>-1 bytes. If you try
    /// to extract more than that, for example by seeking near the end and
    /// reading further, the behavior is unspecified.
    ///
    /// [`Read::read`]: #method.read
    pub fn fill(&mut self, mut buf: &mut [u8]) {
        while !buf.is_empty() {
            let block: [u8; BLOCK_LEN] = self.inner.root_output_block();
            let output_bytes = &block[self.position_within_block as usize..];
            let take = cmp::min(buf.len(), output_bytes.len());
            buf[..take].copy_from_slice(&output_bytes[..take]);
            buf = &mut buf[take..];
            self.position_within_block += take as u8;
            if self.position_within_block == BLOCK_LEN as u8 {
                self.inner.counter += 1;
                self.position_within_block = 0;
            }
        }
    }

    /// Return the current read position in the output stream. This is
    /// equivalent to [`Seek::stream_position`], except that it doesn't return
    /// a `Result`. The position of a new `OutputReader` starts at 0, and each
    /// call to [`fill`] or [`Read::read`] moves the position forward by the
    /// number of bytes read.
    ///
    /// [`Seek::stream_position`]: #method.stream_position
    /// [`fill`]: #method.fill
    /// [`Read::read`]: #method.read
    pub fn position(&self) -> u64 {
        self.inner.counter * BLOCK_LEN as u64 + self.position_within_block as u64
    }

    /// Seek to a new read position in the output stream. This is equivalent to
    /// calling [`Seek::seek`] with [`SeekFrom::Start`], except that it doesn't
    /// return a `Result`.
    ///
    /// [`Seek::seek`]: #method.seek
    /// [`SeekFrom::Start`]: https://doc.rust-lang.org/std/io/enum.SeekFrom.html
    pub fn set_position(&mut self, position: u64) {
        self.position_within_block = (position % BLOCK_LEN as u64) as u8;
        self.inner.counter = position / BLOCK_LEN as u64;
    }
}

// Don't derive(Debug), because the state may be secret.
impl fmt::Debug for OutputReader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("OutputReader")
            .field("position", &self.position())
            .finish()
    }
}

#[cfg(feature = "std")]
impl std::io::Read for OutputReader {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.fill(buf);
        Ok(buf.len())
    }
}

#[cfg(feature = "std")]
impl std::io::Seek for OutputReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let max_position = u64::max_value() as i128;
        let target_position: i128 = match pos {
            std::io::SeekFrom::Start(x) => x as i128,
            std::io::SeekFrom::Current(x) => self.position() as i128 + x as i128,
            std::io::SeekFrom::End(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "seek from end not supported",
                ));
            }
        };
        if target_position < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.set_position(cmp::min(target_position, max_position) as u64);
        Ok(self.position())
    }
}
