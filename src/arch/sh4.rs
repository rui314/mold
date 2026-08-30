//! SH-4 (SuperH 4), a 32-bit RISC ISA from Hitachi, best known from
//! Sega's Dreamcast and still sold by Renesas for embedded systems.
//!
//! Its instructions are 16 bits long for code density, which shapes the
//! ISA: 16 general-purpose registers, two-operand arithmetic, large
//! immediates loaded from memory with PC-relative `mov.l` rather than
//! built with load-high/load-low pairs, and few enough opcodes that the
//! set of relocations a linker has to support is small. Like MIPS and
//! SPARC of the same era, it has a branch delay slot.
//!
//! Notes on the psABI:
//!
//! - Position-independent functions start by loading the address of the
//!   GOT into `r12`, which the PLT relies on for position-independent
//!   output.
//! - The relocations are of the RELA type, yet object files store addends
//!   in the relocated section contents. Dynamic relocations follow the
//!   usual RELA convention.
//! - GCC tends to put dynamically relocated data in `.text`, so outputs
//!   contain plenty of text relocations.
//! - The ecosystem has bit-rotted; some programs using C++ exceptions
//!   don't work even when linked with GNU ld.

use std::marker::PhantomData;
use std::sync::atomic::Ordering;

use crate::arch::{Arch, Family};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{check_tlsle, scan_pcrel, InputSection};
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct Sh4Target<End>(PhantomData<End>);

pub type Sh4 = Sh4Target<LittleEndian>;
pub type Sh4Be = Sh4Target<BigEndian>;

impl<End: Endian> Layout for Sh4Target<End> {
    type Endian = End;
    const IS_64: bool = false;
    const IS_RELA: bool = true;
}

/// Whether the relocation's addend lives in the relocated word.
fn addend_in_place(r_type: u32) -> bool {
    matches!(
        r_type,
        R_SH_DIR32
            | R_SH_REL32
            | R_SH_TLS_GD_32
            | R_SH_TLS_LD_32
            | R_SH_TLS_LDO_32
            | R_SH_TLS_IE_32
            | R_SH_TLS_LE_32
            | R_SH_TLS_DTPMOD32
            | R_SH_TLS_DTPOFF32
            | R_SH_TLS_TPOFF32
            | R_SH_GOT32
            | R_SH_PLT32
            | R_SH_GOTOFF
            | R_SH_GOTPC
            | R_SH_GOTPLT32
    )
}

impl<End: Endian> Sh4Target<End> {
    fn write_insns(buf: &mut [u8], insns: &[u16]) {
        for (i, &insn) in insns.iter().enumerate() {
            End::write_u16(&mut buf[i * 2..], insn);
        }
    }
}

