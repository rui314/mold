//! The global offset table: pointers to symbols, bound by dyld for imported
//! ones. mold's got.rs holds the ELF counterpart.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;

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

/// Gives a symbol a __got slot, unless it has one.
pub fn add_got_symbol<E: Target>(ctx: &mut Context<E>, id: SymbolId) {
    if !ctx.symbols[id].has_got(&ctx.symbols) {
        ctx.symbols.aux_mut(id).got_idx = ctx.got.got_syms.len() as u32;
        ctx.got.got_syms.push(id);
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let seg = crate::output_sections::data_seg(ctx);
    // A kext's are plain data to ld-prime (indexed into the indirect
    // symbol table all the same), and so are a -static image's, but for
    // a PIE's - one that has an indirect symbol table.
    let plain = ctx.args.is_kext() || (ctx.args.static_link && !ctx.args.pie);
    let flags = if plain { S_REGULAR } else { S_NON_LAZY_SYMBOL_POINTERS };
    let hdr = &mut ctx.got.hdr;
    hdr.size = ctx.got.got_syms.len() as u64 * 8;
    hdr.segname = seg;
    hdr.flags = flags;
    // The stand-ins for the __objc_classrefs slots that stay go at their
    // classes' entries (see objc::fold_objc_classrefs).
    for i in 0..ctx.got.stand_ins.len() {
        let (stand_in, class) = ctx.got.stand_ins[i];
        ctx.isecs[stand_in as usize].offset = ctx.symbols[class].got_idx(&ctx.symbols).unwrap() * 8;
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
