//! The string table in __LINKEDIT.

use crate::chunks::ChunkHeader;

/// The string table.
#[derive(Debug)]
pub struct StrtabSection {
    pub hdr: ChunkHeader,
}

impl StrtabSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit() }
    }
}

impl Default for StrtabSection {
    fn default() -> Self {
        Self::new()
    }
}
