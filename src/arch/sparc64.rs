//! SPARC is a RISC ISA developed by Sun Microsystems.
//!
//! The byte order of the processor is big-endian. Anything larger than a
//! byte is stored in the "reverse" order compared to little-endian
//! processors such as x86-64.
//!
//! All instructions are 4 bytes long and aligned to 4 bytes boundaries.
//!
//! A notable feature of SPARC is that, unlike other RISC ISAs, it doesn't
//! need range extension thunks. It is because the SPARC's CALL instruction
//! contains a whopping 30 bits immediate. The processor scales it by 4 to
//! extend it to 32 bits (this is doable because all instructions are
//! aligned to 4 bytes boundaries, so the least significant two bits are
//! always zero). That means CALL's reach is PC ± 2 GiB, elinating the
//! need of range extension thunks. It comes with the cost that the CALL
//! instruction alone takes 1/4th of the instruction encoding space,
//! though.
//!
//! SPARC has 32 general purpose registers. CALL instruction saves a return
//! address to %o7, which is an alias for %r15. Thread pointer is stored to
//! %g7 which is %r7.
//!
//! SPARC does not have PC-relative load/store instructions. To access data
//! in the position-independent manner, we usually first set the address of
//! .got to, for example, %l7, with the following piece of code
//!
//!   sethi  %hi(. - _GLOBAL_OFFSET_TABLE_), %l7
//!   add  %l7, %lo(. - _GLOBAL_OFFSET_TABLE_), %l7
//!   call __sparc_get_pc_thunk.l7
//!   nop
//!
//! where __sparc_get_pc_thunk.l7 is defined as
//!
//!   retl
//!   add  %o7, %l7, %l7
//!
//! . SETHI and the following ADD materialize a 32 bits offset to .got.
//! CALL instruction sets a return address to $o7, and the subsequent ADD
//! adds it to the GOT offset to materialize the absolute address of .got.
//!
//! Note that we have a NOP after CALL and an ADD after RETL because of
//! SPARC's delay branch slots. That is, the SPARC processor always
//! executes one instruction after a branch even if the branch is taken.
//! This may seem like an odd behavior, and indeed it is considered as such
//! (that's a premature optimization for the early pipelined SPARC
//! processors), but that's been a part of the ISA's spec so that's what it
//! is.
//!
//! Note also that the .got address obtained this way is not shared between
//! functions, so functions can use an arbitrary register to hold the .got
//! address. That also means each function needs to execute the above piece
//! of code to become position-independent.
//!
//! https://github.com/rui314/psabi/blob/main/sparc.pdf
//!
//! Relocations carry a second addend in the upper 24 bits of the type
//! field; only `R_SPARC_OLO10` uses it.

use std::sync::atomic::Ordering;

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{check_tlsle, scan_absrel, scan_pcrel, InputSection};
use crate::output_chunks::eh_frame;
use crate::output_chunks::got::plt::SPARC_NUM_SMALL_PLT;
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::util::{bit, bits};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct Sparc64;

impl Layout for Sparc64 {
    type Endian = BigEndian;
    const IS_64: bool = true;
    const IS_RELA: bool = true;
}

// Target-specific ELF data types
/// The relocation type proper, without the second addend.
fn r_type(rel: &ElfRel) -> u32 {
    rel.r_type & 0xff
}

/// SPARC-specific: used for R_SPARC_OLO10
fn r_type_data(rel: &ElfRel) -> u64 {
    (rel.r_type >> 8) as u64
}

fn r32(loc: &[u8]) -> u32 {
    BigEndian::read_u32(loc)
}

fn w16(loc: &mut [u8], v: u64) {
    BigEndian::write_u16(loc, v as u16);
}

fn w32(loc: &mut [u8], v: u64) {
    BigEndian::write_u32(loc, v as u32);
}

fn w64(loc: &mut [u8], v: u64) {
    BigEndian::write_u64(loc, v);
}

fn or32(loc: &mut [u8], v: u64) {
    let cur = r32(loc);
    BigEndian::write_u32(loc, cur | v as u32);
}

/// The high 22 bits of a value, for `sethi`, with the one's complement
/// trick the HIX22 family uses for negative values.
fn hix22(val: i64) -> u64 {
    bits(if val < 0 { !val } else { val } as u64, 31, 10)
}

