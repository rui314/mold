# mold-macho: mold for macOS

mold-macho is a Mach-O linker for macOS, written in Rust as a port of
the [mold linker](https://github.com/rui314/mold) to Apple platforms.
It is a drop-in replacement for Apple's `ld` for common workloads: it
accepts ld64's command line, and links working, correctly code-signed
executables, dylibs and bundles for arm64 and x86-64 macOS.

C, C++, Objective-C, Swift and Rust programs link and run, including
real-world projects (ripgrep builds and passes its full test suite;
the C++ mold tree with vendored tbb/zstd/zlib builds at -O3; the
linker links itself). Debugging works (stabs for lldb/dsymutil), and
so do C++ exceptions, thread-locals, LTO, auto-linking, frameworks and
Objective-C selector stubs.

## Usage

Build with `cargo build --release`. The binary is `target/release/mold`;
`./install.sh` (PREFIX=/usr/local by default) installs it as
`mold` with an `ld64.mold` symlink, the same arrangement as mold's
`ld.mold`. Then point your compiler driver at the linker:

    clang -o hello hello.c --ld-path=path/to/ld64.mold
    swiftc -o hello hello.swift -use-ld=path/to/ld64.mold
    RUSTFLAGS="-C link-arg=--ld-path=path/to/ld64.mold" cargo build

## Feature highlights

- Classic dyld info and chained fixups (the default for deployment
  targets of macOS 13+), ad-hoc code signing, export tries, and
  content-hash UUIDs
- Subsections via symbols, `-dead_strip`, identical code folding
  (on by default, like ld64's deduplication), literal merging
- `__unwind_info` synthesis from compact unwind, `__eh_frame`
  re-synthesis for DWARF-only unwind, range-extension thunks
- Archives with mold's order-insensitive resolution model, .tbd stubs
  with umbrella reexports, frameworks, auto-linking
  (LC_LINKER_OPTION), LTO via libLTO
- Parallel input parsing, output writing and code-signature hashing,
  following the design described in the
  [mold paper](https://arxiv.org/abs/2608.23228)

## Architecture

The code mirrors mold's layout: `src/driver.rs` runs the passes in
order, `src/passes.rs` implements them, `src/target/` isolates the
target-dependent relocation handling (arm64 and x86-64 are each
instantiated in a crate under `arch/` and dispatched by the
executable in `cli/`), and `src/chunks/` builds every piece of
the output file. Parsing is decoupled from resolution: all inputs,
including every archive member, are parsed in parallel, and symbol
resolution ranks competing definitions with a liveness walk deciding
which archive members join the link.

The commit history doubles as a reference on the Mach-O file format
and dyld: each commit's message explains the structures and loader
behavior involved in that change.

## Tests

Tests are shell scripts under `tests/`, one feature per script,
driving the real toolchain through `cc --ld-path=...`; a harness runs
each for arm64 and (under Rosetta) x86-64, writing logs and outputs
under `target/<profile>/mold-test/out/test`. `--all` runs them for the
iOS, tvOS and visionOS simulators too (those with a runtime installed),
and `--triple` (or `TRIPLE`) for one simulator: the programs are built
with its SDK and run on a simulator device the harness boots. The
harness is the `integration` test of the `cli` crate; name that target
to pass it options, which the unit tests would otherwise reject:

    cargo test
    cargo test --test integration -- dead-strip --native --timeout 120
    cargo test --test integration -- --triple arm64-apple-ios-simulator
    cargo test --test integration -- --list

## License

MIT, like mold.
