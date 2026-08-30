//! PowerPC64 ELFv2, the ABI of little-endian PowerPC systems ("ppc64le").
//!
//! PC-relative loads and stores only arrived with Power10 in 2021, so
//! position-independent code for earlier processors can't address the
//! GOT relative to the program counter. Instead, most functions keep
//! `r2`, the TOC pointer, equal to `.got + 0x8000`, so that a GOT entry
//! is one `r2`-relative load away. Since a module has a single GOT, calls
//! within a module leave `r2` alone.
//!
//! A function compiled for such processors has two entry points. The
//! global entry point, which assumes its own address is in `r12`,
//! computes the callee's TOC pointer from `r12` with `addis`/`addi` and
//! falls through to the local entry point, which assumes `r2` is already
//! right. A call into another module therefore loads the callee's address
//! into `r12` (typically from `.got.plt`) and branches there; the callee
//! sets up its own `r2`. The distance between the two entry points is
//! encoded in the symbol's `st_other`.
//!
//! Power10 code uses 8-byte prefixed instructions with 34-bit PC-relative
//! offsets and treats `r2` as a scratch register. `_NOTOC` relocations
//! mark such call sites. When Power10 code calls a function that expects
//! the TOC pointer, or vice versa, the call goes through a range
//! extension thunk that recomputes `r2`.
//!
//! The psABI calls `.got.plt` `.plt`; the runtime only cares about
//! segments, so the sections keep their usual names here.
//!
//! https://github.com/rui314/psabi/blob/main/ppc64v2.pdf

use std::sync::atomic::Ordering;

use crate::arch::{Arch, Family, ThunkLayout};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{check_tlsle, InputSection};
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::thunks::Thunk;
use crate::util::{bits, is_int};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct Ppc64V2;

impl Layout for Ppc64V2 {
    type Endian = LittleEndian;
    const IS_64: bool = true;
    const IS_RELA: bool = true;
}

fn lo(x: u64) -> u64 {
    x & 0xffff
}

fn hi(x: u64) -> u64 {
    x >> 16
}

fn ha(x: u64) -> u64 {
    x.wrapping_add(0x8000) >> 16
}

fn higha(x: u64) -> u64 {
    ha(x) & 0xffff
}

fn r32(loc: &[u8]) -> u32 {
    LittleEndian::read_u32(loc)
}

fn w16(loc: &mut [u8], v: u64) {
    LittleEndian::write_u16(loc, v as u16);
}

fn w32(loc: &mut [u8], v: u32) {
    LittleEndian::write_u32(loc, v);
}

fn w64(loc: &mut [u8], v: u64) {
    LittleEndian::write_u64(loc, v);
}

fn or16(loc: &mut [u8], v: u64) {
    let cur = LittleEndian::read_u16(loc);
    LittleEndian::write_u16(loc, cur | v as u16);
}

fn or32(loc: &mut [u8], v: u32) {
    let cur = r32(loc);
    w32(loc, cur | v);
}

/// Writes the 34-bit immediate of a prefixed instruction, which is split
/// between the prefix word and the instruction word.
fn write34(loc: &mut [u8], x: u64) {
    let prefix = (r32(loc) & 0xfffc_0000) | bits(x, 33, 16) as u32;
    let insn = (r32(&loc[4..]) & 0xffff_0000) | bits(x, 15, 0) as u32;
    w32(loc, prefix);
    w32(&mut loc[4..], insn);
}

/// The 26-bit displacement field of a branch.
fn branch_field(val: i64) -> u32 {
    (bits(val as u64, 25, 2) << 2) as u32
}

/// The address of the TOC pointer, `.got + 0x8000`.
fn toc(ctx: &Context<Ppc64V2>) -> u64 {
    ctx.symbols[ctx.syms.toc.expect("PPC64 has a .TOC. symbol")].addr(ctx)
}

fn is_power10(ctx: &Context<Ppc64V2>) -> bool {
    ctx.is_power10.load(Ordering::Relaxed)
}

