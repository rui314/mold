//! The LC_DYLD_INFO weak-bind opcode stream: the slots dyld redirects when
//! another image's copy of one of this image's weak definitions wins
//! coalescing.

use crate::chunks::{ChunkHeader, segment_and_offset};
use crate::context::Context;
use crate::macho::*;
use crate::target::{RelocClass, Target};
use crate::util::encode_uleb;

/// The weak-bind opcode stream: the slots dyld redirects when another
/// image's copy of one of this image's weak definitions wins
/// coalescing.
#[derive(Debug)]
pub struct WeakBindInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl WeakBindInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

impl Default for WeakBindInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.weak_bind_info.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// Builds the classic weak_bind stream: for every slot that holds the
/// address of one of this image's coalescable weak definitions - GOT
/// entries and data pointers - a bind by name, which dyld applies
/// only if another image's copy of the symbol won coalescing (the
/// slot's rebase already holds this image's copy). Sorted by symbol
/// name, then address, as ld64 writes them.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let mut binds: Vec<(crate::symbol::SymbolId, u64)> = Vec::new();
    {
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if ctx.binds_weak_lookup(id) {
                binds.push((id, ctx.got.slot_addr(i)));
            }
        }
    }
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
                && ctx.binds_weak_lookup(id)
            {
                binds.push((id, base + rel.offset as u64));
            }
        }
    }
    // Strong definitions overriding a dylib's weak export are listed
    // first, by name, flagged non-weak, with no location: dyld then
    // knows this image's copy wins coalescing.
    let mut overrides: Vec<crate::symbol::SymbolId> =
        (0..ctx.symbols.syms.len() as u32).filter(|&i| ctx.overrides_weak_export(i)).collect();
    if binds.is_empty() && overrides.is_empty() {
        return Vec::new();
    }
    overrides.sort_by(|&a, &b| ctx.symbols[a].name().cmp(ctx.symbols[b].name()));
    binds.sort_by(|a, b| ctx.symbols[a.0].name().cmp(ctx.symbols[b.0].name()).then(a.1.cmp(&b.1)));

    let mut buf = Vec::new();
    for id in overrides {
        buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | BIND_SYMBOL_FLAGS_NON_WEAK_DEFINITION);
        buf.extend_from_slice(ctx.symbols[id].name().as_bytes());
        buf.push(0);
    }
    let mut last: Option<crate::symbol::SymbolId> = None;
    for (id, addr) in binds {
        if last != Some(id) {
            buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM);
            buf.extend_from_slice(ctx.symbols[id].name().as_bytes());
            buf.push(0);
            buf.push(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER);
            last = Some(id);
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
