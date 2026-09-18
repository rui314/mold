//! This file contains code for the IBM z/Architecture 64-bit ISA, which is
//! commonly referred to as "s390x" on Linux.
//!
//! z/Architecture is a 64-bit CISC ISA developed by IBM around 2000 for
//! IBM's "big iron" mainframe computers. The computers are direct
//! descendents of IBM System/360 all the way back in 1966. I've never
//! actually seen a mainframe, and you probaly haven't either, but it looks
//! like the mainframe market is still large enough to sustain its ecosystem.
//! Ubuntu for example provides the official support for s390x as of 2022.
//! Since they are being actively maintained, we need to support them.
//!
//! As an instruction set, s390x isn't particularly odd. It has 16 general-
//! purpose registers. Instructions are 2, 4 or 6 bytes long and always
//! aligned to 2 bytes boundaries. Despite unfamiliarty, I found that it
//! just feels like an x86-64 in a parallel universe.
//!
//! Here is the register usage in this ABI:
//!
//!   r0-r1: reserved as scratch registers so we can use them in our PLT
//!   r2:    parameter passing and return values
//!   r3-r6: parameter passing
//!   r12:   address of GOT if position-independent code
//!   r14:   return address
//!   r15:   stack pointer
//!   a1:    upper 32 bits of TP (thread pointer)
//!   a2:    lower 32 bits of TP (thread pointer)
//!
//! Thread-local storage (TLS) is supported on s390x in the same way as it
//! is on other targets with one exeption. On other targets, __tls_get_addr
//! is used to get an address of a thread-local variable. On s390x,
//! __tls_get_offset is used instead. The difference is __tls_get_offset
//! returns an address of a thread-local variable as an offset from TP. So
//! we need to add TP to a return value before use. I don't know why it is
//! different, but that is the way it is.
//!
//! https://github.com/IBM/s390x-abi/releases/download/v1.6.1/lzsabi_s390x.pdf

use crate::arch::{Arch, Family};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::NonAllocReloc;
use crate::input_sections::{check_tlsle, scan_absrel, scan_pcrel, InputSection};
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::util::endian::{
    read_ub16, read_ub32, write_ub16, write_ub32, write_ub64, BigEndian, Ub64,
};
use crate::util::{bits, is_int};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct S390x;

impl Layout for S390x {
    type Endian = BigEndian;
    type Word = Ub64;
    type Sym = Elf64Sym<BigEndian>;
    type Phdr = Elf64Phdr<BigEndian>;
    type Chdr = Elf64Chdr<BigEndian>;
    type Rel = Elf64RelaBe;
}

/// Sets the 12-bit displacement field of a halfword.
fn or12(loc: &mut [u8], val: u64) {
    write_ub16(loc, read_ub16(loc) | bits(val, 11, 0) as u16);
}

/// Writes a 20-bit displacement, which is split into a 12-bit low part
/// and an 8-bit high part.
fn write_mid20(loc: &mut [u8], val: u64) {
    write_ub32(loc, read_ub32(loc) | ((bits(val, 11, 0) << 16) | (bits(val, 19, 12) << 8)) as u32);
}

/// Whether the GOT-loading LGRL at `loc` (opcode 0xc4?8, preceded by
/// the relocated operand) can become an address-materializing LARL.
fn relaxes_gotent(
    ctx: &Context<S390x>,
    isec: &InputSection<S390x>,
    rel: &ElfRel<S390x>,
    sym: &Symbol,
) -> bool {
    if !ctx.args.relax || !sym.is_pcrel_linktime_const(ctx) || rel.r_offset() < 2 {
        return false;
    }
    let op = read_ub16(&isec.contents()[rel.r_offset() as usize - 2..]);
    let val = sym
        .addr(ctx)
        .wrapping_add(rel.r_addend() as u64)
        .wrapping_sub(isec.addr(ctx).wrapping_add(rel.r_offset())) as i64;
    op & 0xff0f == 0xc408 && rel.r_addend() == 2 && val & 1 == 0 && is_int(val, 33)
}

impl Arch for S390x {
    type InputSectionExtra = ();

