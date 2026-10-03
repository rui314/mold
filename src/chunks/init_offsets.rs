//! __TEXT,__init_offsets: 32-bit image-relative initializer offsets,
//! replacing __mod_init_func's absolute pointers.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

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

/// Writes the offsets. An initializer dyld binds has no offset in the
/// image: that is an error.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for (i, &func) in ctx.init_offsets.init_funcs.iter().enumerate() {
        let val = match func {
            InitFunc::Local(isec, off) => ctx.isec_addr(isec) + off - ctx.mach_header.hdr.addr,
            InitFunc::Imported(id) => {
                crate::error!(
                    "__init_offsets entry {i}: target '{}' does not have address",
                    ctx.symbols[id]
                );
                continue;
            }
        };
        buf[i * 4..i * 4 + 4].copy_from_slice(&(val as u32).to_le_bytes());
    }
}
