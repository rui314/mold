//! s390x, IBM's 64-bit z/Architecture.
//!
//! z/Architecture is a big-endian CISC ISA with 16 general-purpose
//! registers and 2-, 4- or 6-byte instructions aligned to 2 bytes. From a
//! linker's point of view it feels like an x86-64 from a parallel
//! universe. `%r0` and `%r1` are scratch registers the PLT may use, `%r12`
//! holds the GOT address in position-independent code, `%r14` the return
//! address and `%r15` the stack pointer.
//!
//! Thread-local storage works as elsewhere except that `__tls_get_offset`
//! takes the place of `__tls_get_addr` and returns an offset from the
//! thread pointer rather than an address.
//!
//! https://github.com/IBM/s390x-abi/releases/download/v1.6.1/lzsabi_s390x.pdf

use crate::arch::{Arch, Family};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{check_tlsle, scan_absrel, scan_pcrel, InputSection};
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::util::{bits, is_int};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct S390x;

impl Layout for S390x {
    type Endian = BigEndian;
    const IS_64: bool = true;
    const IS_RELA: bool = true;
}

fn w16(loc: &mut [u8], v: u16) {
    BigEndian::write_u16(loc, v);
}

fn w32(loc: &mut [u8], v: u32) {
    BigEndian::write_u32(loc, v);
}

fn w64(loc: &mut [u8], v: u64) {
    BigEndian::write_u64(loc, v);
}

fn r16(loc: &[u8]) -> u16 {
    BigEndian::read_u16(loc)
}

fn r32(loc: &[u8]) -> u32 {
    BigEndian::read_u32(loc)
}

/// Sets the 12-bit displacement field of a halfword.
fn or12(loc: &mut [u8], val: u64) {
    w16(loc, r16(loc) | bits(val, 11, 0) as u16);
}

/// Writes a 20-bit displacement, which is split into a 12-bit low part
/// and an 8-bit high part.
fn write_mid20(loc: &mut [u8], val: u64) {
    w32(
        loc,
        r32(loc) | ((bits(val, 11, 0) << 16) | (bits(val, 19, 12) << 8)) as u32,
    );
}

/// Whether the GOT-loading LGRL at `loc` (opcode 0xc4?8, preceded by
/// the relocated operand) can become an address-materializing LARL.
fn relaxes_gotent(ctx: &Context<S390x>, isec: &InputSection, rel: &ElfRel, sym: &Symbol) -> bool {
    if !ctx.args.relax || !sym.is_pcrel_linktime_const(ctx) || rel.r_offset < 2 {
        return false;
    }
    let op = r16(&isec.contents()[rel.r_offset as usize - 2..]);
    let val = sym
        .addr(ctx)
        .wrapping_add(rel.r_addend as u64)
        .wrapping_sub(isec.addr(ctx) + rel.r_offset) as i64;
    op & 0xff0f == 0xc408 && rel.r_addend == 2 && val & 1 == 0 && is_int(val, 33)
}

impl Arch for S390x {
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

