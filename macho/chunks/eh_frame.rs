//! __TEXT,__eh_frame: the re-synthesized DWARF unwind records that
//! compact unwind can't express.

use crate::arch::Target;
use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::input_sections::FdeRecord;

#[derive(Debug)]
pub struct EhFrameSection {
    pub hdr: ChunkHeader,
}

impl EhFrameSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__eh_frame");
        hdr.p2align = 3;
        Self { hdr }
    }
}

impl Default for EhFrameSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Lays out __eh_frame, the surviving DWARF unwind records: the CIEs
/// the kept FDEs use, then the FDEs, as mold's EhFrameSection does (an
/// FDE's CIE pointer is a backward offset). Their offsets are needed
/// before layout, because the __unwind_info encoding embeds each FDE's
/// offset; an FDE it can't reach draws a warning (see
/// warn_eh_frame_too_large).
pub fn construct<E: Target>(ctx: &mut Context<E>) {
    // FDEs of folded copies duplicate their leader's; drop them, and
    // remap the unwind records' FDE indices around the removals as the
    // dead-strip pass does, so that none points past the shortened
    // table.
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    for (i, fde) in std::mem::take(&mut ctx.fdes).into_iter().enumerate() {
        if ctx.isecs[fde.isec as usize].replacement == crate::input_sections::NO_REPLACEMENT {
            fde_map[i] = kept_fdes.len();
            kept_fdes.push(fde);
        }
    }
    ctx.fdes = kept_fdes;
    let num_records = ctx.unwind_records.len();
    ctx.unwind_records.retain_mut(|rec| {
        if rec.fde_idx == crate::input_sections::UNWIND_NONE {
            return true;
        }
        let mapped = fde_map[rec.fde_idx as usize];
        if mapped == usize::MAX {
            // A folded copy's record; its leader has its own.
            return false;
        }
        rec.fde_idx = mapped as u32;
        true
    });
    // The compaction moved the surviving records; refresh the
    // subsections' ranges, which the __unwind_info encoding reads.
    if ctx.unwind_records.len() < num_records {
        crate::input_files::refresh_unwind_ranges(ctx);
    }
    if ctx.fdes.is_empty() {
        return;
    }

    for fde in &ctx.fdes {
        ctx.cies[fde.cie as usize].is_alive = true;
    }
    let mut off = 0;
    for cie in ctx.cies.iter_mut().filter(|cie| cie.is_alive) {
        cie.output_offset = off;
        off += cie.data.len() as u32;
    }
    for fde in &mut ctx.fdes {
        fde.output_offset = off;
        off += fde.data.len() as u32;
    }
    ctx.eh_frame.hdr.size = off as u64;
    ctx.chunks.push(ChunkId::EhFrame);
    warn_eh_frame_too_large(ctx);
}

/// Warns, as ld-prime does, if __unwind_info points a function at an
/// FDE beyond the reach of the 24 bits an entry has for its offset
/// (which it leaves 0 then, see encode_unwind_info).
fn warn_eh_frame_too_large<E: Target>(ctx: &Context<E>) {
    use crate::chunks::unwind_info::MAX_FDE_OFFSET;
    if !ctx.args.warn_eh_frame_too_large
        || ctx.eh_frame.hdr.size <= MAX_FDE_OFFSET as u64
        || !ctx.chunks.contains(&ChunkId::UnwindInfo)
    {
        return;
    }
    let out_of_reach = ctx.unwind_records.iter().any(|rec| {
        let isec = &ctx.isecs[rec.isec as usize];
        isec.is_emitted()
            && rec.fde().is_some_and(|fde| ctx.fdes[fde].output_offset > MAX_FDE_OFFSET)
    });
    if out_of_reach {
        crate::warn!(
            "__eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected"
        );
    }
}

/// Writes the re-synthesized __eh_frame section: live CIEs with their
/// personality cells rewritten to be GOT-relative, then live FDEs with
/// their CIE pointer, function pointer and LSDA pointer re-targeted.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    // `buf` is the chunk's own slice of the output.
    let base_off = 0;
    let base_addr = ctx.eh_frame.hdr.addr;

    for cie in &ctx.cies {
        if !cie.is_alive {
            continue;
        }
        let off = base_off + cie.output_offset as usize;
        buf[off..off + cie.data.len()].copy_from_slice(cie.data);

        if let Some(personality) = cie.personality {
            let cell_addr = base_addr + cie.output_offset as u64 + cie.personality_offset as u64;
            let val = ctx.symbols[personality].got_addr(ctx).wrapping_sub(cell_addr) as u32;
            let cell = off + cie.personality_offset as usize;
            buf[cell..cell + 4].copy_from_slice(&val.to_le_bytes());
        }
    }

    for fde in &ctx.fdes {
        let off = base_off + fde.output_offset as usize;
        let cie = &ctx.cies[fde.cie as usize];
        buf[off..off + fde.data.len()].copy_from_slice(fde.data);
        // The CIE pointer is the distance back to the owning CIE.
        let cie_ptr = fde.output_offset + 4 - cie.output_offset;
        let fde_addr = base_addr + fde.output_offset as u64;
        relocate_fde(ctx, fde, &mut buf[off..], fde_addr, cie_ptr);
    }
}

/// Rewrites the self-relative fields of an FDE copied to the start of
/// `buf` for its address in the output: the CIE pointer, to `cie_ptr`,
/// then pc_begin and the LSDA pointer, each of the size the CIE's
/// encoding gives. A final image and a -r output both write their FDEs
/// so.
pub fn relocate_fde<E: Target>(
    ctx: &Context<E>,
    fde: &FdeRecord,
    buf: &mut [u8],
    fde_addr: u64,
    cie_ptr: u32,
) {
    let cie = &ctx.cies[fde.cie as usize];
    buf[4..8].copy_from_slice(&cie_ptr.to_le_bytes());

    let func_addr = ctx.isecs[fde.isec as usize].addr(ctx) + fde.func_offset as u64;
    let pc_begin = func_addr.wrapping_sub(fde_addr + 8);
    match cie.pc_size() {
        4 => buf[8..12].copy_from_slice(&(pc_begin as u32).to_le_bytes()),
        _ => buf[8..16].copy_from_slice(&pc_begin.to_le_bytes()),
    }

    if let Some((lsda_isec, lsda_off)) = fde.lsda {
        let pos = fde.lsda_pos(cie.pc_size());
        let cell_addr = fde_addr + pos as u64;
        let val =
            (ctx.isecs[lsda_isec as usize].addr(ctx) + lsda_off as u64).wrapping_sub(cell_addr);
        match cie.lsda_size() {
            4 => buf[pos..pos + 4].copy_from_slice(&(val as u32).to_le_bytes()),
            _ => buf[pos..pos + 8].copy_from_slice(&val.to_le_bytes()),
        }
    }
}
