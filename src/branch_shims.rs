//! References to code and data more than 4 GiB away.
//!
//! An arm64 b/bl reaches 128 MiB and an adrp 4 GiB, so a reference
//! between two segments that -segaddr puts 4 GiB apart can't be made
//! directly. ld-prime sends a branch between such segments through a
//! "shim": the target's __stubs entry, which jumps through a __got slot
//! dyld rebases to the target (the slot shared with a GOT load's), in
//! any image dyld loads - not a
//! -static or -preload one, where the branch is a fixup error. It keeps
//! a GOT load between them a load from the slot rather than relaxing it
//! to an adrp+add that could not reach, in any image. A branch from the
//! far segment back to __TEXT's stubs is out of reach all the same.
//! (With classic dyld info ld-prime binds such a stub lazily, by name in
//! the image itself, which crashes when the target is not exported;
//! ours jumps through the rebased slot there too.)
//!
//! Whether two segments are that far apart ld-prime decides before it
//! lays the image out: a segment -segaddr pins (to an address other
//! than 0) is at its address, and every other one a page past the
//! image's base - the end of __PAGEZERO in an executable, else 0 -
//! wherever it lands after all. So two segments are far apart when one
//! is pinned at least 4 GiB from the other, or from that page if the
//! other floats: an unpinned segment of a dylib whose __TEXT is pinned
//! at 8 GiB is far from __TEXT, whatever the distance it ends up at.

use rayon::prelude::*;

use crate::context::Context;
use crate::input_sections::NO_REPLACEMENT;
use crate::macho::CPU_TYPE_ARM64;
use crate::symbol::SymbolId;
use crate::target::{RelocClass, Target};

/// Where ld-prime takes segment `segname` to be when it decides which
/// references can't reach (see the module comment).
fn provisional_addr<E: Target>(ctx: &Context<E>, segname: &[u8]) -> u64 {
    match ctx.args.segaddr(segname) {
        Some(addr) if addr != 0 => addr,
        _ => ctx.args.pagezero_size + ctx.args.segment_align,
    }
}

/// The segment subsection `isec` lands in, once output sections are made.
fn segment_of<E: Target>(ctx: &Context<E>, isec: usize) -> Option<&'static [u8]> {
    let isec = ctx.resolve_isec(isec);
    ctx.isecs[isec].output_section().map(|id| ctx.chunk_header(id).segname)
}

/// Whether a reference from subsection `isec` to symbol `id` spans 4
/// GiB in ld-prime's reckoning: an arm64 image's, between segments it
/// takes to be that far apart. No pin, no such reference.
pub fn is_far<E: Target>(ctx: &Context<E>, isec: usize, id: SymbolId) -> bool {
    if E::CPUTYPE != CPU_TYPE_ARM64 || ctx.args.segaddrs.is_empty() {
        return false;
    }
    let Some(target) = ctx.symbols[id].input_section() else { return false };
    let (Some(from), Some(to)) = (segment_of(ctx, isec), segment_of(ctx, target as usize)) else {
        return false;
    };
    from != to && provisional_addr(ctx, from).abs_diff(provisional_addr(ctx, to)) >= 1 << 32
}

/// Gives the targets of far references what they go through (see the
/// module comment): a branch's target defined here a shim - a stub and
/// a GOT slot - unless it has an addend (ld-prime's shim takes it too,
/// branching into the middle of the stub), and a relaxable GOT load's
/// target a GOT slot. Runs once the input sections have their output
/// sections, before __stubs and __got are sized, and sorts them again
/// if it adds to them.
pub fn add_far_ref_slots<E: Target>(ctx: &mut Context<E>) {
    if E::CPUTYPE != CPU_TYPE_ARM64 || ctx.args.segaddrs.is_empty() || ctx.args.relocatable {
        return;
    }
    let shims = !ctx.args.without_dyld();
    let ctx_ref: &Context<E> = ctx;
    let mut slots: Vec<(SymbolId, bool)> = (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| {
            let isec = &ctx_ref.isecs[i];
            isec.is_alive() && isec.replacement == NO_REPLACEMENT && isec.nrels > 0
        })
        .flat_map_iter(|i| {
            let obj = ctx_ref.isecs[i].file as usize;
            ctx_ref.isec_relocs(i).iter().filter_map(move |r| {
                let id = ctx_ref.reloc_target_sym(obj, r)?;
                let stub = match E::classify_reloc(r.r_type) {
                    RelocClass::Branch
                        if shims && r.addend == 0 && !ctx_ref.binds_at_runtime(id) =>
                    {
                        true
                    }
                    RelocClass::GotLoad if ctx_ref.can_relax_got(id) => false,
                    _ => return None,
                };
                is_far(ctx_ref, i, id).then_some((id, stub))
            })
        })
        .collect();
    if slots.is_empty() {
        return;
    }
    slots.sort_unstable();
    slots.dedup();
    for (id, stub) in slots {
        if stub {
            crate::passes::add_stub(ctx, id);
        }
        crate::passes::add_got(ctx, id);
    }
    crate::passes::sort_stubs_and_got(ctx);
}
