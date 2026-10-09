//! The string table in __LINKEDIT.

use crate::chunks::ChunkHeader;

/// The string table.
#[derive(Debug)]
pub struct StrtabSection {
    pub hdr: ChunkHeader,
}

impl StrtabSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self { hdr }
    }
}

impl Default for StrtabSection {
    fn default() -> Self {
        Self::new()
    }
}
