//! `.rel.dyn` and `.rela.dyn`, dynamic relocations.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::relrdyn::encode_relr;
use crate::chunks::{self, ChunkHeader};
use crate::context::Context;
use crate::elf::*;
use crate::util::encode_sleb;

// .rel.dyn contains relocation infromation for other sections.
#[derive(Debug)]
pub struct RelDynSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub android_encoded: Vec<u8>,
    pub keep_android_size: bool,
}

impl<E: Arch> RelDynSection<E> {
    pub fn new(args: &crate::cmdline::Args) -> RelDynSection<E> {
        let (name, ty, android_ty) = if E::IS_RELA {
            (".rela.dyn", SHT_RELA, SHT_ANDROID_RELA)
        } else {
            (".rel.dyn", SHT_REL, SHT_ANDROID_REL)
        };
        let rel_size = std::mem::size_of::<ElfRel<E>>() as u64;
        let (ty, entsize, align) = if args.pack_dyn_relocs_android {
            (android_ty, 0, 1)
        } else {
            (ty, rel_size, E::WORD_SIZE as u64)
        };
        let mut hdr = ChunkHeader::<E>::new(name, ty, SHF_ALLOC as u64);
        hdr.shdr.sh_entsize.set(entsize);
        hdr.shdr.sh_addralign.set(align);
        RelDynSection {
            hdr,
            android_encoded: Vec::new(),
            keep_android_size: false,
        }
    }
}

/// Gathers the dynamic relocations of all chunks.
pub fn collect_relocs<E: Arch>(ctx: &Context<E>) -> Vec<ElfRel<E>> {
    let count: usize = ctx
        .chunks
        .iter()
        .map(|&id| {
            let hdr = ctx.chunk_header(id);
            (hdr.num_dynrels - hdr.num_relrs) as usize
        })
        .sum();
    let mut out = vec![ElfRel::<E>::default(); count];
    let mut rest = out.as_mut_slice();
    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        let count = (hdr.num_dynrels - hdr.num_relrs) as usize;
        if count != 0 {
            let slots = rest.split_off_mut(..count).unwrap();
            chunks::write_dynrels(ctx, id, slots);
        }
    }
    debug_assert!(rest.is_empty());
    out
}

/// Encodes base relocations of each chunk in RELR form, using offsets
/// relative to the chunk.
pub fn construct_relr<E: Arch>(ctx: &mut Context<E>) {
    debug_assert!(ctx.args.pack_dyn_relocs_relr);
    let word = E::WORD_SIZE as u64;
    let ids = ctx.chunks.clone();

    for &id in &ids {
        let n = chunks::num_dynrels(ctx, id);
        let hdr = ctx.chunk_header_mut(id);
        hdr.num_dynrels = n;
        hdr.num_relrs = 0;
        hdr.relr.clear();

        // Do not use RELR for executable chunks, as they don't usually contain
        // base relocations.
        if hdr.shdr.sh_flags.get() & SHF_EXECINSTR as u64 != 0 {
            continue;
        }
        // --section-start can override a chunk's alignment. Conservatively use
        // .rel[a].dyn if the explicitly assigned address is not word-aligned.
        let name = hdr.name;
        let addr = ctx.args.section_start.get(name);
        if addr.is_some_and(|&addr| addr % word != 0) {
            continue;
        }
        if n != 0 {
            let offsets = chunks::relr_offsets(ctx, id);
            let hdr = ctx.chunk_header_mut(id);
            hdr.num_relrs = offsets.len() as u64;
            debug_assert!(hdr.num_relrs <= hdr.num_dynrels);
            hdr.relr = encode_relr::<E>(&offsets);
        }
    }

    let size: u64 = ids
        .iter()
        .map(|&id| ctx.chunk_header(id).relr.len() as u64 * word)
        .sum();
    if let Some(relrdyn) = &mut ctx.relrdyn {
        relrdyn.hdr.shdr.sh_size.set(size);
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let ids = ctx.chunks.clone();
    if !ctx.args.pack_dyn_relocs_relr {
        for &id in &ids {
            let n = chunks::num_dynrels(ctx, id);
            let hdr = ctx.chunk_header_mut(id);
            hdr.num_dynrels = n;
            hdr.num_relrs = 0;
        }
    }

    let mut num_relocs = 0;
    let mut num_relrs = 0;
    for &id in &ids {
        let hdr = ctx.chunk_header(id);
        num_relocs += hdr.num_dynrels;
        num_relrs += hdr.num_relrs;
    }

    let size = if ctx.args.pack_dyn_relocs_android {
        let relocs = collect_relocs(ctx);
        // APS2 uses SLEB128-encoded deltas, so .rela.dyn size may oscillate
        // as addresses move. If a shrink is followed by a growth, stop
        // shrinking and pad the encoded stream to converge.
        let encoded = encode_android::<E>(relocs);
        let old_size = ctx.reldyn.hdr.shdr.sh_size.get() as usize;
        let reldyn = &mut ctx.reldyn;
        if old_size != 0 && old_size < encoded.len() {
            reldyn.keep_android_size = true;
        }
        reldyn.android_encoded = encoded;
        if reldyn.keep_android_size && reldyn.android_encoded.len() < old_size {
            reldyn.android_encoded.resize(old_size, 0);
        }
        reldyn.android_encoded.len() as u64
    } else {
        (num_relocs - num_relrs) * std::mem::size_of::<ElfRel<E>>() as u64
    };
    ctx.reldyn.hdr.shdr.sh_size.set(size);
    ctx.reldyn.hdr.shdr.sh_link.set(ctx.dynsym.hdr.shndx);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    if ctx.args.pack_dyn_relocs_android {
        buf[..ctx.reldyn.android_encoded.len()].copy_from_slice(&ctx.reldyn.android_encoded);
    } else {
        let size = std::mem::size_of::<ElfRel<E>>();
        let mut rest = buf;
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            let count = (hdr.num_dynrels - hdr.num_relrs) as usize;
            if count != 0 {
                let slots = rest.split_off_mut(..count * size).unwrap();
                chunks::write_dynrels(ctx, id, rels_from_bytes_mut::<E>(slots));
            }
        }
        debug_assert!(rest.is_empty());
    }
}

