//! `.opd`, the function descriptors of PowerPC64 ELFv1.
//!
//! In ELFv1 a function pointer doesn't refer to code but to a
//! descriptor: the entry point, the TOC pointer the function expects in
//! `r2`, and an environment pointer nobody uses. The compiler emits a
//! descriptor for every function into an input `.opd`; the linker
//! dissolves those (see [`crate::arch::ppc64v1`]) and synthesizes
//! descriptors here only for functions whose addresses are taken, in the
//! same way it synthesizes GOT and PLT entries.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::symbol::{AddrFlags, SymbolId};
use crate::util::endian::Endian;

pub const ENTRY_SIZE: u64 = 24;

#[derive(Debug)]
pub struct Ppc64OpdSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub symbols: Vec<SymbolId>,
}

impl<E: Layout> Ppc64OpdSection<E> {
    pub fn new() -> Ppc64OpdSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".opd", SHT_PROGBITS, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.shdr.sh_addralign.set(8);
        Ppc64OpdSection {
            hdr,
            symbols: Vec::new(),
        }
    }
}

impl<E: Layout> Default for Ppc64OpdSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// A descriptor holds the function's own address, not that of its PLT
/// entry or of the descriptor itself.
const ENTRY_POINT: AddrFlags = AddrFlags {
    no_plt: true,
    no_opd: true,
};

fn section<E: Arch>(ctx: &Context<E>) -> &Ppc64OpdSection<E> {
    ctx.ppc64_opd
        .as_ref()
        .expect("PPC64 ELFv1 has an .opd section")
}

fn toc<E: Arch>(ctx: &Context<E>) -> u64 {
    ctx.symbols[ctx.syms.toc.expect("PPC64 has a .TOC. symbol")].addr(ctx)
}

pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
    let opd = ctx
        .ppc64_opd
        .as_mut()
        .expect("PPC64 ELFv1 has an .opd section");
    let idx = opd.symbols.len() as u32;
    assert_ne!(idx, u32::MAX);
    ctx.symbols.aux_mut(sym).opd_idx = idx;
    opd.symbols.push(sym);
    let size = opd.hdr.shdr.sh_size.get() + ENTRY_SIZE;
    opd.hdr.shdr.sh_size.set(size);
}

/// Position-independent output relocates both the entry point and the
/// TOC pointer at load time.
pub fn num_dynrels<E: Arch>(ctx: &Context<E>) -> u64 {
    if ctx.args.pic {
        section(ctx).symbols.len() as u64 * 2
    } else {
        0
    }
}

pub fn relr_offsets<E: Arch>(ctx: &Context<E>) -> Vec<u64> {
    if !ctx.args.pic {
        return Vec::new();
    }
    (0..section(ctx).symbols.len() as u64)
        .flat_map(|i| [i * ENTRY_SIZE, i * ENTRY_SIZE + 8])
        .collect()
}

pub fn write_dynrels<E: Arch>(ctx: &Context<E>, out: &mut [E::Rel]) {
    if !ctx.args.pic {
        debug_assert!(out.is_empty());
        return;
    }
    let opd = section(ctx);
    let relr = ctx.args.pack_dyn_relocs_relr && opd.hdr.num_relrs != 0;
    let toc = toc(ctx);
    let mut j = 0;
    for (i, &id) in opd.symbols.iter().enumerate() {
        let loc = opd.hdr.shdr.sh_addr.get() + i as u64 * ENTRY_SIZE;
        let entry = ctx.symbols[id].addr_with(ctx, ENTRY_POINT);
        for (addr, val) in [(loc, entry), (loc + 8, toc)] {
            if !relr || addr % 8 != 0 {
                out[j] = ElfRel::<E>::new(addr, E::R_RELATIVE, 0, val as i64);
                j += 1;
            }
        }
    }
    debug_assert_eq!(j, out.len());
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let toc = toc(ctx);
    for (i, &id) in section(ctx).symbols.iter().enumerate() {
        let entry = &mut buf[i * ENTRY_SIZE as usize..];
        E::Endian::write_u64(entry, ctx.symbols[id].addr_with(ctx, ENTRY_POINT));
        E::Endian::write_u64(&mut entry[8..], toc);
        E::Endian::write_u64(&mut entry[16..], 0);
    }
}
