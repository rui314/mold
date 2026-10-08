//! Identical code folding.
//!
//! ld64 deduplicates identical functions (ld-prime at -O1 and up or
//! with -deduplicate, this linker unless -no_deduplicate); mold's ICF
//! does the same for ELF. Two subsections can share one copy when their
//! bytes, relocations and unwind information are all identical, *and*
//! folding cannot be observed. ld-prime folds the functions of
//! __TEXT,__text that no one can compare the addresses of: those the
//! compiler marked .weak_def_can_be_hidden (C++ inline functions with
//! unnamed_addr) that the link hid and Swift functions, whether or not
//! their address is taken, and any other unexported one whose address
//! is never taken - mold's --icf=safe.
//!
//! The algorithm follows mold: every candidate gets a hash of its
//! literal content, and a few refinement rounds rehash each candidate
//! with the previous-round hashes of its relocation targets, so the
//! hash comes to describe the whole reachable shape. Groups with equal
//! final hashes are then verified structurally and folded onto their
//! first member. That folds functions that call each other in a cycle
//! (two instances of a mutually recursive sort) as a group, which
//! ld-prime, folding only callers of functions already found equal,
//! leaves apart; either is correct, and ours is the cheaper to find.

use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use portable_atomic::AtomicU64;
use rayon::prelude::*;

use crate::chunks::eh_frame::lsda_pos;
use crate::chunks::unwind_info::{function_lsda, function_personality};
use crate::context::Context;
use crate::input_files::{Fde, FileId, ObjectFile, subsec_name_rank};
use crate::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;
use crate::util::siphash::SipHash13_128;

/// A stable identifier for what a relocation edge points at.
#[derive(Hash, PartialEq, Eq, Clone, Copy)]
enum Edge {
    /// A candidate subsection, compared by its evolving hash.
    Candidate(usize),
    /// Anything else, compared by identity.
    Isec(usize, u64),
    Sym(usize),
}

/// A 128-bit digest. Ordered so equivalence classes group by sorting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Digest {
    hi: u64,
    lo: u64,
}

impl Digest {
    #[inline]
    fn update(self, hasher: &mut SipHash13_128) {
        hasher.update_u64(self.hi);
        hasher.update_u64(self.lo);
    }

    fn from_ne_bytes(bytes: [u8; 16]) -> Self {
        Self {
            hi: u64::from_ne_bytes(bytes[..8].try_into().unwrap()),
            lo: u64::from_ne_bytes(bytes[8..].try_into().unwrap()),
        }
    }
}

#[inline(always)]
fn finish_digest(hasher: SipHash13_128) -> Digest {
    let mut bytes = [0; 16];
    hasher.finish(&mut bytes);
    Digest::from_ne_bytes(bytes)
}

/// A lock-free digest -> leader map, ported from mold. It counts
/// the distinct digests in a propagation round and, as a side effect,
/// records the lowest-index candidate for each digest as its class
/// leader - so counting classes and electing leaders is one pass, no
/// sort. Slots are stamped with the round they were written in and the
/// table is reused across rounds without clearing (a slot from an
/// earlier round reads as vacant).
struct DigestMap {
    round: u64,
    mask: usize,
    slots: Vec<DigestSlot>,
}

#[derive(Default)]
struct DigestSlot {
    hi: AtomicU64,
    lo: AtomicU64,
    leader: AtomicU32,
}

impl DigestMap {
    fn new(n: usize) -> Self {
        let len = n.saturating_mul(2).next_power_of_two();
        Self { round: 1, mask: len - 1, slots: (0..len).map(|_| DigestSlot::default()).collect() }
    }

    fn next_round(&mut self) {
        self.round += 1;
    }