// Sort dynamic relocations. This is the reason why we do it.
// Quote from https://www.airs.com/blog/archives/186
//
//   The dynamic linker in glibc uses a one element cache when processing
//   relocs: if a relocation refers to the same symbol as the previous
//   relocation, then the dynamic linker reuses the value rather than
//   looking up the symbol again. Thus the dynamic linker gets the best
//   results if the dynamic relocations are sorted so that all dynamic
//   relocations for a given dynamic symbol are adjacent.
//
//   Other than that, the linker sorts together all relative relocations,
//   which don't have symbols. Two relative relocations, or two relocations
//   against the same symbol, are sorted by the address in the output
//   file. This tends to optimize paging and caching when there are two
//   references from the same page.
pub fn sort<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    // .rela.dyn contains APS2-encoded bytes, not ElfRel entries.
    if ctx.args.pack_dyn_relocs_android {
        return;
    }

    // We group IFUNC relocations at the end of .rel.dyn because we want to
    // apply all the other relocations before running user-supplied IFUNC
    // resolvers.
    let rank = |r_type: u32| -> u32 {
        if r_type == E::R_RELATIVE {
            0
        } else if Some(r_type) == E::R_IRELATIVE {
            2
        } else {
            1
        }
    };
    let relocs = rels_from_bytes_mut::<E>(buf);
    relocs.par_sort_by_key(|r| (rank(r.r_type()), r.r_sym(), r.r_offset()));
}

// Encode dynamic relocations using the Android Packed Relocation format
// (APS2). The encoded stream begins with the magic bytes "APS2" followed
// by SLEB128-encoded fields. Relocations are emitted in groups; within a
// group, common offset deltas, info values, and addend deltas are
// factored out so each per-relocation entry is just the differing fields.
//
// See bionic's linker/linker_relocs.cpp for the decoder.
pub fn encode_android<E: Arch>(mut rels: Vec<ElfRel<E>>) -> Vec<u8> {
    const GROUPED_BY_INFO: i64 = 1;
    const GROUPED_BY_OFFSET_DELTA: i64 = 2;
    const GROUP_HAS_ADDEND: i64 = 8;

    let r_info = |r: &ElfRel<E>| -> i64 {
        if E::IS_64 {
            (((r.r_sym() as u64) << 32) | r.r_type() as u64) as i64
        } else {
            (((r.r_sym() as u64) << 8) | (r.r_type() & 0xff) as u64) as i64
        }
    };

    let mut buf = b"APS2".to_vec();
    encode_sleb(&mut buf, rels.len() as i64);
    encode_sleb(&mut buf, 0); // initial offset state
    if rels.is_empty() {
        return buf;
    }

    // APS2 offset deltas are signed, so the format doesn't require
    // relocations to be in increasing-offset order. Sort by (r_type,
    // r_sym, r_offset) so all relocs of the dominant type (typically
    // R_RELATIVE, ~90% of dynrels in a real Android binary) land in one
    // contiguous block. That collapses dozens of type-broken groups into
    // a few large info-grouped runs.
    rels.sort_by_key(|r| (r.r_type(), r.r_sym(), r.r_offset()));

    let mut prev_offset = 0i64;
    let mut prev_addend = 0i64;
    let mut i = 0;
    while i < rels.len() {
        let offset_delta = rels[i].r_offset() as i64 - prev_offset;
        let cur_info = r_info(&rels[i]);

        // Greedily extend the group while consecutive relocations share the
        // same offset_delta and r_info.
        let mut j = i + 1;
        while j < rels.len()
            && r_info(&rels[j]) == cur_info
            && rels[j].r_offset() as i64 - rels[j - 1].r_offset() as i64 == offset_delta
        {
            j += 1;
        }

        let size = (j - i) as i64;
        let mut flags = 0;
        if size > 1 {
            flags = GROUPED_BY_INFO | GROUPED_BY_OFFSET_DELTA;
        }
        if E::IS_RELA {
            flags |= GROUP_HAS_ADDEND;
        }
        encode_sleb(&mut buf, size);
        encode_sleb(&mut buf, flags);
        encode_sleb(&mut buf, offset_delta);
        encode_sleb(&mut buf, cur_info);

        if E::IS_RELA {
            for r in &rels[i..j] {
                encode_sleb(&mut buf, r.r_addend() - prev_addend);
                prev_addend = r.r_addend();
            }
        }
        prev_offset = rels[j - 1].r_offset() as i64;
        i = j;
    }
    buf
}
