//! The __lazy_load_got section of the __DATA segment holds the pointers
//! through which the image reaches the symbols of its lazily loaded dylibs
//! (see crate::lazy_load): a slot per symbol, like a GOT entry. Unlike the
//! GOT, dyld doesn't fill these slots at startup. It fills a dylib's slots
//! when __dyld_lazy_load loads the dylib, finding them through the dylib's
//! record (see lazy_load_info.rs).

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::chunks::symtab::{NamedEntry, local_msym};
use crate::context::Context;
use crate::symbol::SymbolId;

/// __DATA,__lazy_load_got: a slot per lazily loaded symbol (on x86-64,
/// a second one for its call helper: see Target::LAZY_CALL_OWN_SLOT),
/// each dylib's together, in the order of the image's first uses (see
/// lazy_load::create_lazy_load_slots). Each dylib's slots
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

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    // Read-only data in the shared region, as its lazy pointers are.
    if ctx.args.shared_region {
        ctx.lazy_load_got.hdr.segname = crate::chunks::data_seg(ctx);
    }
    ctx.lazy_load_got.hdr.size = ctx.lazy_load_got.slots.len() as u64 * 8;
}

/// The slots' local symbols.
pub fn populate_symtab<E: Target>(ctx: &Context<E>, out: &mut Vec<NamedEntry>) {
    let hdr = &ctx.lazy_load_got.hdr;
    for (i, &(_, name)) in ctx.lazy_load_got.slots.iter().enumerate() {
        let addr = ctx.lazy_load_got.slot_addr(i as u32);
        out.push((name, local_msym(hdr.sect_idx, addr), None));
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
