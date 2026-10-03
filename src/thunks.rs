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
//! Thunks are created in two passes. Before addresses are known, every
//! out-of-section call is pessimistically assumed to need a thunk; once
//! the layout is fixed, the entries that turned out to be unneeded are
//! removed. Sections only shrink in the second pass, so no existing
//! reference to a thunk goes out of range because of it.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::chunks::note_property;
use crate::chunks::output_section::OutputSection;
use crate::chunks::{ChunkId, OutputSectionId};
use crate::context::Context;
use crate::elf::*;
use crate::error;
use crate::input_sections::{InputSection, InputSectionId};
use crate::symbol::{AddrFlags, Symbol, SymbolId};
use crate::target::{Family, Target};
use crate::util::align_to;
use crate::util::endian::read_ul32;

/// A block of branch stubs placed between input sections.
#[derive(Debug)]
pub struct Thunk {
    /// Offset within the output section.
    pub offset: u64,
    /// Functions whose landing pads are at the beginning of this thunk. See
    /// find_landing_pads().
    pub landing_pads: Vec<SymbolId>,
    pub symbols: Vec<SymbolId>,
    /// Offset of each stub within the thunk; the last entry is the size.
    pub offsets: Vec<u64>,
    pub name: String,
}

/// The size of a landing pad, which consists of `bti c` and `b <function>`.
pub const LANDING_PAD_SIZE: u64 = 8;

impl Thunk {
    pub fn size(&self) -> u64 {
        self.offsets.last().copied().unwrap_or(0)
    }

    pub fn addr<E: Target>(&self, osec: &OutputSection<E>) -> u64 {
        osec.hdr.shdr.sh_addr.get() + self.offset
    }

    /// The entry offsets of a thunk with fixed-size entries.
    pub fn fixed_offsets<E: Target>(&self) -> Vec<u64> {
        let layout = E::THUNK.expect("target without thunks");
        let base = self.landing_pads.len() as u64 * LANDING_PAD_SIZE + layout.header_size;
        (0..=self.symbols.len()).map(|i| base + i as u64 * layout.entry_size).collect()
    }
}

/// A section offset that hasn't been assigned yet.
const UNPLACED: u64 = u64::MAX;

/// We create thunks for each 32/8/16 MiB code block for
/// ARM64/ARM32/PPC, respectively.
fn batch_size<E: Target>() -> u64 {
    let mib = match E::FAMILY {
        Family::Arm64 => 32,
        Family::Arm32 => 8,
        _ => 16,
    };
    mib << 20
}

/// We assume that a single thunk group is smaller than 16/4/8 MiB
/// for ARM64/ARM32/PPC, respectively.
fn max_thunk_size<E: Target>() -> u64 {
    batch_size::<E>() / 2
}

/// Power10 prefixed instructions must not cross a 64-byte boundary.
/// Aligning each thunk group to a 8-byte boundary guarantees that.
const THUNK_ALIGN: u64 = 8;