/// The low 10 bits with the sign-extension bits of a `LOX10`-style
/// immediate.
fn lox10(val: i64) -> u64 {
    bits(val as u64, 9, 0) | if val < 0 { 0b1_1100_0000_0000 } else { 0 }
}

// Returns the byte offset within .plt of the data pointer for a large SPARC
// PLT entry. See write_plt_entry below for the block layout this assumes.
pub fn plt_ptr_offset(num_plt_symbols: usize, plt_idx: u64) -> u64 {
    let i = plt_idx - SPARC_NUM_SMALL_PLT;
    let block = i / 160;
    let num_large = num_plt_symbols as u64 - SPARC_NUM_SMALL_PLT;
    let num_stubs = (num_large - block * 160).min(160);
    0x100000 + block * 5120 + num_stubs * 24 + (i % 160) * 8
}

impl Arch for Sparc64 {
    const NAME: &'static str = "sparc64";
    const FAMILY: Family = Family::Sparc64;
    const PAGE_SIZE: u64 = 8192;
    const E_MACHINE: u32 = EM_SPARC64;
    const PLT_HDR_SIZE: u64 = 128;
    const PLT_SIZE: u64 = 32;
    const PLTGOT_SIZE: u64 = 32;
    const TRAP: &'static [u8] = &[0x91, 0xd0, 0x20, 0x05]; // ta 5

