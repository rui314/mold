//! The LC_DYLD_INFO lazy-bind opcode stream: one record per lazy pointer,
//! entered by its stub helper on first call.

use crate::arch::Target;
use crate::chunks::bind_info::{self, Op};
use crate::chunks::{ChunkHeader, segment_and_offset};
use crate::context::Context;
use crate::macho::*;

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
        let sym = &ctx.symbols[ctx.stubs.symbols[i as usize]];
        offsets.push(buf.len() as u32);
        let (seg, off) = segment_and_offset(ctx, sym.stub_ptr_addr(ctx, i as usize));
        let flags = if sym.is_weak_ref() { BIND_SYMBOL_FLAGS_WEAK_IMPORT } else { 0 };
        let ops = [
            Op::SegOffset(seg, off),
            Op::Dylib(sym.bind_ordinal(ctx)),
            Op::Symbol(sym.name(), flags),
            Op::Bind,
        ];
        for op in ops {
            bind_info::encode_op(&mut buf, op);
        }
        buf.push(BIND_OPCODE_DONE);
    }
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    (buf, offsets)
}
