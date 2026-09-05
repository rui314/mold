//! `.ARM.exidx`, the exception handling index of ARM32 executables.
//!
//! Where other targets look up the unwind record for a PC in
//! `.eh_frame_hdr`, ARM32 uses `.ARM.exidx`: a table of records, each a
//! signed 31-bit self-relative address of a function's start and a value
//! that is either CANTUNWIND, a compact unwind record encoded into the
//! word (its most significant bit set), or a self-relative address of a
//! larger record in `.ARM.extab`. The table must be sorted by address.
//!
//! The input `.ARM.exidx` sections are gathered into an ordinary output
//! section for layout, but the output is written by this chunk, which
//! sorts the records, appends a sentinel and merges adjacent functions
//! with identical unwind information.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::{output_section, ChunkHeader, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::elf::*;
use crate::util::sign_extend;

const CANTUNWIND: u32 = 1;
const ENTRY_SIZE: usize = 8;

#[derive(Debug)]
pub struct ArmExidxSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    /// The output section holding the input `.ARM.exidx` sections.
    pub output_section: OutputSectionId,
}

/// Replaces the `.ARM.exidx` output section by the synthetic one.
pub fn create<E: Arch>(ctx: &mut Context<E>) {
    let Some(i) = ctx.chunks.iter().position(|&id| match id {
        ChunkId::Output(osec) => {
            ctx.output_sections[osec.index()].hdr.shdr.sh_type.get() == SHT_ARM_EXIDX
        }
        _ => false,
    }) else {
        return;
    };
    let ChunkId::Output(osec) = ctx.chunks[i] else {
        unreachable!()
    };

    let mut hdr = ChunkHeader::<E>::new(".ARM.exidx", SHT_ARM_EXIDX, SHF_ALLOC as u64);
    hdr.shdr.sh_addralign.set(4);
    ctx.arm_exidx = Some(ArmExidxSection {
        hdr,
        output_section: osec,
    });
    ctx.chunks[i] = ChunkId::ArmExidx;

    // The input sections are consumed here rather than copied.
    for m in ctx.output_sections[osec.index()].members.clone() {
        let section = ctx.input_section(m).section_ref();
        ctx.objs[section.file.index()].kill_section(section.shndx as usize);
    }
}

pub fn compute_section_size<E: Arch>(ctx: &mut Context<E>) {
    let osec = ctx.arm_exidx.as_ref().unwrap().output_section;
    output_section::compute_section_size(ctx, osec);
    let size = ctx.output_sections[osec.index()].hdr.shdr.sh_size.get();
    // +8 for sentinel
    let sec = ctx.arm_exidx.as_mut().unwrap();
    sec.hdr.shdr.sh_size.set(size + ENTRY_SIZE as u64);
    // plus the sentinel
}

// .ARM.exidx's sh_link should be set to the .text section index.
// Runtime doesn't care about it, but the binutils's strip command does.
pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    if let Some(text) = ctx.find_chunk_by_name(b".text") {
        let shndx = ctx.chunk_header(text).shndx;
        ctx.arm_exidx.as_mut().unwrap().hdr.shdr.sh_link.set(shndx);
    }
}

/// Once addresses are known, merges the records of adjacent functions
/// with the same unwind information, which shrinks the section.
pub fn remove_duplicate_entries<E: Arch>(ctx: &mut Context<E>) {
    // The input sections are laid out at the synthetic section's address,
    // which their PC-relative records depend on.
    let sec = ctx.arm_exidx.as_ref().unwrap();
    let (osec, addr) = (sec.output_section, sec.hdr.shdr.sh_addr.get());
    ctx.output_sections[osec.index()].hdr.shdr.sh_addr.set(addr);
    let size = contents(ctx).len() as u64;
    ctx.arm_exidx.as_mut().unwrap().hdr.shdr.sh_size.set(size);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let contents = contents(ctx);
    debug_assert_eq!(contents.len(), buf.len());
    buf.copy_from_slice(&contents);
}

