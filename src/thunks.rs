//! Range extension thunks.
//!
//! RISC branch instructions have small immediates: ARM32 branches reach
//! ±16 MiB and ARM64 ones ±128 MiB, unlike x86-64's 32-bit displacements.
//! A call whose target is further away is redirected to a thunk, a
//! linker-synthesized code sequence between input sections that loads the
//! full address into a register and jumps there.
//!
//! Thunks are created in two passes. Before addresses are known, every
//! out-of-section call is pessimistically assumed to need a thunk; once
//! the layout is fixed, the entries that turned out to be unneeded are
//! removed. Sections only shrink in the second pass, so no existing
//! reference to a thunk goes out of range because of it.

use std::cell::UnsafeCell;

use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::chunks::output_section::OutputSection;
use crate::chunks::{ChunkId, OutputSectionId};
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::InputSection;
use crate::symbol::{AddrFlags, Symbol, SymbolId};
use crate::util::align_to;

/// A block of branch stubs placed between input sections.
#[derive(Debug)]
pub struct Thunk {
    /// Offset within the output section.
    pub offset: u64,
    pub symbols: Vec<SymbolId>,
    /// Offset of each stub within the thunk; the last entry is the size.
    pub offsets: Vec<u64>,
    pub name: String,
}

impl Thunk {
    pub fn size(&self) -> u64 {
        self.offsets.last().copied().unwrap_or(0)
    }

    pub fn addr(&self, osec: &OutputSection) -> u64 {
        osec.hdr.shdr.sh_addr + self.offset
    }

    /// The entry offsets of a thunk with fixed-size entries.
    pub fn fixed_offsets<E: Arch>(&self) -> Vec<u64> {
        let layout = E::THUNK.expect("target without thunks");
        (0..=self.symbols.len())
            .map(|i| layout.header_size + i as u64 * layout.entry_size)
            .collect()
    }
}

/// A section offset that hasn't been assigned yet.
const UNPLACED: u64 = u64::MAX;

/// Thunks are created per batch of code of this size.
fn batch_size<E: Arch>() -> u64 {
    let mib = match E::FAMILY {
        Family::Arm64 => 32,
        Family::Arm32 => 8,
        _ => 16,
    };
    mib << 20
}

/// A thunk is assumed to be smaller than half a batch.
fn max_thunk_size<E: Arch>() -> u64 {
    batch_size::<E>() / 2
}

/// Power10 prefixed instructions must not cross a 64-byte boundary.
/// Aligning each thunk to 8 bytes guarantees that.
const THUNK_ALIGN: u64 = 8;

/// The thunk symbols collected by each Rayon worker. This is the equivalent
/// of C++ mold's `enumerable_thread_specific<std::vector<Symbol *>>`.
struct ThunkSymbolBins(Vec<UnsafeCell<Vec<SymbolId>>>);

// SAFETY: a Rayon worker has a unique index and executes at most one closure
// at a time. The only caller does not invoke nested parallel work while using
// its bin; the final merge happens after the parallel traversal has joined.
unsafe impl Sync for ThunkSymbolBins {}

impl ThunkSymbolBins {
    fn new() -> ThunkSymbolBins {
        ThunkSymbolBins(
            (0..=rayon::current_num_threads())
                .map(|_| UnsafeCell::new(Vec::new()))
                .collect(),
        )
    }

    #[inline]
    fn push(&self, sym: SymbolId) {
        let fallback = self.0.len() - 1;
        let i = rayon::current_thread_index()
            .unwrap_or(fallback)
            .min(fallback);
        // SAFETY: the Sync invariant above gives this worker exclusive access
        // to its indexed vector for the duration of this non-nested closure.
        unsafe { &mut *self.0[i].get() }.push(sym);
    }

    fn into_vec(self) -> Vec<SymbolId> {
        let mut bins: Vec<Vec<SymbolId>> = self.0.into_iter().map(UnsafeCell::into_inner).collect();
        let mut symbols = Vec::with_capacity(bins.iter().map(Vec::len).sum());
        for bin in &mut bins {
            symbols.append(bin);
        }
        symbols
    }
}

