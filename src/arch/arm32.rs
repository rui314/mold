//! ARM32 is a bit special from the linker's viewpoint because ARM
//! processors support two different instruction encodings: Thumb and
//! ARM (in a narrower sense). Thumb instructions are either 16 bits or
//! 32 bits, while ARM instructions are all 32 bits. Feature-wise,
//! Thumb is a subset of ARM, so not all ARM instructions are
//! representable in Thumb.
//!
//! ARM processors originally supported only ARM instructions. Thumb
//! instructions were later added to increase code density.
//!
//! ARM processors runs in either ARM mode or Thumb mode. The mode can
//! be switched using BX (branch and mode exchange)-family instructions.
//! We need to use that instructions to, for example, call a function
//! encoded in Thumb from a function encoded in ARM. Sometimes, the
//! linker even has to emit interworking thunk code to switch mode.
//!
//! ARM instructions are aligned to 4 byte boundaries. Thumb are to 2
//! byte boundaries. So the least significant bit of a function address
//! is always 0.
//!
//! To distinguish Thumb functions from ARM fucntions, the LSB of a
//! function address is repurposed as a boolean flag. If the LSB is 0,
//! the function referred to by the address is encoded in ARM;
//! otherwise, Thumb.
//!
//! For example, if a symbol `foo` is of type STT_FUNC and has value
//! 0x2001, `foo` is a function using Thumb instructions whose address
//! is 0x2000 (not 0x2001, as Thumb instructions are always 2-byte
//! aligned). Likewise, if a function pointer has value 0x2001, it
//! refers a Thumb function at 0x2000.
//!
//! https://github.com/ARM-software/abi-aa/blob/main/aaelf32/aaelf32.rst
//!
//! Relocations are of the REL type, and addends are packed into the
//! instruction fields they relocate.
//!
//! Big-endian ARM is rare but comes in two flavors: BE32, with big-endian
//! instructions and data, and BE8, the de facto standard since ARMv6,
//! with little-endian instructions and big-endian data. Only BE8 is
//! supported. Compilers nevertheless emit big-endian instructions into
//! object files compiled for big-endian mode, so relocations are applied
//! in the object files' byte order, and the code in input sections is
//! converted to little-endian at the very end, after it's copied to the
//! output; see [`swap_code_bytes`]. Linker-synthesized code is written
//! in little-endian form to begin with.

use std::marker::PhantomData;

use rayon::prelude::*;

use crate::arch::{Arch, Family, ThunkLayout};
use crate::chunks::eh_frame;
use crate::chunks::output_section::OutputBuffer;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{
    check_tlsle, scan_absrel, scan_pcrel, scan_tlsdesc, InputSection, InputSectionId,
};
use crate::symbol::{Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_TLSGD};
use crate::thunks::Thunk;
use crate::util::endian::{write_ul32, BigEndian, Endian, LittleEndian, Ub32, Ul32};
use crate::util::{align_to, bit, bits, is_int, sign_extend};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct Arm32Target<End>(PhantomData<End>);

pub type Arm32 = Arm32Target<LittleEndian>;
pub type Arm32Be = Arm32Target<BigEndian>;

impl Layout for Arm32Target<LittleEndian> {
    type Endian = LittleEndian;
    type Word = Ul32;
    type Sym = Elf32Sym<LittleEndian>;
    type Phdr = Elf32Phdr<LittleEndian>;
    type Chdr = Elf32Chdr<LittleEndian>;
    type Rel = Elf32RelLe;
}

impl Layout for Arm32Target<BigEndian> {
    type Endian = BigEndian;
    type Word = Ub32;
    type Sym = Elf32Sym<BigEndian>;
    type Phdr = Elf32Phdr<BigEndian>;
    type Chdr = Elf32Chdr<BigEndian>;
    type Rel = Elf32RelBe;
}

fn b(val: u64, hi: u32, lo: u32) -> u32 {
    bits(val, hi, lo) as u32
}

fn bt(val: u64, n: u32) -> u32 {
    bit(val, n) as u32
}

/// The second halfword of a 32-bit Thumb instruction.
fn thm2<End: Endian>(loc: &[u8]) -> u16 {
    End::read_u16(&loc[2..])
}

fn write_arm_mov<End: Endian>(loc: &mut [u8], val: u32) {
    let imm12 = b(val as u64, 11, 0);
    let imm4 = b(val as u64, 15, 12);
    End::write_u32(
        loc,
        (End::read_u32(loc) & 0xfff0_f000) | (imm4 << 16) | imm12,
    );
}

