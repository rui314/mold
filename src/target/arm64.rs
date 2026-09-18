//! This file contains ARM64-specific code. Being new, the ARM64's ELF
//! psABI doesn't have anything peculiar. ARM64 is a clean RISC
//! instruction set that supports PC-relative load/store instructions.
//!
//! Unlike ARM32, instructions length doesn't vary. All ARM64
//! instructions are 4 bytes long.
//!
//! Branch instructions used for function call can jump within ±128 MiB.
//! We need to create range extension thunks to support binaries whose
//! .text is larger than that.
//!
//! Unlike most other targets, the TLSDESC access model is used by default
//! for -fPIC to access thread-local variables instead of the less
//! efficient GD model. You can still enable GD but it needs the
//! -mtls-dialect=trad flag. Since GD is used rarely, we don't need to
//! implement GD → LE relaxation.
//!
//! https://github.com/ARM-software/abi-aa/blob/main/aaelf64/aaelf64.rst
//!
//! Instructions are little-endian even on big-endian targets, where only
//! data is byte-swapped.

use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::NonAllocReloc;
use crate::input_sections::{InputSection, check_tlsle, scan_absrel, scan_pcrel, scan_tlsdesc};
use crate::symbol::{NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD, Symbol};
use crate::target::{Family, Target, ThunkLayout};
use crate::thunks::Thunk;
use crate::util::endian::{read_ul32, write_ul32};
use crate::util::{bits, is_int};
use crate::{error, fatal};

/// ARM64, in either byte order.
#[derive(Clone, Copy, Debug, Default)]
pub struct Arm64Target<const LE: bool>;

pub type Arm64 = Arm64Target<true>;
pub type Arm64Be = Arm64Target<false>;

/// Instructions are always little-endian.
fn insn(loc: &[u8]) -> u32 {
    read_ul32(loc)
}

fn write_insn(loc: &mut [u8], v: u32) {
    write_ul32(loc, v);
}

fn or_insn(loc: &mut [u8], v: u32) {
    write_insn(loc, insn(loc) | v);
}

fn write_adrp(loc: &mut [u8], val: u64) {
    or_insn(loc, ((bits(val, 13, 12) << 29) | (bits(val, 32, 14) << 5)) as u32);
}

fn write_adr(loc: &mut [u8], val: u64) {
    or_insn(loc, ((bits(val, 1, 0) << 29) | (bits(val, 20, 2) << 5)) as u32);
}

/// Rewrites a MOV to MOVZ or MOVN, whichever represents `val`.
fn write_movn_movz(loc: &mut [u8], val: i64) {
    let rd = insn(loc) & 0b0000_0000_0110_0000_0000_0000_0001_1111;
    let imm = if val >= 0 {
        // rewrite to movz
        0xd280_0000 | (bits(val as u64, 15, 0) << 5) as u32
    } else {
        // rewrite to movn
        0x9280_0000 | (bits(!val as u64, 15, 0) << 5) as u32
    };
    write_insn(loc, rd | imm);
}

fn page(val: u64) -> u64 {
    val & !0xfff
}

// https://developer.arm.com/documentation/ddi0596/2021-12/Base-Instructions
fn is_adrp(loc: &[u8]) -> bool {
    // https://developer.arm.com/documentation/ddi0596/2021-12/Base-Instructions/ADRP--Form-PC-relative-address-to-4KB-page-
    bits(insn(loc) as u64, 31, 24) & 0b1001_1111 == 0b1001_0000
}

fn is_ldr(loc: &[u8]) -> bool {
    // https://developer.arm.com/documentation/ddi0596/2021-12/Base-Instructions/LDR--immediate---Load-Register--immediate--
    bits(insn(loc) as u64, 31, 20) & 0b1111_1111_1100 == 0b1111_1001_0100
}

fn is_add(loc: &[u8]) -> bool {
    // https://developer.arm.com/documentation/ddi0596/2021-12/Base-Instructions/ADD--immediate---Add--immediate--
    bits(insn(loc) as u64, 31, 20) & 0b1111_1111_1100 == 0b1001_0001_0000
}

const NOP: u32 = 0xd503_201f;