/// Whether a call needs a thunk. On the first pass, before addresses are
/// known, every call out of the section is assumed to need one.
#[inline(always)]
fn requires_thunk<E: Arch>(
    ctx: &Context<E>,
    isec: &InputSection,
    rel: &ElfRel,
    sym: &Symbol,
    first_pass: bool,
) -> bool {
    if !rel.is_func_call::<E>() {
        return false;
    }

    if first_pass {
        // On the first pass, we pessimistically assume that all out-of-section
        // relocations are out of range.
        match sym.input_section_ref() {
            Some(target) if target.output_section == isec.output_section => {
                // If the target section is in the same output section but
                // hasn't got any address yet, that's unreachable.
                if target.offset() == UNPLACED {
                    return true;
                }
            }
            _ => return true,
        }

        // Even if the target is the same section, we branch to its PLT
        // if it has one. So a symbol with a PLT is also considered an
        // out-of-section reference.
        if sym.has_plt(&ctx.symbols) {
            return true;
        }
    }

    if E::always_needs_thunk(ctx, sym, rel) {
        return true;
    }

    let s = sym.addr_with(ctx, AddrFlags::NO_OPD) as i64;
    let a = isec.rel_addend::<E>(rel);
    let p = (isec.addr(ctx) + rel.r_offset) as i64;
    let val = s.wrapping_add(a).wrapping_sub(p);
    val < -E::branch_distance() || E::branch_distance() <= val
}

/// The executable output sections, in output order.
fn executable_sections<E: Arch>(ctx: &Context<E>) -> Vec<OutputSectionId> {
    ctx.chunks
        .iter()
        .filter_map(|&id| match id {
            ChunkId::Output(osec)
                if ctx.output_sections[osec.index()].hdr.shdr.sh_flags & SHF_EXECINSTR as u64
                    != 0 =>
            {
                Some(osec)
            }
            _ => None,
        })
        .collect()
}

/// Lays out an executable output section, inserting a thunk after each
/// batch of input sections.
///
/// Progress is tracked with four indices into the members, A <= B <= C
/// <= D. The sections between B and C are the current batch; A is the
/// first section that can still reach the batch, and D the last section
/// such that a thunk placed after it is reachable from the whole batch.
///
///  ................................ <input sections> ............
///     A    B    C    D
///                    ^ a thunk for the current batch goes just before D
///          <--->       the current batch, smaller than the batch size
///     <-------->       smaller than the branch distance
///          <-------->  smaller than the branch distance
///     <------------->  reachable from the current batch
pub fn create_range_extension_thunks<E: Arch>(ctx: &mut Context<E>, id: OutputSectionId) {
    let members = std::mem::take(&mut ctx.output_sections[id.index()].members);
    if members.is_empty() {
        return;
    }

    // Initialize input sections with a dummy offset so that we can
    // distinguish sections whose addresses have been assigned from those
    // whose addresses have not.
    members
        .par_iter()
        .for_each(|&member| ctx.input_section(member).set_offset(UNPLACED));

    let distance = E::branch_distance() as u64;
    let batch = batch_size::<E>();
    let max_thunk = max_thunk_size::<E>();
    let n = members.len();

    let mut thunks: Vec<Thunk> = Vec::new();
    let (mut a, mut b, mut d) = (0usize, 0usize, 0usize);
    let mut offset = 0u64;
    // The first thunk that is still reachable from the current batch.
    let mut t = 0usize;

    while b < n {
        // Move D forward as far as a thunk placed after D is reachable
        // from B.
        while d < n {
            let sec = ctx.input_section(members[d]);
            let (p2align, size) = (sec.p2align(), sec.sh_size);
            if b != d {
                let thunk_end =
                    align_to(align_to(offset, 1 << p2align) + size, THUNK_ALIGN) + max_thunk;
                if thunk_end > ctx.input_section(members[b]).offset() + distance {
                    break;
                }
            }
            offset = align_to(offset, 1 << p2align);
            sec.set_offset(offset);
            offset += size;
            d += 1;
        }

        // The batch ends at the first section that ends beyond B plus the
        // batch size. Section ends increase, so binary search; starting
        // from B + 1 guarantees progress.
        let b_offset = ctx.input_section(members[b]).offset();
        let c = b
            + 1
            + members[b + 1..d].partition_point(|&member| {
                let sec = ctx.input_section(member);
                sec.offset() + sec.sh_size < b_offset + batch
            });

        // The first section within branch range of C.
        let c_offset = if c == d {
            offset
        } else {
            ctx.input_section(members[c]).offset()
        };
        a += members[a..b].partition_point(|&member| {
            ctx.input_section(member).offset() < c_offset.saturating_sub(distance)
        });

        // Thunks before A are out of range now.
        while t < thunks.len() && thunks[t].offset < ctx.input_section(members[a]).offset() {
            for &sym in &thunks[t].symbols {
                ctx.symbols[sym].unmark();
            }
            t += 1;
        }

        // Create a thunk at D for the calls of the batch. A symbol
        // already covered by a reachable thunk is marked and skipped.
        offset = align_to(offset, THUNK_ALIGN);
        let symbol_bins = ThunkSymbolBins::new();
        {
            let ctx: &Context<E> = ctx;
            members[b..c].par_iter().for_each(|&member| {
                let isec = ctx.input_section(member);
                let file = &ctx.objs[isec.file.index()];
                for rel in isec.rels::<E>(file) {
                    if !rel.is_func_call::<E>() {
                        continue;
                    }
                    let id = file.base.symbols[rel.r_sym as usize];
                    let sym = &ctx.symbols[id];
                    if requires_thunk(ctx, isec, &rel, sym, true) && sym.mark() {
                        symbol_bins.push(id);
                    }
                }
            });
        }
        let mut thunk = Thunk {
            offset,
            symbols: symbol_bins.into_vec(),
            offsets: Vec::new(),
            name: String::new(),
        };
        thunk.offsets = thunk.fixed_offsets::<E>();
        debug_assert!(thunk.size() < max_thunk);
        offset += thunk.size();
        thunks.push(thunk);

        b = c;
    }

    for thunk in &thunks[t..] {
        for &sym in &thunk.symbols {
            ctx.symbols[sym].unmark();
        }
    }

    // Sort the symbols for deterministic output.
    {
        let ctx: &Context<E> = ctx;
        thunks.par_iter_mut().for_each(|thunk| {
            thunk.symbols.sort_by_key(|&id| {
                let sym = &ctx.symbols[id];
                (sym.file().map_or(0, |f| ctx.file(f).priority), sym.sym_idx)
            });
        });
    }

    let osec = &mut ctx.output_sections[id.index()];
    osec.hdr.shdr.sh_size = offset;
    osec.thunks = thunks;
    osec.members = members;
}

