//! Identical code folding.
//!
//! ld64 deduplicates identical functions by default (disabled with
//! -no_deduplicate); mold's ICF does the same for ELF. Two subsections
//! can share one copy when their bytes, relocations and unwind
//! behavior are all identical, *and* folding cannot be observed:
//! C++ explicitly permits identical instantiations to coalesce (that's
//! what weak definitions are), so folding is restricted to
//! subsections defined only by weak symbols.
//!
//! The algorithm follows mold: every candidate gets a hash of its
//! literal content, and a few refinement rounds rehash each candidate
//! with the previous-round hashes of its relocation targets, so the
//! hash comes to describe the whole reachable shape. Groups with equal
//! final hashes are then verified structurally and folded onto their
//! first member.

use std::hash::Hash;

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;
use crate::macho::input_sections::RelocTarget;

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

struct SipHash13_128 {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
    buf: [u8; 8],
    buflen: u8,
    sum: u8,
}

impl SipHash13_128 {
    #[inline]
    fn new(key: &[u8; 16]) -> SipHash13_128 {
        let k0 = u64::from_le_bytes(key[..8].try_into().unwrap());
        let k1 = u64::from_le_bytes(key[8..].try_into().unwrap());
        SipHash13_128 {
            v0: 0x736f_6d65_7073_6575 ^ k0,
            v1: 0x646f_7261_6e64_6f6d ^ k1 ^ 0xee,
            v2: 0x6c79_6765_6e65_7261 ^ k0,
            v3: 0x7465_6462_7974_6573 ^ k1,
            buf: [0; 8],
            buflen: 0,
            sum: 0,
        }
    }

    #[inline]
    fn update(&mut self, mut msg: &[u8]) {
        self.sum = self.sum.wrapping_add(msg.len() as u8);

        if self.buflen != 0 {
            let buflen = self.buflen as usize;
            if buflen + msg.len() < 8 {
                self.buf[buflen..buflen + msg.len()].copy_from_slice(msg);
                self.buflen += msg.len() as u8;
                return;
            }

            let n = 8 - buflen;
            self.buf[buflen..].copy_from_slice(&msg[..n]);
            self.compress(u64::from_le_bytes(self.buf));
            msg = &msg[n..];
            self.buflen = 0;
        }

        while msg.len() >= 8 {
            self.compress(u64::from_le_bytes(msg[..8].try_into().unwrap()));
            msg = &msg[8..];
        }

        self.buf[..msg.len()].copy_from_slice(msg);
        self.buflen = msg.len() as u8;
    }

    /// Updates the hash with an in-memory `Digest`. Propagation hashes only
    /// complete digests, so this is the aligned 16-byte path through `update`.
    #[inline(always)]
    fn update_digest(&mut self, digest: Digest) {
        debug_assert_eq!(self.buflen, 0);
        self.sum = self.sum.wrapping_add(16);
        self.compress(u64::from_le_bytes(digest.hi.to_ne_bytes()));
        self.compress(u64::from_le_bytes(digest.lo.to_ne_bytes()));
    }

    #[inline]
    fn finish(mut self) -> Digest {
        self.buf[self.buflen as usize..].fill(0);
        self.compress((u64::from(self.sum) << 56) | u64::from_le_bytes(self.buf));

        self.v2 ^= 0xee;
        self.finalize();
        let hi = self.v0 ^ self.v1 ^ self.v2 ^ self.v3;

        self.v1 ^= 0xdd;
        self.finalize();
        let lo = self.v0 ^ self.v1 ^ self.v2 ^ self.v3;
        Digest { hi, lo }
    }

    #[inline(always)]
    fn round(&mut self) {
        self.v0 = self.v0.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(13);
        self.v1 ^= self.v0;
        self.v0 = self.v0.rotate_left(32);
        self.v2 = self.v2.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(16);
        self.v3 ^= self.v2;
        self.v0 = self.v0.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(21);
        self.v3 ^= self.v0;
        self.v2 = self.v2.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(17);
        self.v1 ^= self.v2;
        self.v2 = self.v2.rotate_left(32);
    }

    #[inline(always)]
    fn compress(&mut self, m: u64) {
        self.v3 ^= m;
        self.round();
        self.v0 ^= m;
    }

    #[inline(always)]
    fn finalize(&mut self) {
        self.round();
        self.round();
        self.round();
    }
}

