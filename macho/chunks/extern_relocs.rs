//! The external relocations of a kext (LC_DYSYMTAB's extreloff): each
//! place kmutil fills with the address of a symbol the kext imports
//! from the kernel or another kext, as dyld's binds would for a dylib.
//! Its local relocations (local_relocs.rs) slide the rest. Legacy
//! LINKEDIT (Args::legacy_linkedit) has dyld bind an image's pointers
//! in data by them too, its GOT slots and lazy pointers being bound by
//! the indirect symbol table.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;

#[derive(Debug)]
pub struct ExternRelocsSection {
    pub hdr: ChunkHeader,
    /// The places, each with its symbol and whether it is an x86-64
    /// call, which calls an import directly.
    pub relocs: Vec<(u64, SymbolId, bool)>,
}

impl ExternRelocsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self { hdr, relocs: Vec::new() }
    }
}

impl Default for ExternRelocsSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Collects the GOT slots and data pointers that hold an import's
/// address, and the calls to one (on x86-64, which has no stubs for a
/// kext), and sizes the table. In legacy LINKEDIT, the data pointers
/// dyld binds (see Context::binds_pointer).
pub fn build<E: Target>(ctx: &mut Context<E>) {
    let got = &ctx.got;
    let mut vec: Vec<(u64, SymbolId, bool)> = Vec::new();
    if !ctx.args.legacy_linkedit {
        vec.extend(
            (got.got_syms.iter().enumerate())
                .filter(|&(_, &id)| ctx.symbols[id].is_imported())
                .map(|(i, &id)| (got.slot_addr(i), id, false)),
        );
    }
    let binds = |id| match ctx.args.legacy_linkedit {
        true => ctx.binds_pointer(id),
        false => ctx.symbols[id].is_imported(),
    };
    for isec in ctx.isecs.iter() {
        if !isec.is_emitted() {
            continue;
        }
        let Some(chunk) = isec.output_section() else {
            continue;
        };
        let base = ctx.chunk_header(chunk).addr + isec.offset as u64;
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) else {
                continue;
            };
            if !binds(id) || rel.is_subtracted {
                continue;
            }
            let pointer = E::is_absrel(rel);
            let call = rel.is_func_call::<E>() && ctx.symbols[id].stub_idx(&ctx.symbols).is_none();
            if pointer || call {
                vec.push((base + rel.offset as u64, id, call));
            }
        }
    }
    ctx.extern_relocs.hdr.size = (vec.len() * size_of::<MachRel>()) as u64;
    ctx.extern_relocs.relocs = vec;
}

/// Writes the records once the symbol table is numbered, in ld-prime's
/// order: the pointers, then the calls, each by symbol and address. An
/// address counts from the start of a kext, and in legacy LINKEDIT as
/// a local relocation's does.
pub fn write<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let index = |id: SymbolId| ctx.symtab.output_sym_indices[id as usize];
    let mut relocs: Vec<(bool, u32, u64)> =
        ctx.extern_relocs.relocs.iter().map(|&(addr, id, call)| (call, index(id), addr)).collect();
    relocs.sort_unstable();
    let base = match ctx.args.legacy_linkedit {
        true => crate::chunks::local_relocs::relocation_base(ctx),
        false => ctx.mach_header.hdr.addr,
    };
    let mut off = ctx.extern_relocs.hdr.fileoff as usize;
    for (call, sym, addr) in relocs {
        // A pointer: 8-byte UNSIGNED. A call: 4-byte pc-relative
        // BRANCH (x86-64's).
        let bits = if call {
            sym | (1 << 24) | (2 << 25) | (1 << 27) | (u32::from(X86_64_RELOC_BRANCH) << 28)
        } else {
            sym | (3 << 25) | (1 << 27) | (u32::from(E::RELOC_UNSIGNED) << 28)
        };
        let rel = MachRel { offset: addr.wrapping_sub(base) as u32, bits };
        rel.write_to(&mut buf[off..]);
        off += size_of::<MachRel>();
    }
}
