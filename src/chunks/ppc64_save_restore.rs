//! PowerPC64 register save and restore functions.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::target::Target;

/// `.save_restore_regs`, the register save and restore routines that GCC
/// expects the linker to provide on PowerPC64 ELFv2.
pub fn new_header<E: Target>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(
        ".save_restore_regs",
        SHT_PROGBITS,
        (SHF_ALLOC | SHF_EXECINSTR) as u64,
    );
    hdr.shdr.sh_addralign.set(16);
    let size = (crate::target::ppc64v2::SAVE_RESTORE_INSNS.len() * 4) as u64;
    hdr.shdr.sh_size.set(size);
    hdr
}

pub fn copy_buf<E: Target>(_ctx: &Context<E>, buf: &mut [u8]) {
    buf.copy_from_slice(&crate::target::ppc64v2::save_restore_contents());
}
