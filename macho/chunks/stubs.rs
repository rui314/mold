//! __TEXT,__stubs: jump stubs for calls to imported functions.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::{NO_IDX, SymbolId};

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
    if ctx.sym_aux(id).stub_idx != NO_IDX {
        return;
    }
    ctx.sym_aux_mut(id).stub_idx = ctx.stubs.symbols.len() as u32;
    ctx.stubs.symbols.push(id);
    if ctx.args.lazy_binding && !ctx.binds_weak_lookup(id) {
        crate::chunks::stub_helper::ensure_stub_binder(ctx);
    } else {
        crate::chunks::got::add_got_symbol(ctx, id);
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_stubs(ctx, ctx.stubs.hdr.addr, buf);
}
