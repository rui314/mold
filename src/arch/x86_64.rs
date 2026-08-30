//! x86-64.
//!
//! x86-64 is straightforward: it has PC-relative addressing for
//! position-independent code, and 32-bit branch displacements, so no
//! range extension thunks are needed. `%r11` is neither caller- nor
//! callee-saved, so the PLT uses it as a scratch register.
//!
//! The thread pointer lives in the `%fs` segment register, and for
//! historical reasons points past the end of the TLS block, so offsets to
//! thread-local variables in the main executable are negative.

use crate::arch::{Arch, Family};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::input_sections::{check_tlsle, scan_absrel, scan_pcrel, scan_tlsdesc, InputSection};
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::util::is_int;
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct X86_64;

impl Layout for X86_64 {
    type Endian = LittleEndian;
    const IS_64: bool = true;
    const IS_RELA: bool = true;
}

impl Arch for X86_64 {
    const NAME: &'static str = "x86_64";
    const FAMILY: Family = Family::X86_64;
    const PAGE_SIZE: u64 = 4096;
    const E_MACHINE: u32 = EM_X86_64;
    const PLT_HDR_SIZE: u64 = 32;
    const PLT_SIZE: u64 = 16;
    const PLTGOT_SIZE: u64 = 8;
    const SFRAME_ABI: Option<u8> = Some(SFRAME_ABI_AMD64_ENDIAN_LITTLE);
    const TRAP: &'static [u8] = &[0xcc]; // int3

