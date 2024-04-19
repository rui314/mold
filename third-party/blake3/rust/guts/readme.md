# The BLAKE3 Guts API

## Introduction

This [`blake3_guts`](https://crates.io/crates/blake3_guts) sub-crate contains
low-level, high-performance, platform-specific implementations of the BLAKE3
compression function. This API is complicated and unsafe, and this crate will
never have a stable release. Most callers should instead use the
[`blake3`](https://crates.io/crates/blake3) crate, which will eventually depend
on this one internally.

The code you see here (as of January 2024) is an early stage of a large planned
refactor. The motivation for this refactor is a couple of missing features in
both the Rust and C implementations:

- The output side
  ([`OutputReader`](https://docs.rs/blake3/latest/blake3/struct.OutputReader.html)
  in Rust) doesn't take advantage of the most important SIMD optimizations that
  compute multiple blocks in parallel. This blocks any project that wants to
  use the BLAKE3 XOF as a stream cipher
  ([[1]](https://github.com/oconnor663/bessie),
  [[2]](https://github.com/oconnor663/blake3_aead)).
- Low-level callers like [Bao](https://github.com/oconnor663/bao) that need
  interior nodes of the tree also don't get those SIMD optimizations. They have
  to use a slow, minimalistic, unstable, doc-hidden module [(also called
  `guts`)](https://github.com/BLAKE3-team/BLAKE3/blob/master/src/guts.rs).

The difficulty with adding those features is that they require changes to all
of our optimized assembly and C intrinsics code. That's a couple dozen
different files that are large, platform-specific, difficult to understand, and
full of duplicated code. The higher-level Rust and C implementations of BLAKE3
both depend on these files and will need to coordinate changes.

At the same time, it won't be long before we add support for more platforms:

- RISCV vector extensions
- ARM SVE
- WebAssembly SIMD

It's important to get this refactor done before new platforms make it even
harder to do.

## The private guts API

This is the API that each platform reimplements, so we want it to be as simple
as possible apart from the high-performance work it needs to do. It's
completely `unsafe`, and inputs and outputs are raw pointers that are allowed
to alias (this matters for `hash_parents`, see below).

- `degree`
- `compress`
    - The single compression function, for short inputs and odd-length tails.
- `hash_chunks`
- `hash_parents`
- `xof`
- `xof_xor`
    - As `xof` but XOR'ing the result into the output buffer.
- `universal_hash`
    - This is a new construction specifically to support
      [BLAKE3-AEAD](https://github.com/oconnor663/blake3_aead). Some
      implementations might just stub it out with portable code.

## The public guts API

This is the API that this crate exposes to callers, i.e. to the main `blake3`
crate. It's a thin, portable layer on top of the private API above. The Rust
version of this API is memory-safe.

- `degree`
- `compress`
- `hash_chunks`
- `hash_parents`
    - This handles most levels of the tree, where we keep hashing SIMD_DEGREE
      parents at a time.
- `reduce_parents`
    - This uses the same `hash_parents` private API, but it handles the top
      levels of the tree where we reduce in-place to the root parent node.
- `xof`
- `xof_xor`
- `universal_hash`
