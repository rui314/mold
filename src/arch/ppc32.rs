//! This file implements the PowerPC 32-bit ISA. For 64-bit PowerPC, see
//! ppc64v1.rs and ppc64v2.rs.
//!
//! PPC32 is a RISC ISA. It has 32 general-purpose registers (GPRs).
//! r0, r11 and r12 are reserved for static linkers, so we can use these
//! registers in PLTs and range extension thunks. In addition to that, it
//! has a few special registers. Notable ones are LR which holds a return
//! address and CTR which we can use to store a branch target address.
//!
//! It feels that the PPC32 psABI is unnecessarily complicated at first
//! glance, but that is mainly stemmed from the fact that the ISA lacks
//! PC-relative load/store instructions. Since machine instructions cannot
//! load data relative to its own address, it is not straightforward to
//! support position-independent code (PIC) on PPC32.
//!
//! A position-independent function typically contains the following code
//! in the prologue to obtain its own address:
//!
//!    mflr  r0        // save the current return address to %r0
//!    bcl   20, 31, 4 // call the next instruction as if it were a function
//!    mtlr  r12       // save the return address to %r12
//!    mtlr  r0        // restore the original return address
//!
//! An object file compiled with -fPIC contains a data section named
//! `.got2` to store addresses of locally-defined global variables and
//! constants. A PIC function usually computes its .got2+0x8000 and set it
//! to %r30. This scheme allows the function to access global objects
//! defined in the same input file with a single %r30-relative load/store
//! instruction with a 16-bit offset, given that .got2 is smaller than
//! 0x10000 (or 65536) bytes.
//!
//! Since each object file has its own .got2, %r30 refers to different
//! places in a merged .got2 for two functions that came from different
//! input files. Therefore, %r30 makes sense only within a single function.
//!
//! Technically, we can reuse a %r30 value in our PLT if we create a PLT
//! _for each input file_ (that's what GNU ld seems to be doing), but that
//! doesn't seems to be worth its complexity. Our PLT simply doesn't rely
//! on a %r30 value.
//!
//! https://github.com/rui314/psabi/blob/main/ppc32.pdf

use std::sync::atomic::Ordering;

use crate::arch::{Arch, Family, ThunkLayout};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::NonAllocReloc;
use crate::input_sections::{check_tlsle, scan_absrel, scan_pcrel, InputSection};
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::thunks::Thunk;
use crate::util::endian::{read_ub32, write_ub16, write_ub32, BigEndian, Ub32};
use crate::util::{bits, is_int};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct Ppc32;

impl Layout for Ppc32 {
    type Endian = BigEndian;
    type Word = Ub32;
    type Sym = Elf32Sym<BigEndian>;
    type Phdr = Elf32Phdr<BigEndian>;
    type Chdr = Elf32Chdr<BigEndian>;
    type Rel = Elf32RelaBe;
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

fn or32(loc: &mut [u8], v: u64) {
    let cur = read_ub32(loc);
    write_ub32(loc, cur | v as u32);
}

fn write_insns(buf: &mut [u8], insns: &[u32]) {
    for (i, &insn) in insns.iter().enumerate() {
        write_ub32(&mut buf[i * 4..], insn);
    }
}

/// A PLT entry, also used as a thunk to a PLT symbol: it materializes
/// its own address, loads the destination from the GOT entry at a
/// known offset and jumps there.
const PLT_ENTRY: [u32; 9] = [
    // Get the address of this PLT entry
    0x7c08_02a6, // mflr    r0
    0x429f_0005, // bcl     20, 31, 4
    0x7d88_02a6, // mflr    r12
    0x7c08_03a6, // mtlr    r0
    // Load an address from the GOT/GOTPLT entry and jump to that address
    0x3d6c_0000, // addis   r11, r12, OFFSET@higha
    0x396b_0000, // addi    r11, r11, OFFSET@lo
    0x818b_0000, // lwz     r12, 0(r11)
    0x7d89_03a6, // mtctr   r12
    0x4e80_0420, // bctr
];

/// Writes a PLT entry that loads its destination from `got`.
fn write_plt_like(buf: &mut [u8], got: u64, entry_addr: u64) {
    write_insns(buf, &PLT_ENTRY);
    let offset = got.wrapping_sub(entry_addr).wrapping_sub(8);
    or32(&mut buf[16..], higha(offset));
    or32(&mut buf[20..], lo(offset));
}

impl Arch for Ppc32 {
    type InputSectionExtra = ();

