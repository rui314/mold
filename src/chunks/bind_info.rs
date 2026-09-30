//! The LC_DYLD_INFO bind opcode stream: every slot dyld fills with an
//! import.

use crate::chunks::{ChunkHeader, segment_and_offset};
use crate::context::Context;
use crate::input_files::FileId;
use crate::macho::*;
use crate::target::{RelocClass, Target};
use crate::util::encode_uleb;

/// The bind opcode stream: every slot dyld fills with an import.
#[derive(Debug)]
pub struct BindInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl BindInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

impl Default for BindInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.bind_info.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// Builds the bind opcode stream: it tells dyld which imported symbol to
/// write into each GOT slot. Runs during layout, once every segment
/// before __LINKEDIT has an address.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let mut binds: Vec<(u64, crate::symbol::SymbolId, i64)> = Vec::new();

    // GOT slots for imported symbols.
    {
        let got_addr = ctx.got.hdr.addr;
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if ctx.symbols[id].is_imported() {
                binds.push((got_addr + i as u64 * 8, id, 0));
            }
        }
    }

    // __thread_ptrs slots for thread-locals imported from dylibs: dyld
    // writes the foreign TLV descriptor's address.
    {
        let addr = ctx.thread_ptrs.hdr.addr;
        for (i, &id) in ctx.thread_ptrs.symbols.iter().enumerate() {
            if ctx.symbols[id].is_imported() {
                binds.push((addr + i as u64 * 8, id, 0));
            }
        }
    }

    // Pointers in data sections initialized with an imported symbol's
    // address.
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || isec.replacement != crate::input_sections::NO_REPLACEMENT {
            continue;
        }
        let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            if E::classify_reloc(rel.r_type) != RelocClass::Plain
                || rel.size != 8
                || rel.is_pcrel
                || rel.is_subtracted
                || rel.r_type == E::RELOC_SUBTRACTOR
            {
                continue;
            }
            if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel)
                && ctx.symbols[id].is_imported()
                && !ctx.is_swift_force_load_ref(id)
            {
                binds.push((base + rel.offset as u64, id, rel.addend));
            }
        }
    }

    if binds.is_empty() {
        return Vec::new();
    }
    binds.sort_unstable_by_key(|&(addr, _, _)| addr);

    let mut buf = Vec::new();
    let mut last_addend = 0i64;
    for (addr, id, addend) in binds {
        let sym = &ctx.symbols[id];
        let Some(FileId::Dylib(dylib)) = sym.file() else { unreachable!() };
        let ordinal = ctx.bind_ordinal(dylib);
        // The special ordinals (main executable 0, flat lookup -2) take
        // the SPECIAL_IMM form, as ld64 emits them.
        if ordinal <= 0 {
            buf.push(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM | (ordinal & 0xf) as u8);
        } else if ordinal < 16 {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | ordinal as u8);
        } else {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
            encode_uleb(&mut buf, ordinal as u64);
        }
        let flags = if sym.is_weak_ref() { BIND_SYMBOL_FLAGS_WEAK_IMPORT } else { 0 };
        buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
        buf.extend_from_slice(sym.name().as_bytes());
        buf.push(0);
        buf.push(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER);
        // The addend is bind-machine state: it persists across
        // BIND opcodes, so emit SET_ADDEND_SLEB only on change.
        if addend != last_addend {
            buf.push(BIND_OPCODE_SET_ADDEND_SLEB);
            crate::util::encode_sleb(&mut buf, addend);
            last_addend = addend;
        }
        let (seg, off) = segment_and_offset(ctx, addr);
        buf.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
        encode_uleb(&mut buf, off);
        buf.push(BIND_OPCODE_DO_BIND);
    }

    buf.push(BIND_OPCODE_DONE);
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    buf
}
