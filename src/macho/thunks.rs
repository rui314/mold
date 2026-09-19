//! Range-extension thunks.
//!
//! An arm64 bl/b reaches +-128 MiB; a __TEXT section larger than that
//! needs islands of trampolines so any branch can reach its target.
//! The layout follows mold's design (mold-rust's thunks.rs): thunks
//! are placed after each batch of code, and a batch never grows so
//! large that its own thunk would fall out of reach - the layout
//! cursor and the scan cursor stay within one branch reach (minus
//! margin) of each other, so placing a thunk can never invalidate an
//! earlier layout decision.
//!
//! As in mold-rust, a thunk entry belongs to a *symbol*, not to a
//! relocation: the first pass pessimistically gives every symbol that
//! some branch of the batch might not reach an entry, deduplicated by
//! an atomic mark on the symbol inside the parallel scan, and a symbol
//! keeps its mark - and so gets no second entry - for as long as that
//! entry stays within reach of the batches that follow, and
//! gather_thunk_addresses records each symbol's entry addresses so that
//! applying an out-of-range branch just picks the one within reach.
//!
//! mold-rust additionally trims the pessimistic entries once addresses
//! are final (remove_redundant_thunks) and lays the section out again.
//! Ours does not: the first pass already skips targets the section's
//! size bound proves reachable, so on a debug clang link the trim
//! recovered 0.07% of __text while the rescan of every branch plus the
//! second __TEXT placement (which re-encodes __unwind_info) cost 5% of
//! the link. The extra entries are dead code in the thunk islands.

use crate::macho::arch::{Arch, RelocClass};
use crate::macho::context::Context;
use crate::macho::input_files::FileId;
use crate::macho::input_sections::InputSectionId;
use crate::macho::output_chunks::{self, OutputSectionId};
use crate::macho::symbol::SymbolId;
use crate::util::align_to;

/// Lays out the subsections of one big executable output section with
/// range-extension thunks interleaved. Each subsection gets its
/// output_offset; the thunks, with their symbols, are returned.
pub fn create_range_extension_thunks<E: Arch>(
    ctx: &mut Context<E>,
    isecs: &[InputSectionId],
) -> Vec<output_chunks::Thunk> {
    const BATCH: u64 = 10 * 1024 * 1024;
    const MAX_THUNK: u64 = 1024 * 1024;
    let budget = E::BRANCH_RANGE / 2 - MAX_THUNK - BATCH;
    // A thunk stays usable by a batch while it is this close to the
    // batch's end.
    let reach = E::BRANCH_RANGE / 2 - MAX_THUNK;

    // An upper bound on the section's final size: every subsection
    // with worst-case alignment padding, plus a full thunk per batch.
    // A forward branch's target can't land beyond this, so if the
    // bound is within forward reach of a batch, that batch needs no
    // entries for still-unplaced targets - which is what keeps a
    // merely-large section (bigger than the trigger, far smaller than
    // the branch range) from drowning in reserved-but-unused thunk
    // entries.
    let total_estimate: u64 = isecs.iter().map(|&id| ctx.isecs[id].size as u64 + 16).sum::<u64>();
    let total_estimate = total_estimate + (total_estimate / BATCH + 1) * MAX_THUNK;

    let mut thunks: Vec<output_chunks::Thunk> = Vec::new();
    // Thunks before this index have fallen out of reach of the current
    // batch; their symbols are unmarked so they can take a new entry
    // (mold-rust's cursor A).
    let mut reachable_from = 0usize;
    let mut off: u64 = 0;
    let mut i = 0;

    // Distinguish placed subsections from ones still ahead.
    for &id in isecs {
        ctx.isecs[id].offset = u32::MAX;
    }

    while i < isecs.len() {
        let batch_start_off = off;
        let batch_start = i;

        // A single subsection larger than the budget can't have a thunk
        // after it within reach; its thunk goes in front instead, where
        // at least branches from its first reach's worth of code can
        // use it.
        let first_size = ctx.isecs[isecs[i]].size as u64;
        if first_size > budget {
            let monster = isecs[i];
            let thunk_off = align_to(off, 16);
            release_out_of_reach(ctx, &thunks, &mut reachable_from, thunk_off, reach);
            let fwd_ok = total_estimate - off <= E::BRANCH_RANGE / 2 - MAX_THUNK;
            let n = scan_batch::<E>(ctx, &[monster], thunk_off, fwd_ok, &mut thunks);
            off = thunk_off + n * E::THUNK_SIZE;
            let isec = &mut ctx.isecs[monster];
            off = isec.align_offset(off);
            isec.offset = off as u32;
            off += isec.size as u64;
            i += 1;
            continue;
        }

        // Place a batch: bounded by BATCH bytes, and by the thunk after
        // it staying within reach of the batch start.
        while i < isecs.len() {
            let isec = &ctx.isecs[isecs[i]];
            let aligned = isec.align_offset(off);
            if i != batch_start
                && (aligned + isec.size as u64 - batch_start_off > budget
                    || aligned - batch_start_off >= BATCH)
            {
                break;
            }
            let isec = &mut ctx.isecs[isecs[i]];
            isec.offset = aligned as u32;
            off = aligned + isec.size as u64;
            i += 1;
        }

        let thunk_off = align_to(off, 16);
        // The batch's last branch is just before thunk_off; anything
        // farther back than a reach from there is unusable by it.
        release_out_of_reach(ctx, &thunks, &mut reachable_from, thunk_off, reach);
        let batch: Vec<InputSectionId> = isecs[batch_start..i].to_vec();
        let fwd_ok = total_estimate - batch_start_off <= E::BRANCH_RANGE / 2 - MAX_THUNK;
        let n = scan_batch::<E>(ctx, &batch, thunk_off, fwd_ok, &mut thunks);
        if n > 0 {
            off = thunk_off + n * E::THUNK_SIZE;
        }
    }

    // Marks of the thunks still in reach at the end are cleared too.
    for thunk in &thunks[reachable_from..] {
        for &sym in &thunk.syms {
            ctx.symbols[sym].unmark();
        }
    }
    thunks
}

