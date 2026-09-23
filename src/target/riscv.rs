//! RISC-V is a clean RISC ISA. It supports PC-relative load/store for
//! position-independent code. Its 32-bit and 64-bit ISAs are almost
//! identical. That is, you can think RV32 as a RV64 without 64-bit
//! operations. In this file, we support both RV64 and RV32.
//!
//! RISC-V is essentially little-endian, but the big-endian version is
//! available as an extension. GCC supports `-mbig-endian` to generate
//! big-endian code. Even in big-endian mode, machine instructions are
//! defined to be encoded in little-endian, though. Only the behavior of
//! load/store instructions are different between LE RISC-V and BE RISC-V.
//!
//! From the linker's point of view, the RISC-V's psABI is unique because
//! sections in input object files can be shrunk while being copied to the
//! output file. That is contrary to other psABIs in which sections are an
//! atomic unit of copying. See the file comments in shrink_sections.rs for
//! details.
//!
//! https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc

// Binary literals are grouped by instruction field.
#![allow(clippy::unusual_byte_groupings)]

use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::input_sections::NonAllocReloc;
use crate::input_sections::{
    InputSection, RelocDelta, check_tlsle, r_delta, scan_absrel, scan_pcrel, scan_tlsdesc,
};
use crate::shrink_sections::compute_distance;
use crate::symbol::{NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD, Symbol};
use crate::target::{Class, ElfClass, Family, Target};
use crate::util::{align_to, bit, bits, encode_uleb, is_int, overwrite_uleb, read_uleb};
use crate::{error, fatal};

/// RISC-V of a given word size and byte order.
#[derive(Clone, Copy, Debug, Default)]
pub struct RiscvTarget<const LE: bool, const IS_64: bool>;

pub type Riscv64 = RiscvTarget<true, true>;
pub type Riscv64Be = RiscvTarget<false, true>;
pub type Riscv32 = RiscvTarget<true, false>;
pub type Riscv32Be = RiscvTarget<false, false>;

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
    write32(loc, (insn32(loc) & 0b000000_00000_11111_111_11111_1111111) | (b(val, 11, 0) << 20));
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
    write32(loc, (insn32(loc) & 0b000000_11111_11111_111_00000_1111111) | imm);
}

