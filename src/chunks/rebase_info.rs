//! The LC_DYLD_INFO rebase opcode stream: every pointer dyld slides.
//! mold's reldyn.rs holds the ELF relative relocations it stands in for.

use crate::chunks::{ChunkHeader, segment_and_offset};
use crate::context::Context;
use crate::macho::*;
use crate::passes::{DataField, objc_ref_addr};
use crate::target::{RelocClass, Target};
use crate::util::encode_uleb;

/// The rebase opcode stream: every pointer dyld slides.
#[derive(Debug)]
pub struct RebaseInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl RebaseInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

impl Default for RebaseInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.rebase_info.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// Builds the rebase opcode stream: it tells dyld which pointers in the
/// image it must slide when the image is loaded at a non-default address.
/// Every absolute address the linker writes into a data section gets a
/// record. Runs during layout, once every segment before __LINKEDIT has
/// an address.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let mut locs: Vec<u64> = Vec::new();

    // Pointers written for UNSIGNED relocations to local targets.
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || isec.replacement != crate::input_sections::NO_REPLACEMENT {
            continue;
        }
        let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            if E::classify_reloc(rel.r_type) != RelocClass::Plain
                || rel.size != 8
                || rel.is_pcrel
                || rel.is_subtracted
                || rel.r_type == E::RELOC_SUBTRACTOR
            {
                continue;
            }
            // Pointers to thread-local data are thread-pointer-relative
            // offsets, not addresses, so they are not rebased.
            let imported = ctx
                .reloc_target_sym(isec.file as usize, rel)
                .is_some_and(|id| ctx.symbols[id].is_imported());
            let absolute = ctx
                .reloc_target_sym(isec.file as usize, rel)
                .is_some_and(|id| ctx.is_absolute_symbol(id));
            if !imported && !absolute && !ctx.reloc_target_is_tls(isec.file as usize, rel) {
                locs.push(base + rel.offset as u64);
            }
        }
    }

    // Synthesized selector reference slots hold pointers into
    // __objc_methname (a reused input slot has its own relocation).
    for i in 0..ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len() {
        if !ctx.objc_stub_reuses_selref(i) {
            locs.push(ctx.objc_selref_addr(i));
        }
    }
    // Pointer fields of the synthesized Objective-C records.
    for (addr, _) in data_blob_pointers(ctx) {
        locs.push(addr);
    }
    // Lazy pointers start out pointing at their stub helper entries (a
    // weak-lookup stub's GOT slot is rebased with the GOT).
    if ctx.lazy_binding() {
        for i in 0..ctx.stubs.symbols.len() {
            if !ctx.binds_weak_lookup(ctx.stubs.symbols[i]) {
                locs.push(ctx.stub_ptr_addr(i, ctx.stubs.symbols[i]));
            }
        }
    }

    // GOT slots that hold local addresses.
    {
        let got_addr = ctx.got.hdr.addr;
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if !ctx.symbols[id].is_imported() && !ctx.is_absolute_symbol(id) {
                locs.push(got_addr + i as u64 * 8);
            }
        }
    }

    if locs.is_empty() {
        return Vec::new();
    }
    locs.sort_unstable();

    // Rebase locations cluster (pointer arrays, vtables), and the
    // opcodes have run-length forms for exactly that: a run of
    // adjacent pointers becomes one DO_REBASE_*_TIMES, and since the
    // state machine's address advances past each rebased slot, a gap
    // within a segment costs only an ADD_ADDR_ULEB. ld64 compresses
    // the same way; one SET_SEGMENT per pointer made this stream
    // over 20x larger.
    let mut buf = Vec::new();
    buf.push(REBASE_OPCODE_SET_TYPE_IMM | REBASE_TYPE_POINTER);
    let mut cur: Option<(u8, u64)> = None;
    let mut i = 0;
    while i < locs.len() {
        let (seg, off) = segment_and_offset(ctx, locs[i]);
        match cur {
            Some((cseg, coff)) if cseg == seg as u8 && off >= coff => {
                if off > coff {
                    buf.push(REBASE_OPCODE_ADD_ADDR_ULEB);
                    encode_uleb(&mut buf, off - coff);
                }
            }
            _ => {
                buf.push(REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
                encode_uleb(&mut buf, off);
            }
        }

        // Extend the run over adjacent 8-byte slots.
        let mut n = 1u64;
        while i + (n as usize) < locs.len() && locs[i + n as usize] == locs[i] + n * 8 {
            n += 1;
        }
        if n <= 15 {
            buf.push(REBASE_OPCODE_DO_REBASE_IMM_TIMES | n as u8);
        } else {
            buf.push(REBASE_OPCODE_DO_REBASE_ULEB_TIMES);
            encode_uleb(&mut buf, n);
        }
        cur = Some((seg as u8, off + n * 8));
        i += n as usize;
    }
    buf.push(REBASE_OPCODE_DONE);
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    buf
}

/// The (address, target) of every non-null pointer field of the
/// synthesized Objective-C records: each is a rebase.
pub fn data_blob_pointers<E: Target>(ctx: &Context<E>) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for b in &ctx.data_blobs {
        let mut at = ctx.isec_addr(b.isec as usize);
        for f in &b.fields {
            match f {
                DataField::Bytes(bytes) => at += bytes.len() as u64,
                DataField::Ptr(r) => {
                    let target = objc_ref_addr(ctx, *r);
                    if target != 0 {
                        out.push((at, target));
                    }
                    at += 8;
                }
            }
        }
    }
    out
}
