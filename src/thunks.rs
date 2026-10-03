//! Range-extension thunks.
//!
//! An arm64 b/bl reaches +-128 MiB; code larger than that needs
//! thunks, trampolines placed among the code that branch anywhere
//! within 4 GiB, for the branches that can't reach their targets.
//! ld-prime makes its branch islands only for such branches (one that
//! spans more than 124 MiB of its layout without islands), so an image
//! whose code - from its first code section to the end of its last,
//! __stubs and __objc_stubs included - fits within a branch's reach
//! gets none. need_thunks bounds that span before placement and lays
//! the code out without thunks if it fits.
//!
//! Otherwise every code section is laid out with thunks as mold does:
//! a thunk is placed for each batch of code at D, the farthest point
//! from the batch start that a thunk placed there stays within reach
//! of the whole batch. The subsections up to D thus have their final
//! offsets when the batch is scanned, so a branch to one of them needs
//! an entry only if it really is out of reach; a target beyond D, a
//! branch reach minus a batch away, is assumed to be. Branches to the
//! other code sections and the stubs are judged by bounds on the room
//! the code span takes before and after the section.
//!
//! As in mold, a thunk entry belongs to a *symbol*, not to a
//! relocation: a symbol that some branch of the batch may not reach
//! gets an entry, deduplicated by an atomic mark on the symbol inside
//! the parallel scan, and keeps its mark, getting no second entry, for
//! as long as that entry stays within reach of the batches that follow;
//! gather_thunk_addresses records each symbol's entry addresses so that
//! applying an out-of-range branch just picks the one within reach.
//! mold also trims the entries that turn out unneeded once addresses
//! are final (remove_redundant_thunks) and lays the section out again;
//! ours does not, as the rescan of every branch and the second __TEXT
//! placement (which re-encodes __unwind_info) cost 5% of a debug clang
//! link. The extra entries are dead code.

use rayon::prelude::*;

use crate::chunks::{self, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::input_files::FileId;
use crate::input_sections::InputSectionId;
use crate::macho::{S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS};
use crate::symbol::{NO_IDX, SymbolId};
use crate::target::{RelocClass, Target};
use crate::util::align_to;

/// We create a thunk for each 10 MiB batch of code (mold: 32 MiB).
const BATCH_SIZE: u64 = 10 << 20;

/// We assume that a single thunk is smaller than 1 MiB (mold: 16 MiB).
const MAX_THUNK_SIZE: u64 = 1 << 20;

const THUNK_ALIGN: u64 = 16;

/// A subsection offset that hasn't been assigned yet.
const UNPLACED: u32 = u32::MAX;

/// Whether the link needs range-extension thunks: Some(false) if its
/// code spans no more than a branch reaches, Some(true) if it may span
/// more, and None if only the placement can tell - the span then holds
/// a chunk sized as it is placed (a shared-region image's __stubs come
/// after __unwind_info) or crosses segments. With -no_branch_islands it
/// gets none: a branch out of reach is then a fixup error.
pub fn need_thunks<E: Target>(ctx: &Context<E>) -> Option<bool> {
    if E::THUNK_SIZE == 0 || ctx.args.no_branch_islands {
        return Some(false);
    }
    let Some((first, last)) = code_range(ctx) else {
        return Some(false);
    };
    let segname = ctx.chunk_header(ctx.chunks[first]).segname;
    let span = ctx.chunks[first..=last]
        .iter()
        .map(|&id| chunk_room(ctx, id, segname, false))
        .sum::<Option<u64>>()?;
    Some(span > E::BRANCH_RANGE / 2)
}

/// The span of the placed code: from the start of the first code chunk
/// to the end of the last.
pub fn code_span<E: Target>(ctx: &Context<E>) -> u64 {
    let (lo, hi) = ctx
        .chunks
        .iter()
        .filter(|&&id| is_code(ctx, id))
        .map(|&id| ctx.chunk_header(id))
        .fold((u64::MAX, 0), |(lo, hi), hdr| (lo.min(hdr.addr), hi.max(hdr.addr + hdr.size)));
    hi.saturating_sub(lo)
}

/// Lays out every code section with range-extension thunks, in output
/// order, so that the sections before the one being laid out have
/// their final sizes.
pub fn create_range_extension_thunks<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_range_extension_thunks");
    let Some((first, last)) = code_range(ctx) else {
        return;
    };
    for pos in first..=last {
        if let ChunkId::Output(id) = ctx.chunks[pos]
            && is_code(ctx, ctx.chunks[pos])
        {
            let mut reach = Reach::new(ctx, id, first, pos, last);
            create_thunks(ctx, &mut reach);
        }
    }
}