    /// Inserts (digest -> candidate), keeping the lowest candidate index
    /// as the leader. Returns true if the digest was not already present
    /// this round.
    fn insert(&self, digest: Digest, cand: u32) -> bool {
        const BUSY_BIT: u64 = 1 << 48;
        let tag = digest.hi >> 16;
        let value = (self.round << 49) | tag;
        let mut i = digest.hi as usize & self.mask;
        loop {
            let slot = &self.slots[i];
            let mut x = slot.hi.load(Ordering::Acquire);
            while x >> 49 != self.round {
                match slot.hi.compare_exchange_weak(
                    x,
                    value | BUSY_BIT,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        slot.lo.store(digest.lo, Ordering::Relaxed);
                        slot.leader.store(cand, Ordering::Relaxed);
                        slot.hi.store(value, Ordering::Release);
                        return true;
                    }
                    Err(actual) => x = actual,
                }
            }
            if x & 0xffff_ffff_ffff != tag {
                i = (i + 1) & self.mask;
                continue;
            }
            while x & BUSY_BIT != 0 {
                std::hint::spin_loop();
                x = slot.hi.load(Ordering::Acquire);
            }
            if slot.lo.load(Ordering::Relaxed) != digest.lo {
                i = (i + 1) & self.mask;
                continue;
            }
            // Already present; keep the lowest candidate index as leader.
            let mut cur = slot.leader.load(Ordering::Relaxed);
            while cand < cur {
                match slot.leader.compare_exchange_weak(
                    cur,
                    cand,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => cur = actual,
                }
            }
            return false;
        }
    }

    fn find(&self, digest: Digest) -> u32 {
        let value = (self.round << 49) | (digest.hi >> 16);
        let mut i = digest.hi as usize & self.mask;
        loop {
            let slot = &self.slots[i];
            if slot.hi.load(Ordering::Relaxed) == value
                && slot.lo.load(Ordering::Relaxed) == digest.lo
            {
                return slot.leader.load(Ordering::Relaxed);
            }
            i = (i + 1) & self.mask;
        }
    }
}

/// Whether each subsection is a function whose address is insignificant
/// by declaration, which ld-prime folds even where the address is taken
/// or exported: one the link auto-hid or a Swift function (see below).
fn insignificant_sections<E: Target>(ctx: &Context<E>) -> Vec<bool> {
    let flags: Vec<AtomicBool> = (0..ctx.isecs.len()).map(|_| AtomicBool::new(false)).collect();
    ctx.objs.par_iter().enumerate().for_each(|(i, obj)| {
        if obj.is_alive {
            mark_auto_hidden(ctx, i, obj, &flags);
            mark_swift_functions(ctx, i, obj, &flags);
        }
    });
    flags.into_iter().map(AtomicBool::into_inner).collect()
}

/// Marks the functions of object `i` the link auto-hid: a global, not
/// private extern, whose definition carries N_WEAK_DEF | N_WEAK_REF
/// (.weak_def_can_be_hidden), which clang gives an inline function
/// with unnamed_addr. A private extern one is hidden anyway and is
/// folded as any other hidden function.
fn mark_auto_hidden<E: Target>(ctx: &Context<E>, i: usize, obj: &ObjectFile, flags: &[AtomicBool]) {
    let r = obj.global_range();
    for (msym, &sym_id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
        let sym = &ctx.symbols[sym_id];
        if !msym.is_stab()
            && msym.n_type & N_PEXT == 0
            && msym.desc & (N_WEAK_DEF | N_WEAK_REF) == N_WEAK_DEF | N_WEAK_REF
            && sym.is_private_extern()
            && sym.file() == Some(FileId::Obj(i as u32))
            && let Some(isec) = sym.input_section()
        {
            flags[isec as usize].store(true, Ordering::Relaxed);
        }
    }
}

