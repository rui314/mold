//! __DATA,__la_symbol_ptr: the lazy pointers the stubs jump through, bound
//! by dyld on first call.

use crate::arch::Target;
use crate::chunks::split_info::{Entry, Places, push};
use crate::chunks::{ChunkHeader, ChunkId, stub_helper};
use crate::context::Context;
use crate::macho::*;

/// __DATA,__la_symbol_ptr: the lazy pointers the stubs jump through,
/// bound by dyld on first call.
#[derive(Debug)]
pub struct LazyPtrsSection {
    pub hdr: ChunkHeader,
}

impl LazyPtrsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__DATA", b"__la_symbol_ptr");
        hdr.flags = S_LAZY_SYMBOL_POINTERS;
        hdr.p2align = 3;
        Self { hdr }
    }

    pub fn slot_addr(&self, i: usize) -> u64 {
        self.hdr.addr + i as u64 * 8
    }
}

impl Default for LazyPtrsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    // In the shared region, dyld binds them all at load, and the
    // section joins the read-only data.
    if ctx.args.shared_region {
        ctx.lazy_ptrs.hdr.segname = crate::chunks::data_seg(ctx);
    }
    ctx.lazy_ptrs.hdr.size = ctx.stubs.lazy.len() as u64 * 8;
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    // Each lazy pointer starts at its stub helper entry.
    let helper = ctx.stub_helper.hdr.addr;
    for i in 0..ctx.stubs.lazy.len() {
        let val = helper + crate::chunks::stub_helper::entry_offset(ctx, i as u32);
        buf[i * 8..i * 8 + 8].copy_from_slice(&val.to_le_bytes());
    }
}

/// The lazy pointers, for the loader to slide (see
/// rebase_info::rebase_locations): they start out pointing at their
/// stub helper entries. (A weak-lookup stub's GOT slot is rebased with
/// the GOT.)
pub fn rebase_locations<E: Target>(ctx: &Context<E>, locs: &mut Vec<u64>) {
    for &i in &ctx.stubs.lazy {
        let i = i as usize;
        locs.push(ctx.symbols[ctx.stubs.symbols[i]].stub_ptr_addr(ctx, i));
    }
}

/// The lazy pointers' references to their stub helper entries, for
/// LC_SEGMENT_SPLIT_INFO.
pub(crate) fn split_info_entries<E: Target>(p: &Places<'_, E>, out: &mut Vec<Entry>) {
    let ctx = p.ctx;
    if !ctx.chunks.contains(&ChunkId::LazyPtrs) {
        return;
    }
    for i in 0..ctx.stubs.lazy.len() {
        let to = p.chunk(ChunkId::StubHelper, stub_helper::entry_offset(ctx, i as u32));
        push(
            out,
            p.chunk_addr(ChunkId::LazyPtrs, ctx.lazy_ptrs.slot_addr(i)),
            DYLD_CACHE_ADJ_V2_POINTER_64,
            Some(to),
        );
    }
}