fn write_utype(loc: &mut [u8], val: u64) {
    // U-type instructions are used in combination with I-type
    // instructions. U-type insn sets an immediate to the upper 20-bits
    // of a register. I-type insn sign-extends a 12-bits immediate and
    // adds it to a register value to construct a complete value. 0x800
    // is added here to compensate for the sign-extension.
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
    write32(loc, (insn32(loc) & 0b000000_00000_00000_000_11111_1111111) | imm);
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
    write32(loc, (insn32(loc) & 0b111111_11111_00000_111_11111_1111111) | (rs1 << 15));
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

// RISC-V generally uses the AUIPC + ADDI/LW/SW/etc instruction pair
// to access the AUIPC's address ± 2 GiB. AUIPC materializes the most
// significant 52 bits in a PC-relative manner, and the following
// instruction specifies the remaining least significant 12 bits.
// There are several HI20 and LO12 relocation types for them.
//
// LO12 relocations need to materialize an address relative to AUIPC's
// address, not relative to the instruction that the relocation
// directly refers to.
//
// The problem here is that the instruction pair may not always be
// adjacent. We need a mechanism to find a paired AUIPC for a given
// LO12 relocation. For this purpose, the compiler creates a local
// symbol for each location to which HI20 refers, and the LO12
// relocation refers to that symbol.
//
// This function returns a paired HI20 relocation for a given LO12.
// Since the instructions are typically adjacent, we do a linear
// search.
fn find_paired_reloc<E: Target>(
    ctx: &Context<E>,
    isec: &InputSection<E>,
    rels: &[ElfRel<E>],
    sym: &Symbol,
    i: usize,
) -> usize {
    let value = sym.esym(ctx).st_value();
    if value <= rels[i].r_offset() {
        for j in (0..i).rev() {
            if is_hi20(rels[j].r_type()) && value == rels[j].r_offset() {
                return j;
            }
        }
    } else {
        for (j, rel) in rels.iter().enumerate().skip(i + 1) {
            if is_hi20(rel.r_type()) && value == rel.r_offset() {
                return j;
            }
        }
    }
    let file = &ctx.objs[isec.file.index()];
    fatal!("{}: paired relocation is missing: {i}", isec.display(file));
}

// Returns true if isec's i'th relocation refers to the following
// GOT-load instructioon pair, which is an expeanded form of
// `la t0, foo` pseudo assembly instruction.
//
// .L0
//   auipc t0, 0      # R_RISCV_GOT_HI20(foo),     R_RISCV_RELAX
//   ld    t0, 0(t0)  # R_RISCV_PCREL_LO12_I(.L0), R_RISCV_RELAX
fn is_got_load_pair<E: Target>(
    ctx: &Context<E>,
    isec: &InputSection<E>,
    rels: &[ElfRel<E>],
    i: usize,
) -> bool {
    let file = &ctx.objs[isec.file.index()];
    let contents = isec.original_contents(file);
    i + 3 < rels.len()
        && rels[i].r_type() == R_RISCV_GOT_HI20
        && rels[i + 1].r_type() == R_RISCV_RELAX
        && rels[i + 2].r_type() == R_RISCV_PCREL_LO12_I
        && rels[i + 3].r_type() == R_RISCV_RELAX
        && rels[i].r_offset() == rels[i + 2].r_offset() - 4
        && rels[i].r_offset() == ctx.symbols[file.base.symbols[rels[i + 2].r_sym() as usize]].value
        && rd(&contents[rels[i].r_offset() as usize..])
            == rd(&contents[rels[i + 2].r_offset() as usize..])
}

impl<const LE: bool, const IS_64: bool> Target for RiscvTarget<LE, IS_64>
where
    Class<IS_64>: ElfClass,
{
    const IS_LITTLE: bool = LE;
    type Word = <Class<IS_64> as ElfClass>::Word<Self>;
    type Sym = <Class<IS_64> as ElfClass>::Sym<Self>;
    type Phdr = <Class<IS_64> as ElfClass>::Phdr<Self>;
    type Chdr = <Class<IS_64> as ElfClass>::Chdr<Self>;
    type Rel = ElfRela<Self>;

    type InputSectionExtra = Box<[RelocDelta]>;

    const NAME: &'static str = match (IS_64, Self::IS_LITTLE) {
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
    // The C++ RV64LE and RV32LE target structs each record this instruction:
    // c.ebreak
    // c.ebreak
    const TRAP: &'static [u8] = &[0x02, 0x90];

    const R_COPY: u32 = R_RISCV_COPY;
    const R_GLOB_DAT: u32 = if IS_64 { R_RISCV_64 } else { R_RISCV_32 };
    const R_JUMP_SLOT: u32 = R_RISCV_JUMP_SLOT;
    const R_ABS: u32 = if IS_64 { R_RISCV_64 } else { R_RISCV_32 };
    const R_RELATIVE: u32 = R_RISCV_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_RISCV_IRELATIVE);
    const R_DTPOFF: u32 = if IS_64 { R_RISCV_TLS_DTPREL64 } else { R_RISCV_TLS_DTPREL32 };
    const R_TPOFF: u32 = if IS_64 { R_RISCV_TLS_TPREL64 } else { R_RISCV_TLS_TPREL32 };
    const R_DTPMOD: u32 = if IS_64 { R_RISCV_TLS_DTPMOD64 } else { R_RISCV_TLS_DTPMOD32 };
    const R_TLSDESC: Option<u32> = Some(R_RISCV_TLSDESC);
    const R_FUNCALL: &'static [u32] = &[R_RISCV_CALL, R_RISCV_CALL_PLT];

    fn rel_to_string(r_type: u32) -> std::borrow::Cow<'static, str> {
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
                error!(
                    "{file}: cannot link object files with different floating-point ABI from {first}"
                );
            }
            if flags & EF_RISCV_RVE != ret & EF_RISCV_RVE {
                error!("{file}: cannot link object files with different EF_RISCV_RVE from {first}");
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
        let gotplt = ctx.gotplt.shdr.sh_addr.get();
        let plt = ctx.plt.hdr.shdr.sh_addr.get();
        let disp = gotplt.wrapping_sub(plt);
        write_utype(buf, disp);
        write_itype(&mut buf[8..], disp);
        write_itype(&mut buf[16..], disp);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_plt_stub::<IS_64>(buf, sym.gotplt_addr(ctx).wrapping_sub(sym.plt_addr(ctx)));
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_plt_stub::<IS_64>(buf, sym.got_pltgot_addr(ctx).wrapping_sub(sym.plt_addr(ctx)));
    }

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        isec: &InputSection<Self>,
        rel: &ElfRel<Self>,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        let check = |val: i64, lo: i64, hi: i64| eh_frame::check_range(ctx, isec, rel, val, lo, hi);
        match rel.r_type() {
            R_NONE => {}
            R_RISCV_ADD32 => Self::write_u32(loc, Self::read_u32(loc).wrapping_add(val as u32)),
            R_RISCV_SUB8 => loc[0] = loc[0].wrapping_sub(val as u8),
            R_RISCV_SUB16 => Self::write_u16(loc, Self::read_u16(loc).wrapping_sub(val as u16)),
            R_RISCV_SUB32 => Self::write_u32(loc, Self::read_u32(loc).wrapping_sub(val as u32)),
            R_RISCV_SUB6 => {
                loc[0] = (loc[0] & 0b1100_0000) | (loc[0].wrapping_sub(val as u8) & 0b0011_1111)
            }
            R_RISCV_SET6 => loc[0] = (loc[0] & 0b1100_0000) | (val as u8 & 0b0011_1111),
            R_RISCV_SET8 => loc[0] = val as u8,
            R_RISCV_SET16 => Self::write_u16(loc, val as u16),
            R_RISCV_SET32 => Self::write_u32(loc, val as u32),
            R_RISCV_32_PCREL => {
                check(val.wrapping_sub(p) as i64, -(1 << 31), 1 << 31);
                Self::write_u32(loc, val.wrapping_sub(p) as u32);
            }
            _ => eh_frame::unsupported::<Self>(rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection<Self>) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];

        // Scan relocations
        for rel in isec.relocations(ctx) {
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            match rel.r_type() {
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
                R_RISCV_ALIGN => {
                    // r_addend is the size of the nop sequence at r_offset,
                    // which must be in this section.
                    if rel.r_addend() < 0
                        || rel.r_addend() as u64 > isec.sh_size.saturating_sub(rel.r_offset())
                    {
                        fatal!(
                            "{}: R_RISCV_ALIGN: invalid alignment requirement",
                            isec.display(file)
                        );
                    }
                }
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
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    fn apply_reloc_alloc(
        ctx: &Context<Self>,
        isec: &InputSection<Self>,
        rels: &mut [ElfRel<Self>],
        buf: &mut [u8],
    ) {
        let file = &ctx.objs[isec.file.index()];
        let isec_addr = isec.addr(ctx);
        let got = ctx.got.hdr.shdr.sh_addr.get();
        let contents = isec.original_contents(file);
        let mut i = 0;

        while i < rels.len() {
            let rel_idx = i;
            let rel = rels[rel_idx];
            i += 1;
            if rel.r_type() == R_NONE || rel.r_type() == R_RISCV_RELAX || Self::is_absrel(&rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let (removed, delta) = isec.removed_at(&rel);
            let r_offset = rel.r_offset() - delta as u64;
            let s = sym.addr(ctx);
            let a = rel.r_addend() as u64;
            let p = isec_addr + r_offset;
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, rel_idx, val, lo, hi);
            let utype = |loc: &mut [u8], val: u64| {
                check(val as i64, -(1i64 << 31) - 0x800, (1i64 << 31) - 0x800);
                write_utype(loc, val);
            };
            let loc = &mut buf[r_offset as usize..];
            let orig = &contents[rel.r_offset() as usize..];

            match rel.r_type() {
                R_RISCV_32 => Self::write_u32(loc, sa as u32),
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
                        if ctx.args.emit_relocs {
                            rels[rel_idx].set_r_type(R_RISCV_JAL);
                        }
                    } else if removed == 6 && rd == 0 {
                        // auipc + jalr -> c.j
                        write16(loc, 0b101_00000000000_01);
                        write_cjtype(loc, pcrel);
                        if ctx.args.emit_relocs {
                            rels[rel_idx].set_r_type(R_RISCV_RVC_JUMP);
                        }
                    } else if removed == 6 && rd == 1 {
                        // auipc + jalr -> c.jal
                        debug_assert!(!IS_64);
                        write16(loc, 0b001_00000000000_01);
                        write_cjtype(loc, pcrel);
                        if ctx.args.emit_relocs {
                            rels[rel_idx].set_r_type(R_RISCV_RVC_JUMP);
                        }
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
                    // This relocation usually refers to an AUIPC + LD instruction
                    // pair to load a symbol value from the GOT. If the symbol value
                    // is actually a link-time constant, we can materialize the value
                    // directly into a register to eliminate a memory load.
                    let rd = rd(orig);
                    if removed == 6 {
                        // c.li <rd>, val
                        write16(loc, 0b010_0_00000_00000_01 | (rd as u16) << 7);
                        write_citype(loc, s);

                        // The value is materialized directly, so neither this nor the paired
                        // PCREL_LO12 (the load) needs a relocation anymore.
                        if ctx.args.emit_relocs {
                            rels[rel_idx].set_r_type(R_NONE);
                            rels[rel_idx + 2].set_r_type(R_NONE);
                        }
                        i += 3;
                    } else if removed == 4 {
                        // addi <rd>, zero, val
                        write32(loc, 0b0010011 | (rd << 7));
                        write_itype(loc, s);
                        if ctx.args.emit_relocs {
                            rels[rel_idx].set_r_type(R_NONE);
                            rels[rel_idx + 2].set_r_type(R_NONE);
                        }
                        i += 3;
                    } else {
                        debug_assert_eq!(removed, 0);
                        if ctx.args.relax
                            && sym.is_pcrel_linktime_const(ctx)
                            && is_got_load_pair(ctx, isec, rels, rel_idx)
                            && is_int(pcrel as i64, 32)
                        {
                            // auipc <rd>, %hi20(val)
                            utype(loc, pcrel);

                            // addi <rd>, <rd>, %lo12(val)
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
                    let j = find_paired_reloc(ctx, isec, rels, sym, rel_idx);
                    let rel2 = rels[j];
                    let sym2 = &ctx.symbols[file.base.symbols[rel2.r_sym() as usize]];
                    let write = if rel.r_type() == R_RISCV_PCREL_LO12_I {
                        write_itype
                    } else {
                        write_stype
                    };
                    let a2 = rel2.r_addend() as u64;
                    let p2 = isec_addr + rel2.r_offset() - r_delta(isec, rel2.r_offset()) as u64;
                    match rel2.r_type() {
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
                    // lui (+ addi) => an instruction holding a link-time constant. The lui
                    // may be compressed to c.lui (removed 2 bytes) or deleted outright
                    // (removed 4 bytes); either way it no longer needs a relocation.
                    if removed == 2 {
                        // Rewrite LUI with C.LUI
                        let rd = rd(orig);
                        write16(loc, 0b011_0_00000_00000_01 | (rd as u16) << 7);
                        write_citype(loc, sa.wrapping_add(0x800) >> 12);
                    } else if removed == 0 {
                        utype(loc, sa);
                    }
                    if removed != 0 && ctx.args.emit_relocs {
                        rels[rel_idx].set_r_type(R_NONE);
                    }
                }
                R_RISCV_LO12_I | R_RISCV_LO12_S => {
                    if rel.r_type() == R_RISCV_LO12_I {
                        write_itype(loc, sa);
                    } else {
                        write_stype(loc, sa);
                    }
                    // Rewrite `lw t1, 0(t0)` with `lw t1, 0(x0)` if the address is
                    // accessible relative to the zero register because if that's the
                    // case, corresponding LUI might have been removed by relaxation.
                    if is_int(sa as i64, 12) {
                        set_rs1(loc, 0);
                    }
                }
                R_RISCV_TPREL_HI20 => {
                    debug_assert!(removed == 0 || removed == 4);

                    // lui + add => deleted; the variable is accessed relative to tp directly.
                    if removed == 0 {
                        utype(loc, sa.wrapping_sub(ctx.tp_addr));
                    } else if ctx.args.emit_relocs {
                        rels[rel_idx].set_r_type(R_NONE);
                    }
                }
                R_RISCV_TPREL_ADD => {
                    // This relocation just annotates an ADD instruction that can be
                    // removed when a TPREL is relaxed. No value is needed to be
                    // written.
                    debug_assert!(removed == 0 || removed == 4);
                    if removed != 0 && ctx.args.emit_relocs {
                        rels[rel_idx].set_r_type(R_NONE);
                    }
                }
                R_RISCV_TPREL_LO12_I | R_RISCV_TPREL_LO12_S => {
                    let val = sa.wrapping_sub(ctx.tp_addr);
                    if rel.r_type() == R_RISCV_TPREL_LO12_I {
                        write_itype(loc, val);
                    } else {
                        write_stype(loc, val);
                    }
                    // Rewrite `lw t1, 0(t0)` with `lw t1, 0(tp)` if the address is
                    // directly accessible using tp. tp is x4.
                    if is_int(val as i64, 12) {
                        set_rs1(loc, 4);
                    }
                }
                // RISC-V TLSDESC uses the following code sequence to materialize
                // a TP-relative address in a0.
                //
                //   .L0:
                //   auipc  tX, 0
                //       R_RISCV_TLSDESC_HI20         foo
                //   l[d|w] tY, tX, 0
                //       R_RISCV_TLSDESC_LOAD_LO12_I  .L0
                //   addi   a0, tX, 0
                //       R_RISCV_TLSDESC_ADD_LO12_I   .L0
                //   jalr   t0, tY
                //       R_RISCV_TLSDESC_CALL         .L0
                //
                // For non-dlopen'd DSO, we may relax the instructions to the following:
                //
                //   <deleted>
                //   <deleted>
                //   auipc  a0, %gottp_hi(a0)
                //   l[d|w] a0, %gottp_lo(a0)
                //
                // For executable, if the TP offset is small enough, we'll relax
                // it to the following:
                //
                //   <deleted>
                //   <deleted>
                //   <deleted>
                //   addi   a0, zero, %tpoff_lo(a0)
                //
                // Otherwise, the following sequence is used:
                //
                //   <deleted>
                //   <deleted>
                //   lui    a0, %tpoff_hi(a0)
                //   addi   a0, a0, %tpoff_lo(a0)
                //
                // If the code-shrinking relaxation is disabled, we may leave
                // original useless instructions instead of deleting them, but we
                // accept that because relaxations are enabled by default.
                R_RISCV_TLSDESC_HI20 => {
                    if sym.has_tlsdesc(&ctx.symbols) && removed == 0 {
                        utype(loc, sym.tlsdesc_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else if !sym.has_tlsdesc(&ctx.symbols) && ctx.args.emit_relocs {
                        rels[rel_idx].set_r_type(R_NONE);
                    }
                }
                R_RISCV_TLSDESC_LOAD_LO12 | R_RISCV_TLSDESC_ADD_LO12 | R_RISCV_TLSDESC_CALL => {
                    let j = find_paired_reloc(ctx, isec, rels, sym, rel_idx);
                    let rel2 = rels[j];
                    let sym2 = &ctx.symbols[file.base.symbols[rel2.r_sym() as usize]];
                    if !sym2.has_tlsdesc(&ctx.symbols) && ctx.args.emit_relocs {
                        rels[rel_idx].set_r_type(R_NONE);
                    }
                    if removed == 4 {
                        continue;
                    }
                    let a2 = rel2.r_addend() as u64;
                    let p2 = isec_addr + rel2.r_offset() - r_delta(isec, rel2.r_offset()) as u64;
                    let tprel = sym2.addr(ctx).wrapping_add(a2).wrapping_sub(ctx.tp_addr);
                    match rel.r_type() {
                        R_RISCV_TLSDESC_LOAD_LO12 => {
                            if sym2.has_tlsdesc(&ctx.symbols) {
                                write_itype(
                                    loc,
                                    sym2.tlsdesc_addr(ctx).wrapping_add(a2).wrapping_sub(p2),
                                );
                            } else {
                                write32(loc, NOP); // nop
                            }
                        }
                        R_RISCV_TLSDESC_ADD_LO12 => {
                            if sym2.has_tlsdesc(&ctx.symbols) {
                                write_itype(
                                    loc,
                                    sym2.tlsdesc_addr(ctx).wrapping_add(a2).wrapping_sub(p2),
                                );
                            } else if sym2.has_gottp(&ctx.symbols) {
                                write32(loc, 0x517); // auipc a0,<hi20>
                                utype(loc, sym2.gottp_addr(ctx).wrapping_add(a2).wrapping_sub(p2));
                            } else {
                                write32(loc, 0x537); // lui a0,<hi20>
                                utype(loc, tprel);
                            }
                        }
                        _ => {
                            if sym2.has_tlsdesc(&ctx.symbols) {
                                // Do nothing
                            } else if sym2.has_gottp(&ctx.symbols) {
                                // l[d|w] a0,<lo12>
                                write32(loc, if IS_64 { 0x53503 } else { 0x52503 });
                                write_itype(
                                    loc,
                                    sym2.gottp_addr(ctx).wrapping_add(a2).wrapping_sub(p2),
                                );
                            } else {
                                let insn = if is_int(tprel as i64, 12) {
                                    0x513 // addi a0,zero,<lo12>
                                } else {
                                    0x50513 // addi a0,a0,<lo12>
                                };
                                write32(loc, insn);
                                write_itype(loc, tprel);
                            }
                        }
                    }
                }
                R_RISCV_ADD8 => loc[0] = loc[0].wrapping_add(sa as u8),
                R_RISCV_ADD16 => Self::write_u16(loc, Self::read_u16(loc).wrapping_add(sa as u16)),
                R_RISCV_ADD32 => Self::write_u32(loc, Self::read_u32(loc).wrapping_add(sa as u32)),
                R_RISCV_ADD64 => Self::write_u64(loc, Self::read_u64(loc).wrapping_add(sa)),
                R_RISCV_SUB8 => loc[0] = loc[0].wrapping_sub(sa as u8),
                R_RISCV_SUB16 => Self::write_u16(loc, Self::read_u16(loc).wrapping_sub(sa as u16)),
                R_RISCV_SUB32 => Self::write_u32(loc, Self::read_u32(loc).wrapping_sub(sa as u32)),
                R_RISCV_SUB64 => Self::write_u64(loc, Self::read_u64(loc).wrapping_sub(sa)),
                R_RISCV_ALIGN => {
                    // A R_RISCV_ALIGN is followed by a NOP sequence. We need to remove
                    // zero or more bytes so that the instruction after R_RISCV_ALIGN is
                    // aligned to a given alignment boundary.
                    //
                    // We need to guarantee that the NOP sequence is valid after byte
                    // removal (e.g. we can't remove the first 2 bytes of a 4-byte NOP).
                    // For the sake of simplicity, we always rewrite the entire NOP sequence.
                    let padding = (rel.r_addend() - removed) as usize;
                    debug_assert_eq!(padding & 1, 0);
                    let mut k = 0;
                    while k + 4 <= padding {
                        write32(&mut loc[k..], NOP); // nop
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
                R_RISCV_SET16 => Self::write_u16(loc, sa as u16),
                R_RISCV_SET32 => Self::write_u32(loc, sa as u32),
                R_RISCV_PLT32 | R_RISCV_32_PCREL => Self::write_u32(loc, pcrel as u32),
                R_RISCV_GOT32_PCREL => Self::write_u32(
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

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection<Self>, buf: &mut [u8]) {
        let mut fragment_cache = crate::input_sections::FragmentLookup::default();
        let file = &ctx.objs[isec.file.index()];
        for rel in isec.relocations(ctx) {
            let Some(NonAllocReloc { sym, s, a, frag }) =
                isec.resolve_nonalloc(ctx, file, &rel, &mut fragment_cache)
            else {
                continue;
            };
            let off = rel.r_offset() as usize;
            let sa = s.wrapping_add(a);
            let loc = &mut buf[off..];

            match rel.r_type() {
                R_RISCV_32 => Self::write_u32(loc, sa as u32),
                R_RISCV_64 => match isec.tombstone(ctx, sym, frag) {
                    Some(v) => Self::write_u64(loc, v),
                    None => Self::write_u64(loc, sa),
                },
                R_RISCV_TLS_DTPREL32 => match isec.tombstone(ctx, sym, frag) {
                    Some(v) => Self::write_u32(loc, v as u32),
                    None => Self::write_u32(loc, sa.wrapping_sub(ctx.dtp_addr) as u32),
                },
                R_RISCV_TLS_DTPREL64 => match isec.tombstone(ctx, sym, frag) {
                    Some(v) => Self::write_u64(loc, v),
                    None => Self::write_u64(loc, sa.wrapping_sub(ctx.dtp_addr)),
                },
                R_RISCV_ADD8 => loc[0] = loc[0].wrapping_add(sa as u8),
                R_RISCV_ADD16 => Self::write_u16(loc, Self::read_u16(loc).wrapping_add(sa as u16)),
                R_RISCV_ADD32 => Self::write_u32(loc, Self::read_u32(loc).wrapping_add(sa as u32)),
                R_RISCV_ADD64 => Self::write_u64(loc, Self::read_u64(loc).wrapping_add(sa)),
                R_RISCV_SUB8 => loc[0] = loc[0].wrapping_sub(sa as u8),
                R_RISCV_SUB16 => Self::write_u16(loc, Self::read_u16(loc).wrapping_sub(sa as u16)),
                R_RISCV_SUB32 => Self::write_u32(loc, Self::read_u32(loc).wrapping_sub(sa as u32)),
                R_RISCV_SUB64 => Self::write_u64(loc, Self::read_u64(loc).wrapping_sub(sa)),
                R_RISCV_SUB6 => {
                    loc[0] = (loc[0] & 0b1100_0000) | (loc[0].wrapping_sub(sa as u8) & 0b0011_1111)
                }
                R_RISCV_SET6 => loc[0] = (loc[0] & 0b1100_0000) | (sa as u8 & 0b0011_1111),
                R_RISCV_SET8 => loc[0] = sa as u8,
                R_RISCV_SET16 => Self::write_u16(loc, sa as u16),
                R_RISCV_SET32 => Self::write_u32(loc, sa as u32),
                R_RISCV_SET_ULEB128 => overwrite_uleb(loc, sa),
                R_RISCV_SUB_ULEB128 => {
                    let cur = read_uleb(&mut &loc[..]);
                    overwrite_uleb(loc, cur.wrapping_sub(sa));
                }
                _ => fatal!(
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    // Scan relocations to shrink a given section.
    fn shrink_section(ctx: &Context<Self>, isec: &InputSection<Self>) -> Vec<RelocDelta> {
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels(file);
        let contents = isec.original_contents(file);
        let mut deltas: Vec<RelocDelta> = Vec::new();
        let mut delta = 0i64;

        // True if we can use 2-byte instructions. This is usually true on
        // Unix because RV64GC is generally considered the baseline hardware.
        let use_rvc = file.base.e_flags & EF_RISCV_RVC != 0;

        // Records that `d` bytes go away at relocation `r`.
        fn record<R: RelRecord>(deltas: &mut Vec<RelocDelta>, delta: &mut i64, r: &R, d: i64) {
            *delta += d;
            deltas.push(RelocDelta { offset: r.r_offset(), delta: *delta });
        }

        for i in 0..rels.len() {
            let r = &rels[i];
            let sym = &ctx.symbols[file.base.symbols[r.r_sym() as usize]];

            // Handling R_RISCV_ALIGN is mandatory.
            //
            // R_RISCV_ALIGN refers to NOP instructions. We need to eliminate some
            // or all of the instructions so that the instruction that immediately
            // follows the NOPs is aligned to a specified alignment boundary.
            if r.r_type() == R_RISCV_ALIGN {
                // The total bytes of NOPs is stored to r_addend, so the next
                // instruction is r_addend away. The alignment itself is not recorded
                // anywhere; it is the smallest power of two greater than r_addend,
                // because the assembler emits as many NOP bytes as the worst case
                // requires, which is the alignment minus the minimum instruction
                // size. For example, `.balign 4` yields r_addend 2 in RVC code and
                // `.balign 8` yields 4 in non-RVC code.
                let p = isec.addr(ctx) + r.r_offset() - delta as u64;
                let desired = align_to(p, (r.r_addend() as u64 + 1).next_power_of_two());
                let actual = p + r.r_addend() as u64;
                if desired != actual {
                    record(&mut deltas, &mut delta, r, (actual - desired) as i64);
                }
                continue;
            }

            // Handling other relocations is optional.
            if !ctx.args.relax || i + 1 == rels.len() || rels[i + 1].r_type() != R_RISCV_RELAX {
                continue;
            }

            // Linker-synthesized symbols haven't been assigned their final
            // values when we are shrinking sections because actual values can
            // be computed only after we fix the file layout. Therefore, we
            // assume that relocations against such symbols are always
            // non-relaxable.
            if sym.file() == ctx.internal_obj.map(FileId::Obj) {
                continue;
            }

            let mut remove = |d: i64| record(&mut deltas, &mut delta, r, d);

            match r.r_type() {
                R_RISCV_CALL | R_RISCV_CALL_PLT => {
                    // These relocations refer to an AUIPC + JALR instruction pair to
                    // allow to jump to anywhere in PC ± 2 GiB. If the jump target is
                    // close enough to PC, we can use C.J, C.JAL or JAL instead.
                    let dist = compute_distance(ctx, sym, isec, r);
                    if dist & 1 != 0 {
                        continue;
                    }
                    let rd = rd(&contents[r.r_offset() as usize + 4..]);
                    if use_rvc && rd == 0 && is_int(dist, 12) {
                        // If rd is x0 and the jump target is within ±2 KiB, we can use
                        // C.J, saving 6 bytes.
                        remove(6);
                    } else if use_rvc && !IS_64 && rd == 1 && is_int(dist, 12) {
                        // If rd is x1 and the jump target is within ±2 KiB, we can use
                        // C.JAL. This is RV32 only because C.JAL is RV32-only instruction.
                        remove(6);
                    } else if is_int(dist, 21) {
                        // If the jump target is within ±1 MiB, we can use JAL.
                        remove(4);
                    }
                }
                R_RISCV_GOT_HI20 => {
                    // A GOT_HI20 followed by a PCREL_LO12_I is used to load a value from
                    // GOT. If the loaded value is a link-time constant, we can rewrite
                    // the instructions to directly materialize the value, eliminating a
                    // memory load.
                    if sym.is_absolute() && is_got_load_pair(ctx, isec, rels, i) {
                        let val = sym.addr(ctx).wrapping_add(r.r_addend() as u64) as i64;
                        if use_rvc && is_int(val, 6) && rd(&contents[r.r_offset() as usize..]) != 0
                        {
                            // Replace AUIPC + LD with C.LI.
                            remove(6);
                        } else if is_int(val, 12) {
                            // Replace AUIPC + LD with ADDI.
                            remove(4);
                        }
                    }
                }
                R_RISCV_HI20 => {
                    let val = sym.addr(ctx).wrapping_add(r.r_addend() as u64) as i64;
                    let rd = rd(&contents[r.r_offset() as usize..]);
                    if is_int(val, 12) {
                        // We can replace `lui t0, %hi(foo)` and `add t0, t0, %lo(foo)`
                        // instruction pair with `add t0, x0, %lo(foo)` if foo's bits
                        // [32:11] are all one or all zero.
                        remove(4);
                    } else if use_rvc && rd != 0 && rd != 2 && is_int(val + 0x800, 18) {
                        // If the upper 20 bits can actually be represented in 6 bits,
                        // we can use C.LUI instead of LUI.
                        remove(2);
                    }
                }
                R_RISCV_TPREL_HI20 | R_RISCV_TPREL_ADD => {
                    // These relocations are used to add a high 20-bit value to the
                    // thread pointer. The following two instructions materializes
                    // TP + %tprel_hi20(foo) in %t0, for example.
                    //
                    //  lui  t0, %tprel_hi(foo)         # R_RISCV_TPREL_HI20
                    //  add  t0, t0, tp                 # R_RISCV_TPREL_ADD
                    //
                    // Then thread-local variable `foo` is accessed with the low
                    // 12-bit offset like this:
                    //
                    //  sw   t0, %tprel_lo(foo)(t0)     # R_RISCV_TPREL_LO12_S
                    //
                    // However, if the variable is at TP ± 2 KiB, TP + %tprel_hi20(foo)
                    // is the same as TP, so we can instead access the thread-local
                    // variable directly using TP like this:
                    //
                    //  sw   t0, %tprel_lo(foo)(tp)
                    //
                    // Here, we remove `lui` and `add` if the offset is within ±2 KiB.
                    let val =
                        sym.addr(ctx).wrapping_add(r.r_addend() as u64).wrapping_sub(ctx.tp_addr)
                            as i64;
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
                    let j = find_paired_reloc(ctx, isec, rels, sym, i);
                    let rel2 = &rels[j];
                    let sym2 = &ctx.symbols[file.base.symbols[rel2.r_sym() as usize]];
                    if r.r_type() == R_RISCV_TLSDESC_LOAD_LO12 {
                        if !sym2.has_tlsdesc(&ctx.symbols) {
                            remove(4);
                        }
                    } else if !sym2.has_tlsdesc(&ctx.symbols) && !sym2.has_gottp(&ctx.symbols) {
                        let val = sym2
                            .addr(ctx)
                            .wrapping_add(rel2.r_addend() as u64)
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

// ISA name handlers
//
// An example of ISA name is "rv64i2p1_m2p0_a2p1_f2p2_d2p2_c2p0_zicsr2p0".
// An ISA name starts with the base name (e.g. "rv64i2p1") followed by
// ISA extensions separated by underscores.
//
// There are lots of ISA extensions defined for RISC-V, and they are
// identified by name. Some extensions are of single-letter alphabet such
// as "m" or "q". Newer extension names start with "z" followed by one or
// more alphabets (i.e. "zicsr"). "s" and "x" prefixes are reserved
// for supervisor-level extensions and private extensions, respectively.
//
// Each extension consists of a name, a major version and a minor version.
// For example, "m2p0" indicates the "m" extension of version 2.0. "p" is
// just a separator. Versions are often omitted in documents, but they are
// mandatory in .riscv.attributes. Likewise, abbreviations such as "G"
// (which is short for "IMAFD") are not allowed in .riscv.attributes.
//
// Each RISC-V object file contains an ISA string enumerating extensions
// used by the object file. We need to merge input objects' ISA strings
// into a single ISA string.
//
// In order to guarantee string uniqueness, extensions have to be ordered
// in a specific manner. The exact rule is unfortunately a bit complicated.
//
// The following functions takes care of ISA strings.

#[derive(Clone, Copy, Debug)]
struct Extension<'a> {
    name: &'a str,
    major: u64,
    minor: u64,
}

// As per the RISC-V spec, the extension names must be sorted in a very
// specific way, and unfortunately that's not just an alphabetical order.
// For example, rv64imafd is a legal ISA string, whereas rv64iafdm is not.
// The exact rule is somewhat arbitrary.
//
// This function returns true if the first extension name should precede
// the second one as per the rule.
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
fn parse_arch_string(s: &[u8]) -> Option<Vec<Extension<'_>>> {
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
            && name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !ok_name {
            return None;
        }
        result.push(Extension {
            name,
            major: element[major_start..major_end].parse().ok()?,
            minor: element[minor_start..].parse().ok()?,
        });
    }
    (!result.is_empty()).then_some(result)
}

fn merge_extensions<'a>(x: &[Extension<'a>], y: &[Extension<'a>]) -> Option<Vec<Extension<'a>>> {
    // The base part (i.e. "rv64i" or "rv32i") must match.
    if x[0].name != y[0].name {
        return None;
    }
    let mut result = Vec::new();
    let (mut x, mut y) = (x, y);
    // Merge ISA extension strings
    while let (Some(a), Some(b)) = (x.first(), y.first()) {
        if a.name == b.name {
            result.push(if (a.major, a.minor) < (b.major, b.minor) { *b } else { *a });
            x = &x[1..];
            y = &y[1..];
        } else if extension_precedes(a.name, b.name) {
            result.push(*a);
            x = &x[1..];
        } else {
            result.push(*b);
            y = &y[1..];
        }
    }
    result.extend_from_slice(x);
    result.extend_from_slice(y);
    Some(result)
}

fn arch_string(extensions: &[Extension<'_>]) -> String {
    use std::fmt::Write;

    let mut result = String::new();
    for (i, e) in extensions.iter().enumerate() {
        if i != 0 {
            result.push('_');
        }
        write!(result, "{}{}p{}", e.name, e.major, e.minor).unwrap();
    }
    result
}

// Build the output .riscv.attributes contents.
pub fn attributes_contents<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    let mut stack: Option<u64> = None;
    let mut arch: Vec<Extension<'_>> = Vec::new();
    let mut unaligned = false;

    for file in &ctx.objs {
        let attrs = &file.riscv_attributes;
        if let Some(val) = attrs.stack_align {
            if stack.is_some_and(|s| s != val) {
                error!("{file}: stack alignment requirement mistmatch");
            }
            stack = Some(val);
        }
        if let Some(s) = attrs.arch {
            let Some(arch2) = parse_arch_string(s) else {
                error!(
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
        E::write_u32(&mut bytes, v);
        bytes
    };
    let mut out = vec![b'A']; // Format version
    out.extend_from_slice(&u32_bytes(sub_size as u32)); // Sub-section length
    out.extend_from_slice(b"riscv\0"); // Vendor name
    out.push(ELF_TAG_FILE as u8); // Sub-section tag
    out.extend_from_slice(&u32_bytes(sub_sub_size as u32)); // Sub-sub-section length
    out.extend_from_slice(&attributes);
    out
}