/// Whether a chunk is code, which gets thunks: a non-empty executable
/// section, in whatever segment.
fn is_code<E: Target>(ctx: &Context<E>, id: ChunkId) -> bool {
    let hdr = ctx.chunk_header(id);
    hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0 && hdr.size > 0
}

/// The positions in the output order of the first and the last code
/// chunk.
fn code_range<E: Target>(ctx: &Context<E>) -> Option<(usize, usize)> {
    let first = ctx.chunks.iter().position(|&id| is_code(ctx, id))?;
    let last = ctx.chunks.iter().rposition(|&id| is_code(ctx, id))?;
    Some((first, last))
}

/// An upper bound on the room a chunk takes in the code span: its size
/// and the padding its alignment may put before it, plus, for a code
/// section still to get its thunks (`grows`), those thunks. None for a
/// chunk outside `segname`, which the segments' placement may put
/// anywhere, or one whose size is only known once it is placed.
fn chunk_room<E: Target>(
    ctx: &Context<E>,
    id: ChunkId,
    segname: &[u8],
    grows: bool,
) -> Option<u64> {
    let hdr = ctx.chunk_header(id);
    if hdr.segname != segname || matches!(id, ChunkId::MachHeader | ChunkId::UnwindInfo) {
        return None;
    }
    let align = 1 << hdr.p2align;
    let mut room = hdr.size + align - 1;
    if let ChunkId::Output(osec) = id
        && grows
        && is_code(ctx, id)
    {
        // Every two batches cover at least BATCH_SIZE bytes, and each
        // thunk may shift the member after it to a new alignment.
        let members = ctx.output_section(osec).members.len() as u64;
        let thunks = members.min(2 * hdr.size.div_ceil(BATCH_SIZE) + 1);
        room += thunks * (MAX_THUNK_SIZE + THUNK_ALIGN + align);
    }
    Some(room)
}

/// Where a code chunk lies relative to the section being laid out:
/// within the code span before or after it, or outside the span.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Before,
    After,
    Outside,
}

/// What a branch from the code section being laid out can reach.
struct Reach {
    osec: OutputSectionId,
    /// The side of each output section, and of the stubs.
    sides: Vec<Side>,
    stubs: Side,
    objc_stubs: Side,
    /// Upper bounds on the room the code span takes before the section
    /// and after it, if known.
    before: Option<u64>,
    after: Option<u64>,
    /// Whether all of the code before the current batch's end, and all
    /// of it after the batch's start, is within reach of the batch.
    backward: bool,
    forward: bool,
}

