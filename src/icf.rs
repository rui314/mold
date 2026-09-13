//! This file implements the Identical Code Folding feature which can
//! reduce the output file size of a typical program by a few percent.
//! ICF identifies read-only input sections that happen to be identical
//! and thus can be used interchangeably. ICF leaves one of them and discards
//! the others.
//!
//! ICF is usually used in combination with -ffunction-sections and
//! -fdata-sections compiler options, so that object files have one section
//! for each function or variable instead of having one large .text or .data.
//! The unit of ICF merging is section.
//!
//! Two sections are considered identical by ICF if they have the exact
//! same contents, metadata such as section flags, exception handling
//! records, and relocations. The last one is interesting because two
//! relocations are considered identical if they point to the _same_
//! section in terms of ICF.
//!
//! To see what that means, consider two sections, A and B, which are
//! identical except for one pair of relocations. Say, A has a relocation to
//! section C, and B has a relocation to D. In this case, A and B are
//! considered identical if C and D are considered identical. C and D can be
//! either really the same section or two different sections that are
//! considered identical by ICF. Below is an example of such inputs, A, B, C
//! and D:
//!
//!   void A() { C(); }
//!   void B() { D(); }
//!   void C() { A(); }
//!   void D() { B(); }
//!
//! If we assume A and B are mergeable, we can merge C and D, which makes A
//! and B mergeable. There's no contradiction in our assumption, so we can
//! conclude that A and B as well as C and D are mergeable.
//!
//! This problem boils down to one in graph theory. Input to ICF can be
//! considered as a directed graph in which vertices are sections and edges
//! are relocations. Vertices have labels (section contents, etc.), and so
//! are edges (relocation offsets, etc.). Two vertices are considered
//! identical if and only if the (possibly infinite) their unfoldings into
//! regular trees are equal. Given this formulation, we want to find as
//! many identical vertices as possible.
//!
//! Just like a lot of problems with graph, this problem doesn't have a
//! straightforward "optimal" solution, and we need to resort to heuristics.
//!
//! mold approaches this problem by hashing program trees with increasing depth
//! on each iteration.
//! For example, when we start, we only hash individual functions with
//! their call into other functions omitted. From the second iteration, we
//! put the function they call into the hash by appending the hash of those
//! functions from the previous iteration. This means that the nth iteration
//! hashes call chain up to (n-1) levels deep.
//! We use a cryptographic hash function, so the unique number of hashes will
//! only monotonically increase as we take into account of deeper trees with
//! iterations (otherwise, that means we have found a hash collision). We stop
//! when the unique number of hashes stop increasing; this is based on the fact
//! that once we observe an iteration with the same amount of unique hashes as
//! the previous iteration, it will remain unchanged for further iterations.
//! This is provable, but here we omit the proof for brevity.
//!
//! When compared to other approaches, mold's approach has a relatively cheaper
//! cost per iteration, and as a bonus, is highly parallelizable.
//! For Chromium, mold's ICF finishes in less than 1 second with 20 threads,
//! whereas lld takes 5 seconds and gold takes 50 seconds under the same
//! conditions.
//!
//! Identical Code Folding.
//!
//! ICF merges read-only sections with identical contents, metadata and
//! relocations. Two relocations count as identical if they refer to
//! sections that are themselves identical, which makes this a graph
//! problem: sections are vertices and relocations are edges, and two
//! vertices are equivalent if their (possibly infinite) unfoldings into
//! trees are equal.
//!
//! The approach is to hash each section together with the hashes of the
//! sections it refers to, repeatedly: after n rounds a section's digest
//! covers its references up to depth n. The number of distinct digests
//! can only grow, so once two rounds produce the same count the partition
//! into equivalence classes has converged.

use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;

use crate::arch::Arch;
use crate::cmdline::ReportOutput;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{ObjId, ObjectFile};
use crate::input_sections::{InputSection, SectionRef};
use crate::symbol::{is_c_identifier, OriginValue, Symbol};
use crate::util::perf::Counter;
use crate::util::siphash::SipHash13_128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Digest {
    hi: u64,
    lo: u64,
}

impl Digest {
    #[inline]
    fn update(self, hasher: &mut SipHash13_128) {
        // Match hashing the native Digest representation, as C++ does.
        hasher.update_u64(u64::from_le(self.hi));
        hasher.update_u64(u64::from_le(self.lo));
    }