/// Marks the Swift functions of object `i`: those whose subsection
/// ld-prime names (see Context::subsec_label) by a symbol Swift mangled,
/// "_$s...". Swift promises no function an address of its own, so
/// ld-prime folds one even where its address is taken (a coroutine's
/// resume function, a value witness) or it is exported; it goes by the
/// subsection's name only, so another label at the function's start that
/// outranks the Swift one keeps it apart. An @objc thunk ("...To"), which
/// an Objective-C method list names, is no such function: ld-prime folds
/// one only as it folds a C function (SwiftyMarkdown's unimplemented
/// init()'s thunk stays apart from the identical init() it calls for).
fn mark_swift_functions<E: Target>(
    ctx: &Context<E>,
    i: usize,
    obj: &ObjectFile,
    flags: &[AtomicBool],
) {
    // Only a function whose address is taken needs to be one; most
    // subsections Swift names so are its metadata, of other sections.
    let is_swift = |name: &[u8]| name.starts_with(b"_$s") && !name.ends_with(b"To");
    let wanted =
        |isec: usize| ctx.isecs[isec].is_address_taken() && is_text_function(ctx, &ctx.isecs[isec]);
    for (isec, sym) in subsec_names(ctx, i, obj, wanted) {
        if is_swift(ctx.symbols[sym].name()) {
            flags[isec as usize].store(true, Ordering::Relaxed);
        }
    }
}

/// The labels at the start of object `i`'s subsections that `wanted`
/// takes, but for an exported one another object's definition won:
/// (subsection, rank of the label as ld-prime picks the one naming the
/// subsection, name, symbol). An alternate entry point (N_ALT_ENTRY)
/// names no subsection but where no other label does. (An object's
/// mach_syms are mostly of undefined symbols, which are passed over
/// before their symbols are looked at.)
fn start_labels<'a, E: Target>(
    ctx: &'a Context<E>,
    i: usize,
    obj: &'a ObjectFile,
    wanted: impl Fn(usize) -> bool + 'a,
) -> impl Iterator<Item = (u32, u8, &'static [u8], SymbolId)> + 'a {
    obj.mach_syms.iter().zip(&obj.symbols).filter_map(move |(msym, &id)| {
        if msym.is_stab() || msym.ty() != N_SECT {
            return None;
        }
        let sym = &ctx.symbols[id];
        let isec = sym.input_section()?;
        if sym.value != 0 || sym.file() != Some(FileId::Obj(i as u32)) || !wanted(isec as usize) {
            return None;
        }
        let entry = (msym.desc & N_ALT_ENTRY == 0) as u8;
        Some((isec, entry << 4 | subsec_name_rank(msym, sym.name()), sym.name(), id))
    })
}

/// The symbol naming each of object `i`'s subsections that a label
/// starts (see Context::subsec_label) and `wanted` takes, by
/// subsection.
fn subsec_names<E: Target>(
    ctx: &Context<E>,
    i: usize,
    obj: &ObjectFile,
    wanted: impl Fn(usize) -> bool,
) -> Vec<(u32, SymbolId)> {
    let mut labels: Vec<_> = start_labels(ctx, i, obj, wanted).collect();
    labels.sort_unstable();
    labels.chunk_by(|a, b| a.0 == b.0).map(|run| (run[0].0, run[run.len() - 1].3)).collect()
}

/// The symbols naming the subsections of folded functions: a folded
/// function keeps its name, at the function it folded into. `folded`
/// pairs each folded subsection with the one it folded into.
fn folded_subsec_names<E: Target>(
    ctx: &Context<E>,
    folded: &[(usize, usize)],
) -> hashbrown::HashSet<SymbolId> {
    let mut involved = vec![false; ctx.isecs.len()];
    let mut objs = Vec::new();
    for &(member, _) in folded {
        involved[member] = true;
        objs.push(ctx.isecs[member].file);
    }
    objs.sort_unstable();
    objs.dedup();
    objs.par_iter()
        .flat_map_iter(|&i| subsec_names(ctx, i as usize, &ctx.objs[i as usize], |j| involved[j]))
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(_, id)| id)
        .collect()
}

/// Whether a subsection is a function of __TEXT,__text (ld64 folds no
/// other section) in the output.
fn is_text_function<E: Target>(ctx: &Context<E>, isec: &InputSection) -> bool {
    let hdr = ctx.hdr_of(isec);
    isec.is_emitted() && hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__text")
}

