//! The LC_DYLD_INFO rebase opcode stream: every pointer dyld slides.
//! mold's reldyn.rs holds the ELF relative relocations it stands in for.

use crate::chunks::{ChunkHeader, segment_and_offset};
use crate::context::Context;
use crate::macho::*;
use crate::objc::{DataField, ObjcRef, objc_ref_addr};
use crate::symbol::SymbolId;
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

/// Every pointer a loader must slide when the image lands at another
/// address than its own: the pointers written for absolute relocations
/// to local targets, then the synthesized ones. Unsorted. The rebase
/// stream describes them to dyld, a -static -pie image's local
/// relocations to whatever loads it, and legacy LINKEDIT's to dyld.
pub fn rebase_locations<E: Target>(ctx: &Context<E>) -> Vec<u64> {
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
                .is_some_and(|id| ctx.binds_pointer(id) || ctx.is_dtrace_pointer_target(id));
            let absolute = ctx
                .reloc_target_sym(isec.file as usize, rel)
                .is_some_and(|id| ctx.is_absolute_symbol(id));
            if !imported && !absolute && !ctx.reloc_target_is_tls(isec.file as usize, rel) {
                locs.push(base + rel.offset as u64);
            }
        }
    }

    // Synthesized selector reference slots hold pointers into
    // __objc_methname.
    for i in 0..ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len() {
        locs.push(ctx.objc_selref_addr(i));
    }
    // Pointer fields of the synthesized Objective-C records.
    for (addr, _) in data_blob_pointers(ctx) {
        locs.push(addr);
    }
    // Lazy pointers start out pointing at their stub helper entries (a
    // weak-lookup stub's GOT slot is rebased with the GOT).
    for &i in &ctx.stubs.lazy {
        let i = i as usize;
        locs.push(ctx.stub_ptr_addr(i, ctx.stubs.symbols[i]));
    }

    // GOT slots that hold local addresses. (Legacy LINKEDIT's dyld
    // slides those the indirect symbol table marks local itself, and
    // binds the others by name.)
    if !ctx.args.legacy_linkedit {
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if !ctx.binds_as_import(id) && !ctx.is_absolute_symbol(id) {
                locs.push(ctx.got.slot_addr(i));
            }
        }
    }
    locs
}

/// Whether nothing ever slides the image: a non-PIE executable, which
/// the kernel maps where its segments say and dyld leaves there.
/// ld-prime records no rebases for one, in opcodes or in chains. (A
/// -static image keeps them for whatever loads it.)
pub fn is_never_slid<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.output_type == MH_EXECUTE && !ctx.args.pie && !ctx.args.static_link
}

/// Builds the rebase opcode stream: it tells dyld which pointers in the
/// image it must slide when the image is loaded at a non-default address.
/// Every absolute address the linker writes into a data section gets a
/// record. Runs during layout, once every segment before __LINKEDIT has
/// an address.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    if is_never_slid(ctx) {
        return Vec::new();
    }
    let mut locs = rebase_locations(ctx);
    if locs.is_empty() {
        return Vec::new();
    }
    locs.sort_unstable();

    let mut buf = vec![REBASE_OPCODE_SET_TYPE_IMM | REBASE_TYPE_POINTER];
    for op in compress(rebase_ops(ctx, &locs)) {
        match op {
            Op::SegOffset(seg, off) => {
                buf.push(REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
                encode_uleb(&mut buf, off);
            }
            Op::AddAddr(delta) if delta < 15 * 8 && delta % 8 == 0 => {
                buf.push(REBASE_OPCODE_ADD_ADDR_IMM_SCALED | (delta / 8) as u8)
            }
            Op::AddAddr(delta) => {
                buf.push(REBASE_OPCODE_ADD_ADDR_ULEB);
                encode_uleb(&mut buf, delta);
            }
            Op::Rebase(count) if count < 15 => {
                buf.push(REBASE_OPCODE_DO_REBASE_IMM_TIMES | count as u8)
            }
            Op::Rebase(count) => {
                buf.push(REBASE_OPCODE_DO_REBASE_ULEB_TIMES);
                encode_uleb(&mut buf, count);
            }
            Op::RebaseAddAddr(delta) => {
                buf.push(REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB);
                encode_uleb(&mut buf, delta);
            }
            Op::RebaseTimesSkipping(count, skip) => {
                buf.push(REBASE_OPCODE_DO_REBASE_ULEB_TIMES_SKIPPING_ULEB);
                encode_uleb(&mut buf, count);
                encode_uleb(&mut buf, skip);
            }
        }
    }
    // ld64 writes no DONE: the zeros padding the stream to 8 bytes
    // read as one, and a stream that fills its last 8 bytes simply
    // ends there.
    while buf.len() % 8 != 0 {
        buf.push(REBASE_OPCODE_DONE);
    }
    buf
}

