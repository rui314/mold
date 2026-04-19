This is the reference implementation of BLAKE3. It is used for testing and
as a readable example of the algorithms involved. Section 5.1 of [the BLAKE3
spec](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf)
discusses this implementation. You can render docs for this implementation
by running `cargo doc --open` in this directory.

This implementation is a single file
([`reference_impl.rs`](reference_impl.rs)) with no dependencies. It is
not optimized for performance.

There are ports of this reference implementation to other languages:

- [C](https://github.com/oconnor663/blake3_reference_impl_c)
- [Python](https://github.com/oconnor663/pure_python_blake3)