    const R_COPY: u32 = R_X86_64_COPY;
    const R_GLOB_DAT: u32 = R_X86_64_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_X86_64_JUMP_SLOT;
    const R_ABS: u32 = R_X86_64_64;
    const R_RELATIVE: u32 = R_X86_64_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_X86_64_IRELATIVE);
    const R_DTPOFF: u32 = R_X86_64_DTPOFF64;
    const R_TPOFF: u32 = R_X86_64_TPOFF64;
    const R_DTPMOD: u32 = R_X86_64_DTPMOD64;
    const R_TLSDESC: Option<u32> = Some(R_X86_64_TLSDESC);
    const R_SFRAME: Option<u32> = Some(R_X86_64_PC64);
    const R_FUNCALL: &'static [u32] = &[R_X86_64_PLT32, R_X86_64_PLTOFF64];

    fn finish_output(ctx: &Context<Self>, buf: &mut [u8]) {
        if ctx.args.z_rewrite_endbr {
            rewrite_endbr(ctx, buf);
        }
    }

    fn rel_to_string(r_type: u32) -> String {
        x86_64_rel_to_string(r_type)
    }

    /// The PLT header and entries start with `endbr64` for Intel CET.
    /// Unlike GNU ld's IBT PLT, which splits `.plt` and `.plt.sec` with
    /// 32 bytes per entry, ours keeps 16-byte entries in one section.
    /// Clobbering `%r11` is fine because the resolver does so anyway.
    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u8; 32] = [
            0xf3, 0x0f, 0x1e, 0xfa, // endbr64
            0x41, 0x53, // push %r11
            0xff, 0x35, 0, 0, 0, 0, // push GOTPLT+8(%rip)
            0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
            0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc,
            0xcc, // padding
        ];
        buf[..32].copy_from_slice(&INSN);
        let gotplt = ctx.gotplt.hdr.shdr.sh_addr;
        let plt = ctx.plt.hdr.shdr.sh_addr;
        write_u32(
            &mut buf[8..],
            gotplt.wrapping_sub(plt).wrapping_sub(4) as u32,
        );
        write_u32(
            &mut buf[14..],
            gotplt.wrapping_sub(plt).wrapping_sub(2) as u32,
        );
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let plt_idx = sym.plt_idx(&ctx.symbols).unwrap();
        let disp = sym.gotplt_addr(ctx).wrapping_sub(sym.plt_addr(ctx));
        // Only a canonical PLT entry can be address-taken, so only it
        // needs a landing pad.
        if sym.is_canonical() {
            const INSN: [u8; 16] = [
                0xf3, 0x0f, 0x1e, 0xfa, // endbr64
                0x41, 0xbb, 0, 0, 0, 0, // mov $index_in_relplt, %r11d
                0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOTPLT
            ];
            buf[..16].copy_from_slice(&INSN);
            write_u32(&mut buf[6..], plt_idx);
            write_u32(&mut buf[12..], disp.wrapping_sub(16) as u32);
        } else {
            const INSN: [u8; 16] = [
                0x41, 0xbb, 0, 0, 0, 0, // mov $index_in_relplt, %r11d
                0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOTPLT
                0xcc, 0xcc, 0xcc, 0xcc, // padding
            ];
            buf[..16].copy_from_slice(&INSN);
            write_u32(&mut buf[2..], plt_idx);
            write_u32(&mut buf[8..], disp.wrapping_sub(12) as u32);
        }
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u8; 8] = [
            0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
            0xcc, 0xcc, // padding
        ];
        buf[..8].copy_from_slice(&INSN);
        let disp = sym
            .got_pltgot_addr(ctx)
            .wrapping_sub(sym.plt_addr(ctx))
            .wrapping_sub(6);
        write_u32(&mut buf[2..], disp as u32);
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
            R_X86_64_32 => {
                check(val as i64, 0, 1 << 32);
                write_u32(loc, val as u32);
            }
            R_X86_64_64 => write_u64(loc, val),
            R_X86_64_PC32 => {
                check(val.wrapping_sub(p) as i64, -(1 << 31), 1 << 31);
                write_u32(loc, val.wrapping_sub(p) as u32);
            }
            R_X86_64_PC64 => write_u64(loc, val.wrapping_sub(p)),
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels::<Self>(file);
        let mut i = 0;

        while i < rels.len() {
            let rel = &rels.at(i);
            i += 1;
            if rel.r_type == R_NONE || isec.record_undef_error_with_file(ctx, file, rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            let loc = &isec.contents()[rel.r_offset as usize..];

            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            if rel.r_type == R_X86_64_TLSGD || rel.r_type == R_X86_64_TLSLD {
                let next = rels.get(i).map(|r| r.r_type);
                let ok = matches!(
                    next,
                    Some(
                        R_X86_64_PLT32
                            | R_X86_64_PC32
                            | R_X86_64_PLTOFF64
                            | R_X86_64_GOTPCREL
                            | R_X86_64_GOTPCRELX
                    )
                );
                if !ok {
                    fatal!(
                        ctx,
                        "{}: {} must be followed by PLT or GOTPCREL",
                        isec.display(file),
                        rel.type_name::<Self>()
                    );
                }
            }

            match rel.r_type {
                R_X86_64_8 | R_X86_64_16 | R_X86_64_32 | R_X86_64_32S => {
                    scan_absrel(ctx, isec, sym, rel)
                }
                R_X86_64_PC8 | R_X86_64_PC16 | R_X86_64_PC32 | R_X86_64_PC64 => {
                    scan_pcrel(ctx, isec, sym, rel)
                }
                R_X86_64_GOT32
                | R_X86_64_GOT64
                | R_X86_64_GOTPC32
                | R_X86_64_GOTPC64
                | R_X86_64_GOTPCREL
                | R_X86_64_GOTPCREL64
                | R_X86_64_GOTPCRELX
                | R_X86_64_REX_GOTPCRELX
                | R_X86_64_CODE_4_GOTPCRELX => sym.add_flags(NEEDS_GOT),
                R_X86_64_PLT32 | R_X86_64_PLTOFF64 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_X86_64_TLSGD => {
                    // Always relax with -static because libc.a doesn't
                    // contain __tls_get_addr.
                    if ctx.args.is_static || (ctx.args.relax && sym.is_tprel_linktime_const(ctx)) {
                        i += 1;
                    } else if ctx.args.relax && sym.is_tprel_runtime_const(ctx) {
                        sym.add_flags(NEEDS_GOTTP);
                        i += 1;
                    } else {
                        sym.add_flags(NEEDS_TLSGD);
                    }
                }
                R_X86_64_TLSLD => {
                    if ctx.args.is_static || (ctx.args.relax && !ctx.args.shared) {
                        i += 1;
                    } else {
                        ctx.needs_tlsld
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                R_X86_64_GOTTPOFF | R_X86_64_CODE_4_GOTTPOFF => {
                    if !ctx.args.relax
                        || !sym.is_tprel_linktime_const(ctx)
                        || relax_gottpoff(loc_before(isec, rel), rel) == 0
                    {
                        sym.add_flags(NEEDS_GOTTP);
                    }
                }
                R_X86_64_CODE_6_GOTTPOFF => sym.add_flags(NEEDS_GOTTP),
                R_X86_64_TLSDESC_CALL => scan_tlsdesc(ctx, sym),
                R_X86_64_TPOFF32 | R_X86_64_TPOFF64 => check_tlsle(ctx, isec, sym, rel),
                R_X86_64_64
                | R_X86_64_GOTOFF64
                | R_X86_64_DTPOFF32
                | R_X86_64_DTPOFF64
                | R_X86_64_SIZE32
                | R_X86_64_SIZE64
                | R_X86_64_GOTPC32_TLSDESC
                | R_X86_64_CODE_4_GOTPC32_TLSDESC => {}
                _ => error!(
                    ctx,
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
            let _ = loc;
        }
    }

    fn apply_reloc_alloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels::<Self>(file);
        let mut i = 0;

        while i < rels.len() {
            let rel = &rels.at(i);
            i += 1;
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
            let got_base = ctx.gotplt.hdr.shdr.sh_addr;
            let g = if sym.has_got(&ctx.symbols) {
                sym.got_addr(ctx).wrapping_sub(got_base)
            } else {
                0
            };

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i - 1, val, lo, hi);
            let write32 = |buf: &mut [u8], val: u64| {
                check(val as i64, 0, 1 << 32);
                write_u32(&mut buf[off..], val as u32);
            };
            let write32s = |buf: &mut [u8], val: u64| {
                check(val as i64, -(1 << 31), 1 << 31);
                write_u32(&mut buf[off..], val as u32);
            };

            match rel.r_type {
                R_X86_64_8 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 8);
                    buf[off] = s.wrapping_add(a) as u8;
                }
                R_X86_64_16 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 16);
                    write_u16(&mut buf[off..], s.wrapping_add(a) as u16);
                }
                R_X86_64_32 => write32(buf, s.wrapping_add(a)),
                R_X86_64_32S => write32s(buf, s.wrapping_add(a)),
                // Handled as an absolute relocation by the output section.
                R_X86_64_64 => {}
                R_X86_64_PC8 => {
                    let v = s.wrapping_add(a).wrapping_sub(p);
                    check(v as i64, -(1 << 7), 1 << 7);
                    buf[off] = v as u8;
                }
                R_X86_64_PC16 => {
                    let v = s.wrapping_add(a).wrapping_sub(p);
                    check(v as i64, -(1 << 15), 1 << 15);
                    write_u16(&mut buf[off..], v as u16);
                }
                R_X86_64_PC32 => write32s(buf, s.wrapping_add(a).wrapping_sub(p)),
                R_X86_64_PLT32 => {
                    let v = s.wrapping_add(a).wrapping_sub(p);
                    if !sym.is_remaining_undef_weak() {
                        check(v as i64, -(1 << 31), 1 << 31);
                    }
                    write_u32(&mut buf[off..], v as u32);
                }
                R_X86_64_PC64 => write_u64(&mut buf[off..], s.wrapping_add(a).wrapping_sub(p)),
                R_X86_64_GOT32 => write32(buf, g.wrapping_add(a)),
                R_X86_64_GOT64 => write_u64(&mut buf[off..], g.wrapping_add(a)),
                R_X86_64_GOTOFF64 | R_X86_64_PLTOFF64 => {
                    write_u64(&mut buf[off..], s.wrapping_add(a).wrapping_sub(got_base))
                }
                R_X86_64_GOTPC32 => write32s(buf, got_base.wrapping_add(a).wrapping_sub(p)),
                R_X86_64_GOTPC64 => {
                    write_u64(&mut buf[off..], got_base.wrapping_add(a).wrapping_sub(p))
                }
                R_X86_64_GOTPCREL => write32s(
                    buf,
                    g.wrapping_add(got_base).wrapping_add(a).wrapping_sub(p),
                ),
                R_X86_64_GOTPCREL64 => write_u64(
                    &mut buf[off..],
                    g.wrapping_add(got_base).wrapping_add(a).wrapping_sub(p),
                ),
                R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX | R_X86_64_CODE_4_GOTPCRELX => {
                    // GOTPCRELX is relaxed even with --no-relax because some
                    // static PIE runtime code depends on it.
                    let v = s.wrapping_add(a).wrapping_sub(p);
                    if sym.is_pcrel_linktime_const(ctx) && is_int(v as i64, 32) {
                        let insn = relax_gotpcrelx(&buf[..off], rel);
                        if insn != 0 {
                            buf[off - 2] = (insn >> 8) as u8;
                            buf[off - 1] = insn as u8;
                            write_u32(&mut buf[off..], v as u32);
                            continue;
                        }
                    }
                    write32s(
                        buf,
                        g.wrapping_add(got_base).wrapping_add(a).wrapping_sub(p),
                    );
                }
                R_X86_64_TLSGD => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        write32s(buf, sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else if sym.has_gottp(&ctx.symbols) {
                        let next = &rels.at(i);
                        i += 1;
                        relax_gd_to_ie(buf, off, next, sym.gottp_addr(ctx).wrapping_sub(p));
                    } else {
                        let next = &rels.at(i);
                        i += 1;
                        relax_gd_to_le(buf, off, next, s.wrapping_sub(ctx.tp_addr));
                    }
                }
                R_X86_64_TLSLD => {
                    if ctx.got.has_tlsld() {
                        write32s(
                            buf,
                            ctx.got.tlsld_addr::<Self>().wrapping_add(a).wrapping_sub(p),
                        );
                    } else {
                        let next = &rels.at(i);
                        i += 1;
                        relax_ld_to_le(buf, off, next, ctx.tp_addr.wrapping_sub(ctx.tls_begin));
                    }
                }
                R_X86_64_DTPOFF32 => write32s(buf, s.wrapping_add(a).wrapping_sub(ctx.dtp_addr)),
                R_X86_64_DTPOFF64 => write_u64(
                    &mut buf[off..],
                    s.wrapping_add(a).wrapping_sub(ctx.dtp_addr),
                ),
                R_X86_64_TPOFF32 => write32s(buf, s.wrapping_add(a).wrapping_sub(ctx.tp_addr)),
                R_X86_64_TPOFF64 => {
                    write_u64(&mut buf[off..], s.wrapping_add(a).wrapping_sub(ctx.tp_addr))
                }
                R_X86_64_GOTTPOFF | R_X86_64_CODE_4_GOTTPOFF => {
                    if sym.has_gottp(&ctx.symbols) {
                        write32s(buf, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else {
                        let insn = relax_gottpoff(&buf[..off], rel);
                        buf[off - 3] = (insn >> 16) as u8;
                        buf[off - 2] = (insn >> 8) as u8;
                        buf[off - 1] = insn as u8;
                        write32s(buf, s.wrapping_sub(ctx.tp_addr));
                    }
                }
                R_X86_64_CODE_6_GOTTPOFF => {
                    write32s(buf, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(p))
                }
                R_X86_64_GOTPC32_TLSDESC | R_X86_64_CODE_4_GOTPC32_TLSDESC => {
                    // TLSDESC materializes a TP-relative address in %rax:
                    //
                    //   lea    0(%rip), %rax  # R_X86_64_GOTPC32_TLSDESC
                    //   call   *(%rax)        # R_X86_64_TLSDESC_CALL
                    //
                    // If the address is known at link time it becomes
                    // `mov $foo@TPOFF, %rax; nop`, and if at load time
                    // `mov foo@GOTTPOFF(%rip), %rax; nop`.
                    if sym.has_tlsdesc(&ctx.symbols) {
                        write32s(buf, sym.tlsdesc_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else if sym.has_gottp(&ctx.symbols) {
                        let insn = relax_tlsdesc_to_ie(&buf[..off], rel);
                        if insn == 0 {
                            fatal!(
                                ctx,
                                "{}: illegal instruction sequence for {}",
                                isec.display(file),
                                rel.type_name::<Self>()
                            );
                        }
                        buf[off - 3] = (insn >> 16) as u8;
                        buf[off - 2] = (insn >> 8) as u8;
                        buf[off - 1] = insn as u8;
                        write32s(buf, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else {
                        let insn = relax_tlsdesc_to_le(&buf[..off], rel);
                        if insn == 0 {
                            fatal!(
                                ctx,
                                "{}: illegal instruction sequence for {}",
                                isec.display(file),
                                rel.type_name::<Self>()
                            );
                        }
                        buf[off - 3] = (insn >> 16) as u8;
                        buf[off - 2] = (insn >> 8) as u8;
                        buf[off - 1] = insn as u8;
                        write32s(buf, s.wrapping_sub(ctx.tp_addr));
                    }
                }
                R_X86_64_TLSDESC_CALL => {
                    if !sym.has_tlsdesc(&ctx.symbols) {
                        // call *(%rax) -> nop
                        buf[off] = 0x66;
                        buf[off + 1] = 0x90;
                    }
                }
                R_X86_64_SIZE32 => write32(buf, sym.esym(ctx).st_size.wrapping_add(a)),
                R_X86_64_SIZE64 => {
                    write_u64(&mut buf[off..], sym.esym(ctx).st_size.wrapping_add(a))
                }
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    /// Relocations against non-allocated sections (mostly debug info)
    /// never need GOT or PLT entries, and aren't scanned beforehand.
    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        isec.for_each_reloc::<Self>(ctx, |rel, i| {
            if rel.r_type == R_NONE || isec.record_undef_error_with_file(ctx, file, &rel) {
                return;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            let off = rel.r_offset as usize;
            let frag = isec.fragment_with_file::<Self>(file, &rel);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), rel.r_addend as u64),
            };
            let frag_ref = frag.map(|(f, _)| f);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);
            let write32 = |buf: &mut [u8], val: u64| {
                check(val as i64, 0, 1 << 32);
                write_u32(&mut buf[off..], val as u32);
            };
            let write32s = |buf: &mut [u8], val: u64| {
                check(val as i64, -(1 << 31), 1 << 31);
                write_u32(&mut buf[off..], val as u32);
            };

            match rel.r_type {
                R_X86_64_8 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 8);
                    buf[off] = s.wrapping_add(a) as u8;
                }
                R_X86_64_16 => {
                    check(s.wrapping_add(a) as i64, 0, 1 << 16);
                    write_u16(&mut buf[off..], s.wrapping_add(a) as u16);
                }
                R_X86_64_32 => write32(buf, s.wrapping_add(a)),
                R_X86_64_32S => write32s(buf, s.wrapping_add(a)),
                R_X86_64_64 => match isec.tombstone_with_file(ctx, file, sym, frag_ref) {
                    Some(v) => write_u64(&mut buf[off..], v),
                    None => write_u64(&mut buf[off..], s.wrapping_add(a)),
                },
                R_X86_64_DTPOFF32 => match isec.tombstone_with_file(ctx, file, sym, frag_ref) {
                    Some(v) => write_u32(&mut buf[off..], v as u32),
                    None => write32s(buf, s.wrapping_add(a).wrapping_sub(ctx.dtp_addr)),
                },
                R_X86_64_DTPOFF64 => match isec.tombstone_with_file(ctx, file, sym, frag_ref) {
                    Some(v) => write_u64(&mut buf[off..], v),
                    None => write_u64(
                        &mut buf[off..],
                        s.wrapping_add(a).wrapping_sub(ctx.dtp_addr),
                    ),
                },
                R_X86_64_GOTOFF64 => write_u64(
                    &mut buf[off..],
                    s.wrapping_add(a).wrapping_sub(ctx.gotplt.hdr.shdr.sh_addr),
                ),
                // GCC 6.3 emits this for _GLOBAL_OFFSET_TABLE_ even though a
                // PC-relative relocation makes no sense here.
                R_X86_64_GOTPC64 => {
                    write_u64(&mut buf[off..], ctx.gotplt.hdr.shdr.sh_addr.wrapping_add(a))
                }
                R_X86_64_SIZE32 => write32(buf, sym.esym(ctx).st_size.wrapping_add(a)),
                R_X86_64_SIZE64 => {
                    write_u64(&mut buf[off..], sym.esym(ctx).st_size.wrapping_add(a))
                }
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
        if matches!(
            rel.r_type,
            R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX | R_X86_64_CODE_4_GOTPCRELX
        ) && isec.is_alloc()
        {
            let file = &ctx.objs[isec.file.index()];
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            let s = sym.addr(ctx);
            let p = isec.addr(ctx) + rel.r_offset;
            let v = s.wrapping_add(rel.r_addend as u64).wrapping_sub(p);
            if sym.is_pcrel_linktime_const(ctx)
                && is_int(v as i64, 32)
                && relax_gotpcrelx(loc_before(isec, rel), rel) != 0
            {
                return R_X86_64_PC32;
            }
        }
        rel.r_type
    }
}

fn write_u16(buf: &mut [u8], v: u16) {
    buf[..2].copy_from_slice(&v.to_le_bytes());
}

fn write_u32(buf: &mut [u8], v: u32) {
    buf[..4].copy_from_slice(&v.to_le_bytes());
}

fn write_u64(buf: &mut [u8], v: u64) {
    buf[..8].copy_from_slice(&v.to_le_bytes());
}

/// The bytes of a section preceding a relocated location.
fn loc_before<'a>(isec: &'a InputSection, rel: &ElfRel) -> &'a [u8] {
    &isec.contents()[..rel.r_offset as usize]
}

fn last2(loc: &[u8]) -> u32 {
    let n = loc.len();
    ((loc[n - 2] as u32) << 8) | loc[n - 1] as u32
}

fn last3(loc: &[u8]) -> u32 {
    let n = loc.len();
    ((loc[n - 3] as u32) << 16) | ((loc[n - 2] as u32) << 8) | loc[n - 1] as u32
}

/// The instruction to substitute when a GOT load can become a direct
/// reference, or 0 if the instruction isn't relaxable. `loc` is the code
/// up to the relocated operand.
fn relax_gotpcrelx(loc: &[u8], rel: &ElfRel) -> u32 {
    if rel.r_type == R_X86_64_GOTPCRELX {
        if loc.len() < 2 {
            return 0;
        }
        return match last2(loc) {
            0xff15 => 0x40e8, // call *0(%rip) -> call 0
            0xff25 => 0x40e9, // jmp  *0(%rip) -> jmp  0
            _ => 0,
        };
    }
    if loc.len() < 3 {
        return 0;
    }
    match last3(loc) {
        // mov 0(%rip), %reg -> lea 0(%rip), %reg
        0x488b05 | 0x4c8b05 => 0x8d05,
        0x488b0d | 0x4c8b0d => 0x8d0d,
        0x488b15 | 0x4c8b15 => 0x8d15,
        0x488b1d | 0x4c8b1d => 0x8d1d,
        0x488b25 | 0x4c8b25 => 0x8d25,
        0x488b2d | 0x4c8b2d => 0x8d2d,
        0x488b35 | 0x4c8b35 => 0x8d35,
        0x488b3d | 0x4c8b3d => 0x8d3d,
        _ => 0,
    }
}

fn relax_gottpoff(loc: &[u8], rel: &ElfRel) -> u32 {
    if loc.len() < 3 {
        return 0;
    }
    let insn = last3(loc);
    if rel.r_type == R_X86_64_GOTTPOFF {
        match insn {
            // mov 0(%rip), %reg -> mov $0, %reg
            0x488b05 => 0x48c7c0,
            0x488b0d => 0x48c7c1,
            0x488b15 => 0x48c7c2,
            0x488b1d => 0x48c7c3,
            0x488b25 => 0x48c7c4,
            0x488b2d => 0x48c7c5,
            0x488b35 => 0x48c7c6,
            0x488b3d => 0x48c7c7,
            0x4c8b05 => 0x49c7c0,
            0x4c8b0d => 0x49c7c1,
            0x4c8b15 => 0x49c7c2,
            0x4c8b1d => 0x49c7c3,
            0x4c8b25 => 0x49c7c4,
            0x4c8b2d => 0x49c7c5,
            0x4c8b35 => 0x49c7c6,
            0x4c8b3d => 0x49c7c7,
            _ => 0,
        }
    } else {
        debug_assert_eq!(rel.r_type, R_X86_64_CODE_4_GOTTPOFF);
        match insn {
            // mov 0(%rip), %r16..%r31 -> mov $0, %r16..%r31
            0x488b05 => 0x18c7c0,
            0x488b0d => 0x18c7c1,
            0x488b15 => 0x18c7c2,
            0x488b1d => 0x18c7c3,
            0x488b25 => 0x18c7c4,
            0x488b2d => 0x18c7c5,
            0x488b35 => 0x18c7c6,
            0x488b3d => 0x18c7c7,
            0x4c8b05 => 0x19c7c0,
            0x4c8b0d => 0x19c7c1,
            0x4c8b15 => 0x19c7c2,
            0x4c8b1d => 0x19c7c3,
            0x4c8b25 => 0x19c7c4,
            0x4c8b2d => 0x19c7c5,
            0x4c8b35 => 0x19c7c6,
            0x4c8b3d => 0x19c7c7,
            _ => 0,
        }
    }
}

fn relax_tlsdesc_to_ie(loc: &[u8], rel: &ElfRel) -> u32 {
    if loc.len() < 3 {
        return 0;
    }
    let _ = rel;
    match last3(loc) {
        // lea 0(%rip), %reg -> mov 0(%rip), %reg
        0x488d05 => 0x488b05,
        0x488d0d => 0x488b0d,
        0x488d15 => 0x488b15,
        0x488d1d => 0x488b1d,
        0x488d25 => 0x488b25,
        0x488d2d => 0x488b2d,
        0x488d35 => 0x488b35,
        0x488d3d => 0x488b3d,
        0x4c8d05 => 0x4c8b05,
        0x4c8d0d => 0x4c8b0d,
        0x4c8d15 => 0x4c8b15,
        0x4c8d1d => 0x4c8b1d,
        0x4c8d25 => 0x4c8b25,
        0x4c8d2d => 0x4c8b2d,
        0x4c8d35 => 0x4c8b35,
        0x4c8d3d => 0x4c8b3d,
        _ => 0,
    }
}

fn relax_tlsdesc_to_le(loc: &[u8], rel: &ElfRel) -> u32 {
    if loc.len() < 3 {
        return 0;
    }
    let insn = last3(loc);
    if rel.r_type == R_X86_64_GOTPC32_TLSDESC {
        match insn {
            // lea 0(%rip), %reg -> mov $0, %reg
            0x488d05 => 0x48c7c0,
            0x488d0d => 0x48c7c1,
            0x488d15 => 0x48c7c2,
            0x488d1d => 0x48c7c3,
            0x488d25 => 0x48c7c4,
            0x488d2d => 0x48c7c5,
            0x488d35 => 0x48c7c6,
            0x488d3d => 0x48c7c7,
            0x4c8d05 => 0x49c7c0,
            0x4c8d0d => 0x49c7c1,
            0x4c8d15 => 0x49c7c2,
            0x4c8d1d => 0x49c7c3,
            0x4c8d25 => 0x49c7c4,
            0x4c8d2d => 0x49c7c5,
            0x4c8d35 => 0x49c7c6,
            0x4c8d3d => 0x49c7c7,
            _ => 0,
        }
    } else {
        match insn {
            0x488d05 => 0x18c7c0,
            0x488d0d => 0x18c7c1,
            0x488d15 => 0x18c7c2,
            0x488d1d => 0x18c7c3,
            0x488d25 => 0x18c7c4,
            0x488d2d => 0x18c7c5,
            0x488d35 => 0x18c7c6,
            0x488d3d => 0x18c7c7,
            0x4c8d05 => 0x19c7c0,
            0x4c8d0d => 0x19c7c1,
            0x4c8d15 => 0x19c7c2,
            0x4c8d1d => 0x19c7c3,
            0x4c8d25 => 0x19c7c4,
            0x4c8d2d => 0x19c7c5,
            0x4c8d35 => 0x19c7c6,
            0x4c8d3d => 0x19c7c7,
            _ => 0,
        }
    }
}

/// Rewrites a `__tls_get_addr` call sequence to compute a link-time
/// constant TP-relative address instead.
fn relax_gd_to_le(buf: &mut [u8], off: usize, rel: &ElfRel, val: u64) {
    match rel.r_type {
        R_X86_64_PLT32 | R_X86_64_PC32 | R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX => {
            // lea foo@tlsgd(%rip), %rdi; call __tls_get_addr
            const INSN: [u8; 16] = [
                0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
                0x48, 0x81, 0xc0, 0, 0, 0, 0, // add $tp_offset, %rax
            ];
            buf[off - 4..off + 12].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 8..], val as u32);
        }
        R_X86_64_PLTOFF64 => {
            // lea foo@tlsgd(%rip), %rdi; movabs __tls_get_addr, %rax;
            // add %rbx, %rax; call *%rax
            const INSN: [u8; 22] = [
                0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
                0x48, 0x81, 0xc0, 0, 0, 0, 0, // add $tp_offset, %rax
                0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00, // nop
            ];
            buf[off - 3..off + 19].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 9..], val as u32);
        }
        _ => unreachable!(),
    }
}

