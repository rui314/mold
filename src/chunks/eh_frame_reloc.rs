//! `.rel.eh_frame` and `.rela.eh_frame` for relocatable outputs.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::ObjectFile;

// EhFrameRelocSection contains relocation records for .eh_frame. It is used
// only for relocatable outputs (an .o file rather than an executable or .so).
#[derive(Debug)]
pub struct EhFrameRelocSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> EhFrameRelocSection<E> {
    pub fn new() -> EhFrameRelocSection<E> {
        let (name, ty) = if E::IS_RELA {
            (".rela.eh_frame", SHT_RELA)
        } else {
            (".rel.eh_frame", SHT_REL)
        };
        let mut hdr = ChunkHeader::<E>::new(name, ty, SHF_INFO_LINK as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        hdr.shdr
            .sh_entsize
            .set(std::mem::size_of::<ElfRel<E>>() as u64);
        EhFrameRelocSection { hdr }
    }
}

impl<E: Arch> Default for EhFrameRelocSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let count: usize = ctx
        .objs
        .par_iter()
        .map(|file| {
            let cies: usize = file
                .cies
                .iter()
                .filter(|c| c.is_leader)
                .map(|c| c.rels(file).len())
                .sum();
            let fdes: usize = file.fdes.iter().map(|f| f.rels(file).len()).sum();
            cies + fdes
        })
        .sum();
    let sec = ctx.eh_frame_reloc.as_mut().unwrap();
    sec.hdr
        .shdr
        .sh_size
        .set((count * std::mem::size_of::<ElfRel<E>>()) as u64);
    sec.hdr.shdr.sh_link.set(ctx.symtab.hdr.shndx);
    sec.hdr.shdr.sh_info.set(ctx.eh_frame.hdr.shndx);
}

/// Writes the relocations; with REL and `-r`, addends are written into
/// `.eh_frame` itself, which is passed as `eh_frame_buf`.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8], eh_frame_buf: Option<&mut [u8]>) {
    let out = rels_from_bytes_mut::<E>(buf);
    let mut eh_frame_buf = eh_frame_buf;
    let mut n = 0;

    let mut copy = |file: &ObjectFile<E>,
                    shndx: u32,
                    r: &ElfRel<E>,
                    offset: u64,
                    eh_frame_buf: &mut Option<&mut [u8]>| {
        let isec = file.section_at(shndx);
        let sym = &ctx.symbols[file.base.symbols[r.r_sym() as usize]];
        let mut rel = ElfRel::<E>::new(
            ctx.eh_frame.hdr.shdr.sh_addr.get() + offset,
            r.r_type(),
            0,
            0,
        );

        if sym.st_type() == STT_SECTION {
            // We discard section symbols in input files and re-create new
            // ones for each output section. So we need to adjust relocations'
            // addends if they refer a section symbol.
            let target = sym.input_section_ref(ctx).unwrap();
            rel.set_r_sym(ctx.output_section(target.output_section.unwrap()).hdr.shndx);
            let addend = isec.rel_addend(r) + target.offset() as i64;
            if E::IS_RELA {
                rel.set_r_addend(addend);
            } else if ctx.args.relocatable {
                if let Some(eh) = eh_frame_buf {
                    E::write_addend(&mut eh[offset as usize..], addend, r);
                }
            }
        } else {
            rel.set_r_sym(sym.output_sym_idx(ctx));
            if E::IS_RELA {
                rel.set_r_addend(isec.rel_addend(r));
            }
        }
        out[n] = rel;
        n += 1;
    };

    for file in &ctx.objs {
        for cie in &file.cies {
            if cie.is_leader {
                for rel in cie.rels(file) {
                    let offset =
                        cie.output_offset as u64 + rel.r_offset() - cie.input_offset as u64;
                    copy(file, cie.section, rel, offset, &mut eh_frame_buf);
                }
            }
        }
        for fde in &file.fdes {
            let cie = &file.cies[fde.cie_idx as usize];
            let base = file.fde_offset + fde.output_offset as u64;
            for rel in fde.rels(file) {
                let offset = base + rel.r_offset() - fde.input_offset as u64;
                copy(file, cie.section, rel, offset, &mut eh_frame_buf);
            }
        }
    }
}
