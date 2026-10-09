//! PowerPC64 register save and restore functions.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

// GCC may emit references to the following functions in function
// prologues and epilogues if -Os is specified. For some reason, these
// functions are not in libgcc.a and are expected to be synthesized by the
// linker. There are variants for general-purpose, floating-point and
// vector registers.
pub const SAVE_RESTORE_INSNS: &[(&str, u32)] = &[
    ("_savegpr0_14", 0xf9c1ff70), // std r14,-144(r1)
    ("_savegpr0_15", 0xf9e1ff78), // std r15,-136(r1)
    ("_savegpr0_16", 0xfa01ff80), // std r16,-128(r1)
    ("_savegpr0_17", 0xfa21ff88), // std r17,-120(r1)
    ("_savegpr0_18", 0xfa41ff90), // std r18,-112(r1)
    ("_savegpr0_19", 0xfa61ff98), // std r19,-104(r1)
    ("_savegpr0_20", 0xfa81ffa0), // std r20,-96(r1)
    ("_savegpr0_21", 0xfaa1ffa8), // std r21,-88(r1)
    ("_savegpr0_22", 0xfac1ffb0), // std r22,-80(r1)
    ("_savegpr0_23", 0xfae1ffb8), // std r23,-72(r1)
    ("_savegpr0_24", 0xfb01ffc0), // std r24,-64(r1)
    ("_savegpr0_25", 0xfb21ffc8), // std r25,-56(r1)
    ("_savegpr0_26", 0xfb41ffd0), // std r26,-48(r1)
    ("_savegpr0_27", 0xfb61ffd8), // std r27,-40(r1)
    ("_savegpr0_28", 0xfb81ffe0), // std r28,-32(r1)
    ("_savegpr0_29", 0xfba1ffe8), // std r29,-24(r1)
    ("_savegpr0_30", 0xfbc1fff0), // std r30,-16(r1)
    ("_savegpr0_31", 0xfbe1fff8), // std r31,-8(r1)
    ("", 0xf8010010),             // std r0,16(r1)
    ("", 0x4e800020),             // blr
    ("_restgpr0_14", 0xe9c1ff70), // ld r14,-144(r1)
    ("_restgpr0_15", 0xe9e1ff78), // ld r15,-136(r1)
    ("_restgpr0_16", 0xea01ff80), // ld r16,-128(r1)
    ("_restgpr0_17", 0xea21ff88), // ld r17,-120(r1)
    ("_restgpr0_18", 0xea41ff90), // ld r18,-112(r1)
    ("_restgpr0_19", 0xea61ff98), // ld r19,-104(r1)
    ("_restgpr0_20", 0xea81ffa0), // ld r20,-96(r1)
    ("_restgpr0_21", 0xeaa1ffa8), // ld r21,-88(r1)
    ("_restgpr0_22", 0xeac1ffb0), // ld r22,-80(r1)
    ("_restgpr0_23", 0xeae1ffb8), // ld r23,-72(r1)
    ("_restgpr0_24", 0xeb01ffc0), // ld r24,-64(r1)
    ("_restgpr0_25", 0xeb21ffc8), // ld r25,-56(r1)
    ("_restgpr0_26", 0xeb41ffd0), // ld r26,-48(r1)
    ("_restgpr0_27", 0xeb61ffd8), // ld r27,-40(r1)
    ("_restgpr0_28", 0xeb81ffe0), // ld r28,-32(r1)
    ("_restgpr0_29", 0xe8010010), // ld r0,16(r1)
    ("", 0xeba1ffe8),             // ld r29,-24(r1)
    ("", 0x7c0803a6),             // mtlr r0
    ("", 0xebc1fff0),             // ld r30,-16(r1)
    ("", 0xebe1fff8),             // ld r31,-8(r1)
    ("", 0x4e800020),             // blr
    ("_restgpr0_30", 0xebc1fff0), // ld r30,-16(r1)
    ("_restgpr0_31", 0xe8010010), // ld r0,16(r1)
    ("", 0xebe1fff8),             // ld r31,-8(r1)
    ("", 0x7c0803a6),             // mtlr r0
    ("", 0x4e800020),             // blr
    ("_savegpr1_14", 0xf9ccff70), // std r14,-144(r12)
    ("_savegpr1_15", 0xf9ecff78), // std r15,-136(r12)
    ("_savegpr1_16", 0xfa0cff80), // std r16,-128(r12)
    ("_savegpr1_17", 0xfa2cff88), // std r17,-120(r12)
    ("_savegpr1_18", 0xfa4cff90), // std r18,-112(r12)
    ("_savegpr1_19", 0xfa6cff98), // std r19,-104(r12)
    ("_savegpr1_20", 0xfa8cffa0), // std r20,-96(r12)
    ("_savegpr1_21", 0xfaacffa8), // std r21,-88(r12)
    ("_savegpr1_22", 0xfaccffb0), // std r22,-80(r12)
    ("_savegpr1_23", 0xfaecffb8), // std r23,-72(r12)
    ("_savegpr1_24", 0xfb0cffc0), // std r24,-64(r12)
    ("_savegpr1_25", 0xfb2cffc8), // std r25,-56(r12)
    ("_savegpr1_26", 0xfb4cffd0), // std r26,-48(r12)
    ("_savegpr1_27", 0xfb6cffd8), // std r27,-40(r12)
    ("_savegpr1_28", 0xfb8cffe0), // std r28,-32(r12)
    ("_savegpr1_29", 0xfbacffe8), // std r29,-24(r12)
    ("_savegpr1_30", 0xfbccfff0), // std r30,-16(r12)
    ("_savegpr1_31", 0xfbecfff8), // std r31,-8(r12)
    ("", 0x4e800020),             // blr
    ("_restgpr1_14", 0xe9ccff70), // ld r14,-144(r12)
    ("_restgpr1_15", 0xe9ecff78), // ld r15,-136(r12)
    ("_restgpr1_16", 0xea0cff80), // ld r16,-128(r12)
    ("_restgpr1_17", 0xea2cff88), // ld r17,-120(r12)
    ("_restgpr1_18", 0xea4cff90), // ld r18,-112(r12)
    ("_restgpr1_19", 0xea6cff98), // ld r19,-104(r12)
    ("_restgpr1_20", 0xea8cffa0), // ld r20,-96(r12)
    ("_restgpr1_21", 0xeaacffa8), // ld r21,-88(r12)
    ("_restgpr1_22", 0xeaccffb0), // ld r22,-80(r12)
    ("_restgpr1_23", 0xeaecffb8), // ld r23,-72(r12)
    ("_restgpr1_24", 0xeb0cffc0), // ld r24,-64(r12)
    ("_restgpr1_25", 0xeb2cffc8), // ld r25,-56(r12)
    ("_restgpr1_26", 0xeb4cffd0), // ld r26,-48(r12)
    ("_restgpr1_27", 0xeb6cffd8), // ld r27,-40(r12)
    ("_restgpr1_28", 0xeb8cffe0), // ld r28,-32(r12)
    ("_restgpr1_29", 0xebacffe8), // ld r29,-24(r12)
    ("_restgpr1_30", 0xebccfff0), // ld r30,-16(r12)
    ("_restgpr1_31", 0xebecfff8), // ld r31,-8(r12)
    ("", 0x4e800020),             // blr
    ("_savefpr_14", 0xd9c1ff70),  // stfd f14,-144(r1)
    ("_savefpr_15", 0xd9e1ff78),  // stfd f15,-136(r1)
    ("_savefpr_16", 0xda01ff80),  // stfd f16,-128(r1)
    ("_savefpr_17", 0xda21ff88),  // stfd f17,-120(r1)
    ("_savefpr_18", 0xda41ff90),  // stfd f18,-112(r1)
    ("_savefpr_19", 0xda61ff98),  // stfd f19,-104(r1)
    ("_savefpr_20", 0xda81ffa0),  // stfd f20,-96(r1)
    ("_savefpr_21", 0xdaa1ffa8),  // stfd f21,-88(r1)
    ("_savefpr_22", 0xdac1ffb0),  // stfd f22,-80(r1)
    ("_savefpr_23", 0xdae1ffb8),  // stfd f23,-72(r1)
    ("_savefpr_24", 0xdb01ffc0),  // stfd f24,-64(r1)
    ("_savefpr_25", 0xdb21ffc8),  // stfd f25,-56(r1)
    ("_savefpr_26", 0xdb41ffd0),  // stfd f26,-48(r1)
    ("_savefpr_27", 0xdb61ffd8),  // stfd f27,-40(r1)
    ("_savefpr_28", 0xdb81ffe0),  // stfd f28,-32(r1)
    ("_savefpr_29", 0xdba1ffe8),  // stfd f29,-24(r1)
    ("_savefpr_30", 0xdbc1fff0),  // stfd f30,-16(r1)
    ("_savefpr_31", 0xdbe1fff8),  // stfd f31,-8(r1)
    ("", 0xf8010010),             // std r0,16(r1)
    ("", 0x4e800020),             // blr
    ("_restfpr_14", 0xc9c1ff70),  // lfd f14,-144(r1)
    ("_restfpr_15", 0xc9e1ff78),  // lfd f15,-136(r1)
    ("_restfpr_16", 0xca01ff80),  // lfd f16,-128(r1)
    ("_restfpr_17", 0xca21ff88),  // lfd f17,-120(r1)
    ("_restfpr_18", 0xca41ff90),  // lfd f18,-112(r1)
    ("_restfpr_19", 0xca61ff98),  // lfd f19,-104(r1)
    ("_restfpr_20", 0xca81ffa0),  // lfd f20,-96(r1)
    ("_restfpr_21", 0xcaa1ffa8),  // lfd f21,-88(r1)
    ("_restfpr_22", 0xcac1ffb0),  // lfd f22,-80(r1)
    ("_restfpr_23", 0xcae1ffb8),  // lfd f23,-72(r1)
    ("_restfpr_24", 0xcb01ffc0),  // lfd f24,-64(r1)
    ("_restfpr_25", 0xcb21ffc8),  // lfd f25,-56(r1)
    ("_restfpr_26", 0xcb41ffd0),  // lfd f26,-48(r1)
    ("_restfpr_27", 0xcb61ffd8),  // lfd f27,-40(r1)
    ("_restfpr_28", 0xcb81ffe0),  // lfd f28,-32(r1)
    ("_restfpr_29", 0xe8010010),  // ld r0,16(r1)
    ("", 0xcba1ffe8),             // lfd f29,-24(r1)
    ("", 0x7c0803a6),             // mtlr r0
    ("", 0xcbc1fff0),             // lfd f30,-16(r1)
    ("", 0xcbe1fff8),             // lfd f31,-8(r1)
    ("", 0x4e800020),             // blr
    ("_restfpr_30", 0xcbc1fff0),  // lfd f30,-16(r1)
    ("_restfpr_31", 0xe8010010),  // ld r0,16(r1)
    ("", 0xcbe1fff8),             // lfd f31,-8(r1)
    ("", 0x7c0803a6),             // mtlr r0
    ("", 0x4e800020),             // blr
    ("_savevr_20", 0x3980ff40),   // li r12,-192
    ("", 0x7e8c01ce),             // stvx v20,r12,r0
    ("_savevr_21", 0x3980ff50),   // li r12,-176
    ("", 0x7eac01ce),             // stvx v21,r12,r0
    ("_savevr_22", 0x3980ff60),   // li r12,-160
    ("", 0x7ecc01ce),             // stvx v22,r12,r0
    ("_savevr_23", 0x3980ff70),   // li r12,-144
    ("", 0x7eec01ce),             // stvx v23,r12,r0
    ("_savevr_24", 0x3980ff80),   // li r12,-128
    ("", 0x7f0c01ce),             // stvx v24,r12,r0
    ("_savevr_25", 0x3980ff90),   // li r12,-112
    ("", 0x7f2c01ce),             // stvx v25,r12,r0
    ("_savevr_26", 0x3980ffa0),   // li r12,-96
    ("", 0x7f4c01ce),             // stvx v26,r12,r0
    ("_savevr_27", 0x3980ffb0),   // li r12,-80
    ("", 0x7f6c01ce),             // stvx v27,r12,r0
    ("_savevr_28", 0x3980ffc0),   // li r12,-64
    ("", 0x7f8c01ce),             // stvx v28,r12,r0
    ("_savevr_29", 0x3980ffd0),   // li r12,-48
    ("", 0x7fac01ce),             // stvx v29,r12,r0
    ("_savevr_30", 0x3980ffe0),   // li r12,-32
    ("", 0x7fcc01ce),             // stvx v30,r12,r0
    ("_savevr_31", 0x3980fff0),   // li r12,-16
    ("", 0x7fec01ce),             // stvx v31,r12,r0
    ("", 0x4e800020),             // blr
    ("_restvr_20", 0x3980ff40),   // li r12,-192
    ("", 0x7e8c00ce),             // lvx v20,r12,r0
    ("_restvr_21", 0x3980ff50),   // li r12,-176
    ("", 0x7eac00ce),             // lvx v21,r12,r0
    ("_restvr_22", 0x3980ff60),   // li r12,-160
    ("", 0x7ecc00ce),             // lvx v22,r12,r0
    ("_restvr_23", 0x3980ff70),   // li r12,-144
    ("", 0x7eec00ce),             // lvx v23,r12,r0
    ("_restvr_24", 0x3980ff80),   // li r12,-128
    ("", 0x7f0c00ce),             // lvx v24,r12,r0
    ("_restvr_25", 0x3980ff90),   // li r12,-112
    ("", 0x7f2c00ce),             // lvx v25,r12,r0
    ("_restvr_26", 0x3980ffa0),   // li r12,-96
    ("", 0x7f4c00ce),             // lvx v26,r12,r0
    ("_restvr_27", 0x3980ffb0),   // li r12,-80
    ("", 0x7f6c00ce),             // lvx v27,r12,r0
    ("_restvr_28", 0x3980ffc0),   // li r12,-64
    ("", 0x7f8c00ce),             // lvx v28,r12,r0
    ("_restvr_29", 0x3980ffd0),   // li r12,-48
    ("", 0x7fac00ce),             // lvx v29,r12,r0
    ("_restvr_30", 0x3980ffe0),   // li r12,-32
    ("", 0x7fcc00ce),             // lvx v30,r12,r0
    ("_restvr_31", 0x3980fff0),   // li r12,-16
    ("", 0x7fec00ce),             // lvx v31,r12,r0
    ("", 0x4e800020),             // blr
];

/// `.save_restore_regs`, the register save and restore routines that GCC
/// expects the linker to provide on PowerPC64.
pub fn new_header<E: Target>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(
        ".save_restore_regs",
        SHT_PROGBITS,
        (SHF_ALLOC | SHF_EXECINSTR) as u64,
    );
    hdr.shdr.sh_addralign.set(16);
    let size = (SAVE_RESTORE_INSNS.len() * 4) as u64;
    hdr.shdr.sh_size.set(size);
    hdr
}

pub fn copy_buf<E: Target>(_ctx: &Context<E>, buf: &mut [u8]) {
    for (i, &(_, insn)) in SAVE_RESTORE_INSNS.iter().enumerate() {
        E::write_u32(&mut buf[i * 4..], insn);
    }
}
