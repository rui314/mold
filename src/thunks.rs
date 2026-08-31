//! RISC instructions are usually up to 4 bytes long, so the immediates of
//! their branch instructions are naturally smaller than 32 bits.  This is
//! contrary to x86-64 on which branch instructions take 4 bytes immediates
//! and can jump to anywhere within PC ± 2 GiB.
//!
//! In fact, ARM32's branch instructions can jump only within ±16 MiB and
//! ARM64's ±128 MiB, for example. If a branch target is further than that,
//! we need to let it branch to a linker-synthesized code sequence that
//! construct a full 32 bit address in a register and jump there. That
//! linker-synthesized code is called "thunk".
//!
//! The function in this file creates thunks.
//!
//! Thunk size varies widely across programs. In an ARM64 build of Clang 16,
//! thunks occupy about 30 KiB (0.01%) of a ~300 MiB text section, compared
//! with about 12.5 MiB (2.5%) of a ~500 MiB text section in TensorFlow.
//!
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
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::InputSection;
use crate::output_chunks::output_section::OutputSection;
use crate::output_chunks::{ChunkId, OutputSectionId};
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

/// We create thunks for each 32/8/16 MiB code block for
/// ARM64/ARM32/PPC, respectively.
fn batch_size<E: Arch>() -> u64 {
    let mib = match E::FAMILY {
        Family::Arm64 => 32,
        Family::Arm32 => 8,
        _ => 16,
    };
    mib << 20
}

/// We assume that a single thunk group is smaller than 16/4/8 MiB
/// for ARM64/ARM32/PPC, respectively.
fn max_thunk_size<E: Arch>() -> u64 {
    batch_size::<E>() / 2
}

/// Power10 prefixed instructions must not cross a 64-byte boundary.
/// Aligning each thunk group to a 8-byte boundary guarantees that.
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
                // hasn't got any address yet, that's unreacahble.
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

    // Compute a distance between the relocated place and the symbol
    // and check if they are within reach.
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

/// We create thunks from the beginning of the section to the end.
/// We manage progress using four offsets which increase monotonically.
/// The locations they point to are always A <= B <= C <= D.
///
/// Input sections between B and C are the current batch.
///
/// A is the input section with the smallest address than can reach
/// from the current batch.
///
/// D is the input section with the largest address such that the thunk
/// is reachable from the current batch if it's inserted at D.
///
///  ................................ <input sections> ............
///     A    B    C    D
///                    ^ We insert a thunk for the current batch just before D
///          <--->       The current batch, which is smaller than BATCH_SIZE
///     <-------->       Smaller than BRANCH_DISTANCE
///          <-------->  Smaller than BRANCH_DISTANCE
///     <------------->  Reachable from the current batch
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
    // The smallest thunk index that is reachable from the current batch.
    let mut t = 0usize;

    while b < n {
        // Move D foward as far as we can jump from B to a thunk at D.
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

        // Find the end of the current batch. Section end addresses are sorted,
        // so use binary search. Starting from B + 1 guarantees progress.
        let b_offset = ctx.input_section(members[b]).offset();
        let c = b
            + 1
            + members[b + 1..d].partition_point(|&member| {
                let sec = ctx.input_section(member);
                sec.offset() + sec.sh_size < b_offset + batch
            });

        // Find the first section that is within branch range of C.
        let c_offset = if c == d {
            offset
        } else {
            ctx.input_section(members[c]).offset()
        };
        a += members[a..b].partition_point(|&member| {
            ctx.input_section(member).offset() < c_offset.saturating_sub(distance)
        });

        // Erase references to out-of-range thunks.
        while t < thunks.len() && thunks[t].offset < ctx.input_section(members[a]).offset() {
            for &sym in &thunks[t].symbols {
                ctx.symbols[sym].unmark();
            }
            t += 1;
        }

        // Create a new thunk and place it at D.
        offset = align_to(offset, THUNK_ALIGN);
        let symbol_bins = ThunkSymbolBins::new();
        {
            let ctx: &Context<E> = ctx;
            // Scan relocations between B and C to collect symbols that need
            // entries in the new thunk.
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
        // Add symbols to the thunk
        let mut thunk = Thunk {
            offset,
            symbols: symbol_bins.into_vec(),
            offsets: Vec::new(),
            name: String::new(),
        };
        // Now that we know the number of symbols in the thunk, we can compute
        // the thunk's size.
        thunk.offsets = thunk.fixed_offsets::<E>();
        debug_assert!(thunk.size() < max_thunk);
        offset += thunk.size();
        thunks.push(thunk);

        // Move B forward to point to the begining of the next batch.
        b = c;
    }

    // Clear marks for thunks that are still reachable from the last batch.
    for thunk in &thunks[t..] {
        for &sym in &thunk.symbols {
            ctx.symbols[sym].unmark();
        }
    }

    // Sort symbols for deterministic output.
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

/// create_range_extension_thunks() creates thunks with a pessimistic
/// assumption that all out-of-section references are out of range.
/// After computing output section addresses, we revisit all thunks to
/// remove unneeded entries from them.
///
/// We create more thunks than necessary and then eliminate some of
/// them later, instead of just creating thunks at this stage. This is
/// because we can safely shrink sections after assigning addresses to
/// them without worrying about making existing references to thunks go
/// out of range. On the other hand, if we insert thunks after
/// assigning addresses to sections, references to thunks could become
/// out of range due to the new extra gaps for thunks. Thus, the
/// creation of thunks is a two-pass process.
pub fn remove_redundant_thunks<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("remove_redundant_thunks");
    // Gather output executable sections
    let sections = executable_sections(ctx);

    // Mark all symbols that actually need range extension thunks
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
            // Remove symbols from thunks if they don't actually need range
            // extension thunks
            thunks.par_iter_mut().for_each(|thunk| {
                thunk.symbols.retain(|&sym| ctx.symbols[sym].is_marked());
                thunk.offsets = E::thunk_offsets(ctx, thunk, thunk.addr(osec));
            });
        }

        // Recompute section sizes
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

/// When applying relocations, we want to know the address in a reachable
/// range extension thunk for a given symbol. Doing it by scanning all
/// reachable range extension thunks is too expensive.
///
/// In this function, we create a list of all addresses in range extension
/// thunks for each symbol, so that it is easy to find one.
///
/// Note that thunk_addrs must be sorted for binary search.
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
