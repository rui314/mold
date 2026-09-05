//! `.dynsym`, the dynamic symbol table.

use crate::arch::Arch;
use crate::context::Context;
use crate::elf::*;
use crate::error;
use crate::output_chunks::symtab::to_output_esym;
use crate::output_chunks::ChunkHeader;
use crate::symbol::SymbolId;

// .dynsym contains symbols for dynamic linking. This is similar to
// .symtab, but .dynsym contains data that the runtime uses.
#[derive(Debug)]
pub struct DynsymSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    /// Index 0 is the null symbol.
    pub symbols: Vec<Option<SymbolId>>,
    pub dynstr_offset: u64,
}

impl<E: Arch> DynsymSection<E> {
    pub fn new() -> DynsymSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".dynsym", SHT_DYNSYM, SHF_ALLOC as u64);
        hdr.shdr
            .sh_entsize
            .set(std::mem::size_of::<ElfSym<E>>() as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        DynsymSection {
            hdr,
            symbols: Vec::new(),
            dynstr_offset: 0,
        }
    }
}

impl<E: Arch> Default for DynsymSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
    if ctx.dynsym.symbols.is_empty() {
        ctx.dynsym.symbols.push(None);
    }
    if ctx.symbols[sym].dynsym_idx(&ctx.symbols).is_none() {
        // A placeholder; the real index is assigned by sort_dynsyms.
        ctx.symbols.aux_mut(sym).dynsym_idx = Some(u32::MAX);
        ctx.dynsym.symbols.push(Some(sym));
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    ctx.dynsym.hdr.shdr.sh_link.set(ctx.dynstr.hdr.shndx);
    ctx.dynsym
        .hdr
        .shdr
        .sh_size
        .set(std::mem::size_of::<ElfSym<E>>() as u64 * ctx.dynsym.symbols.len() as u64);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let size = std::mem::size_of::<ElfSym<E>>();
    buf[..size].fill(0);
    let mut offset = ctx.dynsym.dynstr_offset as u32;
    for &id in ctx.dynsym.symbols.iter().skip(1).flatten() {
        let sym = &ctx.symbols[id];
        let (esym, xindex) = to_output_esym(ctx, sym, offset);
        if xindex != 0 {
            let nshdrs = ctx.shdr.as_ref().map_or(0, |s| {
                s.hdr.shdr.sh_size.get() / ElfShdr::<E>::size() as u64
            });
            error!("{}: .dynsym: too many output sections: {nshdrs} requested, but ELF allows at most 65279",
                ctx.args.output
            );
            return;
        }
        esym.write(&mut buf[sym.dynsym_idx(&ctx.symbols).unwrap() as usize * size..]);
        offset += sym.name().len() as u32 + 1;
    }
}
