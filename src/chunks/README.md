# Output chunks

A chunk is a contiguous region of the output file: an ELF header, an
output section built from input sections, or a section the linker
synthesizes. Each kind of section has its own file in this directory.
[`mod.rs`](mod.rs) defines `ChunkId` and `ChunkHeader`, which every chunk
has, and handles the ELF headers.