fn write_thm_b21<End: Endian>(loc: &mut [u8], val: u32) {
    let v = val as u64;
    let (s, j2, j1) = (bt(v, 20), bt(v, 19), bt(v, 18));
    let imm6 = b(v, 17, 12);
    let imm11 = b(v, 11, 1);
    End::write_u16(
        loc,
        ((End::read_u16(loc) & 0b1111_1011_1100_0000) as u32 | (s << 10) | imm6) as u16,
    );
    let second =
        ((thm2::<End>(loc) & 0b1101_0000_0000_0000) as u32 | (j1 << 13) | (j2 << 11) | imm11)
            as u16;
    End::write_u16(&mut loc[2..], second);
}

fn write_thm_b25<End: Endian>(loc: &mut [u8], val: u32) {
    let v = val as u64;
    let (s, i1, i2) = (bt(v, 24), bt(v, 23), bt(v, 22));
    let j1 = (i1 ^ 1) ^ s;
    let j2 = (i2 ^ 1) ^ s;
    let imm10 = b(v, 21, 12);
    let imm11 = b(v, 11, 1);
    End::write_u16(
        loc,
        ((End::read_u16(loc) & 0b1111_1000_0000_0000) as u32 | (s << 10) | imm10) as u16,
    );
    let second =
        ((thm2::<End>(loc) & 0b1101_0000_0000_0000) as u32 | (j1 << 13) | (j2 << 11) | imm11)
            as u16;
    End::write_u16(&mut loc[2..], second);
}

fn write_thm_mov<End: Endian>(loc: &mut [u8], val: u32) {
    let v = val as u64;
    let imm4 = b(v, 15, 12);
    let i = bt(v, 11);
    let imm3 = b(v, 10, 8);
    let imm8 = b(v, 7, 0);
    End::write_u16(
        loc,
        ((End::read_u16(loc) & 0b1111_1011_1111_0000) as u32 | (i << 10) | imm4) as u16,
    );
    let second = ((thm2::<End>(loc) & 0b1000_1111_0000_0000) as u32 | (imm3 << 12) | imm8) as u16;
    End::write_u16(&mut loc[2..], second);
}

/// Sets the second halfword's bit that turns a Thumb BLX into a BL, or
/// clears it for the reverse.
fn set_thm_bl<End: Endian>(loc: &mut [u8], is_bl: bool) {
    let second = if is_bl {
        thm2::<End>(loc) | 0x1000
    } else {
        thm2::<End>(loc) & !0x1000
    };
    End::write_u16(&mut loc[2..], second);
}

// Only function symbols tell in the LSB of their value whether they are
// Thumb or ARM code. A branch to a symbol of another type, such as a
// plain label in hand-written assembly, does not switch the instruction
// set. A branch to a PLT entry always lands on ARM code.
fn is_thumb_func<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> bool {
    matches!(sym.ty(), STT_FUNC | STT_GNU_IFUNC) && sym.addr(ctx) & 1 != 0
}

fn is_arm_func<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> bool {
    sym.has_plt(&ctx.symbols)
        || (matches!(sym.ty(), STT_FUNC | STT_GNU_IFUNC) && sym.addr(ctx) & 1 == 0)
}

const ARM_NOP: u32 = 0xe320_f000;
const THM_NOP_W: u32 = 0x8000_f3af;

const PLT_ENTRY: [u32; 4] = [
    0xe59f_c004, // 1: ldr ip, 2f
    0xe08c_c00f, //    add ip, ip, pc
    0xe59c_f000, //    ldr pc, [ip]
    0x0000_0000, // 2: .word sym@GOT - 1b
];

/// Linker-synthesized code is little-endian whatever the data order.
fn write_code(buf: &mut [u8], words: &[u32]) {
    for (i, &w) in words.iter().enumerate() {
        write_ul32(&mut buf[i * 4..], w);
    }
}

/// What a mapping symbol marks the following bytes as: the width of
/// their instructions, or `None` for data.
fn mapping_symbol_kind(name: &[u8]) -> Option<Option<usize>> {
    let kind = |c: u8| name == [b'$', c] || (name.starts_with(&[b'$', c, b'.']));
    if kind(b'a') {
        Some(Some(4))
    } else if kind(b't') {
        Some(Some(2))
    } else if kind(b'd') {
        Some(None)
    } else {
        None
    }
}

