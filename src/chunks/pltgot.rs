//! `.plt.got`, PLT stubs for symbols already resolved through the GOT.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::SymtabBlock;
use crate::symbol::SymbolId;
use crate::target::{Family, Target};

// .plt.got is similar to .plt but doesn't support lazy symbol resolution.
// If we have the same symbol already in .got, resolving the same symbol
// lazily for .plt is just waste of time. Therefore, in such case, we use
// .plt.got for that symbol instead.
#[derive(Debug)]
pub struct PltGotSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub symbols: Vec<SymbolId>,
}

impl<E: Target> PltGotSection<E> {
    pub fn new() -> Self {
        let mut hdr =
            ChunkHeader::<E>::new(".plt.got", SHT_PROGBITS, (SHF_ALLOC | SHF_EXECINSTR) as u64);
        hdr.shdr.sh_addralign.set(16);
        Self { hdr, symbols: Vec::new() }
    }
}

impl<E: Target> Default for PltGotSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
pub fn add_symbol<E: Target>(ctx: &mut Context<E>, sym: SymbolId) {
    debug_assert!(!ctx.symbols[sym].has_plt(&ctx.symbols));
    debug_assert!(ctx.symbols[sym].has_got(&ctx.symbols));
    let idx = ctx.pltgot.symbols.len() as u32;
    assert_ne!(idx, u32::MAX);
    ctx.symbols.aux_mut(sym).pltgot_idx = idx;
    ctx.pltgot.symbols.push(sym);
    let size = ctx.pltgot.symbols.len() as u64 * E::PLTGOT_SIZE;
    ctx.pltgot.hdr.shdr.sh_size.set(size);
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for (i, &id) in ctx.pltgot.symbols.iter().enumerate() {
        let off = i * E::PLTGOT_SIZE as usize;
        E::write_pltgot_entry(ctx, &mut buf[off..], &ctx.symbols[id]);
    }
}

pub fn compute_symtab_size<E: Target>(ctx: &mut Context<E>) {
    let pltgot = &mut ctx.pltgot;
    let n = pltgot.symbols.len() as u32;
    let strtab_size: u64 = pltgot
        .symbols
        .iter()
        .map(|&id| ctx.symbols[id].name().len() as u64 + "$pltgot".len() as u64 + 1)
        .sum();
    pltgot.hdr.num_local_symtab = if E::FAMILY == Family::Arm32 { n * 3 } else { n };
    pltgot.hdr.strtab_size = strtab_size;
}

pub fn populate_symtab<E: Target>(ctx: &Context<E>, block: &mut SymtabBlock<'_>) {
    let pltgot = &ctx.pltgot;
    if pltgot.hdr.num_local_symtab == 0 {
        return;
    }
    let func = |addr: u64| {
        let mut sym = ElfSym::<E>::default();
        sym.set_st_shndx(pltgot.hdr.shndx);
        sym.set_st_value(addr);
        sym.set_type(STT_FUNC);
        sym
    };
    use crate::chunks::strtab::{ARM, DATA};
    for &id in &pltgot.symbols {
        let sym = &ctx.symbols[id];
        let addr = sym.plt_addr(ctx);
        block.push_synthetic::<E>(sym.name(), b"$pltgot", func(addr));
        if E::FAMILY == Family::Arm32 {
            block.push_mapping_symbol::<E>(ARM, func(addr));
            block.push_mapping_symbol::<E>(DATA, func(addr + 12));
        }
    }
}
