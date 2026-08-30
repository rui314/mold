//! RISC-V, 64- and 32-bit, in either byte order.
//!
//! RISC-V is a clean RISC ISA with PC-relative loads and stores for
//! position-independent code, and RV32 is essentially RV64 without the
//! 64-bit operations. Big-endian RISC-V exists as an extension; even then
//! instructions are little-endian, only data is byte-swapped.
//!
//! What makes RISC-V unusual from the linker's point of view is that
//! sections can shrink while being copied: branches are emitted as
//! instruction pairs reaching ±2 GiB, and the linker replaces them with a
//! single instruction when the target is close enough. See `relax.rs`.
//!
//! https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc

// Binary literals are grouped by instruction field.
#![allow(clippy::unusual_byte_groupings)]

use std::marker::PhantomData;

use crate::arch::{Arch, Family};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::input_sections::{
    check_tlsle, r_delta, scan_absrel, scan_pcrel, scan_tlsdesc, InputSection, RelocDelta,
};
use crate::relax::compute_distance;
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::util::{align_to, bit, bits, encode_uleb, is_int, overwrite_uleb, read_uleb};
use crate::{error, fatal};

/// RISC-V of a given word size and byte order.
#[derive(Clone, Copy, Debug, Default)]
pub struct RiscvTarget<End, const IS_64: bool>(PhantomData<End>);

pub type Riscv64 = RiscvTarget<LittleEndian, true>;
pub type Riscv64Be = RiscvTarget<BigEndian, true>;
pub type Riscv32 = RiscvTarget<LittleEndian, false>;
pub type Riscv32Be = RiscvTarget<BigEndian, false>;

impl<End: Endian, const IS_64: bool> Layout for RiscvTarget<End, IS_64> {
    type Endian = End;
    const IS_64: bool = IS_64;
    const IS_RELA: bool = true;
}

// Instructions are always little-endian.

fn insn32(loc: &[u8]) -> u32 {
    u32::from_le_bytes([loc[0], loc[1], loc[2], loc[3]])
}

fn insn16(loc: &[u8]) -> u16 {
    u16::from_le_bytes([loc[0], loc[1]])
}

fn write32(loc: &mut [u8], v: u32) {
    loc[..4].copy_from_slice(&v.to_le_bytes());
}

fn write16(loc: &mut [u8], v: u16) {
    loc[..2].copy_from_slice(&v.to_le_bytes());
}

fn b(val: u64, hi: u32, lo: u32) -> u32 {
    bits(val, hi, lo) as u32
}

fn write_itype(loc: &mut [u8], val: u64) {
    write32(
        loc,
        (insn32(loc) & 0b000000_00000_11111_111_11111_1111111) | (b(val, 11, 0) << 20),
    );
}

fn write_stype(loc: &mut [u8], val: u64) {
    write32(
        loc,
        (insn32(loc) & 0b000000_11111_11111_111_00000_1111111)
            | (b(val, 11, 5) << 25)
            | (b(val, 4, 0) << 7),
    );
}

fn write_btype(loc: &mut [u8], val: u64) {
    let imm = (bit(val, 12) as u32) << 31
        | b(val, 10, 5) << 25
        | b(val, 4, 1) << 8
        | (bit(val, 11) as u32) << 7;
    write32(
        loc,
        (insn32(loc) & 0b000000_11111_11111_111_00000_1111111) | imm,
    );
}

/// U-type instructions set the upper 20 bits of a register, and a
/// following I-type instruction adds a sign-extended 12-bit immediate;
/// 0x800 compensates for the sign extension.
fn write_utype(loc: &mut [u8], val: u64) {
    write32(
        loc,
        (insn32(loc) & 0b000000_00000_00000_000_11111_1111111)
            | ((val as u32).wrapping_add(0x800) & 0xffff_f000),
    );
}

fn write_jtype(loc: &mut [u8], val: u64) {
    let imm = (bit(val, 20) as u32) << 31
        | b(val, 10, 1) << 21
        | (bit(val, 11) as u32) << 20
        | b(val, 19, 12) << 12;
    write32(
        loc,
        (insn32(loc) & 0b000000_00000_00000_000_11111_1111111) | imm,
    );
}

fn write_citype(loc: &mut [u8], val: u64) {
    let imm = (bit(val, 5) as u16) << 12 | (b(val, 4, 0) as u16) << 2;
    write16(loc, (insn16(loc) & 0b111_0_11111_00000_11) | imm);
}

fn write_cbtype(loc: &mut [u8], val: u64) {
    let bt = |n: u32| bit(val, n) as u16;
    let imm = bt(8) << 12
        | bt(4) << 11
        | bt(3) << 10
        | bt(7) << 6
        | bt(6) << 5
        | bt(2) << 4
        | bt(1) << 3
        | bt(5) << 2;
    write16(loc, (insn16(loc) & 0b111_000_111_00000_11) | imm);
}

fn write_cjtype(loc: &mut [u8], val: u64) {
    let bt = |n: u32| bit(val, n) as u16;
    let imm = bt(11) << 12
        | bt(4) << 11
        | bt(9) << 10
        | bt(8) << 9
        | bt(10) << 8
        | bt(6) << 7
        | bt(7) << 6
        | bt(3) << 5
        | bt(2) << 4
        | bt(1) << 3
        | bt(5) << 2;
    write16(loc, (insn16(loc) & 0b111_00000000000_11) | imm);
}

fn set_rs1(loc: &mut [u8], rs1: u32) {
    debug_assert!(rs1 < 32);
    write32(
        loc,
        (insn32(loc) & 0b111111_11111_00000_111_11111_1111111) | (rs1 << 15),
    );
}

fn rd(loc: &[u8]) -> u32 {
    b(insn32(loc) as u64, 11, 7)
}

const NOP: u32 = 0x13;

fn is_hi20(r_type: u32) -> bool {
    matches!(
        r_type,
        R_RISCV_GOT_HI20
            | R_RISCV_TLS_GOT_HI20
            | R_RISCV_TLS_GD_HI20
            | R_RISCV_PCREL_HI20
            | R_RISCV_TLSDESC_HI20
    )
}

