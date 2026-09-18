//! `.rel.plt` and `.rela.plt`, relocations for PLT entries.

use crate::chunks::{ChunkHeader, plt};
use crate::context::Context;
use crate::elf::*;
use crate::target::Target;

// .rel.plt contains relocation information for .plt.
pub fn new_header<E: Target>() -> ChunkHeader<E> {
    let (name, ty) = if E::IS_RELA { (".rela.plt", SHT_RELA) } else { (".rel.plt", SHT_REL) };
    let mut hdr = ChunkHeader::<E>::new(name, ty, SHF_ALLOC as u64);
    let entsize = ElfRel::<E>::size() as u64;
    hdr.shdr.sh_entsize.set(entsize);
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    hdr
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let size = ctx.plt.symbols.len() as u64 * ElfRel::<E>::size() as u64;
    ctx.relplt.shdr.sh_size.set(size);
    ctx.relplt.shdr.sh_link.set(ctx.dynsym.hdr.shndx);
    if !E::IS_SPARC {
        ctx.relplt.shdr.sh_info.set(ctx.gotplt.shndx);
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
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
                // (target - call) there (see target/sparc64.rs).
                let call = sym.plt_addr(ctx) + 4;
                let ptr = ctx.plt.hdr.shdr.sh_addr.get()
                    + crate::target::sparc64::plt_ptr_offset(ctx.plt.symbols.len(), idx);
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
