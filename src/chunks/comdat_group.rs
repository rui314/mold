//! COMDAT groups in relocatable outputs.

use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::symbol::SymbolId;
use crate::target::Target;

// ComdatGroupSection represents a comdat group for an output file.
// This is used only for the relocatable output (i.e. the `-r` output).
#[derive(Debug)]
pub struct ComdatGroupSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub sym: SymbolId,
    pub members: Vec<ChunkId>,
}

impl<E: Target> ComdatGroupSection<E> {
    pub fn new(sym: SymbolId, members: Vec<ChunkId>) -> Self {
        let mut hdr = ChunkHeader::<E>::new(".group", SHT_GROUP, 0);
        hdr.shdr.sh_entsize.set(4);
        hdr.shdr.sh_addralign.set(4);
        hdr.shdr.sh_size.set((members.len() * 4 + 4) as u64);
        Self { hdr, sym, members }
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>, i: u32) {
    debug_assert!(ctx.args.relocatable);
    let sec = &ctx.comdat_group_sections[i as usize];
    let sym = &ctx.symbols[sec.sym];
    let sh_info = if sym.st_type() == STT_SECTION {
        let isec = sym.input_section_ref(ctx).unwrap();
        ctx.output_section(isec.output_section.unwrap()).hdr.shndx
    } else {
        sym.output_sym_idx(ctx)
    };
    let symtab_shndx = ctx.symtab.shndx;
    let sec = &mut ctx.comdat_group_sections[i as usize];
    sec.hdr.shdr.sh_link.set(symtab_shndx);
    sec.hdr.shdr.sh_info.set(sh_info);
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, i: u32, buf: &mut [u8]) {
    let sec = &ctx.comdat_group_sections[i as usize];
    E::write_u32(buf, GRP_COMDAT);
    for (j, &member) in sec.members.iter().enumerate() {
        E::write_u32(&mut buf[4 + j * 4..], ctx.chunk_header(member).shndx);
    }
}
