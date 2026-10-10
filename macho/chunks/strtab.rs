//! This file contains the string table of the symbol table (see
//! symtab.rs), like ELF's .strtab. Its contents are written along with the
//! symbol table.

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
