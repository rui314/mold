//! The LC_DYLD_INFO weak-bind opcode stream: the slots dyld redirects when
//! another image's copy of one of this image's weak definitions wins
//! coalescing.

use crate::chunks::{ChunkHeader, bind_info};
use crate::context::Context;
use crate::macho::*;
use crate::target::{RelocClass, Target};

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
/// name, then address, and encoded as the bind stream is (see
/// bind_info::bind_ops), each piece of state set only when it changes,
/// as ld64 writes them.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    // A -static image calls its weak definitions directly, but under
    // -no_fixup_chains ld-prime still lists the slots holding their
    // addresses, as the chains do.
    let binds_weak = |id| ctx.binds_weak_lookup(id) || ctx.is_weak_coalesced(id);

    let mut binds: Vec<(crate::symbol::SymbolId, u64)> = Vec::new();
    {
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if binds_weak(id) {
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
                && binds_weak(id)
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
        buf.extend_from_slice(ctx.symbols[id].name());
        buf.push(0);
    }
    let binds: Vec<_> = binds.into_iter().map(|(id, addr)| (addr, id, 0)).collect();
    let ops = bind_info::bind_ops(ctx, &binds, |_| None, |_| 0);
    bind_info::encode(bind_info::compress(ops), buf)
}