/// Whether each subsection is a function -keep_duplicate or
/// -keep_duplicates_list names, a local one too, or one with a DTrace
/// probe site, which is neither folded nor folded into.
fn kept_sections<E: Target>(ctx: &Context<E>) -> Vec<bool> {
    let kept: Vec<AtomicBool> = (0..ctx.isecs.len()).map(|_| AtomicBool::new(false)).collect();
    let keep = &ctx.args.keep_duplicates;
    if !keep.is_empty() {
        ctx.symbols.syms.par_iter().for_each(|sym| {
            if matches!(sym.file(), Some(FileId::Obj(_)))
                && let Some(isec) = sym.input_section()
                && keep.find(sym.name()) != -1
            {
                kept[isec as usize].store(true, Ordering::Relaxed);
            }
        });
    }
    let mut kept: Vec<bool> = kept.into_iter().map(AtomicBool::into_inner).collect();
    for &isec in ctx.dof_sections.iter().flat_map(|dof| &dof.sites) {
        kept[isec as usize] = true;
    }
    kept
}

/// Candidates: the non-empty functions whose addresses no one can
/// compare, but those -keep_duplicate names.
fn is_candidate<E: Target>(
    ctx: &Context<E>,
    insignificant: &[bool],
    kept: &[bool],
    id: usize,
) -> bool {
    let isec = &ctx.isecs[id];
    is_text_function(ctx, isec)
        && isec.size != 0
        && (insignificant[id] || !isec.is_address_taken())
        && !kept[id]
}

/// -verbose_deduplicate: ld-prime's summary of the functions folded
/// away, out of every function of __TEXT,__text, given when it folds
/// any. `total` is the functions' number and size before folding.
fn report_folds<E: Target>(
    ctx: &Context<E>,
    candidates: &[usize],
    leaders: &[u32],
    total: (usize, u64),
) {
    let folded = || (0..leaders.len()).filter(|&i| leaders[i] as usize != i);
    let count = folded().count();
    if count == 0 {
        return;
    }
    let size: u64 = folded().map(|i| ctx.isecs[candidates[i]].size as u64).sum();
    let percent = size as f64 * 100.0 / total.1 as f64;
    crate::error::notice(format_args!(
        "code deduplicated functions {count} (size: {size}) out of total {} (size: {}) ({percent:.2}% size reduction)",
        total.0, total.1
    ));
}

/// What relocation `rel` of object `obj` points at, and its addend. A
/// symbol's offset into a candidate moves into the addend, so that
/// references to the same place in equal candidates compare equal.
fn edge_of<E: Target>(
    ctx: &Context<E>,
    cand_index: &[usize],
    obj: usize,
    rel: &Reloc,
) -> (Edge, i64) {
    match rel.target() {
        RelocTarget::Sym(idx) => {
            let sym_id = ctx.objs[obj].symbols[idx as usize];
            let sym = &ctx.symbols[sym_id];
            if let (Some(FileId::Obj(_)), Some(isec)) = (sym.file(), sym.input_section()) {
                let isec = ctx.resolve_isec(isec as usize);
                if cand_index[isec] != usize::MAX {
                    return (Edge::Candidate(cand_index[isec]), rel.addend + sym.value as i64);
                }
                return (Edge::Isec(isec, sym.value), rel.addend);
            }
            (Edge::Sym(sym_id as usize), rel.addend)
        }
        RelocTarget::Section(isec) => {
            let isec = ctx.resolve_isec(isec as usize);
            if cand_index[isec] != usize::MAX {
                return (Edge::Candidate(cand_index[isec]), rel.addend);
            }
            (Edge::Isec(isec, 0), rel.addend)
        }
    }
}

// A fixed key keeps the link reproducible; the digest is used only to
// group, never emitted.
const KEY: [u8; 16] = *b"mold-macho-icf!!";

