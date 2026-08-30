//! Thread-local storage layout constants.
//!
//! Each thread has its own copy of the thread-local variables, initialized
//! from the PT_TLS segment's template image. A register, the thread
//! pointer (TP), refers to the thread's copy, and where exactly TP points
//! relative to the copy is decided by each psABI. The dynamic thread
//! pointer (DTP) is the base `__tls_get_addr` returns for offset 0.

use crate::arch::{Arch, Family};
use crate::elf::{ElfPhdr, PT_TLS};
use crate::util::{align_down, align_to};

/// The TP value the main executable's TLS block would have if the
/// template image were the block itself.
pub fn tp_addr<E: Arch>(phdr: &ElfPhdr) -> u64 {
    debug_assert_eq!(phdr.p_type, PT_TLS);
    match E::FAMILY {
        // x86, SPARC and s390x put TP past the end of the TLS block, so
        // variables are accessed with negative offsets.
        Family::X86_64 | Family::I386 | Family::Sparc64 | Family::S390x => {
            align_to(phdr.p_vaddr + phdr.p_memsz, phdr.p_align)
        }
        // ARM and SH4 prepend two words to the template image.
        Family::Arm64 | Family::Arm32 | Family::Sh4 => {
            align_down(phdr.p_vaddr - E::WORD_SIZE as u64 * 2, phdr.p_align)
        }
        // PowerPC and m68k put TP 0x7000 bytes past the start of the block
        // to maximize the reach of 16-bit signed displacements.
        Family::Ppc32 | Family::Ppc64V1 | Family::Ppc64V2 | Family::M68k => phdr.p_vaddr + 0x7000,
        // RISC-V and LoongArch use the start of the block.
        Family::RiscV | Family::LoongArch => phdr.p_vaddr,
    }
}

/// The address `__tls_get_addr` returns for offset 0.
pub fn dtp_addr<E: Arch>(phdr: &ElfPhdr) -> u64 {
    debug_assert_eq!(phdr.p_type, PT_TLS);
    match E::FAMILY {
        // PowerPC and m68k bias DTP by 0x8000, RISC-V by 0x800, so that
        // signed displacements cover the whole block.
        Family::Ppc32 | Family::Ppc64V1 | Family::Ppc64V2 | Family::M68k => phdr.p_vaddr + 0x8000,
        Family::RiscV => phdr.p_vaddr + 0x800,
        _ => phdr.p_vaddr,
    }
}
