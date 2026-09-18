//! Padding at the end of a RELRO segment.

use crate::chunks::ChunkHeader;
use crate::elf::*;

// PT_GNU_RELRO works on page granularity. We want to align its end to
// a page boundary. We append this section at end of a segment so that
// the segment always ends at a page boundary.
pub fn new_header<E: Layout>() -> ChunkHeader<E> {
    let mut hdr =
        ChunkHeader::<E>::new(".relro_padding", SHT_NOBITS, (SHF_ALLOC | SHF_WRITE) as u64);
    hdr.is_relro = true;
    hdr.shdr.sh_size.set(1);
    hdr
}