    const NAME: &'static str = "s390x";
    const FAMILY: Family = Family::S390x;
    const PAGE_SIZE: u64 = 4096;
    const E_MACHINE: u32 = EM_S390X;
    const PLT_HDR_SIZE: u64 = 48;
    const PLT_SIZE: u64 = 16;
    const PLTGOT_SIZE: u64 = 16;
    const SFRAME_ABI: Option<u8> = Some(SFRAME_ABI_S390X_ENDIAN_BIG);
    const TRAP: &'static [u8] = &[0x00, 0x00]; // invalid

    const R_COPY: u32 = R_390_COPY;
    const R_GLOB_DAT: u32 = R_390_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_390_JMP_SLOT;
    const R_ABS: u32 = R_390_64;
    const R_RELATIVE: u32 = R_390_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_390_IRELATIVE);
    const R_DTPOFF: u32 = R_390_TLS_DTPOFF;
    const R_TPOFF: u32 = R_390_TLS_TPOFF;
    const R_DTPMOD: u32 = R_390_TLS_DTPMOD;
    const R_SFRAME: Option<u32> = Some(R_390_PC64);
    const R_FUNCALL: &'static [u32] = &[R_390_PLT32DBL];

    fn rel_to_string(r_type: u32) -> std::borrow::Cow<'static, str> {
        s390x_rel_to_string(r_type)
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u8; 48] = [
            // Compute the offset into .rela.plt. This is equivalent to
            // (%r0 - %r1 - 48 - 14) * 3/2 where %r0 is the PLT entry address
            // plus 14, %r1 is the start address of .plt, and 48 is the size
            // of this PLT header. We multiply by 3/2 because each PLT entry
            // is 16 bytes, whereas each .rela.plt entry is 24 bytes.
            0xb9, 0x09, 0x00, 0x01, // sgr   %r0, %r1
            0xa7, 0x0b, 0xff, 0xc2, // aghi  %r0, -62
            0xeb, 0x10, 0x00, 0x01, 0x00, 0x0c, // srlg  %r1, %r0, 1
            0xb9, 0x08, 0x00, 0x01, // agr   %r0, %r1
            // agr   %r0, %r1
            // Store the computed value to 56(%r15) and .got.plt[1] to 48(%15)
            // where %r15 is the stack pointer.
            0xe3, 0x00, 0xf0, 0x38, 0x00, 0x24, // stg   %r0, 56(%r15)
            0xc0, 0x10, 0, 0, 0, 0, // larl  %r1, GOTPLT_OFFSET
            0xd2, 0x07, 0xf0, 0x30, 0x10, 0x08, // mvc   48(8, %r15), 8(%r1)
            // Branch to _dl_runtime_resolve.
            0xe3, 0x10, 0x10, 0x10, 0x00, 0x04, // lg    %r1, 16(%r1)
            0x07, 0xf1, // br    %r1
            0x00, 0x00, 0x00, 0x00, // (filler)
        ];
        buf[..48].copy_from_slice(&INSN);
        let gotplt = ctx.gotplt.shdr.sh_addr.get();
        let plt = ctx.plt.hdr.shdr.sh_addr.get();
        let offset = gotplt.wrapping_sub(plt).wrapping_sub(24);
        write_ub32(&mut buf[26..], (offset >> 1) as u32);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u8; 16] = [
            0xc0, 0x10, 0, 0, 0, 0, // larl  %r1, GOTPLT_ENTRY_OFFSET
            0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, (%r1)
            0x0d, 0x01, // basr  %r0, %r1
            0x00, 0x00, // (filler)
        ];
        buf[..16].copy_from_slice(&INSN);
        write_ub32(
            &mut buf[2..],
            (sym.gotplt_addr(ctx).wrapping_sub(sym.plt_addr(ctx)) >> 1) as u32,
        );
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u8; 16] = [
            0xc0, 0x10, 0, 0, 0, 0, // larl  %r1, GOT_ENTRY_OFFSET
            0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, (%r1)
            0x07, 0xf1, // br    %r1
            0x00, 0x00, // (filler)
        ];
        buf[..16].copy_from_slice(&INSN);
        write_ub32(
            &mut buf[2..],
            (sym.got_pltgot_addr(ctx).wrapping_sub(sym.plt_addr(ctx)) >> 1) as u32,
        );
    }

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        isec: &InputSection<S390x>,
        rel: &Self::Rel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        let check = |val: i64, lo: i64, hi: i64| eh_frame::check_range(ctx, isec, rel, val, lo, hi);
        match rel.r_type() {
            R_NONE => {}
            R_390_PC32 => {
                check(val.wrapping_sub(p) as i64, -(1 << 31), 1 << 31);
                write_ub32(loc, val.wrapping_sub(p) as u32);
            }
            R_390_64 => write_ub64(loc, val),
            _ => eh_frame::unsupported::<Self>(rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection<S390x>) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        // Scan relocations
        for rel in isec.rels(file) {
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            match rel.r_type() {
                R_390_8 | R_390_12 | R_390_16 | R_390_20 | R_390_32 => {
                    scan_absrel(ctx, isec, sym, rel)
                }
                R_390_PC12DBL | R_390_PC16 | R_390_PC16DBL | R_390_PC24DBL | R_390_PC32
                | R_390_PC32DBL | R_390_PC64 => scan_pcrel(ctx, isec, sym, rel),
                R_390_GOT12 | R_390_GOT16 | R_390_GOT20 | R_390_GOT32 | R_390_GOT64
                | R_390_GOTOFF16 | R_390_GOTOFF32 | R_390_GOTOFF64 | R_390_GOTPLT12
                | R_390_GOTPLT16 | R_390_GOTPLT20 | R_390_GOTPLT32 | R_390_GOTPLT64
                | R_390_GOTPC | R_390_GOTPCDBL | R_390_GOTENT => sym.add_flags(NEEDS_GOT),
                R_390_PLT12DBL | R_390_PLT16DBL | R_390_PLT24DBL | R_390_PLT32 | R_390_PLT32DBL
                | R_390_PLT64 | R_390_PLTOFF16 | R_390_PLTOFF32 | R_390_PLTOFF64 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_390_TLS_GOTIE20 | R_390_TLS_IEENT => sym.add_flags(NEEDS_GOTTP),
                R_390_TLS_GD32 | R_390_TLS_GD64 => {
                    // We always want to relax calls to __tls_get_offset() in statically-
                    // linked executables because __tls_get_offset() in libc.a just calls
                    // abort().
                    if ctx.args.is_static || (ctx.args.relax && sym.is_tprel_linktime_const(ctx)) {
                        // Do nothing
                    } else if ctx.args.relax && sym.is_tprel_runtime_const(ctx) {
                        sym.add_flags(NEEDS_GOTTP);
                    } else {
                        sym.add_flags(NEEDS_TLSGD);
                    }
                }
                R_390_TLS_LDM32 | R_390_TLS_LDM64 => {
                    if ctx.args.is_static || (ctx.args.relax && !ctx.args.shared) {
                        // Do nothing
                    } else {
                        ctx.needs_tlsld.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                R_390_TLS_LE32 | R_390_TLS_LE64 => check_tlsle(ctx, isec, sym, rel),
                R_390_64 | R_390_TLS_LDO32 | R_390_TLS_LDO64 | R_390_TLS_GDCALL
                | R_390_TLS_LDCALL => {}
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
        isec: &InputSection<S390x>,
        rels: &mut [Self::Rel],
        buf: &mut [u8],
    ) {
        let file = &ctx.objs[isec.file.index()];
        for (i, rel_mut) in rels.iter_mut().enumerate() {
            let rel = *rel_mut;
            if rel.r_type() == R_NONE {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let off = rel.r_offset() as usize;
            let s = sym.addr(ctx);
            let a = rel.r_addend() as u64;
            let p = isec.addr(ctx) + rel.r_offset();
            let got = ctx.got.hdr.shdr.sh_addr.get();
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);
            // R_390_*DBL relocs should never refer to a symbol at an odd address
            let check_dbl = |val: i64, lo: i64, hi: i64| {
                check(val, lo, hi);
                if val & 1 != 0 {
                    error!(
                        "{}: misaligned symbol {sym} for relocation {}",
                        isec.display(file),
                        rel.type_name::<Self>()
                    );
                }
            };

            match rel.r_type() {
                // Handled as an absolute relocation by the output section.
                R_390_64 => {}
                R_390_8 => {
                    check(sa as i64, 0, 1 << 8);
                    buf[off] = sa as u8;
                }
                R_390_12 => {
                    check(sa as i64, 0, 1 << 12);
                    or12(&mut buf[off..], sa);
                }
                R_390_16 => {
                    check(sa as i64, 0, 1 << 16);
                    write_ub16(&mut buf[off..], sa as u16);
                }
                R_390_20 => {
                    check(sa as i64, 0, 1 << 20);
                    write_mid20(&mut buf[off..], sa);
                }
                R_390_32 | R_390_PLT32 => {
                    check(sa as i64, 0, 1 << 32);
                    write_ub32(&mut buf[off..], sa as u32);
                }
                R_390_PC12DBL | R_390_PLT12DBL => {
                    check_dbl(pcrel as i64, -(1 << 12), 1 << 12);
                    let v = read_ub16(&buf[off..]) | bits(pcrel, 12, 1) as u16;
                    write_ub16(&mut buf[off..], v);
                }
                R_390_PC16 => {
                    check(pcrel as i64, -(1 << 15), 1 << 15);
                    write_ub16(&mut buf[off..], pcrel as u16);
                }
                R_390_PC32 => {
                    check(pcrel as i64, -(1 << 31), 1 << 31);
                    write_ub32(&mut buf[off..], pcrel as u32);
                }
                R_390_PC64 | R_390_PLT64 => write_ub64(&mut buf[off..], pcrel),
                R_390_PC16DBL | R_390_PLT16DBL => {
                    check_dbl(pcrel as i64, -(1 << 16), 1 << 16);
                    write_ub16(&mut buf[off..], (pcrel >> 1) as u16);
                }
                R_390_PC24DBL | R_390_PLT24DBL => {
                    check_dbl(pcrel as i64, -(1 << 24), 1 << 24);
                    let v = read_ub32(&buf[off..]) | bits(pcrel, 24, 1) as u32;
                    write_ub32(&mut buf[off..], v);
                }
                R_390_PC32DBL => {
                    check_dbl(pcrel as i64, -(1 << 32), 1 << 32);
                    write_ub32(&mut buf[off..], (pcrel >> 1) as u32);
                }
                R_390_PLT32DBL => {
                    if !sym.is_remaining_undef_weak() {
                        check_dbl(pcrel as i64, -(1 << 32), 1 << 32);
                    }
                    write_ub32(&mut buf[off..], (pcrel >> 1) as u32);
                }
                R_390_GOT12 | R_390_GOTPLT12 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 12);
                    or12(&mut buf[off..], g().wrapping_add(a));
                }
                R_390_GOT16 | R_390_GOTPLT16 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 16);
                    write_ub16(&mut buf[off..], g().wrapping_add(a) as u16);
                }
                R_390_GOT20 | R_390_GOTPLT20 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 20);
                    write_mid20(&mut buf[off..], g().wrapping_add(a));
                }
                R_390_GOT32 | R_390_GOTPLT32 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 32);
                    write_ub32(&mut buf[off..], g().wrapping_add(a) as u32);
                }
                R_390_GOT64 | R_390_GOTPLT64 => write_ub64(&mut buf[off..], g().wrapping_add(a)),
                R_390_GOTOFF16 | R_390_PLTOFF16 => {
                    check(sa.wrapping_sub(got) as i64, -(1 << 15), 1 << 15);
                    write_ub16(&mut buf[off..], sa.wrapping_sub(got) as u16);
                }
                R_390_GOTOFF32 | R_390_PLTOFF32 => {
                    check(sa.wrapping_sub(got) as i64, -(1 << 31), 1 << 31);
                    write_ub32(&mut buf[off..], sa.wrapping_sub(got) as u32);
                }
                R_390_GOTOFF64 | R_390_PLTOFF64 => {
                    write_ub64(&mut buf[off..], sa.wrapping_sub(got))
                }
                R_390_GOTPC => write_ub64(&mut buf[off..], got.wrapping_add(a).wrapping_sub(p)),
                R_390_GOTPCDBL => {
                    let val = got.wrapping_add(a).wrapping_sub(p);
                    check_dbl(val as i64, -(1 << 32), 1 << 32);
                    write_ub32(&mut buf[off..], (val >> 1) as u32);
                }
                R_390_GOTENT => {
                    // If we can relax a GOT-loading LGRL to an address-materializing
                    // LARL, do that. The format of LGRL is 0xc 0x4 <reg> 0x8 followed
                    // by a 32-bit offset. LARL is 0xc 0x0 <reg> 0x0.
                    if relaxes_gotent(ctx, isec, &rel, sym) {
                        let op = read_ub16(&buf[off - 2..]);
                        write_ub16(&mut buf[off - 2..], 0xc000 | (op & 0x00f0));
                        write_ub32(&mut buf[off..], (pcrel >> 1) as u32);
                        if ctx.args.emit_relocs {
                            rel_mut.set_r_type(R_390_PC32DBL);
                        }
                    } else {
                        let val = got.wrapping_add(g()).wrapping_add(a).wrapping_sub(p);
                        check_dbl(val as i64, -(1 << 32), 1 << 32);
                        write_ub32(&mut buf[off..], (val >> 1) as u32);
                    }
                }
                R_390_TLS_LE32 => write_ub32(&mut buf[off..], sa.wrapping_sub(ctx.tp_addr) as u32),
                R_390_TLS_LE64 => write_ub64(&mut buf[off..], sa.wrapping_sub(ctx.tp_addr)),
                R_390_TLS_GOTIE20 => write_mid20(
                    &mut buf[off..],
                    sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got),
                ),
                R_390_TLS_IEENT => write_ub32(
                    &mut buf[off..],
                    (sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(p) >> 1) as u32,
                ),
                R_390_TLS_GD32 | R_390_TLS_GD64 => {
                    let val = if sym.has_tlsgd(&ctx.symbols) {
                        sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got)
                    } else if sym.has_gottp(&ctx.symbols) {
                        sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got)
                    } else {
                        sa.wrapping_sub(ctx.tp_addr)
                    };
                    if rel.r_type() == R_390_TLS_GD32 {
                        write_ub32(&mut buf[off..], val as u32);
                    } else {
                        write_ub64(&mut buf[off..], val);
                    }
                }
                R_390_TLS_GDCALL => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        // do nothing
                    } else if sym.has_gottp(&ctx.symbols) {
                        buf[off..off + 6].copy_from_slice(&[0xe3, 0x22, 0xc0, 0x00, 0x00, 0x04]);
                    // lg %r2, 0(%r2, %r12)
                    } else {
                        buf[off..off + 6].copy_from_slice(&[0xc0, 0x04, 0x00, 0x00, 0x00, 0x00]);
                        // nop
                    }
                }
                R_390_TLS_LDM32 | R_390_TLS_LDM64 => {
                    let val = if ctx.got.has_tlsld() {
                        ctx.got.tlsld_addr().wrapping_add(a).wrapping_sub(got)
                    } else {
                        ctx.dtp_addr.wrapping_sub(ctx.tp_addr)
                    };
                    if rel.r_type() == R_390_TLS_LDM32 {
                        write_ub32(&mut buf[off..], val as u32);
                    } else {
                        write_ub64(&mut buf[off..], val);
                    }
                }
                R_390_TLS_LDCALL => {
                    if !ctx.got.has_tlsld() {
                        buf[off..off + 6].copy_from_slice(&[0xc0, 0x04, 0x00, 0x00, 0x00, 0x00]);
                        // nop
                    }
                }
                R_390_TLS_LDO32 => {
                    write_ub32(&mut buf[off..], sa.wrapping_sub(ctx.dtp_addr) as u32)
                }
                R_390_TLS_LDO64 => write_ub64(&mut buf[off..], sa.wrapping_sub(ctx.dtp_addr)),
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection<S390x>, buf: &mut [u8]) {
        let mut fragment_cache = crate::input_sections::FragmentLookup::default();
        let file = &ctx.objs[isec.file.index()];
        for (i, rel) in isec.relocations(ctx).enumerate() {
            let Some(NonAllocReloc { sym, s, a, frag }) =
                isec.resolve_nonalloc(ctx, file, &rel, &mut fragment_cache)
            else {
                continue;
            };
            let off = rel.r_offset() as usize;
            let sa = s.wrapping_add(a);
            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);

            match rel.r_type() {
                R_390_32 => {
                    check(sa as i64, 0, 1 << 32);
                    write_ub32(&mut buf[off..], sa as u32);
                }
                R_390_64 => match isec.tombstone(ctx, sym, frag) {
                    Some(v) => write_ub64(&mut buf[off..], v),
                    None => write_ub64(&mut buf[off..], sa),
                },
                R_390_TLS_LDO64 => match isec.tombstone(ctx, sym, frag) {
                    Some(v) => write_ub64(&mut buf[off..], v),
                    None => write_ub64(&mut buf[off..], sa.wrapping_sub(ctx.dtp_addr)),
                },
                _ => fatal!(
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }
}
