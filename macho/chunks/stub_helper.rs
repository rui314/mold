//! __TEXT,__stub_helper: with classic dyld info, the code a lazy pointer
//! initially points at, which enters dyld_stub_binder with the pointer's
//! lazy-bind record - or, in legacy LINKEDIT, dyld_stub_binding_helper
//! with the pointer's address.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;

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

/// With lazy binding, the stub helper enters dyld through
/// dyld_stub_binder (libSystem's): the symbol is bound from whichever
/// loaded dylib exports it - or, where the image may look it up
/// dynamically (-undefined dynamic_lookup, -U), from whatever image dyld
/// finds it in - given a GOT slot, and __dyld_private (the word
/// dyld_stub_binder is handed, ld64 puts it in __DATA,__data) is
/// synthesized. Once, on the first stub.
pub fn ensure_stub_binder<E: Target>(ctx: &mut Context<E>) {
    // Legacy LINKEDIT's helper enters dyld through crt1.o's
    // dyld_stub_binding_helper instead (see passes::resolve_stub_binder).
    if ctx.stub_helper.dyld_stub_binder.is_some() || ctx.args.legacy_linkedit {
        return;
    }
    let Some(id) = ctx.bind_linker_import(b"dyld_stub_binder") else {
        crate::fatal!("lazy binding needs dyld_stub_binder, which no loaded dylib exports");
    };
    ctx.symbols[id].set_is_used(true);
    crate::chunks::got::add_got_symbol(ctx, id);
    ctx.stub_helper.dyld_stub_binder = Some(id);
    let isec = ctx.add_data_word(8);
    ctx.stub_helper.dyld_private_isec = isec;
    ctx.extra_local_syms.push((b"__dyld_private", isec));
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_stub_helper(ctx, ctx.stub_helper.hdr.addr, buf);
}
