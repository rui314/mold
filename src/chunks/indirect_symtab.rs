//! The indirect symbol table: for each __stubs, __got, __weak_got and
//! __la_symbol_ptr slot, and each slot of the inputs' other non-lazy
//! symbol pointer sections, the output symbol it holds.

use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::input_sections::RelocTarget;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// The indirect symbol table: for each __stubs, __got, __weak_got and
/// __la_symbol_ptr slot, and each slot of the inputs' other non-lazy
/// symbol pointer sections, the output symbol it holds.
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
/// where both are in __DATA (-no_data_const) - and the output sections
/// of non-lazy symbol pointers (see output_section_flags) where they
/// are.
pub fn sections<E: Target>(ctx: &Context<E>) -> impl Iterator<Item = ChunkId> + '_ {
    ctx.chunks.iter().copied().filter(|&id| match id {
        ChunkId::Stubs | ChunkId::LazyPtrs | ChunkId::WeakGot | ChunkId::Got => true,
        ChunkId::Output(osec) => {
            !ctx.args.relocatable
                && ctx.output_section(osec).hdr.flags & SECTION_TYPE == S_NON_LAZY_SYMBOL_POINTERS
        }
        _ => false,
    })
}

/// A section's slots: the symbol each holds, or None for
/// INDIRECT_SYMBOL_LOCAL (a GOT slot filled in here).
fn entries<E: Target>(ctx: &Context<E>, id: ChunkId) -> Vec<Option<SymbolId>> {
    let got = &ctx.got;
    let got_slots = |syms: &[SymbolId]| -> Vec<Option<SymbolId>> {
        syms.iter().map(|&id| Some(id).filter(|&id| ctx.binds_at_runtime(id))).collect()
    };
    match id {
        ChunkId::Stubs => ctx.stubs.symbols.iter().map(|&id| Some(id)).collect(),
        ChunkId::LazyPtrs => {
            ctx.stubs.lazy.iter().map(|&i| Some(ctx.stubs.symbols[i as usize])).collect()
        }
        ChunkId::Got => {
            let mut vec = got_slots(&got.got_syms[..got.weak_start]);
            vec.extend(got.input_slots.iter().flat_map(|&isec| input_slots(ctx, isec as usize)));
            vec
        }
        ChunkId::WeakGot => got_slots(&got.got_syms[got.weak_start..]),
        ChunkId::Output(osec) => {
            let members = &ctx.output_section(osec).members;
            members.iter().flat_map(|&isec| input_slots(ctx, isec as usize)).collect()
        }
        _ => unreachable!(),
    }
}

/// The slots of input subsection `isec` of non-lazy symbol pointers:
/// the symbol each relocation points one at, if dyld binds it - a
/// pointer to anything else is one filled in here.
fn input_slots<E: Target>(ctx: &Context<E>, isec: usize) -> Vec<Option<SymbolId>> {
    let obj = ctx.isecs[isec].file as usize;
    let rels = ctx.isec_relocs(isec);
    (0..ctx.isecs[isec].size)
        .step_by(8)
        .map(|off| {
            let rel =
                rels.iter().find(|rel| rel.offset == off && rel.r_type != E::RELOC_SUBTRACTOR)?;
            let RelocTarget::Sym(idx) = rel.target() else { return None };
            Some(ctx.objs[obj].symbols[idx as usize]).filter(|&id| ctx.binds_at_runtime(id))
        })
        .collect()
}

/// Gives each covered section its first entry's index (reserved1),
/// once the chunks are in order, and sizes the table.
pub fn assign_indices<E: Target>(ctx: &mut Context<E>) {
    let ids: Vec<ChunkId> = sections(ctx).collect();
    let mut n = 0;
    for id in ids {
        let len = entries(ctx, id).len() as u32;
        ctx.chunk_header_mut(id).reserved1 = n;
        n += len;
    }
    ctx.indirect_symtab.hdr.size = n as u64 * 4;
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut off = 0;
    for entry in sections(ctx).flat_map(|id| entries(ctx, id)) {
        let val = match entry.map(|id| ctx.symtab.output_sym_indices[id as usize]) {
            None | Some(u32::MAX) => INDIRECT_SYMBOL_LOCAL,
            Some(idx) => idx,
        };
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
        off += 4;
    }
}
