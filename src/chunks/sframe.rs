//! `.sframe`, a compact stack-unwinding format. Like `.eh_frame` it is
//! reconstructed: live FDEs are gathered, their FREs concatenated, and the
//! FDE index sorted by address.

use crate::arch::{Arch, Family};
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::ObjId;

#[derive(Debug)]
pub struct SFrameSection {
    pub hdr: ChunkHeader,
    pub header: SFrameHeader,
    /// The live FDEs as (file, index into the file's `sframe_fdes`).
    pub fdes: Vec<(ObjId, u32)>,
}

impl SFrameSection {
    pub fn new() -> SFrameSection {
        let mut hdr = ChunkHeader::new(".sframe", SHT_GNU_SFRAME, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign = 8;
        SFrameSection {
            hdr,
            header: SFrameHeader::default(),
            fdes: Vec::new(),
        }
    }
}

impl Default for SFrameSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Lays out the output section. If no live function has unwind info the
/// section is left empty and dropped from the output.
pub fn construct<E: Arch>(ctx: &mut Context<E>) {
    let Some(abi) = E::SFRAME_ABI else {
        return;
    };
    let _t = ctx.timer("sframe");

    let mut fdes = Vec::new();
    for file in &ctx.objs {
        for (i, fde) in file.sframe_fdes.iter().enumerate() {
            if file.section_at(fde.section).is_alive() {
                fdes.push((file.id(), i as u32));
            }
        }
    }
    if fdes.is_empty() {
        return;
    }

    // Function pointers are always PC-relative; the index is marked
    // sorted unless this is a relocatable output.
    let mut hdr = SFrameHeader {
        magic: SFRAME_MAGIC,
        version: 3,
        flags: SFRAME_F_FDE_FUNC_START_PCREL
            | if ctx.args.relocatable {
                0
            } else {
                SFRAME_F_FDE_SORTED
            },
        abi_arch: abi,
        cfa_fixed_ra_offset: if E::FAMILY == Family::X86_64 { -8 } else { 0 },
        num_fdes: fdes.len() as u32,
        freoff: (fdes.len() * SFrameFdeIdx::size::<E>()) as u32,
        ..SFrameHeader::default()
    };
    for &(fi, i) in &fdes {
        let fde = &ctx.objs[fi.index()].sframe_fdes[i as usize];
        hdr.fre_len += fde.fre.len() as u32;
        hdr.num_fres += fde.num_fres;
    }

    let sframe = &mut ctx.sframe;
    sframe.hdr.shdr.sh_size = (SFrameHeader::size::<E>() + fdes.len() * SFrameFdeIdx::size::<E>())
        as u64
        + hdr.fre_len as u64;
    sframe.header = hdr;
    sframe.fdes = fdes;
}

/// Sorts the FDEs by function address, which fixes the order in which
/// both `copy_buf` and `sframe_reloc::copy_buf` emit them.
pub fn sort<E: Arch>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }
    let addr = |ctx: &Context<E>, (fi, i): (ObjId, u32)| {
        let fde = &ctx.objs[fi.index()].sframe_fdes[i as usize];
        ctx.symbols[fde.sym]
            .addr(ctx)
            .wrapping_add(fde.addend as u64)
    };
    let mut fdes = std::mem::take(&mut ctx.sframe.fdes);
    fdes.sort_by_key(|&f| addr(ctx, f));
    ctx.sframe.fdes = fdes;
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let sframe = &ctx.sframe;
    let hdr_size = SFrameHeader::size::<E>();
    let idx_size = SFrameFdeIdx::size::<E>();
    sframe.header.write::<E>(buf);

    let fre_base = hdr_size + sframe.header.freoff as usize;
    let mut fre_off = 0usize;
    for (i, &(fi, fi_idx)) in sframe.fdes.iter().enumerate() {
        let fde = &ctx.objs[fi.index()].sframe_fdes[fi_idx as usize];
        buf[fre_base + fre_off..fre_base + fre_off + fde.fre.len()].copy_from_slice(fde.fre);

        let func_start_offset = if ctx.args.relocatable {
            0
        } else {
            let func_addr = ctx.symbols[fde.sym]
                .addr(ctx)
                .wrapping_add(fde.addend as u64);
            let field_addr = sframe.hdr.shdr.sh_addr + hdr_size as u64 + (i * idx_size) as u64;
            func_addr.wrapping_sub(field_addr) as i64
        };
        let ent = SFrameFdeIdx {
            func_start_offset,
            func_size: fde.func_size,
            func_start_fre_off: fre_off as u32,
        };
        ent.write::<E>(&mut buf[hdr_size + i * idx_size..]);
        fre_off += fde.fre.len();
    }
}

/// `.rela.sframe`, for relocatable outputs.
#[derive(Debug)]
pub struct SFrameRelocSection {
    pub hdr: ChunkHeader,
}

impl SFrameRelocSection {
    pub fn new<E: Arch>() -> SFrameRelocSection {
        let mut hdr = ChunkHeader::new(".rela.sframe", SHT_RELA, SHF_INFO_LINK as u64);
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        hdr.shdr.sh_entsize = ElfRel::size::<E>() as u64;
        SFrameRelocSection { hdr }
    }
}

pub mod sframe_reloc {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let n = ctx.sframe.fdes.len();
        let sec = ctx.sframe_reloc.as_mut().unwrap();
        sec.hdr.shdr.sh_size = (n * ElfRel::size::<E>()) as u64;
        sec.hdr.shdr.sh_link = ctx.symtab.hdr.shndx;
        sec.hdr.shdr.sh_info = ctx.sframe.hdr.shndx;
    }

    /// One relocation per FDE for its func_start field, in index order.
    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let Some(r_type) = E::R_SFRAME else {
            return;
        };
        let size = ElfRel::size::<E>();
        for (i, &(fi, fi_idx)) in ctx.sframe.fdes.iter().enumerate() {
            let fde = &ctx.objs[fi.index()].sframe_fdes[fi_idx as usize];
            let sym = &ctx.symbols[fde.sym];
            let r_offset = ctx.sframe.hdr.shdr.sh_addr
                + SFrameHeader::size::<E>() as u64
                + (i * SFrameFdeIdx::size::<E>()) as u64;

            let (r_sym, r_addend) = if sym.st_type() == STT_SECTION {
                // Section symbols are recreated per output section.
                let target = sym.input_section_ref().unwrap();
                (
                    ctx.output_section(target.output_section.unwrap()).hdr.shndx,
                    fde.addend + target.offset() as i64,
                )
            } else {
                (sym.output_sym_idx(ctx), fde.addend)
            };
            ElfRel::new(r_offset, r_type, r_sym, r_addend).write::<E>(&mut buf[i * size..]);
        }
    }
}