fn relax_gd_to_ie(buf: &mut [u8], off: usize, rel: &ElfRel, val: u64) {
    match rel.r_type {
        R_X86_64_PLT32 | R_X86_64_PC32 | R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX => {
            const INSN: [u8; 16] = [
                0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
                0x48, 0x03, 0x05, 0, 0, 0, 0, // add foo@gottpoff(%rip), %rax
            ];
            buf[off - 4..off + 12].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 8..], val.wrapping_sub(12) as u32);
        }
        R_X86_64_PLTOFF64 => {
            const INSN: [u8; 22] = [
                0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
                0x48, 0x03, 0x05, 0, 0, 0, 0, // add foo@gottpoff(%rip), %rax
                0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00, // nop
            ];
            buf[off - 3..off + 19].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 9..], val.wrapping_sub(13) as u32);
        }
        _ => unreachable!(),
    }
}

/// Like `relax_gd_to_le`, but materializes the address of the TLS block
/// rather than of a particular variable.
fn relax_ld_to_le(buf: &mut [u8], off: usize, rel: &ElfRel, tls_size: u64) {
    match rel.r_type {
        R_X86_64_PLT32 | R_X86_64_PC32 => {
            // lea foo@tlsld(%rip), %rdi; call __tls_get_addr
            //
            // The sequence is so short that `mov %fs:0, %rax` (9 bytes)
            // doesn't fit; `xor %eax, %eax` plus `mov %fs:(%rax), %rax` does.
            const INSN: [u8; 12] = [
                0x31, 0xc0, // xor %eax, %eax
                0x64, 0x48, 0x8b, 0x00, // mov %fs:(%rax), %rax
                0x48, 0x2d, 0, 0, 0, 0, // sub $tls_size, %rax
            ];
            buf[off - 3..off + 9].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 5..], tls_size as u32);
        }
        R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX => {
            // lea foo@tlsld(%rip), %rdi; call *__tls_get_addr@GOT(%rip)
            const INSN: [u8; 13] = [
                0x48, 0x31, 0xc0, // xor %rax, %rax
                0x64, 0x48, 0x8b, 0x00, // mov %fs:(%rax), %rax
                0x48, 0x2d, 0, 0, 0, 0, // sub $tls_size, %rax
            ];
            buf[off - 3..off + 10].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 6..], tls_size as u32);
        }
        R_X86_64_PLTOFF64 => {
            // lea foo@tlsld(%rip), %rdi; movabs __tls_get_addr@GOTOFF, %rax;
            // add %rbx, %rax; call *%rax
            const INSN: [u8; 22] = [
                0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
                0x48, 0x2d, 0, 0, 0, 0, // sub $tls_size, %rax
                0x0f, 0x1f, 0x80, 0x00, 0x00, 0x00, 0x00, // nop
            ];
            buf[off - 3..off + 19].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 8..], tls_size as u32);
        }
        _ => unreachable!(),
    }
}

