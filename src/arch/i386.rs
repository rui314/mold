//! i386 is similar to x86-64 but lacks PC-relative memory access
//! instructions. So it's not straightforward to support position-
//! independent code (PIC) on that target.
//!
//! If an object file is compiled with -fPIC, a function that needs to load
//! a value from memory first obtains its own address with the following
//! code
//!
//!   call __x86.get_pc_thunk.bx
//!
//! where __x86.get_pc_thunk.bx is defined as
//!
//!   __x86.get_pc_thunk.bx:
//!     mov (%esp), %ebx  # move the return address to %ebx
//!     ret
//!
//! . With the function's own address (or, more precisely, the address
//! immediately after the call instruction), the function can compute an
//! absolute address of a variable with its address + link-time constant.
//!
//! Executing call-mov-ret isn't very cheap, and allocating one register to
//! store PC isn't cheap too, especially given that i386 has only 8
//! general-purpose registers. But that's the cost of PIC on i386. You need
//! to pay it when creating a .so and a position-independent executable.
//!
//! When a position-independent function calls another function, it sets
//! %ebx to the address of .got. Position-independent PLT entries use that
//! register to load values from .got.plt/.got.
//!
//! If we are creating a position-dependent executable (PDE), we can't
//! assume that %ebx is set to .got. For PDE, we need to create position-
//! dependent PLT entries which don't use %ebx.
//!
//! https://github.com/rui314/psabi/blob/main/i386.pdf
//!
//! Relocations are of the REL type: addends live in the relocated
//! locations rather than in the relocation entries.

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{check_tlsle, scan_absrel, scan_pcrel, scan_tlsdesc, InputSection};
use crate::output_chunks::eh_frame;
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct I386;

impl Layout for I386 {
    type Endian = LittleEndian;
    type Word = Ul32;
    type Sym = Elf32Sym<LittleEndian>;
    type Phdr = Elf32Phdr<LittleEndian>;
    type Chdr = Elf32Chdr<LittleEndian>;
    type Rel = Elf32RelLe;
}

impl Arch for I386 {
    const NAME: &'static str = "i386";
    const FAMILY: Family = Family::I386;
    const PAGE_SIZE: u64 = 4096;
    const E_MACHINE: u32 = EM_386;
    const PLT_HDR_SIZE: u64 = 16;
    const PLT_SIZE: u64 = 16;
    const PLTGOT_SIZE: u64 = 8;
    const TRAP: &'static [u8] = &[0xcc]; // int3