impl Reach {
    fn new<E: Target>(
        ctx: &Context<E>,
        osec: OutputSectionId,
        first: usize,
        pos: usize,
        last: usize,
    ) -> Self {
        // The span is the code of the section's own segment: the
        // segments' placement may put another's code anywhere.
        let hdr = ctx.chunk_header(ctx.chunks[pos]);
        let in_span = |i: &usize| {
            let id = ctx.chunks[*i];
            is_code(ctx, id) && ctx.chunk_header(id).segname == hdr.segname
        };
        let first = (first..=pos).find(in_span).unwrap();
        let last = (pos..=last).rev().find(in_span).unwrap();

        let side = |i: usize| match i {
            _ if i < first || last < i => Side::Outside,
            _ if i < pos => Side::Before,
            _ => Side::After,
        };
        let mut sides = vec![Side::Outside; ctx.output_sections.len()];
        let (mut stubs, mut objc_stubs) = (Side::Outside, Side::Outside);
        for (i, &id) in ctx.chunks.iter().enumerate() {
            match id {
                ChunkId::Output(id) => sides[id.index()] = side(i),
                ChunkId::Stubs => stubs = side(i),
                ChunkId::ObjcStubs => objc_stubs = side(i),
                _ => {}
            }
        }

        let room = |ids: &[ChunkId], grows| {
            ids.iter().map(|&id| chunk_room(ctx, id, hdr.segname, grows)).sum::<Option<u64>>()
        };
        let before = room(&ctx.chunks[first..pos], false).map(|room| room + (1 << hdr.p2align) - 1);
        let after = room(&ctx.chunks[pos + 1..=last], true);
        Self { osec, sides, stubs, objc_stubs, before, after, backward: false, forward: false }
    }
}

/// Lays out one code section with range-extension thunks, as mold's
/// create_range_extension_thunks does. The layout proceeds with four
/// member indices that only move forward, A <= B <= C <= D: [B, C) is
/// the current batch, D the first member not yet placed - the thunk
/// for the batch goes there, the farthest a thunk stays within reach
/// of B - and A the first member within reach of C, before which
/// thunks are out of the batch's reach.
fn create_thunks<E: Target>(ctx: &mut Context<E>, reach: &mut Reach) {
    let id = reach.osec;
    let members = std::mem::take(&mut ctx.output_sections[id.index()].members);
    for &m in &members {
        ctx.isecs[m].offset = UNPLACED;
    }

    let distance = E::BRANCH_RANGE / 2;
    let n = members.len();
    let mut thunks: Vec<chunks::Thunk> = Vec::new();
    let (mut a, mut b, mut d) = (0, 0, 0);
    let mut offset = 0;
    // The first thunk still within reach of the current batch.
    let mut t = 0;

    while b < n {
        // Move D forward as far as a thunk there stays within reach of B.
        while d < n {
            let isec = &ctx.isecs[members[d]];
            let start = isec.align_offset(offset);
            let end = start + isec.size as u64;
            if b != d
                && align_to(end, THUNK_ALIGN) + MAX_THUNK_SIZE
                    > ctx.isecs[members[b]].offset as u64 + distance
            {
                break;
            }
            ctx.isecs[members[d]].offset = start as u32;
            offset = end;
            d += 1;
        }

        let c = batch_end(ctx, &members, b, d);

        // Unmark the symbols of the thunks out of reach of C, so that
        // they can take new entries.
        let c_offset = if c == d { offset } else { ctx.isecs[members[c]].offset as u64 };
        a += members[a..b]
            .partition_point(|&m| (ctx.isecs[m].offset as u64) < c_offset.saturating_sub(distance));
        while t < thunks.len() && thunks[t].offset < ctx.isecs[members[a]].offset as u64 {
            for &sym in &thunks[t].syms {
                ctx.symbols[sym].unmark();
            }
            t += 1;
        }

        // The code after the section is within reach of the batch if
        // the section's end is known closely enough: once all of it is
        // placed, only the thunks of the batches left, this one's
        // included, still go in, at its end.
        let b_offset = ctx.isecs[members[b]].offset as u64;
        reach.backward = reach.before.is_some_and(|before| before + c_offset <= distance);
        reach.forward = d == n
            && reach.after.is_some_and(|after| {
                let batches = batches_left(ctx, &members, b);
                offset + batches * (MAX_THUNK_SIZE + THUNK_ALIGN) + after - b_offset <= distance
            });

        // Create a thunk for the batch and place it at D.
        let syms = scan_batch(ctx, &members[b..c], reach);
        if !syms.is_empty() {
            offset = align_to(offset, THUNK_ALIGN);
            let size = syms.len() as u64 * E::THUNK_SIZE;
            debug_assert!(size <= MAX_THUNK_SIZE);
            thunks.push(chunks::Thunk { offset, syms });
            offset += size;
        }
        b = c;
    }

    // Marks of the thunks still in reach at the end are cleared too.
    for thunk in &thunks[t..] {
        for &sym in &thunk.syms {
            ctx.symbols[sym].unmark();
        }
    }
    let osec = &mut ctx.output_sections[id.index()];
    osec.hdr.size = offset;
    osec.thunks = thunks;
    osec.members = members;
}

