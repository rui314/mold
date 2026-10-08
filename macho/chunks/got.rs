//! The global offset table: pointers to symbols, bound by dyld for imported
//! ones. mold's got.rs holds the ELF counterpart.

use crate::arch::Target;
use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::input_files::add_synthetic_section;
use crate::input_sections::InputSection;
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

/// Replaces the class reference slots that stay (see
/// objc::fold_objc_classrefs), each with its class, by stand-ins for
/// the classes' GOT entries: subsections of a synthetic __got section,
/// placed at the entries once passes::scan_relocations has made them
/// (see update_shdr), so that what refers to a slot reads its class's
/// entry. A stand-in is not alive: the GOT chunk writes the entry, and
/// the slot's local symbol is not emitted.
pub(crate) fn add_classref_stand_ins<E: Target>(ctx: &mut Context<E>, kept: Vec<(u32, SymbolId)>) {
    if kept.is_empty() {
        return;
    }
    let hdr = MachSection {
        sectname: bytes_to_name(b"__got"),
        segname: bytes_to_name(b"__DATA"),
        p2align: 3,
        flags: S_NON_LAZY_SYMBOL_POINTERS,
        ..Default::default()
    };
    let (file, shndx) = add_synthetic_section(ctx, hdr);
    for (slot, class) in kept {
        ctx.isecs.push(InputSection {
            output_section: ChunkId::Got.pack(),
            flags: InputSection::flags_dead(),
            ..InputSection::new(file, shndx, 3, 8, &[])
        });
        let stand_in = (ctx.isecs.len() - 1) as u32;
        ctx.isecs[slot as usize].replacement = stand_in;
        ctx.got.stand_ins.push((stand_in, class));
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let seg = crate::chunks::data_seg(ctx);
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
        false => ctx.symbols[id].binds_as_import(ctx),
    };
    for (i, &id) in ctx.got.got_syms.iter().enumerate() {
        if !binds(id) {
            buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.symbols[id].addr(ctx).to_le_bytes());
        }
    }
}