    const NAME: &'static str = "ppc32";
    const FAMILY: Family = Family::Ppc32;
    const PAGE_SIZE: u64 = 65536;
    const E_MACHINE: u32 = EM_PPC;
    const PLT_HDR_SIZE: u64 = 64;
    const PLT_SIZE: u64 = 36;
    const PLTGOT_SIZE: u64 = 36;
    const THUNK: Option<ThunkLayout> = Some(ThunkLayout { header_size: 0, entry_size: 36 });
    const TRAP: &'static [u8] = &[0x7f, 0xe0, 0x00, 0x08]; // trap

    const R_COPY: u32 = R_PPC_COPY;
    const R_GLOB_DAT: u32 = R_PPC_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_PPC_JMP_SLOT;
    const R_ABS: u32 = R_PPC_ADDR32;
    const R_RELATIVE: u32 = R_PPC_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_PPC_IRELATIVE);
    const R_DTPOFF: u32 = R_PPC_DTPREL32;
    const R_TPOFF: u32 = R_PPC_TPREL32;
    const R_DTPMOD: u32 = R_PPC_DTPMOD32;
    const R_FUNCALL: &'static [u32] = &[R_PPC_REL24, R_PPC_PLTREL24, R_PPC_LOCAL24PC];

    fn rel_to_string(r_type: u32) -> std::borrow::Cow<'static, str> {
        ppc32_rel_to_string(r_type)
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u32; 16] = [
            // Get the address of this PLT section.
            0x7c08_02a6, //    mflr    r0
            0x429f_0005, //    bcl     20, 31, 4
            0x7d88_02a6, // 1: mflr    r12
            0x7c08_03a6, //    mtlr    r0
            // Compute the runtime address of GOTPLT+12.
            0x3d8c_0000, //    addis   r12, r12, (GOTPLT - 1b)@higha
            0x398c_0000, //    addi    r12, r12, (GOTPLT - 1b)@lo
            // Compute the PLT entry offset.
            0x7d6c_5850, //    sub     r11, r11, r12
            0x1d6b_0003, //    mulli   r11, r11, 3
            // Load GOTPLT[2] and branch to GOTPLT[1].
            0x800c_fff8, //    lwz     r0,  -8(r12)
            0x7c09_03a6, //    mtctr   r0
            0x818c_fffc, //    lwz     r12, -4(r12)
            0x4e80_0420, //    bctr
            0x6000_0000, //    nop
            0x6000_0000, //    nop
            0x6000_0000, //    nop
            0x6000_0000, //    nop
        ];
        write_insns(buf, &INSN);
        let gotplt = ctx.gotplt.shdr.sh_addr.get();
        let plt = ctx.plt.hdr.shdr.sh_addr.get();
        let offset = gotplt.wrapping_sub(plt).wrapping_add(4);
        or32(&mut buf[16..], higha(u64::from(offset)));
        or32(&mut buf[20..], lo(u64::from(offset)));
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_plt_like(buf, sym.gotplt_addr(ctx), sym.plt_addr(ctx));
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_plt_like(buf, sym.got_pltgot_addr(ctx), sym.plt_addr(ctx));
    }

    fn apply_eh_reloc(
        _ctx: &Context<Self>,
        _isec: &InputSection<Self>,
        rel: &Self::Rel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        match rel.r_type() {
            R_NONE => {}
            R_PPC_ADDR32 => write_ub32(loc, val as u32),
            R_PPC_REL32 => write_ub32(loc, val.wrapping_sub(p) as u32),
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
                R_PPC_ADDR14 | R_PPC_ADDR16 | R_PPC_UADDR16 | R_PPC_ADDR16_LO | R_PPC_ADDR16_HI
                | R_PPC_ADDR16_HA | R_PPC_ADDR24 | R_PPC_ADDR30 => {
                    scan_absrel(ctx, isec, sym, &rel)
                }
                R_PPC_REL14 | R_PPC_REL16 | R_PPC_REL16_LO | R_PPC_REL16_HI | R_PPC_REL16_HA
                | R_PPC_REL32 => scan_pcrel(ctx, isec, sym, &rel),
                R_PPC_GOT16 | R_PPC_GOT16_LO | R_PPC_GOT16_HI | R_PPC_GOT16_HA | R_PPC_PLT16_LO
                | R_PPC_PLT16_HI | R_PPC_PLT16_HA | R_PPC_PLT32 => sym.add_flags(NEEDS_GOT),
                R_PPC_REL24 | R_PPC_PLTREL24 | R_PPC_PLTREL32 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_PPC_GOT_TLSGD16 => sym.add_flags(NEEDS_TLSGD),
                R_PPC_GOT_TLSLD16 => ctx.needs_tlsld.store(true, Ordering::Relaxed),
                R_PPC_GOT_TPREL16 => sym.add_flags(NEEDS_GOTTP),
                R_PPC_TPREL16_LO | R_PPC_TPREL16_HI | R_PPC_TPREL16_HA => {
                    check_tlsle(ctx, isec, sym, &rel)
                }
                R_PPC_ADDR32 | R_PPC_UADDR32 | R_PPC_LOCAL24PC | R_PPC_TLS | R_PPC_TLSGD
                | R_PPC_TLSLD | R_PPC_DTPREL16_LO | R_PPC_DTPREL16_HI | R_PPC_DTPREL16_HA
                | R_PPC_PLTSEQ | R_PPC_PLTCALL => {}
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
        rels: &mut [Self::Rel],
        buf: &mut [u8],
    ) {
        let file = &ctx.objs[isec.file.index()];
        let got = u64::from(ctx.got.hdr.shdr.sh_addr.get());
        let got2 = file.got2.map_or(0, |shndx| file.section_at(shndx).addr(ctx));

        for rel in rels {
            if rel.r_type() == R_NONE {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let s = sym.addr(ctx);
            let a = rel.r_addend() as u64;
            let p = isec.addr(ctx) + rel.r_offset();
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);
            // PLT16/PLT32 relocations are relative to the file's .got2.
            let plt = || sym.got_addr(ctx).wrapping_sub(a).wrapping_sub(got2);
            let loc = &mut buf[rel.r_offset() as usize..];

            match rel.r_type() {
                R_PPC_ADDR14 => or32(loc, bits(sa, 15, 2) << 2),
                R_PPC_ADDR16 | R_PPC_UADDR16 | R_PPC_ADDR16_LO => write_ub16(loc, lo(sa) as u16),
                R_PPC_ADDR16_HI => write_ub16(loc, hi(sa) as u16),
                R_PPC_ADDR16_HA => write_ub16(loc, ha(sa) as u16),
                R_PPC_ADDR24 => or32(loc, bits(sa, 25, 2) << 2),
                R_PPC_ADDR30 => or32(loc, bits(sa, 31, 2) << 2),
                R_PPC_PLT16_LO => write_ub16(loc, lo(plt()) as u16),
                R_PPC_PLT16_HI => write_ub16(loc, hi(plt()) as u16),
                R_PPC_PLT16_HA => write_ub16(loc, ha(plt()) as u16),
                R_PPC_PLT32 => write_ub32(loc, plt() as u32),
                R_PPC_REL14 => or32(loc, bits(pcrel, 15, 2) << 2),
                R_PPC_REL16 | R_PPC_REL16_LO => write_ub16(loc, lo(pcrel) as u16),
                R_PPC_REL16_HI => write_ub16(loc, hi(pcrel) as u16),
                R_PPC_REL16_HA => write_ub16(loc, ha(pcrel) as u16),
                R_PPC_REL24 | R_PPC_LOCAL24PC => {
                    let mut val = pcrel as i64;
                    if !is_int(val, 26) {
                        val = sym.thunk_addr(ctx, p).wrapping_sub(p) as i64;
                    }
                    or32(loc, bits(val as u64, 25, 2) << 2);
                }
                R_PPC_PLTREL24 => {
                    let mut val = s.wrapping_sub(p) as i64;
                    if sym.has_plt(&ctx.symbols) || !is_int(val, 26) {
                        val = sym.thunk_addr(ctx, p).wrapping_sub(p) as i64;
                    }
                    or32(loc, bits(val as u64, 25, 2) << 2);
                }
                R_PPC_REL32 | R_PPC_PLTREL32 => write_ub32(loc, pcrel as u32),
                R_PPC_GOT16 | R_PPC_GOT16_LO => write_ub16(loc, lo(g().wrapping_add(a)) as u16),
                R_PPC_GOT16_HI => write_ub16(loc, hi(g().wrapping_add(a)) as u16),
                R_PPC_GOT16_HA => write_ub16(loc, ha(g().wrapping_add(a)) as u16),
                R_PPC_TPREL16_LO => write_ub16(loc, lo(sa.wrapping_sub(ctx.tp_addr)) as u16),
                R_PPC_TPREL16_HI => write_ub16(loc, hi(sa.wrapping_sub(ctx.tp_addr)) as u16),
                R_PPC_TPREL16_HA => write_ub16(loc, ha(sa.wrapping_sub(ctx.tp_addr)) as u16),
                R_PPC_DTPREL16_LO => write_ub16(loc, lo(sa.wrapping_sub(ctx.dtp_addr)) as u16),
                R_PPC_DTPREL16_HI => write_ub16(loc, hi(sa.wrapping_sub(ctx.dtp_addr)) as u16),
                R_PPC_DTPREL16_HA => write_ub16(loc, ha(sa.wrapping_sub(ctx.dtp_addr)) as u16),
                R_PPC_GOT_TLSGD16 => write_ub16(loc, sym.tlsgd_addr(ctx).wrapping_sub(got) as u16),
                R_PPC_GOT_TLSLD16 => write_ub16(loc, ctx.got.tlsld_addr().wrapping_sub(got) as u16),
                R_PPC_GOT_TPREL16 => write_ub16(loc, sym.gottp_addr(ctx).wrapping_sub(got) as u16),
                R_PPC_ADDR32 | R_PPC_UADDR32 | R_PPC_TLS | R_PPC_TLSGD | R_PPC_TLSLD
                | R_PPC_PLTSEQ | R_PPC_PLTCALL => {}
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection<Self>, buf: &mut [u8]) {
        let mut fragment_cache = crate::input_sections::FragmentLookup::default();
        let file = &ctx.objs[isec.file.index()];
        for rel in isec.rels(file) {
            let Some(NonAllocReloc { sym, s, a, frag }) =
                isec.resolve_nonalloc(ctx, file, rel, &mut fragment_cache)
            else {
                continue;
            };
            let sa = s.wrapping_add(a);
            let tombstone = isec.tombstone(ctx, sym, frag);
            let loc = &mut buf[rel.r_offset() as usize..];

            match rel.r_type() {
                R_PPC_ADDR32 => write_ub32(loc, tombstone.unwrap_or(sa) as u32),
                R_PPC_DTPREL32 => {
                    write_ub32(loc, tombstone.unwrap_or(sa.wrapping_sub(ctx.dtp_addr)) as u32)
                }
                _ => fatal!(
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    /// On PowerPC, all PLT calls go through range extension thunks.
    fn always_needs_thunk(ctx: &Context<Self>, sym: &Symbol, _rel: &Self::Rel) -> bool {
        sym.has_plt(&ctx.symbols)
    }

    fn write_thunk(ctx: &Context<Self>, thunk: &Thunk, addr: u64, buf: &mut [u8]) {
        const LOCAL_THUNK: [u32; 9] = [
            // Get this thunk's address.
            0x7c08_02a6, // mflr    r0
            0x429f_0005, // bcl     20, 31, 4
            0x7d88_02a6, // mflr    r12
            0x7c08_03a6, // mtlr    r0
            // Materialize the destination's address in %r11 and jump to that address
            0x3d6c_0000, // addis   r11, r12, OFFSET@higha
            0x396b_0000, // addi    r11, r11, OFFSET@lo
            0x7d69_03a6, // mtctr   r11
            0x4e80_0420, // bctr
            0x6000_0000, // nop
        ];

        for (i, &id) in thunk.symbols.iter().enumerate() {
            let sym = &ctx.symbols[id];
            let p = addr + thunk.offsets[i];
            let entry = &mut buf[thunk.offsets[i] as usize..][..36];

            if sym.has_plt(&ctx.symbols) {
                let got = if sym.has_got(&ctx.symbols) {
                    sym.got_addr(ctx)
                } else {
                    sym.gotplt_addr(ctx)
                };
                write_plt_like(entry, got, p);
            } else {
                write_insns(entry, &LOCAL_THUNK);
                let val = sym.addr(ctx).wrapping_sub(p).wrapping_sub(8);
                or32(&mut entry[16..], higha(val));
                or32(&mut entry[20..], lo(val));
            }
        }
    }
}
