//! The merged __objc_imageinfo section: the Objective-C runtime reads
//! exactly one 8-byte record per image.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::target::Target;

/// The merged __objc_imageinfo section: the Objective-C runtime reads
/// exactly one 8-byte record per image.
#[derive(Debug)]
pub struct ObjcImageInfoSection {
    pub hdr: ChunkHeader,
    /// The merged flags word.
    pub flags: u32,
}

impl ObjcImageInfoSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__DATA", b"__objc_imageinfo");
        hdr.p2align = 2;
        Self { hdr, flags: 0 }
    }
}

impl Default for ObjcImageInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    buf[..4].copy_from_slice(&0u32.to_le_bytes());
    buf[4..8].copy_from_slice(&ctx.objc_imageinfo.flags.to_le_bytes());
}
