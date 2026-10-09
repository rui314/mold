//! `.plt`, stubs for runtime lazy symbol resolution.

use rayon::prelude::*;

use crate::arch::{Family, Target};
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::SymtabBlock;
use crate::symbol::SymbolId;

// .plt contains linker-synthesized stub code that acts as if they are
// functions. They in fact immediately branch to real function entry
// points. .plt is used as a stub for runtime lazy symbol resolution.
#[derive(Debug)]
pub struct PltSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub symbols: Vec<SymbolId>,
}

impl<E: Target> PltSection<E> {
    pub fn new() -> Self {
        let mut hdr =
            ChunkHeader::<E>::new(".plt", SHT_PROGBITS, (SHF_ALLOC | SHF_EXECINSTR) as u64);
        if E::IS_SPARC {
            let flags = hdr.shdr.sh_flags.get() | SHF_WRITE as u64;
            hdr.shdr.sh_flags.set(flags);
            hdr.shdr.sh_addralign.set(256);
        } else {
            hdr.shdr.sh_addralign.set(16);
        }
        Self { hdr, symbols: Vec::new() }
    }
}

impl<E: Target> Default for PltSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
pub fn add_symbol<E: Target>(ctx: &mut Context<E>, sym: SymbolId) {
    debug_assert!(!ctx.symbols[sym].has_plt(&ctx.symbols));
    let idx = ctx.plt.symbols.len() as u32;
    assert_ne!(idx, u32::MAX);
    ctx.symbols.aux_mut(sym).plt_idx = idx;
    ctx.plt.symbols.push(sym);
    ctx.dynsym.add_symbol(&mut ctx.symbols, sym);
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let n = ctx.plt.symbols.len() as u32;
    ctx.plt.hdr.shdr.sh_size.set(if n == 0 {
        0
    } else if E::IS_SPARC {
        // Large SPARC PLT entries are interleaved with data pointers (see
        // Sparc64::write_plt_entry), but each entry takes 32 bytes in total.
        E::plt_entry_offset(ctx, 0) + n as u64 * 32
    } else {
        E::plt_entry_offset(ctx, n)
    });
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_plt_header(ctx, buf);
    for (i, &id) in ctx.plt.symbols.iter().enumerate() {
        let off = E::plt_entry_offset(ctx, i as u32) as usize;
        E::write_plt_entry(ctx, &mut buf[off..], &ctx.symbols[id]);
    }
}

pub fn compute_symtab_size<E: Target>(ctx: &mut Context<E>) {
    let plt = &mut ctx.plt;
    let n = plt.symbols.len() as u32;
    let strtab_size: u64 = plt
        .symbols
        .par_iter()
        .map(|&id| ctx.symbols[id].name().len() as u64 + "$plt".len() as u64 + 1)
        .sum();
    plt.hdr.num_local_symtab = if E::FAMILY == Family::Arm32 { n * 3 + 2 } else { n };
    plt.hdr.strtab_size = strtab_size;
}

pub fn populate_symtab<E: Target>(ctx: &Context<E>, block: &mut SymtabBlock<'_>) {
    let plt = &ctx.plt;
    if plt.hdr.num_local_symtab == 0 {
        return;
    }
    let func = |addr: u64| {
        let mut sym = ElfSym::<E>::default();
        sym.set_st_shndx(plt.hdr.shndx);
        sym.set_st_value(addr);
        sym.set_type(STT_FUNC);
        sym
    };
    use crate::chunks::strtab::{ARM, DATA};
    if E::FAMILY == Family::Arm32 {
        block.push_mapping_symbol::<E>(ARM, plt.hdr.shndx, plt.hdr.shdr.sh_addr.get());
        block.push_mapping_symbol::<E>(DATA, plt.hdr.shndx, plt.hdr.shdr.sh_addr.get() + 16);
    }
    for &id in &plt.symbols {
        let sym = &ctx.symbols[id];
        let addr = sym.plt_addr(ctx);
        block.push_synthetic::<E>(sym.name(), b"$plt", func(addr));
        if E::FAMILY == Family::Arm32 {
            block.push_mapping_symbol::<E>(ARM, plt.hdr.shndx, addr);
            block.push_mapping_symbol::<E>(DATA, plt.hdr.shndx, addr + 12);
        }
    }
}
