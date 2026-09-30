//! The global offset table: pointers to symbols, bound by dyld for imported
//! ones. mold's got.rs holds the ELF counterpart.

use crate::chunks::ChunkHeader;
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
    /// Synthetic subsections standing for __got slots that absorbed
    /// __objc_classrefs entries (see fold_objc_classrefs), with their
    /// classes; they are placed at the classes' slots once the section
    /// exists.
    pub objc_classref_slots: Vec<(u32, SymbolId)>,
}

impl GotSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__DATA", "__got");
        hdr.flags = S_NON_LAZY_SYMBOL_POINTERS;
        hdr.p2align = 3;
        Self { hdr, got_syms: Vec::new(), objc_classref_slots: Vec::new() }
    }
}

impl Default for GotSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    // Slots for imported symbols stay zero; dyld fills them via
    // the bind stream.
    for (i, &id) in ctx.got.got_syms.iter().enumerate() {
        if !ctx.symbols[id].is_imported() {
            buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.sym_addr(id).to_le_bytes());
        }
    }
}