// Even though using ARM32 in big-endian mode is very rare, the processor
// technically supports both little- and big-endian modes. There are two
// variants of big-endian mode: BE32 and BE8. In BE32, instructions and
// data are encoded in big-endian. In BE8, instructions are encoded in
// little-endian, and only data is in big-endian. BE8 is the de facto
// standard for ARMv6 or later. We support only BE8.
//
// A tricky thing is that instructions in an object file are always
// big-endian if the file is compiled for big-endian mode. In other words,
// the compiler always emit code in BE32 if -mbig-endian is specified. It
// is the linker's responsibility to rewrite instructions from big-endian
// to little-endian for an BE8 output. This function does that.
//
// The text section may contain a mix of 32-bit ARM instructions, 16-bit
// Thumb instructions, and data. We need to distinguish them to swap 4
// bytes, 2 bytes, or not swap bytes, respectively. The beginning of ARM
// code, Thumb code, and data is labeled with a mapping symbol of $a, $t,
// and $d, respectively. We use mapping symbols to determine what to do
// with the text section.
//
// This function is called after we copy the input section contents to the
// output file. We rewrite instructions in the output buffer in place.
pub fn swap_code_bytes<End: Endian>(ctx: &Context<Arm32Target<End>>, buf: &mut [u8])
where
    Arm32Target<End>: Layout<Endian = End>,
{
    let output = OutputBuffer::new(buf);
    ctx.objs.par_iter().for_each(|file| {
        // Collect mapping symbols
        let mut marks: Vec<(InputSectionId, u64, Option<usize>)> = file
            .base
            .local_symbols()
            .iter()
            .map(|&id| &ctx.symbols[id])
            .filter_map(|sym| {
                let kind = mapping_symbol_kind(sym.name())?;
                let sec = sym.input_section()?;
                let isec = ctx.input_section(sec);
                (isec.is_alive() && isec.sh_flags & SHF_EXECINSTR as u64 != 0)
                    .then_some((sec, sym.value, kind))
            })
            .collect();
        // Group mapping symbols by input section and sort by address
        marks.sort_by_key(|&(sec, offset, _)| (ctx.input_section(sec).shndx, offset));

        // Swap bytes
        for (i, &(sec, start, kind)) in marks.iter().enumerate() {
            let Some(width) = kind else { continue };
            let isec = ctx.input_section(sec);
            let end = match marks.get(i + 1) {
                Some(&(next, offset, _)) if next == sec => offset,
                _ => isec.sh_size,
            };
            let osec = ctx.output_section(isec.output_section.expect("output section"));
            let base = osec.hdr.shdr.sh_offset.get() + isec.offset();
            // SAFETY: live input sections occupy disjoint output ranges, and
            // this file's mapping-symbol ranges are processed sequentially.
            unsafe {
                output.with_slice((base + start) as usize..(base + end) as usize, |buf| {
                    for insn in buf.chunks_exact_mut(width) {
                        insn.reverse();
                    }
                });
            }
        }
    });
}

