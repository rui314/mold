# Architecture crates

Each subdirectory is a small crate that instantiates the generic Mach-O
linker for one target architecture. Separate crates let Cargo compile the
architectures in parallel. Instantiating all targets in one crate leaves a
long serial compilation step. A directory is named after its crate without
the `mold-` prefix, e.g. `macho-arm64` holds `mold-macho-arm64`.

The shared linker implementation lives in [`macho/`](../macho/), with
target-specific code in [`macho/target/`](../macho/target/). These crates
provide entry points that the [`cli/`](../cli/) executable selects for the
input files' target.