    const R_COPY: u32 = R_386_COPY;
    const R_GLOB_DAT: u32 = R_386_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_386_JUMP_SLOT;
    const R_ABS: u32 = R_386_32;
    const R_RELATIVE: u32 = R_386_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_386_IRELATIVE);
    const R_DTPOFF: u32 = R_386_TLS_DTPOFF32;
    const R_TPOFF: u32 = R_386_TLS_TPOFF;
    const R_DTPMOD: u32 = R_386_TLS_DTPMOD32;
    const R_TLSDESC: Option<u32> = Some(R_386_TLS_DESC);
    const R_FUNCALL: &'static [u32] = &[R_386_PLT32];

    fn rel_to_string(r_type: u32) -> String {
        i386_rel_to_string(r_type)
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        let gotplt = u64::from(ctx.gotplt.hdr.shdr.sh_addr.get());
        if ctx.args.pic {
            const INSN: [u8; 16] = [
                0x51, // push %ecx
                0x8d, 0x8b, 0, 0, 0, 0, // lea GOTPLT+4(%ebx), %ecx
                0xff, 0x31, // push (%ecx)
                0xff, 0x61, 0x04, // jmp *0x4(%ecx)
                0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            ];
            buf[..16].copy_from_slice(&INSN);
            write_u32(
                &mut buf[3..],
                gotplt
                    .wrapping_sub(u64::from(ctx.got.hdr.shdr.sh_addr.get()))
                    .wrapping_add(4) as u32,
            );
        } else {
            const INSN: [u8; 16] = [
                0x51, // push %ecx
                0xb9, 0, 0, 0, 0, // mov GOTPLT+4, %ecx
                0xff, 0x31, // push (%ecx)
                0xff, 0x61, 0x04, // jmp *0x4(%ecx)
                0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            ];
            buf[..16].copy_from_slice(&INSN);
            write_u32(&mut buf[2..], (gotplt + 4) as u32);
        }
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let reloc_offset =
            sym.plt_idx(&ctx.symbols).unwrap() as u64 * std::mem::size_of::<ElfRel<Self>>() as u64;
        if ctx.args.pic {
            const INSN: [u8; 16] = [
                0xb9, 0, 0, 0, 0, // mov $reloc_offset, %ecx
                0xff, 0xa3, 0, 0, 0, 0, // jmp *foo@GOT(%ebx)
                0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            ];
            buf[..16].copy_from_slice(&INSN);
            write_u32(&mut buf[1..], reloc_offset as u32);
            write_u32(
                &mut buf[7..],
                sym.gotplt_addr(ctx)
                    .wrapping_sub(u64::from(ctx.got.hdr.shdr.sh_addr.get())) as u32,
            );
        } else {
            const INSN: [u8; 16] = [
                0xb9, 0, 0, 0, 0, // mov $reloc_offset, %ecx
                0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
                0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            ];
            buf[..16].copy_from_slice(&INSN);
            write_u32(&mut buf[1..], reloc_offset as u32);
            write_u32(&mut buf[7..], sym.gotplt_addr(ctx) as u32);
        }
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        if ctx.args.pic {
            const INSN: [u8; 8] = [
                0xff, 0xa3, 0, 0, 0, 0, // jmp *foo@GOT(%ebx)
                0xcc, 0xcc, // (padding)
            ];
            buf[..8].copy_from_slice(&INSN);
            write_u32(
                &mut buf[2..],
                sym.got_pltgot_addr(ctx)
                    .wrapping_sub(u64::from(ctx.got.hdr.shdr.sh_addr.get())) as u32,
            );
        } else {
            const INSN: [u8; 8] = [
                0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
                0xcc, 0xcc, // (padding)
            ];
            buf[..8].copy_from_slice(&INSN);
            write_u32(&mut buf[2..], sym.got_pltgot_addr(ctx) as u32);
        }
    }

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        _isec: &InputSection,
        rel: &Self::Rel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        match rel.r_type() {
            R_NONE => {}
            R_386_32 => write_u32(loc, val as u32),
            R_386_PC32 => write_u32(loc, val.wrapping_sub(p) as u32),
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels::<Self>(file);
        let mut i = 0;

        // Scan relocations
        while i < rels.len() {
            let rel = &rels[i];
            i += 1;
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];

            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            if rel.r_type() == R_386_TLS_GD || rel.r_type() == R_386_TLS_LDM {
                let next = rels.get(i).map(|r| r.r_type());
                if !matches!(
                    next,
                    Some(R_386_PLT32 | R_386_PC32 | R_386_GOT32 | R_386_GOT32X)
                ) {
                    fatal!(
                        ctx,
                        "{}: {} must be followed by PLT or GOT32",
                        isec.display(file),
                        rel.type_name::<Self>()
                    );
                }
            }

            match rel.r_type() {
                R_386_8 | R_386_16 => scan_absrel(ctx, isec, sym, rel),
                R_386_PC8 | R_386_PC16 | R_386_PC32 => scan_pcrel(ctx, isec, sym, rel),
                R_386_GOT32 | R_386_GOTPC => sym.add_flags(NEEDS_GOT),
                R_386_GOT32X => {
                    // We always want to relax GOT32X even if --no-relax is given
                    // because static PIE doesn't work without it.
                    if sym.is_pcrel_linktime_const(ctx) && relax_got32x(loc_before(isec, rel)) != 0
                    {
                        // Do nothing
                    } else {
                        sym.add_flags(NEEDS_GOT);
                    }
                }
                R_386_PLT32 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_386_TLS_GOTIE | R_386_TLS_IE => sym.add_flags(NEEDS_GOTTP),
                R_386_TLS_GD => {
                    // We always relax if -static because libc.a doesn't contain
                    // __tls_get_addr().
                    if ctx.args.is_static || (ctx.args.relax && sym.is_tprel_linktime_const(ctx)) {
                        i += 1;
                    } else {
                        sym.add_flags(NEEDS_TLSGD);
                    }
                }
                R_386_TLS_LDM => {
                    // We always relax if -static because libc.a doesn't contain
                    // __tls_get_addr().
                    if ctx.args.is_static || (ctx.args.relax && !ctx.args.shared) {
                        i += 1;
                    } else {
                        ctx.needs_tlsld
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                R_386_TLS_GOTDESC => scan_tlsdesc(ctx, sym),
                R_386_TLS_LE => check_tlsle(ctx, isec, sym, rel),
                R_386_32 | R_386_GOTOFF | R_386_TLS_LDO_32 | R_386_SIZE32 | R_386_TLS_DESC_CALL => {
                }
                _ => error!(
                    ctx,
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    fn apply_reloc_alloc(
        ctx: &Context<Self>,
        isec: &InputSection,
        rels: &mut [Self::Rel],
        buf: &mut [u8],
    ) {
        let file = &ctx.objs[isec.file.index()];
        let mut i = 0;

        while i < rels.len() {
            let rel_idx = i;
            let rel = rels[rel_idx];
            i += 1;
            if rel.r_type() == R_NONE {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let off = rel.r_offset() as usize;
            let s = sym.addr(ctx);
            let a = isec.rel_addend::<Self>(&rel) as u64;
            let p = isec.addr(ctx) + rel.r_offset();
            let got = u64::from(ctx.got.hdr.shdr.sh_addr.get());
            let g = || sym.got_addr(ctx).wrapping_sub(got);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, rel_idx, val, lo, hi);

            match rel.r_type() {
                R_386_8 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 8);
                    buf[off] = s.wrapping_add(a) as u8;
                }
                R_386_16 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 16);
                    write_u16(&mut buf[off..], s.wrapping_add(a) as u16);
                }
                // Handled as an absolute relocation by the output section.
                R_386_32 => {}
                R_386_PC8 => {
                    let v = s.wrapping_add(a).wrapping_sub(p);
                    check(v as i64, -(1 << 7), 1 << 7);
                    buf[off] = v as u8;
                }
                R_386_PC16 => {
                    let v = s.wrapping_add(a).wrapping_sub(p);
                    check(v as i64, -(1 << 15), 1 << 15);
                    write_u16(&mut buf[off..], v as u16);
                }
                R_386_PC32 | R_386_PLT32 => {
                    write_u32(&mut buf[off..], s.wrapping_add(a).wrapping_sub(p) as u32)
                }
                R_386_GOT32 => write_u32(&mut buf[off..], g().wrapping_add(a) as u32),
                R_386_GOT32X => {
                    if sym.has_got(&ctx.symbols) {
                        write_u32(&mut buf[off..], g().wrapping_add(a) as u32);
                    } else {
                        let insn = relax_got32x(&buf[..off]);
                        debug_assert!(insn != 0);
                        buf[off - 2] = (insn >> 8) as u8;
                        buf[off - 1] = insn as u8;
                        write_u32(&mut buf[off..], s.wrapping_add(a).wrapping_sub(got) as u32);
                        if ctx.args.emit_relocs {
                            rels[rel_idx].set_r_type(R_386_GOTOFF);
                        }
                    }
                }
                R_386_GOTOFF => {
                    write_u32(&mut buf[off..], s.wrapping_add(a).wrapping_sub(got) as u32)
                }
                R_386_GOTPC => {
                    write_u32(&mut buf[off..], got.wrapping_add(a).wrapping_sub(p) as u32)
                }
                R_386_TLS_GOTIE => write_u32(
                    &mut buf[off..],
                    sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got) as u32,
                ),
                R_386_TLS_LE => write_u32(
                    &mut buf[off..],
                    s.wrapping_add(a).wrapping_sub(ctx.tp_addr) as u32,
                ),
                R_386_TLS_IE => {
                    write_u32(&mut buf[off..], sym.gottp_addr(ctx).wrapping_add(a) as u32)
                }
                R_386_TLS_GD => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        write_u32(
                            &mut buf[off..],
                            sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got) as u32,
                        );
                    } else {
                        let next = &rels[i];
                        i += 1;
                        relax_gd_to_le(buf, off, next, s.wrapping_sub(ctx.tp_addr));
                    }
                }
                R_386_TLS_LDM => {
                    if ctx.got.has_tlsld() {
                        write_u32(
                            &mut buf[off..],
                            ctx.got.tlsld_addr().wrapping_add(a).wrapping_sub(got) as u32,
                        );
                    } else {
                        let next = &rels[i];
                        i += 1;
                        relax_ld_to_le(buf, off, next, ctx.tp_addr.wrapping_sub(ctx.tls_begin));
                    }
                }
                R_386_TLS_LDO_32 => write_u32(
                    &mut buf[off..],
                    s.wrapping_add(a).wrapping_sub(ctx.dtp_addr) as u32,
                ),
                R_386_SIZE32 => write_u32(
                    &mut buf[off..],
                    (sym.esym(ctx).st_size().get() as u64).wrapping_add(a) as u32,
                ),
                R_386_TLS_GOTDESC => {
                    // i386 TLSDESC uses the following code sequence to materialize
                    // a TP-relative address in %eax.
                    //
                    // lea    0(%ebx), %eax
                    // R_386_TLS_GOTDESC   foo
                    // call   *(%eax)
                    // R_386_TLS_DESC_CALL foo
                    //
                    // We may relax the instructions to the following if its TP-relative
                    // address is known at link-time
                    //
                    // mov     $foo@TPOFF, %eax
                    // nop
                    //
                    // or to the following if the TP-relative address is known at
                    // process startup time.
                    //
                    // mov     foo@GOTTPOFF(%ebx), %eax
                    // nop
                    //
                    // We allow the following alternative code sequence too because
                    // LLVM emits such code.
                    //
                    // lea    0(%ebx), %reg
                    // R_386_TLS_GOTDESC   foo
                    // mov    %reg, %eax
                    // call   *(%eax)
                    // R_386_TLS_DESC_CALL foo
                    //
                    // Note that the compiler always uses the local-exec TLS model
                    // for -fno-pic, so TLSDESC code is always PIC (i.e. uses %ebx to
                    // store the address of GOT.)
                    if sym.has_tlsdesc(&ctx.symbols) {
                        write_u32(
                            &mut buf[off..],
                            sym.tlsdesc_addr(ctx).wrapping_add(a).wrapping_sub(got) as u32,
                        );
                    } else if sym.has_gottp(&ctx.symbols) {
                        let insn = relax_tlsdesc_to_ie(&buf[..off]);
                        if insn == 0 {
                            fatal!(
                                ctx,
                                "{}: illegal instruction sequence for TLSDESC",
                                isec.display(file)
                            );
                        }
                        buf[off - 2] = (insn >> 8) as u8;
                        buf[off - 1] = insn as u8;
                        write_u32(
                            &mut buf[off..],
                            sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got) as u32,
                        );
                    } else {
                        let insn = relax_tlsdesc_to_le(&buf[..off]);
                        if insn == 0 {
                            fatal!(
                                ctx,
                                "{}: illegal instruction sequence for TLSDESC",
                                isec.display(file)
                            );
                        }
                        buf[off - 2] = (insn >> 8) as u8;
                        buf[off - 1] = insn as u8;
                        write_u32(
                            &mut buf[off..],
                            s.wrapping_add(a).wrapping_sub(ctx.tp_addr) as u32,
                        );
                    }
                }
                R_386_TLS_DESC_CALL => {
                    if !sym.has_tlsdesc(&ctx.symbols) {
                        // call *(%eax) -> nop
                        buf[off] = 0x66;
                        buf[off + 1] = 0x90;
                    }
                }
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        for (i, rel) in isec.relocations::<Self>(ctx).enumerate() {
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            let off = rel.r_offset() as usize;
            let frag = isec.fragment(ctx, &rel);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), isec.rel_addend::<Self>(&rel) as u64),
            };
            let frag_ref = frag.map(|(f, _)| f);
            let got = u64::from(ctx.got.hdr.shdr.sh_addr.get());

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);

            match rel.r_type() {
                R_386_8 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 8);
                    buf[off] = s.wrapping_add(a) as u8;
                }
                R_386_16 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 16);
                    write_u16(&mut buf[off..], s.wrapping_add(a) as u16);
                }
                R_386_32 => match isec.tombstone(ctx, sym, frag_ref) {
                    Some(v) => write_u32(&mut buf[off..], v as u32),
                    None => write_u32(&mut buf[off..], s.wrapping_add(a) as u32),
                },
                R_386_PC8 => {
                    check(s.wrapping_add(a) as i64, -(1 << 7), 1 << 7);
                    buf[off] = s.wrapping_add(a) as u8;
                }
                R_386_PC16 => {
                    check(s.wrapping_add(a) as i64, -(1 << 15), 1 << 15);
                    write_u16(&mut buf[off..], s.wrapping_add(a) as u16);
                }
                R_386_PC32 => write_u32(&mut buf[off..], s.wrapping_add(a) as u32),
                R_386_GOTPC => write_u32(&mut buf[off..], got.wrapping_add(a) as u32),
                R_386_GOTOFF => {
                    write_u32(&mut buf[off..], s.wrapping_add(a).wrapping_sub(got) as u32)
                }
                R_386_TLS_LDO_32 => match isec.tombstone(ctx, sym, frag_ref) {
                    Some(v) => write_u32(&mut buf[off..], v as u32),
                    None => write_u32(
                        &mut buf[off..],
                        s.wrapping_add(a).wrapping_sub(ctx.dtp_addr) as u32,
                    ),
                },
                R_386_SIZE32 => write_u32(
                    &mut buf[off..],
                    (sym.esym(ctx).st_size().get() as u64).wrapping_add(a) as u32,
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

    fn write_addend(loc: &mut [u8], val: i64, rel: &Self::Rel) {
        match rel.r_type() {
            R_386_NONE => {}
            R_386_8 | R_386_PC8 => loc[0] = val as u8,
            R_386_16 | R_386_PC16 => write_u16(loc, val as u16),
            R_386_32 | R_386_PC32 | R_386_GOT32 | R_386_GOT32X | R_386_PLT32 | R_386_GOTOFF
            | R_386_GOTPC | R_386_TLS_LDM | R_386_TLS_GOTIE | R_386_TLS_LE | R_386_TLS_IE
            | R_386_TLS_GD | R_386_TLS_LDO_32 | R_386_SIZE32 | R_386_TLS_GOTDESC => {
                write_u32(loc, val as u32)
            }
            _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
        }
    }

    fn get_addend(loc: &[u8], rel: &Self::Rel) -> i64 {
        match rel.r_type() {
            R_386_8 | R_386_PC8 => loc[0] as i8 as i64,
            R_386_16 | R_386_PC16 => i16::from_le_bytes([loc[0], loc[1]]) as i64,
            R_386_32 | R_386_PC32 | R_386_GOT32 | R_386_GOT32X | R_386_PLT32 | R_386_GOTOFF
            | R_386_GOTPC | R_386_TLS_LDM | R_386_TLS_GOTIE | R_386_TLS_LE | R_386_TLS_IE
            | R_386_TLS_GD | R_386_TLS_LDO_32 | R_386_SIZE32 | R_386_TLS_GOTDESC => {
                i32::from_le_bytes([loc[0], loc[1], loc[2], loc[3]]) as i64
            }
            _ => 0,
        }
    }
}

