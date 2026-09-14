# Simplification implementation — September 15, 2026

All 28 candidates in the [original audit](simplification-audit.md) have been implemented, each in a separate commit, with one additional relocation review fix. The starting point was `47a35e8`; the implementation ends at `3ccb8e2`. The changes remove a net 726 lines across 63 existing or new source and configuration files, including the added tests. Documentation is excluded from that count.

**No benchmarks were run.** The user requested implementation of all candidates and deferred measurements because the machine is busy. Correctness checks do not establish acceptable performance. In particular, items 22–28 remain experiments pending measurement.

## Commits

| Audit item | Commit | Result |
| --- | --- | --- |
| 1 | `aa4a728` | Remove unused `TargetInfo` and architecture metadata registry. |
| 2 | `1e8dae1` | Remove the priority registry and DSO keep mask; retain stable backing storage. |
| 3 | `f7e0ff8` | Remove the unused machine-detection callback. |
| 4 | `2308083` | Remove unused symbol and signed-endian helpers. |
| 5 | `de4910f` | Keep parser-only settings local and update diagnostic state directly. |
| 6 | `7d6d588` | Derive page size and glob emptiness from existing state. |
| 7 | `95fef65` | Replace nineteen header-only wrappers with `ChunkHeader`. |
| 8 | `d677902` | Generate shared and mutable chunk-header access from one mapping. |
| 9 | `2712c5e` | Share paired ELF accessors while preserving explicit on-disk layouts. |
| 10 | `4980d26` | Centralize persistent worker bins and caches in `WorkerLocal`. |
| 11 | `d6039a2` | Centralize background job ownership and cooperative joins. |
| 12 | `67ba746` | Introduce a borrowed argument cursor and declarations for simple switches and LTO forwarding. |
| 13 | `1c63947` | Share nonallocated relocation preparation across architecture backends. |
| 14 | `284c571` | Share output symbol and addend mapping for emitted relocations. |
| 15 | `15b984a` | Sort owned reader results before assigning file IDs and priorities. |
| 16 | `9bca3ea` | Traverse archive members lazily through one iterator. |
| 17 | `65479de` | Store output files together with their mapping state. |
| 18 | `9d9f0e9` | Compute final statistics locally instead of registering global atomic counters. |
| 19 | `8e0967d` | Stream dependency reports through the existing buffered writer. |
| 20 | `8a56434` | Limit implementation-module visibility and remove stale or repeated explanations. |
| 21 | `bb257d5` | Flatten test-process errors and finalize status files on every result path. |
| 22 | `c77f241` | Replace raw CIE handles with indices and share CIE deduplication. |
| 23 | `19162be` | Use the standard indexed parallel iterator over collected live-file references. |
| 24 | `46bbf3d` | Collect owned sparse output-section contributions per file. |
| 25 | `ebc789e` | Retain existing concurrent-map entry IDs for GDB names. |
| 26 | `e918027` | Use bounded static chunks for GDB background work. |
| 27 | `b2effe0` | Use safe, bounded `memchr` searches for NUL-terminated strings. |
| 28 | `4658ec4` | Reuse native zlib for decompression and remove the alternate backend dependency subtree. |
| Review fix for 14 | `3ccb8e2` | Preserve lazy addend reads for discarded section references and ordinary REL `.eh_frame` symbols. |

The twenty architecture instantiation crates, compact symbol and GDB record layouts, stable boxed file ownership, specialized concurrent tables, worker caches, output mapping optimizations, parallel compression, fast glob paths, and compatibility branches remain in place. Reducing module visibility may require source changes for external consumers of internal `mold` library modules; workspace consumers compile against the retained public interfaces.

## Correctness validation

The workspace check passed for all twenty architecture crates and the CLI without warnings:

```sh
cargo check --locked --offline --workspace -j 4
```

After the relocation review fix, all **34 unit tests** passed: 32 linker tests and two test-runner tests.

```sh
cargo test --locked --offline -p mold -p mold-tests --lib -j 4
```

Focused coverage added during the refactors exercises persistent worker storage, cooperative joining with one worker, GNU argument boundaries and raw path bytes, file-map transitions, live-file ordering and retained backing storage, and skipped implicit-addend reads. Existing unit tests cover the remaining utility behavior.

