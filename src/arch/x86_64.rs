//! Supporting x86-64 is straightforward. Unlike its predecessor, i386,
//! x86-64 supports PC-relative addressing for position-independent code.
//! Being CISC, its instructions are variable in size. Branch instructions
//! take 4 bytes offsets, so we don't need range extension thunks.
//!
//! The psABI specifies %r11 as neither caller- nor callee-saved. It's
//! intentionally left out so that we can use it as a scratch register in
//! PLT.
//!
//! Thread Pointer (TP) is stored not to a general-purpose register but to
//! FS segment register. Segment register is a 64-bits register which can
//! be used as a base address for memory access. Each thread has a unique
//! FS value, and they access their thread-local variables relative to FS
//! as %fs:offset_from_tp.
//!
//! The value of a segment register itself is not generally readable from
//! the user space. As a workaround, libc initializes %fs:0 (the first word
//! referenced by FS) to the value of %fs itself. So we can obtain TP just
//! by `mov %fs:0, %rax` if we need it.
//!
//! For historical reasons, TP points past the end of the TLS block on x86.
//! This is contrary to other psABIs which usually use the beginning of the
//! TLS block as TP (with some addend). As a result, offsets from TP to
//! thread-local variables (TLVs) in the main executable are all negative.
//!
//! https://gitlab.com/x86-psABIs/x86-64-ABI

use crate::arch::{Arch, Family};
use crate::chunks::eh_frame;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::NonAllocReloc;
use crate::input_sections::{InputSection, check_tlsle, scan_absrel, scan_pcrel, scan_tlsdesc};
use crate::symbol::{NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD, Symbol};
use crate::util::endian::{LittleEndian, Ul64};
use crate::util::is_int;
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct X86_64;

impl Layout for X86_64 {
    type Endian = LittleEndian;
    type Word = Ul64;
    type Sym = Elf64Sym<LittleEndian>;
    type Phdr = Elf64Phdr<LittleEndian>;
    type Chdr = Elf64Chdr<LittleEndian>;
    type Rel = Elf64RelaLe;
}

impl Arch for X86_64 {
    type InputSectionExtra = ();

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

