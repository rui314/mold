# BLAKE3

BLAKE3 is a cryptographic hash function that is:

- **Much faster** than MD5, SHA-1, SHA-2, SHA-3, and BLAKE2.
- **Secure**, unlike MD5 and SHA-1. And secure against length extension,
  unlike SHA-2.
- **Highly parallelizable** across any number of threads and SIMD lanes,
  because it's a Merkle tree on the inside.
- Capable of **verified streaming** and **incremental updates**, again
  because it's a Merkle tree.
- A **PRF**, **MAC**, **KDF**, and **XOF**, as well as a regular hash.
- **One algorithm with no variants**, which is fast on x86-64 and also
  on smaller architectures.

The [chart below](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/benchmarks/bar_chart.py)
is an example benchmark of 16 KiB inputs on a Cascade Lake-SP 8275CL server CPU
from 2019. For more detailed benchmarks, see the
[BLAKE3 paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf).

<p align="center">
<img src="media/speed.svg" alt="performance graph">
</p>

BLAKE3 is based on an optimized instance of the established hash
function [BLAKE2](https://blake2.net) and on the [original Bao tree
mode](https://github.com/oconnor663/bao/blob/master/docs/spec_0.9.1.md).
The specifications and design rationale are available in the [BLAKE3
paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf).
The default output size is 256 bits. The current version of
[Bao](https://github.com/oconnor663/bao) implements verified streaming
with BLAKE3.

This repository is the official implementation of BLAKE3. It includes:

* The [`blake3`](https://crates.io/crates/blake3) Rust crate, which
  includes optimized implementations for SSE2, SSE4.1, AVX2, AVX-512,
  NEON, and WASM, with automatic runtime CPU feature detection on x86.
  The `rayon` feature provides multithreading.

* The [`b3sum`](https://crates.io/crates/b3sum) Rust crate, which
  provides a command line interface. It uses multithreading by default,
  making it an order of magnitude faster than e.g. `sha256sum` on
  typical desktop hardware.

* The [C implementation](c), which like the Rust implementation includes SIMD
  optimizations (all except WASM), CPU feature detection on x86, and optional
  multithreading. See [`c/README.md`](c/README.md).

* The [Rust reference implementation](reference_impl/reference_impl.rs),
  which is discussed in Section 5.1 of the [BLAKE3
  paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf).
  This implementation is much smaller and simpler than the optimized
  ones above. If you want to see how BLAKE3 works, or you're writing a
  port that doesn't need multithreading or SIMD optimizations, start
  here. Ports of the reference implementation to other languages are
  hosted in separate repositories
  ([C](https://github.com/oconnor663/blake3_reference_impl_c),
  [Python](https://github.com/oconnor663/pure_python_blake3)).

* A [set of test
  vectors](https://github.com/BLAKE3-team/BLAKE3/blob/master/test_vectors/test_vectors.json)
  that covers extended outputs, all three modes, and a variety of input
  lengths.

* [![Actions Status](https://github.com/BLAKE3-team/BLAKE3/workflows/tests/badge.svg)](https://github.com/BLAKE3-team/BLAKE3/actions)

BLAKE3 was designed by:

* [@oconnor663] (Jack O'Connor)
* [@sneves] (Samuel Neves)
* [@veorq] (Jean-Philippe Aumasson)
* [@zookozcash] (Zooko)

The development of BLAKE3 was sponsored by [Electric Coin Company](https://electriccoin.co).

BLAKE3 is also [specified](https://c2sp.org/BLAKE3) in the [Community
Cryptography Specification Project (C2SP)](https://c2sp.org).

*NOTE: BLAKE3 is not a password hashing algorithm, because it's
designed to be fast, whereas password hashing should not be fast. If you
hash passwords to store the hashes or if you derive keys from passwords,
we recommend [Argon2](https://github.com/P-H-C/phc-winner-argon2).*

## Usage

### The `b3sum` utility

The `b3sum` command line utility prints the BLAKE3 hashes of files or of
standard input. Prebuilt binaries are available for Linux, Windows, and
macOS (requiring the [unidentified developer
workaround](https://support.apple.com/guide/mac-help/open-a-mac-app-from-an-unidentified-developer-mh40616/mac))
on the [releases page](https://github.com/BLAKE3-team/BLAKE3/releases).
If you've [installed Rust and
Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html),
you can also build `b3sum` yourself with:

```bash
cargo install b3sum
```

If `rustup` didn't configure your `PATH` for you, you might need to go
looking for the installed binary in e.g. `~/.cargo/bin`. You can test
out how fast BLAKE3 is on your machine by creating a big file and
hashing it, for example:

```bash
# Create a 1 GB file.
head -c 1000000000 /dev/zero > /tmp/bigfile
# Hash it with SHA-256.
time openssl sha256 /tmp/bigfile
# Hash it with BLAKE3.
time b3sum /tmp/bigfile
```

### The `blake3` crate [![docs.rs](https://docs.rs/blake3/badge.svg)](https://docs.rs/blake3)

To use BLAKE3 from Rust code, add a dependency on the `blake3` crate to
your `Cargo.toml`. Here's an example of hashing some input bytes:

```rust
// Hash an input all at once.
let hash1 = blake3::hash(b"foobarbaz");

// Hash an input incrementally.
let mut hasher = blake3::Hasher::new();
hasher.update(b"foo");
hasher.update(b"bar");
hasher.update(b"baz");
let hash2 = hasher.finalize();
assert_eq!(hash1, hash2);

// Extended output. OutputReader also implements Read and Seek.
let mut output = [0; 1000];
let mut output_reader = hasher.finalize_xof();
output_reader.fill(&mut output);
assert_eq!(hash1, output[..32]);

// Print a hash as hex.
println!("{}", hash1);
```

Besides `hash`, BLAKE3 provides two other modes, `keyed_hash` and
`derive_key`. The `keyed_hash` mode takes a 256-bit key:

```rust
// MAC an input all at once.
let example_key = [42u8; 32];
let mac1 = blake3::keyed_hash(&example_key, b"example input");

// MAC incrementally.
let mut hasher = blake3::Hasher::new_keyed(&example_key);
hasher.update(b"example input");
let mac2 = hasher.finalize();
assert_eq!(mac1, mac2);
```

The `derive_key` mode takes a context string and some key material (not a
password). The context string should be hardcoded, globally unique, and
application-specific. A good default format for the context string is
`"[application] [commit timestamp] [purpose]"`:

```rust
// Derive a couple of subkeys for different purposes.
const EMAIL_CONTEXT: &str = "BLAKE3 example 2020-01-07 17:10:44 email key";
const API_CONTEXT: &str = "BLAKE3 example 2020-01-07 17:11:21 API key";
let input_key_material = b"usually at least 32 random bytes, not a password";
let email_key = blake3::derive_key(EMAIL_CONTEXT, input_key_material);
let api_key = blake3::derive_key(API_CONTEXT, input_key_material);
assert_ne!(email_key, api_key);
```

### The C implementation

See [`c/README.md`](c/README.md).

### Other implementations

There are too many implementations out there for us to keep track of,
but some highlights include [an optimized Go
implementation](https://github.com/zeebo/blake3), [Wasm bindings for
Node.js and browsers](https://github.com/connor4312/blake3), [binary
wheels for Python](https://github.com/oconnor663/blake3-py), [.NET
bindings](https://github.com/xoofx/Blake3.NET), and [a pure Java
implementation](https://commons.apache.org/proper/commons-codec/apidocs/org/apache/commons/codec/digest/Blake3.html).

## Contributing

Please see [CONTRIBUTING.md](CONTRIBUTING.md).

## Licenses

This work is released into the public domain with [CC0 1.0](./LICENSE_CC0).
Alternatively, it is licensed under any of the following:

* [Apache 2.0](./LICENSE_A2)
* [Apache 2.0 with LLVM exceptions](./LICENSE_A2LLVM)


## Adoption & deployment

* [Bazel](https://github.com/bazelbuild/bazel/releases/tag/6.4.0)
* [Cargo](https://github.com/rust-lang/cargo/pull/14137)
* [Ccache](https://ccache.dev/releasenotes.html#_ccache_4_0)
* [Chia](https://github.com/Chia-Network/chia-blockchain/blob/main/CHANGELOG.md#10beta8-aka-beta-18---2020-07-16)
* [Clickhouse](https://github.com/ClickHouse/ClickHouse/blob/master/rust/chcache/Cargo.toml#L7)
* [Farcaster](https://www.farcaster.xyz/)
* [IPFS](https://github.com/ipfs/go-verifcid/issues/13)
* [Iroh](https://www.iroh.computer/blog/blake3-hazmat-api)
* [LLVM](https://reviews.llvm.org/D121510)
* [Nix](https://github.com/NixOS/nix/pull/12379)
* [Nym](https://github.com/nymtech/nym/blob/59056a22c5e6b01a38da2124662bd1fa3c8abef2/common/nymsphinx/params/src/lib.rs#L5)
* [OpenZFS](https://github.com/openzfs/zfs/)
* [Redox](https://www.redox-os.org/news/pkgar-introduction/)
* [Solana](https://docs.rs/solana-program/1.9.5/solana_program/blake3/index.html)
* [Tekken 8](https://x.com/rodarmor/status/1751567502050771189)
* [Wasmer](https://github.com/wasmerio/wasmer/blob/4f935a8c162bf604df223003e434e4f7ca253688/lib/cache/src/hash.rs#L21)


## Miscellany

- [@veorq] and [@oconnor663] did [an interview with Cryptography FM](https://cryptography.fireside.fm/3).
- [@oconnor663] did [an interview with Saito](https://www.youtube.com/watch?v=cJkmIt7yN_E).

[@oconnor663]: https://github.com/oconnor663
[@sneves]: https://github.com/sneves
[@veorq]: https://github.com/veorq
[@zookozcash]: https://github.com/zookozcash