/// Whether a call needs a thunk. On the first pass, before addresses are
/// known, every call out of the section is assumed to need one.
fn requires_thunk<E: Target>(
    ctx: &Context<E>,
    isec: &InputSection<E>,
    rel: &ElfRel<E>,
    sym: &Symbol,
    first_pass: bool,
) -> bool {
    if !rel.is_func_call::<E>() {
        return false;
    }

    if first_pass {
        // On the first pass, we pessimistically assume that all out-of-section
        // relocations are out of range.
        match sym.input_section_ref(ctx) {
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
    let a = isec.rel_addend(rel);
    let p = (isec.addr(ctx) + rel.r_offset()) as i64;
    let val = s.wrapping_add(a).wrapping_sub(p);
    val < -E::branch_distance() || E::branch_distance() <= val
}

/// The executable output sections, in output order.
fn executable_sections<E: Target>(ctx: &Context<E>) -> Vec<OutputSectionId> {
    ctx.chunks
        .iter()
        .filter_map(|&id| match id {
            ChunkId::Output(osec)
                if ctx.output_sections[osec.index()].hdr.shdr.sh_flags.get()
                    & SHF_EXECINSTR as u64
                    != 0 =>
            {
                Some(osec)
            }
            _ => None,
        })
        .collect()
}

/// The key to sort symbols in a thunk for deterministic output.
fn sort_key<E: Target>(ctx: &Context<E>, id: SymbolId) -> (u32, u32) {
    let sym = &ctx.symbols[id];
    (sym.file().map_or(0, |f| ctx.file(f).priority), sym.sym_idx())
}

/// The input section containing the code of `sym`. A section folded by ICF
/// has no code of its own, so this returns the section it was folded into.
fn code_section<E: Target>(ctx: &Context<E>, sym: &Symbol) -> Option<InputSectionId> {
    let id = sym.input_section()?;
    match ctx.input_section(id).icf_leader() {
        Some(leader) => ctx.objs[leader.file.index()].section_id(leader.shndx as usize),
        None => Some(id),
    }
}

/// Whether `sym` begins with an instruction that an indirect branch may land
/// on if BTI is enabled, i.e., `bti c`, `bti j`, `bti jc`, `paciasp` or
/// `pacibsp`. If `sym` is not within `isec`'s contents, we can't tell, so
/// we assume it is one as lld does.
fn is_landing_pad<E: Target>(isec: &InputSection<E>, sym: &Symbol) -> bool {
    let off = sym.value as usize;
    let Some(loc) = isec.contents().get(off..off + 4) else {
        return true;
    };
    matches!(read_ul32(loc), 0xd503_245f | 0xd503_249f | 0xd503_24df | 0xd503_233f | 0xd503_237f)
}

/// If ARM64 BTI is enabled, an indirect branch must land on a landing pad
/// instruction such as `bti c`, and a thunk jumps to its destination with
/// an indirect branch. However, the compiler doesn't emit a landing pad at
/// the beginning of a function that is only called directly, so such a
/// function can't be a destination of a thunk as is.
///
/// For such a function, we create a landing pad consisting of `bti c` and
/// `b <function>` at the beginning of the first thunk after the function's
/// section, and other thunks jump to the landing pad instead. The thunk is
/// always within direct branch range of the function because it's
/// reachable from all sections in its batch, and the batch starts at or
/// before the function's section.
///
/// This function returns functions that may need a landing pad, grouped by
/// the sections containing them. We don't know yet which calls are out of
/// range, so any function called from another section is a candidate.
pub fn find_landing_pads<E: Target>(ctx: &Context<E>) -> HashMap<InputSectionId, Vec<SymbolId>> {
    let mut map: HashMap<InputSectionId, Vec<SymbolId>> = HashMap::new();
    if ctx.args.relocatable || !note_property::is_bti(ctx) {
        return map;
    }

    let members: Vec<InputSectionId> = executable_sections(ctx)
        .into_iter()
        .flat_map(|id| ctx.output_sections[id.index()].members.iter().copied())
        .collect();

    let mut syms: Vec<(InputSectionId, SymbolId)> = members
        .par_iter()
        .flat_map_iter(|&member| {
            let isec = ctx.input_section(member);
            let file = &ctx.objs[isec.file.index()];
            isec.rels(file).iter().filter_map(move |rel| {
                let id = file.base.symbols[rel.r_sym() as usize];
                let sym = &ctx.symbols[id];
                if !rel.is_func_call::<E>() || sym.has_plt(&ctx.symbols) {
                    return None;
                }
                let target = code_section(ctx, sym)?;
                if target == member || is_landing_pad(ctx.input_section(target), sym) {
                    return None;
                }
                Some((target, id))
            })
        })
        .collect();

    syms.par_sort_by_key(|&(target, id)| (target, sort_key(ctx, id)));
    syms.dedup();
    for (target, id) in syms {
        map.entry(target).or_default().push(id);
    }
    map
}

/// Returns the address of the landing pad of `id` if it has one.
pub fn landing_pad_addr<E: Target>(ctx: &Context<E>, id: SymbolId) -> Option<u64> {
    if !note_property::is_bti(ctx) {
        return None;
    }
    let isec = ctx.input_section(code_section(ctx, &ctx.symbols[id])?);
    let osec = &ctx.output_sections[isec.output_section?.index()];

    // Find the first thunk after the section.
    let thunk = osec.thunks.get(osec.thunks.partition_point(|t| t.offset <= isec.offset()))?;
    let key = sort_key(ctx, id);
    let i = thunk.landing_pads.binary_search_by_key(&key, |&x| sort_key(ctx, x)).ok()?;
    Some(thunk.addr(osec) + i as u64 * LANDING_PAD_SIZE)
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
///
/// `landing_pads` is the result of find_landing_pads().
pub fn create_range_extension_thunks<E: Target>(
    ctx: &mut Context<E>,
    id: OutputSectionId,
    landing_pads: &HashMap<InputSectionId, Vec<SymbolId>>,
) {
    let members = std::mem::take(&mut ctx.output_sections[id.index()].members);
    if members.is_empty() {
        return;
    }

    // Initialize input sections with a dummy offset so that we can
    // distinguish sections whose addresses have been assigned from those
    // whose addresses have not.
    members.par_iter().for_each(|&member| ctx.input_section(member).set_offset(UNPLACED));

    let distance = E::branch_distance() as u64;
    let batch = batch_size::<E>();
    let max_thunk = max_thunk_size::<E>();
    let n = members.len();

    let mut thunks: Vec<Thunk> = Vec::new();
    let (mut a, mut b, mut d) = (0usize, 0usize, 0usize);
    let mut offset = 0u64;
    // The smallest thunk index that is reachable from the current batch.
    let mut t = 0usize;
    // Sections before this index precede an already created thunk.
    let mut prev_d = 0usize;

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
        let c_offset = if c == d { offset } else { ctx.input_section(members[c]).offset() };
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
        let symbols = {
            let ctx: &Context<E> = ctx;
            // Scan relocations between B and C to collect symbols that need
            // entries in the new thunk.
            members[b..c]
                .par_iter()
                .fold(Vec::new, |mut symbols, &member| {
                    let isec = ctx.input_section(member);
                    let file = &ctx.objs[isec.file.index()];
                    for rel in isec.rels(file) {
                        if !rel.is_func_call::<E>() {
                            continue;
                        }
                        let id = file.base.symbols[rel.r_sym() as usize];
                        let sym = &ctx.symbols[id];
                        if requires_thunk(ctx, isec, rel, sym, true) && sym.mark() {
                            symbols.push(id);
                        }
                    }
                    symbols
                })
                .reduce(Vec::new, |mut symbols, mut other| {
                    symbols.append(&mut other);
                    symbols
                })
        };
        // Add symbols to the thunk. Functions in the sections placed since
        // the previous thunk get their landing pads in this thunk.
        let mut thunk = Thunk {
            offset,
            landing_pads: members[prev_d..d]
                .iter()
                .filter_map(|member| landing_pads.get(member))
                .flatten()
                .copied()
                .collect(),
            symbols,
            offsets: Vec::new(),
            name: String::new(),
        };
        prev_d = d;
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
            thunk.landing_pads.sort_by_key(|&id| sort_key(ctx, id));
            thunk.symbols.sort_by_key(|&id| sort_key(ctx, id));
        });
    }

    let osec = &mut ctx.output_sections[id.index()];
    osec.hdr.shdr.sh_size.set(offset);
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
pub fn remove_redundant_thunks<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("remove_redundant_thunks");
    // Gather output executable sections
    let sections = executable_sections(ctx);

    // Mark all symbols that actually need range extension thunks
    {
        let ctx: &Context<E> = ctx;
        for &id in &sections {
            ctx.output_sections[id.index()].members.par_iter().for_each(|&m| {
                let isec = ctx.input_section(m);
                let file = &ctx.objs[isec.file.index()];
                for rel in isec.rels(file) {
                    if !rel.is_func_call::<E>() {
                        continue;
                    }
                    let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];

                    // A thunk jumps to the start of a symbol, so it can't serve a
                    // branch to an offset from a section symbol, which assemblers
                    // emit for calls to static functions. On RELA targets, such
                    // branches already refer to symbols at their destinations
                    // (see redirect_section_relocations()), but an addend in a
                    // REL instruction can't be moved into a symbol that way.
                    if !E::IS_RELA
                        && sym.ty() == STT_SECTION
                        && isec.rel_addend(rel) != 0
                        && requires_thunk(ctx, isec, rel, sym, false)
                    {
                        error!(
                            "{}: relocation {} against {} needs a range extension thunk, \
                             which can't jump to an offset from a section; recompile \
                             with -ffunction-sections",
                            isec.display(file),
                            rel.type_name::<E>(),
                            sym
                        );
                    }

                    if !sym.is_marked() && requires_thunk(ctx, isec, rel, sym, false) {
                        sym.mark();
                    }
                }
            });
        }
    }

    // Remove symbols from thunks if they don't actually need range extension
    // thunks. The same goes for landing pads.
    for &id in &sections {
        let symbols = &ctx.symbols;
        ctx.output_sections[id.index()].thunks.par_iter_mut().for_each(|thunk| {
            thunk.symbols.retain(|&sym| symbols[sym].is_marked());
            thunk.landing_pads.retain(|&sym| symbols[sym].is_marked());
        });
    }

    for &id in &sections {
        // A thunk's size depends on the distances to its destinations, which
        // may be landing pads in other thunks, so compute sizes while all
        // thunks are in place.
        let offsets: Vec<Vec<u64>> = {
            let ctx: &Context<E> = ctx;
            let osec = &ctx.output_sections[id.index()];
            osec.thunks
                .par_iter()
                .map(|thunk| E::thunk_offsets(ctx, thunk, thunk.addr(osec)))
                .collect()
        };
        let mut thunks = std::mem::take(&mut ctx.output_sections[id.index()].thunks);
        for (thunk, offsets) in thunks.iter_mut().zip(offsets) {
            thunk.offsets = offsets;
        }

        // Recompute section sizes
        let members = &ctx.output_sections[id.index()].members;
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
        osec.hdr.shdr.sh_size.set(offset);
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
pub fn gather_thunk_addresses<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("gather_thunk_addresses");
    let mut sections = executable_sections(ctx);
    sections.sort_by_key(|id| ctx.output_sections[id.index()].hdr.shdr.sh_addr.get());

    let output_sections = &ctx.output_sections;
    let symbols = &mut ctx.symbols;
    for id in sections {
        let osec = &output_sections[id.index()];
        for thunk in &osec.thunks {
            let base = thunk.addr(osec);
            for (&sym, &offset) in thunk.symbols.iter().zip(&thunk.offsets) {
                symbols.add_thunk_addr(sym, base + offset);
            }
        }
    }
}

/// Writes a thunk's stubs into the output section buffer.
pub fn copy_buf<E: Target>(
    ctx: &Context<E>,
    osec: &OutputSection<E>,
    thunk: &Thunk,
    buf: &mut [u8],
) {
    debug_assert_eq!(buf.len(), thunk.size() as usize);
    E::write_thunk(ctx, thunk, thunk.addr(osec), buf);
}
