//! The local relocations of a -static -pie image or a kext
//! (LC_DYSYMTAB's locreloff): one per pointer a loader must slide, the
//! job dyld's rebase info does for a dynamic image. A kernel slides
//! itself by them, kmutil a kext. mold's reldyn.rs holds the ELF
//! relative relocations they stand in for.

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
    check_fixup_range(ctx, &locs);
    locs.into_iter().map(|(_, addr)| addr).collect()
}

/// ld-prime rejects a link where a relocation address, a signed 32-bit
/// r_address, cannot reach a pointer. It counts the addresses on x86-64
/// from the first writable segment, as mold does, but on arm64 from
/// -image_base plus the __PAGEZERO size (or from a __TEXT -segaddr pins,
/// as ld64's machHeaderVmAddr does) - so with the default 4 GiB
/// __PAGEZERO, any nonzero -image_base fails. mold counts arm64
/// addresses from __TEXT, where the image starts, but refuses the same
/// links.
fn check_fixup_range<E: Target>(ctx: &Context<E>, locs: &[(u64, u64)]) {
    let base = if E::CPUTYPE == CPU_TYPE_ARM64 {
        let image_base = ctx.args.image_base.unwrap_or(0);
        ctx.args.segaddr("__TEXT").unwrap_or(image_base.wrapping_add(ctx.args.pagezero_size))
    } else {
        relocation_base(ctx)
    };
    let Some(&(atom, addr)) =
        locs.iter().find(|&&(_, addr)| i32::try_from(addr.wrapping_sub(base) as i64).is_err())
    else {
        return;
    };
    let (name, file) = atom_name(ctx, atom);
    crate::error!("atom address cannot fit in a fixup at '{name}' ({file})+{}", addr - atom);
}

/// The name of the atom starting at `addr`, and its file, for a
/// diagnostic.
fn atom_name<E: Target>(ctx: &Context<E>, addr: u64) -> (String, String) {
    for (id, isec) in ctx.isecs.iter().enumerate() {
        if !isec.is_alive() || ctx.isec_addr(id) != addr {
            continue;
        }
        let obj = &ctx.objs[isec.file as usize];
        return (ctx.atom_name(id).into_owned(), crate::passes::resolved_file_name(obj.mf));
    }
    (String::new(), String::new())
}

/// Where the relocation addresses count from: the first segment, or on
/// x86-64 the first writable one, as ld64 bases x86-64 relocations, but
/// for a kext's. A pointer below it (in a segment -segaddr pins lower,
/// like XNU's __HIB) gets a negative address.
fn relocation_base<E: Target>(ctx: &Context<E>) -> u64 {
    ctx.segments
        .iter()
        .filter(|seg| seg.name != "__PAGEZERO")
        .find(|seg| {
            E::CPUTYPE != CPU_TYPE_X86_64
                || ctx.args.is_kext()
                || segment_prot(seg.name) & VM_PROT_WRITE != 0
        })
        .map_or(0, |seg| seg.cmd.vmaddr)
}

/// Writes the records once the sections are in the buffer. Each is a
/// non-extern 8-byte UNSIGNED relocation whose r_symbolnum is the
/// ordinal of the section the pointer points into, and whose address
/// counts from relocation_base.
pub fn write<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut sects: Vec<(u64, u8)> = ctx
        .chunks
        .iter()
        .map(|&id| ctx.chunk_header(id))
        .filter(|hdr| hdr.is_sect)
        .map(|hdr| (hdr.addr, hdr.n_sect))
        .collect();
    sects.sort_unstable();
    let base = relocation_base(ctx);

    let mut off = ctx.local_relocs.hdr.fileoff as usize;
    for &addr in &ctx.local_relocs.locs {
        let (seg, seg_off) = segment_and_offset(ctx, addr);
        let pos = (ctx.segments[seg].cmd.fileoff + seg_off) as usize;
        let value = u64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap());
        // The last section starting at or below the target.
        let i = sects.partition_point(|&(start, _)| start <= value);
        let n_sect = sects[i.saturating_sub(1)].1;
        let rel = MachRel {
            r_address: addr.wrapping_sub(base) as u32,
            bits: u32::from(n_sect) | (3 << 25) | (u32::from(E::RELOC_UNSIGNED) << 28),
        };
        rel.write_to(&mut buf[off..]);
        off += size_of::<MachRel>();
    }
}