impl<End: Endian> Arch for Sh4Target<End> {
    const NAME: &'static str = if End::IS_LITTLE { "sh4" } else { "sh4be" };
    const FAMILY: Family = Family::Sh4;
    const PAGE_SIZE: u64 = 4096;
    const E_MACHINE: u32 = EM_SH;
    const PLT_HDR_SIZE: u64 = 16;
    const PLT_SIZE: u64 = 20;
    const PLTGOT_SIZE: u64 = 12;
    const TRAP: &'static [u8] = if End::IS_LITTLE {
        &[0xfd, 0xff]
    } else {
        &[0xff, 0xfd]
    }; // illegal

    const R_COPY: u32 = R_SH_COPY;
    const R_GLOB_DAT: u32 = R_SH_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_SH_JMP_SLOT;
    const R_ABS: u32 = R_SH_DIR32;
    const R_RELATIVE: u32 = R_SH_RELATIVE;
    const R_DTPOFF: u32 = R_SH_TLS_DTPOFF32;
    const R_TPOFF: u32 = R_SH_TLS_TPOFF32;
    const R_DTPMOD: u32 = R_SH_TLS_DTPMOD32;
    const R_FUNCALL: &'static [u32] = &[R_SH_PLT32];

    fn rel_to_string(r_type: u32) -> String {
        sh4_rel_to_string(r_type)
    }

    fn get_addend(loc: &[u8], rel: &ElfRel) -> i64 {
        if addend_in_place(rel.r_type) {
            End::read_u32(loc) as i32 as i64
        } else {
            0
        }
    }

    fn write_addend(loc: &mut [u8], val: i64, rel: &ElfRel) {
        if addend_in_place(rel.r_type) {
            End::write_u32(loc, val as u32);
        }
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        if ctx.args.pic {
            const INSN: [u16; 6] = [
                0xd202, //    mov.l   1f, r2
                0x32cc, //    add     r12, r2
                0x5022, //    mov.l   @(8, r2), r0
                0x5221, //    mov.l   @(4, r2), r2
                0x402b, //    jmp     @r0
                0xe000, //    mov     #0, r0
            ]; // 1: .long GOTPLT
            Self::write_insns(buf, &INSN);
            End::write_u32(
                &mut buf[12..],
                ctx.gotplt
                    .hdr
                    .shdr
                    .sh_addr
                    .wrapping_sub(ctx.got.hdr.shdr.sh_addr) as u32,
            );
        } else {
            const INSN: [u16; 6] = [
                0xd202, //    mov.l   1f, r2
                0x5022, //    mov.l   @(8, r2), r0
                0x5221, //    mov.l   @(4, r2), r2
                0x402b, //    jmp     @r0
                0xe000, //    mov     #0, r0
                0xfffd, //    (illegal)
            ]; // 1: .long GOTPLT
            Self::write_insns(buf, &INSN);
            End::write_u32(&mut buf[12..], ctx.gotplt.hdr.shdr.sh_addr as u32);
        }
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let gotplt = sym.gotplt_addr(ctx);
        if ctx.args.pic {
            const INSN: [u16; 6] = [
                0xd002, //    mov.l   1f, r0
                0x00ce, //    mov.l   @(r0, r12), r0
                0xd102, //    mov.l   2f, r1
                0x402b, //    jmp     @r0
                0x0009, //    nop
                0x0009, //    nop
            ]; // 1: .long GOTPLT_ENTRY; 2: .long INDEX_IN_RELPLT
            Self::write_insns(buf, &INSN);
            End::write_u32(
                &mut buf[12..],
                gotplt.wrapping_sub(ctx.got.hdr.shdr.sh_addr) as u32,
            );
        } else {
            const INSN: [u16; 6] = [
                0xd002, //    mov.l   1f, r0
                0x6002, //    mov.l   @r0, r0
                0xd102, //    mov.l   2f, r1
                0x402b, //    jmp     @r0
                0x0009, //    nop
                0x0009, //    nop
            ]; // 1: .long GOTPLT_ENTRY; 2: .long INDEX_IN_RELPLT
            Self::write_insns(buf, &INSN);
            End::write_u32(&mut buf[12..], gotplt as u32);
        }
        End::write_u32(
            &mut buf[16..],
            sym.plt_idx(&ctx.symbols).unwrap() * ElfRel::size::<Self>() as u32,
        );
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let got = sym.got_pltgot_addr(ctx);
        if ctx.args.pic {
            const INSN: [u16; 4] = [
                0xd001, //    mov.l   1f, r0
                0x00ce, //    mov.l   @(r0, r12), r0
                0x402b, //    jmp     @r0
                0x0009, //    nop
            ]; // 1: .long GOT_ENTRY
            Self::write_insns(buf, &INSN);
            End::write_u32(
                &mut buf[8..],
                got.wrapping_sub(ctx.got.hdr.shdr.sh_addr) as u32,
            );
        } else {
            const INSN: [u16; 4] = [
                0xd001, //    mov.l   1f, r0
                0x6002, //    mov.l   @r0, r0
                0x402b, //    jmp     @r0
                0x0009, //    nop
            ]; // 1: .long GOT_ENTRY
            Self::write_insns(buf, &INSN);
            End::write_u32(&mut buf[8..], got as u32);
        }
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
            R_SH_DIR32 => End::write_u32(loc, val as u32),
            R_SH_REL32 => End::write_u32(loc, val.wrapping_sub(p) as u32),
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
                error!(ctx, "{sym}: GNU ifunc symbol is not supported on sh4");
            }

            match rel.r_type {
                R_SH_REL32 => scan_pcrel(ctx, isec, sym, &rel),
                R_SH_GOT32 => sym.add_flags(NEEDS_GOT),
                R_SH_PLT32 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_SH_TLS_GD_32 => sym.add_flags(NEEDS_TLSGD),
                R_SH_TLS_LD_32 => ctx.needs_tlsld.store(true, Ordering::Relaxed),
                R_SH_TLS_IE_32 => sym.add_flags(NEEDS_GOTTP),
                R_SH_TLS_LE_32 => check_tlsle(ctx, isec, sym, &rel),
                R_SH_DIR32 | R_SH_GOTPC | R_SH_GOTOFF | R_SH_TLS_LDO_32 => {}
                _ => fatal!(
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

        for rel in isec.rels::<Self>(file) {
            if rel.r_type == R_NONE {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let s = sym.addr(ctx);
            let a = isec.rel_addend::<Self>(&rel) as u64;
            let p = isec.addr(ctx) + rel.r_offset;
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let loc = &mut buf[rel.r_offset as usize..];

            let val = match rel.r_type {
                // Handled as an absolute relocation by the output section.
                R_SH_DIR32 => continue,
                R_SH_REL32 | R_SH_PLT32 => sa.wrapping_sub(p),
                R_SH_GOT32 => g(),
                R_SH_GOTPC => got.wrapping_add(a).wrapping_sub(p),
                R_SH_GOTOFF => sa.wrapping_sub(got),
                R_SH_TLS_GD_32 => sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got),
                R_SH_TLS_LD_32 => ctx
                    .got
                    .tlsld_addr::<Self>()
                    .wrapping_add(a)
                    .wrapping_sub(got),
                R_SH_TLS_LDO_32 => sa.wrapping_sub(ctx.dtp_addr),
                R_SH_TLS_IE_32 => sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got),
                R_SH_TLS_LE_32 => sa.wrapping_sub(ctx.tp_addr),
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            };
            End::write_u32(loc, val as u32);
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
                None => (sym.addr(ctx), isec.rel_addend::<Self>(&rel) as u64),
            };
            let sa = s.wrapping_add(a);
            let tombstone = isec.tombstone(ctx, sym, frag.map(|(f, _)| f));
            let loc = &mut buf[rel.r_offset as usize..];

            match rel.r_type {
                R_SH_DIR32 => End::write_u32(loc, tombstone.unwrap_or(sa) as u32),
                R_SH_TLS_LDO_32 => End::write_u32(
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