/// Rewrites `endbr64` landing pads of functions whose address is never
/// taken with NOPs. The compiler emits a landing pad for every global
/// function since it can't know whether the address is taken elsewhere;
/// the linker sees all translation units and can do better.
pub fn rewrite_endbr(ctx: &Context<X86_64>, buf: &mut [u8]) {
    const ENDBR64: [u8; 4] = [0xf3, 0x0f, 0x1e, 0xfa];
    const NOP: [u8; 4] = [0x0f, 0x1f, 0x40, 0x00];

    let output_offset = |isec: &InputSection| -> Option<u64> {
        let osec = &ctx.output_sections[isec.output_section?.index()];
        Some(osec.hdr.shdr.sh_offset + isec.offset())
    };

    // Rewrite the landing pads of all global functions. File-scoped
    // functions don't get one unless their address is taken anyway.
    for (fi, file) in ctx.objs.iter().enumerate() {
        for &id in file.base.global_symbols() {
            let sym = &ctx.symbols[id];
            if sym.file() != Some(FileId::Obj(crate::input_files::ObjId(fi as u32)))
                || sym.st_type() != STT_FUNC
            {
                continue;
            }
            let Some(isec) = sym.input_section_ref() else {
                continue;
            };
            if isec.sh_flags & SHF_EXECINSTR as u64 == 0 {
                continue;
            }
            let Some(base) = output_offset(isec) else {
                continue;
            };
            let pos = (base + sym.value) as usize;
            if buf.get(pos..pos + 4) == Some(&ENDBR64) {
                buf[pos..pos + 4].copy_from_slice(&NOP);
            }
        }
    }

    // Restore the landing pads that address-taking relocations refer to.
    let mut write_back = |isec: Option<&InputSection>, offset: i64| {
        let Some(isec) = isec else { return };
        let size = isec.contents().len() as i64;
        if isec.sh_flags & SHF_EXECINSTR as u64 == 0 || offset < 0 || offset > size - 4 {
            return;
        }
        let Some(base) = output_offset(isec) else {
            return;
        };
        if isec.contents()[offset as usize..offset as usize + 4] == ENDBR64 {
            let pos = (base as i64 + offset) as usize;
            buf[pos..pos + 4].copy_from_slice(&ENDBR64);
        }
    };

    for file in &ctx.objs {
        for isec in file.input_sections() {
            if !isec.is_alive() || !isec.is_alloc() {
                continue;
            }
            for rel in isec.rels::<X86_64>(file) {
                if rel.is_func_call::<X86_64>() {
                    continue;
                }
                let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
                let target = sym.input_section_ref();
                if sym.st_type() == STT_SECTION {
                    write_back(target, rel.r_addend);
                } else {
                    write_back(target, sym.value as i64);
                }
            }
        }
    }

    // Symbols whose addresses are recorded in the ELF header, .dynamic or
    // .dynsym keep their landing pads.
    let mut keep = |id: SymbolId| {
        let sym = &ctx.symbols[id];
        write_back(sym.input_section_ref(), sym.value as i64);
    };
    keep(ctx.syms.entry);
    keep(ctx.syms.init);
    keep(ctx.syms.fini);
    for &id in ctx.dynsym.symbols.iter().flatten() {
        if ctx.symbols[id].is_exported() {
            keep(id);
        }
    }
}

use crate::symbol::SymbolId;
