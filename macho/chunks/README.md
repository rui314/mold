# Output chunks

A chunk is a contiguous region of the output file: the Mach header with
its load commands, an output section built from input sections, or a
table the linker synthesizes in `__LINKEDIT`. Each kind of section has
its own file in this directory. [`mod.rs`](mod.rs) defines `ChunkId` and
`ChunkHeader`, which every chunk has, and handles the Mach header and
load commands.