/// The distance from a function's global entry point to its local one.
fn local_entry_offset(ctx: &Context<Ppc64V2>, sym: &Symbol) -> u64 {
    match sym.esym(ctx).ppc64_local_entry() {
        0 | 1 => 0,
        7 => fatal!(ctx, "{sym}: local entry offset 7 is reserved"),
        n => 1 << n,
    }
}

/// GCC may call these functions from prologues and epilogues under `-Os`.
/// They aren't in libgcc; the linker synthesizes them. Each label names
/// the instruction it precedes, and execution falls through the entries.
/// There are variants for general-purpose, floating-point and vector
/// registers.
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

/// The contents of the `.save_restore_regs` section.
pub fn save_restore_contents() -> Vec<u8> {
    SAVE_RESTORE_INSNS
        .iter()
        .flat_map(|&(_, insn)| insn.to_le_bytes())
        .collect()
}

impl Arch for Ppc64V2 {
    const NAME: &'static str = "ppc64v2";
    const FAMILY: Family = Family::Ppc64V2;
    const PAGE_SIZE: u64 = 65536;
    const E_MACHINE: u32 = EM_PPC64;
    const PLT_HDR_SIZE: u64 = 52;
    const PLT_SIZE: u64 = 4;
    const PLTGOT_SIZE: u64 = 0;
    const THUNK: Option<ThunkLayout> = Some(ThunkLayout {
        header_size: 0,
        entry_size: 24,
    });
    const TRAP: &'static [u8] = &[0x08, 0x00, 0xe0, 0x7f]; // trap