/// A rebase opcode before encoding.
#[derive(Clone, Copy)]
enum Op {
    SegOffset(usize, u64),
    AddAddr(u64),
    Rebase(u64),
    RebaseAddAddr(u64),
    RebaseTimesSkipping(u64, u64),
}

/// The opcodes rebasing the sorted `locs`: the address moves by
/// ADD_ADDR_ULEB within a segment and by SET_SEGMENT_AND_OFFSET_ULEB
/// into another one, and a run of adjacent pointers is one
/// DO_REBASE_ULEB_TIMES, since each rebase advances the address past
/// its slot. A run ends at its segment's end, where dyld's bounds
/// check would reject a rebase past it.
fn rebase_ops<E: Target>(ctx: &Context<E>, locs: &[u64]) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut seg_start = 0;
    let mut seg_end = 0;
    let mut cur_addr = 0;
    let mut i = 0;
    while i < locs.len() {
        let addr = locs[i];
        if addr < seg_start || seg_end <= addr {
            let (seg, off) = segment_and_offset(ctx, addr);
            seg_start = addr - off;
            seg_end = seg_start + ctx.segments[seg].cmd.vmsize;
            ops.push(Op::SegOffset(seg, off));
        } else if addr != cur_addr {
            ops.push(Op::AddAddr(addr.wrapping_sub(cur_addr)));
        }
        let mut n = 1;
        while i + n < locs.len() && locs[i + n] == addr + n as u64 * 8 && locs[i + n] < seg_end {
            n += 1;
        }
        ops.push(Op::Rebase(n as u64));
        cur_addr = addr + n as u64 * 8;
        i += n;
    }
    ops
}

/// ld64's compression of a rebase opcode list: a single rebase followed
/// by an address step becomes one DO_REBASE_ADD_ADDR_ULEB, and three or
/// more of those with one step become DO_REBASE_ULEB_TIMES_SKIPPING_ULEB.
/// (Encoding then writes a small, pointer-aligned step as
/// ADD_ADDR_IMM_SCALED and a short run as DO_REBASE_IMM_TIMES.)
fn compress(ops: Vec<Op>) -> Vec<Op> {
    let mut paired = Vec::with_capacity(ops.len());
    let mut it = ops.into_iter().peekable();
    while let Some(op) = it.next() {
        match (op, it.peek()) {
            (Op::Rebase(1), Some(&Op::AddAddr(delta))) => {
                it.next();
                paired.push(Op::RebaseAddAddr(delta));
            }
            (op, _) => paired.push(op),
        }
    }
    let mut out = Vec::with_capacity(paired.len());
    let mut i = 0;
    while i < paired.len() {
        if let Op::RebaseAddAddr(delta) = paired[i] {
            let count = paired[i..]
                .iter()
                .take_while(|op| matches!(op, Op::RebaseAddAddr(d) if *d == delta))
                .count();
            if count >= 3 {
                out.push(Op::RebaseTimesSkipping(count as u64, delta));
                i += count;
                continue;
            }
        }
        out.push(paired[i]);
        i += 1;
    }
    out
}

/// The (address, target) of every pointer field of the synthesized
/// records (see objc::DataBlob).
fn data_blob_fields<E: Target>(ctx: &Context<E>) -> Vec<(u64, ObjcRef)> {
    let mut out = Vec::new();
    for b in &ctx.data_blobs {
        let mut at = ctx.isec_addr(b.isec as usize);
        for f in &b.fields {
            match f {
                DataField::Bytes(bytes) => at += bytes.len() as u64,
                DataField::Ptr(r) => {
                    out.push((at, *r));
                    at += 8;
                }
            }
        }
    }
    out
}

/// The (address, target) of every non-null pointer field of the
/// synthesized records into the image: each is a rebase.
pub fn data_blob_pointers<E: Target>(ctx: &Context<E>) -> Vec<(u64, u64)> {
    let fields = data_blob_fields(ctx).into_iter().filter(|(_, r)| r.import(ctx).is_none());
    fields.map(|(at, r)| (at, objc_ref_addr(ctx, r))).filter(|&(_, target)| target != 0).collect()
}

/// The (address, symbol) of every pointer field of the synthesized
/// records to an import: each is a bind.
pub fn data_blob_binds<E: Target>(ctx: &Context<E>) -> Vec<(u64, SymbolId)> {
    data_blob_fields(ctx).into_iter().filter_map(|(at, r)| Some((at, r.import(ctx)?))).collect()
}
