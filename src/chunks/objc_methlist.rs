//! __TEXT,__objc_methlist: the Objective-C method lists rewritten in the
//! relative (12-byte entry) form, which needs no fixups.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::objc::{ObjcMethList, objc_ref_addr};
use crate::target::Target;

/// __TEXT,__objc_methlist: the Objective-C method lists rewritten in
/// the relative (12-byte entry) form, which needs no fixups.
#[derive(Debug)]
pub struct ObjcMethlistSection {
    pub hdr: ChunkHeader,
    /// The rewritten lists, each with its synthetic subsection here.
    pub lists: Vec<ObjcMethList>,
}

impl ObjcMethlistSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__TEXT", "__objc_methlist");
        hdr.p2align = 3;
        Self { hdr, lists: Vec::new() }
    }
}

impl Default for ObjcMethlistSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for list in &ctx.objc_methlist.lists {
        let isec = &ctx.isecs[list.isec as usize];
        let base = isec.offset as usize;
        let addr = ctx.objc_methlist.hdr.addr + base as u64;
        let count = list.methods.len() as u32;
        buf[base..base + 4].copy_from_slice(&(12u32 | 0x8000_0000).to_le_bytes());
        buf[base + 4..base + 8].copy_from_slice(&count.to_le_bytes());
        for (i, m) in list.methods.iter().enumerate() {
            let at = base + 8 + 12 * i;
            let field = addr + 8 + 12 * i as u64;
            for (k, r) in [m.name, m.types, m.imp].into_iter().enumerate() {
                let target = objc_ref_addr(ctx, r);
                let rel =
                    if target == 0 { 0 } else { target.wrapping_sub(field + 4 * k as u64) as i64 };
                if rel != rel as i32 as i64 {
                    crate::fatal!("relative method list entry out of range");
                }
                buf[at + 4 * k..at + 4 * k + 4].copy_from_slice(&(rel as i32).to_le_bytes());
            }
        }
    }
}