    fn rel_to_string(r_type: u32) -> String {
        s390x_rel_to_string(r_type)
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u8; 48] = [
            // The offset into .rela.plt is (%r0 - %r1 - 48 - 14) * 3/2,
            // where %r0 is the PLT entry address plus 14, %r1 the start of
            // .plt, and 48 the size of this header; PLT entries are 16
            // bytes and .rela.plt entries 24.
            0xb9, 0x09, 0x00, 0x01, // sgr   %r0, %r1
            0xa7, 0x0b, 0xff, 0xc2, // aghi  %r0, -62
            0xeb, 0x10, 0x00, 0x01, 0x00, 0x0c, // srlg  %r1, %r0, 1
            0xb9, 0x08, 0x00, 0x01, // agr   %r0, %r1
            // Store the result at 56(%r15) and .got.plt[1] at 48(%r15),
            // %r15 being the stack pointer.
            0xe3, 0x00, 0xf0, 0x38, 0x00, 0x24, // stg   %r0, 56(%r15)
            0xc0, 0x10, 0, 0, 0, 0, // larl  %r1, GOTPLT_OFFSET
            0xd2, 0x07, 0xf0, 0x30, 0x10, 0x08, // mvc   48(8, %r15), 8(%r1)
            // Branch to _dl_runtime_resolve.
            0xe3, 0x10, 0x10, 0x10, 0x00, 0x04, // lg    %r1, 16(%r1)
            0x07, 0xf1, // br    %r1
            0x00, 0x00, 0x00, 0x00, // (filler)
        ];
        buf[..48].copy_from_slice(&INSN);
        w32(
            &mut buf[26..],
            (ctx.gotplt
                .hdr
                .shdr
                .sh_addr
                .wrapping_sub(ctx.plt.hdr.shdr.sh_addr)
                .wrapping_sub(24)
                >> 1) as u32,
        );
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u8; 16] = [
            0xc0, 0x10, 0, 0, 0, 0, // larl  %r1, GOTPLT_ENTRY_OFFSET
            0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, (%r1)
            0x0d, 0x01, // basr  %r0, %r1
            0x00, 0x00, // (filler)
        ];
        buf[..16].copy_from_slice(&INSN);
        w32(
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
        w32(
            &mut buf[2..],
            (sym.got_pltgot_addr(ctx).wrapping_sub(sym.plt_addr(ctx)) >> 1) as u32,
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
            R_390_PC32 => {
                check(val.wrapping_sub(p) as i64, -(1 << 31), 1 << 31);
                w32(loc, val.wrapping_sub(p) as u32);
            }
            R_390_64 => w64(loc, val),
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
                R_390_8 | R_390_12 | R_390_16 | R_390_20 | R_390_32 => {
                    scan_absrel(ctx, isec, sym, &rel)
                }
                R_390_PC12DBL | R_390_PC16 | R_390_PC16DBL | R_390_PC24DBL | R_390_PC32
                | R_390_PC32DBL | R_390_PC64 => scan_pcrel(ctx, isec, sym, &rel),
                R_390_GOT12 | R_390_GOT16 | R_390_GOT20 | R_390_GOT32 | R_390_GOT64
                | R_390_GOTOFF16 | R_390_GOTOFF32 | R_390_GOTOFF64 | R_390_GOTPLT12
                | R_390_GOTPLT16 | R_390_GOTPLT20 | R_390_GOTPLT32 | R_390_GOTPLT64
                | R_390_GOTPC | R_390_GOTPCDBL => sym.add_flags(NEEDS_GOT),
                R_390_GOTENT => {
                    if !relaxes_gotent(ctx, isec, &rel, sym) {
                        sym.add_flags(NEEDS_GOT);
                    }
                }
                R_390_PLT12DBL | R_390_PLT16DBL | R_390_PLT24DBL | R_390_PLT32 | R_390_PLT32DBL
                | R_390_PLT64 | R_390_PLTOFF16 | R_390_PLTOFF32 | R_390_PLTOFF64 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_390_TLS_GOTIE20 | R_390_TLS_IEENT => sym.add_flags(NEEDS_GOTTP),
                R_390_TLS_GD32 | R_390_TLS_GD64 => {
                    // Calls to __tls_get_offset are always relaxed in a
                    // static executable, since libc.a's just aborts.
                    if ctx.args.is_static || (ctx.args.relax && sym.is_tprel_linktime_const(ctx)) {
                        // Nothing to do.
                    } else if ctx.args.relax && sym.is_tprel_runtime_const(ctx) {
                        sym.add_flags(NEEDS_GOTTP);
                    } else {
                        sym.add_flags(NEEDS_TLSGD);
                    }
                }
                R_390_TLS_LDM32 | R_390_TLS_LDM64 => {
                    if !(ctx.args.is_static || (ctx.args.relax && !ctx.args.shared)) {
                        ctx.needs_tlsld
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                R_390_TLS_LE32 | R_390_TLS_LE64 => check_tlsle(ctx, isec, sym, &rel),
                R_390_64 | R_390_TLS_LDO32 | R_390_TLS_LDO64 | R_390_TLS_GDCALL
                | R_390_TLS_LDCALL => {}
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
            let got = ctx.got.hdr.shdr.sh_addr;
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);
            // *DBL relocations must not refer to odd addresses.
            let check_dbl = |val: i64, lo: i64, hi: i64| {
                check(val, lo, hi);
                if val & 1 != 0 {
                    error!(
                        ctx,
                        "{}: misaligned symbol {sym} for relocation {}",
                        isec.display(file),
                        rel.type_name::<Self>()
                    );
                }
            };

            match rel.r_type {
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
                    w16(&mut buf[off..], sa as u16);
                }
                R_390_20 => {
                    check(sa as i64, 0, 1 << 20);
                    write_mid20(&mut buf[off..], sa);
                }
                R_390_32 | R_390_PLT32 => {
                    check(sa as i64, 0, 1 << 32);
                    w32(&mut buf[off..], sa as u32);
                }
                R_390_PC12DBL | R_390_PLT12DBL => {
                    check_dbl(pcrel as i64, -(1 << 12), 1 << 12);
                    let v = r16(&buf[off..]) | bits(pcrel, 12, 1) as u16;
                    w16(&mut buf[off..], v);
                }
                R_390_PC16 => {
                    check(pcrel as i64, -(1 << 15), 1 << 15);
                    w16(&mut buf[off..], pcrel as u16);
                }
                R_390_PC32 => {
                    check(pcrel as i64, -(1 << 31), 1 << 31);
                    w32(&mut buf[off..], pcrel as u32);
                }
                R_390_PC64 | R_390_PLT64 => w64(&mut buf[off..], pcrel),
                R_390_PC16DBL | R_390_PLT16DBL => {
                    check_dbl(pcrel as i64, -(1 << 16), 1 << 16);
                    w16(&mut buf[off..], (pcrel >> 1) as u16);
                }
                R_390_PC24DBL | R_390_PLT24DBL => {
                    check_dbl(pcrel as i64, -(1 << 24), 1 << 24);
                    let v = r32(&buf[off..]) | bits(pcrel, 24, 1) as u32;
                    w32(&mut buf[off..], v);
                }
                R_390_PC32DBL => {
                    check_dbl(pcrel as i64, -(1 << 32), 1 << 32);
                    w32(&mut buf[off..], (pcrel >> 1) as u32);
                }
                R_390_PLT32DBL => {
                    if !sym.is_remaining_undef_weak() {
                        check_dbl(pcrel as i64, -(1 << 32), 1 << 32);
                    }
                    w32(&mut buf[off..], (pcrel >> 1) as u32);
                }
                R_390_GOT12 | R_390_GOTPLT12 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 12);
                    or12(&mut buf[off..], g().wrapping_add(a));
                }
                R_390_GOT16 | R_390_GOTPLT16 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 16);
                    w16(&mut buf[off..], g().wrapping_add(a) as u16);
                }
                R_390_GOT20 | R_390_GOTPLT20 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 20);
                    write_mid20(&mut buf[off..], g().wrapping_add(a));
                }
                R_390_GOT32 | R_390_GOTPLT32 => {
                    check(g().wrapping_add(a) as i64, 0, 1 << 32);
                    w32(&mut buf[off..], g().wrapping_add(a) as u32);
                }
                R_390_GOT64 | R_390_GOTPLT64 => w64(&mut buf[off..], g().wrapping_add(a)),
                R_390_GOTOFF16 | R_390_PLTOFF16 => {
                    check(sa.wrapping_sub(got) as i64, -(1 << 15), 1 << 15);
                    w16(&mut buf[off..], sa.wrapping_sub(got) as u16);
                }
                R_390_GOTOFF32 | R_390_PLTOFF32 => {
                    check(sa.wrapping_sub(got) as i64, -(1 << 31), 1 << 31);
                    w32(&mut buf[off..], sa.wrapping_sub(got) as u32);
                }
                R_390_GOTOFF64 | R_390_PLTOFF64 => w64(&mut buf[off..], sa.wrapping_sub(got)),
                R_390_GOTPC => w64(&mut buf[off..], got.wrapping_add(a).wrapping_sub(p)),
                R_390_GOTPCDBL => {
                    let val = got.wrapping_add(a).wrapping_sub(p);
                    check_dbl(val as i64, -(1 << 32), 1 << 32);
                    w32(&mut buf[off..], (val >> 1) as u32);
                }
                R_390_GOTENT => {
                    // A GOT-loading LGRL (0xc4?8 followed by a 32-bit offset)
                    // becomes an address-materializing LARL (0xc0?0) if the
                    // address is a link-time constant.
                    if relaxes_gotent(ctx, isec, &rel, sym) {
                        let op = r16(&buf[off - 2..]);
                        w16(&mut buf[off - 2..], 0xc000 | (op & 0x00f0));
                        w32(&mut buf[off..], (pcrel >> 1) as u32);
                    } else {
                        let val = got.wrapping_add(g()).wrapping_add(a).wrapping_sub(p);
                        check_dbl(val as i64, -(1 << 32), 1 << 32);
                        w32(&mut buf[off..], (val >> 1) as u32);
                    }
                }
                R_390_TLS_LE32 => w32(&mut buf[off..], sa.wrapping_sub(ctx.tp_addr) as u32),
                R_390_TLS_LE64 => w64(&mut buf[off..], sa.wrapping_sub(ctx.tp_addr)),
                R_390_TLS_GOTIE20 => write_mid20(
                    &mut buf[off..],
                    sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got),
                ),
                R_390_TLS_IEENT => w32(
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
                    if rel.r_type == R_390_TLS_GD32 {
                        w32(&mut buf[off..], val as u32);
                    } else {
                        w64(&mut buf[off..], val);
                    }
                }
                R_390_TLS_GDCALL => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        // Nothing to do.
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
                        ctx.got
                            .tlsld_addr::<Self>()
                            .wrapping_add(a)
                            .wrapping_sub(got)
                    } else {
                        ctx.dtp_addr.wrapping_sub(ctx.tp_addr)
                    };
                    if rel.r_type == R_390_TLS_LDM32 {
                        w32(&mut buf[off..], val as u32);
                    } else {
                        w64(&mut buf[off..], val);
                    }
                }
                R_390_TLS_LDCALL => {
                    if !ctx.got.has_tlsld() {
                        buf[off..off + 6].copy_from_slice(&[0xc0, 0x04, 0x00, 0x00, 0x00, 0x00]);
                        // nop
                    }
                }
                R_390_TLS_LDO32 => w32(&mut buf[off..], sa.wrapping_sub(ctx.dtp_addr) as u32),
                R_390_TLS_LDO64 => w64(&mut buf[off..], sa.wrapping_sub(ctx.dtp_addr)),
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
            let off = rel.r_offset as usize;
            let frag = isec.fragment(ctx, &rel);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), rel.r_addend as u64),
            };
            let frag_ref = frag.map(|(f, _)| f);
            let sa = s.wrapping_add(a);
            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);

            match rel.r_type {
                R_390_32 => {
                    check(sa as i64, 0, 1 << 32);
                    w32(&mut buf[off..], sa as u32);
                }
                R_390_64 => match isec.tombstone(ctx, sym, frag_ref) {
                    Some(v) => w64(&mut buf[off..], v),
                    None => w64(&mut buf[off..], sa),
                },
                R_390_TLS_LDO64 => match isec.tombstone(ctx, sym, frag_ref) {
                    Some(v) => w64(&mut buf[off..], v),
                    None => w64(&mut buf[off..], sa.wrapping_sub(ctx.dtp_addr)),
                },
                _ => fatal!(
                    ctx,
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        });
    }

    fn emitted_rel_type(ctx: &Context<Self>, isec: &InputSection, rel: &ElfRel, _i: usize) -> u32 {
        if rel.r_type == R_390_GOTENT && isec.is_alloc() {
            let file = &ctx.objs[isec.file.index()];
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if relaxes_gotent(ctx, isec, rel, sym) {
                return R_390_PC32DBL;
            }
        }
        rel.r_type
    }
}
