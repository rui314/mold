//! LC_ATOM_INFO: the mergeable record of a -make_mergeable dylib, which
//! make_mergeable builds (see there).

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::mergeable::header;

/// LC_ATOM_INFO's data in __LINKEDIT: the record, but for what depends
/// on where it lands in the file, filled in as it is copied out.
#[derive(Debug)]
pub struct MergeableRecordSection {
    pub hdr: ChunkHeader,
    /// The record, with the content offsets of the entries whose bytes
    /// are the image's left to fill.
    pub contents: Vec<u8>,
    /// Where in the record each such offset goes, and the file offset
    /// of the bytes it points at.
    pub image_contents: Vec<(u32, u64)>,
    /// The content pool's offset in the record.
    pub pool_offset: u32,
}

impl MergeableRecordSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self { hdr, contents: Vec::new(), image_contents: Vec::new(), pool_offset: 0 }
    }
}

impl Default for MergeableRecordSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Copies the record out, pointing its entries at their bytes now that
/// the record has its place: by offsets back from the content pool, and
/// the whole file by one back from the record.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = &ctx.mergeable_record;
    buf[..sec.contents.len()].copy_from_slice(&sec.contents);
    let pool = sec.hdr.fileoff as i64 + sec.pool_offset as i64;
    for &(at, fileoff) in &sec.image_contents {
        let off = (fileoff as i64 - pool) as i32;
        buf[at as usize..at as usize + 4].copy_from_slice(&off.to_le_bytes());
    }
    let image = header::IMAGE;
    buf[image..image + 4].copy_from_slice(&(-(sec.hdr.fileoff as i32)).to_le_bytes());
    buf[image + 4..image + 8].copy_from_slice(&(ctx.output_size as u32).to_le_bytes());
}
