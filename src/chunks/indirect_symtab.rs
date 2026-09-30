//! The indirect symbol table: for each __stubs, __got and __la_symbol_ptr
//! slot, the output symbol it holds.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::target::Target;

/// The indirect symbol table: for each __stubs, __got and
/// __la_symbol_ptr slot, the output symbol it holds.
#[derive(Debug)]
pub struct IndirectSymtabSection {
    pub hdr: ChunkHeader,
}

impl IndirectSymtabSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit() }
    }
}

impl Default for IndirectSymtabSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut off = 0;
    let lazy = ctx.stubs.lazy.iter().map(|&i| ctx.stubs.symbols[i as usize]);
    // A GOT slot holding a definition of this image that dyld never
    // rebinds is INDIRECT_SYMBOL_LOCAL, as ld64 writes it, whatever
    // the symbol's scope; a stub's or an imported (or
    // weak-coalesced) symbol's slot names the symbol.
    let entries = ctx
        .stubs
        .symbols
        .iter()
        .map(|&id| (id, false))
        .chain(ctx.got.got_syms.iter().map(|&id| (id, !ctx.binds_at_runtime(id))))
        .chain(lazy.map(|id| (id, false)));
    for (id, local) in entries {
        let val = match ctx.symtab.output_sym_indices[id as usize] {
            _ if local => INDIRECT_SYMBOL_LOCAL,
            u32::MAX => INDIRECT_SYMBOL_LOCAL,
            idx => idx,
        };
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
        off += 4;
    }
}