    fn rel_to_string(r_type: u32) -> std::borrow::Cow<'static, str> {
        x86_64_rel_to_string(r_type)
    }

    // This is a security-enhanced version of the regular PLT. The PLT
    // header and each PLT entry starts with endbr64 for the Intel's
    // control-flow enforcement security mechanism.
    //
    // Note that our IBT-enabled PLT instruction sequence is different
    // from the one used in GNU ld. GNU's IBTPLT implementation uses two
    // separate sections (.plt and .plt.sec) in which one PLT entry takes
    // 32 bytes in total. Our IBTPLT consists of just .plt and each entry
    // is 16 bytes long.
    //
    // Our PLT entry clobbers %r11, but that's fine because the resolver
    // function (_dl_runtime_resolve) clobbers %r11 anyway.
    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u8; 32] = [
            0xf3, 0x0f, 0x1e, 0xfa, // endbr64
            0x41, 0x53, // push %r11
            0xff, 0x35, 0, 0, 0, 0, // push GOTPLT+8(%rip)
            0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
            0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            0xcc, 0xcc, // (padding)
        ];
        buf[..32].copy_from_slice(&INSN);
        let gotplt = ctx.gotplt.shdr.sh_addr.get();
        let plt = ctx.plt.hdr.shdr.sh_addr.get();
        write_u32(&mut buf[8..], gotplt.wrapping_sub(plt).wrapping_sub(4) as u32);
        write_u32(&mut buf[14..], gotplt.wrapping_sub(plt).wrapping_sub(2) as u32);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let plt_idx = sym.plt_idx(&ctx.symbols).unwrap();
        let disp = sym.gotplt_addr(ctx).wrapping_sub(sym.plt_addr(ctx));
        // Only a canonical PLT can be address-taken; there's no way to take
        // an address of a non-canonical PLT. Therefore, a non-canonical PLT
        // doesn't have to start with an endbr64.
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
                0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            ];
            buf[..16].copy_from_slice(&INSN);
            write_u32(&mut buf[2..], plt_idx);
            write_u32(&mut buf[8..], disp.wrapping_sub(12) as u32);
        }
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u8; 8] = [
            0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
            0xcc, 0xcc, // (padding)
        ];
        buf[..8].copy_from_slice(&INSN);
        let disp = sym.got_pltgot_addr(ctx).wrapping_sub(sym.plt_addr(ctx)).wrapping_sub(6);
        write_u32(&mut buf[2..], disp as u32);
    }

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        isec: &InputSection<Self>,
        rel: &Self::Rel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        let check = |val: i64, lo: i64, hi: i64| eh_frame::check_range(ctx, isec, rel, val, lo, hi);
        match rel.r_type() {
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
            _ => eh_frame::unsupported::<Self>(rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection<Self>) {
        // Linker has to create data structures in an output file to apply
        // some type of relocations. For example, if a relocation refers a GOT
        // or a PLT entry of a symbol, linker has to create an entry in .got
        // or in .plt for that symbol. In order to fix the file layout, we
        // need to scan relocations.
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        let rels = isec.rels(file);
        let mut i = 0;

        // Scan relocations
        while i < rels.len() {
            let rel = &rels[i];
            i += 1;
            if rel.r_type() == R_NONE || isec.record_undef_error_with_file(ctx, file, rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];

            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            if rel.r_type() == R_X86_64_TLSGD || rel.r_type() == R_X86_64_TLSLD {
                let next = rels.get(i).map(|r| r.r_type());
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
                        "{}: {} must be followed by PLT or GOTPCREL",
                        isec.display(file),
                        rel.type_name::<Self>()
                    );
                }
            }

            match rel.r_type() {
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
                    if ctx.args.is_static || (ctx.args.relax && sym.is_tprel_linktime_const(ctx)) {
                        // We always relax if -static because libc.a doesn't contain
                        // __tls_get_addr().
                        i += 1;
                    } else if ctx.args.relax && sym.is_tprel_runtime_const(ctx) {
                        sym.add_flags(NEEDS_GOTTP);
                        i += 1;
                    } else {
                        sym.add_flags(NEEDS_TLSGD);
                    }
                }
                R_X86_64_TLSLD => {
                    // We always relax if -static because libc.a doesn't contain
                    // __tls_get_addr().
                    if ctx.args.is_static || (ctx.args.relax && !ctx.args.shared) {
                        i += 1;
                    } else {
                        ctx.needs_tlsld.store(true, std::sync::atomic::Ordering::Relaxed);
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
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    // Apply relocations to SHF_ALLOC sections (i.e. sections that are
    // mapped to memory at runtime) based on the result of
    // scan_relocations().
    fn apply_reloc_alloc(
        ctx: &Context<Self>,
        isec: &InputSection<Self>,
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
            let a = rel.r_addend() as u64;
            let p = isec.addr(ctx) + rel.r_offset();
            let got_base = ctx.gotplt.shdr.sh_addr.get();
            let g = if sym.has_got(&ctx.symbols) {
                sym.got_addr(ctx).wrapping_sub(got_base)
            } else {
                0
            };

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, rel_idx, val, lo, hi);
            let write32 = |buf: &mut [u8], val: u64| {
                check(val as i64, 0, 1 << 32);
                write_u32(&mut buf[off..], val as u32);
            };
            let write32s = |buf: &mut [u8], val: u64| {
                check(val as i64, -(1 << 31), 1 << 31);
                write_u32(&mut buf[off..], val as u32);
            };

            match rel.r_type() {
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
                R_X86_64_GOTPCREL => {
                    write32s(buf, g.wrapping_add(got_base).wrapping_add(a).wrapping_sub(p))
                }
                R_X86_64_GOTPCREL64 => write_u64(
                    &mut buf[off..],
                    g.wrapping_add(got_base).wrapping_add(a).wrapping_sub(p),
                ),
                R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX | R_X86_64_CODE_4_GOTPCRELX => {
                    // We always want to relax GOTPCRELX relocs even if --no-relax
                    // was given because some static PIE runtime code depends on these
                    // relaxations.
                    let v = s.wrapping_add(a).wrapping_sub(p);
                    if sym.is_pcrel_linktime_const(ctx) && is_int(v as i64, 32) {
                        let insn = relax_gotpcrelx(&buf[..off], &rel);
                        if insn != 0 {
                            buf[off - 2] = (insn >> 8) as u8;
                            buf[off - 1] = insn as u8;
                            write_u32(&mut buf[off..], v as u32);
                            if ctx.args.emit_relocs {
                                rels[rel_idx].set_r_type(R_X86_64_PC32);
                            }
                            continue;
                        }
                    }
                    write32s(buf, g.wrapping_add(got_base).wrapping_add(a).wrapping_sub(p));
                }
                R_X86_64_TLSGD => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        write32s(buf, sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else if sym.has_gottp(&ctx.symbols) {
                        let next = &rels[i];
                        i += 1;
                        relax_gd_to_ie(buf, off, next, sym.gottp_addr(ctx).wrapping_sub(p));
                    } else {
                        let next = &rels[i];
                        i += 1;
                        relax_gd_to_le(buf, off, next, s.wrapping_sub(ctx.tp_addr));
                    }
                }
                R_X86_64_TLSLD => {
                    if ctx.got.has_tlsld() {
                        write32s(buf, ctx.got.tlsld_addr().wrapping_add(a).wrapping_sub(p));
                    } else {
                        let next = &rels[i];
                        i += 1;
                        relax_ld_to_le(buf, off, next, ctx.tp_addr.wrapping_sub(ctx.tls_begin));
                    }
                }
                R_X86_64_DTPOFF32 => write32s(buf, s.wrapping_add(a).wrapping_sub(ctx.dtp_addr)),
                R_X86_64_DTPOFF64 => {
                    write_u64(&mut buf[off..], s.wrapping_add(a).wrapping_sub(ctx.dtp_addr))
                }
                R_X86_64_TPOFF32 => write32s(buf, s.wrapping_add(a).wrapping_sub(ctx.tp_addr)),
                R_X86_64_TPOFF64 => {
                    write_u64(&mut buf[off..], s.wrapping_add(a).wrapping_sub(ctx.tp_addr))
                }
                R_X86_64_GOTTPOFF | R_X86_64_CODE_4_GOTTPOFF => {
                    if sym.has_gottp(&ctx.symbols) {
                        write32s(buf, sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else {
                        let insn = relax_gottpoff(&buf[..off], &rel);
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
                    // x86-64 TLSDESC uses the following code sequence to materialize
                    // a TP-relative address in %rax.
                    //
                    //   lea    0(%rip), %rax
                    //       R_X86_64_GOTPC32_TLSDESC    foo
                    //   call   *(%rax)
                    //       R_X86_64_TLSDESC_CALL       foo
                    //
                    // We may relax the instructions to the following if its TP-relative
                    // address is known at link-time
                    //
                    //   mov     $foo@TPOFF, %rax
                    //   nop
                    //
                    // or to the following if the TP-relative address is known at
                    // process startup time.
                    //
                    //   mov     foo@GOTTPOFF(%rip), %rax
                    //   nop
                    //
                    // We allow the following alternative code sequence too because
                    // LLVM emits such code.
                    //
                    //   lea    0(%rip), %reg
                    //       R_X86_64_GOTPC32_TLSDESC    foo
                    //   mov    %reg, %rax
                    //   call   *(%rax)
                    //       R_X86_64_TLSDESC_CALL       foo
                    if sym.has_tlsdesc(&ctx.symbols) {
                        write32s(buf, sym.tlsdesc_addr(ctx).wrapping_add(a).wrapping_sub(p));
                    } else if sym.has_gottp(&ctx.symbols) {
                        let insn = relax_tlsdesc_to_ie(&buf[..off], &rel);
                        if insn == 0 {
                            fatal!(
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
                        let insn = relax_tlsdesc_to_le(&buf[..off], &rel);
                        if insn == 0 {
                            fatal!(
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
                R_X86_64_SIZE32 => write32(buf, sym.esym(ctx).st_size().wrapping_add(a)),
                R_X86_64_SIZE64 => {
                    write_u64(&mut buf[off..], sym.esym(ctx).st_size().wrapping_add(a))
                }
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    // This function is responsible for applying relocations against
    // non-SHF_ALLOC sections (i.e. sections that are not mapped to memory
    // at runtime).
    //
    // Relocations against non-SHF_ALLOC sections are much easier to
    // handle than that against SHF_ALLOC sections. It is because, since
    // they are not mapped to memory, they don't contain any variable or
    // function and never need PLT or GOT. Non-SHF_ALLOC sections are
    // mostly debug info sections.
    //
    // Relocations against non-SHF_ALLOC sections are not scanned by
    // scan_relocations.
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
            let write32 = |buf: &mut [u8], val: u64| {
                check(val as i64, 0, 1 << 32);
                write_u32(&mut buf[off..], val as u32);
            };
            let write32s = |buf: &mut [u8], val: u64| {
                check(val as i64, -(1 << 31), 1 << 31);
                write_u32(&mut buf[off..], val as u32);
            };

            match rel.r_type() {
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
                R_X86_64_64 => match isec.tombstone_with_file(ctx, file, sym, frag) {
                    Some(v) => write_u64(&mut buf[off..], v),
                    None => write_u64(&mut buf[off..], s.wrapping_add(a)),
                },
                R_X86_64_DTPOFF32 => match isec.tombstone_with_file(ctx, file, sym, frag) {
                    Some(v) => write_u32(&mut buf[off..], v as u32),
                    None => write32s(buf, s.wrapping_add(a).wrapping_sub(ctx.dtp_addr)),
                },
                R_X86_64_DTPOFF64 => match isec.tombstone_with_file(ctx, file, sym, frag) {
                    Some(v) => write_u64(&mut buf[off..], v),
                    None => {
                        write_u64(&mut buf[off..], s.wrapping_add(a).wrapping_sub(ctx.dtp_addr))
                    }
                },
                R_X86_64_GOTOFF64 => write_u64(
                    &mut buf[off..],
                    s.wrapping_add(a).wrapping_sub(ctx.gotplt.shdr.sh_addr.get()),
                ),
                R_X86_64_GOTPC64 => {
                    // PC-relative relocation doesn't make sense for non-memory-allocated
                    // section, but GCC 6.3.0 seems to create this reloc for
                    // _GLOBAL_OFFSET_TABLE_.
                    write_u64(&mut buf[off..], ctx.gotplt.shdr.sh_addr.get().wrapping_add(a))
                }
                R_X86_64_SIZE32 => write32(buf, sym.esym(ctx).st_size().wrapping_add(a)),
                R_X86_64_SIZE64 => {
                    write_u64(&mut buf[off..], sym.esym(ctx).st_size().wrapping_add(a))
                }
                _ => fatal!(
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
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
fn loc_before<'a>(isec: &'a InputSection<X86_64>, rel: &ElfRel<X86_64>) -> &'a [u8] {
    &isec.contents()[..rel.r_offset() as usize]
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
fn relax_gotpcrelx(loc: &[u8], rel: &ElfRel<X86_64>) -> u32 {
    if rel.r_type() == R_X86_64_GOTPCRELX {
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
        0x488b05 => 0x8d05, // mov 0(%rip), %rax -> lea 0(%rip), %rax
        0x488b0d => 0x8d0d, // mov 0(%rip), %rcx -> lea 0(%rip), %rcx
        0x488b15 => 0x8d15, // mov 0(%rip), %rdx -> lea 0(%rip), %rdx
        0x488b1d => 0x8d1d, // mov 0(%rip), %rbx -> lea 0(%rip), %rbx
        0x488b25 => 0x8d25, // mov 0(%rip), %rsp -> lea 0(%rip), %rsp
        0x488b2d => 0x8d2d, // mov 0(%rip), %rbp -> lea 0(%rip), %rbp
        0x488b35 => 0x8d35, // mov 0(%rip), %rsi -> lea 0(%rip), %rsi
        0x488b3d => 0x8d3d, // mov 0(%rip), %rdi -> lea 0(%rip), %rdi
        0x4c8b05 => 0x8d05, // mov 0(%rip), %r8  -> lea 0(%rip), %r8
        0x4c8b0d => 0x8d0d, // mov 0(%rip), %r9  -> lea 0(%rip), %r9
        0x4c8b15 => 0x8d15, // mov 0(%rip), %r10 -> lea 0(%rip), %r10
        0x4c8b1d => 0x8d1d, // mov 0(%rip), %r11 -> lea 0(%rip), %r11
        0x4c8b25 => 0x8d25, // mov 0(%rip), %r12 -> lea 0(%rip), %r12
        0x4c8b2d => 0x8d2d, // mov 0(%rip), %r13 -> lea 0(%rip), %r13
        0x4c8b35 => 0x8d35, // mov 0(%rip), %r14 -> lea 0(%rip), %r14
        0x4c8b3d => 0x8d3d, // mov 0(%rip), %r15 -> lea 0(%rip), %r15
        _ => 0,
    }
}

fn relax_gottpoff(loc: &[u8], rel: &ElfRel<X86_64>) -> u32 {
    if loc.len() < 3 {
        return 0;
    }
    let insn = last3(loc);
    if rel.r_type() == R_X86_64_GOTTPOFF {
        match insn {
            0x488b05 => 0x48c7c0, // mov 0(%rip), %rax -> mov $0, %rax
            0x488b0d => 0x48c7c1, // mov 0(%rip), %rcx -> mov $0, %rcx
            0x488b15 => 0x48c7c2, // mov 0(%rip), %rdx -> mov $0, %rdx
            0x488b1d => 0x48c7c3, // mov 0(%rip), %rbx -> mov $0, %rbx
            0x488b25 => 0x48c7c4, // mov 0(%rip), %rsp -> mov $0, %rsp
            0x488b2d => 0x48c7c5, // mov 0(%rip), %rbp -> mov $0, %rbp
            0x488b35 => 0x48c7c6, // mov 0(%rip), %rsi -> mov $0, %rsi
            0x488b3d => 0x48c7c7, // mov 0(%rip), %rdi -> mov $0, %rdi
            0x4c8b05 => 0x49c7c0, // mov 0(%rip), %r8  -> mov $0, %r8
            0x4c8b0d => 0x49c7c1, // mov 0(%rip), %r9  -> mov $0, %r9
            0x4c8b15 => 0x49c7c2, // mov 0(%rip), %r10 -> mov $0, %r10
            0x4c8b1d => 0x49c7c3, // mov 0(%rip), %r11 -> mov $0, %r11
            0x4c8b25 => 0x49c7c4, // mov 0(%rip), %r12 -> mov $0, %r12
            0x4c8b2d => 0x49c7c5, // mov 0(%rip), %r13 -> mov $0, %r13
            0x4c8b35 => 0x49c7c6, // mov 0(%rip), %r14 -> mov $0, %r14
            0x4c8b3d => 0x49c7c7, // mov 0(%rip), %r15 -> mov $0, %r15
            _ => 0,
        }
    } else {
        debug_assert_eq!(rel.r_type(), R_X86_64_CODE_4_GOTTPOFF);
        match insn {
            0x488b05 => 0x18c7c0, // mov 0(%rip), %r16 -> mov $0, %r16
            0x488b0d => 0x18c7c1, // mov 0(%rip), %r17 -> mov $0, %r17
            0x488b15 => 0x18c7c2, // mov 0(%rip), %r18 -> mov $0, %r18
            0x488b1d => 0x18c7c3, // mov 0(%rip), %r19 -> mov $0, %r19
            0x488b25 => 0x18c7c4, // mov 0(%rip), %r20 -> mov $0, %r20
            0x488b2d => 0x18c7c5, // mov 0(%rip), %r21 -> mov $0, %r21
            0x488b35 => 0x18c7c6, // mov 0(%rip), %r22 -> mov $0, %r22
            0x488b3d => 0x18c7c7, // mov 0(%rip), %r23 -> mov $0, %r23
            0x4c8b05 => 0x19c7c0, // mov 0(%rip), %r24 -> mov $0, %r24
            0x4c8b0d => 0x19c7c1, // mov 0(%rip), %r25 -> mov $0, %r25
            0x4c8b15 => 0x19c7c2, // mov 0(%rip), %r26 -> mov $0, %r26
            0x4c8b1d => 0x19c7c3, // mov 0(%rip), %r27 -> mov $0, %r27
            0x4c8b25 => 0x19c7c4, // mov 0(%rip), %r28 -> mov $0, %r28
            0x4c8b2d => 0x19c7c5, // mov 0(%rip), %r29 -> mov $0, %r29
            0x4c8b35 => 0x19c7c6, // mov 0(%rip), %r30 -> mov $0, %r30
            0x4c8b3d => 0x19c7c7, // mov 0(%rip), %r31 -> mov $0, %r31
            _ => 0,
        }
    }
}

fn relax_tlsdesc_to_ie(loc: &[u8], rel: &ElfRel<X86_64>) -> u32 {
    if loc.len() < 3 {
        return 0;
    }
    let _ = rel;
    match last3(loc) {
        // lea 0(%rip), %r16 -> mov 0(%rip), %r16
        0x488d05 => 0x488b05, // lea 0(%rip), %rax -> mov 0(%rip), %rax
        // lea 0(%rip), %r17 -> mov 0(%rip), %r17
        0x488d0d => 0x488b0d, // lea 0(%rip), %rcx -> mov 0(%rip), %rcx
        // lea 0(%rip), %r18 -> mov 0(%rip), %r18
        0x488d15 => 0x488b15, // lea 0(%rip), %rdx -> mov 0(%rip), %rdx
        // lea 0(%rip), %r19 -> mov 0(%rip), %r19
        0x488d1d => 0x488b1d, // lea 0(%rip), %rbx -> mov 0(%rip), %rbx
        // lea 0(%rip), %r20 -> mov 0(%rip), %r20
        0x488d25 => 0x488b25, // lea 0(%rip), %rsp -> mov 0(%rip), %rsp
        // lea 0(%rip), %r21 -> mov 0(%rip), %r21
        0x488d2d => 0x488b2d, // lea 0(%rip), %rbp -> mov 0(%rip), %rbp
        // lea 0(%rip), %r22 -> mov 0(%rip), %r22
        0x488d35 => 0x488b35, // lea 0(%rip), %rsi -> mov 0(%rip), %rsi
        // lea 0(%rip), %r23 -> mov 0(%rip), %r23
        0x488d3d => 0x488b3d, // lea 0(%rip), %rdi -> mov 0(%rip), %rdi
        // lea 0(%rip), %r24 -> mov 0(%rip), %r24
        0x4c8d05 => 0x4c8b05, // lea 0(%rip), %r8  -> mov 0(%rip), %r8
        // lea 0(%rip), %r25 -> mov 0(%rip), %r25
        0x4c8d0d => 0x4c8b0d, // lea 0(%rip), %r9  -> mov 0(%rip), %r9
        // lea 0(%rip), %r26 -> mov 0(%rip), %r26
        0x4c8d15 => 0x4c8b15, // lea 0(%rip), %r10 -> mov 0(%rip), %r10
        // lea 0(%rip), %r27 -> mov 0(%rip), %r27
        0x4c8d1d => 0x4c8b1d, // lea 0(%rip), %r11 -> mov 0(%rip), %r11
        // lea 0(%rip), %r28 -> mov 0(%rip), %r28
        0x4c8d25 => 0x4c8b25, // lea 0(%rip), %r12 -> mov 0(%rip), %r12
        // lea 0(%rip), %r29 -> mov 0(%rip), %r29
        0x4c8d2d => 0x4c8b2d, // lea 0(%rip), %r13 -> mov 0(%rip), %r13
        // lea 0(%rip), %r30 -> mov 0(%rip), %r30
        0x4c8d35 => 0x4c8b35, // lea 0(%rip), %r14 -> mov 0(%rip), %r14
        // lea 0(%rip), %r31 -> mov 0(%rip), %r31
        0x4c8d3d => 0x4c8b3d, // lea 0(%rip), %r15 -> mov 0(%rip), %r15
        _ => 0,
    }
}

fn relax_tlsdesc_to_le(loc: &[u8], rel: &ElfRel<X86_64>) -> u32 {
    if loc.len() < 3 {
        return 0;
    }
    let insn = last3(loc);
    if rel.r_type() == R_X86_64_GOTPC32_TLSDESC {
        match insn {
            0x488d05 => 0x48c7c0, // lea 0(%rip), %rax -> mov $0, %rax
            0x488d0d => 0x48c7c1, // lea 0(%rip), %rcx -> mov $0, %rcx
            0x488d15 => 0x48c7c2, // lea 0(%rip), %rdx -> mov $0, %rdx
            0x488d1d => 0x48c7c3, // lea 0(%rip), %rbx -> mov $0, %rbx
            0x488d25 => 0x48c7c4, // lea 0(%rip), %rsp -> mov $0, %rsp
            0x488d2d => 0x48c7c5, // lea 0(%rip), %rbp -> mov $0, %rbp
            0x488d35 => 0x48c7c6, // lea 0(%rip), %rsi -> mov $0, %rsi
            0x488d3d => 0x48c7c7, // lea 0(%rip), %rdi -> mov $0, %rdi
            0x4c8d05 => 0x49c7c0, // lea 0(%rip), %r8  -> mov $0, %r8
            0x4c8d0d => 0x49c7c1, // lea 0(%rip), %r9  -> mov $0, %r9
            0x4c8d15 => 0x49c7c2, // lea 0(%rip), %r10 -> mov $0, %r10
            0x4c8d1d => 0x49c7c3, // lea 0(%rip), %r11 -> mov $0, %r11
            0x4c8d25 => 0x49c7c4, // lea 0(%rip), %r12 -> mov $0, %r12
            0x4c8d2d => 0x49c7c5, // lea 0(%rip), %r13 -> mov $0, %r13
            0x4c8d35 => 0x49c7c6, // lea 0(%rip), %r14 -> mov $0, %r14
            0x4c8d3d => 0x49c7c7, // lea 0(%rip), %r15 -> mov $0, %r15
            _ => 0,
        }
    } else {
        match insn {
            0x488d05 => 0x18c7c0, // lea 0(%rip), %r16 -> mov $0, %r16
            0x488d0d => 0x18c7c1, // lea 0(%rip), %r17 -> mov $0, %r17
            0x488d15 => 0x18c7c2, // lea 0(%rip), %r18 -> mov $0, %r18
            0x488d1d => 0x18c7c3, // lea 0(%rip), %r19 -> mov $0, %r19
            0x488d25 => 0x18c7c4, // lea 0(%rip), %r20 -> mov $0, %r20
            0x488d2d => 0x18c7c5, // lea 0(%rip), %r21 -> mov $0, %r21
            0x488d35 => 0x18c7c6, // lea 0(%rip), %r22 -> mov $0, %r22
            0x488d3d => 0x18c7c7, // lea 0(%rip), %r23 -> mov $0, %r23
            0x4c8d05 => 0x19c7c0, // lea 0(%rip), %r24 -> mov $0, %r24
            0x4c8d0d => 0x19c7c1, // lea 0(%rip), %r25 -> mov $0, %r25
            0x4c8d15 => 0x19c7c2, // lea 0(%rip), %r26 -> mov $0, %r26
            0x4c8d1d => 0x19c7c3, // lea 0(%rip), %r27 -> mov $0, %r27
            0x4c8d25 => 0x19c7c4, // lea 0(%rip), %r28 -> mov $0, %r28
            0x4c8d2d => 0x19c7c5, // lea 0(%rip), %r29 -> mov $0, %r29
            0x4c8d35 => 0x19c7c6, // lea 0(%rip), %r30 -> mov $0, %r30
            0x4c8d3d => 0x19c7c7, // lea 0(%rip), %r31 -> mov $0, %r31
            _ => 0,
        }
    }
}

// Rewrite a function call to __tls_get_addr to a cheaper instruction
// sequence. We can do this when we know the thread-local variable's TP-
// relative address at link-time.
fn relax_gd_to_le(buf: &mut [u8], off: usize, rel: &ElfRel<X86_64>, val: u64) {
    match rel.r_type() {
        R_X86_64_PLT32 | R_X86_64_PC32 | R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX => {
            // The original instructions are the following:
            //
            //  66 48 8d 3d 00 00 00 00    lea  foo@tlsgd(%rip), %rdi
            //  66 66 48 e8 00 00 00 00    call __tls_get_addr
            //
            // or
            //
            //  66 48 8d 3d 00 00 00 00    lea foo@tlsgd(%rip), %rdi
            //  66 48 ff 15 00 00 00 00    call *__tls_get_addr@GOT(%rip)
            const INSN: [u8; 16] = [
                0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
                0x48, 0x81, 0xc0, 0, 0, 0, 0, // add $tp_offset, %rax
            ];
            buf[off - 4..off + 12].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 8..], val as u32);
        }
        R_X86_64_PLTOFF64 => {
            // The original instructions are the following:
            //
            //  48 8d 3d 00 00 00 00           lea    foo@tlsgd(%rip), %rdi
            //  48 b8 00 00 00 00 00 00 00 00  movabs __tls_get_addr, %rax
            //  48 01 d8                       add    %rbx, %rax
            //  ff d0                          call   *%rax
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

fn relax_gd_to_ie(buf: &mut [u8], off: usize, rel: &ElfRel<X86_64>, val: u64) {
    match rel.r_type() {
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

// Rewrite a function call to __tls_get_addr to a cheaper instruction
// sequence. The difference from relax_gd_to_le is that we are materializing
// the address of the beginning of TLS block instead of an address of a
// particular thread-local variable.
fn relax_ld_to_le(buf: &mut [u8], off: usize, rel: &ElfRel<X86_64>, tls_size: u64) {
    match rel.r_type() {
        R_X86_64_PLT32 | R_X86_64_PC32 => {
            // The original instructions are the following:
            //
            //  48 8d 3d 00 00 00 00    lea    foo@tlsld(%rip), %rdi
            //  e8 00 00 00 00          call   __tls_get_addr
            //
            // Because the original instruction sequence is so short that we need a
            // little bit of code golfing here. "mov %fs:0, %rax" is 9 byte long, so
            // xor + mov is shorter. Note that `xor %eax, %eax` zero-clears %eax.
            const INSN: [u8; 12] = [
                0x31, 0xc0, // xor %eax, %eax
                0x64, 0x48, 0x8b, 0x00, // mov %fs:(%rax), %rax
                0x48, 0x2d, 0, 0, 0, 0, // sub $tls_size, %rax
            ];
            buf[off - 3..off + 9].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 5..], tls_size as u32);
        }
        R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX => {
            // The original instructions are the following:
            //
            //  48 8d 3d 00 00 00 00    lea    foo@tlsld(%rip), %rdi
            //  ff 15 00 00 00 00       call   *__tls_get_addr@GOT(%rip)
            const INSN: [u8; 13] = [
                0x48, 0x31, 0xc0, // xor %rax, %rax
                0x64, 0x48, 0x8b, 0x00, // mov %fs:(%rax), %rax
                0x48, 0x2d, 0, 0, 0, 0, // sub $tls_size, %rax
            ];
            buf[off - 3..off + 10].copy_from_slice(&INSN);
            write_u32(&mut buf[off + 6..], tls_size as u32);
        }
        R_X86_64_PLTOFF64 => {
            // The original instructions are the following:
            //
            //  48 8d 3d 00 00 00 00           lea    foo@tlsld(%rip), %rdi
            //  48 b8 00 00 00 00 00 00 00 00  movabs __tls_get_addr@GOTOFF, %rax
            //  48 01 d8                       add    %rbx, %rax
            //  ff d0                          call   *%rax
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
