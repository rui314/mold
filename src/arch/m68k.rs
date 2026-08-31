//! This file contains code for the Motorola 68000 series microprocessors,
//! which is often abbreviated as m68k. Running a Unix-like system on a
//! m68k-based machine today is a retro-computing hobby activity, but the
//! processor was a popular choice to build Unix computers during '80s.
//! Early Sun workstations for example used m68k. Macintosh until 1994 were
//! based on m68k as well until they switched to PowerPC (and then to x86
//! and to ARM.)
//!
//! From the linker's point of view, it is not hard to support m68k. It's
//! just a 32-bit big-endian CISC ISA. Compared to comtemporary i386,
//! m68k's psABI is actually simpler because m68k has PC-relative memory
//! access instructions and therefore can support position-independent
//! code without too much hassle.
//!
//! https://github.com/rui314/psabi/blob/main/m68k.pdf

use std::sync::atomic::Ordering;

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{check_tlsle, scan_absrel, scan_pcrel, InputSection};
use crate::output_chunks::eh_frame;
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct M68k;

impl Layout for M68k {
    type Endian = BigEndian;
    const IS_64: bool = false;
    const IS_RELA: bool = true;
}

fn w32(loc: &mut [u8], v: u32) {
    BigEndian::write_u32(loc, v);
}

impl Arch for M68k {
    const NAME: &'static str = "m68k";
    const FAMILY: Family = Family::M68k;
    const PAGE_SIZE: u64 = 8192;
    const E_MACHINE: u32 = EM_68K;
    const PLT_HDR_SIZE: u64 = 18;
    const PLT_SIZE: u64 = 14;
    const PLTGOT_SIZE: u64 = 8;
    const TRAP: &'static [u8] = &[0x4a, 0xfc]; // illegal

    const R_COPY: u32 = R_68K_COPY;
    const R_GLOB_DAT: u32 = R_68K_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_68K_JMP_SLOT;
    const R_ABS: u32 = R_68K_32;
    const R_RELATIVE: u32 = R_68K_RELATIVE;
    const R_DTPOFF: u32 = R_68K_TLS_DTPREL32;
    const R_TPOFF: u32 = R_68K_TLS_TPREL32;
    const R_DTPMOD: u32 = R_68K_TLS_DTPMOD32;
    const R_FUNCALL: &'static [u32] = &[R_68K_PLT32];

