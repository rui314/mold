//! __DATA,__la_symbol_ptr: the lazy pointers the stubs jump through, bound
//! by dyld on first call.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::target::Target;

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
}

impl Default for LazyPtrsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    // Each lazy pointer starts at its stub helper entry.
    let helper = ctx.stub_helper.hdr.addr + ctx.stub_helper_header_size();
    for i in 0..ctx.stubs.lazy.len() {
        let val = helper + i as u64 * E::STUB_HELPER_ENTRY_SIZE;
        buf[i * 8..i * 8 + 8].copy_from_slice(&val.to_le_bytes());
    }
}
