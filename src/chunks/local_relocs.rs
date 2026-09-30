//! The local relocations of a -static -pie image (LC_DYSYMTAB's
//! locreloff): one per pointer a loader must slide, the job dyld's
//! rebase info does for a dynamic image. A kernel slides itself by
//! them. mold's reldyn.rs holds the ELF relative relocations they
//! stand in for.

use crate::chunks::{ChunkHeader, segment_and_offset, segment_prot};
use crate::context::Context;
use crate::macho::*;
use crate::target::Target;

#[derive(Debug)]
pub struct LocalRelocsSection {
    pub hdr: ChunkHeader,
    /// The addresses of the pointers, in the order the records list
    /// them.
    pub locs: Vec<u64>,
}

impl LocalRelocsSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), locs: Vec::new() }
    }
}

impl Default for LocalRelocsSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Lists the pointers as ld-prime does: atom by atom in address order,
/// each atom's from the last to the first, the order an assembler
/// emits relocations in.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u64> {
    let mut locs = crate::chunks::rebase_info::rebase_locations(ctx);
    locs.sort_unstable_by_key(|&(atom, addr)| (atom, std::cmp::Reverse(addr)));
    locs.into_iter().map(|(_, addr)| addr).collect()
}

/// Writes the records once the sections are in the buffer. Each is a
/// non-extern 8-byte UNSIGNED relocation whose r_symbolnum is the
/// ordinal of the section the pointer points into, and whose address
/// counts from the first segment - on x86-64 from the first writable
/// one, as ld64 bases x86-64 relocations.
pub fn write<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut sects: Vec<(u64, u8)> = ctx
        .chunks
        .iter()
        .map(|&id| ctx.chunk_header(id))
        .filter(|hdr| hdr.is_sect)
        .map(|hdr| (hdr.addr, hdr.n_sect))
        .collect();
    sects.sort_unstable();
    let base = ctx
        .segments
        .iter()
        .filter(|seg| seg.name != "__PAGEZERO")
        .find(|seg| E::CPUTYPE != CPU_TYPE_X86_64 || segment_prot(seg.name) & VM_PROT_WRITE != 0)
        .map_or(0, |seg| seg.cmd.vmaddr);

    let mut off = ctx.local_relocs.hdr.fileoff as usize;
    for &addr in &ctx.local_relocs.locs {
        let (seg, seg_off) = segment_and_offset(ctx, addr);
        let pos = (ctx.segments[seg].cmd.fileoff + seg_off) as usize;
        let value = u64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap());
        // The last section starting at or below the target.
        let i = sects.partition_point(|&(start, _)| start <= value);
        let n_sect = sects[i.saturating_sub(1)].1;
        let rel = MachRel {
            r_address: (addr - base) as u32,
            bits: u32::from(n_sect) | (3 << 25) | (u32::from(E::RELOC_UNSIGNED) << 28),
        };
        rel.write_to(&mut buf[off..]);
        off += size_of::<MachRel>();
    }
}
