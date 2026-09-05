//! Padding at the end of a RELRO segment.

use crate::elf::*;
use crate::output_chunks::ChunkHeader;

// PT_GNU_RELRO works on page granularity. We want to align its end to
// a page boundary. We append this section at end of a segment so that
// the segment always ends at a page boundary.
#[derive(Debug)]
pub struct RelroPaddingSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Layout> RelroPaddingSection<E> {
    pub fn new() -> RelroPaddingSection<E> {
        let mut hdr =
            ChunkHeader::<E>::new(".relro_padding", SHT_NOBITS, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = true;
        hdr.shdr.sh_size.set(1);
        RelroPaddingSection { hdr }
    }
}

impl<E: Layout> Default for RelroPaddingSection<E> {
    fn default() -> Self {
        Self::new()
    }
}
