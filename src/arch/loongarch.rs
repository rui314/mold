//! LoongArch is a new RISC ISA announced in 2021 by Loongson. The ISA
//! feels like a modernized MIPS with a hint of RISC-V flavor, although
//! it's not compatible with either one.
//!
//! While LoongArch is a fresh and clean ISA, its technological advantage
//! over other modern RISC ISAs such as RISC-V doesn't seem to be very
//! significant. It appears that the real selling point of LoongArch is
//! that the ISA is developed and controlled by a Chinese company,
//! reflecting a desire for domestic CPUs. Loongson is actively working on
//! bootstrapping the entire ecosystem for LoongArch, sending patches to
//! Linux, GCC, LLVM, etc.
//!
//! Speaking of the ISA, all instructions are 4 byte long and aligned to 4
//! byte boundaries in LoongArch. It has 32 general-purpose registers.
//! Among these, $t0 - $t8 (aliases for $r12 - $r20) are temporary
//! registers that we can use in our PLT.
//!
//! Just like RISC-V, LoongArch supports section-shrinking relaxations.
//! That is, it allows linkers to rewrite certain instruction sequences to
//! shorter ones. Sections are not an atomic unit of copying.
//!
//! https://github.com/loongson/la-abi-specs/blob/release/laelf.adoc

// Binary literals are grouped by instruction field.
#![allow(clippy::unusual_byte_groupings)]

use std::marker::PhantomData;

use crate::arch::{Arch, Family};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::input_sections::{
    check_tlsle, scan_absrel, scan_pcrel, scan_tlsdesc, InputSection, RelocDelta,
};
use crate::relax::compute_distance;
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::util::{align_to, bits, is_int, overwrite_uleb, read_uleb, sign_extend};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct LoongArchTarget<const IS_64: bool>(PhantomData<()>);

pub type LoongArch64 = LoongArchTarget<true>;
pub type LoongArch32 = LoongArchTarget<false>;

impl<const IS_64: bool> Layout for LoongArchTarget<IS_64> {
    type Endian = LittleEndian;
    const IS_64: bool = IS_64;
    const IS_RELA: bool = true;
}

fn page(val: u64) -> u64 {
    val & !0xfff
}

// A PC-relative address with a 32 bit offset is materialized in a
// register with the following instructions:
//
// pcalau12i $rN, %pc_hi20(sym)
// addi.d    $rN, $rN, %lo12(sym)
//
// PCALAU12I materializes bits [63:12] by computing (pc + imm << 12)
// and zero-clear [11:0]. ADDI.D sign-extends its 12 bit immediate and
// add it to the register. To compensate the sign-extension, PCALAU12I
// needs to materialize a 0x1000 larger value than the desired [63:12]
// if [11:0] is sign-extended.
//
// This is similar but different from RISC-V because RISC-V's AUIPC
// doesn't zero-clear [11:0].
fn hi20(val: u64, pc: u64) -> u64 {
    bits(page(val.wrapping_add(0x800)).wrapping_sub(page(pc)), 31, 12)
}

// A PC-relative 64-bit address is materialized with the following
// instructions for the large code model:
//
// pcalau12i $rN, %pc_hi20(sym)
// addi.d    $rM, $zero, %lo12(sym)
// lu32i.d   $rM, %pc64_lo20(sym)
// lu52i.d   $rM, $r12, %pc64_hi12(sym)
// add.d     $rN, $rN, $rM
//
// PCALAU12I computes (pc + imm << 12) to materialize a 64-bit value.
// ADDI.D adds a sign-extended 12 bit value to a register. LU32I.D and
// LU52I.D simply set bits to [51:31] and to [63:53], respectively.
//
// Compensating all the sign-extensions is a bit complicated. The
// psABI gives the following formula.
fn higher(val: u64, pc: u64) -> u64 {
    let compensation = if val & 0x800 != 0 {
        0x1000u64.wrapping_sub(0x1_0000_0000)
    } else {
        0
    };
    let val = val.wrapping_add(0x8000_0000).wrapping_add(compensation);
    page(val).wrapping_sub(page(pc.wrapping_sub(8)))
}

fn higher20(val: u64, pc: u64) -> u64 {
    bits(higher(val, pc), 51, 32)
}

fn highest12(val: u64, pc: u64) -> u64 {
    bits(higher(val, pc), 63, 52)
}

fn insn(loc: &[u8]) -> u32 {
    LittleEndian::read_u32(loc)
}

fn set_insn(loc: &mut [u8], v: u32) {
    LittleEndian::write_u32(loc, v);
}

/// Instruction formats, named after their immediate fields.
fn write_k12(loc: &mut [u8], val: u64) {
    // opcode, [11:0], rj, rd
    set_insn(
        loc,
        (insn(loc) & 0b1111111111_000000000000_11111_11111) | (bits(val, 11, 0) << 10) as u32,
    );
}

fn write_k16(loc: &mut [u8], val: u64) {
    // opcode, [15:0], rj, rd
    set_insn(
        loc,
        (insn(loc) & 0b111111_0000000000000000_11111_11111) | (bits(val, 15, 0) << 10) as u32,
    );
}

fn write_j20(loc: &mut [u8], val: u64) {
    // opcode, [19:0], rd
    set_insn(
        loc,
        (insn(loc) & 0b1111111_00000000000000000000_11111) | (bits(val, 19, 0) << 5) as u32,
    );
}

fn write_d5k16(loc: &mut [u8], val: u64) {
    // opcode, [15:0], rj, [20:16]
    let v = (insn(loc) & 0b111111_0000000000000000_11111_00000)
        | (bits(val, 15, 0) << 10) as u32
        | bits(val, 20, 16) as u32;
    set_insn(loc, v);
}

fn write_d10k16(loc: &mut [u8], val: u64) {
    // opcode, [15:0], [25:16]
    let v = (insn(loc) & 0b111111_0000000000000000_0000000000)
        | (bits(val, 15, 0) << 10) as u32
        | bits(val, 25, 16) as u32;
    set_insn(loc, v);
}