    const R_COPY: u32 = R_SPARC_COPY;
    const R_GLOB_DAT: u32 = R_SPARC_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_SPARC_JMP_SLOT;
    const R_ABS: u32 = R_SPARC_64;
    const R_RELATIVE: u32 = R_SPARC_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_SPARC_IRELATIVE);
    const R_DTPOFF: u32 = R_SPARC_TLS_DTPOFF64;
    const R_TPOFF: u32 = R_SPARC_TLS_TPOFF64;
    const R_DTPMOD: u32 = R_SPARC_TLS_DTPMOD64;
    const R_FUNCALL: &'static [u32] = &[R_SPARC_WPLT30, R_SPARC_WDISP30];

    fn rel_to_string(r_type: u32) -> String {
        sparc64_rel_to_string(r_type & 0xff)
    }

    // SPARC's PLT section is writable despite containing executable code.
    // We don't need to write the PLT header entry because the dynamic loader
    // will do that for us.
    //
    // We also don't need a .got.plt section to store the result of lazy PLT
    // symbol resolution because the dynamic symbol resolver directly mutates
    // instructions in PLT so that they jump to the right places next time.
    // That's why each PLT entry contains lots of NOPs; they are a placeholder
    // for the runtime to add more instructions.
    //
    // Self-modifying code is nowadays considered really bad from the security
    // point of view, though.
    fn write_plt_header(_ctx: &Context<Self>, buf: &mut [u8]) {
        buf[..Self::PLT_HDR_SIZE as usize].fill(0);
    }

    // SPARC uses two PLT entry formats. A "small" entry branches directly to
    // the resolver stub (.PLT1) with a BPcc instruction whose reach is only
    // ±1 MiB. Once the PLT grows past that (0x100000 bytes), we switch to a
    // "large" format: rather than branching to the resolver, a large entry
    // loads a 64-bit value from a nearby data pointer and jumps to (that value
    // + its own address).
    //
    // Large entries are grouped into blocks of 160: each block is 160 code
    // stubs followed by 160 8-byte data pointers, one per stub. The stubs must
    // sit on a fixed grid, 24 bytes apart, because that is how the loader finds
    // them: on a lazy bind the stub jumps to the resolver (.PLT0) with only its
    // own address, and the loader derives which symbol to resolve from that
    // address. Nothing may sit between two stubs, so each stub's pointer lives
    // in the block's pointer region, which it reaches with a signed 13-bit ldx
    // offset (see to_plt_offset). This layout is dictated by the loader; we
    // cannot rearrange or simplify it.
    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let idx = sym.plt_idx(&ctx.symbols).unwrap() as u64;
        let plt = ctx.plt.hdr.shdr.sh_addr;
        let entry = sym.plt_addr(ctx);

        if idx < SPARC_NUM_SMALL_PLT {
            const INSN: [u32; 8] = [
                0x0300_0000, // sethi (. - .PLT0), %g1
                0x3068_0000, // ba,a  %xcc, .PLT1
                0x0100_0000, // nop
                0x0100_0000, // nop
                0x0100_0000, // nop
                0x0100_0000, // nop
                0x0100_0000, // nop
                0x0100_0000, // nop
            ];
            for (i, &insn) in INSN.iter().enumerate() {
                w32(&mut buf[i * 4..], insn as u64);
            }
            let plt1 = plt + Self::PLT_SIZE;
            or32(buf, bits(entry - plt, 21, 0));
            or32(
                &mut buf[4..],
                bits(plt1.wrapping_sub(entry).wrapping_sub(4), 20, 2),
            );
        } else {
            const INSN: [u32; 6] = [
                0x8a10_000f, // mov  %o7, %g5
                0x4000_0002, // call . + 8
                0x0100_0000, // nop
                0xc25b_e000, // ldx  [ %o7 + .ptr ], %g1
                0x83c3_c001, // jmpl %o7 + %g1, %g1
                0x9e10_0005, // mov  %g5, %o7
            ];
            for (i, &insn) in INSN.iter().enumerate() {
                w32(&mut buf[i * 4..], insn as u64);
            }
            let call = entry + 4;
            let ptroff = plt_ptr_offset(ctx.plt.symbols.len(), idx);
            or32(
                &mut buf[12..],
                bits((plt + ptroff).wrapping_sub(call), 12, 0),
            );

            // The data pointer initially holds (.PLT0 - call) so that the first call
            // jumps to .PLT0, where the loader's lazy resolver lives. The resolver
            // later overwrites it with (target - call).
            let ptr = (ptroff - (entry - plt)) as usize;
            w64(&mut buf[ptr..], plt.wrapping_sub(call));
        }
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        const INSN: [u32; 6] = [
            0x8a10_000f, // mov  %o7, %g5
            0x4000_0002, // call . + 8
            0xc25b_e014, // ldx  [ %o7 + 20 ], %g1
            0xc25b_c001, // ldx  [ %o7 + %g1 ], %g1
            0x81c0_4000, // jmp  %g1
            0x9e10_0005, // mov  %g5, %o7
        ]; // .quad $plt_entry - $got_entry
        for (i, &insn) in INSN.iter().enumerate() {
            w32(&mut buf[i * 4..], insn as u64);
        }
        w64(
            &mut buf[24..],
            sym.got_pltgot_addr(ctx)
                .wrapping_sub(sym.plt_addr(ctx))
                .wrapping_sub(4),
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
        match r_type(rel) {
            R_NONE => {}
            R_SPARC_64 | R_SPARC_UA64 => w64(loc, val),
            R_SPARC_DISP32 => {
                eh_frame::check_range(
                    ctx,
                    isec,
                    rel,
                    val.wrapping_sub(p) as i64,
                    -(1 << 31),
                    1 << 31,
                );
                w32(loc, val.wrapping_sub(p));
            }
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        let mut needs_tlsgd = false;

        // Scan relocations
        for rel in isec.rels::<Self>(file) {
            if rel.r_type == R_NONE || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT);
            }

            match r_type(&rel) {
                R_SPARC_8 | R_SPARC_5 | R_SPARC_6 | R_SPARC_7 | R_SPARC_10 | R_SPARC_11
                | R_SPARC_13 | R_SPARC_16 | R_SPARC_22 | R_SPARC_32 | R_SPARC_REGISTER
                | R_SPARC_UA16 | R_SPARC_UA32 | R_SPARC_PC_HM10 | R_SPARC_OLO10 | R_SPARC_LOX10
                | R_SPARC_HM10 | R_SPARC_M44 | R_SPARC_HIX22 | R_SPARC_LO10 | R_SPARC_L44
                | R_SPARC_LM22 | R_SPARC_HI22 | R_SPARC_H44 | R_SPARC_HH22 => {
                    scan_absrel(ctx, isec, sym, &rel)
                }
                R_SPARC_PLT32 | R_SPARC_WPLT30 | R_SPARC_WDISP30 | R_SPARC_HIPLT22
                | R_SPARC_LOPLT10 | R_SPARC_PCPLT32 | R_SPARC_PCPLT22 | R_SPARC_PCPLT10
                | R_SPARC_PLT64 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_SPARC_GOT13 | R_SPARC_GOT10 | R_SPARC_GOT22 | R_SPARC_GOTDATA_HIX22 => {
                    sym.add_flags(NEEDS_GOT)
                }
                R_SPARC_GOTDATA_OP_HIX22 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_GOT);
                    }
                }
                R_SPARC_DISP16 | R_SPARC_DISP32 | R_SPARC_DISP64 | R_SPARC_DISP8 | R_SPARC_PC10
                | R_SPARC_PC22 | R_SPARC_PC_LM22 | R_SPARC_WDISP16 | R_SPARC_WDISP19
                | R_SPARC_WDISP22 | R_SPARC_PC_HH22 => scan_pcrel(ctx, isec, sym, &rel),
                R_SPARC_TLS_GD_HI22 => {
                    // We always relax if -static because libc.a doesn't contain
                    // __tls_get_addr().
                    if ctx.args.is_static || (ctx.args.relax && sym.is_tprel_linktime_const(ctx)) {
                        // do nothing
                    } else if ctx.args.relax && sym.is_tprel_runtime_const(ctx) {
                        sym.add_flags(NEEDS_GOTTP);
                    } else {
                        sym.add_flags(NEEDS_TLSGD);
                        needs_tlsgd = true;
                    }
                }
                R_SPARC_TLS_LDM_HI22 => {
                    // We always relax if -static because libc.a doesn't contain
                    // __tls_get_addr().
                    if ctx.args.is_static || (ctx.args.relax && !ctx.args.shared) {
                        // do nothing
                    } else {
                        ctx.needs_tlsld.store(true, Ordering::Relaxed);
                    }
                }
                R_SPARC_TLS_IE_HI22 => sym.add_flags(NEEDS_GOTTP),
                R_SPARC_TLS_LE_HIX22 | R_SPARC_TLS_LE_LOX10 => check_tlsle(ctx, isec, sym, &rel),
                R_SPARC_64
                | R_SPARC_UA64
                | R_SPARC_GOTDATA_OP_LOX10
                | R_SPARC_GOTDATA_OP
                | R_SPARC_GOTDATA_LOX10
                | R_SPARC_TLS_GD_LO10
                | R_SPARC_TLS_GD_ADD
                | R_SPARC_TLS_GD_CALL
                | R_SPARC_TLS_LDM_LO10
                | R_SPARC_TLS_LDM_ADD
                | R_SPARC_TLS_LDM_CALL
                | R_SPARC_TLS_LDO_HIX22
                | R_SPARC_TLS_LDO_LOX10
                | R_SPARC_TLS_LDO_ADD
                | R_SPARC_TLS_IE_ADD
                | R_SPARC_TLS_IE_LD
                | R_SPARC_TLS_IE_LDX
                | R_SPARC_TLS_IE_LO10
                | R_SPARC_SIZE32 => {}
                _ => error!(
                    ctx,
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }

        // TLS_GD_CALL and TLS_LDM_CALL relocations implicitly refer to
        // __tls_get_addr, which may be dynamically linked from libc.so.
        if let Some(id) = ctx.syms.tls_get_addr {
            let sym = &ctx.symbols[id];
            if sym.is_imported() && (needs_tlsgd || ctx.needs_tlsld.load(Ordering::Relaxed)) {
                sym.add_flags(NEEDS_PLT);
            }
        }
    }

    fn apply_reloc_alloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        let got = ctx.got.hdr.shdr.sh_addr;
        let tls_get_addr =
            || ctx.symbols[ctx.syms.tls_get_addr.expect("SPARC has __tls_get_addr")].addr(ctx);

        // We iterate over relocations in reverse order so that it is easy
        // to swap instructions for R_SPARC_TLS_GD_CALL.
        for (i, rel) in isec.rels::<Self>(file).iter().enumerate().rev() {
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
            let pcrel = sa.wrapping_sub(p);
            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);

            // Register fields of the instruction being rewritten.
            let insn = r32(&buf[off..]);
            let rs1 = insn & (0b11111 << 14);
            let rs2 = insn & 0b11111;
            let rd = insn & (0b11111 << 25);
            let loc = &mut buf[off..];

            match r_type(&rel) {
                R_SPARC_5 => {
                    check(sa as i64, 0, 1 << 5);
                    or32(loc, bits(sa, 4, 0));
                }
                R_SPARC_6 => {
                    check(sa as i64, 0, 1 << 6);
                    or32(loc, bits(sa, 5, 0));
                }
                R_SPARC_7 => {
                    check(sa as i64, 0, 1 << 7);
                    or32(loc, bits(sa, 6, 0));
                }
                R_SPARC_8 => {
                    check(sa as i64, 0, 1 << 8);
                    loc[0] = sa as u8;
                }
                R_SPARC_10 => {
                    check(sa as i64, 0, 1 << 10);
                    or32(loc, bits(sa, 9, 0));
                }
                R_SPARC_LO10 | R_SPARC_LOPLT10 => or32(loc, bits(sa, 9, 0)),
                R_SPARC_11 => {
                    check(sa as i64, 0, 1 << 11);
                    or32(loc, bits(sa, 10, 0));
                }
                R_SPARC_13 => {
                    check(sa as i64, 0, 1 << 13);
                    or32(loc, bits(sa, 12, 0));
                }
                R_SPARC_16 | R_SPARC_UA16 => {
                    check(sa as i64, 0, 1 << 16);
                    w16(loc, sa);
                }
                R_SPARC_22 => {
                    check(sa as i64, 0, 1 << 22);
                    or32(loc, bits(sa, 21, 0));
                }
                R_SPARC_32 | R_SPARC_UA32 | R_SPARC_PLT32 => {
                    check(sa as i64, 0, 1 << 32);
                    w32(loc, sa);
                }
                R_SPARC_PLT64 | R_SPARC_REGISTER => w64(loc, sa),
                R_SPARC_DISP8 => {
                    check(pcrel as i64, -(1 << 7), 1 << 7);
                    loc[0] = pcrel as u8;
                }
                R_SPARC_DISP16 => {
                    check(pcrel as i64, -(1 << 15), 1 << 15);
                    w16(loc, pcrel);
                }
                R_SPARC_DISP32 | R_SPARC_PCPLT32 => {
                    check(pcrel as i64, -(1 << 31), 1 << 31);
                    w32(loc, pcrel);
                }
                R_SPARC_DISP64 => w64(loc, pcrel),
                R_SPARC_WDISP16 => {
                    check(pcrel as i64, -(1 << 16), 1 << 16);
                    let field = (bit(pcrel, 16) << 21) | bits(pcrel, 15, 2);
                    let cur = BigEndian::read_u16(loc);
                    w16(loc, cur as u64 | field);
                }
                R_SPARC_WDISP19 => {
                    check(pcrel as i64, -(1 << 20), 1 << 20);
                    or32(loc, bits(pcrel, 20, 2));
                }
                R_SPARC_WDISP22 => {
                    check(pcrel as i64, -(1 << 23), 1 << 23);
                    or32(loc, bits(pcrel, 23, 2));
                }
                R_SPARC_WDISP30 | R_SPARC_WPLT30 => {
                    if !sym.is_remaining_undef_weak() {
                        check(pcrel as i64, -(1 << 31), 1 << 31);
                    }
                    or32(loc, bits(pcrel, 31, 2));
                }
                R_SPARC_HI22 | R_SPARC_HIPLT22 | R_SPARC_LM22 => or32(loc, bits(sa, 31, 10)),
                R_SPARC_GOT10 => or32(loc, bits(g(), 9, 0)),
                R_SPARC_GOT13 => {
                    check(g() as i64, 0, 1 << 12);
                    or32(loc, bits(g(), 12, 0));
                }
                R_SPARC_GOT22 => or32(loc, bits(g(), 31, 10)),
                R_SPARC_GOTDATA_HIX22 => or32(loc, hix22(sa.wrapping_sub(got) as i64)),
                R_SPARC_GOTDATA_LOX10 => or32(loc, lox10(sa.wrapping_sub(got) as i64)),
                // We always have to relax a GOT load to a load immediate if a
                // symbol is local, because R_SPARC_GOTDATA_OP cannot represent
                // an addend for a local symbol.
                R_SPARC_GOTDATA_OP_HIX22 => {
                    if sym.is_absolute() {
                        or32(loc, hix22(sa as i64));
                    } else if sym.is_pcrel_linktime_const(ctx) {
                        or32(loc, hix22(sa.wrapping_sub(got) as i64));
                    } else {
                        or32(loc, bits(g(), 31, 10));
                    }
                }
                R_SPARC_GOTDATA_OP_LOX10 => {
                    if sym.is_absolute() {
                        or32(loc, lox10(sa as i64));
                    } else if sym.is_pcrel_linktime_const(ctx) {
                        or32(loc, lox10(sa.wrapping_sub(got) as i64));
                    } else {
                        or32(loc, bits(g(), 9, 0));
                    }
                }
                R_SPARC_GOTDATA_OP => {
                    if sym.is_absolute() {
                        // ldx [ %rs1 + %rs2 ], %rd  →  mov %rs2, %rd
                        w32(loc, (0x8010_0000 | rs2 | rd) as u64);
                    } else if sym.is_pcrel_linktime_const(ctx) {
                        // ldx [ %rs1 + %rs2 ], %rd  →  add %rs1, %rs2, %rd
                        w32(loc, (0x8000_0000 | rs1 | rs2 | rd) as u64);
                    }
                }
                R_SPARC_PC10 | R_SPARC_PCPLT10 => or32(loc, bits(pcrel, 9, 0)),
                R_SPARC_PC22 | R_SPARC_PCPLT22 | R_SPARC_PC_LM22 => or32(loc, bits(pcrel, 31, 10)),
                R_SPARC_OLO10 => or32(
                    loc,
                    bits(bits(sa, 9, 0).wrapping_add(r_type_data(&rel)), 12, 0),
                ),
                R_SPARC_HH22 => or32(loc, bits(sa, 63, 42)),
                R_SPARC_HM10 => or32(loc, bits(sa, 41, 32)),
                R_SPARC_PC_HH22 => or32(loc, bits(pcrel, 63, 42)),
                R_SPARC_PC_HM10 => or32(loc, bits(pcrel, 41, 32)),
                R_SPARC_HIX22 => or32(loc, bits(!sa, 31, 10)),
                R_SPARC_LOX10 => or32(loc, bits(sa, 9, 0) | 0b1_1100_0000_0000),
                R_SPARC_H44 => or32(loc, bits(sa, 43, 22)),
                R_SPARC_M44 => or32(loc, bits(sa, 21, 12)),
                R_SPARC_L44 => or32(loc, bits(sa, 11, 0)),
                R_SPARC_TLS_GD_HI22 => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        or32(
                            loc,
                            bits(
                                sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got),
                                31,
                                10,
                            ),
                        );
                    } else if sym.has_gottp(&ctx.symbols) {
                        or32(
                            loc,
                            bits(
                                sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got),
                                31,
                                10,
                            ),
                        );
                    } else {
                        or32(loc, bits(!sa.wrapping_sub(ctx.tp_addr), 31, 10));
                    }
                }
                R_SPARC_TLS_GD_LO10 => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        or32(
                            loc,
                            bits(sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got), 9, 0),
                        );
                    } else if sym.has_gottp(&ctx.symbols) {
                        // add %rs1, %rs2, %rd → or %rs1, $imm, %rd
                        w32(loc, (0x8010_2000 | rs1 | rd) as u64);
                        or32(
                            loc,
                            bits(sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got), 9, 0),
                        );
                    } else {
                        // add %rs1, %rs2, %rd → xor %rs1, $imm, %rd
                        w32(loc, (0x8018_2000 | rs1 | rd) as u64);
                        or32(
                            loc,
                            bits(sa.wrapping_sub(ctx.tp_addr), 9, 0) | 0b1_1100_0000_0000,
                        );
                    }
                }
                R_SPARC_TLS_GD_ADD => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        // do nothing
                    } else if sym.has_gottp(&ctx.symbols) {
                        // add %rs1, %rs2, %rd → ldx [ %rs1 + %rs2 ], %rd
                        w32(loc, (0xc058_0000 | rs1 | rs2 | rd) as u64);
                    } else {
                        // add %rs1, %rs2, %rd → add %g7, %rs2, %rd
                        w32(loc, (0x8001_c000 | rs2 | rd) as u64);
                    }
                }
                R_SPARC_TLS_GD_CALL => {
                    if sym.has_tlsgd(&ctx.symbols) {
                        or32(
                            loc,
                            bits(tls_get_addr().wrapping_add(a).wrapping_sub(p), 31, 2),
                        );
                    } else if sym.has_gottp(&ctx.symbols) {
                        // When we rewrite a branch instruction with a non-branch one,
                        // we need to swap the instruction and the following one so that
                        // the original execution order, which is inverted due to the
                        // branch delay slot, is preserved.
                        //
                        // Since we apply relocations from the end to the beginning,
                        // the instruction at loc + 4 is already complete.
                        let next = r32(&loc[4..]);
                        w32(loc, next as u64);
                        w32(&mut loc[4..], 0x9001_c008); // add %g7, %o0, %o0
                    } else {
                        w32(loc, 0x0100_0000); // call → nop
                    }
                }
                R_SPARC_TLS_LDM_HI22 => {
                    if ctx.got.has_tlsld() {
                        or32(
                            loc,
                            bits(
                                ctx.got
                                    .tlsld_addr::<Self>()
                                    .wrapping_add(a)
                                    .wrapping_sub(got),
                                31,
                                10,
                            ),
                        );
                    } else {
                        or32(loc, bits(ctx.tp_addr.wrapping_sub(ctx.tls_begin), 31, 10));
                    }
                }
                R_SPARC_TLS_LDM_LO10 => {
                    if ctx.got.has_tlsld() {
                        or32(
                            loc,
                            bits(
                                ctx.got
                                    .tlsld_addr::<Self>()
                                    .wrapping_add(a)
                                    .wrapping_sub(got),
                                9,
                                0,
                            ),
                        );
                    } else {
                        or32(loc, bits(ctx.tp_addr.wrapping_sub(ctx.tls_begin), 9, 0));
                    }
                }
                R_SPARC_TLS_LDM_ADD => {
                    if !ctx.got.has_tlsld() {
                        w32(loc, (0x8021_c000 | rs2 | rd) as u64); // sub %g7, %rs2, %rd
                    }
                }
                R_SPARC_TLS_LDM_CALL => {
                    if ctx.got.has_tlsld() {
                        or32(
                            loc,
                            bits(tls_get_addr().wrapping_add(a).wrapping_sub(p), 31, 2),
                        );
                    } else {
                        w32(loc, 0x0100_0000); // nop
                    }
                }
                R_SPARC_TLS_LDO_HIX22 => or32(loc, bits(sa.wrapping_sub(ctx.dtp_addr), 31, 10)),
                R_SPARC_TLS_LDO_LOX10 => or32(loc, bits(sa.wrapping_sub(ctx.dtp_addr), 9, 0)),
                R_SPARC_TLS_IE_HI22 => or32(
                    loc,
                    bits(
                        sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got),
                        31,
                        10,
                    ),
                ),
                R_SPARC_TLS_IE_LO10 => or32(
                    loc,
                    bits(sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(got), 9, 0),
                ),
                R_SPARC_TLS_LE_HIX22 => or32(loc, bits(!sa.wrapping_sub(ctx.tp_addr), 31, 10)),
                R_SPARC_TLS_LE_LOX10 => or32(
                    loc,
                    bits(sa.wrapping_sub(ctx.tp_addr), 9, 0) | 0b1_1100_0000_0000,
                ),
                R_SPARC_SIZE32 => w32(loc, sym.esym(ctx).st_size.wrapping_add(a)),
                R_SPARC_64 | R_SPARC_UA64 | R_SPARC_TLS_LDO_ADD | R_SPARC_TLS_IE_LD
                | R_SPARC_TLS_IE_LDX | R_SPARC_TLS_IE_ADD => {}
                _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        for (i, rel) in isec.relocations::<Self>(ctx).enumerate() {
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
            let loc = &mut buf[rel.r_offset as usize..];

            match r_type(&rel) {
                R_SPARC_64 | R_SPARC_UA64 => w64(
                    loc,
                    isec.tombstone(ctx, sym, frag.map(|(f, _)| f)).unwrap_or(sa),
                ),
                R_SPARC_32 | R_SPARC_UA32 => {
                    isec.check_range(ctx, i, sa as i64, 0, 1 << 32);
                    w32(loc, sa);
                }
                R_SPARC_TLS_DTPOFF32 => w32(loc, sa.wrapping_sub(ctx.dtp_addr)),
                R_SPARC_TLS_DTPOFF64 => w64(loc, sa.wrapping_sub(ctx.dtp_addr)),
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
