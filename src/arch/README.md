# Target architectures

This directory contains one file per instruction set or per ABI. Each
file defines a type per target it supports, and the types implement
`Arch` from [`mod.rs`](mod.rs), which holds the constants and code that
differ between targets. The rest of the linker is generic over it. The
crates under [`arch/`](../../arch/) instantiate the linker for each type.
