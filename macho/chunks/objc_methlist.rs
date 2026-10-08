//! __TEXT,__objc_methlist: the Objective-C method lists rewritten in the
//! relative (12-byte entry) form, which needs no fixups.

use crate::arch::Target;
use crate::chunks::output_section::append_tail;
use crate::chunks::{ChunkHeader, ChunkId, OutputSectionId, Tail};
use crate::context::Context;
use crate::objc::ObjcMethList;
use crate::passes::{SectionName, record_section};
use crate::symbol_moves::{Move, MoveOption};
use crate::util::align_to;

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

/// Lays out __objc_methlist, the method lists rewritten in the relative
/// form (see convert_objc_method_lists), in the order they were made,
/// each 8-byte aligned (the class records point at them); category
/// merging also retires some after their first placement. The lists
/// -move_to_ro_segment takes to another segment (see symbol_moves) go
/// alike to an __objc_methlist there.
pub fn lay_out_objc_method_lists<E: Target>(
    ctx: &mut Context<E>,
    text: SectionName,
    moves: &hashbrown::HashMap<u32, Move>,
) {
    if ctx.objc_methlist.lists.is_empty() {
        return;
    }
    let order: Vec<u32> = ctx.objc_methlist.lists.iter().map(|l| l.isec).collect();

    // The lists of each section, by the section: None for
    // __TEXT,__objc_methlist. (Of the symbol moves, only
    // -move_to_ro_segment's takes code.)
    let mut groups: Vec<(Option<OutputSectionId>, Vec<u32>)> = Vec::new();
    for isec in order {
        let sec = &ctx.isecs[isec as usize];
        let hdr = *sec.hdr(&ctx.objs[sec.file as usize]);
        let m = moves.get(&isec).filter(|m| m.option == MoveOption::Ro);
        let dest = m.and_then(|&m| record_section(ctx, &hdr, text, Some(m)));
        match groups.iter_mut().find(|(d, _)| *d == dest) {
            Some((_, lists)) => lists.push(isec),
            None => groups.push((dest, vec![isec])),
        }
    }
    for (dest, lists) in groups {
        let mut off = 0u64;
        for &isec in &lists {
            off = align_to(off, 8);
            ctx.isecs[isec as usize].offset = off as u32;
            off += ctx.isecs[isec as usize].size as u64;
        }
        let (chunk, base) = match dest {
            None => {
                ctx.objc_methlist.hdr.size = off;
                ctx.chunks.push(ChunkId::ObjcMethlist);
                (ChunkId::ObjcMethlist, 0)
            }
            Some(id) => {
                let osec = ctx.output_section_mut(id);
                append_tail(osec, 3, Tail::ObjcMethlists, off);
                (ChunkId::Output(id), osec.tail_off)
            }
        };
        for isec in lists {
            let isec = &mut ctx.isecs[isec as usize];
            isec.offset += base as u32;
            isec.set_output_section(chunk);
        }
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    write_lists(ctx, ChunkId::ObjcMethlist, ctx.objc_methlist.hdr.addr, buf);
}

/// Writes the lists laid out in the chunk `chunk` at `chunk_addr` - this
/// section, or another segment's __objc_methlist a symbol move took
/// some to (see lay_out_objc_method_lists) - each at
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
