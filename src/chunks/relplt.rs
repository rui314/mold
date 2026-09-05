//! `.rel.plt` and `.rela.plt`, relocations for PLT entries.

use crate::arch::Arch;
use crate::chunks::{plt, ChunkHeader};
use crate::context::Context;
use crate::elf::*;

// .rel.plt contains relocation information for .plt.
#[derive(Debug)]
pub struct RelPltSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> RelPltSection<E> {
    pub fn new() -> RelPltSection<E> {
        let (name, ty) = if E::IS_RELA {
            (".rela.plt", SHT_RELA)
        } else {
            (".rel.plt", SHT_REL)
        };
        let mut hdr = ChunkHeader::<E>::new(name, ty, SHF_ALLOC as u64);
        hdr.shdr
            .sh_entsize
            .set(std::mem::size_of::<ElfRel<E>>() as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        RelPltSection { hdr }
    }
}

impl<E: Arch> Default for RelPltSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    ctx.relplt
        .hdr
        .shdr
        .sh_size
        .set(ctx.plt.symbols.len() as u64 * std::mem::size_of::<ElfRel<E>>() as u64);
    ctx.relplt.hdr.shdr.sh_link.set(ctx.dynsym.hdr.shndx);
    if !E::IS_SPARC {
        ctx.relplt.hdr.shdr.sh_info.set(ctx.gotplt.hdr.shndx);
    }
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let out = rels_from_bytes_mut::<E>(buf);
    debug_assert_eq!(out.len(), ctx.plt.symbols.len());
    for (i, &id) in ctx.plt.symbols.iter().enumerate() {
        let sym = &ctx.symbols[id];
        let rel = if E::IS_SPARC {
            // SPARC doesn't have a .got.plt because its role is merged to .plt.
            // On SPARC, .plt is writable (!) and the dynamic linker directly
            // modifies .plt's machine instructions as it resolves dynamic symbols.
            // Therefore, it doesn't need a separate section to store the symbol
            // resolution results. That is of course horrible from the security
            // point of view, though.
            let idx = sym.plt_idx(&ctx.symbols).unwrap() as u64;
            if idx < plt::SPARC_NUM_SMALL_PLT {
                ElfRel::<E>::new(
                    sym.plt_addr(ctx),
                    E::R_JUMP_SLOT,
                    sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                    0,
                )
            } else {
                // A large PLT entry resolves through a data pointer rather than
                // self-modifying code, so its relocation targets that pointer and
                // carries -(call address) as the addend, making the loader store
                // (target - call) there (see arch/sparc64.rs).
                let call = sym.plt_addr(ctx) + 4;
                let ptr = ctx.plt.hdr.shdr.sh_addr.get()
                    + crate::arch::sparc64::plt_ptr_offset(ctx.plt.symbols.len(), idx);
                ElfRel::<E>::new(
                    ptr,
                    E::R_JUMP_SLOT,
                    sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                    -(call as i64),
                )
            }
        } else {
            ElfRel::<E>::new(
                sym.gotplt_addr(ctx),
                E::R_JUMP_SLOT,
                sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                0,
            )
        };
        out[i] = rel;
    }
}