fn rd(insn: u32) -> u32 {
    bits(insn as u64, 4, 0) as u32
}

fn rj(insn: u32) -> u32 {
    bits(insn as u64, 9, 5) as u32
}

fn set_rj(loc: &mut [u8], rj: u32) {
    debug_assert!(rj < 32);
    set_insn(
        loc,
        (insn(loc) & 0b111111_1111111111111111_00000_11111) | (rj << 5),
    );
}

/// Rewrites the instruction at `loc` into `pcaddi $rd, imm`, keeping
/// its destination register.
fn write_pcaddi(loc: &mut [u8], val: u64) {
    set_insn(loc, 0x1800_0000 | rd(insn(loc)));
    write_j20(loc, val);
}

/// Adds `val` to the value at `loc`, or subtracts it, for the ADD/SUB
/// relocation families that debug info and tables are built from.
fn add_bits(loc: &mut [u8], size: u32, val: u64, subtract: bool) {
    let apply = |cur: u64| {
        if subtract {
            cur.wrapping_sub(val)
        } else {
            cur.wrapping_add(val)
        }
    };
    match size {
        6 => loc[0] = (loc[0] & 0b1100_0000) | (apply(loc[0] as u64) as u8 & 0b0011_1111),
        8 => loc[0] = apply(loc[0] as u64) as u8,
        16 => LittleEndian::write_u16(loc, apply(LittleEndian::read_u16(loc) as u64) as u16),
        32 => LittleEndian::write_u32(loc, apply(LittleEndian::read_u32(loc) as u64) as u32),
        64 => LittleEndian::write_u64(loc, apply(LittleEndian::read_u64(loc))),
        _ => unreachable!(),
    }
}

fn add_uleb(loc: &mut [u8], val: u64, subtract: bool) {
    let cur = read_uleb(&mut &*loc);
    overwrite_uleb(
        loc,
        if subtract {
            cur.wrapping_sub(val)
        } else {
            cur.wrapping_add(val)
        },
    );
}

// Returns true if isec's i'th relocation refers to the following
// relaxable instructioon pair.
//
// pcalau12i $t0, 0         # R_LARCH_GOT_PC_HI20, R_LARCH_RELAX
// ld.d      $t0, $t0, 0    # R_LARCH_GOT_PC_LO12, R_LARCH_RELAX
fn is_relaxable_got_load<E: Arch>(ctx: &Context<E>, isec: &InputSection, i: usize) -> bool {
    let rels = isec.rels::<E>(&ctx.objs[isec.file.index()]);
    let file = &ctx.objs[isec.file.index()];
    let sym = &ctx.symbols[file.base.symbols[rels.at(i).r_sym as usize]];
    let contents = isec.original_contents(file);

    if !ctx.args.relax
        || !sym.is_pcrel_linktime_const(ctx)
        || i + 3 >= rels.len()
        || rels.at(i + 1).r_type != R_LARCH_RELAX
        || rels.at(i + 2).r_type != R_LARCH_GOT_PC_LO12
        || rels.at(i + 2).r_offset != rels.at(i).r_offset + 4
        || rels.at(i + 3).r_type != R_LARCH_RELAX
    {
        return false;
    }
    let insn1 = insn(&contents[rels.at(i).r_offset as usize..]);
    let insn2 = insn(&contents[rels.at(i).r_offset as usize + 4..]);
    let is_ld_d = insn2 & 0xffc0_0000 == 0x28c0_0000;
    rd(insn1) == rd(insn2) && rd(insn2) == rj(insn2) && is_ld_d
}

/// Relocations that only guide relaxation and never relocate anything.
fn is_marker(r_type: u32) -> bool {
    matches!(
        r_type,
        R_NONE | R_LARCH_RELAX | R_LARCH_MARK_LA | R_LARCH_MARK_PCREL | R_LARCH_ALIGN
    )
}

const PLT_ENTRY_64: [u32; 4] = [
    0x1a00_000f, // pcalau12i $t3, %pc_hi20(func@.got.plt)
    0x28c0_01ef, // ld.d      $t3, $t3, %lo12(func@.got.plt)
    0x4c00_01ed, // jirl      $t1, $t3, 0
    0x002a_0000, // break
];

const PLT_ENTRY_32: [u32; 4] = [
    0x1a00_000f, // pcalau12i $t3, %pc_hi20(func@.got.plt)
    0x2880_01ef, // ld.w      $t3, $t3, %lo12(func@.got.plt)
    0x4c00_01ed, // jirl      $t1, $t3, 0
    0x002a_0000, // break
];

fn write_insns(buf: &mut [u8], insns: &[u32]) {
    for (i, &insn) in insns.iter().enumerate() {
        set_insn(&mut buf[i * 4..], insn);
    }
}

impl<const IS_64: bool> LoongArchTarget<IS_64> {
    /// Writes a PLT-style entry that jumps to the address stored at `got`.
    fn write_plt_stub(buf: &mut [u8], got: u64, plt: u64) {
        write_insns(buf, if IS_64 { &PLT_ENTRY_64 } else { &PLT_ENTRY_32 });
        write_j20(buf, hi20(got, plt));
        write_k12(&mut buf[4..], got);
    }
}

impl<const IS_64: bool> Arch for LoongArchTarget<IS_64> {
    const NAME: &'static str = if IS_64 { "loongarch64" } else { "loongarch32" };
    const FAMILY: Family = Family::LoongArch;
    const PAGE_SIZE: u64 = 65536;
    const E_MACHINE: u32 = EM_LOONGARCH;
    const PLT_HDR_SIZE: u64 = 32;
    const PLT_SIZE: u64 = 16;
    const PLTGOT_SIZE: u64 = 16;
    // The C++ LOONGARCH64 and LOONGARCH32 target structs each record this
    // instruction:
    // break 0
    // break 0
    const TRAP: &'static [u8] = &[0x00, 0x00, 0x2a, 0x00];