/// The end of the batch that starts at member `b`: it takes the members
/// that end within BATCH_SIZE bytes of it, one at least, and none at D
/// or beyond.
fn batch_end<E: Target>(ctx: &Context<E>, members: &[InputSectionId], b: usize, d: usize) -> usize {
    let limit = ctx.isecs[members[b]].offset as u64 + BATCH_SIZE;
    b + 1
        + members[b + 1..d].partition_point(|&m| {
            let isec = &ctx.isecs[m];
            (isec.offset as u64 + isec.size as u64) < limit
        })
}

/// The number of batches from member `b` to the end of a section whose
/// members are all placed.
fn batches_left<E: Target>(ctx: &Context<E>, members: &[InputSectionId], b: usize) -> u64 {
    let mut batches = 0;
    let mut i = b;
    while i < members.len() {
        i = batch_end(ctx, members, i, members.len());
        batches += 1;
    }
    batches
}

/// Scans `batch`'s branch relocations in parallel and returns the
/// symbols that need an entry in the batch's thunk: those whose target
/// may be out of reach and that no thunk still within reach covers (a
/// symbol claims its entry with mark()). A thunk entry jumps to its
/// symbol, so a branch with an addend never gets one; applying it
/// reports it if it is out of reach. mold scans each batch's members
/// with par_iter and dedups with the symbol's atomic mark the same way.
fn scan_batch<E: Target>(
    ctx: &Context<E>,
    batch: &[InputSectionId],
    reach: &Reach,
) -> Vec<SymbolId> {
    let mut syms: Vec<SymbolId> = batch
        .par_iter()
        .fold(Vec::new, |mut syms, &id| {
            let isec = &ctx.isecs[id];
            let obj = isec.file as usize;
            let rels = &ctx.objs[obj].relocs[isec.rel_offset as usize..][..isec.nrels as usize];
            for rel in rels {
                if E::classify_reloc(rel.r_type) != RelocClass::Branch || rel.addend != 0 {
                    continue;
                }
                let Some(sym) = ctx.reloc_target_sym(obj, rel) else {
                    continue;
                };
                let p = isec.offset as u64 + rel.offset as u64;
                if needs_thunk(ctx, reach, id, p, sym) && ctx.symbols[sym].mark() {
                    syms.push(sym);
                }
            }
            syms
        })
        .reduce(Vec::new, |mut syms, mut other| {
            syms.append(&mut other);
            syms
        });
    // Deterministic entry order regardless of which thread claimed
    // each symbol.
    syms.par_sort_unstable();
    syms
}

