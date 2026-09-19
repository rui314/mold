//! The LC_DYLD_INFO opcode streams in __LINKEDIT: rebase, bind, weak
//! bind and lazy bind. mold-rust's dynamic.rs holds the ELF dynamic
//! relocation tables they stand in for.

use crate::macho::arch::{Arch, RelocClass};
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;
use crate::macho::output_chunks::ChunkHeader;
use crate::macho::passes::{DataField, objc_ref_addr};
use crate::util::encode_uleb;

/// The rebase opcode stream: every pointer dyld slides.
#[derive(Debug)]
pub struct RebaseInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl RebaseInfoSection {
    pub fn new() -> RebaseInfoSection {
        RebaseInfoSection { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

pub mod rebase_info {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let data = &ctx.rebase_info.contents;
        buf[..data.len()].copy_from_slice(data);
    }
}

/// The bind opcode stream: every slot dyld fills with an import.
#[derive(Debug)]
pub struct BindInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl BindInfoSection {
    pub fn new() -> BindInfoSection {
        BindInfoSection { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

pub mod bind_info {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let data = &ctx.bind_info.contents;
        buf[..data.len()].copy_from_slice(data);
    }
}

/// The weak-bind opcode stream: the slots dyld redirects when another
/// image's copy of one of this image's weak definitions wins
/// coalescing.
#[derive(Debug)]
pub struct WeakBindInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl WeakBindInfoSection {
    pub fn new() -> WeakBindInfoSection {
        WeakBindInfoSection { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

pub mod weak_bind_info {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let data = &ctx.weak_bind_info.contents;
        buf[..data.len()].copy_from_slice(data);
    }
}

/// The lazy-bind opcode stream: one record per lazy pointer, entered
/// by its stub helper on first call.
#[derive(Debug)]
pub struct LazyBindInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout, and each stub's record offset
    /// in it (what its stub helper entry pushes for dyld_stub_binder).
    pub contents: Vec<u8>,
    pub offsets: Vec<u32>,
}

impl LazyBindInfoSection {
    pub fn new() -> LazyBindInfoSection {
        LazyBindInfoSection {
            hdr: ChunkHeader::linkedit(),
            contents: Vec::new(),
            offsets: Vec::new(),
        }
    }
}

pub mod lazy_bind_info {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let data = &ctx.lazy_bind_info.contents;
        buf[..data.len()].copy_from_slice(data);
    }
}

/// Returns the load-command index of the segment containing `addr`, and
/// the offset within it.
fn segment_and_offset<E: Arch>(ctx: &Context<E>, addr: u64) -> (usize, u64) {
    for (i, seg) in ctx.segments.iter().enumerate() {
        if seg.cmd.vmaddr <= addr
            && addr < seg.cmd.vmaddr + seg.cmd.vmsize
            && seg.name != "__PAGEZERO"
        {
            return (i, addr - seg.cmd.vmaddr);
        }
    }
    unreachable!("no segment contains address {addr:#x}");
}

/// Builds the rebase opcode stream: it tells dyld which pointers in the
/// image it must slide when the image is loaded at a non-default address.
/// Every absolute address the linker writes into a data section gets a
/// record. Runs during layout, once every segment before __LINKEDIT has
/// an address.
pub fn build_rebase_info<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let mut locs: Vec<u64> = Vec::new();

    // Pointers written for UNSIGNED relocations to local targets.
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || isec.replacement != crate::macho::input_sections::NO_REPLACEMENT {
            continue;
        }
        let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
        for rel in crate::macho::input_files::isec_relocs_of(&ctx.objs, isec) {
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

    // __thread_ptrs slots hold descriptor addresses, which need
    // sliding.
    {
        let addr = ctx.thread_ptrs.hdr.addr;
        for (i, &id) in ctx.thread_ptrs.symbols.iter().enumerate() {
            if !ctx.symbols[id].is_imported() {
                locs.push(addr + i as u64 * 8);
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

/// Builds the bind opcode stream: it tells dyld which imported symbol to
/// write into each GOT slot. Runs during layout, once every segment
/// before __LINKEDIT has an address.
/// The lazy-bind opcode stream: one self-contained record per stub
/// (segment/offset of its lazy pointer, dylib ordinal, symbol, bind,
/// done), and each record's offset, which the stub helper entry pushes
/// for dyld_stub_binder. ld64's layout, byte for byte.
pub fn build_lazy_bind_info<E: Arch>(ctx: &Context<E>) -> (Vec<u8>, Vec<u32>) {
    if !ctx.lazy_binding() || ctx.stubs.symbols.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut buf = Vec::new();
    let mut offsets = Vec::with_capacity(ctx.stubs.symbols.len());
    for (i, &id) in ctx.stubs.symbols.iter().enumerate() {
        offsets.push(buf.len() as u32);
        // A stub for a symbol bound by weak lookup jumps through its
        // GOT slot, not lazily.
        if ctx.binds_weak_lookup(id) {
            continue;
        }
        let addr = ctx.stub_ptr_addr(i, id);
        let (seg, off) = segment_and_offset(ctx, addr);
        buf.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
        encode_uleb(&mut buf, off);
        let sym = &ctx.symbols[id];
        let Some(FileId::Dylib(dylib)) = sym.file() else { unreachable!() };
        let ordinal = ctx.bind_ordinal(dylib);
        if ordinal <= 0 {
            buf.push(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM | (ordinal & 0xf) as u8);
        } else if ordinal < 16 {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | ordinal as u8);
        } else {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
            encode_uleb(&mut buf, ordinal as u64);
        }
        let flags = if sym.is_weak_ref() { BIND_SYMBOL_FLAGS_WEAK_IMPORT } else { 0 };
        buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
        buf.extend_from_slice(sym.name().as_bytes());
        buf.push(0);
        buf.push(BIND_OPCODE_DO_BIND);
        buf.push(BIND_OPCODE_DONE);
    }
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    (buf, offsets)
}

pub fn build_bind_info<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let mut binds: Vec<(u64, crate::macho::symbol::SymbolId, i64)> = Vec::new();

    // GOT slots for imported symbols.
    {
        let got_addr = ctx.got.hdr.addr;
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if ctx.symbols[id].is_imported() {
                binds.push((got_addr + i as u64 * 8, id, 0));
            }
        }
    }

    // __thread_ptrs slots for thread-locals imported from dylibs: dyld
    // writes the foreign TLV descriptor's address.
    {
        let addr = ctx.thread_ptrs.hdr.addr;
        for (i, &id) in ctx.thread_ptrs.symbols.iter().enumerate() {
            if ctx.symbols[id].is_imported() {
                binds.push((addr + i as u64 * 8, id, 0));
            }
        }
    }

    // Pointers in data sections initialized with an imported symbol's
    // address.
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || isec.replacement != crate::macho::input_sections::NO_REPLACEMENT {
            continue;
        }
        let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
        for rel in crate::macho::input_files::isec_relocs_of(&ctx.objs, isec) {
            if E::classify_reloc(rel.r_type) != RelocClass::Plain
                || rel.size != 8
                || rel.is_pcrel
                || rel.is_subtracted
                || rel.r_type == E::RELOC_SUBTRACTOR
            {
                continue;
            }
            if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) {
                if ctx.symbols[id].is_imported() {
                    binds.push((base + rel.offset as u64, id, rel.addend));
                }
            }
        }
    }

    if binds.is_empty() {
        return Vec::new();
    }
    binds.sort_unstable_by_key(|&(addr, _, _)| addr);

    let mut buf = Vec::new();
    let mut last_addend = 0i64;
    for (addr, id, addend) in binds {
        let sym = &ctx.symbols[id];
        let Some(FileId::Dylib(dylib)) = sym.file() else { unreachable!() };
        let ordinal = ctx.bind_ordinal(dylib);
        // The special ordinals (main executable 0, flat lookup -2) take
        // the SPECIAL_IMM form, as ld64 emits them.
        if ordinal <= 0 {
            buf.push(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM | (ordinal & 0xf) as u8);
        } else if ordinal < 16 {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | ordinal as u8);
        } else {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
            encode_uleb(&mut buf, ordinal as u64);
        }
        let flags = if sym.is_weak_ref() { BIND_SYMBOL_FLAGS_WEAK_IMPORT } else { 0 };
        buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
        buf.extend_from_slice(sym.name().as_bytes());
        buf.push(0);
        buf.push(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER);
        // The addend is bind-machine state: it persists across
        // BIND opcodes, so emit SET_ADDEND_SLEB only on change.
        if addend != last_addend {
            buf.push(BIND_OPCODE_SET_ADDEND_SLEB);
            crate::util::encode_sleb(&mut buf, addend);
            last_addend = addend;
        }
        let (seg, off) = segment_and_offset(ctx, addr);
        buf.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
        encode_uleb(&mut buf, off);
        buf.push(BIND_OPCODE_DO_BIND);
    }

    buf.push(BIND_OPCODE_DONE);
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    buf
}

/// Collects every dynamic fixup location: rebases (the linker wrote an
/// absolute address that dyld must slide) and binds (dyld writes an
/// imported symbol's address), with the bind addends.
/// The (address, target) of every non-null pointer field of the
/// synthesized Objective-C records: each is a rebase.
pub fn data_blob_pointers<E: Arch>(ctx: &Context<E>) -> Vec<(u64, u64)> {
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

/// Builds the classic weak_bind stream: for every slot that holds the
/// address of one of this image's coalescable weak definitions - GOT
/// entries and data pointers - a bind by name, which dyld applies
/// only if another image's copy of the symbol won coalescing (the
/// slot's rebase already holds this image's copy). Sorted by symbol
/// name, then address, as ld64 writes them.
pub fn build_weak_bind_info<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let mut binds: Vec<(crate::macho::symbol::SymbolId, u64)> = Vec::new();
    {
        let got_addr = ctx.got.hdr.addr;
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if ctx.binds_weak_lookup(id) {
                binds.push((id, got_addr + i as u64 * 8));
            }
        }
    }
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || isec.replacement != crate::macho::input_sections::NO_REPLACEMENT {
            continue;
        }
        let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
        for rel in crate::macho::input_files::isec_relocs_of(&ctx.objs, isec) {
            if E::classify_reloc(rel.r_type) != RelocClass::Plain
                || rel.size != 8
                || rel.is_pcrel
                || rel.is_subtracted
                || rel.r_type == E::RELOC_SUBTRACTOR
            {
                continue;
            }
            if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) {
                if ctx.binds_weak_lookup(id) {
                    binds.push((id, base + rel.offset as u64));
                }
            }
        }
    }
    // Strong definitions overriding a dylib's weak export are listed
    // first, by name, flagged non-weak, with no location: dyld then
    // knows this image's copy wins coalescing.
    let mut overrides: Vec<crate::macho::symbol::SymbolId> =
        (0..ctx.symbols.syms.len() as u32).filter(|&i| ctx.overrides_weak_export(i)).collect();
    if binds.is_empty() && overrides.is_empty() {
        return Vec::new();
    }
    overrides.sort_by(|&a, &b| ctx.symbols[a].name().cmp(ctx.symbols[b].name()));
    binds.sort_by(|a, b| ctx.symbols[a.0].name().cmp(ctx.symbols[b.0].name()).then(a.1.cmp(&b.1)));

    let mut buf = Vec::new();
    for id in overrides {
        buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | BIND_SYMBOL_FLAGS_NON_WEAK_DEFINITION);
        buf.extend_from_slice(ctx.symbols[id].name().as_bytes());
        buf.push(0);
    }
    let mut last: Option<crate::macho::symbol::SymbolId> = None;
    for (id, addr) in binds {
        if last != Some(id) {
            buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM);
            buf.extend_from_slice(ctx.symbols[id].name().as_bytes());
            buf.push(0);
            buf.push(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER);
            last = Some(id);
        }
        let (seg, off) = segment_and_offset(ctx, addr);
        buf.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
        encode_uleb(&mut buf, off);
        buf.push(BIND_OPCODE_DO_BIND);
    }
    buf.push(BIND_OPCODE_DONE);
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    buf
}