    const R_COPY: u32 = R_LARCH_COPY;
    const R_GLOB_DAT: u32 = if IS_64 { R_LARCH_64 } else { R_LARCH_32 };
    const R_JUMP_SLOT: u32 = R_LARCH_JUMP_SLOT;
    const R_ABS: u32 = if IS_64 { R_LARCH_64 } else { R_LARCH_32 };
    const R_RELATIVE: u32 = R_LARCH_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_LARCH_IRELATIVE);
    const R_DTPOFF: u32 = if IS_64 {
        R_LARCH_TLS_DTPREL64
    } else {
        R_LARCH_TLS_DTPREL32
    };
    const R_TPOFF: u32 = if IS_64 {
        R_LARCH_TLS_TPREL64
    } else {
        R_LARCH_TLS_TPREL32
    };
    const R_DTPMOD: u32 = if IS_64 {
        R_LARCH_TLS_DTPMOD64
    } else {
        R_LARCH_TLS_DTPMOD32
    };
    const R_TLSDESC: Option<u32> = Some(if IS_64 {
        R_LARCH_TLS_DESC64
    } else {
        R_LARCH_TLS_DESC32
    });
    const R_FUNCALL: &'static [u32] = &[R_LARCH_B26, R_LARCH_CALL36];

    fn rel_to_string(r_type: u32) -> String {
        loongarch_rel_to_string(r_type)
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN_64: [u32; 8] = [
            0x1a00_000e, // pcalau12i $t2, %pc_hi20(.got.plt)
            0x0011_bdad, // sub.d     $t1, $t1, $t3
            0x28c0_01cf, // ld.d      $t3, $t2, %lo12(.got.plt) # _dl_runtime_resolve
            0x02ff_51ad, // addi.d    $t1, $t1, -44             # .plt entry
            0x02c0_01cc, // addi.d    $t0, $t2, %lo12(.got.plt) # &.got.plt
            0x0045_05ad, // srli.d    $t1, $t1, 1               # .plt entry offset
            0x28c0_218c, // ld.d      $t0, $t0, 8               # link map
            0x4c00_01e0, // jr        $t3
        ];
        const INSN_32: [u32; 8] = [
            0x1a00_000e, // pcalau12i $t2, %pc_hi20(.got.plt)
            0x0011_3dad, // sub.w     $t1, $t1, $t3
            0x2880_01cf, // ld.w      $t3, $t2, %lo12(.got.plt) # _dl_runtime_resolve
            0x02bf_51ad, // addi.w    $t1, $t1, -44             # .plt entry
            0x0280_01cc, // addi.w    $t0, $t2, %lo12(.got.plt) # &.got.plt
            0x0044_89ad, // srli.w    $t1, $t1, 2               # .plt entry offset
            0x2880_118c, // ld.w      $t0, $t0, 4               # link map
            0x4c00_01e0, // jr        $t3
        ];
        let gotplt = ctx.gotplt.hdr.shdr.sh_addr;
        let plt = ctx.plt.hdr.shdr.sh_addr;
        write_insns(buf, if IS_64 { &INSN_64 } else { &INSN_32 });
        write_j20(buf, hi20(gotplt, plt));
        write_k12(&mut buf[8..], gotplt);
        write_k12(&mut buf[16..], gotplt);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        Self::write_plt_stub(buf, sym.gotplt_addr(ctx), sym.plt_addr(ctx));
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        Self::write_plt_stub(buf, sym.got_pltgot_addr(ctx), sym.plt_addr(ctx));
    }

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
            R_LARCH_ADD6 => add_bits(loc, 6, val, false),
            R_LARCH_ADD8 => add_bits(loc, 8, val, false),
            R_LARCH_ADD16 => add_bits(loc, 16, val, false),
            R_LARCH_ADD32 => add_bits(loc, 32, val, false),
            R_LARCH_ADD64 => add_bits(loc, 64, val, false),
            R_LARCH_SUB6 => add_bits(loc, 6, val, true),
            R_LARCH_SUB8 => add_bits(loc, 8, val, true),
            R_LARCH_SUB16 => add_bits(loc, 16, val, true),
            R_LARCH_SUB32 => add_bits(loc, 32, val, true),
            R_LARCH_SUB64 => add_bits(loc, 64, val, true),
            R_LARCH_32_PCREL => {
                eh_frame::check_range(
                    ctx,
                    isec,
                    rel,
                    val.wrapping_sub(p) as i64,
                    -(1 << 31),
                    1 << 31,
                );
                LittleEndian::write_u32(loc, val.wrapping_sub(p) as u32);
            }
            R_LARCH_64_PCREL => LittleEndian::write_u64(loc, val.wrapping_sub(p)),
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        // Scan relocations
        for rel in isec.rels::<Self>(file) {
            if is_marker(rel.r_type) || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            match rel.r_type {
                R_LARCH_32 => {
                    if IS_64 {
                        scan_absrel(ctx, isec, sym, &rel);
                    }
                }
                R_LARCH_B26 | R_LARCH_PCALA_HI20 | R_LARCH_CALL36 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_LARCH_GOT_HI20 | R_LARCH_GOT_PC_HI20 => sym.add_flags(NEEDS_GOT),
                R_LARCH_TLS_IE_HI20 | R_LARCH_TLS_IE_PC_HI20 => sym.add_flags(NEEDS_GOTTP),
                R_LARCH_TLS_GD_PC_HI20
                | R_LARCH_TLS_LD_PC_HI20
                | R_LARCH_TLS_GD_HI20
                | R_LARCH_TLS_LD_HI20 => sym.add_flags(NEEDS_TLSGD),
                R_LARCH_32_PCREL | R_LARCH_64_PCREL => scan_pcrel(ctx, isec, sym, &rel),
                R_LARCH_TLS_LE_HI20
                | R_LARCH_TLS_LE_LO12
                | R_LARCH_TLS_LE64_LO20
                | R_LARCH_TLS_LE64_HI12
                | R_LARCH_TLS_LE_HI20_R
                | R_LARCH_TLS_LE_LO12_R => check_tlsle(ctx, isec, sym, &rel),
                R_LARCH_TLS_DESC_CALL => scan_tlsdesc(ctx, sym),
                R_LARCH_64
                | R_LARCH_B16
                | R_LARCH_B21
                | R_LARCH_ABS_HI20
                | R_LARCH_ABS_LO12
                | R_LARCH_ABS64_LO20
                | R_LARCH_ABS64_HI12
                | R_LARCH_PCALA_LO12
                | R_LARCH_PCALA64_LO20
                | R_LARCH_PCALA64_HI12
                | R_LARCH_GOT_PC_LO12
                | R_LARCH_GOT64_PC_LO20
                | R_LARCH_GOT64_PC_HI12
                | R_LARCH_GOT_LO12
                | R_LARCH_GOT64_LO20
                | R_LARCH_GOT64_HI12
                | R_LARCH_TLS_IE_PC_LO12
                | R_LARCH_TLS_IE64_PC_LO20
                | R_LARCH_TLS_IE64_PC_HI12
                | R_LARCH_TLS_IE_LO12
                | R_LARCH_TLS_IE64_LO20
                | R_LARCH_TLS_IE64_HI12
                | R_LARCH_ADD6
                | R_LARCH_SUB6
                | R_LARCH_ADD8
                | R_LARCH_SUB8
                | R_LARCH_ADD16
                | R_LARCH_SUB16
                | R_LARCH_ADD32
                | R_LARCH_SUB32
                | R_LARCH_ADD64
                | R_LARCH_SUB64
                | R_LARCH_ADD_ULEB128
                | R_LARCH_SUB_ULEB128
                | R_LARCH_TLS_DESC_PC_HI20
                | R_LARCH_TLS_DESC_PC_LO12
                | R_LARCH_TLS_DESC_LD
                | R_LARCH_TLS_LE_ADD_R => {}
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
        let rels = isec.rels::<Self>(file);
        let contents = isec.original_contents(file);
        let got = ctx.got.hdr.shdr.sh_addr;
        let mut i = 0;

        while i < rels.len() {
            let rel = &rels.at(i);
            i += 1;
            if is_marker(rel.r_type) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let (removed, delta) = isec.removed_at(rel);
            let r_offset = rel.r_offset - delta as u64;
            let s = sym.addr(ctx);
            let a = rel.r_addend as u64;
            let p = isec.addr(ctx) + r_offset;
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);

            // Unlike other psABIs, the LoongArch ABI uses the same relocation
            // types to refer to GOT entries for thread-local symbols and regular
            // ones. Therefore, G may refer to a TLSGD or a regular GOT slot
            // depending on the symbol type.
            //
            // Note that even though LoongArch defines relocations for TLSLD, TLSLD
            // is not actually supported on it. GCC and LLVM emit identical machine
            // code for -ftls-model=global-dynamic and -ftls-model=local-dynamic,
            // and we need to handle TLSLD relocations as equivalent to TLSGD
            // relocations. This is clearly a compiler bug, but it's too late to
            // fix. The only way to fix it would be to define a new set of
            // relocations for true TLSLD and deprecate the current ones. But it
            // appears that migrating to TLSDESC is a better choice, so it's
            // unlikely to happen.
            let g = || {
                let entry = if sym.has_tlsgd(&ctx.symbols) {
                    sym.tlsgd_addr(ctx)
                } else {
                    sym.got_addr(ctx)
                };
                entry.wrapping_sub(got)
            };
            let got_entry = || got.wrapping_add(g()).wrapping_add(a);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i - 1, val, lo, hi);
            let check_branch = |val: i64, lo: i64, hi: i64| {
                check(val, lo, hi);
                if val & 0b11 != 0 {
                    error!(
                        ctx,
                        "{}: misaligned symbol {sym} for relocation {}",
                        isec.display(file),
                        rel.type_name::<Self>()
                    );
                }
            };
            let loc = &mut buf[r_offset as usize..];

            match rel.r_type {
                R_LARCH_32 => {
                    debug_assert!(IS_64);
                    LittleEndian::write_u32(loc, sa as u32);
                }
                R_LARCH_B16 => {
                    check_branch(pcrel as i64, -(1 << 17), 1 << 17);
                    write_k16(loc, pcrel >> 2);
                }
                R_LARCH_B21 => {
                    check_branch(pcrel as i64, -(1 << 22), 1 << 22);
                    write_d5k16(loc, pcrel >> 2);
                }
                R_LARCH_B26 => {
                    if !sym.is_remaining_undef_weak() {
                        check_branch(pcrel as i64, -(1 << 27), 1 << 27);
                    }
                    write_d10k16(loc, pcrel >> 2);
                }
                R_LARCH_ABS_LO12 => write_k12(loc, sa),
                R_LARCH_ABS_HI20 => write_j20(loc, sa >> 12),
                R_LARCH_ABS64_LO20 => write_j20(loc, sa >> 32),
                R_LARCH_ABS64_HI12 => write_k12(loc, sa >> 52),
                R_LARCH_PCALA_LO12 => {
                    // It looks like R_LARCH_PCALA_LO12 is sometimes used for JIRL even
                    // though the instruction takes a 16 bit immediate rather than 12 bits.
                    // It is contrary to the psABI document, but GNU ld has special
                    // code to handle it, so we accept it too.
                    if insn(loc) & 0xfc00_0000 == 0x4c00_0000 {
                        write_k16(loc, (sign_extend(sa, 12) >> 2) as u64);
                    } else {
                        write_k12(loc, sa);
                    }
                }
                R_LARCH_PCALA_HI20 => {
                    if removed == 0 {
                        write_j20(loc, hi20(sa, p));
                    } else {
                        // Rewrite pcalau12i + addi.d with pcaddi. The high part vanishes and
                        // the low part becomes the pcaddi's PC-relative relocation.
                        debug_assert_eq!(removed, 4);
                        write_pcaddi(loc, pcrel >> 2);
                        i += 3;
                    }
                }
                R_LARCH_PCALA64_LO20 => write_j20(loc, higher20(sa, p)),
                R_LARCH_PCALA64_HI12 => write_k12(loc, highest12(sa, p)),
                R_LARCH_GOT_PC_LO12 => write_k12(loc, got_entry()),
                R_LARCH_GOT_PC_HI20 => {
                    if removed == 0 {
                        // If the PC-relative symbol address is known at link-time, we can
                        // rewrite the following GOT load
                        //
                        // pcalau12i $t0, 0         # R_LARCH_GOT_PC_HI20
                        // ld.d      $t0, $t0, 0    # R_LARCH_GOT_PC_LO12
                        //
                        // with the following address materialization
                        //
                        // pcalau12i $t0, 0
                        // addi.d    $t0, $t0, 0
                        if is_relaxable_got_load(ctx, isec, i - 1)
                            && is_int(compute_distance(ctx, sym, isec, rel), 32)
                        {
                            let reg = rd(insn(loc));
                            set_insn(&mut loc[4..], 0x02c0_0000 | (reg << 5) | reg); // addi.d
                            write_j20(loc, hi20(sa, p));
                            write_k12(&mut loc[4..], sa);
                            i += 3;
                        } else {
                            write_j20(loc, hi20(got_entry(), p));
                        }
                    } else {
                        // Rewrite pcalau12i + ld.d with pcaddi. The high part vanishes and the
                        // low part becomes the pcaddi's PC-relative relocation.
                        debug_assert_eq!(removed, 4);
                        write_pcaddi(loc, pcrel >> 2);
                        i += 3;
                    }
                }
                R_LARCH_GOT64_PC_LO20 => write_j20(loc, higher20(got_entry(), p)),
                R_LARCH_GOT64_PC_HI12 => write_k12(loc, highest12(got_entry(), p)),
                R_LARCH_GOT_LO12 => write_k12(loc, got_entry()),
                R_LARCH_GOT_HI20 => write_j20(loc, got_entry() >> 12),
                R_LARCH_GOT64_LO20 => write_j20(loc, got_entry() >> 32),
                R_LARCH_GOT64_HI12 => write_k12(loc, got_entry() >> 52),
                R_LARCH_TLS_LE_LO12 => write_k12(loc, sa.wrapping_sub(ctx.tp_addr)),
                R_LARCH_TLS_LE_HI20 => write_j20(loc, sa.wrapping_sub(ctx.tp_addr) >> 12),
                R_LARCH_TLS_LE64_LO20 => write_j20(loc, sa.wrapping_sub(ctx.tp_addr) >> 32),
                R_LARCH_TLS_LE64_HI12 => write_k12(loc, sa.wrapping_sub(ctx.tp_addr) >> 52),
                R_LARCH_TLS_IE_PC_LO12 => write_k12(loc, sym.gottp_addr(ctx).wrapping_add(a)),
                R_LARCH_TLS_IE_PC_HI20 => {
                    write_j20(loc, hi20(sym.gottp_addr(ctx).wrapping_add(a), p))
                }
                R_LARCH_TLS_IE64_PC_LO20 => {
                    write_j20(loc, higher20(sym.gottp_addr(ctx).wrapping_add(a), p))
                }
                R_LARCH_TLS_IE64_PC_HI12 => {
                    write_k12(loc, highest12(sym.gottp_addr(ctx).wrapping_add(a), p))
                }
                R_LARCH_TLS_IE_LO12 => write_k12(loc, sym.gottp_addr(ctx).wrapping_add(a)),
                R_LARCH_TLS_IE_HI20 => write_j20(loc, sym.gottp_addr(ctx).wrapping_add(a) >> 12),
                R_LARCH_TLS_IE64_LO20 => write_j20(loc, sym.gottp_addr(ctx).wrapping_add(a) >> 32),
                R_LARCH_TLS_IE64_HI12 => write_k12(loc, sym.gottp_addr(ctx).wrapping_add(a) >> 52),
                R_LARCH_TLS_GD_PC_HI20 | R_LARCH_TLS_LD_PC_HI20 => {
                    let val = sym.tlsgd_addr(ctx).wrapping_add(a);
                    check(val.wrapping_sub(p) as i64, -(1 << 31), 1 << 31);
                    write_j20(loc, hi20(val, p));
                }
                R_LARCH_TLS_GD_HI20 | R_LARCH_TLS_LD_HI20 => {
                    write_j20(loc, sym.tlsgd_addr(ctx).wrapping_add(a) >> 12)
                }
                R_LARCH_ADD6 => add_bits(loc, 6, sa, false),
                R_LARCH_ADD8 => add_bits(loc, 8, sa, false),
                R_LARCH_ADD16 => add_bits(loc, 16, sa, false),
                R_LARCH_ADD32 => add_bits(loc, 32, sa, false),
                R_LARCH_ADD64 => add_bits(loc, 64, sa, false),
                R_LARCH_SUB6 => add_bits(loc, 6, sa, true),
                R_LARCH_SUB8 => add_bits(loc, 8, sa, true),
                R_LARCH_SUB16 => add_bits(loc, 16, sa, true),
                R_LARCH_SUB32 => add_bits(loc, 32, sa, true),
                R_LARCH_SUB64 => add_bits(loc, 64, sa, true),
                R_LARCH_32_PCREL => {
                    check(pcrel as i64, -(1 << 31), 1 << 31);
                    LittleEndian::write_u32(loc, pcrel as u32);
                }
                R_LARCH_64_PCREL => LittleEndian::write_u64(loc, pcrel),
                R_LARCH_CALL36 => {
                    if removed == 0 {
                        if !sym.is_remaining_undef_weak() {
                            check_branch(pcrel as i64, -(1 << 37) - 0x20000, (1 << 37) - 0x20000);
                        }
                        write_j20(loc, pcrel.wrapping_add(0x20000) >> 18);
                        write_k16(&mut loc[4..], pcrel >> 2);
                    } else {
                        // Rewrite PCADDU18I + JIRL to B or BL
                        debug_assert_eq!(removed, 4);
                        let jirl = insn(&contents[rel.r_offset as usize + 4..]);
                        set_insn(
                            loc,
                            if rd(jirl) == 0 {
                                0x5000_0000
                            } else {
                                0x5400_0000
                            },
                        );
                        write_d10k16(loc, pcrel >> 2);
                    }
                }
                R_LARCH_ADD_ULEB128 => add_uleb(loc, sa, false),
                R_LARCH_SUB_ULEB128 => add_uleb(loc, sa, true),
                // LoongArch TLSDESC uses the following code sequence to materialize
                // a TP-relative address in a0.
                //
                // pcalau12i $a0, 0
                // R_LARCH_TLS_DESC_PC_HI20    foo
                // addi.[dw] $a0, $a0, 0
                // R_LARCH_TLS_DESC_PC_LO12    foo
                // ld.d      $ra, $a0, 0
                // R_LARCH_TLS_DESC_LD         foo
                // jirl      $ra, $ra, 0
                // R_LARCH_TLS_DESC_CALL       foo
                //
                // We may relax the instructions to the following if its TP-relative
                // address is known at link-time
                //
                // <deleted>
                // <deleted>
                // lu12i.w   $a0, foo@TPOFF
                // addi.w    $a0, $a0, foo@TPOFF
                //
                // or to the following if the TP offset is small enough.
                //
                // <deleted>
                // <deleted>
                // <deleted>
                // ori       $a0, $zero, foo@TPOFF
                //
                // If the TP-relative address is known at process startup time, we
                // may relax the instructions to the following.
                //
                // <deleted>
                // <deleted>
                // pcalau12i $a0, foo@GOTTP
                // ld.[dw]   $a0, $a0, foo@GOTTP
                //
                // If we don't know anything about the symbol, we can still relax
                // the first two instructions to a single pcaddi as shown below.
                //
                // <deleted>
                // pcaddi    $a0, foo@GOTDESC
                // ld.d      $ra, $a0, 0
                // jirl      $ra, $ra, 0
                R_LARCH_TLS_DESC_PC_HI20 => {
                    if sym.has_tlsdesc(&ctx.symbols) && removed == 0 {
                        write_j20(loc, hi20(sym.tlsdesc_addr(ctx).wrapping_add(a), p));
                    }
                }
                R_LARCH_TLS_DESC_PC_LO12 => {
                    if sym.has_tlsdesc(&ctx.symbols) && removed == 0 {
                        let dist = sym.tlsdesc_addr(ctx).wrapping_add(a).wrapping_sub(p) as i64;
                        if is_int(dist, 22) {
                            write_pcaddi(loc, (dist >> 2) as u64);
                        } else {
                            write_k12(loc, sym.tlsdesc_addr(ctx).wrapping_add(a));
                        }
                    }
                }
                R_LARCH_TLS_DESC_LD => {
                    // TLSDESC is relaxed to IE or LE. The ld.d slot holds the first
                    // instruction of the resulting sequence.
                    if sym.has_tlsdesc(&ctx.symbols) {
                        // Do nothing (TLSDESC kept)
                    } else if removed == 4 {
                        // Small TP offset: the instruction was deleted.
                    } else if sym.has_gottp(&ctx.symbols) {
                        set_insn(loc, 0x1a00_0004); // pcalau12i $a0, 0
                        write_j20(loc, hi20(sym.gottp_addr(ctx).wrapping_add(a), p));
                    } else {
                        set_insn(loc, 0x1400_0004); // lu12i.w $a0, 0
                        write_j20(loc, sa.wrapping_add(0x800).wrapping_sub(ctx.tp_addr) >> 12);
                    }
                }
                R_LARCH_TLS_DESC_CALL => {
                    // The jirl slot holds the second instruction of the IE
                    // or LE sequence.
                    if sym.has_tlsdesc(&ctx.symbols) {
                        // Do nothing
                    } else if sym.has_gottp(&ctx.symbols) {
                        // ld.d $a0, $a0, 0
                        // ld.w $a0, $a0, 0
                        set_insn(loc, if IS_64 { 0x28c0_0084 } else { 0x2880_0084 }); // ld.[dw] $a0, $a0, 0
                        write_k12(loc, sym.gottp_addr(ctx).wrapping_add(a));
                    } else {
                        let val = sa.wrapping_sub(ctx.tp_addr) as i64;
                        set_insn(
                            loc,
                            if (0..0x1000).contains(&val) {
                                0x0380_0004
                            } else {
                                0x0280_0084
                            },
                        ); // ori $a0, $zero, 0 / addi.w $a0, $a0, 0
                        write_k12(loc, val as u64);
                    }
                }
                // lu12i.w + add.d + addi.d => addi.d when the variable is within 2 KiB
                // of TP, in which case the lu12i.w loses its relocation.
                R_LARCH_TLS_LE_HI20_R => {
                    if removed == 0 {
                        write_j20(loc, sa.wrapping_add(0x800).wrapping_sub(ctx.tp_addr) >> 12);
                    }
                }
                R_LARCH_TLS_LE_LO12_R => {
                    let val = sa.wrapping_sub(ctx.tp_addr) as i64;
                    write_k12(loc, val as u64);
                    // Rewrite `addi.d $t0, $t0, <offset>` with `addi.d $t0, $tp, <offset>`
                    // if the offset is directly accessible using tp. tp is r2.
                    if is_int(val, 12) {
                        set_rj(loc, 2); // $tp
                    }
                }
                R_LARCH_TLS_LE_ADD_R | R_LARCH_64 => {
                    // add.d that materializes TP + offset; removed together with the
                    // lu12i.w when the variable is within 2 KiB of TP.
                }
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        isec.for_each_reloc::<Self>(ctx, |rel, _| {
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
            let tombstone = || isec.tombstone(ctx, sym, frag.map(|(f, _)| f));
            let loc = &mut buf[rel.r_offset as usize..];

            match rel.r_type {
                R_LARCH_32 => LittleEndian::write_u32(loc, sa as u32),
                R_LARCH_64 => LittleEndian::write_u64(loc, tombstone().unwrap_or(sa)),
                R_LARCH_ADD6 => add_bits(loc, 6, sa, false),
                R_LARCH_ADD8 => add_bits(loc, 8, sa, false),
                R_LARCH_ADD16 => add_bits(loc, 16, sa, false),
                R_LARCH_ADD32 => add_bits(loc, 32, sa, false),
                R_LARCH_ADD64 => add_bits(loc, 64, sa, false),
                R_LARCH_SUB6 => add_bits(loc, 6, sa, true),
                R_LARCH_SUB8 => add_bits(loc, 8, sa, true),
                R_LARCH_SUB16 => add_bits(loc, 16, sa, true),
                R_LARCH_SUB32 => add_bits(loc, 32, sa, true),
                R_LARCH_SUB64 => add_bits(loc, 64, sa, true),
                R_LARCH_TLS_DTPREL32 => LittleEndian::write_u32(
                    loc,
                    tombstone().unwrap_or(sa.wrapping_sub(ctx.dtp_addr)) as u32,
                ),
                R_LARCH_TLS_DTPREL64 => LittleEndian::write_u64(
                    loc,
                    tombstone().unwrap_or(sa.wrapping_sub(ctx.dtp_addr)),
                ),
                R_LARCH_ADD_ULEB128 => add_uleb(loc, sa, false),
                R_LARCH_SUB_ULEB128 => add_uleb(loc, sa, true),
                _ => fatal!(
                    ctx,
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        });
    }

    fn emitted_rel_type(ctx: &Context<Self>, isec: &InputSection, rel: &ElfRel, i: usize) -> u32 {
        if !isec.is_alloc() {
            return rel.r_type;
        }
        let rels = isec.rels::<Self>(&ctx.objs[isec.file.index()]);
        let file = &ctx.objs[isec.file.index()];
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
        let (removed, delta) = isec.removed_at(rel);

        // The low half of a pair whose high half folded into a pcaddi.
        let folded_into_pcaddi = || {
            i >= 2
                && matches!(
                    rels.at(i - 2).r_type,
                    R_LARCH_PCALA_HI20 | R_LARCH_GOT_PC_HI20
                )
                && rels.at(i - 2).r_offset + 4 == rel.r_offset
                && isec.removed_at(&rels.at(i - 2)).0 != 0
        };

        match rel.r_type {
            R_LARCH_PCALA_HI20 | R_LARCH_GOT_PC_HI20 if removed != 0 => R_NONE,
            R_LARCH_PCALA_LO12 | R_LARCH_GOT_PC_LO12 if folded_into_pcaddi() => R_LARCH_PCREL20_S2,
            R_LARCH_CALL36 if removed != 0 => R_LARCH_B26,
            // pcalau12i + addi.d => pcaddi when TLSDESC is kept; both deleted when it
            // is relaxed to IE/LE. Either way the high part loses its relocation.
            // The folded pcaddi's relocation is emitted from the LO12 slot below.
            R_LARCH_TLS_DESC_PC_HI20 if !sym.has_tlsdesc(&ctx.symbols) || removed != 0 => R_NONE,
            R_LARCH_TLS_DESC_PC_LO12 => {
                if !sym.has_tlsdesc(&ctx.symbols) {
                    R_NONE
                } else {
                    let p = isec.addr(ctx) + rel.r_offset - delta as u64;
                    let dist = sym
                        .tlsdesc_addr(ctx)
                        .wrapping_add(rel.r_addend as u64)
                        .wrapping_sub(p) as i64;
                    if removed == 0 && is_int(dist, 22) {
                        R_LARCH_TLS_DESC_PCREL20_S2
                    } else {
                        rel.r_type
                    }
                }
            }
            R_LARCH_TLS_DESC_LD if !sym.has_tlsdesc(&ctx.symbols) => {
                if removed == 4 {
                    R_NONE
                } else if sym.has_gottp(&ctx.symbols) {
                    R_LARCH_TLS_IE_PC_HI20
                } else {
                    R_LARCH_TLS_LE_HI20
                }
            }
            R_LARCH_TLS_DESC_CALL if !sym.has_tlsdesc(&ctx.symbols) => {
                if sym.has_gottp(&ctx.symbols) {
                    R_LARCH_TLS_IE_PC_LO12
                } else {
                    R_LARCH_TLS_LE_LO12
                }
            }
            R_LARCH_TLS_LE_HI20_R | R_LARCH_TLS_LE_ADD_R if removed != 0 => R_NONE,
            _ => rel.r_type,
        }
    }

    fn shrink_section(ctx: &Context<Self>, isec: &InputSection) -> Vec<RelocDelta> {
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels::<Self>(file);
        let contents = isec.original_contents(file);
        let mut deltas: Vec<RelocDelta> = Vec::new();
        let mut delta: i64 = 0;

        // Records that `d` bytes go away at relocation `r`.
        fn record(deltas: &mut Vec<RelocDelta>, delta: &mut i64, r: &ElfRel, d: i64) {
            *delta += d;
            deltas.push(RelocDelta {
                offset: r.r_offset,
                delta: *delta,
            });
        }

        for i in 0..rels.len() {
            let r = &rels.at(i);
            let sym = &ctx.symbols[file.base.symbols[r.r_sym as usize]];

            // A R_LARCH_ALIGN relocation refers to the beginning of a nop
            // sequence. We need to remove some or all of them so that the
            // instruction that immediately follows that is aligned to a specified
            // boundary. To allow that, a R_LARCH_ALIGN relocation that requests
            // 2^n alignment refers to 2^n - 4 bytes of nop instructions.
            if r.r_type == R_LARCH_ALIGN {
                // The actual rule for storing the alignment size is a bit weird.
                // In particular, the most significant 56 bits of r_addend is
                // sometimes used to store the upper limit of the alignment,
                // allowing the instruction that follows nops _not_ to be aligned at
                // all. I think that's a spec bug, so we don't want to support that.
                let alignment = if r.r_sym != 0 {
                    if r.r_addend >> 8 != 0 {
                        fatal!(
                            ctx,
                            "{}: ternary R_LARCH_ALIGN is not supported: {i}",
                            isec.display(file)
                        );
                    }
                    1u64 << r.r_addend
                } else {
                    let alignment = r.r_addend as u64 + 4;
                    if !alignment.is_power_of_two() {
                        fatal!(
                            ctx,
                            "{}: R_LARCH_ALIGN: invalid alignment requirement: {i}",
                            isec.display(file)
                        );
                    }
                    alignment
                };
                let p = isec.addr(ctx) + r.r_offset - delta as u64;
                let desired = align_to(p, alignment);
                let actual = p + alignment - 4;
                if desired != actual {
                    record(&mut deltas, &mut delta, r, (actual - desired) as i64);
                }
                continue;
            }

            // Handling other relocations is optional.
            if !ctx.args.relax || i + 1 == rels.len() || rels.at(i + 1).r_type != R_LARCH_RELAX {
                continue;
            }

            // Skip linker-synthesized symbols because their final addresses
            // are not fixed yet.
            if sym.file() == ctx.internal_obj.map(FileId::Obj) {
                continue;
            }

            let mut remove = |d: i64| record(&mut deltas, &mut delta, r, d);

            match r.r_type {
                // LoongArch uses the following three instructions to access
                // TP ± 2 GiB.
                //
                // lu12i.w $t0, 0           # R_LARCH_TLS_LE_HI20_R
                // add.d   $t0, $t0, $tp    # R_LARCH_TLS_LE_ADD_R
                // addi.d  $t0, $t0, 0      # R_LARCH_TLS_LE_LO12_R
                //
                // If the thread-local variable is within TP ± 2 KiB, we can
                // relax them into the following single instruction.
                //
                // addi.d  $t0, $tp, <tp-offset>
                R_LARCH_TLS_LE_HI20_R | R_LARCH_TLS_LE_ADD_R => {
                    let val = sym
                        .addr(ctx)
                        .wrapping_add(r.r_addend as u64)
                        .wrapping_sub(ctx.tp_addr) as i64;
                    if is_int(val, 12) {
                        remove(4);
                    }
                }
                // The following two instructions are used to materialize a
                // PC-relative address with a 32 bit displacement.
                //
                // pcalau12i $t0, 0         # R_LARCH_PCALA_HI20
                // addi.d    $t0, $t0, 0    # R_LARCH_PCALA_LO12
                //
                // If the displacement is within ±2 MiB, we can relax them to
                // the following instruction.
                //
                // pcaddi    $t0, <offset>
                R_LARCH_PCALA_HI20 => {
                    if i + 3 < rels.len()
                        && rels.at(i + 2).r_type == R_LARCH_PCALA_LO12
                        && rels.at(i + 2).r_offset == r.r_offset + 4
                        && rels.at(i + 3).r_type == R_LARCH_RELAX
                    {
                        let dist = compute_distance(ctx, sym, isec, r);
                        let insn1 = insn(&contents[r.r_offset as usize..]);
                        let insn2 = insn(&contents[r.r_offset as usize + 4..]);
                        let is_addi_d = insn2 & 0xffc0_0000 == 0x02c0_0000;
                        if dist & 0b11 == 0
                            && is_int(dist, 22)
                            && is_addi_d
                            && rd(insn1) == rd(insn2)
                            && rd(insn2) == rj(insn2)
                        {
                            remove(4);
                        }
                    }
                }
                // A CALL36 relocation referes to the following instruction pair
                // to jump to PC ± 128 GiB.
                //
                // pcaddu18i $t0,       0         # R_LARCH_CALL36
                // jirl      $zero/$ra, $t0, 0
                //
                // If the displacement is PC ± 128 MiB, we can use B or BL instead.
                // Note that $zero is $r0 and $ra is $r1.
                R_LARCH_CALL36 => {
                    let dist = compute_distance(ctx, sym, isec, r);
                    let jirl = insn(&contents[r.r_offset as usize + 4..]);
                    if is_int(dist, 28) && (rd(jirl) == 0 || rd(jirl) == 1) {
                        remove(4);
                    }
                }
                // The following two instructions are used to load a symbol address
                // from the GOT.
                //
                // pcalau12i $t0, 0         # R_LARCH_GOT_PC_HI20
                // ld.d      $t0, $t0, 0    # R_LARCH_GOT_PC_LO12
                //
                // If the PC-relative symbol address is known at link-time, we can
                // relax them to the following instruction.
                //
                // pcaddi    $t0, <offset>
                R_LARCH_GOT_PC_HI20 => {
                    if is_relaxable_got_load(ctx, isec, i) {
                        let dist = compute_distance(ctx, sym, isec, r);
                        if is_int(dist, 22) && dist & 3 == 0 {
                            remove(4);
                        }
                    }
                }
                R_LARCH_TLS_DESC_PC_HI20 => {
                    if sym.has_tlsdesc(&ctx.symbols) {
                        let p = isec.addr(ctx) + r.r_offset;
                        let dist = sym
                            .tlsdesc_addr(ctx)
                            .wrapping_add(r.r_addend as u64)
                            .wrapping_sub(p) as i64;
                        if is_int(dist, 22) {
                            remove(4);
                        }
                    } else {
                        remove(4);
                    }
                }
                R_LARCH_TLS_DESC_PC_LO12 => {
                    if !sym.has_tlsdesc(&ctx.symbols) {
                        remove(4);
                    }
                }
                R_LARCH_TLS_DESC_LD
                    if !sym.has_tlsdesc(&ctx.symbols) && !sym.has_gottp(&ctx.symbols) =>
                {
                    let val = sym
                        .addr(ctx)
                        .wrapping_add(r.r_addend as u64)
                        .wrapping_sub(ctx.tp_addr) as i64;
                    if (0..0x1000).contains(&val) {
                        remove(4);
                    }
                }
                _ => {}
            }
        }
        deltas
    }
}