/// A lock-free digest -> leader map, ported from mold-rust. It counts
/// the distinct digests in a propagation round and, as a side effect,
/// records the lowest-index candidate for each digest as its class
/// leader - so counting classes and electing leaders is one pass, no
/// sort. Slots are stamped with the round they were written in and the
/// table is reused across rounds without clearing (a slot from an
/// earlier round reads as vacant).
struct DigestSlot {
    hi: std::sync::atomic::AtomicU64,
    lo: std::sync::atomic::AtomicU64,
    leader: std::sync::atomic::AtomicU32,
}
struct DigestMap {
    round: u64,
    mask: usize,
    slots: Vec<DigestSlot>,
}
impl DigestMap {
    fn new(n: usize) -> DigestMap {
        let len = n.saturating_mul(2).next_power_of_two();
        DigestMap {
            round: 1,
            mask: len - 1,
            slots: (0..len)
                .map(|_| DigestSlot {
                    hi: std::sync::atomic::AtomicU64::new(0),
                    lo: std::sync::atomic::AtomicU64::new(0),
                    leader: std::sync::atomic::AtomicU32::new(0),
                })
                .collect(),
        }
    }

    fn next_round(&mut self) {
        self.round += 1;
    }

    /// Inserts (digest -> candidate), keeping the lowest candidate index
    /// as the leader. Returns true if the digest was not already present
    /// this round.
    fn insert(&self, digest: Digest, cand: u32) -> bool {
        use std::sync::atomic::Ordering;
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
        use std::sync::atomic::Ordering;
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

pub fn icf_sections<E: Arch>(ctx: &mut Context<E>) {
    use rayon::prelude::*;

    // Candidates: live, executable, and defined exclusively by weak
    // symbols, so no one may rely on their addresses being distinct.
    // The per-subsection weak-only AND accumulates in parallel as a
    // three-state atomic: unset, all-weak-so-far, or poisoned by a
    // non-weak definition (which wins under any ordering).
    use std::sync::atomic::{AtomicU8, Ordering};
    let weak_state: Vec<AtomicU8> = (0..ctx.isecs.len()).map(|_| AtomicU8::new(0)).collect();
    ctx.objs.par_iter().for_each(|obj| {
        for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if nlist.is_stab() || nlist.n_type() != N_SECT {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            // Compiler-generated temporary labels don't make an atom's
            // address observable.
            if !nlist.is_extern() && (sym.name().starts_with('l') || sym.name().starts_with('L')) {
                continue;
            }
            let Some(isec) = sym.input_section().map(|i| i as usize) else {
                continue;
            };
            if nlist.n_desc & N_WEAK_DEF != 0 {
                let _ =
                    weak_state[isec].compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed);
            } else {
                weak_state[isec].store(2, Ordering::Relaxed);
            }
        }
    });
    let weak_only: Vec<Option<bool>> = weak_state
        .into_iter()
        .map(|s| match s.into_inner() {
            0 => None,
            1 => Some(true),
            _ => Some(false),
        })
        .collect();

    let is_candidate = |ctx: &Context<E>, id: usize| -> bool {
        let isec = &ctx.isecs[id];
        isec.is_alive()
            && isec.replacement == crate::macho::input_sections::NO_REPLACEMENT
            && ctx.hdr_of(isec).segname() == "__TEXT"
            && ctx.hdr_of(isec).flags & S_ATTR_PURE_INSTRUCTIONS != 0
            && ctx.hdr_of(isec).flags & (S_ATTR_NO_DEAD_STRIP | S_ATTR_LIVE_SUPPORT) == 0
            && weak_only[id] == Some(true)
            // Don't fold functions carrying debug info: it would leave
            // their DWARF describing folded-away code. ld64 disables
            // its deduplication pass for debug objects for the same
            // reason (it folds freely on release links).
            && !ctx.objs[isec.file as usize].has_debug_info
    };

    let __t = std::time::Instant::now();
    let candidates: Vec<usize> =
        (0..ctx.isecs.len()).into_par_iter().filter(|&i| is_candidate(ctx, i)).collect();
    if candidates.len() < 2 {
        return;
    }
    let mut cand_index = vec![usize::MAX; ctx.isecs.len()];
    for (i, &id) in candidates.iter().enumerate() {
        cand_index[id] = i;
    }

    let edge_of = |ctx: &Context<E>, obj: usize, target: RelocTarget, addend: i64| -> (Edge, i64) {
        match target {
            RelocTarget::Sym(idx) => {
                let sym_id = ctx.objs[obj].symbols[idx as usize];
                let sym = &ctx.symbols[sym_id];
                if let (Some(FileId::Obj(_)), Some(isec)) = (sym.file(), sym.input_section()) {
                    let isec = ctx.resolve_isec(isec as usize);
                    if cand_index[isec] != usize::MAX {
                        return (Edge::Candidate(cand_index[isec]), addend + sym.value as i64);
                    }
                    return (Edge::Isec(isec, sym.value), addend);
                }
                (Edge::Sym(sym_id as usize), addend)
            }
            RelocTarget::Section(isec) => {
                let isec = ctx.resolve_isec(isec as usize);
                if cand_index[isec] != usize::MAX {
                    return (Edge::Candidate(cand_index[isec]), addend);
                }
                (Edge::Isec(isec, 0), addend)
            }
        }
    };

    // The base digest of a candidate: its bytes and the non-candidate
    // parts of its edges, hashed with SipHash13-128 exactly as
    // mold-rust's compute_digest does (candidate edges are mixed in
    // during the rounds instead). A fixed key keeps the link
    // reproducible; the digest is used only to group, never emitted.
    const KEY: [u8; 16] = *b"mold-macho-icf!!";
    let base_hash = |ctx: &Context<E>, id: usize| -> Digest {
        let isec = &ctx.isecs[id];
        let mut h = SipHash13_128::new(&KEY);
        h.update(&ctx.hdr_of(isec).flags.to_ne_bytes());
        h.update(&isec.size.to_ne_bytes());
        h.update(&isec.data().len().to_ne_bytes());
        h.update(isec.data());
        for rel in ctx.isec_relocs(id) {
            h.update(&rel.offset.to_ne_bytes());
            h.update(&rel.r_type.to_ne_bytes());
            h.update(&[rel.size, rel.is_pcrel as u8, rel.is_subtracted as u8]);
            let (edge, addend) = edge_of(ctx, isec.file as usize, rel.target(), rel.addend);
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
        // Unwinding is part of a function's identity; the subsection
        // holds its record range.
        let recs = isec.unwind_offset as usize..(isec.unwind_offset + isec.nunwind) as usize;
        for rec in &ctx.unwind_records[recs] {
            h.update(&rec.input_offset.to_ne_bytes());
            h.update(&rec.code_len.to_ne_bytes());
            h.update(&rec.encoding.to_ne_bytes());
            h.update(&rec.personality().map_or(u64::MAX, |p| p as u64).to_ne_bytes());
            h.update(&rec.fde().map_or(u64::MAX, |f| f as u64).to_ne_bytes());
            if let Some((lsda, off)) = rec.lsda() {
                h.update(&ctx.resolve_isec(lsda).to_ne_bytes());
                h.update(&off.to_ne_bytes());
            }
        }
        h.finish()
    };

    // Refinement rounds propagate hashes along edges; log2(n) rounds
    // reach across any chain of distinct shapes.
    if std::env::var_os("MOLD_TIMING").is_some() {
        eprintln!("      icf-prep {:?} candidates {}", __t.elapsed(), candidates.len());
    }
    let __t = std::time::Instant::now();

    // Content is hashed exactly once; the refinement rounds mix only
    // fixed-size digests - each candidate's base digest plus its
    // candidate-edge targets' previous-round digests - via
    // update_digest, mold-rust's aligned 16-byte fast path, so a round
    // is cheap however many rounds log2(n) requires.
    let base: Vec<Digest> = candidates.par_iter().map(|&id| base_hash(ctx, id)).collect();

    // Candidate edges in CSR form - one flat values array indexed by a
    // per-candidate prefix-summed offset, exactly mold-rust's Edges
    // (gather_edges). The propagation loop is the hot path; a single
    // contiguous array keeps it cache-friendly, where a Vec<Vec<u32>>
    // would chase one heap allocation per candidate.
    let counts: Vec<u32> = candidates
        .par_iter()
        .map(|&id| {
            let obj = ctx.isecs[id].file as usize;
            ctx.isec_relocs(id)
                .iter()
                .filter(|rel| {
                    matches!(edge_of(ctx, obj, rel.target(), rel.addend).0, Edge::Candidate(_))
                })
                .count() as u32
        })
        .collect();
    let mut edge_indices: Vec<u32> = Vec::with_capacity(candidates.len() + 1);
    edge_indices.push(0);
    for c in &counts {
        edge_indices.push(edge_indices.last().unwrap() + c);
    }
    let mut edge_values: Vec<u32> = vec![0; *edge_indices.last().unwrap() as usize];
    {
        struct EdgeBuf(*mut u32);
        unsafe impl Sync for EdgeBuf {}
        let out = EdgeBuf(edge_values.as_mut_ptr());
        let out = &out;
        let edge_indices = &edge_indices;
        candidates.par_iter().enumerate().for_each(|(vertex, &id)| {
            let isec = &ctx.isecs[id];
            let mut i = edge_indices[vertex] as usize;
            for rel in ctx.isec_relocs(id) {
                if let Edge::Candidate(c) =
                    edge_of(ctx, isec.file as usize, rel.target(), rel.addend).0
                {
                    // SAFETY: this vertex alone owns its prefix-sum range.
                    unsafe { *out.0.add(i) = c as u32 };
                    i += 1;
                }
            }
        });
    }

    // Refine until the number of equivalence classes stops growing, as
    // mold does. Counting the classes costs about as much as a
    // propagation, so propagate three times per count (mold's ratio),
    // ping-ponging between two buffers. count_classes inserts every
    // digest into the reused DigestMap - which counts distinct digests
    // and elects each class's leader in one pass - so no sort is needed
    // here or afterward. Classes only ever split, so the count is
    // monotone and the loop terminates.
    let mut hashes = base;
    let mut scratch = vec![Digest::default(); hashes.len()];
    let mut map = DigestMap::new(candidates.len());
    let mut prev_classes = usize::MAX;
    loop {
        for _ in 0..3 {
            let cur = &hashes;
            (0..candidates.len())
                .into_par_iter()
                .map(|i| {
                    // next[i] = H(cur[i], cur[neighbors]) - mold-rust's
                    // propagate hashes the vertex's own current digest,
                    // so its nth-round digest is a hash of its unfolding
                    // into a tree of depth n.
                    let mut h = SipHash13_128::new(&KEY);
                    h.update_digest(cur[i]);
                    for &c in &edge_values[edge_indices[i] as usize..edge_indices[i + 1] as usize] {
                        h.update_digest(cur[c as usize]);
                    }
                    h.finish()
                })
                .collect_into_vec(&mut scratch);
            std::mem::swap(&mut hashes, &mut scratch);
        }
        map.next_round();
        let n: usize = (0..candidates.len())
            .into_par_iter()
            .map(|i| map.insert(hashes[i], i as u32) as usize)
            .sum();
        if n == prev_classes {
            break;
        }
        prev_classes = n;
    }

    if std::env::var_os("MOLD_TIMING").is_some() {
        eprintln!("      icf-rounds {:?}", __t.elapsed());
    }
    let __t = std::time::Instant::now();

    // The final counting round elected a leader (lowest candidate index)
    // for every digest; look each candidate's leader up and fold the
    // non-leaders onto it. The converged 128-bit digests are the
    // equivalence classes, folded directly, as mold does; debug builds
    // re-verify byte equality as an assertion.
    let leaders: Vec<u32> =
        (0..candidates.len()).into_par_iter().map(|i| map.find(hashes[i])).collect();

    #[cfg(debug_assertions)]
    {
        let equal = |a: usize, b: usize| -> bool {
            let (x, y) = (&ctx.isecs[a], &ctx.isecs[b]);
            let (xr, yr) = (ctx.isec_relocs(a), ctx.isec_relocs(b));
            x.data() == y.data()
                && ctx.hdr_of(x).flags == ctx.hdr_of(y).flags
                && xr.len() == yr.len()
                && xr.iter().zip(yr).all(|(r, s)| {
                    r.offset == s.offset
                        && r.r_type == s.r_type
                        && r.size == s.size
                        && r.is_pcrel == s.is_pcrel
                        && edge_of(ctx, x.file as usize, r.target(), r.addend)
                            == edge_of(ctx, y.file as usize, s.target(), s.addend)
                })
        };
        for (i, &l) in leaders.iter().enumerate() {
            debug_assert!(equal(candidates[i], candidates[l as usize]));
        }
    }

    // Fold members onto leaders and give each leader the strongest
    // alignment among its members (mold's update_alignment): the leader
    // is laid out for every folded reference.
    for (i, &l) in leaders.iter().enumerate() {
        let l = l as usize;
        if l != i {
            let member = candidates[i];
            let leader = candidates[l];
            ctx.isecs[member].replacement = leader as u32;
            let a = ctx.isecs[member].p2align;
            ctx.isecs[leader].p2align = ctx.isecs[leader].p2align.max(a);
        }
    }
    if std::env::var_os("MOLD_TIMING").is_some() {
        eprintln!("      icf-fold {:?}", __t.elapsed());
    }
}