/// The base digest of a candidate: its bytes and the non-candidate
/// parts of its edges, hashed with SipHash13-128 exactly as mold's
/// compute_digest does (candidate edges are mixed in during the rounds
/// instead). Every candidate is in __TEXT,__text, so unlike mold's the
/// section flags are left out: ld-prime folds functions whose sections
/// differ only in attributes such as S_ATTR_NO_DEAD_STRIP.
fn compute_digest<E: Target>(ctx: &Context<E>, cand_index: &[usize], id: usize) -> Digest {
    let isec = &ctx.isecs[id];
    let mut h = SipHash13_128::new(&KEY);
    h.update(&isec.size.to_ne_bytes());
    h.update(&isec.data().len().to_ne_bytes());
    h.update(isec.data());
    for rel in ctx.isec_relocs(id) {
        h.update(&rel.offset.to_ne_bytes());
        h.update(&rel.ty.to_ne_bytes());
        h.update(&[rel.size, rel.is_pcrel as u8, rel.is_subtracted as u8]);
        let (edge, addend) = edge_of(ctx, cand_index, isec.file as usize, rel);
        h.update(&addend.to_ne_bytes());
        // A candidate edge contributes nothing to the base; the
        // rounds fold in the target's evolving digest.
        match edge {
            Edge::Candidate(_) => h.update(b"c"),
            Edge::Isec(i, v) => {
                h.update(b"i");
                h.update(&i.to_ne_bytes());
                h.update(&v.to_ne_bytes());
            }
            Edge::Sym(s) => {
                h.update(b"s");
                h.update(&s.to_ne_bytes());
            }
        }
    }
    // A function's unwind information is part of its identity, as mold
    // hashes a section's CIEs and FDEs: two functions fold only if they
    // are unwound alike, by records at the same offsets with the same
    // length, compact encoding, personality, LSDA and DWARF CFI, so that
    // the survivor's describes every caller's frames. (ld-prime compares
    // only the personality and the LSDA, and folds a function that has
    // unwind information into one that has none.) A record with an FDE
    // is in DWARF mode whether its object said so or the linker made it.
    let recs = isec.unwind_offset as usize..(isec.unwind_offset + isec.nunwind) as usize;
    h.update(&isec.nunwind.to_ne_bytes());
    for rec in &ctx.unwind_records[recs] {
        let encoding = if rec.fde().is_some() { E::UNWIND_MODE_DWARF } else { rec.encoding };
        h.update(&rec.input_offset.to_ne_bytes());
        h.update(&rec.code_len.to_ne_bytes());
        h.update(&encoding.to_ne_bytes());
        let personality = function_personality(ctx, rec);
        h.update(&personality.map_or(u64::MAX, |p| p as u64).to_ne_bytes());
        let (lsda, off) = function_lsda(ctx, rec)
            .map_or((usize::MAX, 0), |(lsda, off)| (ctx.resolve_isec(lsda), off));
        h.update(&lsda.to_ne_bytes());
        h.update(&off.to_ne_bytes());
        if let Some(fde) = rec.fde() {
            hash_dwarf_cfi(ctx, &mut h, &ctx.fdes[fde]);
        }
    }
    finish_digest(h)
}

/// Hashes the CIE and the FDE that unwind a function in DWARF mode, with
/// the fields that depend on where the records or their targets are -
/// the CIE pointer, the function's address and the personality and LSDA
/// pointers, which compute_digest hashes as targets - zeroed. Like mold,
/// it leaves out the length and the trailing DW_CFA_nops, which pad a
/// record to the section's alignment without meaning anything.
fn hash_dwarf_cfi<E: Target>(ctx: &Context<E>, h: &mut SipHash13_128, fde: &Fde) {
    let hash_record = |h: &mut SipHash13_128, body: &[u8]| {
        let len = body.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        h.update(&len.to_ne_bytes());
        h.update(&body[..len]);
    };
    let cie = &ctx.cies[fde.cie as usize];
    let mut buf = cie.data.to_vec();
    if cie.personality.is_some() {
        let pos = cie.personality_offset as usize;
        buf[pos..pos + 4].fill(0);
    }
    hash_record(h, &buf[4..]);

    let pc_size = cie.pc_size();
    let mut buf = fde.data.to_vec();
    buf[4..8 + pc_size].fill(0);
    if fde.lsda.is_some() {
        let pos = lsda_pos(fde.data, pc_size);
        buf[pos..pos + cie.lsda_size()].fill(0);
    }
    hash_record(h, &buf[4..]);
}