    fn from_ne_bytes(bytes: [u8; 16]) -> Digest {
        Digest {
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

// A concurrent hash map from digest to section. We use it to count the
// number of distinct digests and to elect the leader section of each
// digest's equivalence class.
//
// The number of distinct digests cannot exceed the number of digests,
// so we can allocate a large enough table upfront and never need to
// grow it.
//
// The map is reused across propagation rounds. Instead of clearing the
// table at the start of each round, we stamp each slot with the round
// number in which it was written; a slot stamped with an earlier round
// is treated as vacant.
//
// Each slot consists of two 64-bit words and a section pointer. The
// first word packs the round number, a busy bit, and 48 bits of the
// digest; the second word holds another 64 bits. An inserter claims a
// vacant slot by installing the first word with compare-and-swap with
// the busy bit set, writes the second word and the pointer, and then
// rewrites the first word with the busy bit cleared to publish the
// slot. Since the slot index is derived from digest bits that the
// first word doesn't contain, a successful match effectively compares
// an entire 128-bit digest, so the map is exact under the same
// hash-collision assumption the surrounding algorithm is built on.
//
// Of all sections inserted with the same digest in the same round, the
// slot ends up pointing to the one with the lowest priority, which ICF
// uses as the leader of the digest's equivalence class.
struct DigestMap {
    // Time begins in round 1 so that all-zero slots, the initial state
    // of the table, read as vacant.
    round: u64,
    mask: usize,
    slots: Vec<DigestSlot>,
}

#[derive(Default)]
struct DigestSlot {
    hi: AtomicU64,
    lo: AtomicU64,
    leader: AtomicU64,
}

impl DigestMap {
    fn new(n: usize) -> DigestMap {
        let len = n.saturating_mul(2).next_power_of_two();
        DigestMap {
            round: 1,
            mask: len - 1,
            slots: (0..len).map(|_| DigestSlot::default()).collect(),
        }
    }

    fn next_round(&mut self) {
        self.round += 1;
        if self.round == 1 << 15 {
            // The round number wrapped around, making stale slot stamps
            // ambiguous, so reset the table. In practice, ICF converges long
            // before this point.
            self.slots
                .par_iter()
                .for_each(|slot| slot.hi.store(0, Ordering::Relaxed));
            self.round = 1;
        }
    }

    // Returns true if the digest was not in the table.
    fn insert<E: Arch>(&self, ctx: &Context<E>, digest: Digest, isec: SectionRef) -> bool {
        const BUSY_BIT: u64 = 1 << 48;
        let tag = digest.hi >> 16;
        let value = (self.round << 49) | tag;

        let mut i = digest.hi as usize & self.mask;
        loop {
            let slot = &self.slots[i];
            let mut x = slot.hi.load(Ordering::Acquire);

            // If the slot was last written in an earlier round, it's vacant;
            // try to claim it.
            while x >> 49 != self.round {
                match slot.hi.compare_exchange_weak(
                    x,
                    value | BUSY_BIT,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        slot.lo.store(digest.lo, Ordering::Relaxed);
                        slot.leader.store(isec.encode(), Ordering::Relaxed);
                        slot.hi.store(value, Ordering::Release);
                        return true;
                    }
                    Err(actual) => x = actual,
                }
            }

            // The slot is occupied. If it holds a different digest, try the
            // next slot.
            if x & 0xffff_ffff_ffff != tag {
                i = (i + 1) & self.mask;
                continue;
            }

            // The tags match; compare the digest bits in the second word,
            // waiting for the writer to publish them if the slot is still
            // being claimed.
            while x & BUSY_BIT != 0 {
                std::hint::spin_loop();
                x = slot.hi.load(Ordering::Acquire);
            }
            if slot.lo.load(Ordering::Relaxed) != digest.lo {
                i = (i + 1) & self.mask;
                continue;
            }

            // The digest is already in the table; keep the slot pointing to
            // the lowest-priority section.
            let candidate_priority = ctx.section(isec).priority(&ctx.objs[isec.file.index()]);
            let candidate = isec.encode();
            let mut cur = slot.leader.load(Ordering::Relaxed);
            loop {
                let current = SectionRef::decode(cur);
                if candidate_priority
                    >= ctx
                        .section(current)
                        .priority(&ctx.objs[current.file.index()])
                {
                    break;
                }
                match slot.leader.compare_exchange_weak(
                    cur,
                    candidate,
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

    // Returns the section associated with a digest. The digest must have
    // been inserted in the current round, and no insertion may be running
    // concurrently.
    fn find(&self, digest: Digest) -> SectionRef {
        let value = (self.round << 49) | (digest.hi >> 16);
        let mut i = digest.hi as usize & self.mask;
        loop {
            let slot = &self.slots[i];
            if slot.hi.load(Ordering::Relaxed) == value
                && slot.lo.load(Ordering::Relaxed) == digest.lo
            {
                return SectionRef::decode(slot.leader.load(Ordering::Relaxed));
            }
            i = (i + 1) & self.mask;
        }
    }
}

fn uniquify_cies<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("uniquify_cies");
    let mut leaders: Vec<crate::chunks::eh_frame::CieHandle<E>> = Vec::new();
    for file in &mut ctx.objs {
        let file_ptr = file as *mut ObjectFile<E>;
        let cies = file.cies.as_mut_ptr();
        for ci in 0..file.cies.len() {
            // SAFETY: object files are boxed and ICF does not resize their CIE
            // vectors, so leaders remain stable through this serial pass.
            let cie = unsafe { crate::chunks::eh_frame::CieHandle::new(file_ptr, cies.add(ci)) };
            let found = leaders.iter().position(|&leader| leader.equals(cie));
            match found {
                Some(idx) => cie.icf_idx(idx as u32),
                None => {
                    cie.icf_idx(leaders.len() as u32);
                    leaders.push(cie);
                }
            }
        }
    }
}

fn is_eligible<E: Arch>(ctx: &Context<E>, isec: &InputSection<E>) -> bool {
    let file = &ctx.objs[isec.file.index()];
    let name: &[u8] = isec.name(file);
    if isec.sh_size == 0
        || !isec.is_alloc()
        || isec.sh_type(file) == SHT_NOBITS
        || is_c_identifier(name)
    {
        return false;
    }
    if isec.sh_flags & SHF_EXECINSTR as u64 != 0 {
        return (ctx.args.icf_all || !isec.is_address_taken())
            && name != b".init"
            && name != b".fini";
    }
    // .gcc_except_table contains a compiler-generated table. Pointer
    // equality for the section is not significant because only the C++
    // exception handling code will use the table at runtime.
    if name == b".gcc_except_table" || name.starts_with(b".gcc_except_table.") {
        return true;
    }
    let is_readonly = isec.sh_flags & SHF_WRITE as u64 == 0;
    let is_relro = name.starts_with(b".data.rel.ro");
    (ctx.args.ignore_data_address_equality || !isec.is_address_taken()) && (is_readonly || is_relro)
}

fn compute_digest<E: Arch>(ctx: &Context<E>, key: &[u8; 16], r: SectionRef) -> Digest {
    let file = &ctx.objs[r.file.index()];
    let isec = file.section_at(r.shndx);
    let mut h = SipHash13_128::new(key);

    let hash_u32 = |h: &mut SipHash13_128, v: u32| h.update(&v.to_ne_bytes());
    let hash_u64 = |h: &mut SipHash13_128, v: u64| h.update(&v.to_ne_bytes());
    let hash_i64 = |h: &mut SipHash13_128, v: i64| h.update(&v.to_ne_bytes());
    let hash_bytes = |h: &mut SipHash13_128, b: &[u8]| {
        h.update(&b.len().to_ne_bytes());
        h.update(b);
    };
    let hash_symbol = |h: &mut SipHash13_128, id: crate::symbol::SymbolId, sym: &Symbol| {
        if sym.file().is_none() || sym.is_imported() {
            h.update(b"1");
            hash_u64(h, id.0 as u64);
        } else {
            match sym.origin() {
                OriginValue::Fragment(frag) => {
                    h.update(b"2");
                    hash_u64(h, ((frag.section.0 as u64) << 32) | frag.entry.raw() as u64);
                }
                OriginValue::InputSection(sec) => {
                    let isec = ctx.input_section(sec);
                    if isec.icf_index().is_some() {
                        h.update(b"4");
                    } else {
                        h.update(b"5");
                        hash_u64(h, isec.section_ref().encode());
                    }
                }
                _ => h.update(b"3"),
            }
        }
        hash_u64(h, sym.value);
    };

    hash_bytes(&mut h, isec.contents());
    hash_u64(&mut h, isec.sh_flags);
    let fdes = isec.fdes(file);
    h.update(&fdes.len().to_ne_bytes());
    h.update(&isec.rels(file).len().to_ne_bytes());

    for fde in fdes {
        let cie = &file.cies[fde.cie_idx as usize];
        hash_u32(&mut h, cie.icf_idx);
        // Bytes 0 to 4 contain the length of this record, and
        // bytes 4 to 8 contain an offset to CIE.
        hash_bytes(&mut h, &fde.contents::<E>(file)[8..]);
        let rels = fde.rels(file);
        h.update(&rels.len().to_ne_bytes());
        for rel in rels.iter().skip(1) {
            let id = file.base.symbols[rel.r_sym() as usize];
            hash_symbol(&mut h, id, &ctx.symbols[id]);
            hash_u32(&mut h, rel.r_type());
            hash_u64(&mut h, rel.r_offset() - fde.input_offset as u64);
            hash_i64(&mut h, file.section_at(cie.section).rel_addend(rel));
        }
    }

    for rel in isec.rels(file) {
        hash_u64(&mut h, rel.r_offset());
        hash_u32(&mut h, rel.r_type());
        hash_i64(&mut h, isec.rel_addend(rel));
        let id = file.base.symbols[rel.r_sym() as usize];
        hash_symbol(&mut h, id, &ctx.symbols[id]);
    }
    finish_digest(h)
}

fn gather_sections<E: Arch>(ctx: &Context<E>) -> Vec<SectionRef> {
    let _t = ctx.timer("gather_sections");

    static ELIGIBLE: Counter = Counter::new("icf_eligibles");
    static NON_ELIGIBLE: Counter = Counter::new("icf_non_eligibles");

    // Count the number of eligible input sections for each input file
    // and turn the counts into starting indices with a prefix sum.
    let counts: Vec<usize> = ctx
        .objs
        .par_iter()
        .map(|file| {
            let mut count = 0;
            let mut non_eligible = 0;
            for isec in file.input_sections() {
                if !isec.is_alive() {
                    continue;
                }
                if is_eligible(ctx, isec) {
                    isec.set_icf_index(0);
                    count += 1;
                } else {
                    non_eligible += 1;
                }
            }
            ELIGIBLE.add(count as i64);
            NON_ELIGIBLE.add(non_eligible);
            count
        })
        .collect();

    let mut file_indices = Vec::with_capacity(counts.len() + 1);
    file_indices.push(0usize);
    for count in counts {
        file_indices.push(file_indices.last().unwrap() + count);
    }
    let total = *file_indices.last().unwrap();
    assert!(
        u32::try_from(total).is_ok(),
        "too many ICF-eligible sections"
    );
    let mut sections = vec![
        SectionRef {
            file: ObjId(0),
            shndx: 0,
        };
        total
    ];

    let mut rest = sections.as_mut_slice();
    let mut section_chunks = Vec::with_capacity(ctx.objs.len());
    for range in file_indices.windows(2) {
        let (chunk, tail) = rest.split_at_mut(range[1] - range[0]);
        section_chunks.push(chunk);
        rest = tail;
    }

    // Fill `sections` contents.
    ctx.objs
        .par_iter()
        .zip(section_chunks.into_par_iter())
        .enumerate()
        .for_each(|(fi, (file, out))| {
            let base = file_indices[fi];
            let mut local = 0;
            for isec in file.input_sections() {
                if isec.icf_index().is_some() {
                    isec.set_icf_index((base + local) as u32);
                    out[local] = SectionRef {
                        file: file.id(),
                        shndx: isec.shndx,
                    };
                    local += 1;
                }
            }
            debug_assert_eq!(local, out.len());
        });

    sections
}

fn for_each_edge<E: Arch>(ctx: &Context<E>, r: SectionRef, mut f: impl FnMut(u32)) {
    let file: &ObjectFile<E> = &ctx.objs[r.file.index()];
    let isec = file.section_at(r.shndx);
    let mut add = |sym_idx: u32| {
        let sym = &ctx.symbols[file.base.symbols[sym_idx as usize]];
        if let Some(target) = sym.input_section_ref(ctx).and_then(InputSection::icf_index) {
            f(target);
        }
    };
    for fde in isec.fdes(file) {
        for rel in fde.rels(file).iter().skip(1) {
            add(rel.r_sym());
        }
    }
    for rel in isec.rels(file) {
        add(rel.r_sym());
    }
}

struct Edges {
    values: Vec<u32>,
    indices: Vec<u32>,
}

// Build a graph, treating every function as a vertex and every function call
// as an edge. See the description at the top for a more detailed formulation.
// We use u32 indices here to improve cache locality.
//
// Relocations in a section's FDEs are edges too, because compute_digest
// hashes eligible relocation targets without identity, and every such
// target must be represented as an edge to remain distinguishable. In
// particular, an FDE's reference to an LSDA is an edge; without it, two
// identical functions whose exception tables catch different types would
// be folded into one.
fn gather_edges<E: Arch>(ctx: &Context<E>, sections: &[SectionRef]) -> Edges {
    let _t = ctx.timer("gather_edges");

    // Count the number of outgoing edges for each vertex and turn the
    // counts into starting indices with a prefix sum. The extra entry at
    // the end makes edge_indices[i + 1] valid for every vertex, so that
    // vertex i's edges are edge_indices[i] to edge_indices[i + 1].
    let counts: Vec<u32> = sections
        .par_iter()
        .map(|&r| {
            let mut count = 0u32;
            for_each_edge::<E>(ctx, r, |_| count += 1);
            count
        })
        .collect();

    let mut indices = Vec::with_capacity(sections.len() + 1);
    indices.push(0u32);
    for count in counts {
        indices.push(
            indices
                .last()
                .unwrap()
                .checked_add(count)
                .expect("too many ICF edges"),
        );
    }

    let mut values = vec![0; *indices.last().unwrap() as usize];
    // Split at vertex boundaries so each task owns a disjoint slice of
    // the edge array.
    rayon::iter::split(
        (0..sections.len(), values.as_mut_slice()),
        |(range, out)| {
            if range.len() <= 1 {
                return ((range, out), None);
            }
            let mid = range.start + range.len() / 2;
            let (left, right) = out.split_at_mut((indices[mid] - indices[range.start]) as usize);
            ((range.start..mid, left), Some((mid..range.end, right)))
        },
    )
    .for_each(|(range, out)| {
        let base = indices[range.start] as usize;
        let mut i = 0;
        for vertex in range {
            for_each_edge::<E>(ctx, sections[vertex], |edge| {
                out[i] = edge;
                i += 1;
            });
            debug_assert_eq!(base + i, indices[vertex + 1] as usize);
        }
    });

    Edges { values, indices }
}

// Compute the next-round digest of each vertex by hashing its current
// digest and the current digests of the vertices it refers to. A
// vertex's digest after the nth round is therefore a hash of its
// unfolding into a tree of depth n.
fn propagate(key: &[u8; 16], cur: &mut Vec<Digest>, next: &mut Vec<Digest>, edges: &Edges) {
    next.par_iter_mut().enumerate().for_each(|(i, out)| {
        let mut h = SipHash13_128::new(key);
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

fn count_num_classes<E: Arch>(
    ctx: &Context<E>,
    digests: &[Digest],
    sections: &[SectionRef],
    map: &mut DigestMap,
) -> usize {
    map.next_round();
    let count = digests
        .par_iter()
        .zip(sections)
        .map(|(&digest, &isec)| usize::from(map.insert(ctx, digest, isec)))
        .sum();
    static COUNTER: Counter = Counter::new("icf_round");
    COUNTER.increment();
    count
}

fn print_icf_sections<E: Arch>(ctx: &Context<E>, sections: &[SectionRef], output: &ReportOutput) {
    let mut leaders: Vec<(SectionRef, Vec<SectionRef>)> = Vec::new();
    let mut map: std::collections::HashMap<SectionRef, usize> = std::collections::HashMap::new();
    for &r in sections {
        let leader = ctx.section(r).icf_leader_in_round();
        if leader == r {
            map.entry(leader).or_insert_with(|| {
                leaders.push((leader, Vec::new()));
                leaders.len() - 1
            });
        }
    }
    for &r in sections {
        let leader = ctx.section(r).icf_leader_in_round();
        if leader != r {
            let idx = *map.entry(leader).or_insert_with(|| {
                leaders.push((leader, Vec::new()));
                leaders.len() - 1
            });
            leaders[idx].1.push(r);
        }
    }
    leaders.sort_by_key(|(l, _)| ctx.section(*l).priority(&ctx.objs[l.file.index()]));

    let mut out = String::new();
    let mut saved = 0usize;
    for (leader, members) in &leaders {
        if members.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "selected section {}\n",
            ctx.section_display(*leader)
        ));
        for m in members {
            out.push_str(&format!(
                "  removing identical section {}\n",
                ctx.section_display(*m)
            ));
            saved += ctx.section(*leader).contents().len();
        }
    }
    out.push_str(&format!("ICF saved {saved} bytes\n"));

    output.write("--print-icf-sections", out.as_bytes());
}

pub fn icf_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("icf");
    if ctx.objs.is_empty() {
        return;
    }
    let mut key = [0u8; 16];
    crate::util::random_bytes(&mut key);

    uniquify_cies(ctx);
    // Prepare for the propagation rounds.
    let sections = gather_sections(ctx);

    // `digests` holds the current digest of each vertex.
    let mut digests: Vec<Digest> = {
        let _t = ctx.timer("compute_digests");
        sections
            .par_iter()
            .map(|&r| compute_digest(ctx, &key, r))
            .collect()
    };

    let edges = gather_edges(ctx, &sections);

    // The digest map is used to count the number of distinct digests in
    // the loop below. As a side effect, it records the lowest-priority
    // section for each digest, which the grouping step after the loop
    // uses as the leader of each equivalence class.
    let mut map = DigestMap::new(digests.len());

    // Execute the propagation rounds until convergence is obtained.
    //
    // The number of distinct digests can only monotonically increase as
    // the rounds hash ever-deeper trees, so once two consecutive rounds
    // yield the same count, the partition of sections into equivalence
    // classes has stopped changing and will remain unchanged for further
    // iterations (proof omitted for brevity). Note that individual
    // digests may well still be changing at that point; sections that
    // have a cycle in downstream (i.e. recursive functions and functions
    // that call them) never settle on a digest. That doesn't matter
    // because sections in the same class change their digests in
    // lockstep, keeping the partition intact.
    {
        let _t = ctx.timer("propagate");
        let mut num_classes = usize::MAX;
        let mut scratch = vec![Digest::default(); digests.len()];
        loop {
            // count_num_classes is as expensive as propagate, so we propagate
            // a few times before counting the number of distinct groups.
            propagate(&key, &mut digests, &mut scratch, &edges);
            propagate(&key, &mut digests, &mut scratch, &edges);
            propagate(&key, &mut digests, &mut scratch, &edges);

            let n = count_num_classes(ctx, &digests, &sections, &mut map);
            if n == num_classes {
                break;
            }
            num_classes = n;
        }
    }

    // Group sections by digest. The final counting round has already
    // elected a leader for each digest; look it up.
    {
        let _t = ctx.timer("group");
        sections
            .par_iter()
            .zip(&digests)
            .for_each(|(&r, &digest)| ctx.section(r).set_icf_leader(map.find(digest)));
    }

    if let Some(output) = &ctx.args.print_icf_sections {
        print_icf_sections(ctx, &sections, output);
    }

    // Update alignment of leaders.
    {
        let _t = ctx.timer("update_alignment");
        sections.par_iter().for_each(|&r| {
            let isec = ctx.section(r);
            let leader = isec.icf_leader_in_round();
            if leader != r {
                ctx.section(leader).update_p2align(isec.p2align());
            }
        });
    }

    // Eliminate duplicate sections.
    // Symbols pointing to eliminated sections will be redirected on the fly when
    // exporting to the symtab.
    {
        let _t = ctx.timer("sweep");
        static ELIMINATED: Counter = Counter::new("icf_eliminated");
        ELIMINATED.add(0);
        sections.par_iter().for_each(|&r| {
            let isec = ctx.section(r);
            if isec.icf_leader_in_round() != r {
                isec.set_icf_removed();
                ctx.objs[r.file.index()].kill_section(r.shndx as usize);
                ELIMINATED.increment();
            } else {
                isec.set_offset(u64::MAX);
            }
        });
    }
}
