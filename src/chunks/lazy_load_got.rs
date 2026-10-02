//! __DATA,__lazy_load_got: the pointers through which the lazy-load
//! helpers reach the symbols of the dylibs dyld loads lazily.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::symbol::SymbolId;
use crate::target::Target;

/// __DATA,__lazy_load_got: a slot per lazily loaded symbol (on x86-64,
/// a second one for its call helper: see Target::LAZY_CALL_OWN_SLOT),
/// each dylib's (in load order) together, by name. Each dylib's slots
/// are a chain of DYLD_CHAINED_PTR_64 binds, which
/// LC_DYLD_CHAINED_FIXUPS does not list: the dylib's
/// LC_LAZY_LOAD_DYLIB_INFO record points dyld at the chain's first
/// slot, and dyld binds the chain when __dyld_lazy_load loads the
/// dylib.
#[derive(Debug)]
pub struct LazyLoadGotSection {
    pub hdr: ChunkHeader,
    /// Each slot's symbol, and the slot's local symbol
    /// (_foo$lazyGOT).
    pub slots: Vec<(SymbolId, &'static [u8])>,
}

impl LazyLoadGotSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__DATA", b"__lazy_load_got");
        hdr.p2align = 3;
        Self { hdr, slots: Vec::new() }
    }

    pub fn slot_addr(&self, i: u32) -> u64 {
        self.hdr.addr + i as u64 * 8
    }
}

impl Default for LazyLoadGotSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes each dylib's chain: a bind of the symbol at its index in the
/// dylib's record (ordinal:24 addend:8 reserved:19 next:12 bind:1),
/// `next` counting 4-byte strides to the next slot, 0 at the last.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for dylib in &ctx.lazy_load_info.dylibs {
        let n = dylib.syms.len() as u64;
        for i in 0..n {
            let next = if i + 1 < n { 2 } else { 0 };
            let word = (1u64 << 63) | (next << 51) | i;
            let off = (dylib.got_start as usize + i as usize) * 8;
            buf[off..off + 8].copy_from_slice(&word.to_le_bytes());
        }
    }
}
