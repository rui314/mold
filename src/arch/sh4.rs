//! SH-4 (SuperH 4) is a 32-bit RISC ISA developed by Hitachi in the early
//! '90s. Some relatively powerful systems were developed with SH-4.
//! A notable example is Sega's Dreamcast game console which debuted in 1998.
//! Hitachi later spun off its semiconductor division as an independent
//! company, Renesas, and Renesas is still selling SH-4 processors for the
//! embedded market. It has never been as popular as ARM is, and its
//! popularity continues to decline though.
//!
//! SH-4's most distinctive feature compared to other RISC ISAs is that its
//! instructions are 16 bits in length instead of more common 32 bits for
//! better code density. This difference affects various aspects of its
//! instruction set as shown below:
//!
//!  - SH-4 has 16 general-purpose registers (GPRs) instead of the most
//!    commmon 32 GPR configuration to save one bit to specify a register.
//!
//!  - Binary instructions such as ADD normally take three register in
//!    RISC ISAs (e.g. x ← y ⊕ z where x, y and z are registers), but
//!    SH-4's instructions take only two registers. The result of an
//!    operation is written to one of the source registers (e.g. x ← x ⊕ y).
//!
//!  - Usual RISC ISAs have "load high" and "load low" instructions to set
//!    an immediate to most significant and least significant bits in a
//!    register to construct a full 32-bit value in a register. This
//!    technique is hard to use in SH-4, as 16 bit instructions are too
//!    small to contain large immediates. On SH-4, large immediates are
//!    loaded from memory using `mov.l` PC-relative load instruction.
//!
//!  - Many RISC ISAs are, despite their name, actually fairly complex.
//!    They tend to have hundreds if not thousands of different instructions.
//!    SH-4 doesn't really have that many instructions because its 16-bit
//!    machine code simply can't encode many different opcodes. As a
//!    result, the number of relocations the linker has to support is also
//!    small.
//!
//! Beside these, SH-4 has a delay branch slot just like contemporary MIPS
//! and SPARC. That is, one instruction after a branch instruction will
//! always be executed even if the branch is taken. Delay branch slot allows
//! a pipelined CPU to start and finish executing an instruction after a
//! branch regardless of the branch's condition, simplifying the processor's
//! implementation. It's considered a bad premature optimization nowadays,
//! though. Modern RISC processors don't have it.
//!
//! Here are notes about the SH-4 psABI:
//!
//!  - If a source file is compiled with -fPIC, each function starts
//!    with a piece of code to store the address of .got to %r12.
//!    We can use the register in our PLT for position-independent output.
//!
//!  - Even though it uses RELA-type relocations, object files store
//!    addends not in the r_addend field but in the relocated section
//!    contents. Dynamic relocations, however, follow the usual RELA
//!    convention.
//!
//!  - It looks like the ecosystem has bit-rotted. Some tests, especially
//!    one using C++ exceptions, don't pass even with GNU ld.
//!
//!  - GCC/SH4 tends to write dynamically-relocated data into .text, so the
//!    output from the linker contains lots of text relocations. That's not
//!    a problem with embedded programming, I guess.

use std::marker::PhantomData;
use std::sync::atomic::Ordering;

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{check_tlsle, scan_pcrel, InputSection};
use crate::output_chunks::eh_frame;
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct Sh4Target<End>(PhantomData<End>);

pub type Sh4 = Sh4Target<LittleEndian>;
pub type Sh4Be = Sh4Target<BigEndian>;

impl Layout for Sh4Target<LittleEndian> {
    type Endian = LittleEndian;
    type Word = Ul32;
    type Sym = Elf32Sym<LittleEndian>;
    type Phdr = Elf32Phdr<LittleEndian>;
    type Chdr = Elf32Chdr<LittleEndian>;
    type Rel = Elf32RelaLe;
}

impl Layout for Sh4Target<BigEndian> {
    type Endian = BigEndian;
    type Word = Ub32;
    type Sym = Elf32Sym<BigEndian>;
    type Phdr = Elf32Phdr<BigEndian>;
    type Chdr = Elf32Chdr<BigEndian>;
    type Rel = Elf32RelaBe;
}

// Even though SH-4 uses RELA-type relocations, addends are stored in
// the relocated places for some reason.
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

impl<End: Endian> Arch for Sh4Target<End>
where
    Self: Layout<Endian = End>,
{
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

    fn get_addend(loc: &[u8], rel: &Self::Rel) -> i64 {
        if addend_in_place(rel.r_type()) {
            End::read_u32(loc) as i32 as i64
        } else {
            0
        }
    }

    fn write_addend(loc: &mut [u8], val: i64, rel: &Self::Rel) {
        if addend_in_place(rel.r_type()) {
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
                    .get()
                    .wrapping_sub(ctx.got.hdr.shdr.sh_addr.get()) as u32,
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
            End::write_u32(&mut buf[12..], ctx.gotplt.hdr.shdr.sh_addr.get() as u32);
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
                gotplt.wrapping_sub(ctx.got.hdr.shdr.sh_addr.get()) as u32,
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
            sym.plt_idx(&ctx.symbols).unwrap() * std::mem::size_of::<ElfRel<Self>>() as u32,
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
                got.wrapping_sub(ctx.got.hdr.shdr.sh_addr.get()) as u32,
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
        rel: &Self::Rel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        match rel.r_type() {
            R_NONE => {}
            R_SH_DIR32 => End::write_u32(loc, val as u32),
            R_SH_REL32 => End::write_u32(loc, val.wrapping_sub(p) as u32),
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];
        for rel in isec.relocations::<Self>(ctx) {
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.is_ifunc() {
                error!(ctx, "{sym}: GNU ifunc symbol is not supported on sh4");
            }

            match rel.r_type() {
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
        }
    }

    fn apply_reloc_alloc(ctx: &Context<Self>, isec: &InputSection, buf: &mut [u8]) {
        let file = &ctx.objs[isec.file.index()];
        let got = ctx.got.hdr.shdr.sh_addr.get();

        for rel in isec.rels::<Self>(file) {
            if rel.r_type() == R_NONE {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let s = sym.addr(ctx);
            let a = isec.rel_addend::<Self>(rel) as u64;
            let p = isec.addr(ctx) + rel.r_offset();
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let loc = &mut buf[rel.r_offset() as usize..];

            let val = match rel.r_type() {
                // Handled as an absolute relocation by the output section.
                R_SH_DIR32 => continue,
                R_SH_REL32 | R_SH_PLT32 => sa.wrapping_sub(p),
                R_SH_GOT32 => g(),
                R_SH_GOTPC => got.wrapping_add(a).wrapping_sub(p),
                R_SH_GOTOFF => sa.wrapping_sub(got),
                R_SH_TLS_GD_32 => sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(got),
                R_SH_TLS_LD_32 => ctx.got.tlsld_addr().wrapping_add(a).wrapping_sub(got),
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
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            let frag = isec.fragment(ctx, rel);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), isec.rel_addend::<Self>(rel) as u64),
            };
            let sa = s.wrapping_add(a);
            let tombstone = isec.tombstone(ctx, sym, frag.map(|(f, _)| f));
            let loc = &mut buf[rel.r_offset() as usize..];

            match rel.r_type() {
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
