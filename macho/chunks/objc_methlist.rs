//! __TEXT,__objc_methlist: the Objective-C method lists rewritten in the
//! relative (12-byte entry) form, which needs no fixups.

use crate::arch::Target;
use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::objc::ObjcMethList;

/// __TEXT,__objc_methlist: the Objective-C method lists rewritten in
/// the relative (12-byte entry) form, which needs no fixups.
#[derive(Debug)]
pub struct ObjcMethlistSection {
    pub hdr: ChunkHeader,
    /// The rewritten lists, each with its synthetic subsection here, in
    /// the order of their subsections (each list is added with the
    /// subsection made for it).
    pub lists: Vec<ObjcMethList>,
}

impl ObjcMethlistSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__objc_methlist");
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
    write_lists(ctx, ChunkId::ObjcMethlist, ctx.objc_methlist.hdr.addr, buf);
}

/// Writes the lists laid out in the chunk `chunk` at `chunk_addr` - this
/// section, or another segment's __objc_methlist a symbol move took
/// some to (see output_sections::lay_out_objc_method_lists) - each at
/// its offset in `buf`, the chunk's contents.
pub fn write_lists<E: Target>(ctx: &Context<E>, chunk: ChunkId, chunk_addr: u64, buf: &mut [u8]) {
    let lists = ctx.objc_methlist.lists.iter();
    for list in lists.filter(|l| ctx.isecs[l.isec as usize].output_section() == Some(chunk)) {
        let isec = &ctx.isecs[list.isec as usize];
        let base = isec.offset as usize;
        let addr = chunk_addr + base as u64;
        let count = list.methods.len() as u32;
        buf[base..base + 4].copy_from_slice(&(12u32 | 0x8000_0000).to_le_bytes());
        buf[base + 4..base + 8].copy_from_slice(&count.to_le_bytes());
        for (i, m) in list.methods.iter().enumerate() {
            let at = base + 8 + 12 * i;
            let field = addr + 8 + 12 * i as u64;
            for (k, r) in [m.name, m.types, m.imp].into_iter().enumerate() {
                let target = r.addr(ctx);
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