/// Unmarks the symbols of every thunk that a branch at `from` can no
/// longer reach, advancing the reachable-thunk cursor past them.
fn release_out_of_reach<E: Arch>(
    ctx: &Context<E>,
    thunks: &[output_chunks::Thunk],
    reachable_from: &mut usize,
    from: u64,
    reach: u64,
) {
    while *reachable_from < thunks.len() && thunks[*reachable_from].offset + reach <= from {
        for &sym in &thunks[*reachable_from].syms {
            ctx.symbols[sym].unmark();
        }
        *reachable_from += 1;
    }
}

/// Scans `batch`'s branch relocations in parallel and, for every target
/// that may be out of reach and is not already covered by a thunk still
/// in reach (its symbol is marked), claims the symbol with mark() and
/// gives it an entry in a new thunk at `thunk_off`. Returns the entry
/// count. mold-rust scans each batch's members with par_iter and
/// dedups with the symbol's atomic mark the same way.
fn scan_batch<E: Arch>(
    ctx: &mut Context<E>,
    batch: &[InputSectionId],
    thunk_off: u64,
    forward_reachable: bool,
    thunks: &mut Vec<output_chunks::Thunk>,
) -> u64 {
    use rayon::prelude::*;
    let ctx_ref: &Context<E> = ctx;
    let mut syms: Vec<SymbolId> = batch
        .par_iter()
        .flat_map_iter(|&isec_id| {
            let mut out = Vec::new();
            let obj = ctx_ref.isecs[isec_id].file as usize;
            let osec = ctx_ref.isecs[isec_id].output_section();
            let ro = ctx_ref.isecs[isec_id].rel_offset as usize;
            let nr = ctx_ref.isecs[isec_id].nrels as usize;
            for r in 0..nr {
                let rel = ctx_ref.objs[obj].relocs[ro + r];
                if E::classify_reloc(rel.r_type) != RelocClass::Branch {
                    continue;
                }
                let Some(sym_id) = ctx_ref.reloc_target_sym(obj, &rel) else {
                    continue;
                };
                let sym = &ctx_ref.symbols[sym_id];
                if let (Some(FileId::Obj(_)), Some(target)) = (sym.file(), sym.input_section()) {
                    let t = &ctx_ref.isecs[ctx_ref.resolve_isec(target as usize)];
                    // A target in another output section has no offset
                    // in this section's space; reserve an entry.
                    if t.output_section() != osec {
                        // conservative: fall through to the entry below
                    } else if t.offset != u32::MAX {
                        let target_off = t.offset as u64 + sym.value;
                        if thunk_off.saturating_sub(target_off) < E::BRANCH_RANGE / 2 - 1024 * 1024
                        {
                            continue;
                        }
                    } else if forward_reachable {
                        // Still unplaced, but the whole section fits
                        // within forward reach of this batch.
                        continue;
                    }
                }
                if sym.mark() {
                    out.push(sym_id);
                }
            }
            out
        })
        .collect();
    if syms.is_empty() {
        return 0;
    }
    // Deterministic entry order regardless of which thread claimed
    // each symbol.
    syms.par_sort_unstable();
    let n = syms.len() as u64;
    thunks.push(output_chunks::Thunk { offset: thunk_off, syms });
    n
}

/// Records every thunk entry's address on its symbol (SymAux::
/// thunk_addrs), in address order, so that applying an out-of-range
/// branch can pick the entry within reach. mold-rust's
/// gather_thunk_addresses.
pub fn gather_thunk_addresses<E: Arch>(ctx: &mut Context<E>, osecs: &[OutputSectionId]) {
    // The sections are read while the symbol tables are written, so
    // the borrows are split and the addresses recorded as the thunks
    // are walked, without a temporary list (mold-rust 94e2104).
    let output_sections = &ctx.output_sections;
    let symtab = &mut ctx.symbols;
    let sym_aux = &mut ctx.sym_aux;
    for &id in osecs {
        let osec = &output_sections[id.index()];
        let base = osec.hdr.addr;
        for thunk in &osec.thunks {
            for (i, &sym) in thunk.syms.iter().enumerate() {
                let addr = base + thunk.offset + i as u64 * E::THUNK_SIZE;
                Context::<E>::sym_aux_mut_in(symtab, sym_aux, sym).thunk_addrs.push(addr);
            }
        }
    }
}

/// The address of a thunk entry for `sym` that a branch at `pc` can
/// reach, if it has one.
#[inline]
pub fn reachable_thunk_addr<E: Arch>(ctx: &Context<E>, sym: SymbolId, pc: u64) -> Option<u64> {
    let range = (E::BRANCH_RANGE / 2) as i64;
    ctx.sym_aux(sym).thunk_addrs.iter().copied().find(|&t| {
        let d = t.wrapping_sub(pc) as i64;
        (-range..range).contains(&d)
    })
}