fn write_u16(buf: &mut [u8], v: u16) {
    buf[..2].copy_from_slice(&v.to_le_bytes());
}

fn write_u32(buf: &mut [u8], v: u32) {
    buf[..4].copy_from_slice(&v.to_le_bytes());
}

/// The bytes of a section preceding a relocated location.
fn loc_before<'a>(isec: &'a InputSection, rel: &ElfRel<I386>) -> &'a [u8] {
    &isec.contents()[..rel.r_offset() as usize]
}

/// The last two bytes before a relocated location, as an opcode.
fn last2(loc: &[u8]) -> u32 {
    match loc {
        [.., a, b] => ((*a as u32) << 8) | *b as u32,
        _ => 0,
    }
}

// mov imm(%reg1), %reg2 -> lea imm(%reg1), %reg2
fn relax_got32x(loc: &[u8]) -> u32 {
    match loc {
        [.., 0x8b, modrm] => 0x8d00 | *modrm as u32,
        _ => 0,
    }
}

// Relax GD to LE
fn relax_gd_to_le(buf: &mut [u8], off: usize, rel: &ElfRel<I386>, val: u64) {
    const INSN: [u8; 12] = [
        0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
        0x81, 0xc0, 0, 0, 0, 0, // add $tp_offset, %eax
    ];
    match rel.r_type() {
        R_386_PLT32 | R_386_PC32 => {
            buf[off - 3..off + 9].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 5..], val as u32);
        }
        R_386_GOT32 | R_386_GOT32X => {
            buf[off - 2..off + 10].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 6..], val as u32);
        }
        _ => unreachable!(),
    }
}

