//! __TEXT,__init_offsets: 32-bit image-relative initializer offsets,
//! replacing __mod_init_func's absolute pointers.

use mold_common::error;

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;

/// __TEXT,__init_offsets: 32-bit image-relative initializer offsets,
/// replacing __mod_init_func's absolute pointers.
#[derive(Debug)]
pub struct InitOffsetsSection {
    pub hdr: ChunkHeader,
    /// Initializer targets in run order.
    pub init_funcs: Vec<InitFunc>,
}

/// An initializer function: a subsection and the function's offset in
/// it, or a symbol with no offset in the image (one dyld binds, or an
/// absolute one).
#[derive(Clone, Copy, Debug)]
pub enum InitFunc {
    Local(usize, u64),
    Imported(SymbolId),
}

impl InitFunc {
    /// The initializer symbol `id` is. One dyld binds, or an absolute
    /// one, has no offset in the image: the link fails as it is written
    /// (see copy_buf).
    pub fn new<E: Target>(ctx: &Context<E>, id: SymbolId) -> Self {
        let sym = &ctx.symbols[id];
        match sym.input_section() {
            Some(isec) => InitFunc::Local(ctx.isecs.resolve(isec as usize), sym.value),
            None => InitFunc::Imported(id),
        }
    }
}

impl InitOffsetsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__init_offsets");
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

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    ctx.init_offsets.hdr.size = ctx.init_offsets.init_funcs.len() as u64 * 4;
}

/// Writes the offsets. An initializer dyld binds has no offset in the
/// image: that is an error.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for (i, &func) in ctx.init_offsets.init_funcs.iter().enumerate() {
        let val = match func {
            InitFunc::Local(isec, off) => {
                ctx.isecs[isec].addr(ctx) + off - ctx.mach_header.hdr.addr
            }
            InitFunc::Imported(id) => {
                error!(
                    "__init_offsets entry {i}: target '{}' does not have address",
                    ctx.symbols[id]
                );
                continue;
            }
        };
        buf[i * 4..i * 4 + 4].copy_from_slice(&(val as u32).to_le_bytes());
    }
}