/// Finds the HI20 relocation a LO12 relocation is paired with.
///
/// AUIPC materializes the upper 52 bits of a PC-relative address and a
/// following instruction the low 12 bits, but the pair need not be
/// adjacent. So the compiler creates a local symbol at the AUIPC and the
/// LO12 relocation refers to that symbol. The pair usually is adjacent,
/// which a linear search from `i` exploits.
fn find_paired_reloc<E: Arch>(
    ctx: &Context<E>,
    isec: &InputSection,
    sym: &Symbol,
    i: usize,
) -> usize {
    let rels = isec.rels::<E>(&ctx.objs[isec.file.index()]);
    let value = sym.esym(ctx).st_value;
    let candidates: Box<dyn Iterator<Item = usize>> = if value <= rels.at(i).r_offset {
        Box::new((0..i).rev())
    } else {
        Box::new(i + 1..rels.len())
    };
    for j in candidates {
        if is_hi20(rels.at(j).r_type) && value == rels.at(j).r_offset {
            return j;
        }
    }
    let file = &ctx.objs[isec.file.index()];
    fatal!(
        ctx,
        "{}: paired relocation is missing: {i}",
        isec.display(file)
    );
}

/// Whether the relocation at `i` heads the GOT-loading instruction pair
/// `la rd, foo` expands to:
///
/// ```text
/// .L0:
///   auipc rd, 0      # R_RISCV_GOT_HI20(foo),     R_RISCV_RELAX
///   ld    rd, 0(rd)  # R_RISCV_PCREL_LO12_I(.L0), R_RISCV_RELAX
/// ```
fn is_got_load_pair<E: Arch>(ctx: &Context<E>, isec: &InputSection, i: usize) -> bool {
    let rels = isec.rels::<E>(&ctx.objs[isec.file.index()]);
    let file = &ctx.objs[isec.file.index()];
    let contents = isec.original_contents(file);
    i + 3 < rels.len()
        && rels.at(i).r_type == R_RISCV_GOT_HI20
        && rels.at(i + 1).r_type == R_RISCV_RELAX
        && rels.at(i + 2).r_type == R_RISCV_PCREL_LO12_I
        && rels.at(i + 3).r_type == R_RISCV_RELAX
        && rels.at(i).r_offset == rels.at(i + 2).r_offset - 4
        && rels.at(i).r_offset
            == ctx.symbols[file.base.symbols[rels.at(i + 2).r_sym as usize]].value
        && rd(&contents[rels.at(i).r_offset as usize..])
            == rd(&contents[rels.at(i + 2).r_offset as usize..])
}

