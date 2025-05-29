# b3sum

A command line utility for calculating
[BLAKE3](https://github.com/BLAKE3-team/BLAKE3) hashes, similar to
Coreutils tools like `b2sum` or `md5sum`.

```
Usage: b3sum [OPTIONS] [FILE]...

Arguments:
  [FILE]...  Files to hash, or checkfiles to check

Options:
      --keyed                 Use the keyed mode, reading the 32-byte key from stdin
      --derive-key <CONTEXT>  Use the key derivation mode, with the given context string
  -l, --length <LEN>          The number of output bytes, before hex encoding [default: 32]
      --seek <SEEK>           The starting output byte offset, before hex encoding [default: 0]
      --num-threads <NUM>     The maximum number of threads to use
      --no-mmap               Disable memory mapping
      --no-names              Omit filenames in the output
      --raw                   Write raw output bytes to stdout, rather than hex
      --tag                   Output BSD-style checksums: BLAKE3 ([FILE]) = [HASH]
  -c, --check                 Read BLAKE3 sums from the [FILE]s and check them
      --quiet                 Skip printing OK for each checked file
  -h, --help                  Print help (see more with '--help')
  -V, --version               Print version
```

See also [this document about how the `--check` flag
works](https://github.com/BLAKE3-team/BLAKE3/blob/master/b3sum/what_does_check_do.md).

# Example

Hash the file `foo.txt`:

```bash
b3sum foo.txt
```

Time hashing a gigabyte of data, to see how fast it is:

```bash
# Create a 1 GB file.
head -c 1000000000 /dev/zero > /tmp/bigfile
# Hash it with SHA-256.
time openssl sha256 /tmp/bigfile
# Hash it with BLAKE3.
time b3sum /tmp/bigfile
```


# Installation

Prebuilt binaries are available for Linux, Windows, and macOS (requiring
the [unidentified developer
workaround](https://support.apple.com/guide/mac-help/open-a-mac-app-from-an-unidentified-developer-mh40616/mac))
on the [releases page](https://github.com/BLAKE3-team/BLAKE3/releases).
If you've [installed Rust and
Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html),
you can also build `b3sum` yourself with:

```
cargo install b3sum
```

On Linux for example, Cargo will put the compiled binary in
`~/.cargo/bin`. You might want to add that directory to your `$PATH`, or
`rustup` might have done it for you when you installed Cargo.

If you want to install directly from this directory, you can run `cargo
install --path .`. Or you can just build with `cargo build --release`,
which puts the binary at `./target/release/b3sum`.