// Relax LD to LE
fn relax_ld_to_le(buf: &mut [u8], off: usize, rel: &ElfRel<I386>, tls_size: u64) {
    match rel.r_type() {
        R_386_PLT32 | R_386_PC32 => {
            const INSN: [u8; 11] = [
                0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
                0x2d, 0, 0, 0, 0, // sub $tls_size, %eax
            ];
            buf[off - 2..off + 9].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 5..], tls_size as u32);
        }
        R_386_GOT32 | R_386_GOT32X => {
            const INSN: [u8; 12] = [
                0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
                0x81, 0xe8, 0, 0, 0, 0, // sub $tls_size, %eax
            ];
            buf[off - 2..off + 10].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 6..], tls_size as u32);
        }
        _ => unreachable!(),
    }
}

fn relax_tlsdesc_to_ie(loc: &[u8]) -> u32 {
    match last2(loc) {
        0x8d83 => 0x8b83, // lea 0(%ebx), %eax -> mov 0(%ebx), %eax
        0x8d9b => 0x8b9b, // lea 0(%ebx), %ebx -> mov 0(%ebx), %ebx
        0x8d8b => 0x8b8b, // lea 0(%ebx), %ecx -> mov 0(%ebx), %ecx
        0x8d93 => 0x8b93, // lea 0(%ebx), %edx -> mov 0(%ebx), %edx
        0x8db3 => 0x8bb3, // lea 0(%ebx), %esi -> mov 0(%ebx), %esi
        0x8dbb => 0x8bbb, // lea 0(%ebx), %edi -> mov 0(%ebx), %edi
        0x8da3 => 0x8ba3, // lea 0(%ebx), %esp -> mov 0(%ebx), %esp
        0x8dab => 0x8bab, // lea 0(%ebx), %ebp -> mov 0(%ebx), %ebp
        _ => 0,
    }
}

fn relax_tlsdesc_to_le(loc: &[u8]) -> u32 {
    match last2(loc) {
        0x8d83 => 0x90b8, // lea 0(%ebx), %eax -> mov $0, %eax
        0x8d9b => 0x90bb, // lea 0(%ebx), %ebx -> mov $0, %ebx
        0x8d8b => 0x90b9, // lea 0(%ebx), %ecx -> mov $0, %ecx
        0x8d93 => 0x90ba, // lea 0(%ebx), %edx -> mov $0, %edx
        0x8db3 => 0x90be, // lea 0(%ebx), %esi -> mov $0, %esi
        0x8dbb => 0x90bf, // lea 0(%ebx), %edi -> mov $0, %edi
        0x8da3 => 0x90bc, // lea 0(%ebx), %esp -> mov $0, %esp
        0x8dab => 0x90bd, // lea 0(%ebx), %ebp -> mov $0, %ebp
        _ => 0,
    }
}