/// Calls `f` with the candidate index of each candidate that candidate
/// `id` references.
fn for_each_edge<E: Target>(
    ctx: &Context<E>,
    cand_index: &[usize],
    id: usize,
    mut f: impl FnMut(u32),
) {
    let obj = ctx.isecs[id].file as usize;
    for rel in ctx.isec_relocs(id) {
        if let Edge::Candidate(c) = edge_of(ctx, cand_index, obj, rel).0 {
            f(c as u32);
        }
    }
}

/// The candidate edges in CSR form: candidate i's edges are
/// values[indices[i]..indices[i + 1]]. The propagation loop is the hot
/// path; a single contiguous array keeps it cache-friendly, where a
/// Vec<Vec<u32>> would chase one heap allocation per candidate.
struct Edges {
    values: Vec<u32>,
    indices: Vec<u32>,
}

/// Builds the graph whose vertices are the candidates and whose edges
/// are their references to other candidates, as mold's gather_edges.
fn gather_edges<E: Target>(ctx: &Context<E>, cand_index: &[usize], candidates: &[usize]) -> Edges {
    // Count the outgoing edges of each vertex and turn the counts into
    // starting indices with a prefix sum. The extra entry at the end
    // makes indices[i + 1] valid for every vertex.
    let mut indices = vec![0u32; candidates.len() + 1];
    indices[..candidates.len()]
        .par_iter_mut()
        .zip(candidates)
        .for_each(|(count, &id)| for_each_edge(ctx, cand_index, id, |_| *count += 1));
    let mut sum = 0u32;
    for count in &mut indices {
        let next = sum.checked_add(*count).expect("too many ICF edges");
        *count = sum;
        sum = next;
    }

    let mut values = vec![0; *indices.last().unwrap() as usize];
    // Split at vertex boundaries so each task owns a disjoint slice of
    // the edge array.
    rayon::iter::split((0..candidates.len(), values.as_mut_slice()), |(range, out)| {
        if range.len() <= 1 {
            return ((range, out), None);
        }
        let mid = range.start + range.len() / 2;
        let (left, right) = out.split_at_mut((indices[mid] - indices[range.start]) as usize);
        ((range.start..mid, left), Some((mid..range.end, right)))
    })
    .for_each(|(range, out)| {
        let mut i = 0;
        for vertex in range {
            for_each_edge(ctx, cand_index, candidates[vertex], |edge| {
                out[i] = edge;
                i += 1;
            });
        }
    });

    Edges { values, indices }
}

/// Computes the next-round digest of each vertex by hashing its current
/// digest and the current digests of the vertices it refers to, so its
/// nth-round digest is a hash of its unfolding into a tree of depth n.
/// Content is hashed exactly once, in compute_digest; a round mixes
/// only fixed-size digests, through SipHash's aligned 8-byte
/// fast path, so a round is cheap however many rounds are needed.
fn propagate(cur: &mut Vec<Digest>, next: &mut Vec<Digest>, edges: &Edges) {
    next.par_iter_mut().enumerate().for_each(|(i, out)| {
        let mut h = SipHash13_128::new(&KEY);
        cur[i].update(&mut h);
        let begin = edges.indices[i] as usize;
        let end = edges.indices[i + 1] as usize;
        for &j in &edges.values[begin..end] {
            cur[j as usize].update(&mut h);
        }
        *out = finish_digest(h);
    });
    std::mem::swap(cur, next);
}

/// Counts the distinct digests, electing each one's leader (its lowest
/// candidate index) as a side effect.
fn count_num_classes(digests: &[Digest], map: &mut DigestMap) -> usize {
    map.next_round();
    (0..digests.len()).into_par_iter().map(|i| map.insert(digests[i], i as u32) as usize).sum()
}

