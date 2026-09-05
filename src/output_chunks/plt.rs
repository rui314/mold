//! `.plt`, stubs for runtime lazy symbol resolution.

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::SymtabBlock;
use crate::output_chunks::ChunkHeader;
use crate::symbol::SymbolId;

// .plt contains linker-synthesized stub code that acts as if they are
// functions. They are in fact immediately branches to real function entry
// points. .plt is used as a stub for runtime lazy symbol resolution.
#[derive(Debug)]
pub struct PltSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub symbols: Vec<SymbolId>,
}

impl<E: Arch> PltSection<E> {
    pub fn new() -> PltSection<E> {
        let mut hdr =
            ChunkHeader::<E>::new(".plt", SHT_PROGBITS, (SHF_ALLOC | SHF_EXECINSTR) as u64);
        if E::IS_SPARC {
            hdr.shdr
                .sh_flags
                .set(hdr.shdr.sh_flags.get() | SHF_WRITE as u64);
            hdr.shdr.sh_addralign.set(256);
        } else {
            hdr.shdr.sh_addralign.set(16);
        }
        PltSection {
            hdr,
            symbols: Vec::new(),
        }
    }
}

impl<E: Arch> Default for PltSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

// On SPARC, .plt uses 32-byte "small" entries until it grows past 0x100000
// bytes (the reach of a small entry's branch to the resolver), after which
// it switches to a "large" entry format. This is how many small entries fit.
pub const SPARC_NUM_SMALL_PLT: u64 = (0x100000 - 128) / 32;

/// The offset of a PLT entry within `.plt`.
pub fn entry_offset<E: Arch>(idx: u32) -> u64 {
    let idx = idx as u64;
    match E::FAMILY {
        Family::Ppc64V1 => {
            // The PPC64 ELFv1 ABI requires PLT entries to vary in size
            // depending on their indices. For entries whose PLT index is
            // less than 32768, the entry size is 8 bytes. Other entries are
            // 12 bytes long.
            if idx < 0x8000 {
                E::PLT_HDR_SIZE + idx * 8
            } else {
                E::PLT_HDR_SIZE + 0x8000 * 8 + (idx - 0x8000) * 12
            }
        }
        Family::Sparc64 => {
            // SPARC large PLT entries are grouped into blocks of 160, each holding
            // 160 24-byte code stubs followed by 160 8-byte data pointers (so a
            // stub's `ldx` reaches its pointer within a signed 13-bit offset). This
            // returns the offset of pltidx's code stub.
            if idx < SPARC_NUM_SMALL_PLT {
                E::PLT_HDR_SIZE + idx * E::PLT_SIZE
            } else {
                let i = idx - SPARC_NUM_SMALL_PLT;
                0x100000 + (i / 160) * 5120 + (i % 160) * 24
            }
        }
        _ => E::PLT_HDR_SIZE + idx * E::PLT_SIZE,
    }
}

pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
    debug_assert!(!ctx.symbols[sym].has_plt(&ctx.symbols));
    let idx = ctx.plt.symbols.len() as u32;
    ctx.symbols.aux_mut(sym).plt_idx = Some(idx);
    ctx.plt.symbols.push(sym);
    crate::output_chunks::dynsym::add_symbol(ctx, sym);
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let n = ctx.plt.symbols.len() as u64;
    ctx.plt.hdr.shdr.sh_size.set(if n == 0 {
        0
    } else if E::IS_SPARC {
        E::PLT_HDR_SIZE + n * E::PLT_SIZE
    } else {
        entry_offset::<E>(n as u32)
    });
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_plt_header(ctx, buf);
    for (i, &id) in ctx.plt.symbols.iter().enumerate() {
        let off = entry_offset::<E>(i as u32) as usize;
        E::write_plt_entry(ctx, &mut buf[off..], &ctx.symbols[id]);
    }
}

pub fn compute_symtab_size<E: Arch>(ctx: &mut Context<E>) {
    let n = ctx.plt.symbols.len() as u32;
    let strtab_size: u64 = ctx
        .plt
        .symbols
        .iter()
        .map(|&id| ctx.symbols[id].name().len() as u64 + "$plt".len() as u64 + 1)
        .sum();
    ctx.plt.hdr.num_local_symtab = if E::FAMILY == Family::Arm32 {
        n * 3 + 2
    } else {
        n
    };
    ctx.plt.hdr.strtab_size = strtab_size;
}

pub fn populate_symtab<E: Arch>(ctx: &Context<E>, block: &mut SymtabBlock<'_>) {
    let plt = &ctx.plt;
    if plt.hdr.num_local_symtab == 0 {
        return;
    }
    let func = |addr: u64| {
        let mut sym = ElfSym::<E>::default();
        sym.st_shndx_mut().set(plt.hdr.shndx as u16);
        sym.st_value_mut().set(addr);
        sym.set_type(STT_FUNC);
        sym
    };
    use crate::output_chunks::strtab::{ARM, DATA};
    if E::FAMILY == Family::Arm32 {
        block.push_mapping_symbol::<E>(ARM, func(plt.hdr.shdr.sh_addr.get()));
        block.push_mapping_symbol::<E>(DATA, func(plt.hdr.shdr.sh_addr.get() + 16));
    }
    for &id in &plt.symbols {
        let sym = &ctx.symbols[id];
        let addr = sym.plt_addr(ctx);
        block.push_synthetic::<E>(sym.name(), b"$plt", func(addr));
        if E::FAMILY == Family::Arm32 {
            block.push_mapping_symbol::<E>(ARM, func(addr));
            block.push_mapping_symbol::<E>(DATA, func(addr + 12));
        }
    }
}
