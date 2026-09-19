//! __TEXT,__eh_frame: the re-synthesized DWARF unwind records that
//! compact unwind can't express.

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::output_chunks::ChunkHeader;

#[derive(Debug)]
pub struct EhFrameSection {
    pub hdr: ChunkHeader,
}

impl EhFrameSection {
    pub fn new() -> EhFrameSection {
        let mut hdr = ChunkHeader::new("__TEXT", "__eh_frame");
        hdr.p2align = 3;
        EhFrameSection { hdr }
    }
}

/// Writes the re-synthesized __eh_frame section: live CIEs with their
/// personality cells rewritten to be GOT-relative, then live FDEs with
/// their CIE pointer, function pointer and LSDA pointer re-targeted.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    // `buf` is the chunk's own slice of the output.
    let base_off = 0;
    let base_addr = ctx.eh_frame.hdr.addr;

    for cie in &ctx.cies {
        if !cie.is_alive {
            continue;
        }
        let off = base_off + cie.output_offset as usize;
        buf[off..off + cie.data.len()].copy_from_slice(&cie.data);

        if let Some(personality) = cie.personality {
            let cell_addr = base_addr + cie.output_offset as u64 + cie.personality_offset as u64;
            let val = ctx.sym_got_addr(personality).wrapping_sub(cell_addr) as u32;
            let cell = off + cie.personality_offset as usize;
            buf[cell..cell + 4].copy_from_slice(&val.to_le_bytes());
        }
    }

    for fde in &ctx.fdes {
        let off = base_off + fde.output_offset as usize;
        let fde_addr = base_addr + fde.output_offset as u64;
        buf[off..off + fde.data.len()].copy_from_slice(&fde.data);

        // The CIE pointer is the distance back to the owning CIE.
        let cie_ptr = fde.output_offset + 4 - ctx.cies[fde.cie as usize].output_offset;
        buf[off + 4..off + 8].copy_from_slice(&cie_ptr.to_le_bytes());

        // pc_begin: self-relative pointer to the function.
        let func_addr = ctx.isec_addr(fde.isec as usize) + fde.func_offset as u64;
        let pc_begin = func_addr.wrapping_sub(fde_addr + 8) as i64;
        buf[off + 8..off + 16].copy_from_slice(&pc_begin.to_le_bytes());

        if let Some((lsda_isec, lsda_off)) = fde.lsda {
            let mut pos = 24;
            // Skip the augmentation data length.
            while buf[off + pos] & 0x80 != 0 {
                pos += 1;
            }
            pos += 1;
            let cell_addr = fde_addr + pos as u64;
            let val = (ctx.isec_addr(lsda_isec as usize) + lsda_off as u64).wrapping_sub(cell_addr);
            match ctx.cies[fde.cie as usize].lsda_size {
                4 => buf[off + pos..off + pos + 4].copy_from_slice(&(val as u32).to_le_bytes()),
                8 => buf[off + pos..off + pos + 8].copy_from_slice(&val.to_le_bytes()),
                _ => unreachable!(),
            }
        }
    }
}
