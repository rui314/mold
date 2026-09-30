//! __TEXT,__init_offsets: 32-bit image-relative initializer offsets,
//! replacing __mod_init_func's absolute pointers.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::target::Target;

/// __TEXT,__init_offsets: 32-bit image-relative initializer offsets,
/// replacing __mod_init_func's absolute pointers.
#[derive(Debug)]
pub struct InitOffsetsSection {
    pub hdr: ChunkHeader,
    /// Initializer targets in run order: the subsection and offset of
    /// each initializer function.
    pub init_funcs: Vec<(usize, u64)>,
}

impl InitOffsetsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__TEXT", "__init_offsets");
        hdr.flags = S_INIT_FUNC_OFFSETS;
        hdr.p2align = 2;
        Self { hdr, init_funcs: Vec::new() }
    }
}

impl Default for InitOffsetsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for (i, &(isec, off)) in ctx.init_offsets.init_funcs.iter().enumerate() {
        let val = (ctx.isec_addr(isec) + off - ctx.mach_header.hdr.addr) as u32;
        buf[i * 4..i * 4 + 4].copy_from_slice(&val.to_le_bytes());
    }
}