    const R_COPY: u32 = R_PPC64_COPY;
    const R_GLOB_DAT: u32 = R_PPC64_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_PPC64_JMP_SLOT;
    const R_ABS: u32 = R_PPC64_ADDR64;
    const R_RELATIVE: u32 = R_PPC64_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_PPC64_IRELATIVE);
    const R_DTPOFF: u32 = R_PPC64_DTPREL64;
    const R_TPOFF: u32 = R_PPC64_TPREL64;
    const R_DTPMOD: u32 = R_PPC64_DTPMOD64;
    const R_FUNCALL: &'static [u32] = &[R_PPC64_REL24, R_PPC64_REL24_NOTOC];

    fn rel_to_string(r_type: u32) -> String {
        ppc64_rel_to_string(r_type)
    }

    fn eflags(_ctx: &Context<Self>) -> u32 {
        2
    }

    /// `.plt` serves lazy symbol resolution only. All PLT calls go through
    /// thunks, which read the target from `.got.plt` and jump there, so
    /// once a symbol is resolved the PLT is skipped.
    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u32; 13] = [
            // Get PC.
            0x7c08_02a6, // mflr    r0
            0x429f_0005, // bcl     20, 31, 4
            0x7d68_02a6, // mflr    r11
            0x7c08_03a6, // mtlr    r0
            // Compute the PLT entry index.
            0x398c_ffd4, // addi    r12, r12, -44
            0x7c0b_6050, // subf    r0, r11, r12
            0x7800_f082, // rldicl  r0, r0, 62, 2
            // Compute the address of .got.plt.
            0x3d6b_0000, // addis   r11, r11, GOTPLT_OFFSET@ha
            0x396b_0000, // addi    r11, r11, GOTPLT_OFFSET@lo
            // Load .got.plt[0] and .got.plt[1] and branch to .got.plt[0].
            0xe98b_0000, // ld      r12, 0(r11)
            0x7d89_03a6, // mtctr   r12
            0xe96b_0008, // ld      r11, 8(r11)
            0x4e80_0420, // bctr
        ];
        for (i, &insn) in INSN.iter().enumerate() {
            w32(&mut buf[i * 4..], insn);
        }
        let val = ctx
            .gotplt
            .hdr
            .shdr
            .sh_addr
            .wrapping_sub(ctx.plt.hdr.shdr.sh_addr)
            .wrapping_sub(8);
        or32(&mut buf[28..], higha(val) as u32);
        or32(&mut buf[32..], lo(val) as u32);
    }

    /// The caller has already put the entry's address in `r12`.
    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let offset = ctx.plt.hdr.shdr.sh_addr.wrapping_sub(sym.plt_addr(ctx));
        w32(buf, 0x4b00_0000 | (offset as u32 & 0x00ff_ffff)); // b plt0
    }

    /// Thunks read GOT entries directly, so `.plt.got` has no entries.
    fn write_pltgot_entry(_ctx: &Context<Self>, _buf: &mut [u8], _sym: &Symbol) {}

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        isec: &InputSection,
        rel: &ElfRel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        match rel.r_type {
            R_NONE => {}
            R_PPC64_ADDR64 => w64(loc, val),
            R_PPC64_REL32 => {
                eh_frame::check_range(
                    ctx,
                    isec,
                    rel,
                    val.wrapping_sub(p) as i64,
                    -(1 << 31),
                    1 << 31,
                );
                w32(loc, val.wrapping_sub(p) as u32);
            }
            R_PPC64_REL64 => w64(loc, val.wrapping_sub(p)),
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        for rel in isec.rels::<Self>(file) {
            if rel.r_type == R_NONE || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            match rel.r_type {
                R_PPC64_GOT_TPREL16_HA | R_PPC64_GOT_TPREL_PCREL34 => sym.add_flags(NEEDS_GOTTP),
                R_PPC64_REL24 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_PPC64_REL24_NOTOC => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                    ctx.is_power10.store(true, Ordering::Relaxed);
                }
                R_PPC64_GOT16
                | R_PPC64_GOT16_LO
                | R_PPC64_GOT16_HI
                | R_PPC64_GOT16_HA
                | R_PPC64_PLT16_HA
                | R_PPC64_PLT_PCREL34
                | R_PPC64_PLT_PCREL34_NOTOC
                | R_PPC64_GOT_PCREL34 => sym.add_flags(NEEDS_GOT),
                R_PPC64_GOT_TLSGD16_HA | R_PPC64_GOT_TLSGD_PCREL34 => sym.add_flags(NEEDS_TLSGD),
                R_PPC64_GOT_TLSLD16_HA | R_PPC64_GOT_TLSLD_PCREL34 => {
                    ctx.needs_tlsld.store(true, Ordering::Relaxed)
                }
                R_PPC64_TPREL16_HA | R_PPC64_TPREL34 => check_tlsle(ctx, isec, sym, &rel),
                R_PPC64_ADDR64
                | R_PPC64_REL14
                | R_PPC64_REL32
                | R_PPC64_REL64
                | R_PPC64_TOC16_HA
                | R_PPC64_TOC16_LO
                | R_PPC64_TOC16_LO_DS
                | R_PPC64_TOC16_DS
                | R_PPC64_REL16_HA
                | R_PPC64_REL16_LO
                | R_PPC64_PLT16_HI
                | R_PPC64_PLT16_LO
                | R_PPC64_PLT16_LO_DS
                | R_PPC64_PCREL34
                | R_PPC64_PLTSEQ
                | R_PPC64_PLTSEQ_NOTOC
                | R_PPC64_PLTCALL
                | R_PPC64_PLTCALL_NOTOC
                | R_PPC64_GOT_TPREL16_LO_DS
                | R_PPC64_GOT_TLSGD16_LO
                | R_PPC64_GOT_TLSLD16_LO
                | R_PPC64_TPREL16_LO
                | R_PPC64_TPREL16_LO_DS
                | R_PPC64_TLS
                | R_PPC64_TLSGD
                | R_PPC64_TLSLD
                | R_PPC64_DTPREL16_HA
                | R_PPC64_DTPREL16_LO
                | R_PPC64_DTPREL16_LO_DS
                | R_PPC64_DTPREL34
                | R_PPC64_ENTRY => {}
                _ => error!(
                    ctx,
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    fn apply_reloc_alloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        let toc = toc(ctx);
        let got = ctx.got.hdr.shdr.sh_addr;

        for (i, rel) in isec.rels::<Self>(file).iter().enumerate() {
            if rel.r_type == R_NONE {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let off = rel.r_offset as usize;
            let s = sym.addr(ctx);
            let a = rel.r_addend as u64;
            let p = isec.addr(ctx) + rel.r_offset;
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);
            let loc = &mut buf[off..];

            // Thunk entries have two entry points: one that saves r2 to
            // the caller's stack frame first, and one 8 bytes in that
            // doesn't.
            let r2save_thunk = || sym.thunk_addr(ctx, p);
            let no_r2save_thunk = || sym.thunk_addr(ctx, p) + 8;

            match rel.r_type {
                R_PPC64_TOC16_HA => w16(loc, ha(sa.wrapping_sub(toc))),
                R_PPC64_TOC16_LO => w16(loc, lo(sa.wrapping_sub(toc))),
                R_PPC64_TOC16_DS => {
                    isec.check_range(ctx, i, sa.wrapping_sub(toc) as i64, -(1 << 15), 1 << 15);
                    or16(loc, sa.wrapping_sub(toc) & 0xfffc);
                }
                R_PPC64_TOC16_LO_DS => or16(loc, sa.wrapping_sub(toc) & 0xfffc),
                R_PPC64_REL24 => {
                    if sym.has_plt(&ctx.symbols) || !sym.esym(ctx).ppc64_preserves_r2() {
                        let val = r2save_thunk().wrapping_add(a).wrapping_sub(p) as i64;
                        or32(loc, branch_field(val));

                        // The thunk saves r2 to the caller's r2 save slot,
                        // which must be restored after the call returns.
                        // The placeholder for that is usually a NOP after
                        // the BL.
                        if loc.len() >= 8 && r32(&loc[4..]) == 0x6000_0000 {
                            w32(&mut loc[4..], 0xe841_0018); // ld r2, 24(r1)
                        }
                    } else {
                        let mut val = s
                            .wrapping_add(local_entry_offset(ctx, sym))
                            .wrapping_add(a)
                            .wrapping_sub(p) as i64;
                        if !is_int(val, 26) {
                            val = no_r2save_thunk().wrapping_add(a).wrapping_sub(p) as i64;
                        }
                        or32(loc, branch_field(val));
                    }
                }
                R_PPC64_REL24_NOTOC => {
                    let mut val = pcrel as i64;
                    if sym.has_plt(&ctx.symbols)
                        || sym.esym(ctx).ppc64_uses_toc()
                        || !is_int(val, 26)
                    {
                        val = no_r2save_thunk().wrapping_add(a).wrapping_sub(p) as i64;
                    }
                    or32(loc, branch_field(val));
                }
                R_PPC64_REL14 => or32(loc, (bits(pcrel, 15, 2) << 2) as u32),
                R_PPC64_REL32 => w32(loc, pcrel as u32),
                R_PPC64_REL64 => w64(loc, pcrel),
                R_PPC64_REL16_HA => w16(loc, ha(pcrel)),
                R_PPC64_REL16_LO => w16(loc, lo(pcrel)),
                R_PPC64_GOT16 => w16(loc, g().wrapping_sub(toc)),
                R_PPC64_GOT16_LO => w16(loc, lo(g().wrapping_sub(toc))),
                R_PPC64_GOT16_HI => w16(loc, hi(g().wrapping_sub(toc))),
                R_PPC64_GOT16_HA => w16(loc, ha(g().wrapping_sub(toc))),
                R_PPC64_PLT16_HA => w16(loc, ha(sym.got_addr(ctx).wrapping_sub(toc))),
                R_PPC64_PLT16_HI => w16(loc, hi(sym.got_addr(ctx).wrapping_sub(toc))),
                R_PPC64_PLT16_LO => w16(loc, lo(sym.got_addr(ctx).wrapping_sub(toc))),
                R_PPC64_PLT16_LO_DS => or16(loc, sym.got_addr(ctx).wrapping_sub(toc) & 0xfffc),
                R_PPC64_PLT_PCREL34 | R_PPC64_PLT_PCREL34_NOTOC | R_PPC64_GOT_PCREL34 => {
                    write34(loc, sym.got_addr(ctx).wrapping_sub(p))
                }
                R_PPC64_PCREL34 => write34(loc, pcrel),
                R_PPC64_GOT_TPREL16_HA => w16(loc, ha(sym.gottp_addr(ctx).wrapping_sub(toc))),
                R_PPC64_GOT_TPREL16_LO_DS => {
                    or16(loc, sym.gottp_addr(ctx).wrapping_sub(toc) & 0xfffc)
                }
                R_PPC64_GOT_TPREL_PCREL34 => write34(loc, sym.gottp_addr(ctx).wrapping_sub(p)),
                R_PPC64_GOT_TLSGD16_HA => w16(loc, ha(sym.tlsgd_addr(ctx).wrapping_sub(toc))),
                R_PPC64_GOT_TLSGD16_LO => w16(loc, lo(sym.tlsgd_addr(ctx).wrapping_sub(toc))),
                R_PPC64_GOT_TLSGD_PCREL34 => write34(loc, sym.tlsgd_addr(ctx).wrapping_sub(p)),
                R_PPC64_GOT_TLSLD16_HA => {
                    w16(loc, ha(ctx.got.tlsld_addr::<Self>().wrapping_sub(toc)))
                }
                R_PPC64_GOT_TLSLD16_LO => {
                    w16(loc, lo(ctx.got.tlsld_addr::<Self>().wrapping_sub(toc)))
                }
                R_PPC64_GOT_TLSLD_PCREL34 => {
                    write34(loc, ctx.got.tlsld_addr::<Self>().wrapping_sub(p))
                }
                R_PPC64_DTPREL16_HA => w16(loc, ha(sa.wrapping_sub(ctx.dtp_addr))),
                R_PPC64_DTPREL16_LO => w16(loc, lo(sa.wrapping_sub(ctx.dtp_addr))),
                R_PPC64_DTPREL16_LO_DS => or16(loc, sa.wrapping_sub(ctx.dtp_addr) & 0xfffc),
                R_PPC64_DTPREL34 => write34(loc, sa.wrapping_sub(ctx.dtp_addr)),
                R_PPC64_TPREL16_HA => w16(loc, ha(sa.wrapping_sub(ctx.tp_addr))),
                R_PPC64_TPREL16_LO => w16(loc, lo(sa.wrapping_sub(ctx.tp_addr))),
                R_PPC64_TPREL16_LO_DS => or16(loc, sa.wrapping_sub(ctx.tp_addr) & 0xfffc),
                R_PPC64_TPREL34 => write34(loc, sa.wrapping_sub(ctx.tp_addr)),
                R_PPC64_ADDR64
                | R_PPC64_PLTSEQ
                | R_PPC64_PLTSEQ_NOTOC
                | R_PPC64_PLTCALL
                | R_PPC64_PLTCALL_NOTOC
                | R_PPC64_TLS
                | R_PPC64_TLSGD
                | R_PPC64_TLSLD
                | R_PPC64_ENTRY => {}
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        isec.for_each_reloc::<Self>(ctx, |rel, i| {
            if rel.r_type == R_NONE || isec.record_undef_error(ctx, &rel) {
                return;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            let frag = isec.fragment(ctx, &rel);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), rel.r_addend as u64),
            };
            let sa = s.wrapping_add(a);
            let loc = &mut buf[rel.r_offset as usize..];

            match rel.r_type {
                R_PPC64_ADDR64 => match isec.tombstone(ctx, sym, frag.map(|(f, _)| f)) {
                    Some(v) => w64(loc, v),
                    None => w64(loc, sa),
                },
                R_PPC64_ADDR32 => {
                    isec.check_range(ctx, i, sa as i64, 0, 1 << 32);
                    w32(loc, sa as u32);
                }
                R_PPC64_DTPREL64 => w64(loc, sa.wrapping_sub(ctx.dtp_addr)),
                _ => fatal!(
                    ctx,
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        });
    }

    /// All PLT calls go through thunks. So do calls between code that
    /// expects `r2` to hold the TOC pointer and code that doesn't.
    fn always_needs_thunk(ctx: &Context<Self>, sym: &Symbol, rel: &ElfRel) -> bool {
        sym.has_plt(&ctx.symbols)
            || (rel.r_type == R_PPC64_REL24 && !sym.esym(ctx).ppc64_preserves_r2())
            || (rel.r_type == R_PPC64_REL24_NOTOC && sym.esym(ctx).ppc64_uses_toc())
    }

    fn write_thunk(ctx: &Context<Self>, thunk: &Thunk, addr: u64, buf: &mut [u8]) {
        // A PLT thunk reads the destination from .got.plt or .got and
        // jumps there.
        const PLT_THUNK: [u32; 6] = [
            0xf841_0018, // std   r2, 24(r1)
            0x6000_0000, // nop
            0x3d82_0000, // addis r12, r2, foo@gotplt@toc@ha
            0xe98c_0000, // ld    r12, foo@gotplt@toc@lo(r12)
            0x7d89_03a6, // mtctr r12
            0x4e80_0420, // bctr
        ];
        const PLT_THUNK_POWER10: [u32; 6] = [
            0xf841_0018, // std   r2, 24(r1)
            0x6000_0000, // nop
            0x0410_0000, // pld   r12, foo@gotplt@pcrel
            0xe580_0000,
            0x7d89_03a6, // mtctr r12
            0x4e80_0420, // bctr
        ];
        // A thunk to a function in this module jumps to its local entry
        // point.
        const LOCAL_THUNK: [u32; 6] = [
            0xf841_0018, // std   r2, 24(r1)
            0x6000_0000, // nop
            0x3d82_0000, // addis r12, r2,  foo@toc@ha
            0x398c_0000, // addi  r12, r12, foo@toc@lo
            0x7d89_03a6, // mtctr r12
            0x4e80_0420, // bctr
        ];
        const LOCAL_THUNK_POWER10: [u32; 6] = [
            0xf841_0018, // std   r2, 24(r1)
            0x6000_0000, // nop
            0x0610_0000, // pla   r12, foo@pcrel
            0x3980_0000,
            0x7d89_03a6, // mtctr r12
            0x4e80_0420, // bctr
        ];

        let toc = toc(ctx);
        let power10 = is_power10(ctx);

        for (i, &id) in thunk.symbols.iter().enumerate() {
            let sym = &ctx.symbols[id];
            let p = addr + thunk.offsets[i];
            let entry = &mut buf[thunk.offsets[i] as usize..][..24];

            let (insns, target) = if sym.has_plt(&ctx.symbols) {
                let got = if sym.has_got(&ctx.symbols) {
                    sym.got_addr(ctx)
                } else {
                    sym.gotplt_addr(ctx)
                };
                (
                    if power10 {
                        &PLT_THUNK_POWER10
                    } else {
                        &PLT_THUNK
                    },
                    got,
                )
            } else {
                (
                    if power10 {
                        &LOCAL_THUNK_POWER10
                    } else {
                        &LOCAL_THUNK
                    },
                    sym.addr(ctx),
                )
            };
            for (j, &insn) in insns.iter().enumerate() {
                w32(&mut entry[j * 4..], insn);
            }
            if power10 {
                write34(&mut entry[8..], target.wrapping_sub(p).wrapping_sub(8));
            } else {
                or32(&mut entry[8..], higha(target.wrapping_sub(toc)) as u32);
                or32(&mut entry[12..], lo(target.wrapping_sub(toc)) as u32);
            }
        }
    }
}
