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
    /// input pointers - __objc_classrefs entries (see
    /// fold_objc_classrefs) and an input __got's slots (see
    /// fold_input_got) - with their symbols; they are placed at the
    /// symbols' slots once the section exists.
    pub stand_ins: Vec<(u32, SymbolId)>,
}

impl GotSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__DATA", "__got");
        hdr.flags = S_NON_LAZY_SYMBOL_POINTERS;
        hdr.p2align = 3;
        let mut weak_hdr = ChunkHeader::new("__DATA", "__weak_got");
        weak_hdr.flags = S_NON_LAZY_SYMBOL_POINTERS;
        weak_hdr.p2align = 3;
        Self { hdr, got_syms: Vec::new(), weak_hdr, weak_start: 0, stand_ins: Vec::new() }
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
    // the bind stream.
    for (i, &id) in syms.iter().enumerate() {
        if !ctx.symbols[id].is_imported() {
            buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.sym_addr(id).to_le_bytes());
        }
    }
}
