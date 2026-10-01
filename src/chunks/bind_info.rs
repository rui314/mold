//! The LC_DYLD_INFO bind opcode stream: every slot dyld fills with an
//! import.

use crate::chunks::{ChunkHeader, segment_and_offset};
use crate::context::Context;
use crate::macho::*;
use crate::target::{RelocClass, Target};
use crate::util::encode_uleb;

/// The bind opcode stream: every slot dyld fills with an import.
#[derive(Debug)]
pub struct BindInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl BindInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

impl Default for BindInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.bind_info.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// Builds the bind opcode stream: it tells dyld which imported symbol to
/// write into each GOT slot. Runs during layout, once every segment
/// before __LINKEDIT has an address.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let mut binds: Vec<(u64, crate::symbol::SymbolId, i64)> = Vec::new();

    // GOT slots for imported symbols.
    {
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if ctx.binds_as_import(id) {
                binds.push((ctx.got.slot_addr(i), id, 0));
            }
        }
    }

    // Pointers in data sections initialized with an imported symbol's
    // address.
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
            if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel)
                && ctx.binds_as_import(id)
                && !ctx.is_swift_force_load_ref(id)
            {
                binds.push((base + rel.offset as u64, id, rel.addend));
            }
        }
    }

    if binds.is_empty() {
        return Vec::new();
    }
    // ld64 lists the binds by library, symbol, addend, then address, so
    // each library and symbol is set once.
    let ordinal = |id| ctx.sym_bind_ordinal(id);
    binds.sort_by(|a, b| {
        (ordinal(a.1), ctx.symbols[a.1].name().as_bytes(), a.2, a.0).cmp(&(
            ordinal(b.1),
            ctx.symbols[b.1].name().as_bytes(),
            b.2,
            b.0,
        ))
    });
    let flags = |id: crate::symbol::SymbolId| {
        if ctx.symbols[id].is_weak_ref() { BIND_SYMBOL_FLAGS_WEAK_IMPORT } else { 0 }
    };
    encode(compress(bind_ops(ctx, &binds, |id| Some(ordinal(id)), flags)), Vec::new())
}

/// Encodes bind opcodes after `buf`'s, ending the stream.
pub(crate) fn encode(ops: Vec<Op>, mut buf: Vec<u8>) -> Vec<u8> {
    for op in ops {
        match op {
            Op::Dylib(ord) if ord <= 0 => {
                buf.push(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM | (ord & 0xf) as u8)
            }
            Op::Dylib(ord) if ord < 16 => buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | ord as u8),
            Op::Dylib(ord) => {
                buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
                encode_uleb(&mut buf, ord as u64);
            }
            Op::Symbol(name, flags) => {
                buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
                buf.extend_from_slice(name.as_bytes());
                buf.push(0);
            }
            Op::Type => buf.push(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER),
            Op::SegOffset(seg, off) => {
                buf.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
                encode_uleb(&mut buf, off);
            }
            Op::AddAddr(delta) => {
                buf.push(BIND_OPCODE_ADD_ADDR_ULEB);
                encode_uleb(&mut buf, delta);
            }
            Op::Addend(addend) => {
                buf.push(BIND_OPCODE_SET_ADDEND_SLEB);
                crate::util::encode_sleb(&mut buf, addend);
            }
            Op::Bind => buf.push(BIND_OPCODE_DO_BIND),
            Op::BindAddAddr(delta) if delta < 15 * 8 && delta % 8 == 0 => {
                buf.push(BIND_OPCODE_DO_BIND_ADD_ADDR_IMM_SCALED | (delta / 8) as u8)
            }
            Op::BindAddAddr(delta) => {
                buf.push(BIND_OPCODE_DO_BIND_ADD_ADDR_ULEB);
                encode_uleb(&mut buf, delta);
            }
            Op::BindTimesSkipping(count, skip) => {
                buf.push(BIND_OPCODE_DO_BIND_ULEB_TIMES_SKIPPING_ULEB);
                encode_uleb(&mut buf, count);
                encode_uleb(&mut buf, skip);
            }
        }
    }

    buf.push(BIND_OPCODE_DONE);
    while !buf.len().is_multiple_of(8) {
        buf.push(0);
    }
    buf
}

