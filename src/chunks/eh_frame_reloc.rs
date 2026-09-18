//! `.rel.eh_frame` and `.rela.eh_frame` for relocatable outputs.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::ObjectFile;

// EhFrameRelocSection contains relocation records for .eh_frame. It is used
// only for relocatable outputs (an .o file rather than an executable or .so).
pub fn new_header<E: Arch>() -> ChunkHeader<E> {
    let (name, ty) =
        if E::IS_RELA { (".rela.eh_frame", SHT_RELA) } else { (".rel.eh_frame", SHT_REL) };
    let mut hdr = ChunkHeader::<E>::new(name, ty, SHF_INFO_LINK as u64);
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    let entsize = std::mem::size_of::<ElfRel<E>>() as u64;
    hdr.shdr.sh_entsize.set(entsize);
    hdr
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let count: usize = ctx
        .objs
        .par_iter()
        .map(|file| {
            let cies: usize =
                file.cies.iter().filter(|c| c.is_leader).map(|c| c.rels(file).len()).sum();
            let fdes: usize = file.fdes.iter().map(|f| f.rels(file).len()).sum();
            cies + fdes
        })
        .sum();
    let size = (count * std::mem::size_of::<ElfRel<E>>()) as u64;
    let sec = ctx.eh_frame_reloc.as_mut().unwrap();
    sec.shdr.sh_size.set(size);
    sec.shdr.sh_link.set(ctx.symtab.shndx);
    sec.shdr.sh_info.set(ctx.eh_frame.shndx);
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
        let mut rel = ElfRel::<E>::new(ctx.eh_frame.shdr.sh_addr.get() + offset, r.r_type(), 0, 0);

        let is_section = sym.st_type() == STT_SECTION;
        let (r_sym, addend) = crate::chunks::reloc::output_symidx_addend(ctx, sym, || {
            if E::IS_RELA || is_section {
                isec.rel_addend(r)
            } else {
                // Ordinary REL symbols keep the addend already in .eh_frame.
                0
            }
        })
        .expect("relocation refers to a section without output");
        rel.set_r_sym(r_sym);
        if E::IS_RELA {
            rel.set_r_addend(addend);
        } else if ctx.args.relocatable && is_section {
            if let Some(eh) = eh_frame_buf {
                E::write_addend(&mut eh[offset as usize..], addend, r);
            }
        }
        out[n] = rel;
        n += 1;
    };

    // Relocations must be sorted by offset, because a consumer of this
    // object (including mold itself) pairs each CIE and FDE with a
    // contiguous run of relocations while scanning .eh_frame once. The
    // output places every leader CIE before all FDEs, so emit the CIE
    // relocations of all files first and only then the FDE relocations.
    // Interleaving them per file would place a later file's CIE
    // relocation (typically a personality routine pointer) after an
    // earlier file's FDE relocations.
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
    }
    for file in &ctx.objs {
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
