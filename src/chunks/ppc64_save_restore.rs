//! PowerPC64 register save and restore functions.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

/// `.save_restore_regs`, the register save and restore routines that GCC
/// expects the linker to provide on PowerPC64 ELFv2.
#[derive(Debug)]
pub struct Ppc64SaveRestoreSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Layout> Ppc64SaveRestoreSection<E> {
    pub fn new() -> Ppc64SaveRestoreSection<E> {
        let mut hdr = ChunkHeader::<E>::new(
            ".save_restore_regs",
            SHT_PROGBITS,
            (SHF_ALLOC | SHF_EXECINSTR) as u64,
        );
        hdr.shdr.sh_addralign.set(16);
        let size = (crate::arch::ppc64v2::SAVE_RESTORE_INSNS.len() * 4) as u64;
        hdr.shdr.sh_size.set(size);
        Ppc64SaveRestoreSection { hdr }
    }
}

impl<E: Layout> Default for Ppc64SaveRestoreSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Arch>(_ctx: &Context<E>, buf: &mut [u8]) {
    buf.copy_from_slice(&crate::arch::ppc64v2::save_restore_contents());
}
