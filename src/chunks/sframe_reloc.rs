//! `.rela.sframe`, SFrame relocations for relocatable outputs.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

// SFrameRelocSection holds the relocations for .sframe. We use it only for
// relocatable outputs, where function addresses aren't known yet and each
// FDE's func_start has to remain a relocation for the final link to
// resolve. It is the .sframe counterpart of EhFrameRelocSection.
pub fn new_header<E: Arch>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(".rela.sframe", SHT_RELA, SHF_INFO_LINK as u64);
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    let entsize = std::mem::size_of::<ElfRel<E>>() as u64;
    hdr.shdr.sh_entsize.set(entsize);
    hdr
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let n = ctx.sframe.fdes.len();
    let size = (n * std::mem::size_of::<ElfRel<E>>()) as u64;
    let sec = ctx.sframe_reloc.as_mut().unwrap();
    sec.shdr.sh_size.set(size);
    sec.shdr.sh_link.set(ctx.symtab.shndx);
    sec.shdr.sh_info.set(ctx.sframe.hdr.shndx);
}

// Emit one relocation per FDE for its func_start field. The entries are
// written in the same order as SFrameSection::copy_buf lays out the index.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let Some(r_type) = E::R_SFRAME else {
        return;
    };
    let out = rels_from_bytes_mut::<E>(buf);
    debug_assert_eq!(out.len(), ctx.sframe.fdes.len());
    for (i, &(fi, fi_idx)) in ctx.sframe.fdes.iter().enumerate() {
        let fde = &ctx.objs[fi.index()].sframe_fdes[fi_idx as usize];
        let sym = &ctx.symbols[fde.sym];
        let r_offset = ctx.sframe.hdr.shdr.sh_addr.get()
            + SFrameHeader::<E>::size() as u64
            + (i * SFrameFdeIdx::<E>::size()) as u64;

        let (r_sym, r_addend) = crate::chunks::reloc::output_symidx_addend(ctx, sym, fde.addend)
            .expect("relocation refers to a section without output");
        out[i] = ElfRel::<E>::new(r_offset, r_type, r_sym, r_addend);
    }
}
