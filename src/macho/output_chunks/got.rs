//! The synthetic pointer and stub sections: __stubs, __stub_helper,
//! __la_symbol_ptr, __got and __thread_ptrs. mold-rust's got.rs holds
//! their ELF counterparts (.plt, .plt.got, .got.plt, .got).

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::output_chunks::ChunkHeader;
use crate::macho::symbol::SymbolId;

/// __TEXT,__stubs: jump stubs for calls to imported functions.
#[derive(Debug)]
pub struct StubsSection {
    pub hdr: ChunkHeader,
    /// Symbols with a __stubs entry, in stub order.
    pub symbols: Vec<SymbolId>,
}

impl StubsSection {
    pub fn new() -> StubsSection {
        let mut hdr = ChunkHeader::new("__TEXT", "__stubs");
        hdr.flags = S_SYMBOL_STUBS | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 2;
        StubsSection { hdr, symbols: Vec::new() }
    }
}

pub mod stubs {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        E::write_stubs(ctx, ctx.stubs.hdr.addr, buf);
    }
}

/// __TEXT,__stub_helper: with classic dyld info, the code a lazy
/// pointer initially points at, which enters dyld_stub_binder with the
/// pointer's lazy-bind record.
#[derive(Debug)]
pub struct StubHelperSection {
    pub hdr: ChunkHeader,
    /// dyld_stub_binder, resolved from the loaded dylibs when lazy
    /// binding is in use, and the __dyld_private word the stub helper
    /// hands it (a synthesized record in __DATA,__data).
    pub dyld_stub_binder: Option<SymbolId>,
    pub dyld_private_isec: u32,
}

impl StubHelperSection {
    pub fn new() -> StubHelperSection {
        let mut hdr = ChunkHeader::new("__TEXT", "__stub_helper");
        hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 2;
        StubHelperSection { hdr, dyld_stub_binder: None, dyld_private_isec: u32::MAX }
    }
}

pub mod stub_helper {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        E::write_stub_helper(ctx, ctx.stub_helper.hdr.addr, buf);
    }
}

/// __DATA,__la_symbol_ptr: the lazy pointers the stubs jump through,
/// bound by dyld on first call.
#[derive(Debug)]
pub struct LazyPtrsSection {
    pub hdr: ChunkHeader,
}

impl LazyPtrsSection {
    pub fn new() -> LazyPtrsSection {
        let mut hdr = ChunkHeader::new("__DATA", "__la_symbol_ptr");
        hdr.flags = S_LAZY_SYMBOL_POINTERS;
        hdr.p2align = 3;
        LazyPtrsSection { hdr }
    }
}

pub mod lazy_ptrs {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        // Each lazy pointer starts at its stub helper entry.
        let helper = ctx.stub_helper.hdr.addr + E::STUB_HELPER_HEADER_SIZE;
        for i in 0..ctx.stubs.symbols.len() {
            let val = helper + i as u64 * E::STUB_HELPER_ENTRY_SIZE;
            buf[i * 8..i * 8 + 8].copy_from_slice(&val.to_le_bytes());
        }
    }
}

/// The global offset table: pointers to symbols, bound by dyld for
/// imported ones.
#[derive(Debug)]
pub struct GotSection {
    pub hdr: ChunkHeader,
    /// Symbols with a __got slot, in slot order.
    pub got_syms: Vec<SymbolId>,
    /// Synthetic subsections standing for __got slots that absorbed
    /// __objc_classrefs entries (see fold_objc_classrefs); they are
    /// placed in this section once it exists.
    pub objc_classref_slots: Vec<u32>,
}

impl GotSection {
    pub fn new() -> GotSection {
        let mut hdr = ChunkHeader::new("__DATA", "__got");
        hdr.flags = S_NON_LAZY_SYMBOL_POINTERS;
        hdr.p2align = 3;
        GotSection { hdr, got_syms: Vec::new(), objc_classref_slots: Vec::new() }
    }
}

pub mod got {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        // Slots for imported symbols stay zero; dyld fills them via
        // the bind stream.
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if !ctx.symbols[id].is_imported() {
                buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.sym_addr(id).to_le_bytes());
            }
        }
    }
}

/// __DATA,__thread_ptrs: pointers to thread-local variable
/// descriptors, what a TLVP-relocated instruction sequence loads from.
#[derive(Debug)]
pub struct ThreadPtrsSection {
    pub hdr: ChunkHeader,
    /// Thread-local symbols with a __thread_ptrs slot, in slot order.
    pub symbols: Vec<SymbolId>,
}

impl ThreadPtrsSection {
    pub fn new() -> ThreadPtrsSection {
        let mut hdr = ChunkHeader::new("__DATA", "__thread_ptrs");
        hdr.flags = S_THREAD_LOCAL_VARIABLE_POINTERS;
        hdr.p2align = 3;
        ThreadPtrsSection { hdr, symbols: Vec::new() }
    }
}

pub mod thread_ptrs {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        for (i, &id) in ctx.thread_ptrs.symbols.iter().enumerate() {
            if !ctx.symbols[id].is_imported() {
                buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.sym_addr(id).to_le_bytes());
            }
        }
    }
}
