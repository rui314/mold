# mold, in Rust

A port of the [mold](https://github.com/rui314/mold) linker (version
2.42.0) to Rust. It is a drop-in replacement for `ld`: it takes the same
command line, produces byte-for-byte the same kind of output, and passes
mold's own test suite.

## Building

```
cargo build --release
```

The binary is `target/release/mold`. It answers to `ld` as well, so a
symlink named `ld` in a directory passed to the compiler with `-B` makes
GCC and Clang use it. The build also compiles three C parts: mimalloc from
the pinned `mimalloc_rust` dependency, used as the global allocator,
`mold-wrapper.so`, the preload library behind `mold -run`, and the variadic
adapter the LTO plugin API needs.

The linker is generic over the target, and instantiating it for all
twenty targets in one crate keeps the compiler on a single core for
minutes. So the crate is a workspace: `mold` is the generic library, each
`targets/<name>` crate instantiates it for one target, and `cli` is the
executable, with a feature per target. Building the executable alone for
one target is much faster:

```
cargo build --release -p mold-cli --no-default-features --features x86_64
```

## Testing

The tests are mold's own shell scripts, copied unchanged into `tests/cases/`.
`cargo test` runs the Rust unit tests and the complete available shell-test
matrix in parallel:

```
cargo test
cargo test tls-
cargo test -- --test-threads 8
cargo test -p mold-cli --test integration -- --native
cargo test -p mold-cli --test integration -- \
  --triple aarch64-linux-gnu
```

The last two forms select only the native target or one cross target. Logs go
to `target/debug/mold-test/out/test/results/<machine>/` (or the corresponding
Cargo profile directory). As in mold's CMake setup, a target runs the generic
tests plus the ones prefixed with its architecture; tests whose prerequisites
are missing skip themselves.

## Targets

x86-64, i386, ARM64 (both byte orders), ARM32 (little-endian and BE8),
RISC-V (32- and 64-bit, both byte orders), PowerPC (32-bit, and 64-bit
ELFv1 and ELFv2), s390x, SPARC64, m68k, SH-4 (both byte orders) and
LoongArch (32- and 64-bit). Every target for which a cross toolchain was
available passes its full suite: x86-64, i386, ARM64, ARM32, RISC-V 64,
PowerPC 32/64/64LE, s390x, SPARC64, m68k and SH-4.

## Layout

| Module | What it holds |
| --- | --- |
| `driver` | The link in order: reading inputs, resolving symbols, laying out the output, writing it |
| `args`, `linker_script` | Command line and script parsing |
| `input_files`, `input_sections`, `symbol` | Object files, shared libraries, their sections and symbols |
| `passes`, `gc_sections`, `icf`, `relax`, `thunks` | The link's passes, from symbol resolution to range extension thunks |
| `chunks` | Everything that ends up in the output: output sections and the synthesized ones (`.got`, `.plt`, `.dynamic`, `.eh_frame`, ...) |
| `arch` | One module per target: relocation scanning and application, PLT stubs, thunks, relaxation |
| `lto`, `gdb_index`, `output_file`, `elf`, `util` | The LTO plugin bridge, `.gdb_index` generation, output files, the ELF format and helpers |
