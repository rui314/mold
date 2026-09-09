# Upstream synchronization

The Rust port incorporates the applicable C++ mold changes from
`92eb17a07c60a88b3a1a8c9cef014ac84eaf0cf9` through
`dad2695f3e62d0dd6024b0b795dc2f940907be12` (September 8, 2026).

| Upstream commits | Rust implementation |
| --- | --- |
| `60606576`, `18d17369` | Cache dynamic symbol names and string offsets; compute offsets by block prefix sums and write both dynamic tables in parallel. |
| `cbcf79c0`, `85663581` | Stable parallel symbol partitioning and contiguous GNU hash sort keys. |
| `07061983`, `9887339f`, `41d2f376` | Allocate auxiliary records by input file in first-use order, initialize records and assign indices in parallel, prefetch symbols and records, and inline small table insertion functions. |
| `477db5f3`, `95137b23`, `ec7c8032` | Parallel PLT string sizing, defined-symbol version indices, and version conflict checks. |
| `046a6449` | Evaluate the GNU symbol hash four bytes at a time. |
| `2c402f70` | Cache consecutive debug-relocation symbol lookups and try the next string fragment before binary search; prefetch subsequent fragments. Each relocation stream owns its cache. |
| `0f582b0a`, `f585fee6` | Hash fragments while splitting input; merge and lay out non-allocated sections in the background. The background task owns its metadata and publishes it before output section sorting. |
| `f8cb3daf` | Stop pattern matching once its maximum priority is found; share priorities between consecutive patterns assigning the same version. |
| `84188021` | Keep flat symbol-map shards, but mix out the fixed shard-selection bits before bucket placement and compare cached hashes before strings. Preserve hashbrown's high-bit lookup tags. |
| `f9bdf4b4` | Create fragment-symbol records while reattaching relocations, then publish exactly those symbols to the central vector. Remove the preliminary relocation counting pass. |
| `3a58944f`, `53715c07` | Scan global names and version separators together; reuse cached version flags for ordinary COMDAT signatures. |
| `91252d8a`, `ff0cb501` | Skip redundant COMDAT updates on newly parsed files; size the CREL section table once. |
| `fcd9dea7` | Test Debian 12 in CI. |

The remaining commits already have appropriate Rust equivalents:

- `95dbca67`: GC roots are collected in local vectors by Rayon workers.
- `fa1e8edc`: `Symbol` already stores exact `u32` name lengths, and remains 48 bytes.
- `e9de4def`: One parallel iteration already writes the disjoint symbol-table ranges belonging to chunks, objects, and shared libraries.
- `4a11b784`: Rust uses vectors and mimalloc; the C++ thread-local arena block size has no corresponding allocator setting here.
- `dad2695f`: Rust formats temporary output names without the C++ string expression that triggered `-Wrestrict`.

The shell test cases and their file modes remain identical to this upstream
revision. Rust unit tests additionally cover the partition, hash, pattern
priority, and fragment-hint changes.

Rust-specific details preserve performance without changing the upstream
algorithms. Fragment reattachment keeps compact temporary records before
constructing the final symbols, and retains a cheap CREL-header capacity
estimate to avoid moving the central symbol vector. It does not scan
relocations to count fragment symbols.

Auxiliary symbol records use four-byte optional indices, matching C++'s
64-byte record size. Per-file allocation preserves first-use order without
sorting global symbol IDs; the owning-file filter guarantees disjoint writes.
A unit test covers duplicate IDs and preservation of existing records.
GC avoids repeated writes when a shared library is
already marked reachable and allocates child-work buffers lazily. The
literal-pattern matcher inlines each byte transition. ICF feeds complete
digest words to SipHash directly; the byte order and hash are unchanged.
A unit test compares word updates with byte updates, including partial
buffers and length wraparound.

Symbol address calculation inlines its common path and keeps rare
discarded-section diagnostics out of line. PLT checks read the auxiliary
record once. This preserves address and diagnostic behavior while reducing
the cost of relocation and symbol-table writes.

Validation on September 9, 2026:

- The standard release build compiles all 20 linker targets.
- The full integration run passed 5,447 cases, skipped 649, and failed none
  across the 14 available target configurations. Compiler/QEMU combinations
  for `aarch64_be`, `armeb`, `riscv32`, and `sh4aeb` were unavailable.
- All 20 Rust library unit tests pass in release mode.
- Workspace Clippy with all targets and `-D warnings`, formatting, and
  whitespace checks pass.

The [release performance comparison](upstream-sync-performance.md) covers 21
workloads. Updated Rust takes 6.1% less time than the original Rust build by
geometric mean and 2.9% more time than C++. The largest remaining C++ gap is
9.5% on Chromium ARM64 debug. The report includes the build configuration,
noise checks, and raw samples.
