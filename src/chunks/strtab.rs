//! `.strtab`, names of non-dynamic symbols.

use crate::arch::{Arch, Family};
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

// .strtab is referenced by .strtab and contains symbol names. Note that
// .strtab is not needed at runtime; one can remove the section from an
// ELF file without breaking it. Strings that runtime accesses are stored
// in .dynstr.
#[derive(Debug)]
pub struct StrtabSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Layout> StrtabSection<E> {
    pub fn new() -> StrtabSection<E> {
        StrtabSection {
            hdr: ChunkHeader::<E>::new(".strtab", SHT_STRTAB, 0),
        }
    }
}

impl<E: Layout> Default for StrtabSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

// Offsets in .strtab for ARM32 mapping symbols
pub const ARM: u32 = 1;
pub const THUMB: u32 = 4;
pub const DATA: u32 = 7;

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let mut offset = 1u64;

    // ARM32 uses $a, $t and $t mapping symbols to mark the beginning of
    // ARM, Thumb and data in text, respectively. These symbols don't
    // affect correctness of the program but helps disassembler to
    // disassemble machine code appropriately.
    if E::FAMILY == Family::Arm32 && !ctx.args.strip_all {
        offset += b"$a\0$t\0$d\0".len() as u64;
    }

    for id in ctx.chunks.clone() {
        let hdr = ctx.chunk_header_mut(id);
        hdr.strtab_offset = offset;
        offset += hdr.strtab_size;
    }
    for file in &mut ctx.objs {
        file.base.strtab_offset = offset;
        offset += file.base.strtab_size;
    }
    for file in &mut ctx.dsos {
        file.base.strtab_offset = offset;
        offset += file.base.strtab_size;
    }
    ctx.strtab
        .hdr
        .shdr
        .sh_size
        .set(if offset == 1 { 0 } else { offset });
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    buf[0] = 0;
    if E::FAMILY == Family::Arm32 && !ctx.args.strip_all {
        buf[1..10].copy_from_slice(b"$a\0$t\0$d\0");
    }
}