Symbol inspection of the complete release executable found no standalone definitions of `resolve_nonalloc` or `output_symidx_addend`, consistent with the intended helper inlining. This is a code-generation check, not a throughput measurement.

The full release shell suite passed with four concurrent tests and a longer correctness-test timeout for the busy host:

```sh
cargo test --locked --offline --release -p mold-cli --test integration -j 4 -- \
  --all --test-threads 4 --timeout 180
```

**6,279 shell cases completed: 5,630 passed, 649 skipped, and zero failed.** The suite exercised 14 target configurations: 13 target families plus the PPC64LE POWER10 configuration.

| Target | Passed | Skipped | Failed |
| --- | ---: | ---: | ---: |
| aarch64 | 409 | 37 | 0 |
| arm | 401 | 49 | 0 |
| i686 | 406 | 39 | 0 |
| loongarch64 | 394 | 54 | 0 |
| m68k | 380 | 60 | 0 |
| ppc | 393 | 47 | 0 |
| ppc64 | 385 | 56 | 0 |
| ppc64le | 393 | 49 | 0 |
| ppc64le-power10 | 391 | 51 | 0 |
| riscv64 | 409 | 46 | 0 |
| s390x | 395 | 47 | 0 |
| sh4 | 378 | 62 | 0 |
| sparc64 | 393 | 47 | 0 |
| x86_64 | 503 | 5 | 0 |
| total | 5630 | 649 | 0 |

The runner reported these unavailable targets because a compiler or QEMU was missing: `aarch64_be`, `armeb`, `riscv32`, and `sh4aeb`. Runtime coverage is limited to the configurations in the table; all twenty architecture crates received build coverage. Per-test logs and status files are in `target/release/mold-test/out/test/results/`.

The native LTO claim-order and mixed-archive tests, GDB-index determinism and compressed-output tests, compressed-debug-input tests, relocatable exception tests, and emitted C++ relocation tests all passed. The offline dependency tree confirms that `flate2` uses `libz-sys`; the alternate `miniz_oxide` backend is no longer active.

## Deferred performance checks

Use `47a35e8` as the overall baseline and the parent of each relevant commit for attribution. Build baseline and candidate with the same compiler, release profile, target features, and allocator. Run measurements only after the user confirms that the host is quiet. The [existing performance report](upstream-sync-performance.md) supplies workload and measurement context; its older results do not validate these changes.

| Changes | What needs measurement |
| --- | --- |
| Worker storage, reader ordering, background helper (10, 11, 15) | Small and many-file links at one and many threads; allocation and peak RSS. Worker slots retain locks and are now consistently padded to 128-byte alignment. |
| Shared relocation preparation and mapping (13, 14) | Relocation-heavy links, emitted relocations, and `-r`; inspect optimized code for helper inlining. |
| Indexed CIE deduplication (22) | Exception-heavy C++ and ICF, including many distinct CIEs. Leaders now carry indices plus the assigned value, with extra indexed access and a larger leader record. |
| Standard mutable file iterator (23) | Many-file links and repeated mutable passes. Each invocation allocates a reference vector, approximately eight bytes per live file on a 64-bit host. |
| Sparse section contributions (24) | Many files, many output sections, and very large input-section counts. Dense shared cells are gone; per-file grouping and a serial sparse transpose precede the retained parallel flattening and bulk copy. |
| GDB entry IDs (25) | Large GDB indexes with millions of names, including indexing time and RSS. Each ID needs resolution through the map. |
| Static GDB chunks (26) | Uneven compilation units and name counts, plus one-thread operation. The worker cap remains; static chunks may balance less evenly than the removed work queue. |
| Bounded NUL searches (27) | Large symbol tables, short and long names, and debug input. `memchr` and libc may differ in dispatch and throughput. |
| Native zlib decoding (28) | Compressed debug input and prefix reads. Decoder throughput and malformed-input error reporting may differ. |

Use warm-ups, rotated run order, multiple measured rounds, separate output files, and consistent `--no-fork` settings; record elapsed time, CPU time, and peak RSS. Compare representative Chrome, TensorFlow, Clang, and Godot inputs, plus small links and the special cases above. The original audit's proposed 1–2% aggregate and 5% per-workload slowdown limits are discussion points, not measured results or user-specified thresholds. Revise or revert individual experimental commits if their measured cost is unacceptable.