/// Whether a branch at offset `p` of the section being laid out, in
/// subsection `isec`, may not reach `sym`, where it goes: its
/// subsection, its stub (an import, a weak definition that may be
/// interposed, or a shim for a branch from 4 GiB away; see
/// branch_shims) or its _objc_msgSend stub.
fn needs_thunk<E: Target>(
    ctx: &Context<E>,
    reach: &Reach,
    isec: InputSectionId,
    p: u64,
    id: SymbolId,
) -> bool {
    let sym = &ctx.symbols[id];
    let aux = ctx.sym_aux(id);
    let side = match sym.file() {
        _ if aux.stub_idx != NO_IDX && ctx.is_interposable(id) => reach.stubs,
        _ if ctx.has_branch_shim(id) && crate::branch_shims::is_far(ctx, isec as usize, id) => {
            reach.stubs
        }
        Some(FileId::Dylib(_)) if aux.stub_idx != NO_IDX => reach.stubs,
        Some(FileId::Obj(_)) => match sym.input_section() {
            Some(target) => {
                let target = &ctx.isecs[ctx.resolve_isec(target as usize)];
                match target.output_section() {
                    Some(ChunkId::Output(osec)) if osec == reach.osec => {
                        if target.offset == UNPLACED {
                            Side::After
                        } else {
                            let distance = (E::BRANCH_RANGE / 2) as i64;
                            let t = target.offset as u64 + sym.value;
                            return !(-distance..distance).contains(&(t.wrapping_sub(p) as i64));
                        }
                    }
                    Some(ChunkId::Output(osec)) => reach.sides[osec.index()],
                    _ => Side::Outside,
                }
            }
            None if aux.objc_stub_idx != NO_IDX => reach.objc_stubs,
            None => Side::Outside,
        },
        // A DTrace symbol, at address 0: a probe site needs no thunk,
        // being no branch in the output (see dtrace).
        None => return false,
        _ => Side::Outside,
    };
    match side {
        Side::Before => !reach.backward,
        Side::After => !reach.forward,
        Side::Outside => true,
    }
}

/// Records every thunk entry's address on its symbol (SymAux::
/// thunk_addrs), in address order, so that applying an out-of-range
/// branch can pick the entry within reach. mold's
/// gather_thunk_addresses.
pub fn gather_thunk_addresses<E: Target>(ctx: &mut Context<E>) {
    // The sections are read while the symbols' aux data is written, so
    // the borrows are split.
    let chunks = &ctx.chunks;
    let output_sections = &ctx.output_sections;
    let symtab = &mut ctx.symbols;
    let sym_aux = &mut ctx.sym_aux;
    for &id in chunks {
        let ChunkId::Output(id) = id else { continue };
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

/// The local symbols naming the thunk entries, as (address, section
/// ordinal, name). ld-prime lists each branch island among the locals,
/// named after its target: "<target>.island" for the target's first
/// and "<target>.island<n>" for its n-th, in address order, which is
/// the order gather_thunk_addresses recorded them in.
pub fn island_symbols<E: Target>(ctx: &Context<E>) -> Vec<(u64, u8, &'static [u8])> {
    use std::io::Write;

    let mut syms = Vec::new();
    for osec in &ctx.output_sections {
        let hdr = &osec.hdr;
        syms.par_extend(osec.thunks.par_iter().flat_map_iter(|thunk| {
            let addr = |i: usize| hdr.addr + thunk.offset + i as u64 * E::THUNK_SIZE;
            // A thunk can have many thousands of entries; their names
            // share one allocation.
            let mut buf = Vec::new();
            let mut ends = Vec::with_capacity(thunk.syms.len());
            for (i, &sym) in thunk.syms.iter().enumerate() {
                let addrs = &ctx.sym_aux(sym).thunk_addrs;
                let n = addrs.iter().position(|&a| a == addr(i)).unwrap() + 1;
                buf.extend_from_slice(ctx.symbols[sym].name());
                buf.extend_from_slice(b".island");
                if n > 1 {
                    write!(buf, "{n}").unwrap();
                }
                ends.push(buf.len());
            }
            let buf = crate::util::leak_bytes(buf);
            let mut start = 0;
            ends.into_iter().enumerate().map(move |(i, end)| {
                let name = &buf[start..end];
                start = end;
                (addr(i), hdr.n_sect, name)
            })
        }));
    }
    syms
}

/// The address of a thunk entry for `sym` that a branch at `pc` can
/// reach, if it has one.
#[inline]
pub fn reachable_thunk_addr<E: Target>(ctx: &Context<E>, sym: SymbolId, pc: u64) -> Option<u64> {
    let range = (E::BRANCH_RANGE / 2) as i64;
    ctx.sym_aux(sym).thunk_addrs.iter().copied().find(|&t| {
        let d = t.wrapping_sub(pc) as i64;
        (-range..range).contains(&d)
    })
}