impl<End: Endian, const IS_64: bool> Arch for RiscvTarget<End, IS_64> {
    const NAME: &'static str = match (IS_64, End::IS_LITTLE) {
        (true, true) => "riscv64",
        (true, false) => "riscv64be",
        (false, true) => "riscv32",
        (false, false) => "riscv32be",
    };
    const FAMILY: Family = Family::RiscV;
    const PAGE_SIZE: u64 = 4096;
    const E_MACHINE: u32 = EM_RISCV;
    const PLT_HDR_SIZE: u64 = 32;
    const PLT_SIZE: u64 = 16;
    const PLTGOT_SIZE: u64 = 16;
    const TRAP: &'static [u8] = &[0x02, 0x90]; // c.ebreak

    const R_COPY: u32 = R_RISCV_COPY;
    const R_GLOB_DAT: u32 = if IS_64 { R_RISCV_64 } else { R_RISCV_32 };
    const R_JUMP_SLOT: u32 = R_RISCV_JUMP_SLOT;
    const R_ABS: u32 = if IS_64 { R_RISCV_64 } else { R_RISCV_32 };
    const R_RELATIVE: u32 = R_RISCV_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_RISCV_IRELATIVE);
    const R_DTPOFF: u32 = if IS_64 {
        R_RISCV_TLS_DTPREL64
    } else {
        R_RISCV_TLS_DTPREL32
    };
    const R_TPOFF: u32 = if IS_64 {
        R_RISCV_TLS_TPREL64
    } else {
        R_RISCV_TLS_TPREL32
    };
    const R_DTPMOD: u32 = if IS_64 {
        R_RISCV_TLS_DTPMOD64
    } else {
        R_RISCV_TLS_DTPMOD32
    };
    const R_TLSDESC: Option<u32> = Some(R_RISCV_TLSDESC);
    const R_FUNCALL: &'static [u32] = &[R_RISCV_CALL, R_RISCV_CALL_PLT];

    fn rel_to_string(r_type: u32) -> String {
        riscv_rel_to_string(r_type)
    }

    fn eflags(ctx: &Context<Self>) -> u32 {
        let internal = ctx.internal_obj;
        let mut objs = ctx.objs.iter().filter(|file| Some(file.id()) != internal);
        let Some(first) = objs.next() else { return 0 };
        let mut ret = first.base.e_flags;
        for file in objs {
            let flags = file.base.e_flags;
            if flags & EF_RISCV_RVC != 0 {
                ret |= EF_RISCV_RVC;
            }
            if flags & EF_RISCV_FLOAT_ABI != ret & EF_RISCV_FLOAT_ABI {
                error!(ctx, "{file}: cannot link object files with different floating-point ABI from {first}");
            }
            if flags & EF_RISCV_RVE != ret & EF_RISCV_RVE {
                error!(
                    ctx,
                    "{file}: cannot link object files with different EF_RISCV_RVE from {first}"
                );
            }
        }
        ret
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN_64: [u32; 8] = [
            0x0000_0397, // auipc  t2, %pcrel_hi(.got.plt)
            0x41c3_0333, // sub    t1, t1, t3               # .plt entry + hdr + 12
            0x0003_be03, // ld     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
            0xfd43_0313, // addi   t1, t1, -44              # .plt entry
            0x0003_8293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
            0x0013_5313, // srli   t1, t1, 1                # .plt entry offset
            0x0082_b283, // ld     t0, 8(t0)                # link map
            0x000e_0067, // jr     t3
        ];
        const INSN_32: [u32; 8] = [
            0x0000_0397, // auipc  t2, %pcrel_hi(.got.plt)
            0x41c3_0333, // sub    t1, t1, t3               # .plt entry + hdr + 12
            0x0003_ae03, // lw     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
            0xfd43_0313, // addi   t1, t1, -44              # .plt entry
            0x0003_8293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
            0x0023_5313, // srli   t1, t1, 2                # .plt entry offset
            0x0042_a283, // lw     t0, 4(t0)                # link map
            0x000e_0067, // jr     t3
        ];
        let insn = if IS_64 { &INSN_64 } else { &INSN_32 };
        for (i, &v) in insn.iter().enumerate() {
            write32(&mut buf[i * 4..], v);
        }
        let disp = ctx
            .gotplt
            .hdr
            .shdr
            .sh_addr
            .wrapping_sub(ctx.plt.hdr.shdr.sh_addr);
        write_utype(buf, disp);
        write_itype(&mut buf[8..], disp);
        write_itype(&mut buf[16..], disp);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_plt_stub::<IS_64>(buf, sym.gotplt_addr(ctx).wrapping_sub(sym.plt_addr(ctx)));
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_plt_stub::<IS_64>(
            buf,
            sym.got_pltgot_addr(ctx).wrapping_sub(sym.plt_addr(ctx)),
        );
    }

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        isec: &InputSection,
        rel: &ElfRel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        let check = |val: i64, lo: i64, hi: i64| eh_frame::check_range(ctx, isec, rel, val, lo, hi);
        match rel.r_type {
            R_NONE => {}
            R_RISCV_ADD32 => End::write_u32(loc, End::read_u32(loc).wrapping_add(val as u32)),
            R_RISCV_SUB8 => loc[0] = loc[0].wrapping_sub(val as u8),
            R_RISCV_SUB16 => End::write_u16(loc, End::read_u16(loc).wrapping_sub(val as u16)),
            R_RISCV_SUB32 => End::write_u32(loc, End::read_u32(loc).wrapping_sub(val as u32)),
            R_RISCV_SUB6 => {
                loc[0] = (loc[0] & 0b1100_0000) | (loc[0].wrapping_sub(val as u8) & 0b0011_1111)
            }
            R_RISCV_SET6 => loc[0] = (loc[0] & 0b1100_0000) | (val as u8 & 0b0011_1111),
            R_RISCV_SET8 => loc[0] = val as u8,
            R_RISCV_SET16 => End::write_u16(loc, val as u16),
            R_RISCV_SET32 => End::write_u32(loc, val as u32),
            R_RISCV_32_PCREL => {
                check(val.wrapping_sub(p) as i64, -(1 << 31), 1 << 31);
                End::write_u32(loc, val.wrapping_sub(p) as u32);
            }
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        isec.for_each_reloc::<Self>(ctx, |rel, _| {
            if rel.r_type == R_NONE || isec.record_undef_error(ctx, &rel) {
                return;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            match rel.r_type {
                R_RISCV_32 => {
                    if IS_64 {
                        scan_absrel(ctx, isec, sym, &rel);
                    }
                }
                R_RISCV_HI20 => scan_absrel(ctx, isec, sym, &rel),
                R_RISCV_CALL | R_RISCV_CALL_PLT | R_RISCV_PLT32 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_RISCV_GOT_HI20 | R_RISCV_GOT32_PCREL => sym.add_flags(NEEDS_GOT),
                R_RISCV_TLS_GOT_HI20 => sym.add_flags(NEEDS_GOTTP),
                R_RISCV_TLS_GD_HI20 => sym.add_flags(NEEDS_TLSGD),
                R_RISCV_TLSDESC_HI20 => scan_tlsdesc(ctx, sym),
                R_RISCV_32_PCREL | R_RISCV_PCREL_HI20 => scan_pcrel(ctx, isec, sym, &rel),
                R_RISCV_TPREL_HI20 => check_tlsle(ctx, isec, sym, &rel),
                R_RISCV_64
                | R_RISCV_BRANCH
                | R_RISCV_JAL
                | R_RISCV_PCREL_LO12_I
                | R_RISCV_PCREL_LO12_S
                | R_RISCV_LO12_I
                | R_RISCV_LO12_S
                | R_RISCV_TPREL_LO12_I
                | R_RISCV_TPREL_LO12_S
                | R_RISCV_TPREL_ADD
                | R_RISCV_TLSDESC_LOAD_LO12
                | R_RISCV_TLSDESC_ADD_LO12
                | R_RISCV_TLSDESC_CALL
                | R_RISCV_ADD8
                | R_RISCV_ADD16
                | R_RISCV_ADD32
                | R_RISCV_ADD64
                | R_RISCV_SUB8
                | R_RISCV_SUB16
                | R_RISCV_SUB32
                | R_RISCV_SUB64
                | R_RISCV_ALIGN
                | R_RISCV_RVC_BRANCH
                | R_RISCV_RVC_JUMP
                | R_RISCV_RELAX
                | R_RISCV_SUB6
                | R_RISCV_SET6
                | R_RISCV_SET8
                | R_RISCV_SET16
                | R_RISCV_SET32
                | R_RISCV_SET_ULEB128
                | R_RISCV_SUB_ULEB128 => {}
                _ => error!(
                    ctx,
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        });
    }

    fn apply_reloc_alloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels::<Self>(file);
        let contents = isec.original_contents(file);
        let mut i = 0;

        while i < rels.len() {
            let rel = &rels.at(i);
            i += 1;
            if rel.r_type == R_NONE || rel.r_type == R_RISCV_RELAX {
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
            let got = ctx.got.hdr.shdr.sh_addr;
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i - 1, val, lo, hi);
            let utype = |loc: &mut [u8], val: u64| {
                check(val as i64, -(1i64 << 31) - 0x800, (1i64 << 31) - 0x800);
                write_utype(loc, val);
            };
            let loc = &mut buf[r_offset as usize..];
            let orig = &contents[rel.r_offset as usize..];

            match rel.r_type {
                R_RISCV_32 => {
                    if IS_64 {
                        End::write_u32(loc, sa as u32);
                    }
                }
                // Handled as absolute relocations by the output section.
                R_RISCV_64 => {}
                R_RISCV_BRANCH => {
                    check(pcrel as i64, -(1 << 12), 1 << 12);
                    write_btype(loc, pcrel);
                }
                R_RISCV_JAL => {
                    check(pcrel as i64, -(1 << 20), 1 << 20);
                    write_jtype(loc, pcrel);
                }
                R_RISCV_CALL | R_RISCV_CALL_PLT => {
                    let rd = rd(&orig[4..]);
                    if removed == 4 {
                        // auipc + jalr -> jal
                        write32(loc, (rd << 7) | 0b1101111);
                        write_jtype(loc, pcrel);
                    } else if removed == 6 && rd == 0 {
                        // auipc + jalr -> c.j
                        write16(loc, 0b101_00000000000_01);
                        write_cjtype(loc, pcrel);
                    } else if removed == 6 && rd == 1 {
                        // auipc + jalr -> c.jal (RV32 only)
                        debug_assert!(!IS_64);
                        write16(loc, 0b001_00000000000_01);
                        write_cjtype(loc, pcrel);
                    } else {
                        debug_assert_eq!(removed, 0);
                        if !sym.is_remaining_undef_weak() {
                            check(pcrel as i64, -(1i64 << 31) - 0x800, (1i64 << 31) - 0x800);
                        }
                        write_utype(loc, pcrel);
                        write_itype(&mut loc[4..], pcrel);
                    }
                }
                R_RISCV_GOT_HI20 => {
                    // This relocation usually heads an AUIPC+LD pair loading
                    // a symbol value from the GOT. If the value is a
                    // link-time constant, it can be materialized directly.
                    let rd = rd(orig);
                    if removed == 6 {
                        // c.li rd, val
                        write16(loc, 0b010_0_00000_00000_01 | (rd as u16) << 7);
                        write_citype(loc, s);
                        i += 3;
                    } else if removed == 4 {
                        // addi rd, zero, val
                        write32(loc, 0b0010011 | (rd << 7));
                        write_itype(loc, s);
                        i += 3;
                    } else {
                        debug_assert_eq!(removed, 0);
                        if ctx.args.relax
                            && sym.is_pcrel_linktime_const(ctx)
                            && is_got_load_pair(ctx, isec, i - 1)
                            && is_int(pcrel as i64, 32)
                        {
                            // auipc rd, %hi20(val); addi rd, rd, %lo12(val)
                            utype(loc, pcrel);
                            write32(&mut loc[4..], 0b0010011 | (rd << 15) | (rd << 7));
                            write_itype(&mut loc[4..], pcrel);
                            i += 3;
                        } else {
                            utype(loc, g().wrapping_add(got).wrapping_add(a).wrapping_sub(p));
                        }
                    }
                }
                R_RISCV_TLS_GOT_HI20 => {
                    utype(loc, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(p))
                }
                R_RISCV_TLS_GD_HI20 => {
                    utype(loc, sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(p))
                }
                R_RISCV_PCREL_HI20 => utype(loc, pcrel),
                R_RISCV_PCREL_LO12_I | R_RISCV_PCREL_LO12_S => {
                    let j = find_paired_reloc(ctx, isec, sym, i - 1);
                    let rel2 = &rels.at(j);
                    let sym2 = &ctx.symbols[file.base.symbols[rel2.r_sym as usize]];
                    let write = if rel.r_type == R_RISCV_PCREL_LO12_I {
                        write_itype
                    } else {
                        write_stype
                    };
                    let a2 = rel2.r_addend as u64;
                    let p2 = isec.addr(ctx) + rel2.r_offset - r_delta(isec, rel2.r_offset) as u64;
                    match rel2.r_type {
                        R_RISCV_GOT_HI20 => {
                            write(loc, sym2.got_addr(ctx).wrapping_add(a2).wrapping_sub(p2))
                        }
                        R_RISCV_TLS_GOT_HI20 => {
                            write(loc, sym2.gottp_addr(ctx).wrapping_add(a2).wrapping_sub(p2))
                        }
                        R_RISCV_TLS_GD_HI20 => {
                            write(loc, sym2.tlsgd_addr(ctx).wrapping_add(a2).wrapping_sub(p2))
                        }
                        R_RISCV_PCREL_HI20 => {
                            write(loc, sym2.addr(ctx).wrapping_add(a2).wrapping_sub(p2))
                        }
                        _ => {}
                    }
                }
                R_RISCV_HI20 => {
                    // `lui` materializes the upper bits of a link-time
                    // constant; it may have been compressed to `c.lui` or
                    // removed by relaxation.
                    if removed == 2 {
                        let rd = rd(orig);
                        write16(loc, 0b011_0_00000_00000_01 | (rd as u16) << 7);
                        write_citype(loc, sa.wrapping_add(0x800) >> 12);
                    } else if removed == 0 {
                        utype(loc, sa);
                    }
                }
                R_RISCV_LO12_I | R_RISCV_LO12_S => {
                    if rel.r_type == R_RISCV_LO12_I {
                        write_itype(loc, sa);
                    } else {
                        write_stype(loc, sa);
                    }
                    // If the address fits in 12 bits, the `lui` may have
                    // been removed, so address relative to the zero
                    // register.
                    if is_int(sa as i64, 12) {
                        set_rs1(loc, 0);
                    }
                }
                R_RISCV_TPREL_HI20 => {
                    debug_assert!(removed == 0 || removed == 4);
                    if removed == 0 {
                        utype(loc, sa.wrapping_sub(ctx.tp_addr));
                    }
                }
                // This only annotates an ADD that relaxation may remove.
                R_RISCV_TPREL_ADD => {}
                R_RISCV_TPREL_LO12_I | R_RISCV_TPREL_LO12_S => {
                    let val = sa.wrapping_sub(ctx.tp_addr);
                    if rel.r_type == R_RISCV_TPREL_LO12_I {
                        write_itype(loc, val);
                    } else {
                        write_stype(loc, val);
                    }
                    // If the offset fits in 12 bits, address relative to
                    // tp (x4) directly.
                    if is_int(val as i64, 12) {
                        set_rs1(loc, 4);
                    }
                }
                // TLSDESC materializes a TP-relative address in a0:
                //
                //   .L0:
                //   auipc  tX, 0          # R_RISCV_TLSDESC_HI20        foo
                //   l[d|w] tY, tX, 0      # R_RISCV_TLSDESC_LOAD_LO12_I .L0
                //   addi   a0, tX, 0      # R_RISCV_TLSDESC_ADD_LO12_I  .L0
                //   jalr   t0, tY         # R_RISCV_TLSDESC_CALL        .L0
                //
                // Without a descriptor, the first two instructions are
                // deleted by relaxation and the rest becomes either
                // `auipc a0, %gottp_hi; l[d|w] a0, %gottp_lo(a0)`, or for an
                // executable `addi a0, zero, %tpoff_lo` (the addi also
                // deleted) or `lui a0, %tpoff_hi; addi a0, a0, %tpoff_lo`.
                // Without relaxation the useless instructions remain.
                R_RISCV_TLSDESC_HI20 => {
                    if sym.has_tlsdesc(&ctx.symbols) && removed == 0 {
                        utype(loc, sym.tlsdesc_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    }
                }
                R_RISCV_TLSDESC_LOAD_LO12 | R_RISCV_TLSDESC_ADD_LO12 | R_RISCV_TLSDESC_CALL => {
                    if removed == 4 {
                        continue;
                    }
                    let j = find_paired_reloc(ctx, isec, sym, i - 1);
                    let rel2 = &rels.at(j);
                    let sym2 = &ctx.symbols[file.base.symbols[rel2.r_sym as usize]];
                    let a2 = rel2.r_addend as u64;
                    let p2 = isec.addr(ctx) + rel2.r_offset - r_delta(isec, rel2.r_offset) as u64;
                    let tprel = sym2.addr(ctx).wrapping_add(a2).wrapping_sub(ctx.tp_addr);
                    match rel.r_type {
                        R_RISCV_TLSDESC_LOAD_LO12 => {
                            if sym2.has_tlsdesc(&ctx.symbols) {
                                write_itype(
                                    loc,
                                    sym2.tlsdesc_addr(ctx).wrapping_add(a2).wrapping_sub(p2),
                                );
                            } else {
                                write32(loc, NOP);
                            }
                        }
                        R_RISCV_TLSDESC_ADD_LO12 => {
                            if sym2.has_tlsdesc(&ctx.symbols) {
                                write_itype(
                                    loc,
                                    sym2.tlsdesc_addr(ctx).wrapping_add(a2).wrapping_sub(p2),
                                );
                            } else if sym2.has_gottp(&ctx.symbols) {
                                write32(loc, 0x517); // auipc a0, <hi20>
                                utype(loc, sym2.gottp_addr(ctx).wrapping_add(a2).wrapping_sub(p2));
                            } else {
                                write32(loc, 0x537); // lui a0, <hi20>
                                utype(loc, tprel);
                            }
                        }
                        _ => {
                            if sym2.has_tlsdesc(&ctx.symbols) {
                                // Nothing to do.
                            } else if sym2.has_gottp(&ctx.symbols) {
                                write32(loc, if IS_64 { 0x53503 } else { 0x52503 }); // l[d|w] a0, <lo12>
                                write_itype(
                                    loc,
                                    sym2.gottp_addr(ctx).wrapping_add(a2).wrapping_sub(p2),
                                );
                            } else {
                                write32(
                                    loc,
                                    if is_int(tprel as i64, 12) {
                                        0x513
                                    } else {
                                        0x50513
                                    },
                                ); // addi a0, zero|a0, <lo12>
                                write_itype(loc, tprel);
                            }
                        }
                    }
                }
                R_RISCV_ADD8 => loc[0] = loc[0].wrapping_add(sa as u8),
                R_RISCV_ADD16 => End::write_u16(loc, End::read_u16(loc).wrapping_add(sa as u16)),
                R_RISCV_ADD32 => End::write_u32(loc, End::read_u32(loc).wrapping_add(sa as u32)),
                R_RISCV_ADD64 => End::write_u64(loc, End::read_u64(loc).wrapping_add(sa)),
                R_RISCV_SUB8 => loc[0] = loc[0].wrapping_sub(sa as u8),
                R_RISCV_SUB16 => End::write_u16(loc, End::read_u16(loc).wrapping_sub(sa as u16)),
                R_RISCV_SUB32 => End::write_u32(loc, End::read_u32(loc).wrapping_sub(sa as u32)),
                R_RISCV_SUB64 => End::write_u64(loc, End::read_u64(loc).wrapping_sub(sa)),
                R_RISCV_ALIGN => {
                    // R_RISCV_ALIGN is followed by NOPs, some of which may
                    // have been removed to align the next instruction. The
                    // whole NOP sequence is rewritten so that it stays valid
                    // (the first two bytes of a 4-byte NOP can't go alone).
                    let padding = (rel.r_addend - removed) as usize;
                    debug_assert_eq!(padding & 1, 0);
                    let mut k = 0;
                    while k + 4 <= padding {
                        write32(&mut loc[k..], NOP);
                        k += 4;
                    }
                    if k < padding {
                        write16(&mut loc[k..], 0x0001); // c.nop
                    }
                }
                R_RISCV_RVC_BRANCH => {
                    check(pcrel as i64, -(1 << 8), 1 << 8);
                    write_cbtype(loc, pcrel);
                }
                R_RISCV_RVC_JUMP => {
                    check(pcrel as i64, -(1 << 11), 1 << 11);
                    write_cjtype(loc, pcrel);
                }
                R_RISCV_SUB6 => {
                    loc[0] = (loc[0] & 0b1100_0000) | (loc[0].wrapping_sub(sa as u8) & 0b0011_1111)
                }
                R_RISCV_SET6 => loc[0] = (loc[0] & 0b1100_0000) | (sa as u8 & 0b0011_1111),
                R_RISCV_SET8 => loc[0] = sa as u8,
                R_RISCV_SET16 => End::write_u16(loc, sa as u16),
                R_RISCV_SET32 => End::write_u32(loc, sa as u32),
                R_RISCV_PLT32 | R_RISCV_32_PCREL => End::write_u32(loc, pcrel as u32),
                R_RISCV_GOT32_PCREL => End::write_u32(
                    loc,
                    g().wrapping_add(got).wrapping_add(a).wrapping_sub(p) as u32,
                ),
                R_RISCV_SET_ULEB128 => overwrite_uleb(loc, sa),
                R_RISCV_SUB_ULEB128 => {
                    let cur = read_uleb(&mut &loc[..]);
                    overwrite_uleb(loc, cur.wrapping_sub(sa));
                }
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        for rel in isec.rels::<Self>(file) {
            if rel.r_type == R_NONE || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            let off = rel.r_offset as usize;
            let frag = isec.fragment(ctx, &rel);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), rel.r_addend as u64),
            };
            let frag_ref = frag.map(|(f, _)| f);
            let sa = s.wrapping_add(a);
            let loc = &mut buf[off..];

            match rel.r_type {
                R_RISCV_32 => End::write_u32(loc, sa as u32),
                R_RISCV_64 => match isec.tombstone(ctx, sym, frag_ref) {
                    Some(v) => End::write_u64(loc, v),
                    None => End::write_u64(loc, sa),
                },
                R_RISCV_TLS_DTPREL32 => match isec.tombstone(ctx, sym, frag_ref) {
                    Some(v) => End::write_u32(loc, v as u32),
                    None => End::write_u32(loc, sa.wrapping_sub(ctx.dtp_addr) as u32),
                },
                R_RISCV_TLS_DTPREL64 => match isec.tombstone(ctx, sym, frag_ref) {
                    Some(v) => End::write_u64(loc, v),
                    None => End::write_u64(loc, sa.wrapping_sub(ctx.dtp_addr)),
                },
                R_RISCV_ADD8 => loc[0] = loc[0].wrapping_add(sa as u8),
                R_RISCV_ADD16 => End::write_u16(loc, End::read_u16(loc).wrapping_add(sa as u16)),
                R_RISCV_ADD32 => End::write_u32(loc, End::read_u32(loc).wrapping_add(sa as u32)),
                R_RISCV_ADD64 => End::write_u64(loc, End::read_u64(loc).wrapping_add(sa)),
                R_RISCV_SUB8 => loc[0] = loc[0].wrapping_sub(sa as u8),
                R_RISCV_SUB16 => End::write_u16(loc, End::read_u16(loc).wrapping_sub(sa as u16)),
                R_RISCV_SUB32 => End::write_u32(loc, End::read_u32(loc).wrapping_sub(sa as u32)),
                R_RISCV_SUB64 => End::write_u64(loc, End::read_u64(loc).wrapping_sub(sa)),
                R_RISCV_SUB6 => {
                    loc[0] = (loc[0] & 0b1100_0000) | (loc[0].wrapping_sub(sa as u8) & 0b0011_1111)
                }
                R_RISCV_SET6 => loc[0] = (loc[0] & 0b1100_0000) | (sa as u8 & 0b0011_1111),
                R_RISCV_SET8 => loc[0] = sa as u8,
                R_RISCV_SET16 => End::write_u16(loc, sa as u16),
                R_RISCV_SET32 => End::write_u32(loc, sa as u32),
                R_RISCV_SET_ULEB128 => overwrite_uleb(loc, sa),
                R_RISCV_SUB_ULEB128 => {
                    let cur = read_uleb(&mut &loc[..]);
                    overwrite_uleb(loc, cur.wrapping_sub(sa));
                }
                _ => fatal!(
                    ctx,
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    fn emitted_rel_type(ctx: &Context<Self>, isec: &InputSection, rel: &ElfRel, i: usize) -> u32 {
        if !isec.is_alloc() {
            return rel.r_type;
        }
        let rels = isec.rels::<Self>(&ctx.objs[isec.file.index()]);
        let file = &ctx.objs[isec.file.index()];
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
        let (removed, _) = isec.removed_at(rel);

        match rel.r_type {
            R_RISCV_CALL | R_RISCV_CALL_PLT if removed == 4 => R_RISCV_JAL,
            R_RISCV_CALL | R_RISCV_CALL_PLT if removed == 6 => R_RISCV_RVC_JUMP,
            R_RISCV_GOT_HI20 | R_RISCV_HI20 | R_RISCV_TPREL_HI20 | R_RISCV_TPREL_ADD
                if removed != 0 =>
            {
                R_NONE
            }
            R_RISCV_PCREL_LO12_I | R_RISCV_PCREL_LO12_S => {
                // The load of a materialized GOT value is gone with it.
                let j = find_paired_reloc(ctx, isec, sym, i);
                if rels.at(j).r_type == R_RISCV_GOT_HI20 && isec.removed_at(&rels.at(j)).0 != 0 {
                    R_NONE
                } else {
                    rel.r_type
                }
            }
            R_RISCV_TLSDESC_HI20 if !sym.has_tlsdesc(&ctx.symbols) => R_NONE,
            R_RISCV_TLSDESC_LOAD_LO12 | R_RISCV_TLSDESC_ADD_LO12 | R_RISCV_TLSDESC_CALL => {
                let j = find_paired_reloc(ctx, isec, sym, i);
                let sym2 = &ctx.symbols[file.base.symbols[rels.at(j).r_sym as usize]];
                if sym2.has_tlsdesc(&ctx.symbols) {
                    rel.r_type
                } else {
                    R_NONE
                }
            }
            _ => rel.r_type,
        }
    }

    fn shrink_section(ctx: &Context<Self>, isec: &InputSection) -> Vec<RelocDelta> {
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels::<Self>(file);
        let contents = isec.original_contents(file);
        let mut deltas: Vec<RelocDelta> = Vec::new();
        let mut delta = 0i64;

        // Whether 2-byte instructions may be used. They usually may on
        // Unix, since RV64GC is the common baseline.
        let use_rvc = file.base.e_flags & EF_RISCV_RVC != 0;

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

            // R_RISCV_ALIGN must be handled: it refers to NOPs, some or all
            // of which are removed so that the following instruction is
            // aligned. r_addend holds the number of NOP bytes, and the
            // alignment is the smallest power of two greater than that,
            // since the assembler emits enough NOPs for the worst case.
            if r.r_type == R_RISCV_ALIGN {
                let p = isec.addr(ctx) + r.r_offset - delta as u64;
                let desired = align_to(p, (r.r_addend as u64 + 1).next_power_of_two());
                let actual = p + r.r_addend as u64;
                if desired != actual {
                    record(&mut deltas, &mut delta, r, (actual - desired) as i64);
                }
                continue;
            }

            // Other relaxations are optional.
            if !ctx.args.relax || i + 1 == rels.len() || rels.at(i + 1).r_type != R_RISCV_RELAX {
                continue;
            }

            // Linker-synthesized symbols get their values only once the
            // layout is fixed, so they're never relaxed against.
            if sym.file() == ctx.internal_obj.map(FileId::Obj) {
                continue;
            }

            let mut remove = |d: i64| record(&mut deltas, &mut delta, r, d);

            match r.r_type {
                R_RISCV_CALL | R_RISCV_CALL_PLT => {
                    // AUIPC+JALR reaches ±2 GiB; C.J, C.JAL or JAL do for
                    // nearer targets.
                    let dist = compute_distance(ctx, sym, isec, r);
                    if dist & 1 != 0 {
                        continue;
                    }
                    let rd = rd(&contents[r.r_offset as usize + 4..]);
                    if use_rvc && rd == 0 && is_int(dist, 12) {
                        // x0 and within ±2 KiB: C.J saves 6 bytes.
                        remove(6);
                    } else if use_rvc && !IS_64 && rd == 1 && is_int(dist, 12) {
                        // x1 and within ±2 KiB: C.JAL, which is RV32-only.
                        remove(6);
                    } else if is_int(dist, 21) {
                        // Within ±1 MiB: JAL.
                        remove(4);
                    }
                }
                R_RISCV_GOT_HI20 => {
                    // A GOT load of a link-time constant becomes a direct
                    // materialization of the value.
                    if sym.is_absolute() && is_got_load_pair(ctx, isec, i) {
                        let val = sym.addr(ctx).wrapping_add(r.r_addend as u64) as i64;
                        if use_rvc && is_int(val, 6) && rd(&contents[r.r_offset as usize..]) != 0 {
                            remove(6); // AUIPC+LD -> C.LI
                        } else if is_int(val, 12) {
                            remove(4); // AUIPC+LD -> ADDI
                        }
                    }
                }
                R_RISCV_HI20 => {
                    let val = sym.addr(ctx).wrapping_add(r.r_addend as u64) as i64;
                    let rd = rd(&contents[r.r_offset as usize..]);
                    if is_int(val, 12) {
                        // `lui t0, %hi(foo); add t0, t0, %lo(foo)` becomes
                        // `add t0, x0, %lo(foo)` if bits 32..11 of foo are all
                        // ones or all zeros.
                        remove(4);
                    } else if use_rvc && rd != 0 && rd != 2 && is_int(val + 0x800, 18) {
                        // The upper 20 bits fit in 6 bits: C.LUI.
                        remove(2);
                    }
                }
                R_RISCV_TPREL_HI20 | R_RISCV_TPREL_ADD => {
                    // `lui t0, %tprel_hi(foo); add t0, t0, tp` compute
                    // TP + %tprel_hi20(foo), which the low 12-bit access is
                    // relative to. Within TP ± 2 KiB that is TP itself, so
                    // both instructions go and the access uses tp directly.
                    let val = sym
                        .addr(ctx)
                        .wrapping_add(r.r_addend as u64)
                        .wrapping_sub(ctx.tp_addr) as i64;
                    if is_int(val, 12) {
                        remove(4);
                    }
                }
                R_RISCV_TLSDESC_HI20 => {
                    if !sym.has_tlsdesc(&ctx.symbols) {
                        remove(4);
                    }
                }
                R_RISCV_TLSDESC_LOAD_LO12 | R_RISCV_TLSDESC_ADD_LO12 => {
                    let j = find_paired_reloc(ctx, isec, sym, i);
                    let rel2 = &rels.at(j);
                    let sym2 = &ctx.symbols[file.base.symbols[rel2.r_sym as usize]];
                    if r.r_type == R_RISCV_TLSDESC_LOAD_LO12 {
                        if !sym2.has_tlsdesc(&ctx.symbols) {
                            remove(4);
                        }
                    } else if !sym2.has_tlsdesc(&ctx.symbols) && !sym2.has_gottp(&ctx.symbols) {
                        let val = sym2
                            .addr(ctx)
                            .wrapping_add(rel2.r_addend as u64)
                            .wrapping_sub(ctx.tp_addr) as i64;
                        if is_int(val, 12) {
                            remove(4);
                        }
                    }
                }
                _ => {}
            }
        }
        deltas
    }
}

/// Writes a PLT stub reaching the GOT entry at `disp` from the stub.
fn write_plt_stub<const IS_64: bool>(buf: &mut [u8], disp: u64) {
    const ENTRY_64: [u32; 4] = [
        0x0000_0e17, // auipc   t3, %pcrel_hi(function@.got.plt)
        0x000e_3e03, // ld      t3, %pcrel_lo(1b)(t3)
        0x000e_0367, // jalr    t1, t3
        0x0010_0073, // ebreak
    ];
    const ENTRY_32: [u32; 4] = [
        0x0000_0e17, // auipc   t3, %pcrel_hi(function@.got.plt)
        0x000e_2e03, // lw      t3, %pcrel_lo(1b)(t3)
        0x000e_0367, // jalr    t1, t3
        0x0010_0073, // ebreak
    ];
    let entry = if IS_64 { &ENTRY_64 } else { &ENTRY_32 };
    for (i, &v) in entry.iter().enumerate() {
        write32(&mut buf[i * 4..], v);
    }
    write_utype(buf, disp);
    write_itype(&mut buf[4..], disp);
}

// ISA strings
//
// An ISA string such as "rv64i2p1_m2p0_a2p1_f2p2_d2p2_c2p0_zicsr2p0" names
// the base ISA followed by extensions, each with a mandatory major and
// minor version ("m2p0" is the "m" extension, version 2.0). Single-letter
// extensions come first; "z" extensions are named by several letters,
// and "s" and "x" prefixes are reserved for supervisor-level and private
// extensions. Every input object records the string of the extensions it
// uses, and the output gets the merged string. Extensions must appear in
// a specific, not quite alphabetical order for the string to be unique.

#[derive(Clone, Debug)]
struct Extension {
    name: String,
    major: u64,
    minor: u64,
}

/// Whether extension `x` must precede extension `y`. For example,
/// rv64imafd is legal but rv64iafdm isn't.
fn extension_precedes(x: &str, y: &str) -> bool {
    fn single_letter_rank(c: u8) -> i64 {
        const ORDER: &[u8] = b"iemafdqlcbkjtpvnh";
        match ORDER.iter().position(|&e| e == c) {
            Some(pos) => pos as i64,
            None => (c - b'a') as i64 + ORDER.len() as i64,
        }
    }
    fn rank(s: &str) -> i64 {
        let bytes = s.as_bytes();
        match bytes[0] {
            b'x' => 1 << 20,
            b's' => 1 << 19,
            b'z' => (1 << 18) + single_letter_rank(bytes[1]),
            c => single_letter_rank(c),
        }
    }
    (rank(x), x) < (rank(y), y)
}

/// Parses an ISA string into its extensions. Each element is a name
/// (letters and digits, starting and ending with a letter) followed by
/// `<major>p<minor>`; elements are separated by underscores.
fn parse_arch_string(s: &[u8]) -> Option<Vec<Extension>> {
    let mut result = Vec::new();
    for element in s.split(|&b| b == b'_') {
        let element = std::str::from_utf8(element).ok()?;
        let end_of_minor = element.len();
        let minor_start = element.rfind(|c: char| !c.is_ascii_digit())? + 1;
        if minor_start == end_of_minor || !element[..minor_start].ends_with('p') {
            return None;
        }
        let major_end = minor_start - 1;
        let major_start = element[..major_end].rfind(|c: char| !c.is_ascii_digit())? + 1;
        if major_start == major_end {
            return None;
        }
        let name = &element[..major_start];
        let ok_name = name.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
            && name.bytes().last().is_some_and(|c| c.is_ascii_lowercase())
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !ok_name {
            return None;
        }
        result.push(Extension {
            name: name.to_string(),
            major: element[major_start..major_end].parse().ok()?,
            minor: element[minor_start..].parse().ok()?,
        });
    }
    (!result.is_empty()).then_some(result)
}

/// Merges two extension lists, keeping the newer version of an extension
/// present in both. The base ISAs must match.
fn merge_extensions(x: &[Extension], y: &[Extension]) -> Option<Vec<Extension>> {
    if x[0].name != y[0].name {
        return None;
    }
    let mut result = Vec::new();
    let (mut x, mut y) = (x, y);
    while let (Some(a), Some(b)) = (x.first(), y.first()) {
        if a.name == b.name {
            result.push(if (a.major, a.minor) < (b.major, b.minor) {
                b.clone()
            } else {
                a.clone()
            });
            x = &x[1..];
            y = &y[1..];
        } else if extension_precedes(&a.name, &b.name) {
            result.push(a.clone());
            x = &x[1..];
        } else {
            result.push(b.clone());
            y = &y[1..];
        }
    }
    result.extend(x.iter().cloned());
    result.extend(y.iter().cloned());
    Some(result)
}

fn arch_string(extensions: &[Extension]) -> String {
    extensions
        .iter()
        .map(|e| format!("{}{}p{}", e.name, e.major, e.minor))
        .collect::<Vec<_>>()
        .join("_")
}

/// The contents of the output `.riscv.attributes`: the merged ISA string
/// and stack alignment of the inputs.
pub fn attributes_contents<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    let mut stack: Option<u64> = None;
    let mut arch: Vec<Extension> = Vec::new();
    let mut unaligned = false;

    for file in &ctx.objs {
        let attrs = &file.riscv_attributes;
        if let Some(val) = attrs.stack_align {
            if stack.is_some_and(|s| s != val) {
                error!(ctx, "{file}: stack alignment requirement mistmatch");
            }
            stack = Some(val);
        }
        if let Some(s) = attrs.arch {
            let Some(arch2) = parse_arch_string(s) else {
                error!(
                    ctx,
                    "{file}: corrupted .riscv.attributes ISA string: {}",
                    String::from_utf8_lossy(s)
                );
                continue;
            };
            if arch.is_empty() {
                arch = arch2;
            } else {
                match merge_extensions(&arch, &arch2) {
                    Some(merged) => arch = merged,
                    None => error!(
                        ctx,
                        "{file}: incompatible .riscv.attributes ISA string: {}",
                        String::from_utf8_lossy(s)
                    ),
                }
            }
        }
        if attrs.unaligned_access {
            unaligned = true;
        }
    }
    if arch.is_empty() {
        return Vec::new();
    }

    // Format version, then one "riscv" sub-section holding a file-scoped
    // sub-sub-section of tagged attributes. Both carry their length.
    let mut attributes = Vec::new();
    if let Some(stack) = stack {
        encode_uleb(&mut attributes, ELF_TAG_RISCV_STACK_ALIGN as u64);
        encode_uleb(&mut attributes, stack);
    }
    encode_uleb(&mut attributes, ELF_TAG_RISCV_ARCH as u64);
    attributes.extend_from_slice(arch_string(&arch).as_bytes());
    attributes.push(0);
    if unaligned {
        encode_uleb(&mut attributes, ELF_TAG_RISCV_UNALIGNED_ACCESS as u64);
        encode_uleb(&mut attributes, 1);
    }

    let sub_sub_size = 1 + 4 + attributes.len();
    let sub_size = 4 + b"riscv\0".len() + sub_sub_size;
    let u32_bytes = |v: u32| {
        let mut bytes = [0u8; 4];
        E::Endian::write_u32(&mut bytes, v);
        bytes
    };
    let mut out = vec![b'A'];
    out.extend_from_slice(&u32_bytes(sub_size as u32));
    out.extend_from_slice(b"riscv\0");
    out.push(ELF_TAG_FILE as u8);
    out.extend_from_slice(&u32_bytes(sub_sub_size as u32));
    out.extend_from_slice(&attributes);
    out
}
