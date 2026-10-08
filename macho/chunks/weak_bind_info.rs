//! The LC_DYLD_INFO weak-bind opcode stream: the slots dyld redirects when
//! another image's copy of one of this image's weak definitions wins
//! coalescing.

use crate::arch::Target;
use crate::chunks::bind_info::{self, Op};
use crate::chunks::{ChunkHeader, rebase_info};
use crate::context::Context;
use crate::macho::*;

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
    let binds_weak = |id: crate::symbol::SymbolId| {
        let sym = &ctx.symbols[id];
        sym.binds_weak_lookup(ctx) || sym.is_weak_coalesced(ctx)
    };

    let mut binds: Vec<(crate::symbol::SymbolId, u64)> = Vec::new();
    for (i, &id) in ctx.got.got_syms.iter().enumerate() {
        if binds_weak(id) {
            binds.push((id, ctx.got.slot_addr(i)));
        }
    }
    for isec in ctx.isecs.iter() {
        if !isec.is_emitted() {
            continue;
        }
        let file = &ctx.objs[isec.file as usize];
        for (addr, rel) in rebase_info::pointer_relocs(ctx, isec) {
            if let Some(id) = rel.sym(file)
                && binds_weak(id)
            {
                binds.push((id, addr));
            }
        }
    }
    // Strong definitions overriding a dylib's weak export are listed
    // first, by name, flagged non-weak, with no location: dyld then
    // knows this image's copy wins coalescing.
    let mut overrides: Vec<crate::symbol::SymbolId> = (0..ctx.symbols.syms.len() as u32)
        .filter(|&i| ctx.symbols[i].overrides_weak_export(ctx))
        .collect();
    if binds.is_empty() && overrides.is_empty() {
        return Vec::new();
    }
    overrides.sort_by(|&a, &b| ctx.symbols[a].name().cmp(ctx.symbols[b].name()));
    binds.sort_by(|a, b| ctx.symbols[a.0].name().cmp(ctx.symbols[b.0].name()).then(a.1.cmp(&b.1)));

    let flags = BIND_SYMBOL_FLAGS_NON_WEAK_DEFINITION;
    let mut ops: Vec<Op> =
        overrides.iter().map(|&id| Op::Symbol(ctx.symbols[id].name(), flags)).collect();
    let binds: Vec<_> = binds.into_iter().map(|(id, addr)| (addr, id, 0)).collect();
    ops.extend(bind_info::bind_ops(ctx, &binds, |_| None, |_| 0));
    bind_info::encode(ops)
}
