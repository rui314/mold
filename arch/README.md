# Architecture crates

Each subdirectory is a small crate that instantiates the generic ELF or
Mach-O linker for one target architecture. Separate crates let Cargo compile
the architectures in parallel. Instantiating all targets in one crate leaves
a long serial compilation step. A directory is named after its crate without
the `mold-` prefix, e.g. `elf-x86_64` holds `mold-elf-x86_64`.

The ELF linker lives in [`elf/`](../elf/), with target-specific code in
[`elf/arch/`](../elf/arch/), and the Mach-O linker in [`macho/`](../macho/),
with target-specific code in [`macho/arm64.rs`](../macho/arm64.rs) and
[`macho/x86_64.rs`](../macho/x86_64.rs). These crates provide entry points
that the [`cli/`](../cli/) executable selects for the input files' target.
