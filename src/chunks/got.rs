//! The global offset table: pointers to symbols, bound by dyld for imported
//! ones. mold's got.rs holds the ELF counterpart.

use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// The global offset table: pointers to symbols, bound by dyld for
/// imported ones.
#[derive(Debug)]
pub struct GotSection {
    pub hdr: ChunkHeader,
    /// Symbols with a __got slot, in slot order.
    pub got_syms: Vec<SymbolId>,
    /// __weak_got: in an image bound for the shared region, ld-prime
    /// gives the slots of the symbols dyld binds by weak lookup (last in
    /// the order) a section of their own. `weak_start` is the first
    /// such slot, or the slot count when there is none.
    pub weak_hdr: ChunkHeader,
    pub weak_start: usize,
    /// Synthetic subsections standing for __got slots that absorbed
    /// an input __got's slots (see fold_input_got), with their symbols;
    /// they are placed at the symbols' slots once the section exists.
    pub stand_ins: Vec<(u32, SymbolId)>,
    /// The slots of the inputs' __got that are no plain pointer to a
    /// symbol (see fold_input_got), after the symbols' slots in __got:
    /// each keeps its bytes and relocation, as data does.
    pub input_slots: Vec<u32>,
}

impl GotSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__DATA", b"__got");
        hdr.flags = S_NON_LAZY_SYMBOL_POINTERS;
        hdr.p2align = 3;
        let mut weak_hdr = ChunkHeader::new(b"__DATA", b"__weak_got");
        weak_hdr.flags = S_NON_LAZY_SYMBOL_POINTERS;
        weak_hdr.p2align = 3;
        Self {
            hdr,
            got_syms: Vec::new(),
            weak_hdr,
            weak_start: 0,
            stand_ins: Vec::new(),
            input_slots: Vec::new(),
        }
    }

    /// The section slot `i` lies in, and its offset there.
    pub fn slot_place(&self, i: usize) -> (ChunkId, u64) {
        match i.checked_sub(self.weak_start) {
            Some(j) => (ChunkId::WeakGot, j as u64 * 8),
            None => (ChunkId::Got, i as u64 * 8),
        }
    }

    pub fn slot_addr(&self, i: usize) -> u64 {
        let base = if i < self.weak_start { self.hdr.addr } else { self.weak_hdr.addr };
        base + self.slot_place(i).1
    }
}

impl Default for GotSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes __got's slots, or __weak_got's.
pub fn copy_buf<E: Target>(ctx: &Context<E>, weak: bool, buf: &mut [u8]) {
    let got = &ctx.got;
    let syms = if weak { &got.got_syms[got.weak_start..] } else { &got.got_syms[..got.weak_start] };
    // Slots for imported symbols stay zero; dyld fills them via
    // the bind stream. Legacy LINKEDIT's slot of an interposable
    // export, which dyld binds by name too, starts out with its
    // address all the same, as ld-prime writes it.
    let binds = |id: crate::symbol::SymbolId| match ctx.args.legacy_linkedit {
        true => ctx.symbols[id].is_imported(),
        false => ctx.binds_as_import(id),
    };
    for (i, &id) in syms.iter().enumerate() {
        if !binds(id) {
            buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.sym_addr(id).to_le_bytes());
        }
    }
    if weak {
        return;
    }
    for &id in &got.input_slots {
        let isec = &ctx.isecs[id as usize];
        let off = isec.offset as usize;
        let slice = &mut buf[off..off + isec.size as usize];
        slice.copy_from_slice(isec.data());
        let rels = ctx.isec_relocs(id as usize);
        E::apply_relocs(ctx, rels, id as usize, got.hdr.addr + off as u64, slice);
    }
}
