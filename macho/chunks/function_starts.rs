//! LC_FUNCTION_STARTS data: delta-encoded function addresses, used by
//! debuggers and crash reporters.

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_files::FileId;
use crate::macho::S_ATTR_PURE_INSTRUCTIONS;
use crate::util::encode_uleb;

/// LC_FUNCTION_STARTS data: delta-encoded function addresses, used by
/// debuggers and crash reporters.
#[derive(Debug)]
pub struct FunctionStartsSection {
    pub hdr: ChunkHeader,
    /// The encoded table, built during layout.
    pub contents: Vec<u8>,
}

impl FunctionStartsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self { hdr, contents: Vec::new() }
    }
}

impl Default for FunctionStartsSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Lists, as ld-prime does, the start of each piece it splits the
/// image's code into: each non-empty subsection of an output section
/// of pure instructions (__text with the __StaticInit static
/// initializers merged into it, or any other), each symbol inside one -
/// a local, an alt entry or an l-prefixed label too, but not a label at
/// its end - and each thunk entry, ld-prime's branch islands. The stubs
/// of every kind, sections of their own, are left out.
pub fn construct<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    if !ctx.args.function_starts {
        return Vec::new();
    }
    let is_code = |hdr: &ChunkHeader| hdr.flags & S_ATTR_PURE_INSTRUCTIONS != 0;
    let starts =
        ctx.output_sections.par_iter().filter(|osec| is_code(&osec.hdr)).flat_map(|osec| {
            let base = osec.hdr.addr;
            let isecs = osec.members.par_iter().filter_map(move |&id| {
                let isec = &ctx.isecs[id as usize];
                (isec.size != 0 && isec.is_emitted()).then_some(base + isec.offset as u64)
            });
            let thunks = osec.thunks.par_iter().flat_map_iter(move |thunk| {
                (0..thunk.syms.len() as u64).map(move |i| base + thunk.offset + i * E::THUNK_SIZE)
            });
            isecs.chain(thunks)
        });
    // The labels inside the subsections. One at a subsection's start
    // (the usual case: subsections split at symbols) repeats its entry.
    let syms = ctx.symbols.syms.par_iter().filter_map(|sym| {
        if sym.value == 0 || !matches!(sym.file(), Some(FileId::Obj(_))) {
            return None;
        }
        let isec = &ctx.isecs[ctx.isecs.resolve(sym.input_section()? as usize)];
        let osec = ctx.chunk_header(isec.output_section()?);
        (isec.is_alive() && is_code(osec) && sym.value < isec.size as u64)
            .then(|| osec.addr + isec.offset as u64 + sym.value)
    });
    let mut addrs: Vec<u64> = starts.chain(syms).collect();
    // No functions: the terminator alone, padded like any table.
    if addrs.is_empty() {
        return vec![0; 8];
    }
    addrs.par_sort_unstable();
    addrs.dedup();

    let mut buf = Vec::new();
    // Offsets count from the image's start, the mach header, which
    // -image_base (a kernel's) moves.
    let mut last = ctx.mach_header.hdr.addr;
    for addr in addrs {
        encode_uleb(&mut buf, addr - last);
        last = addr;
    }
    buf.push(0);
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    buf
}
