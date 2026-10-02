//! The LC_DYLD_INFO lazy-bind opcode stream: one record per lazy pointer,
//! entered by its stub helper on first call.

use crate::chunks::{ChunkHeader, segment_and_offset};
use crate::context::Context;
use crate::macho::*;
use crate::target::Target;
use crate::util::encode_uleb;

/// The lazy-bind opcode stream: one record per lazy pointer, entered
/// by its stub helper on first call.
#[derive(Debug)]
pub struct LazyBindInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout, and each stub's record offset
    /// in it (what its stub helper entry pushes for dyld_stub_binder).
    pub contents: Vec<u8>,
    pub offsets: Vec<u32>,
}

impl LazyBindInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new(), offsets: Vec::new() }
    }
}

impl Default for LazyBindInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.lazy_bind_info.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// The lazy-bind opcode stream: one self-contained record per lazily
/// bound stub (segment/offset of its lazy pointer, dylib ordinal,
/// symbol, bind, done), and each record's offset, which the stub helper
/// entry pushes for dyld_stub_binder. ld64's layout, byte for byte.
pub fn build<E: Target>(ctx: &Context<E>) -> (Vec<u8>, Vec<u32>) {
    if ctx.stubs.lazy.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut buf = Vec::new();
    let mut offsets = Vec::with_capacity(ctx.stubs.lazy.len());
    for &i in &ctx.stubs.lazy {
        let id = ctx.stubs.symbols[i as usize];
        offsets.push(buf.len() as u32);
        let addr = ctx.stub_ptr_addr(i as usize, id);
        let (seg, off) = segment_and_offset(ctx, addr);
        buf.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
        encode_uleb(&mut buf, off);
        let sym = &ctx.symbols[id];
        let ordinal = ctx.sym_bind_ordinal(id);
        if ordinal <= 0 {
            buf.push(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM | (ordinal & 0xf) as u8);
        } else if ordinal < 16 {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | ordinal as u8);
        } else {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
            encode_uleb(&mut buf, ordinal as u64);
        }
        let flags = if sym.is_weak_ref() { BIND_SYMBOL_FLAGS_WEAK_IMPORT } else { 0 };
        buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
        buf.extend_from_slice(sym.name());
        buf.push(0);
        buf.push(BIND_OPCODE_DO_BIND);
        buf.push(BIND_OPCODE_DONE);
    }
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    (buf, offsets)
}
