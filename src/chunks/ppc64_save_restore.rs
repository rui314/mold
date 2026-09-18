//! PowerPC64 register save and restore functions.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

/// `.save_restore_regs`, the register save and restore routines that GCC
/// expects the linker to provide on PowerPC64 ELFv2.
pub fn new_header<E: Layout>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(
        ".save_restore_regs",
        SHT_PROGBITS,
        (SHF_ALLOC | SHF_EXECINSTR) as u64,
    );
    hdr.shdr.sh_addralign.set(16);
    let size = (crate::arch::ppc64v2::SAVE_RESTORE_INSNS.len() * 4) as u64;
    hdr.shdr.sh_size.set(size);
    hdr
}

pub fn copy_buf<E: Arch>(_ctx: &Context<E>, buf: &mut [u8]) {
    buf.copy_from_slice(&crate::arch::ppc64v2::save_restore_contents());
}
