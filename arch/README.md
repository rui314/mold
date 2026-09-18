# Architecture crates

Each subdirectory is a small crate that instantiates the generic linker for
one target architecture. Separate crates let Cargo compile the architectures
in parallel. Instantiating all targets in one crate leaves a long serial
compilation step.

The shared linker implementation lives in [`src/`](../src/), with
architecture-specific code in [`src/arch/`](../src/arch/). These crates provide
entry points that the [`cli/`](../cli/) executable selects for the input
files' target.