/// Now that addresses are known, drops the thunk entries for calls that
/// turned out to be in range, and shrinks the sections accordingly.
pub fn remove_redundant_thunks<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("remove_redundant_thunks");
    let sections = executable_sections(ctx);

    // Mark the symbols that really need thunks.
    {
        let ctx: &Context<E> = ctx;
        for &id in &sections {
            ctx.output_sections[id.index()]
                .members
                .par_iter()
                .for_each(|&m| {
                    let isec = ctx.input_section(m);
                    let file = &ctx.objs[isec.file.index()];
                    for rel in isec.rels::<E>(file) {
                        if !rel.is_func_call::<E>() {
                            continue;
                        }
                        let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
                        if !sym.is_marked() && requires_thunk(ctx, isec, &rel, sym, false) {
                            sym.mark();
                        }
                    }
                });
        }
    }

    for &id in &sections {
        let mut thunks = std::mem::take(&mut ctx.output_sections[id.index()].thunks);
        {
            let ctx: &Context<E> = ctx;
            let osec = &ctx.output_sections[id.index()];
            thunks.par_iter_mut().for_each(|thunk| {
                thunk.symbols.retain(|&sym| ctx.symbols[sym].is_marked());
                thunk.offsets = E::thunk_offsets(ctx, thunk, thunk.addr(osec));
            });
        }

        // Lay the members and thunks out again, in their existing order.
        let members = ctx.output_sections[id.index()].members.clone();
        let (mut mi, mut ti) = (0, 0);
        let mut offset = 0;
        while mi < members.len() || ti < thunks.len() {
            let member_first = mi < members.len()
                && (ti >= thunks.len()
                    || ctx.input_section(members[mi]).offset() < thunks[ti].offset);
            if member_first {
                let sec = ctx.input_section(members[mi]);
                offset = align_to(offset, 1 << sec.p2align());
                sec.set_offset(offset);
                offset += sec.sh_size;
                mi += 1;
            } else {
                offset = align_to(offset, THUNK_ALIGN);
                thunks[ti].offset = offset;
                offset += thunks[ti].size();
                ti += 1;
            }
        }
        let osec = &mut ctx.output_sections[id.index()];
        osec.hdr.shdr.sh_size = offset;
        osec.thunks = thunks;
    }
}

/// Records with each symbol the addresses of its thunk entries, so that
/// applying a branch relocation can find one in range quickly.
pub fn gather_thunk_addresses<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("gather_thunk_addresses");
    let mut sections = executable_sections(ctx);
    sections.sort_by_key(|id| ctx.output_sections[id.index()].hdr.shdr.sh_addr);

    for id in sections {
        let osec = &ctx.output_sections[id.index()];
        let entries: Vec<(SymbolId, u64)> = osec
            .thunks
            .iter()
            .flat_map(|thunk| {
                let base = thunk.addr(osec);
                thunk
                    .symbols
                    .iter()
                    .enumerate()
                    .map(move |(i, &sym)| (sym, base + thunk.offsets[i]))
            })
            .collect();
        for (sym, addr) in entries {
            ctx.symbols.add_thunk_addr(sym, addr);
        }
    }
}

/// Writes a thunk's stubs into the output section buffer.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, osec: &OutputSection, thunk: &Thunk, buf: &mut [u8]) {
    debug_assert_eq!(buf.len(), thunk.size() as usize);
    E::write_thunk(ctx, thunk, thunk.addr(osec), buf);
}
