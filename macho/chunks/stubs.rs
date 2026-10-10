//! This file creates __TEXT,__stubs, the Mach-O counterpart of the PLT. A
//! call to a function in another image goes to the function's stub, a few
//! instructions that jump through a pointer: the function's lazy pointer
//! when functions are bound lazily (see lazy_ptrs.rs and stub_helper.rs),
//! or otherwise its GOT slot, which dyld fills in at load time (see
//! got.rs). Unlike the PLT, __stubs has no header entry; the code that
//! enters dyld for lazy binding is in __stub_helper.

use crate::arch::Target;
use crate::chunks::split_info::{Entry, Places};
use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;

/// __TEXT,__stubs: jump stubs for calls to imported functions.
#[derive(Debug)]
pub struct StubsSection {
    pub hdr: ChunkHeader,
    /// Symbols with a __stubs entry, in stub order.
    pub symbols: Vec<SymbolId>,
    /// The stubs bound lazily, by index into `symbols`, in stub order:
    /// only these have a lazy pointer and a stub helper entry (a
    /// weak-lookup stub jumps through its GOT slot).
    pub lazy: Vec<u32>,
}

impl StubsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__stubs");
        hdr.flags = S_SYMBOL_STUBS | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 2;
        Self { hdr, symbols: Vec::new(), lazy: Vec::new() }
    }
}

impl Default for StubsSection {
    fn default() -> Self {
        Self::new()
    }
}

/// The offset of stub `idx` in __stubs.
pub fn entry_offset<E: Target>(idx: u32) -> u64 {
    idx as u64 * E::STUB_SIZE
}

/// Gives a symbol a stub, unless it has one, and the stub what it jumps
/// through: the symbol's lazy pointer, which the stub helper fills in,
/// or, without lazy binding, its GOT slot. A symbol dyld resolves by
/// weak lookup goes through its GOT slot either way, never a lazy
/// pointer, as ld64 has it.
pub fn add_symbol<E: Target>(ctx: &mut Context<E>, id: SymbolId) {
    if ctx.symbols[id].stub_idx(&ctx.symbols).is_some() {
        return;
    }
    ctx.symbols.aux_mut(id).stub_idx = ctx.stubs.symbols.len() as u32;
    ctx.stubs.symbols.push(id);
    if ctx.args.lazy_binding && !ctx.symbols[id].binds_weak_lookup(ctx) {
        crate::chunks::stub_helper::ensure_stub_binder(ctx);
    } else {
        crate::chunks::got::add_got_symbol(ctx, id);
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.text_exec {
        ctx.stubs.hdr.segname = b"__TEXT_EXEC";
    }
    ctx.stubs.hdr.reserved2 = E::STUB_SIZE as u32;
    ctx.stubs.hdr.size = ctx.stubs.symbols.len() as u64 * E::STUB_SIZE;
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_stubs(ctx, ctx.stubs.hdr.addr, buf);
}

/// The stubs' references to the pointers they jump through, for
/// LC_SEGMENT_SPLIT_INFO: lazy pointers, or GOT slots.
pub(crate) fn split_info_entries<E: Target>(p: &Places<'_, E>, out: &mut Vec<Entry>) {
    let ctx = p.ctx;
    if !ctx.chunks.contains(&ChunkId::Stubs) {
        return;
    }
    for (i, &id) in ctx.stubs.symbols.iter().enumerate() {
        let slot = if ctx.args.lazy_binding && !ctx.symbols[id].binds_weak_lookup(ctx) {
            let lazy = ctx.stubs.lazy.binary_search(&(i as u32)).unwrap();
            p.chunk_addr(ChunkId::LazyPtrs, ctx.lazy_ptrs.slot_addr(lazy))
        } else {
            p.got_slot(id)
        };
        let off = entry_offset::<E>(i as u32) + E::STUB_REF_OFF;
        p.pcrel(out, p.chunk(ChunkId::Stubs, off), Some(slot));
    }
}