impl<const LE: bool> Arm64Target<LE> {
    /// Whether the ADRP+ADD pair at relocation `i` can become NOP+ADR,
    /// which the psABI allows when the target is within ±1 MiB.
    fn relaxes_adrp_add(
        ctx: &Context<Self>,
        isec: &InputSection<Self>,
        rels: &[ElfRel<Self>],
        i: usize,
    ) -> bool {
        let rel = &rels[i];
        if !matches!(rel.r_type(), R_AARCH64_ADR_PREL_PG_HI21 | R_AARCH64_ADR_PREL_PG_HI21_NC)
            || !ctx.args.relax
        {
            return false;
        }
        let Some(rel2) = rels.get(i + 1) else {
            return false;
        };
        let file = &ctx.objs[isec.file.index()];
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
        if !sym.is_pcrel_linktime_const(ctx) {
            return false;
        }
        let s = sym.addr(ctx);
        let p = isec.addr(ctx) + rel.r_offset();
        let val = s.wrapping_add(rel.r_addend() as u64).wrapping_sub(p).wrapping_sub(4) as i64;
        let off = rel.r_offset() as usize;
        let loc = &isec.contents()[off..];
        is_int(val, 21)
            && rel2.r_type() == R_AARCH64_ADD_ABS_LO12_NC
            && rel2.r_sym() == rel.r_sym()
            && rel2.r_offset() == rel.r_offset() + 4
            && rel2.r_addend() == rel.r_addend()
            && is_adrp(loc)
            && is_add(&loc[4..])
            && bits(insn(loc) as u64, 4, 0) == bits(insn(&loc[4..]) as u64, 4, 0)
    }
}

impl<const LE: bool> Target for Arm64Target<LE> {
    const IS_LITTLE: bool = LE;
    type Word = U64<Self>;
    type Sym = Elf64Sym<Self>;
    type Phdr = Elf64Phdr<Self>;
    type Chdr = Elf64Chdr<Self>;
    type Rel = ElfRela<Self>;

    type InputSectionExtra = ();

    const NAME: &'static str = if Self::IS_LITTLE { "arm64" } else { "arm64be" };
    const FAMILY: Family = Family::Arm64;
    const PAGE_SIZE: u64 = 65536;
    const E_MACHINE: u32 = EM_AARCH64;
    const PLT_HDR_SIZE: u64 = 32;
    const PLT_SIZE: u64 = 16;
    const PLTGOT_SIZE: u64 = 16;
    const THUNK: Option<ThunkLayout> = Some(ThunkLayout { header_size: 0, entry_size: 24 });
    const SFRAME_ABI: Option<u8> = Some(if Self::IS_LITTLE {
        SFRAME_ABI_AARCH64_ENDIAN_LITTLE
    } else {
        SFRAME_ABI_AARCH64_ENDIAN_BIG
    });
    const TRAP: &'static [u8] = &[0x00, 0x7d, 0x20, 0xd4]; // brk