// Returns the end of the text segment
fn text_end<E: Arch>(ctx: &Context<E>) -> u64 {
    ctx.chunks
        .iter()
        .map(|&id| ctx.chunk_header(id).shdr)
        .filter(|shdr| shdr.sh_flags.get() & SHF_EXECINSTR as u64 != 0)
        .map(|shdr| shdr.sh_addr.get() + shdr.sh_size.get())
        .max()
        .unwrap_or(0)
}

// ARM executables use an .ARM.exidx section to look up an exception
// handling record for the current instruction pointer. The table needs
// to be sorted by their addresses.
//
// Other target uses .eh_frame_hdr instead for the same purpose.
// I don't know why only ARM uses the different mechanism, but it's
// likely that it's due to some historical reason.
//
// This function returns contents of .ARM.exidx.
fn contents<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let sec = ctx.arm_exidx.as_ref().unwrap();
    let osec = &ctx.output_sections[sec.output_section.index()];
    let base = sec.hdr.shdr.sh_addr.get();

    // .ARM.exidx records consists of a signed 31-bit relative address
    // and a 32-bit value. The relative address indicates the start
    // address of a function that the record covers. The value is one of
    // the followings:
    //
    // 1. CANTUNWIND indicating that there's no unwinding info for the function,
    // 2. a compact unwinding record encoded into a 32-bit value, or
    // 3. a 31-bit relative address which points to a larger record in
    // the .ARM.extab section.
    //
    // CANTUNWIND is value 1. The most significant bit is set in (2) but
    // not in (3). So we can distinguished them just by looking at a value.

    // We reserve one extra slot for the sentinel
    let num_entries = osec.hdr.shdr.sh_size.get() as usize / ENTRY_SIZE + 1;
    let mut buf = vec![0u8; num_entries * ENTRY_SIZE];

    // Write section contents to the buffer
    output_section::write_to(ctx, sec.output_section, &mut buf);
    let sentinel_addr = base + ((num_entries - 1) * ENTRY_SIZE) as u64;
    let mut entries: Vec<(u32, u32)> = buf
        .chunks_exact(ENTRY_SIZE)
        .map(|e| (E::Endian::read_u32(e), E::Endian::read_u32(&e[4..])))
        .collect();
    // Fill in sentinel fields
    entries[num_entries - 1] = (text_end(ctx).wrapping_sub(sentinel_addr) as u32, CANTUNWIND);

    // Entry's addresses are relative to themselves. In order to sort
    // records by address, we first translate them so that the addresses
    // are relative to the beginning of the section.
    let is_relative = |val: u32| val != CANTUNWIND && val & 0x8000_0000 == 0;
    entries
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, (addr, val))| {
            let offset = (i * ENTRY_SIZE) as u32;
            *addr = (sign_extend(*addr as u64, 31) as u32).wrapping_add(offset);
            if is_relative(*val) {
                *val = 0x7fff_ffff & val.wrapping_add(offset);
            }
        });
    entries.sort_by_key(|&(addr, _)| addr);

    // Remove duplicate adjacent entries. That is, if two adjacent functions
    // have the same compact unwind info or are both CANTUNWIND, we can
    // merge them into a single address range.
    entries.dedup_by_key(|&mut (_, val)| val);

    // Make addresses relative to themselves.
    entries
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, (addr, val))| {
            let offset = (i * ENTRY_SIZE) as u32;
            *addr = 0x7fff_ffff & addr.wrapping_sub(offset);
            if is_relative(*val) {
                *val = 0x7fff_ffff & val.wrapping_sub(offset);
            }
        });

    let mut out = vec![0u8; entries.len() * ENTRY_SIZE];
    for (i, (addr, val)) in entries.iter().enumerate() {
        E::Endian::write_u32(&mut out[i * ENTRY_SIZE..], *addr);
        E::Endian::write_u32(&mut out[i * ENTRY_SIZE + 4..], *val);
    }
    out
}