    fn rel_to_string(r_type: u32) -> String {
        m68k_rel_to_string(r_type)
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u8; 18] = [
            0x2f, 0x00, // move.l %d0, -(%sp)
            0x2f, 0x3b, 0x01, 0x70, 0, 0, 0, 0, // move.l (GOTPLT+4, %pc), -(%sp)
            0x4e, 0xfb, 0x01, 0x71, 0, 0, 0, 0, // jmp    ([GOTPLT+8, %pc])
        ];
        buf[..18].copy_from_slice(&INSN);
        let gotplt = ctx
            .gotplt
            .hdr
            .shdr
            .sh_addr
            .wrapping_sub(ctx.plt.hdr.shdr.sh_addr);
        w32(&mut buf[6..], gotplt as u32);
        w32(&mut buf[14..], gotplt.wrapping_sub(4) as u32);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u8; 14] = [
            0x20, 0x3c, 0, 0, 0, 0, // move.l PLT_OFFSET, %d0
            0x4e, 0xfb, 0x01, 0x71, 0, 0, 0, 0, // jmp    ([GOTPLT_ENTRY, %pc])
        ];
        buf[..14].copy_from_slice(&INSN);
        w32(
            &mut buf[2..],
            sym.plt_idx(&ctx.symbols).unwrap() * ElfRel::size::<Self>() as u32,
        );
        w32(
            &mut buf[10..],
            sym.gotplt_addr(ctx)
                .wrapping_sub(sym.plt_addr(ctx))
                .wrapping_sub(8) as u32,
        );
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u8; 8] = [0x4e, 0xfb, 0x01, 0x71, 0, 0, 0, 0]; // jmp ([GOT_ENTRY, %pc])
        buf[..8].copy_from_slice(&INSN);
        w32(
            &mut buf[4..],
            sym.got_pltgot_addr(ctx)
                .wrapping_sub(sym.plt_addr(ctx))
                .wrapping_sub(2) as u32,
        );
    }

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        _isec: &InputSection,
        rel: &ElfRel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        match rel.r_type {
            R_NONE => {}
            R_68K_32 => w32(loc, val as u32),
            R_68K_PC32 => w32(loc, val.wrapping_sub(p) as u32),
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
                error!(ctx, "{sym}: GNU ifunc symbol is not supported on m68k");
            }

            match rel.r_type {
                R_68K_16 | R_68K_8 => scan_absrel(ctx, isec, sym, &rel),
                R_68K_PC32 | R_68K_PC16 | R_68K_PC8 => scan_pcrel(ctx, isec, sym, &rel),
                R_68K_GOTPCREL32 | R_68K_GOTPCREL16 | R_68K_GOTPCREL8 | R_68K_GOTOFF32
                | R_68K_GOTOFF16 | R_68K_GOTOFF8 => sym.add_flags(NEEDS_GOT),
                R_68K_PLT32 | R_68K_PLT16 | R_68K_PLT8 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_68K_TLS_GD32 | R_68K_TLS_GD16 | R_68K_TLS_GD8 => sym.add_flags(NEEDS_TLSGD),
                R_68K_TLS_LDM32 | R_68K_TLS_LDM16 | R_68K_TLS_LDM8 => {
                    ctx.needs_tlsld.store(true, Ordering::Relaxed)
                }
                R_68K_TLS_IE32 | R_68K_TLS_IE16 | R_68K_TLS_IE8 => sym.add_flags(NEEDS_GOTTP),
                R_68K_TLS_LE32 | R_68K_TLS_LE16 | R_68K_TLS_LE8 => {
                    check_tlsle(ctx, isec, sym, &rel)
                }
                R_68K_32 | R_68K_TLS_LDO32 | R_68K_TLS_LDO16 | R_68K_TLS_LDO8 => {}
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
            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);

            // The narrower fields come in unsigned and signed flavors.
            let write32 = |buf: &mut [u8], val: u64| w32(&mut buf[off..], val as u32);
            let write16 = |buf: &mut [u8], val: u64| {
                check(val as i64, 0, 1 << 16);
                BigEndian::write_u16(&mut buf[off..], val as u16);
            };
            let write16s = |buf: &mut [u8], val: u64| {
                check(val as i64, -(1 << 15), 1 << 15);
                BigEndian::write_u16(&mut buf[off..], val as u16);
            };
            let write8 = |buf: &mut [u8], val: u64| {
                check(val as i64, 0, 1 << 8);
                buf[off] = val as u8;
            };
            let write8s = |buf: &mut [u8], val: u64| {
                check(val as i64, -(1 << 7), 1 << 7);
                buf[off] = val as u8;
            };

            match rel.r_type {
                // Handled as an absolute relocation by the output section.
                R_68K_32 => {}
                R_68K_16 => write16(buf, sa),
                R_68K_8 => write8(buf, sa),
                R_68K_PC32 | R_68K_PLT32 => write32(buf, sa.wrapping_sub(p)),
                R_68K_PC16 | R_68K_PLT16 => write16s(buf, sa.wrapping_sub(p)),
                R_68K_PC8 | R_68K_PLT8 => write8s(buf, sa.wrapping_sub(p)),
                R_68K_GOTPCREL32 => write32(buf, got.wrapping_add(a).wrapping_sub(p)),
                R_68K_GOTPCREL16 => write16s(buf, got.wrapping_add(a).wrapping_sub(p)),
                R_68K_GOTPCREL8 => write8s(buf, got.wrapping_add(a).wrapping_sub(p)),
                R_68K_GOTOFF32 => write32(buf, g().wrapping_add(a)),
                R_68K_GOTOFF16 => write16(buf, g().wrapping_add(a)),
                R_68K_GOTOFF8 => write8(buf, g().wrapping_add(a)),
                R_68K_TLS_GD32 => {
                    write32(buf, sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got))
                }
                R_68K_TLS_GD16 => {
                    write16(buf, sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got))
                }
                R_68K_TLS_GD8 => write8(buf, sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got)),
                R_68K_TLS_LDM32 => write32(
                    buf,
                    ctx.got
                        .tlsld_addr::<Self>()
                        .wrapping_add(a)
                        .wrapping_sub(got),
                ),
                R_68K_TLS_LDM16 => write16(
                    buf,
                    ctx.got
                        .tlsld_addr::<Self>()
                        .wrapping_add(a)
                        .wrapping_sub(got),
                ),
                R_68K_TLS_LDM8 => write8(
                    buf,
                    ctx.got
                        .tlsld_addr::<Self>()
                        .wrapping_add(a)
                        .wrapping_sub(got),
                ),
                R_68K_TLS_LDO32 => write32(buf, sa.wrapping_sub(ctx.dtp_addr)),
                R_68K_TLS_LDO16 => write16s(buf, sa.wrapping_sub(ctx.dtp_addr)),
                R_68K_TLS_LDO8 => write8s(buf, sa.wrapping_sub(ctx.dtp_addr)),
                R_68K_TLS_IE32 => {
                    write32(buf, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got))
                }
                R_68K_TLS_IE16 => {
                    write16(buf, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got))
                }
                R_68K_TLS_IE8 => write8(buf, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got)),
                R_68K_TLS_LE32 => write32(buf, sa.wrapping_sub(ctx.tp_addr)),
                R_68K_TLS_LE16 => write16(buf, sa.wrapping_sub(ctx.tp_addr)),
                R_68K_TLS_LE8 => write8(buf, sa.wrapping_sub(ctx.tp_addr)),
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
            let frag = isec.fragment(ctx, &rel);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), rel.r_addend as u64),
            };
            let sa = s.wrapping_add(a);
            let tombstone = isec.tombstone(ctx, sym, frag.map(|(f, _)| f));
            let loc = &mut buf[rel.r_offset as usize..];

            match rel.r_type {
                R_68K_32 => w32(loc, tombstone.unwrap_or(sa) as u32),
                R_68K_TLS_LDO32 => w32(
                    loc,
                    tombstone.unwrap_or(sa.wrapping_sub(ctx.dtp_addr)) as u32,
                ),
                _ => fatal!(
                    ctx,
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }
}