/// A bind opcode before encoding.
pub(crate) enum Op {
    Dylib(i32),
    Symbol(&'static str, u8),
    Type,
    SegOffset(usize, u64),
    AddAddr(u64),
    Addend(i64),
    Bind,
    BindAddAddr(u64),
    BindTimesSkipping(u64, u64),
}

/// The opcodes binding the sorted `binds`, each piece of the bind
/// machine's state set only when it changes, as ld-prime writes them:
/// the address moves by ADD_ADDR_ULEB within a segment, backwards too,
/// and by SET_SEGMENT_AND_OFFSET_ULEB into another one. `ordinal` is the
/// library each symbol binds to, if the stream names one, and `flags`
/// the flags its name goes with.
pub(crate) fn bind_ops<E: Target>(
    ctx: &Context<E>,
    binds: &[(u64, crate::symbol::SymbolId, i64)],
    ordinal: impl Fn(crate::symbol::SymbolId) -> Option<i32>,
    flags: impl Fn(crate::symbol::SymbolId) -> u8,
) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut cur_ordinal = None;
    let mut cur_symbol = None;
    let mut cur_seg = None;
    let mut cur_addr = 0;
    let mut cur_addend = 0;
    for &(addr, id, addend) in binds {
        let sym = &ctx.symbols[id];
        if let Some(ord) = ordinal(id)
            && cur_ordinal != Some(ord)
        {
            ops.push(Op::Dylib(ord));
            cur_ordinal = Some(ord);
        }
        let flags = flags(id);
        if cur_symbol != Some((id, flags)) {
            ops.push(Op::Symbol(sym.name(), flags));
            if cur_symbol.is_none() {
                ops.push(Op::Type);
            }
            cur_symbol = Some((id, flags));
        }
        if cur_seg.is_none() || addr != cur_addr {
            let (seg, off) = segment_and_offset(ctx, addr);
            if cur_seg == Some(seg) {
                ops.push(Op::AddAddr(addr.wrapping_sub(cur_addr)));
            } else {
                ops.push(Op::SegOffset(seg, off));
                cur_seg = Some(seg);
            }
        }
        if addend != cur_addend {
            ops.push(Op::Addend(addend));
            cur_addend = addend;
        }
        ops.push(Op::Bind);
        cur_addr = addr + 8;
    }
    ops
}

/// ld64's compression of a bind opcode list: a bind followed by an
/// address step becomes one opcode, and a run of those with one step
/// becomes DO_BIND_ULEB_TIMES_SKIPPING_ULEB. (Encoding then writes a
/// small, pointer-aligned step as DO_BIND_ADD_ADDR_IMM_SCALED.)
pub(crate) fn compress(ops: Vec<Op>) -> Vec<Op> {
    let mut paired = Vec::with_capacity(ops.len());
    let mut it = ops.into_iter().peekable();
    while let Some(op) = it.next() {
        match (op, it.peek()) {
            (Op::Bind, Some(&Op::AddAddr(delta))) => {
                it.next();
                paired.push(Op::BindAddAddr(delta));
            }
            (op, _) => paired.push(op),
        }
    }
    let mut out = Vec::with_capacity(paired.len());
    let mut it = paired.into_iter().peekable();
    while let Some(op) = it.next() {
        match (op, it.peek()) {
            (Op::BindAddAddr(delta), Some(&Op::BindAddAddr(next))) if next == delta => {
                let mut count = 1;
                while let Some(&Op::BindAddAddr(next)) = it.peek()
                    && next == delta
                {
                    it.next();
                    count += 1;
                }
                out.push(Op::BindTimesSkipping(count, delta));
            }
            (op, _) => out.push(op),
        }
    }
    out
}
