//! `.eh_frame_hdr`, the lookup table for exception-handling records.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::util::endian::Endian;

// .eh_frame_hdr is a lookup table for .eh_frame. Entries in .eh_frame_hdr
// are sorted by their dcorresponding function addresses, so tha the
// runtime can quickly find an exception-handling record for the current
// function by binary search. Without .eh_frame_hdr, the runtime would
// have had to do linear search in .eh_frame.
#[derive(Debug)]
pub struct EhFrameHdrSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub num_fdes: u64,
}

impl<E: Layout> EhFrameHdrSection<E> {
    pub const HEADER_SIZE: u64 = 12;

    pub fn new() -> EhFrameHdrSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".eh_frame_hdr", SHT_PROGBITS, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(4);
        hdr.shdr.sh_size.set(Self::HEADER_SIZE);
        EhFrameHdrSection { hdr, num_fdes: 0 }
    }
}

impl<E: Layout> Default for EhFrameHdrSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let num_fdes: u64 = ctx.objs.iter().map(|f| f.fdes.len() as u64).sum();
    let size = EhFrameHdrSection::<E>::HEADER_SIZE + num_fdes * 8;
    let sec = ctx.eh_frame_hdr.as_mut().unwrap();
    sec.num_fdes = num_fdes;
    sec.hdr.shdr.sh_size.set(size);
}

pub fn write_header<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = ctx.eh_frame_hdr.as_ref().unwrap();

    // Write a header. The actual table is written by EhFrameSection::copy_buf.
    buf[0] = 1;
    buf[1] = (DW_EH_PE_pcrel | DW_EH_PE_sdata4) as u8;
    buf[2] = DW_EH_PE_udata4 as u8;
    buf[3] = (DW_EH_PE_datarel | DW_EH_PE_sdata4) as u8;
    let eh_frame = ctx.eh_frame.hdr.shdr.sh_addr.get();
    let hdr = sec.hdr.shdr.sh_addr.get();
    let offset = eh_frame.wrapping_sub(hdr).wrapping_sub(4);
    E::Endian::write_u32(&mut buf[4..], offset as u32);
    E::Endian::write_u32(&mut buf[8..], sec.num_fdes as u32);
}
