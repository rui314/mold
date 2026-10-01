//! __TEXT,__chain_starts: where the fixup chains of an image no dyld
//! loads start, for its own loader to walk (-fixup_chains_section), in
//! place of the LC_DYLD_CHAINED_FIXUPS payload.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::target::Target;

/// __TEXT,__chain_starts: a dyld_chained_starts_offsets - the chains'
/// pointer format, their count, then each chain's first fixup as an
/// offset from the image's address, and zeros for the room ld-prime
/// leaves (see chained_fixups::section_chain_starts).
#[derive(Debug)]
pub struct ChainStartsSection {
    pub hdr: ChunkHeader,
    /// The offsets of the chains' first fixups.
    pub starts: Vec<u32>,
}

impl ChainStartsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__chain_starts");
        hdr.p2align = 2;
        hdr.size = Self::size(0);
        Self { hdr, starts: Vec::new() }
    }

    /// The section's size with room for `n` chains.
    pub fn size(n: usize) -> u64 {
        8 + n as u64 * 4
    }
}

impl Default for ChainStartsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let starts = &ctx.chain_starts.starts;
    let format = crate::chunks::chained_fixups::pointer_format(ctx) as u32;
    buf[0..4].copy_from_slice(&format.to_le_bytes());
    buf[4..8].copy_from_slice(&(starts.len() as u32).to_le_bytes());
    for (i, &start) in starts.iter().enumerate() {
        buf[8 + i * 4..12 + i * 4].copy_from_slice(&start.to_le_bytes());
    }
}