    const R_COPY: u32 = R_AARCH64_COPY;
    const R_GLOB_DAT: u32 = R_AARCH64_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_AARCH64_JUMP_SLOT;
    const R_ABS: u32 = R_AARCH64_ABS64;
    const R_RELATIVE: u32 = R_AARCH64_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_AARCH64_IRELATIVE);
    const R_DTPOFF: u32 = R_AARCH64_TLS_DTPREL64;
    const R_TPOFF: u32 = R_AARCH64_TLS_TPREL64;
    const R_DTPMOD: u32 = R_AARCH64_TLS_DTPMOD64;
    const R_TLSDESC: Option<u32> = Some(R_AARCH64_TLSDESC);
    const R_SFRAME: Option<u32> = Some(R_AARCH64_PREL64);
    const R_FUNCALL: &'static [u32] = &[R_AARCH64_JUMP26, R_AARCH64_CALL26];

    fn rel_to_string(r_type: u32) -> std::borrow::Cow<'static, str> {
        arm64_rel_to_string(r_type)
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u32; 8] = [
            0xa9bf_7bf0, // stp  x16, x30, [sp,#-16]!
            0x9000_0010, // adrp x16, .got.plt[2]
            0xf940_0211, // ldr  x17, [x16, .got.plt[2]]
            0x9100_0210, // add  x16, x16, .got.plt[2]
            0xd61f_0220, // br   x17
            0xd420_7d00, // brk
            0xd420_7d00, // brk
            0xd420_7d00, // brk
        ];
        for (i, &v) in INSN.iter().enumerate() {
            write_insn(&mut buf[i * 4..], v);
        }
        let gotplt = ctx.gotplt.shdr.sh_addr.get() + 16;
        let plt = ctx.plt.hdr.shdr.sh_addr.get();
        write_adrp(&mut buf[4..], page(gotplt).wrapping_sub(page(plt + 4)));
        or_insn(&mut buf[8..], (bits(gotplt, 11, 3) << 10) as u32);
        or_insn(&mut buf[12..], ((gotplt & 0xfff) << 10) as u32);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u32; 4] = [
            0x9000_0010, // adrp x16, .got.plt[n]
            0xf940_0211, // ldr  x17, [x16, .got.plt[n]]
            0x9100_0210, // add  x16, x16, .got.plt[n]
            0xd61f_0220, // br   x17
        ];
        for (i, &v) in INSN.iter().enumerate() {
            write_insn(&mut buf[i * 4..], v);
        }
        let gotplt = sym.gotplt_addr(ctx);
        let plt = sym.plt_addr(ctx);
        write_adrp(buf, page(gotplt).wrapping_sub(page(plt)));
        or_insn(&mut buf[4..], (bits(gotplt, 11, 3) << 10) as u32);
        or_insn(&mut buf[8..], ((gotplt & 0xfff) << 10) as u32);
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u32; 4] = [
            0x9000_0010, // adrp x16, GOT[n]
            0xf940_0211, // ldr  x17, [x16, GOT[n]]
            0xd61f_0220, // br   x17
            0xd420_7d00, // brk
        ];
        for (i, &v) in INSN.iter().enumerate() {
            write_insn(&mut buf[i * 4..], v);
        }
        let got = sym.got_pltgot_addr(ctx);
        let plt = sym.plt_addr(ctx);
        write_adrp(buf, page(got).wrapping_sub(page(plt)));
        or_insn(&mut buf[4..], (bits(got, 11, 3) << 10) as u32);
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
            R_AARCH64_ABS64 => Self::write_u64(loc, val),
            R_AARCH64_PREL32 => {
                check(val.wrapping_sub(p) as i64, -(1 << 31), 1 << 31);
                Self::write_u32(loc, val.wrapping_sub(p) as u32);
            }
            R_AARCH64_PREL64 => Self::write_u64(loc, val.wrapping_sub(p)),
            _ => eh_frame::unsupported::<Self>(rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection<Self>) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels(file);
        let mut i = 0;

        // Scan relocations
        while i < rels.len() {
            let rel = &rels[i];
            i += 1;
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            let loc = &isec.contents()[rel.r_offset() as usize..];

            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            match rel.r_type() {
                R_AARCH64_MOVW_UABS_G3 => scan_absrel(ctx, isec, sym, rel),
                R_AARCH64_ADR_GOT_PAGE => {
                    // An ADR_GOT_PAGE and GOT_LO12_NC relocation pair is used to load a
                    // symbol's address from GOT. If the GOT value is a link-time
                    // constant, we may be able to rewrite the ADRP+LDR instruction pair
                    // with an ADRP+ADD, eliminating a GOT memory load.
                    let relaxable = ctx.args.relax
                        && sym.is_pcrel_linktime_const(ctx)
                        && rels.get(i).is_some_and(|rel2| {
                            // ADRP+LDR must be consecutive and use the same register to relax.
                            rel2.r_type() == R_AARCH64_LD64_GOT_LO12_NC
                                && rel2.r_offset() == rel.r_offset() + 4
                                && rel2.r_sym() == rel.r_sym()
                                && rel.r_addend() == 0
                                && rel2.r_addend() == 0
                                && is_adrp(loc)
                                && is_ldr(&loc[4..])
                                && {
                                    let rd = bits(insn(loc) as u64, 4, 0);
                                    let rn = bits(insn(&loc[4..]) as u64, 9, 5);
                                    let rt = bits(insn(&loc[4..]) as u64, 4, 0);
                                    rd == rn && rn == rt
                                }
                        });
                    if relaxable {
                        i += 1;
                    } else {
                        sym.add_flags(NEEDS_GOT);
                    }
                }
                R_AARCH64_LD64_GOT_LO12_NC | R_AARCH64_LD64_GOTPAGE_LO15 | R_AARCH64_GOTPCREL32 => {
                    sym.add_flags(NEEDS_GOT)
                }
                R_AARCH64_CALL26 | R_AARCH64_JUMP26 | R_AARCH64_PLT32 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21 | R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC => {
                    sym.add_flags(NEEDS_GOTTP)
                }
                R_AARCH64_ADR_PREL_PG_HI21 | R_AARCH64_ADR_PREL_PG_HI21_NC => {
                    scan_pcrel(ctx, isec, sym, rel)
                }
                R_AARCH64_TLSGD_ADR_PAGE21 => sym.add_flags(NEEDS_TLSGD),
                R_AARCH64_TLSDESC_CALL => scan_tlsdesc(ctx, sym),
                R_AARCH64_TLSLE_MOVW_TPREL_G2
                | R_AARCH64_TLSLE_ADD_TPREL_LO12
                | R_AARCH64_TLSLE_ADD_TPREL_LO12_NC
                | R_AARCH64_TLSLE_LDST8_TPREL_LO12_NC
                | R_AARCH64_TLSLE_LDST16_TPREL_LO12_NC
                | R_AARCH64_TLSLE_LDST32_TPREL_LO12_NC
                | R_AARCH64_TLSLE_LDST64_TPREL_LO12_NC
                | R_AARCH64_TLSLE_LDST128_TPREL_LO12_NC => check_tlsle(ctx, isec, sym, rel),
                R_AARCH64_ABS64
                | R_AARCH64_ADD_ABS_LO12_NC
                | R_AARCH64_ADR_PREL_LO21
                | R_AARCH64_CONDBR19
                | R_AARCH64_LD_PREL_LO19
                | R_AARCH64_LDST16_ABS_LO12_NC
                | R_AARCH64_LDST32_ABS_LO12_NC
                | R_AARCH64_LDST64_ABS_LO12_NC
                | R_AARCH64_LDST128_ABS_LO12_NC
                | R_AARCH64_LDST8_ABS_LO12_NC
                | R_AARCH64_MOVW_UABS_G0
                | R_AARCH64_MOVW_UABS_G0_NC
                | R_AARCH64_MOVW_UABS_G1
                | R_AARCH64_MOVW_UABS_G1_NC
                | R_AARCH64_MOVW_UABS_G2
                | R_AARCH64_MOVW_UABS_G2_NC
                | R_AARCH64_MOVW_PREL_G0
                | R_AARCH64_MOVW_PREL_G0_NC
                | R_AARCH64_MOVW_PREL_G1
                | R_AARCH64_MOVW_PREL_G1_NC
                | R_AARCH64_MOVW_PREL_G2
                | R_AARCH64_MOVW_PREL_G2_NC
                | R_AARCH64_MOVW_PREL_G3
                | R_AARCH64_PREL16
                | R_AARCH64_PREL32
                | R_AARCH64_PREL64
                | R_AARCH64_TLSGD_ADD_LO12_NC
                | R_AARCH64_TLSLE_MOVW_TPREL_G0
                | R_AARCH64_TLSLE_MOVW_TPREL_G0_NC
                | R_AARCH64_TLSLE_MOVW_TPREL_G1
                | R_AARCH64_TLSLE_MOVW_TPREL_G1_NC
                | R_AARCH64_TLSLE_ADD_TPREL_HI12
                | R_AARCH64_TLSDESC_ADR_PAGE21
                | R_AARCH64_TLSDESC_LD64_LO12
                | R_AARCH64_TLSDESC_ADD_LO12 => {}
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
        let mut i = 0;

        while i < rels.len() {
            let rel = rels[i];
            i += 1;
            if rel.r_type() == R_NONE || Self::is_absrel(&rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let off = rel.r_offset() as usize;
            let s = sym.addr(ctx);
            let a = rel.r_addend() as u64;
            let p = isec_addr + rel.r_offset();
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);
            let tprel = sa.wrapping_sub(ctx.tp_addr);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i - 1, val, lo, hi);
            let loc = &mut buf[off..];

            match rel.r_type() {
                R_AARCH64_LDST8_ABS_LO12_NC | R_AARCH64_ADD_ABS_LO12_NC => {
                    or_insn(loc, (bits(sa, 11, 0) << 10) as u32)
                }
                R_AARCH64_LDST16_ABS_LO12_NC => or_insn(loc, (bits(sa, 11, 1) << 10) as u32),
                R_AARCH64_LDST32_ABS_LO12_NC => or_insn(loc, (bits(sa, 11, 2) << 10) as u32),
                R_AARCH64_LDST64_ABS_LO12_NC => or_insn(loc, (bits(sa, 11, 3) << 10) as u32),
                R_AARCH64_LDST128_ABS_LO12_NC => or_insn(loc, (bits(sa, 11, 4) << 10) as u32),
                R_AARCH64_MOVW_UABS_G0 => {
                    check(sa as i64, 0, 1 << 16);
                    or_insn(loc, (bits(sa, 15, 0) << 5) as u32);
                }
                R_AARCH64_MOVW_UABS_G0_NC => or_insn(loc, (bits(sa, 15, 0) << 5) as u32),
                R_AARCH64_MOVW_UABS_G1 => {
                    check(sa as i64, 0, 1 << 32);
                    or_insn(loc, (bits(sa, 31, 16) << 5) as u32);
                }
                R_AARCH64_MOVW_UABS_G1_NC => or_insn(loc, (bits(sa, 31, 16) << 5) as u32),
                R_AARCH64_MOVW_UABS_G2 => {
                    check(sa as i64, 0, 1 << 48);
                    or_insn(loc, (bits(sa, 47, 32) << 5) as u32);
                }
                R_AARCH64_MOVW_UABS_G2_NC => or_insn(loc, (bits(sa, 47, 32) << 5) as u32),
                R_AARCH64_MOVW_UABS_G3 => or_insn(loc, (bits(sa, 63, 48) << 5) as u32),
                R_AARCH64_MOVW_PREL_G0 => {
                    check(pcrel as i64, -(1 << 15), 1 << 15);
                    write_movn_movz(loc, pcrel as i64);
                }
                R_AARCH64_MOVW_PREL_G0_NC => or_insn(loc, (bits(pcrel, 15, 0) << 5) as u32),
                R_AARCH64_MOVW_PREL_G1 => {
                    check(pcrel as i64, -(1 << 31), 1 << 31);
                    write_movn_movz(loc, (pcrel as i64) >> 16);
                }
                R_AARCH64_MOVW_PREL_G1_NC => or_insn(loc, (bits(pcrel, 31, 16) << 5) as u32),
                R_AARCH64_MOVW_PREL_G2 => {
                    check(pcrel as i64, -(1 << 47), 1 << 47);
                    write_movn_movz(loc, (pcrel as i64) >> 32);
                }
                R_AARCH64_MOVW_PREL_G2_NC => or_insn(loc, (bits(pcrel, 47, 32) << 5) as u32),
                R_AARCH64_MOVW_PREL_G3 => or_insn(loc, (bits(pcrel, 63, 48) << 5) as u32),
                R_AARCH64_ADR_GOT_PAGE => {
                    if sym.has_got(&ctx.symbols) {
                        let val = page(g().wrapping_add(got).wrapping_add(a)).wrapping_sub(page(p));
                        check(val as i64, -(1 << 32), 1 << 32);
                        write_adrp(loc, val);
                    } else {
                        // Relax GOT-loading ADRP+LDR to an immediate ADRP+ADD
                        let val = page(sa).wrapping_sub(page(p));
                        check(val as i64, -(1 << 32), 1 << 32);
                        write_adrp(loc, val);
                        let reg = bits(insn(loc) as u64, 4, 0) as u32;
                        write_insn(
                            &mut loc[4..],
                            0x9100_0000 | (reg << 5) | reg | (bits(sa, 11, 0) << 10) as u32,
                        );
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_AARCH64_ADR_PREL_PG_HI21);
                            rels[i].set_r_type(R_AARCH64_ADD_ABS_LO12_NC);
                        }
                        i += 1;
                    }
                }
                R_AARCH64_ADR_PREL_PG_HI21 | R_AARCH64_ADR_PREL_PG_HI21_NC => {
                    // The ARM64 psABI defines that an `ADRP x0, foo` and `ADD x0, x0,
                    // :lo12: foo` instruction pair to materialize a PC-relative address
                    // in a register can be relaxed to `NOP` followed by `ADR x0, foo`
                    // if foo is in PC ± 1 MiB.
                    if Self::relaxes_adrp_add(ctx, isec, rels, i - 1) {
                        let reg = bits(insn(loc) as u64, 4, 0) as u32;
                        write_insn(loc, NOP);
                        write_insn(&mut loc[4..], 0x1000_0000 | reg);
                        write_adr(&mut loc[4..], pcrel.wrapping_sub(4));
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_NONE);
                            rels[i].set_r_type(R_AARCH64_ADR_PREL_LO21);
                        }
                        i += 1;
                    } else {
                        let val = page(sa).wrapping_sub(page(p));
                        if rel.r_type() == R_AARCH64_ADR_PREL_PG_HI21 {
                            check(val as i64, -(1 << 32), 1 << 32);
                        }
                        write_adrp(loc, val);
                    }
                }
                R_AARCH64_ADR_PREL_LO21 => {
                    check(pcrel as i64, -(1 << 20), 1 << 20);
                    write_adr(loc, pcrel);
                }
                R_AARCH64_CALL26 | R_AARCH64_JUMP26 => {
                    if sym.is_remaining_undef_weak() {
                        // On ARM, calling an weak undefined symbol jumps to the
                        // next instruction.
                        write_insn(loc, NOP);
                    } else {
                        let mut val = pcrel;
                        if !is_int(val as i64, 28) {
                            val = sym.thunk_addr(ctx, p).wrapping_add(a).wrapping_sub(p);
                        }
                        or_insn(loc, bits(val, 27, 2) as u32);
                    }
                }
                R_AARCH64_PLT32 => {
                    check(pcrel as i64, -(1 << 31), 1 << 31);
                    Self::write_u32(loc, pcrel as u32);
                }
                R_AARCH64_GOTPCREL32 => {
                    let val = g().wrapping_add(got).wrapping_add(a).wrapping_sub(p);
                    check(val as i64, -(1 << 31), 1 << 31);
                    Self::write_u32(loc, val as u32);
                }
                R_AARCH64_CONDBR19 | R_AARCH64_LD_PREL_LO19 => {
                    check(pcrel as i64, -(1 << 20), 1 << 20);
                    or_insn(loc, (bits(pcrel, 20, 2) << 5) as u32);
                }
                R_AARCH64_PREL16 => {
                    check(pcrel as i64, -(1 << 15), 1 << 16);
                    Self::write_u16(loc, pcrel as u16);
                }
                R_AARCH64_PREL32 => {
                    check(pcrel as i64, -(1 << 31), 1 << 32);
                    Self::write_u32(loc, pcrel as u32);
                }
                R_AARCH64_PREL64 => Self::write_u64(loc, pcrel),
                R_AARCH64_LD64_GOT_LO12_NC => {
                    or_insn(loc, (bits(g().wrapping_add(got).wrapping_add(a), 11, 3) << 10) as u32)
                }
                R_AARCH64_LD64_GOTPAGE_LO15 => {
                    let val = g().wrapping_add(got).wrapping_add(a).wrapping_sub(page(got));
                    check(val as i64, 0, 1 << 15);
                    or_insn(loc, (bits(val, 14, 3) << 10) as u32);
                }
                R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21 => {
                    let val = page(sym.gottp_addr(ctx).wrapping_add(a)).wrapping_sub(page(p));
                    check(val as i64, -(1 << 32), 1 << 32);
                    write_adrp(loc, val);
                }
                R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC => {
                    or_insn(loc, (bits(sym.gottp_addr(ctx).wrapping_add(a), 11, 3) << 10) as u32)
                }
                R_AARCH64_TLSLE_MOVW_TPREL_G0 => {
                    check(tprel as i64, -(1 << 15), 1 << 15);
                    write_movn_movz(loc, tprel as i64);
                }
                R_AARCH64_TLSLE_MOVW_TPREL_G0_NC => or_insn(loc, (bits(tprel, 15, 0) << 5) as u32),
                R_AARCH64_TLSLE_MOVW_TPREL_G1 => {
                    check(tprel as i64, -(1 << 31), 1 << 31);
                    write_movn_movz(loc, (tprel as i64) >> 16);
                }
                R_AARCH64_TLSLE_MOVW_TPREL_G1_NC => or_insn(loc, (bits(tprel, 31, 16) << 5) as u32),
                R_AARCH64_TLSLE_MOVW_TPREL_G2 => {
                    check(tprel as i64, -(1 << 47), 1 << 47);
                    write_movn_movz(loc, (tprel as i64) >> 32);
                }
                R_AARCH64_TLSLE_ADD_TPREL_HI12 => {
                    check(tprel as i64, 0, 1 << 24);
                    or_insn(loc, (bits(tprel, 23, 12) << 10) as u32);
                }
                R_AARCH64_TLSLE_ADD_TPREL_LO12 => {
                    check(tprel as i64, 0, 1 << 12);
                    or_insn(loc, (bits(tprel, 11, 0) << 10) as u32);
                }
                R_AARCH64_TLSLE_ADD_TPREL_LO12_NC => {
                    or_insn(loc, (bits(tprel, 11, 0) << 10) as u32)
                }
                R_AARCH64_TLSLE_LDST8_TPREL_LO12_NC => {
                    or_insn(loc, (bits(tprel, 11, 0) << 10) as u32)
                }
                R_AARCH64_TLSLE_LDST16_TPREL_LO12_NC => {
                    or_insn(loc, (bits(tprel, 11, 1) << 10) as u32)
                }
                R_AARCH64_TLSLE_LDST32_TPREL_LO12_NC => {
                    or_insn(loc, (bits(tprel, 11, 2) << 10) as u32)
                }
                R_AARCH64_TLSLE_LDST64_TPREL_LO12_NC => {
                    or_insn(loc, (bits(tprel, 11, 3) << 10) as u32)
                }
                R_AARCH64_TLSLE_LDST128_TPREL_LO12_NC => {
                    or_insn(loc, (bits(tprel, 11, 4) << 10) as u32)
                }
                R_AARCH64_TLSGD_ADR_PAGE21 => {
                    let val = page(sym.tlsgd_addr(ctx).wrapping_add(a)).wrapping_sub(page(p));
                    check(val as i64, -(1 << 32), 1 << 32);
                    write_adrp(loc, val);
                }
                R_AARCH64_TLSGD_ADD_LO12_NC => {
                    or_insn(loc, (bits(sym.tlsgd_addr(ctx).wrapping_add(a), 11, 0) << 10) as u32)
                }
                // ARM64 TLSDESC uses the following code sequence to materialize
                // a TP-relative address in x0.
                //
                // adrp    x0, 0
                // R_AARCH64_TLSDESC_ADR_PAGE21 foo
                // ldr     x1, [x0]
                // R_AARCH64_TLSDESC_LD64_LO12  foo
                // add     x0, x0, #0
                // R_AARCH64_TLSDESC_ADD_LO12   foo
                // blr     x1
                // R_AARCH64_TLSDESC_CALL       foo
                //
                // We may relax the instructions to the following if its TP-relative
                // address is known at link-time
                //
                // nop
                // nop
                // movz    x0, :tls_offset_hi:foo, lsl #16
                // movk    x0, :tls_offset_lo:foo
                //
                // or to the following if the TP-relative address is known at
                // process startup time.
                //
                // nop
                // nop
                // adrp    x0, :gottprel:foo
                // ldr     x0, [x0, :gottprel_lo12:foo]
                R_AARCH64_TLSDESC_ADR_PAGE21 => {
                    if sym.has_tlsdesc(&ctx.symbols) {
                        let val = page(sym.tlsdesc_addr(ctx).wrapping_add(a)).wrapping_sub(page(p));
                        check(val as i64, -(1 << 32), 1 << 32);
                        write_adrp(loc, val);
                    } else {
                        write_insn(loc, NOP);
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_NONE);
                        }
                    }
                }
                R_AARCH64_TLSDESC_LD64_LO12 => {
                    if sym.has_tlsdesc(&ctx.symbols) {
                        or_insn(
                            loc,
                            (bits(sym.tlsdesc_addr(ctx).wrapping_add(a), 11, 3) << 10) as u32,
                        );
                    } else {
                        write_insn(loc, NOP);
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_NONE);
                        }
                    }
                }
                R_AARCH64_TLSDESC_ADD_LO12 => {
                    if sym.has_tlsdesc(&ctx.symbols) {
                        or_insn(
                            loc,
                            (bits(sym.tlsdesc_addr(ctx).wrapping_add(a), 11, 0) << 10) as u32,
                        );
                    } else if sym.has_gottp(&ctx.symbols) {
                        write_insn(loc, 0x9000_0000); // adrp x0, 0
                        write_adrp(
                            loc,
                            page(sym.gottp_addr(ctx).wrapping_add(a)).wrapping_sub(page(p)),
                        );
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21);
                        }
                    } else {
                        write_insn(loc, 0xd2a0_0000 | (bits(tprel, 32, 16) << 5) as u32);
                        // movz x0, 0, lsl #16
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_AARCH64_TLSLE_MOVW_TPREL_G1);
                        }
                    }
                }
                R_AARCH64_TLSDESC_CALL => {
                    if sym.has_tlsdesc(&ctx.symbols) {
                        // Do nothing
                    } else if sym.has_gottp(&ctx.symbols) {
                        write_insn(
                            loc,
                            0xf940_0000
                                | (bits(sym.gottp_addr(ctx).wrapping_add(a), 11, 3) << 10) as u32,
                        ); // ldr x0, [x0, 0]
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC);
                        }
                    } else {
                        write_insn(loc, 0xf280_0000 | (bits(tprel, 15, 0) << 5) as u32);
                        // movk x0, 0
                        if ctx.args.emit_relocs {
                            rels[i - 1].set_r_type(R_AARCH64_TLSLE_MOVW_TPREL_G0_NC);
                        }
                    }
                }
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection<Self>, buf: &mut [u8]) {
        let mut fragment_cache = crate::input_sections::FragmentLookup::default();
        let file = &ctx.objs[isec.file.index()];
        for (i, rel) in isec.relocations(ctx).enumerate() {
            let Some(NonAllocReloc { sym, s, a, frag }) =
                isec.resolve_nonalloc(ctx, file, &rel, &mut fragment_cache)
            else {
                continue;
            };
            let off = rel.r_offset() as usize;
            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);
            let loc = &mut buf[off..];

            match rel.r_type() {
                R_AARCH64_ABS64 => match isec.tombstone(ctx, sym, frag) {
                    Some(v) => Self::write_u64(loc, v),
                    None => Self::write_u64(loc, s.wrapping_add(a)),
                },
                R_AARCH64_ABS32 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 32);
                    Self::write_u32(loc, s.wrapping_add(a) as u32);
                }
                R_AARCH64_TLS_DTPREL64 => match isec.tombstone(ctx, sym, frag) {
                    Some(v) => Self::write_u64(loc, v),
                    None => Self::write_u64(loc, s.wrapping_add(a).wrapping_sub(ctx.dtp_addr)),
                },
                _ => fatal!(
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    // The size of a thunk entry varies on ARM64 depending on the distance to
    // the branch target. This function computes the size of each thunk entry.
    fn thunk_offsets(ctx: &Context<Self>, thunk: &Thunk, addr: u64) -> Vec<u64> {
        // The distance between S and P is only reduced by shrink_size(), but
        // page(S) – page(P) may still increase by one page due to address
        // changes, so we add a safety margin.
        //
        // For example, page(0x1200) – page(0x1000) is 0, whereas
        // page(0x1100) – page(0xfff) is 0x1000, even though the latter
        // distance is shorter than the former.
        let is_small = |prel: i64| is_int(prel + 0x1000, 33) && is_int(prel - 0x1000, 33);
        let mut offsets = vec![0];
        let mut off = 0;
        for &sym in &thunk.symbols {
            let s = ctx.symbols[sym].addr(ctx);
            let p = addr + off;
            let prel = page(s).wrapping_sub(page(p)) as i64;
            off += if is_small(prel) { 12 } else { 24 };
            offsets.push(off);
        }
        offsets
    }

    fn write_thunk(ctx: &Context<Self>, thunk: &Thunk, addr: u64, buf: &mut [u8]) {
        // Short thunk with a 33 bit displacement
        const SHORT: [u32; 3] = [
            0x9000_0010, // adrp x16, 0
            0x9100_0210, // add  x16, x16
            0xd61f_0200, // br   x16
        ];
        // Long thunk with a 64 bit displacement
        const LONG: [u32; 6] = [
            0x1000_0010, // adr  x16, 0
            0xd2a0_0011, // movz x17, 0, lsl #16
            0xf2c0_0011, // movk x17, 0, lsl #32
            0xf2e0_0011, // movk x17, 0, lsl #48
            0x8b11_0210, // add  x16, x16, x17
            0xd61f_0200, // br   x16
        ];

        for (i, &sym) in thunk.symbols.iter().enumerate() {
            let s = ctx.symbols[sym].addr(ctx);
            let p = addr + thunk.offsets[i];
            let entry = &mut buf[thunk.offsets[i] as usize..thunk.offsets[i + 1] as usize];
            if entry.len() == 12 {
                let prel = page(s).wrapping_sub(page(p));
                debug_assert!(is_int(prel as i64, 33));
                for (j, &v) in SHORT.iter().enumerate() {
                    write_insn(&mut entry[j * 4..], v);
                }
                write_adrp(entry, prel);
                or_insn(&mut entry[4..], (bits(s, 11, 0) << 10) as u32);
            } else {
                let disp = s.wrapping_sub(p);
                for (j, &v) in LONG.iter().enumerate() {
                    write_insn(&mut entry[j * 4..], v);
                }
                write_adr(entry, bits(disp, 15, 0));
                or_insn(&mut entry[4..], (bits(disp, 31, 16) << 5) as u32);
                or_insn(&mut entry[8..], (bits(disp, 47, 32) << 5) as u32);
                or_insn(&mut entry[12..], (bits(disp, 63, 48) << 5) as u32);
            }
        }
    }
}