impl<End: Endian> Arch for Arm32Target<End>
where
    Self: Layout<Endian = End>,
{
    type InputSectionExtra = u32;

    const NAME: &'static str = if End::IS_LITTLE { "arm32" } else { "arm32be" };
    const FAMILY: Family = Family::Arm32;
    const PAGE_SIZE: u64 = 65536;
    const E_MACHINE: u32 = EM_ARM;
    const PLT_HDR_SIZE: u64 = 32;
    const PLT_SIZE: u64 = 16;
    const PLTGOT_SIZE: u64 = 16;
    const THUNK: Option<ThunkLayout> = Some(ThunkLayout {
        header_size: 16,
        entry_size: 16,
    });
    const TRAP: &'static [u8] = &[0xff, 0xde]; // udf

    const R_COPY: u32 = R_ARM_COPY;
    const R_GLOB_DAT: u32 = R_ARM_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_ARM_JUMP_SLOT;
    const R_ABS: u32 = R_ARM_ABS32;
    const R_RELATIVE: u32 = R_ARM_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_ARM_IRELATIVE);
    const R_DTPOFF: u32 = R_ARM_TLS_DTPOFF32;
    const R_TPOFF: u32 = R_ARM_TLS_TPOFF32;
    const R_DTPMOD: u32 = R_ARM_TLS_DTPMOD32;
    const R_TLSDESC: Option<u32> = Some(R_ARM_TLS_DESC);
    const R_FUNCALL: &'static [u32] = &[
        R_ARM_JUMP24,
        R_ARM_THM_JUMP24,
        R_ARM_CALL,
        R_ARM_THM_CALL,
        R_ARM_PLT32,
    ];

    fn rel_to_string(r_type: u32) -> String {
        arm32_rel_to_string(r_type)
    }

    fn eflags(_ctx: &Context<Self>) -> u32 {
        if End::IS_LITTLE {
            EF_ARM_EABI_VER5
        } else {
            EF_ARM_EABI_VER5 | EF_ARM_BE8
        }
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u32; 8] = [
            0xe52d_e004, //    push {lr}
            0xe59f_e004, //    ldr lr, 2f
            0xe08f_e00e, // 1: add lr, pc, lr
            0xe5be_f008, //    ldr pc, [lr, #8]!
            0x0000_0000, // 2: .word .got.plt - 1b - 8
            0x0000_0000, //    (padding)
            0x0000_0000, //    (padding)
            0x0000_0000, //    (padding)
        ];
        write_code(buf, &INSN);
        let gotplt_addr = ctx.gotplt.hdr.shdr.sh_addr.get();
        let plt_addr = ctx.plt.hdr.shdr.sh_addr.get();
        let gotplt = gotplt_addr.wrapping_sub(plt_addr).wrapping_sub(16);
        End::write_u32(&mut buf[16..], gotplt as u32);
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_code(buf, &PLT_ENTRY);
        End::write_u32(
            &mut buf[12..],
            sym.gotplt_addr(ctx)
                .wrapping_sub(sym.plt_addr(ctx))
                .wrapping_sub(12) as u32,
        );
    }

    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        write_code(buf, &PLT_ENTRY);
        End::write_u32(
            &mut buf[12..],
            sym.got_pltgot_addr(ctx)
                .wrapping_sub(sym.plt_addr(ctx))
                .wrapping_sub(12) as u32,
        );
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
            R_ARM_ABS32 => End::write_u32(loc, val as u32),
            R_ARM_REL32 => End::write_u32(loc, val.wrapping_sub(p) as u32),
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
                R_ARM_MOVW_ABS_NC | R_ARM_THM_MOVW_ABS_NC => scan_absrel(ctx, isec, sym, &rel),
                R_ARM_THM_CALL | R_ARM_CALL | R_ARM_JUMP24 | R_ARM_PLT32 | R_ARM_THM_JUMP24 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_ARM_GOT_PREL | R_ARM_GOT_BREL | R_ARM_TARGET2 => sym.add_flags(NEEDS_GOT),
                R_ARM_MOVT_PREL | R_ARM_THM_MOVT_PREL | R_ARM_PREL31 => {
                    scan_pcrel(ctx, isec, sym, &rel)
                }
                R_ARM_TLS_GD32 => sym.add_flags(NEEDS_TLSGD),
                R_ARM_TLS_LDM32 => ctx
                    .needs_tlsld
                    .store(true, std::sync::atomic::Ordering::Relaxed),
                R_ARM_TLS_IE32 => sym.add_flags(NEEDS_GOTTP),
                R_ARM_TLS_CALL | R_ARM_THM_TLS_CALL => scan_tlsdesc(ctx, sym),
                R_ARM_TLS_LE32 => check_tlsle(ctx, isec, sym, &rel),
                R_ARM_ABS32
                | R_ARM_TARGET1
                | R_ARM_MOVT_ABS
                | R_ARM_THM_MOVT_ABS
                | R_ARM_REL32
                | R_ARM_BASE_PREL
                | R_ARM_GOTOFF32
                | R_ARM_THM_JUMP8
                | R_ARM_THM_JUMP11
                | R_ARM_THM_JUMP19
                | R_ARM_MOVW_PREL_NC
                | R_ARM_THM_MOVW_PREL_NC
                | R_ARM_TLS_LDO32
                | R_ARM_V4BX
                | R_ARM_TLS_GOTDESC => {}
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
        let osec = &ctx.output_sections[isec.output_section.expect("output section").index()];

        for (i, rel) in rels.iter().enumerate() {
            if rel.r_type() == R_NONE || rel.r_type() == R_ARM_V4BX {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let off = rel.r_offset() as usize;
            let s = sym.addr(ctx);
            let a = isec.rel_addend(rel) as u64;
            let p = isec.addr(ctx) + rel.r_offset();
            let t = is_thumb_func(ctx, sym) as u64;
            let got = ctx.got.hdr.shdr.sh_addr.get();
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);

            let check = |val: i64, lo: i64, hi: i64| isec.check_range(ctx, i, val, lo, hi);
            // A thunk has two entry points: +0 for Thumb, +4 for ARM.
            let thumb_thunk = || sym.thunk_addr(ctx, p);
            let arm_thunk = || sym.thunk_addr(ctx, p) + 4;
            // The TLSDESC trampoline is the header of the next thunk.
            let tlsdesc_trampoline = || {
                let i = osec.thunks.partition_point(|thunk| thunk.addr(osec) <= p);
                osec.thunks.get(i).map_or_else(
                    || {
                        fatal!(
                            "{}: no TLSDESC trampoline after the call",
                            isec.display(file)
                        )
                    },
                    |thunk| thunk.addr(osec),
                )
            };
            let loc = &mut buf[off..];
            let write32 = |loc: &mut [u8], v: u32| End::write_u32(loc, v);
            let write16 = |loc: &mut [u8], v: u16| End::write_u16(loc, v);

            match rel.r_type() {
                // Handled as absolute relocations by the output section.
                R_ARM_ABS32 | R_ARM_TARGET1 => {}
                R_ARM_REL32 => write32(loc, pcrel as u32),
                R_ARM_THM_CALL => {
                    if sym.is_remaining_undef_weak() {
                        // On ARM, calling an weak undefined symbol jumps to the
                        // next instruction.
                        // NOP.W
                        write32(loc, THM_NOP_W);
                        continue;
                    }
                    // THM_CALL relocation refers to either BL or BLX instruction.
                    // They are different in only one bit. We need to use BLX if the
                    // jump target is an ARM function. Otherwise, use BL.
                    let val1 = pcrel as i64;
                    let val2 = align_to(pcrel, 4) as i64;
                    let arm = is_arm_func(ctx, sym);
                    if !arm && is_int(val1, 25) {
                        set_thm_bl::<End>(loc, true);
                        write_thm_b25::<End>(loc, val1 as u32);
                    } else if arm && is_int(val2, 25) {
                        set_thm_bl::<End>(loc, false);
                        write_thm_b25::<End>(loc, val2 as u32);
                    } else {
                        set_thm_bl::<End>(loc, true);
                        write_thm_b25::<End>(
                            loc,
                            thumb_thunk().wrapping_add(a).wrapping_sub(p) as u32,
                        );
                    }
                }
                R_ARM_BASE_PREL => write32(loc, got.wrapping_add(a).wrapping_sub(p) as u32),
                R_ARM_GOTOFF32 => write32(loc, (sa | t).wrapping_sub(got) as u32),
                R_ARM_GOT_PREL | R_ARM_TARGET2 => write32(
                    loc,
                    got.wrapping_add(g()).wrapping_add(a).wrapping_sub(p) as u32,
                ),
                R_ARM_GOT_BREL => write32(loc, g().wrapping_add(a) as u32),
                R_ARM_CALL => {
                    if sym.is_remaining_undef_weak() {
                        write32(loc, ARM_NOP); // NOP
                        continue;
                    }
                    // Just like THM_CALL, ARM_CALL relocation refers to either BL or
                    // BLX instruction. We may need to rewrite BL → BLX or BLX → BL.
                    let insn = End::read_u32(loc);
                    let is_bl = insn & 0xff00_0000 == 0xeb00_0000;
                    let is_blx = insn & 0xfe00_0000 == 0xfa00_0000;
                    if !is_bl && !is_blx {
                        fatal!(
                            "{}: R_ARM_CALL refers to neither BL nor BLX",
                            isec.display(file)
                        );
                    }
                    if is_int(pcrel as i64, 26) {
                        if t != 0 {
                            write32(loc, 0xfa00_0000 | (bt(pcrel, 1) << 24) | b(pcrel, 25, 2));
                        // BLX
                        } else {
                            write32(loc, 0xeb00_0000 | b(pcrel, 25, 2)); // BL
                        }
                    } else {
                        write32(
                            loc,
                            0xeb00_0000 | b(arm_thunk().wrapping_add(a).wrapping_sub(p), 25, 2),
                        ); // BL
                    }
                }
                R_ARM_JUMP24 => {
                    if sym.is_remaining_undef_weak() {
                        write32(loc, ARM_NOP); // NOP
                        continue;
                    }
                    // These relocs refers to a B (unconditional branch) instruction.
                    // Unlike BL or BLX, we can't rewrite B to BX in place when the
                    // processor mode switch is required because BX doesn't takes an
                    // immediate; it takes only a register. So if mode switch is
                    // required, we jump to a linker-synthesized thunk which does the
                    // job with a longer code sequence.
                    let mut val = pcrel;
                    if t != 0 || !is_int(val as i64, 26) {
                        val = arm_thunk().wrapping_add(a).wrapping_sub(p);
                    }
                    write32(loc, (End::read_u32(loc) & 0xff00_0000) | b(val, 25, 2));
                }
                R_ARM_PLT32 => {
                    if sym.is_remaining_undef_weak() {
                        write32(loc, ARM_NOP); // NOP
                    } else {
                        let val = if t != 0 { arm_thunk() } else { s }
                            .wrapping_add(a)
                            .wrapping_sub(p);
                        write32(loc, (End::read_u32(loc) & 0xff00_0000) | b(val, 25, 2));
                    }
                }
                R_ARM_THM_JUMP8 => {
                    check(pcrel as i64, -(1 << 8), 1 << 8);
                    write16(loc, (End::read_u16(loc) & 0xff00) | b(pcrel, 8, 1) as u16);
                }
                R_ARM_THM_JUMP11 => {
                    check(pcrel as i64, -(1 << 11), 1 << 11);
                    write16(loc, (End::read_u16(loc) & 0xf800) | b(pcrel, 11, 1) as u16);
                }
                R_ARM_THM_JUMP19 => {
                    check(pcrel as i64, -(1 << 20), 1 << 20);
                    write_thm_b21::<End>(loc, pcrel as u32);
                }
                R_ARM_THM_JUMP24 => {
                    if sym.is_remaining_undef_weak() {
                        write32(loc, THM_NOP_W); // NOP
                        continue;
                    }
                    // Just like R_ARM_JUMP24, we need to jump to a thunk if we need to
                    // switch processor mode.
                    let mut val = pcrel;
                    if is_arm_func(ctx, sym) || !is_int(val as i64, 25) {
                        val = thumb_thunk().wrapping_add(a).wrapping_sub(p);
                    }
                    write_thm_b25::<End>(loc, val as u32);
                }
                R_ARM_MOVW_PREL_NC => write_arm_mov::<End>(loc, (sa | t).wrapping_sub(p) as u32),
                R_ARM_MOVW_ABS_NC => write_arm_mov::<End>(loc, (sa | t) as u32),
                R_ARM_THM_MOVW_PREL_NC => {
                    write_thm_mov::<End>(loc, (sa | t).wrapping_sub(p) as u32)
                }
                R_ARM_PREL31 => {
                    check(pcrel as i64, -(1 << 30), 1 << 30);
                    write32(
                        loc,
                        (End::read_u32(loc) & 0x8000_0000) | (pcrel as u32 & 0x7fff_ffff),
                    );
                }
                R_ARM_THM_MOVW_ABS_NC => write_thm_mov::<End>(loc, (sa | t) as u32),
                R_ARM_MOVT_PREL => write_arm_mov::<End>(loc, (pcrel >> 16) as u32),
                R_ARM_THM_MOVT_PREL => write_thm_mov::<End>(loc, (pcrel >> 16) as u32),
                R_ARM_MOVT_ABS => write_arm_mov::<End>(loc, (sa >> 16) as u32),
                R_ARM_THM_MOVT_ABS => write_thm_mov::<End>(loc, (sa >> 16) as u32),
                R_ARM_TLS_GD32 => write32(
                    loc,
                    sym.tlsgd_addr(ctx).wrapping_add(a).wrapping_sub(p) as u32,
                ),
                R_ARM_TLS_LDM32 => write32(
                    loc,
                    ctx.got.tlsld_addr().wrapping_add(a).wrapping_sub(p) as u32,
                ),
                R_ARM_TLS_LDO32 => write32(loc, sa.wrapping_sub(ctx.dtp_addr) as u32),
                R_ARM_TLS_IE32 => write32(
                    loc,
                    sym.gottp_addr(ctx).wrapping_add(a).wrapping_sub(p) as u32,
                ),
                R_ARM_TLS_LE32 => write32(loc, sa.wrapping_sub(ctx.tp_addr) as u32),
                // ARM32 TLSDESC uses the following code sequence to materialize
                // a TP-relative address in r0.
                //
                // ldr     r0, .L2
                // .L1: bl      foo
                // R_ARM_TLS_CALL
                // .L2: .word   foo + . - .L1
                // R_ARM_TLS_GOTDESC
                //
                // We may relax the instructions to the following if its TP-relative
                // address is known at link-time
                //
                // ldr     r0, .L2
                // .L1: nop
                // ...
                // .L2: .word   foo(tpoff)
                //
                // or to the following if the TP-relative address is known at
                // process startup time.
                //
                // ldr     r0, .L2
                // .L1: ldr r0, [pc, r0]
                // ...
                // .L2: .word   foo(gottpoff) + . - .L1
                R_ARM_TLS_GOTDESC => {
                    // A is odd if the corresponding TLS_CALL is Thumb.
                    if sym.has_tlsdesc(&ctx.symbols) {
                        let adjust = if a & 1 != 0 { 6 } else { 4 };
                        write32(
                            loc,
                            sym.tlsdesc_addr(ctx)
                                .wrapping_sub(p)
                                .wrapping_add(a)
                                .wrapping_sub(adjust) as u32,
                        );
                    } else if sym.has_gottp(&ctx.symbols) {
                        let adjust = if a & 1 != 0 { 5 } else { 8 };
                        write32(
                            loc,
                            sym.gottp_addr(ctx)
                                .wrapping_sub(p)
                                .wrapping_add(a)
                                .wrapping_sub(adjust) as u32,
                        );
                    } else {
                        write32(loc, s.wrapping_sub(ctx.tp_addr) as u32);
                    }
                }
                R_ARM_TLS_CALL => {
                    if sym.has_tlsdesc(&ctx.symbols) {
                        write32(
                            loc,
                            0xeb00_0000
                                | b(tlsdesc_trampoline().wrapping_sub(p).wrapping_sub(8), 25, 2),
                        ); // bl 0
                    } else if sym.has_gottp(&ctx.symbols) {
                        write32(loc, 0xe79f_0000); // ldr r0, [pc, r0]
                    } else {
                        write32(loc, ARM_NOP);
                    }
                }
                R_ARM_THM_TLS_CALL => {
                    if sym.has_tlsdesc(&ctx.symbols) {
                        let val = align_to(tlsdesc_trampoline().wrapping_sub(p).wrapping_sub(4), 4);
                        write_thm_b25::<End>(loc, val as u32);
                        // rewrite BL with BLX
                        set_thm_bl::<End>(loc, false);
                    } else if sym.has_gottp(&ctx.symbols) {
                        // Since `ldr r0, [pc, r0]` is not representable in Thumb,
                        // we use two instructions instead.
                        write16(loc, 0x4478); // add r0, pc
                        write16(&mut loc[2..], 0x6800); // ldr r0, [r0]
                    } else {
                        // nop.w
                        write32(loc, THM_NOP_W);
                    }
                }
                _ => error!(
                    "{}: unknown relocation: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection<Self>, buf: &mut [u8]) {
        let mut fragment_cache = crate::input_sections::FragmentLookup::default();
        let file = &ctx.objs[isec.file.index()];
        for rel in isec.rels(file) {
            if rel.r_type() == R_NONE || isec.record_undef_error(ctx, rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            let frag = isec.fragment(ctx, rel, &mut fragment_cache);
            let (s, a) = match frag {
                Some((frag, addend)) => (ctx.fragment_addr(frag), addend as u64),
                None => (sym.addr(ctx), isec.rel_addend(rel) as u64),
            };
            let tombstone = isec.tombstone(ctx, sym, frag.map(|(f, _)| f));
            let loc = &mut buf[rel.r_offset() as usize..];

            match rel.r_type() {
                R_ARM_ABS32 => End::write_u32(loc, tombstone.unwrap_or(s.wrapping_add(a)) as u32),
                R_ARM_TLS_LDO32 => End::write_u32(
                    loc,
                    tombstone.unwrap_or(s.wrapping_add(a).wrapping_sub(ctx.dtp_addr)) as u32,
                ),
                _ => fatal!(
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    /// Thumb and ARM B instructions cannot be converted to BX, so we
    /// always have to make them jump to a thunk to switch processor mode
    /// even if their destinations are reachable.
    fn always_needs_thunk(ctx: &Context<Self>, sym: &Symbol, rel: &Self::Rel) -> bool {
        match rel.r_type() {
            R_ARM_JUMP24 | R_ARM_PLT32 => is_thumb_func(ctx, sym),
            R_ARM_THM_JUMP24 => is_arm_func(ctx, sym),
            _ => false,
        }
    }

    fn write_thunk(ctx: &Context<Self>, thunk: &Thunk, addr: u64, buf: &mut [u8]) {
        // TLS trampoline code. ARM32's TLSDESC is designed so that this
        // common piece of code is factored out from object files to reduce
        // output size. Since no one provide, the linker has to synthesize it.
        const HDR: [u32; 4] = [
            0xe08e_0000, // add r0, lr, r0
            0xe590_1004, // ldr r1, [r0, #4]
            0xe12f_ff11, // bx  r1
            0xe320_f000, // nop
        ];
        // This is a range extension and mode switch thunk.
        // It has two entry points: +0 for Thumb and +4 for ARM.
        const ENTRY: [u8; 16] = [
            // .thumb
            0x78, 0x47, //    bx   pc  # jumps to 1f
            0xc0, 0x46, //    nop
            // .arm
            0x00, 0xc0, 0x9f, 0xe5, // 1: ldr  ip, 3f
            0x0f, 0xf0, 0x8c, 0xe0, // 2: add  pc, ip, pc
            0x00, 0x00, 0x00, 0x00, // 3: .word sym - 2b
        ];
        write_code(buf, &HDR);
        for (i, &sym) in thunk.symbols.iter().enumerate() {
            let s = ctx.symbols[sym].addr(ctx);
            let p = addr + thunk.offsets[i];
            let entry = &mut buf[thunk.offsets[i] as usize..];
            entry[..16].copy_from_slice(&ENTRY);
            End::write_u32(&mut entry[12..], s.wrapping_sub(p).wrapping_sub(16) as u32);
        }
    }

    fn finish_output(ctx: &Context<Self>, buf: &mut [u8]) {
        if !End::IS_LITTLE {
            swap_code_bytes(ctx, buf);
        }
    }

    fn write_addend(loc: &mut [u8], val: i64, rel: &Self::Rel) {
        let v = val as u64;
        match rel.r_type() {
            R_ARM_NONE => {}
            R_ARM_ABS32 | R_ARM_REL32 | R_ARM_BASE_PREL | R_ARM_GOTOFF32 | R_ARM_GOT_PREL
            | R_ARM_GOT_BREL | R_ARM_TLS_GD32 | R_ARM_TLS_LDM32 | R_ARM_TLS_LDO32
            | R_ARM_TLS_IE32 | R_ARM_TLS_LE32 | R_ARM_TLS_GOTDESC | R_ARM_TARGET1
            | R_ARM_TARGET2 => End::write_u32(loc, val as u32),
            R_ARM_THM_JUMP8 => {
                End::write_u16(loc, (End::read_u16(loc) & 0xff00) | b(v, 8, 1) as u16)
            }
            R_ARM_THM_JUMP11 => {
                End::write_u16(loc, (End::read_u16(loc) & 0xf800) | b(v, 11, 1) as u16)
            }
            R_ARM_THM_CALL | R_ARM_THM_JUMP24 | R_ARM_THM_TLS_CALL => {
                write_thm_b25::<End>(loc, val as u32)
            }
            R_ARM_CALL | R_ARM_JUMP24 | R_ARM_PLT32 => {
                End::write_u32(loc, (End::read_u32(loc) & 0xff00_0000) | b(v, 25, 2))
            }
            R_ARM_MOVW_PREL_NC | R_ARM_MOVW_ABS_NC | R_ARM_MOVT_PREL | R_ARM_MOVT_ABS => {
                write_arm_mov::<End>(loc, val as u32)
            }
            R_ARM_PREL31 => End::write_u32(
                loc,
                (End::read_u32(loc) & 0x8000_0000) | (val as u32 & 0x7fff_ffff),
            ),
            R_ARM_THM_MOVW_PREL_NC
            | R_ARM_THM_MOVW_ABS_NC
            | R_ARM_THM_MOVT_PREL
            | R_ARM_THM_MOVT_ABS => write_thm_mov::<End>(loc, val as u32),
            _ => unreachable!("unexpected relocation {}", rel.type_name::<Self>()),
        }
    }

    fn get_addend(loc: &[u8], rel: &Self::Rel) -> i64 {
        let arm = || End::read_u32(loc) as u64;
        let thm = |i: usize| End::read_u16(&loc[i * 2..]) as u64;
        match rel.r_type() {
            R_ARM_ABS32 | R_ARM_REL32 | R_ARM_BASE_PREL | R_ARM_GOTOFF32 | R_ARM_GOT_PREL
            | R_ARM_GOT_BREL | R_ARM_TLS_GD32 | R_ARM_TLS_LDM32 | R_ARM_TLS_LDO32
            | R_ARM_TLS_IE32 | R_ARM_TLS_LE32 | R_ARM_TLS_GOTDESC | R_ARM_TARGET1
            | R_ARM_TARGET2 => End::read_u32(loc) as i32 as i64,
            R_ARM_THM_JUMP8 => sign_extend(thm(0), 8) << 1,
            R_ARM_THM_JUMP11 => sign_extend(thm(0), 11) << 1,
            R_ARM_THM_JUMP19 => {
                // https://developer.arm.com/documentation/ddi0597/2024-12/Base-Instructions/B--Branch-
                let s = bit(thm(0), 10);
                let j2 = bit(thm(1), 11);
                let j1 = bit(thm(1), 13);
                let imm6 = bits(thm(0), 5, 0);
                let imm11 = bits(thm(1), 10, 0);
                sign_extend(
                    (s << 20) | (j2 << 19) | (j1 << 18) | (imm6 << 12) | (imm11 << 1),
                    21,
                )
            }
            R_ARM_THM_CALL | R_ARM_THM_JUMP24 | R_ARM_THM_TLS_CALL => {
                // https://developer.arm.com/documentation/ddi0597/2024-12/Base-Instructions/BL--BLX--immediate---Branch-with-Link-and-optional-Exchange--immediate--
                let s = bit(thm(0), 10);
                let j1 = bit(thm(1), 13);
                let j2 = bit(thm(1), 11);
                let i1 = (j1 ^ s) ^ 1;
                let i2 = (j2 ^ s) ^ 1;
                let imm10 = bits(thm(0), 9, 0);
                let imm11 = bits(thm(1), 10, 0);
                sign_extend(
                    (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1),
                    25,
                )
            }
            R_ARM_CALL | R_ARM_JUMP24 | R_ARM_PLT32 | R_ARM_TLS_CALL => sign_extend(arm(), 24) << 2,
            R_ARM_MOVW_PREL_NC | R_ARM_MOVW_ABS_NC | R_ARM_MOVT_PREL | R_ARM_MOVT_ABS => {
                // https://developer.arm.com/documentation/ddi0597/2024-12/Base-Instructions/MOV--MOVS--immediate---Move--immediate--
                let imm4 = bits(arm(), 19, 16);
                let imm12 = bits(arm(), 11, 0);
                sign_extend((imm4 << 12) | imm12, 16)
            }
            R_ARM_PREL31 => sign_extend(arm(), 31),
            R_ARM_THM_MOVW_PREL_NC
            | R_ARM_THM_MOVW_ABS_NC
            | R_ARM_THM_MOVT_PREL
            | R_ARM_THM_MOVT_ABS => {
                // https://developer.arm.com/documentation/ddi0597/2024-12/Base-Instructions/MOVT--Move-Top-
                let imm4 = bits(thm(0), 3, 0);
                let i = bit(thm(0), 10);
                let imm3 = bits(thm(1), 14, 12);
                let imm8 = bits(thm(1), 7, 0);
                sign_extend((imm4 << 12) | (i << 11) | (imm3 << 8) | imm8, 16)
            }
            _ => 0,
        }
    }
}
