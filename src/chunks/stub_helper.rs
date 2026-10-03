//! __TEXT,__stub_helper: with classic dyld info, the code a lazy pointer
//! initially points at, which enters dyld_stub_binder with the pointer's
//! lazy-bind record - or, in legacy LINKEDIT, dyld_stub_binding_helper
//! with the pointer's address.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// __TEXT,__stub_helper: with classic dyld info, the code a lazy
/// pointer initially points at, which enters dyld_stub_binder with the
/// pointer's lazy-bind record - or, in legacy LINKEDIT,
/// dyld_stub_binding_helper with the pointer's address.
#[derive(Debug)]
pub struct StubHelperSection {
    pub hdr: ChunkHeader,
    /// dyld_stub_binder, resolved from the loaded dylibs when lazy
    /// binding is in use, and the __dyld_private word the stub helper
    /// hands it (a synthesized record in __DATA,__data).
    pub dyld_stub_binder: Option<SymbolId>,
    pub dyld_private_isec: u32,
    /// In legacy LINKEDIT (Args::legacy_linkedit), what the entries
    /// jump to instead: crt1.o's dyld_stub_binding_helper, if defined.
    pub binding_helper: Option<SymbolId>,
}

impl StubHelperSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__stub_helper");
        hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 2;
        Self { hdr, dyld_stub_binder: None, dyld_private_isec: u32::MAX, binding_helper: None }
    }
}

impl Default for StubHelperSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_stub_helper(ctx, ctx.stub_helper.hdr.addr, buf);
}
