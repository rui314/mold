//! __TEXT,__stub_helper: with classic dyld info, the code a lazy pointer
//! initially points at, which enters dyld_stub_binder with the pointer's
//! lazy-bind record - or, in legacy LINKEDIT, dyld_stub_binding_helper
//! with the pointer's address.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_files::add_data_word;
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

/// The size of __stub_helper's header, the code its entries jump to
/// that enters dyld_stub_binder. Legacy LINKEDIT's entries go to
/// crt1.o's dyld_stub_binding_helper instead, and its helper has no
/// header.
pub fn header_size<E: Target>(ctx: &Context<E>) -> u64 {
    if ctx.args.legacy_linkedit { 0 } else { E::STUB_HELPER_HEADER_SIZE }
}

/// The offset in __stub_helper of the entry of lazily bound stub `idx`
/// (an index into StubsSection::lazy), past the header.
pub fn entry_offset<E: Target>(ctx: &Context<E>, idx: u32) -> u64 {
    header_size(ctx) + idx as u64 * E::STUB_HELPER_ENTRY_SIZE
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
    // dyld_stub_binding_helper instead (see resolve_stub_binder).
    if ctx.stub_helper.dyld_stub_binder.is_some() || ctx.args.legacy_linkedit {
        return;
    }
    let Some(id) = ctx.bind_linker_import(b"dyld_stub_binder") else {
        crate::fatal!("lazy binding needs dyld_stub_binder, which no loaded dylib exports");
    };
    ctx.symbols[id].set_used(true);
    crate::chunks::got::add_got_symbol(ctx, id);
    ctx.stub_helper.dyld_stub_binder = Some(id);
    let isec = add_data_word(ctx, 8);
    ctx.stub_helper.dyld_private_isec = isec;
    ctx.extra_local_syms.push((b"__dyld_private", isec));
}

/// Finds the helper legacy LINKEDIT's stub helper entries jump to. It
/// binds no dyld_stub_binder: its entries go to dyld_stub_binding_helper,
/// which crt1.o, dylib1.o or bundle1.o defines; no dylib exports it.
/// (Otherwise dyld_stub_binder is bound once a stub needs it; see
/// ensure_stub_binder.)
pub fn resolve_stub_binder<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.legacy_linkedit {
        let id = ctx.symbols.lookup(b"dyld_stub_binding_helper");
        let id = id.filter(|&id| ctx.symbols[id].input_section().is_some());
        ctx.stub_helper.binding_helper = id;
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    ctx.stub_helper.hdr.size =
        header_size(ctx) + ctx.stubs.lazy.len() as u64 * E::STUB_HELPER_ENTRY_SIZE;
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_stub_helper(ctx, ctx.stub_helper.hdr.addr, buf);
}
