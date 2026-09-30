//! The indirect symbol table: for each __stubs, __got, __weak_got and
//! __la_symbol_ptr slot, the output symbol it holds.

use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// The indirect symbol table: for each __stubs, __got, __weak_got and
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

/// The sections the table covers, in the image's order, as ld-prime
/// lists them: __stubs, then __got before __la_symbol_ptr, or after it
/// where both are in __DATA (-no_data_const).
fn sections<E: Target>(ctx: &Context<E>) -> impl Iterator<Item = ChunkId> + '_ {
    ctx.chunks.iter().copied().filter(|id| {
        matches!(id, ChunkId::Stubs | ChunkId::LazyPtrs | ChunkId::WeakGot | ChunkId::Got)
    })
}

/// A section's slots: the symbol each holds, and whether the entry is
/// INDIRECT_SYMBOL_LOCAL (a GOT slot filled in here).
fn entries<E: Target>(ctx: &Context<E>, id: ChunkId) -> Vec<(SymbolId, bool)> {
    let got = &ctx.got;
    let got_slots = |syms: &[SymbolId]| -> Vec<(SymbolId, bool)> {
        syms.iter().map(|&id| (id, !ctx.binds_at_runtime(id))).collect()
    };
    match id {
        ChunkId::Stubs => ctx.stubs.symbols.iter().map(|&id| (id, false)).collect(),
        ChunkId::LazyPtrs => {
            ctx.stubs.lazy.iter().map(|&i| (ctx.stubs.symbols[i as usize], false)).collect()
        }
        ChunkId::Got => got_slots(&got.got_syms[..got.weak_start]),
        _ => got_slots(&got.got_syms[got.weak_start..]),
    }
}

/// Gives each covered section its first entry's index (reserved1),
/// once the chunks are in order.
pub fn assign_indices<E: Target>(ctx: &mut Context<E>) {
    let ids: Vec<ChunkId> = sections(ctx).collect();
    let mut n = 0;
    for id in ids {
        let len = entries(ctx, id).len() as u32;
        ctx.chunk_header_mut(id).reserved1 = n;
        n += len;
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut off = 0;
    for (id, local) in sections(ctx).flat_map(|id| entries(ctx, id)) {
        let val = match ctx.symtab.output_sym_indices[id as usize] {
            _ if local => INDIRECT_SYMBOL_LOCAL,
            u32::MAX => INDIRECT_SYMBOL_LOCAL,
            idx => idx,
        };
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
        off += 4;
    }
}
