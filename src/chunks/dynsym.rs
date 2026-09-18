//! `.dynsym`, the dynamic symbol table.

use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::symtab::to_output_esym;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::error;
use crate::symbol::{SymbolId, SymbolTable};

// .dynsym contains symbols for dynamic linking. This is similar to
// .symtab, but .dynsym contains data that the runtime uses.
#[derive(Debug)]
pub struct DynsymSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    /// Index 0 is the null symbol.
    pub symbols: Vec<Option<SymbolId>>,
    pub dynstr_entries: Vec<DynstrEntry>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DynstrEntry {
    pub name: &'static [u8],
    pub offset: u64,
}

impl<E: Arch> DynsymSection<E> {
    pub fn new() -> DynsymSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".dynsym", SHT_DYNSYM, SHF_ALLOC as u64);
        let entsize = std::mem::size_of::<ElfSym<E>>() as u64;
        hdr.shdr.sh_entsize.set(entsize);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        DynsymSection { hdr, symbols: Vec::new(), dynstr_entries: Vec::new() }
    }

    #[inline]
    pub fn add_symbol(&mut self, symbols: &mut SymbolTable, sym: SymbolId) {
        if self.symbols.is_empty() {
            self.symbols.push(None);
        }
        if symbols[sym].dynsym_idx(symbols).is_none() {
            // Mark the symbol as queued before sort_dynsyms assigns its real index.
            symbols.aux_mut(sym).dynsym_idx = u32::MAX - 1;
            self.symbols.push(Some(sym));
        }
    }
}

impl<E: Arch> Default for DynsymSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let size = std::mem::size_of::<ElfSym<E>>() as u64 * ctx.dynsym.symbols.len() as u64;
    ctx.dynsym.hdr.shdr.sh_link.set(ctx.dynstr.hdr.shndx);
    ctx.dynsym.hdr.shdr.sh_size.set(size);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let size = std::mem::size_of::<ElfSym<E>>();
    buf[..size].fill(0);
    let overflow = AtomicBool::new(false);
    buf.par_chunks_exact_mut(size)
        .zip(&ctx.dynsym.symbols)
        .zip(&ctx.dynsym.dynstr_entries)
        .enumerate()
        .skip(1)
        .for_each(|(i, ((out, id), entry))| {
            let sym = &ctx.symbols[id.unwrap()];
            debug_assert_eq!(sym.dynsym_idx(&ctx.symbols), Some(i as u32));
            let (esym, xindex) = to_output_esym(ctx, sym, entry.offset as u32);
            if xindex != 0 {
                overflow.store(true, Ordering::Relaxed);
            } else {
                esym.write(out);
            }
        });
    if overflow.load(Ordering::Relaxed) {
        let nshdrs =
            ctx.shdr.as_ref().map_or(0, |s| s.shdr.sh_size.get() / ElfShdr::<E>::size() as u64);
        error!("{}: .dynsym: too many output sections: {nshdrs} requested, but ELF allows at most 65279",
            ctx.args.output.display()
        );
    }
}
