//! `.sframe`, a compact stack-unwinding format. Like `.eh_frame` it is
//! reconstructed: live FDEs are gathered, their FREs concatenated, and the
//! FDE index sorted by address.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::ObjId;
use crate::target::{Family, Target};

// .sframe is a compact stack-unwinding format. Like .eh_frame, the linker
// has to parse and reconstruct it: the output section is a single sorted
// index of FDEs (one per live function) followed by their FREs, so we
// gather the live FDEs from all input files, drop dead ones, concatenate
// their FREs, sort the index by PC and rewrite the header. mold reads and
// writes SFrame Version 3.
#[derive(Debug)]
pub struct SFrameSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub header: SFrameHeader<E>,
    /// The live FDEs as (file, index into the file's `sframe_fdes`).
    pub fdes: Vec<(ObjId, u32)>,
}

impl<E: Target> SFrameSection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".sframe", SHT_GNU_SFRAME, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(8);
        Self { hdr, header: SFrameHeader::<E>::default(), fdes: Vec::new() }
    }
}

impl<E: Target> Default for SFrameSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

// Lay out the output .sframe section. Like .eh_frame, .sframe is parsed
// and reconstructed by the linker, so here we pick the FDEs for live
// functions and arrange the FRE subsection. The header and the PC-sorted
// FDE index are written later by copy_buf, once addresses are known.
pub fn construct<E: Target>(ctx: &mut Context<E>) {
    let Some(abi) = E::SFRAME_ABI else {
        return;
    };
    let _t = ctx.timer("sframe");

    // Gather the FDEs that describe live functions and total the size of the
    // FRE subsection. FREs carry no relocations, so their contents are
    // position-independent and are simply concatenated by copy_buf.
    let mut fdes = Vec::new();
    let mut fre_len = 0u32;
    let mut num_fres = 0u32;
    for file in &ctx.objs {
        for (i, fde) in file.sframe_fdes.iter().enumerate() {
            if file.section_at(fde.section).is_alive() {
                fdes.push((file.id(), i as u32));
                fre_len += fde.fre.len() as u32;
                num_fres += fde.num_fres;
            }
        }
    }
    // If no live function has unwind info, leave the section empty so that
    // it is removed from the output.
    if fdes.is_empty() {
        return;
    }

    // We always emit PC-relative function pointers; we can additionally
    // mark the index as sorted unless this is a relocatable output,
    // where the final addresses (and hence the order) aren't known.
    let hdr = SFrameHeader::<E> {
        magic: U16::new(SFRAME_MAGIC),
        version: 3,
        flags: SFRAME_F_FDE_FUNC_START_PCREL
            | if ctx.args.relocatable { 0 } else { SFRAME_F_FDE_SORTED },
        abi_arch: abi,
        cfa_fixed_ra_offset: if E::FAMILY == Family::X86_64 { -8 } else { 0 },
        num_fdes: U32::new(fdes.len() as u32),
        freoff: U32::new((fdes.len() * SFrameFdeIdx::<E>::size()) as u32),
        fre_len: U32::new(fre_len),
        num_fres: U32::new(num_fres),
        ..SFrameHeader::<E>::default()
    };

    let sframe = &mut ctx.sframe;
    sframe.hdr.shdr.sh_size.set(
        (SFrameHeader::<E>::size() + fdes.len() * SFrameFdeIdx::<E>::size()) as u64
            + u64::from(hdr.fre_len.get()),
    );
    sframe.header = hdr;
    sframe.fdes = fdes;
}

pub fn sort<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }
    // The FDE index must be sorted by function address so that the runtime
    // can locate an entry by binary search. A relocatable output is the
    // exception: the addresses aren't known yet, so the index is left in its
    // current order and SFrameRelocSection emits a func_start relocation for
    // each entry in that same order.
    let addr = |ctx: &Context<E>, (fi, i): (ObjId, u32)| {
        let fde = &ctx.objs[fi.index()].sframe_fdes[i as usize];
        ctx.symbols[fde.sym].addr(ctx).wrapping_add(fde.addend as u64)
    };
    let mut fdes = std::mem::take(&mut ctx.sframe.fdes);
    fdes.sort_by_key(|&f| addr(ctx, f));
    ctx.sframe.fdes = fdes;
}

// Write the output .sframe section: the header, the FDE index and the
// concatenated FRE blocks. For an executable or a shared library the index
// is sorted by function address and func_start is resolved in place. For a
// relocatable output, addresses aren't known yet, so the index is left
// unsorted and func_start is emitted as a relocation by SFrameRelocSection.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let sframe = &ctx.sframe;
    let hdr_size = SFrameHeader::<E>::size();
    let idx_size = SFrameFdeIdx::<E>::size();
    // Write the header.
    sframe.header.write(buf);

    // Write the FDE index and concatenate the FRE blocks. Because
    // SFRAME_F_FDE_FUNC_START_PCREL is set, func_start_offset is the distance
    // from the field itself to the function the FDE describes.
    let fre_base = hdr_size + sframe.header.freoff.get() as usize;
    let mut fre_off = 0usize;
    for (i, &(fi, fi_idx)) in sframe.fdes.iter().enumerate() {
        let fde = &ctx.objs[fi.index()].sframe_fdes[fi_idx as usize];
        buf[fre_base + fre_off..fre_base + fre_off + fde.fre.len()].copy_from_slice(fde.fre);

        let func_start_offset = if ctx.args.relocatable {
            0
        } else {
            let func_addr = ctx.symbols[fde.sym].addr(ctx).wrapping_add(fde.addend as u64);
            let field_addr =
                sframe.hdr.shdr.sh_addr.get() + hdr_size as u64 + (i * idx_size) as u64;
            func_addr.wrapping_sub(field_addr) as i64
        };
        let ent = SFrameFdeIdx::<E> {
            func_start_offset: I64::new(func_start_offset),
            func_size: U32::new(fde.func_size),
            func_start_fre_off: U32::new(fre_off as u32),
        };
        ent.write(&mut buf[hdr_size + i * idx_size..]);
        fre_off += fde.fre.len();
    }
}
