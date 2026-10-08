//! The LC_DYLD_INFO bind opcode stream: every slot dyld fills with an
//! import.

use crate::arch::Target;
use crate::chunks::{ChunkHeader, rebase_info, segment_and_offset};
use crate::context::Context;
use crate::macho::*;
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

/// Builds the bind opcode stream: it tells dyld which imported symbol to
/// write into each GOT slot. Runs during layout, once every segment
/// before __LINKEDIT has an address.
pub fn construct<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let mut binds: Vec<(u64, crate::symbol::SymbolId, i64)> = Vec::new();

    // GOT slots for imported symbols.
    for (i, &id) in ctx.got.got_syms.iter().enumerate() {
        if ctx.symbols[id].binds_as_import(ctx) {
            binds.push((ctx.got.slot_addr(i), id, 0));
        }
    }

    // Pointers in data sections initialized with an imported symbol's
    // address.
    for isec in ctx.isecs.iter() {
        if !isec.is_emitted() {
            continue;
        }
        let file = &ctx.objs[isec.file as usize];
        for (addr, rel) in rebase_info::pointer_relocs(ctx, isec) {
            if let Some(id) = rel.sym(file)
                && (ctx.symbols[id].binds_as_import(ctx)
                    || ctx.symbols[id].is_dtrace_pointer_target())
            {
                binds.push((addr, id, rel.addend));
            }
        }
    }
    for (addr, id) in rebase_info::data_blob_binds(ctx) {
        binds.push((addr, id, 0));
    }

    if binds.is_empty() {
        return Vec::new();
    }
    // ld64 lists the binds by library, symbol, addend, then address, so
    // each library and symbol is set once.
    let ordinal = |id: crate::symbol::SymbolId| ctx.symbols[id].bind_ordinal(ctx);
    binds.sort_by(|a, b| {
        (ordinal(a.1), ctx.symbols[a.1].name(), a.2, a.0).cmp(&(
            ordinal(b.1),
            ctx.symbols[b.1].name(),
            b.2,
            b.0,
        ))
    });
    let flags = |id: crate::symbol::SymbolId| {
        if ctx.symbols[id].is_weak_ref() { BIND_SYMBOL_FLAGS_WEAK_IMPORT } else { 0 }
    };
    encode(bind_ops(ctx, &binds, |id| Some(ordinal(id)), flags))
}

/// Encodes a stream of bind opcodes, ending it.
pub(crate) fn encode(ops: Vec<Op>) -> Vec<u8> {
    let mut buf = Vec::new();
    for op in ops {
        encode_op(&mut buf, op);
    }
    buf.push(BIND_OPCODE_DONE);
    while !buf.len().is_multiple_of(8) {
        buf.push(0);
    }
    buf
}

/// Appends a bind opcode.
pub(crate) fn encode_op(buf: &mut Vec<u8>, op: Op) {
    match op {
        Op::Dylib(ord) if ord <= 0 => {
            buf.push(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM | (ord & 0xf) as u8)
        }
        Op::Dylib(ord) if ord < 16 => buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | ord as u8),
        Op::Dylib(ord) => {
            buf.push(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
            encode_uleb(buf, ord as u64);
        }
        Op::Symbol(name, flags) => {
            buf.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
            buf.extend_from_slice(name);
            buf.push(0);
        }
        Op::Type => buf.push(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER),
        Op::SegOffset(seg, off) => {
            buf.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
            encode_uleb(buf, off);
        }
        Op::AddAddr(delta) => {
            buf.push(BIND_OPCODE_ADD_ADDR_ULEB);
            encode_uleb(buf, delta);
        }
        Op::Addend(addend) => {
            buf.push(BIND_OPCODE_SET_ADDEND_SLEB);
            crate::util::encode_sleb(buf, addend);
        }
        Op::Bind => buf.push(BIND_OPCODE_DO_BIND),
    }
}

/// A bind opcode before encoding.
pub(crate) enum Op {
    Dylib(i32),
    Symbol(&'static [u8], u8),
    Type,
    SegOffset(usize, u64),
    AddAddr(u64),
    Addend(i64),
    Bind,
}

/// The opcodes binding the sorted `binds`, each piece of the bind
/// machine's state set only when it changes:
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
