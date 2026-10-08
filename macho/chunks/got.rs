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
    /// The subsections standing for the GOT entries of the classes whose
    /// __objc_classrefs slots stay when the slots fold into __got (see
    /// objc::fold_objc_classrefs), with the classes.
    pub stand_ins: Vec<(u32, SymbolId)>,
}

impl GotSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__DATA", b"__got");
        hdr.flags = S_NON_LAZY_SYMBOL_POINTERS;
        hdr.p2align = 3;
        Self { hdr, got_syms: Vec::new(), stand_ins: Vec::new() }
    }

    pub fn slot_addr(&self, i: usize) -> u64 {
        self.hdr.addr + i as u64 * 8
    }
}

impl Default for GotSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    // Slots for imported symbols stay zero; dyld fills them via
    // the bind stream. Legacy LINKEDIT's slot of an interposable
    // export, which dyld binds by name too, starts out with its
    // address all the same, as ld-prime writes it.
    let binds = |id: crate::symbol::SymbolId| match ctx.args.legacy_linkedit {
        true => ctx.symbols[id].is_imported(),
        false => ctx.binds_as_import(id),
    };
    for (i, &id) in ctx.got.got_syms.iter().enumerate() {
        if !binds(id) {
            buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.sym_addr(id).to_le_bytes());
        }
    }
}
