# Targets

This directory contains one file per instruction set. Each file defines a
type for the target it supports, and the types implement `Target` from
[`mod.rs`](mod.rs), which holds the constants and code that differ between
targets. The rest of the linker is generic over it. The crates under
[`arch/`](../../arch/) instantiate the linker for each type.
