//! LC_FUNCTION_STARTS data: delta-encoded function addresses, used by
//! debuggers and crash reporters.

use rayon::prelude::*;

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_files::FileId;
use crate::target::Target;
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
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

impl Default for FunctionStartsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.function_starts.contents;
    buf[..data.len()].copy_from_slice(data);
}

pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    if !ctx.args.function_starts {
        return Vec::new();
    }
    let mut addrs: Vec<u64> = ctx
        .symbols
        .syms
        .par_iter()
        .filter_map(|sym| {
            if !matches!(sym.file(), Some(FileId::Obj(_))) {
                return None;
            }
            let isec = &ctx.isecs[ctx.resolve_isec(sym.input_section()? as usize)];
            if isec.is_alive()
                && ctx.hdr_of(isec).segname() == "__TEXT"
                && ctx.hdr_of(isec).sectname() == "__text"
            {
                Some(
                    ctx.chunk_header(isec.output_section().unwrap()).addr
                        + isec.offset as u64
                        + sym.value,
                )
            } else {
                None
            }
        })
        .collect();
    if addrs.is_empty() {
        return Vec::new();
    }
    addrs.par_sort_unstable();
    addrs.dedup();

    let mut buf = Vec::new();
    let mut last = ctx.args.pagezero_size;
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