/// Debug builds re-verify that each candidate is byte-for-byte equal to
/// its leader.
#[cfg(debug_assertions)]
fn verify_leaders<E: Target>(
    ctx: &Context<E>,
    cand_index: &[usize],
    candidates: &[usize],
    leaders: &[u32],
) {
    // Two members may reference different candidates of one class: those
    // were folded together, so an edge names the class's leader.
    let edge = |obj: usize, r: &Reloc| match edge_of(ctx, cand_index, obj, r) {
        (Edge::Candidate(c), addend) => (Edge::Candidate(leaders[c] as usize), addend),
        e => e,
    };
    let equal = |a: usize, b: usize| -> bool {
        let (x, y) = (&ctx.isecs[a], &ctx.isecs[b]);
        let (xr, yr) = (ctx.isec_relocs(a), ctx.isec_relocs(b));
        x.data() == y.data()
            && xr.len() == yr.len()
            && xr.iter().zip(yr).all(|(r, s)| {
                r.offset == s.offset
                    && r.ty == s.ty
                    && r.size == s.size
                    && r.is_pcrel == s.is_pcrel
                    && edge(x.file as usize, r) == edge(y.file as usize, s)
            })
    };
    for (i, &l) in leaders.iter().enumerate() {
        debug_assert!(equal(candidates[i], candidates[l as usize]));
    }
}

pub fn icf_sections<E: Target>(ctx: &mut Context<E>) {
    let _t_all = ctx.timer("icf");
    let mut t = ctx.timer("icf-prep");
    let insignificant = insignificant_sections(ctx);
    let kept = kept_sections(ctx);
    let candidates: Vec<usize> = (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| is_candidate(ctx, &insignificant, &kept, i))
        .collect();
    if candidates.len() < 2 {
        return;
    }
    let mut cand_index = vec![usize::MAX; ctx.isecs.len()];
    for (i, &id) in candidates.iter().enumerate() {
        cand_index[id] = i;
    }
    let functions = ctx.args.verbose_deduplicate.then(|| {
        let functions = ctx.isecs.iter().filter(|isec| is_text_function(ctx, isec));
        functions.fold((0, 0), |(n, size), isec| (n + 1, size + isec.size as u64))
    });
    t.stop();

    let mut t = ctx.timer("icf-rounds");
    let mut digests: Vec<Digest> =
        candidates.par_iter().map(|&id| compute_digest(ctx, &cand_index, id)).collect();
    let edges = gather_edges(ctx, &cand_index, &candidates);

    // Refine until the number of equivalence classes stops growing, as
    // mold does. Counting the classes costs about as much as a
    // propagation, so propagate three times per count (mold's ratio).
    // count_num_classes inserts every digest into the reused DigestMap,
    // which counts distinct digests and elects each class's leader in
    // one pass, so no sort is needed here or afterward. Classes only
    // ever split, so the count is monotone and the loop terminates.
    let mut map = DigestMap::new(candidates.len());
    let mut scratch = vec![Digest::default(); digests.len()];
    let mut num_classes = usize::MAX;
    loop {
        propagate(&mut digests, &mut scratch, &edges);
        propagate(&mut digests, &mut scratch, &edges);
        propagate(&mut digests, &mut scratch, &edges);

        let n = count_num_classes(&digests, &mut map);
        if n == num_classes {
            break;
        }
        num_classes = n;
    }
    t.stop();

    let _t = ctx.timer("icf-fold");

    // The final counting round elected a leader for every digest; look
    // each candidate's leader up. The converged 128-bit digests are the
    // equivalence classes, folded directly, as mold does.
    let leaders: Vec<u32> = digests.par_iter().map(|&digest| map.find(digest)).collect();

    #[cfg(debug_assertions)]
    verify_leaders(ctx, &cand_index, &candidates, &leaders);
    if let Some(total) = functions {
        report_folds(ctx, &candidates, &leaders, total);
    }

    // Fold members onto leaders and give each leader the strongest
    // alignment among its members (mold's update_alignment): the leader
    // is laid out for every folded reference.
    let mut folded = Vec::new();
    for (i, &l) in leaders.iter().enumerate() {
        let l = l as usize;
        if l != i {
            let member = candidates[i];
            let leader = candidates[l];
            ctx.isecs[member].replacement = leader as u32;
            let a = ctx.isecs[member].p2align;
            ctx.isecs[leader].p2align = ctx.isecs[leader].p2align.max(a);
            folded.push((member, leader));
        }
    }
    ctx.folded_subsec_names = folded_subsec_names(ctx, &folded);
}
