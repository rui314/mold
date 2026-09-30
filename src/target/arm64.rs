//! The ARM64 (AArch64) target.

use std::path::Path;

use rayon::prelude::*;

use crate::context::Context;
use crate::error;
use crate::fatal;
use crate::input_files::ObjectFile;
use crate::input_sections::{Reloc, RelocTarget};
use crate::macho::*;
use crate::target::Target;
use crate::util::{bits, sign_extend};

#[derive(Clone, Copy, Default)]
pub struct Arm64;

fn page(val: u64) -> u64 {
    val & !0xfff
}

/// Computes the ADRP immediate for reaching `hi`'s page from `lo`'s page.
fn page_offset(hi: u64, lo: u64) -> u32 {
    let val = page(hi).wrapping_sub(page(lo));
    ((bits(val, 13, 12) << 29) | (bits(val, 32, 14) << 5)) as u32
}

fn read32(loc: &[u8]) -> u32 {
    u32::from_le_bytes(loc[..4].try_into().unwrap())
}

/// Whether an instruction is "ldr Xt|Wt, [Xn, #imm]".
fn is_ldr_imm(insn: u32) -> bool {
    insn & 0xbfc0_0000 == 0xb940_0000
}

fn write32(loc: &mut [u8], val: u32) {
    loc[..4].copy_from_slice(&val.to_le_bytes());
}

fn write64(loc: &mut [u8], val: u64) {
    loc[..8].copy_from_slice(&val.to_le_bytes());
}

/// Writes an immediate to an ADD, LDR or STR instruction.
fn write_add_ldst(loc: &mut [u8], val: u64) {
    let insn = read32(loc);
    let mut scale = 0;

    if insn & 0x3b00_0000 == 0x3900_0000 {
        // LDR/STR accesses an aligned 1, 2, 4, 8 or 16 byte data on memory.
        // The immediate is scaled by the data size, so we need to know the
        // data size to write a correct immediate.
        //
        // The most significant two bits of the instruction usually
        // specifies the data size.
        scale = bits(insn as u64, 31, 30);

        // Vector and byte LDR/STR shares the same scale bits.
        // We can distinguish them by looking at other bits.
        if scale == 0 && insn & 0x0480_0000 == 0x0480_0000 {
            scale = 4;
        }
    }

    // The scaled unsigned-offset encoding cannot represent a
    // misaligned target. Silently dropping the low bits would load a
    // neighboring slot.
    if scale > 0 && val & ((1u64 << scale) - 1) != 0 {
        fatal!("PAGEOFF12 target {val:#x} is not aligned to {}", 1u64 << scale);
    }

    // Bits [21:10] hold the 12-bit immediate. Compilers usually leave
    // them zero, but OR-ing without clearing would mix a leftover
    // placeholder with the final page offset.
    const IMM12: u32 = 0x003f_fc00;
    let imm = (bits(val, 11, scale as u32) as u32) << 10;
    write32(loc, (insn & !IMM12) | imm);
}

// Linker optimization hints (LC_LINKER_OPTIMIZATION_HINT). A compiler
// can't know how far a symbol will land, so it materializes addresses
// with adrp and leaves hints naming the instructions of each sequence,
// for the linker to shorten once addresses are final. ld-prime ignores
// them; they are applied here as ld64 does, under its conditions, with
// the target address read back from the relocated instructions.

const NOP: u32 = 0xd503_201f;

/// ld64's withinOneMeg: whether `to` is in reach of an adr or a
/// literal load at `from`.
fn within_1mb(from: u64, to: u64) -> bool {
    let delta = to.wrapping_sub(from) as i64;
    -(1 << 20) < delta && delta < 1 << 20
}

fn is_adrp(insn: u32) -> bool {
    insn & 0x9f00_0000 == 0x9000_0000
}

/// The page an adrp at `pc` puts in its register.
fn adrp_page(insn: u32, pc: u64) -> u64 {
    let imm = bits(insn as u64, 30, 29) | (bits(insn as u64, 23, 5) << 2);
    page(pc).wrapping_add_signed(sign_extend(imm, 21) << 12)
}

fn adr(rd: u32, target: u64, pc: u64) -> u32 {
    let delta = target.wrapping_sub(pc) as u32;
    0x1000_0000 | ((delta & 3) << 29) | ((delta & 0x1f_fffc) << 3) | rd
}

/// "add Xd, Xn, #imm".
struct Add {
    rd: u32,
    rn: u32,
    imm: u64,
}

fn parse_add(insn: u32) -> Option<Add> {
    (insn & 0xffc0_0000 == 0x9100_0000).then(|| Add {
        rd: insn & 0x1f,
        rn: (insn >> 5) & 0x1f,
        imm: bits(insn as u64, 21, 10),
    })
}

/// A load or store with a scaled unsigned offset, as ld64's
/// parseLoadOrStore takes it apart.
struct LoadStore {
    insn: u32,
    reg: u32,
    base: u32,
    /// In bytes: the 12-bit immediate times `size`.
    offset: u64,
    size: u64,
    is_store: bool,
    is_float: bool,
    is_ldrsw: bool,
}

fn parse_ldst(insn: u32) -> Option<LoadStore> {
    if insn & 0x3b00_0000 != 0x3900_0000 {
        return None;
    }
    // The size and opc fields. A vector register's 16-byte access
    // takes the encodings of the sign-extending byte loads.
    let is_float = insn & 0x0400_0000 != 0;
    let (size, is_store) = match insn & 0xc0c0_0000 {
        0x0000_0000 => (1, true),
        0x0040_0000 => (1, false),
        0x0080_0000 if is_float => (16, true),
        0x00c0_0000 if is_float => (16, false),
        0x0080_0000 | 0x00c0_0000 => (1, false),
        0x4000_0000 => (2, true),
        0x4040_0000 | 0x4080_0000 | 0x40c0_0000 => (2, false),
        0x8000_0000 => (4, true),
        0x8040_0000 | 0x8080_0000 => (4, false),
        0xc000_0000 => (8, true),
        0xc040_0000 => (8, false),
        _ => return None,
    };
    Some(LoadStore {
        insn,
        reg: insn & 0x1f,
        base: (insn >> 5) & 0x1f,
        offset: bits(insn as u64, 21, 10) * size,
        size,
        is_store,
        is_float,
        is_ldrsw: insn & 0xc0c0_0000 == 0x8080_0000,
    })
}

impl LoadStore {
    /// ld64's literalableSize: loads of 4, 8 and 16 bytes have a
    /// pc-relative literal form.
    fn has_literal_form(&self) -> bool {
        self.size >= 4 && !self.is_store
    }

    /// This load as a literal load of `target`.
    fn to_literal(&self, target: u64, pc: u64) -> u32 {
        let op = match (self.size, self.is_float) {
            (4, true) => 0x1c00_0000,
            (4, false) if self.is_ldrsw => 0x9800_0000,
            (4, false) => 0x1800_0000,
            (8, true) => 0x5c00_0000,
            (8, false) => 0x5800_0000,
            _ => 0x9c00_0000,
        };
        let delta = target.wrapping_sub(pc) as u32;
        op | ((delta << 3) & 0x00ff_ffe0) | self.reg
    }

    /// This access through another base register and byte offset.
    fn with_base(&self, base: u32, offset: u64) -> u32 {
        (self.insn & 0xffc0_001f) | (base << 5) | (((offset / self.size) as u32) << 10)
    }
}

/// Whether a relocation is the page half of a reference: of a GOT
/// slot's, or one relaxed from it, if `got`.
fn is_page_rel(rel: Option<&Reloc>, got: bool) -> bool {
    match rel.map(|r| r.r_type) {
        Some(ARM64_RELOC_PAGE21) => !got,
        Some(ARM64_RELOC_GOT_LOAD_PAGE21 | ARM64_RELOC_TLVP_LOAD_PAGE21) => true,
        _ => false,
    }
}

fn is_pageoff_rel(rel: Option<&Reloc>, got: bool) -> bool {
    match rel.map(|r| r.r_type) {
        Some(ARM64_RELOC_PAGEOFF12) => !got,
        Some(ARM64_RELOC_GOT_LOAD_PAGEOFF12 | ARM64_RELOC_TLVP_LOAD_PAGEOFF12) => true,
        _ => false,
    }
}

/// An instruction a hint names: its offset in its subsection, its
/// output address and the relocation the object put on it.
#[derive(Clone, Copy, Default)]
struct HintInsn<'a> {
    off: usize,
    addr: u64,
    rel: Option<&'a Reloc>,
}

/// A hint's instructions, in their subsection's output bytes.
struct Hint<'a> {
    buf: &'a mut [u8],
    insns: [HintInsn<'a>; 3],
    n: usize,
}

impl Hint<'_> {
    fn get(&self, i: usize) -> u32 {
        read32(&self.buf[self.insns[i].off..])
    }

    fn set(&mut self, i: usize, insn: u32) {
        write32(&mut self.buf[self.insns[i].off..], insn);
    }

    fn addr(&self, i: usize) -> u64 {
        self.insns[i].addr
    }

    /// ld64's checks on the relocations: the first two instructions
    /// carry the page and page-offset halves of one reference, and a
    /// third carries none - its offset is the compiler's own.
    fn has_page_pair(&self, got: bool) -> bool {
        let (a, b) = (self.insns[0].rel, self.insns[1].rel);
        is_page_rel(a, got)
            && is_pageoff_rel(b, got)
            && a.map(|r| (r.target, r.addend)) == b.map(|r| (r.target, r.addend))
            && (self.n < 3 || self.insns[2].rel.is_none())
    }
}

/// AdrpAdrp: two adrp of one page into one register; the second is
/// redundant. It runs after the other kinds, which may have rewritten
/// either adrp.
fn loh_adrp_adrp(h: &mut Hint) {
    let (a, b) = (h.get(0), h.get(1));
    if is_page_rel(h.insns[0].rel, false)
        && is_page_rel(h.insns[1].rel, false)
        && is_adrp(a)
        && is_adrp(b)
        && a & 0x1f == b & 0x1f
        && adrp_page(a, h.addr(0)) == adrp_page(b, h.addr(1))
    {
        h.set(1, NOP);
    }
}

/// AdrpLdr: a load from within 1 MiB of the ldr becomes a literal load.
fn loh_adrp_ldr(h: &mut Hint) {
    let a = h.get(0);
    let Some(ld) = parse_ldst(h.get(1)) else { return };
    if !h.has_page_pair(false) || !is_adrp(a) || ld.base != a & 0x1f {
        return;
    }
    let target = adrp_page(a, h.addr(0)) + ld.offset;
    if ld.has_literal_form() && target.is_multiple_of(4) && within_1mb(h.addr(1), target) {
        h.set(0, NOP);
        h.set(1, ld.to_literal(target, h.addr(1)));
    }
}

/// AdrpAdd: an address within 1 MiB of the adrp becomes an adr.
fn loh_adrp_add(h: &mut Hint) {
    let a = h.get(0);
    let Some(add) = parse_add(h.get(1)) else { return };
    if !h.has_page_pair(false) || !is_adrp(a) || add.rn != a & 0x1f {
        return;
    }
    let target = adrp_page(a, h.addr(0)) + add.imm;
    if within_1mb(h.addr(0), target) {
        h.set(0, adr(add.rd, target, h.addr(0)));
        h.set(1, NOP);
    }
}

/// The adrp+add of AdrpAddLdr and AdrpAddStr, with the access through
/// it: the address the pair computes, the add, and the access.
fn adrp_add_ldst(h: &Hint) -> Option<(u64, Add, LoadStore)> {
    let a = h.get(0);
    let add = parse_add(h.get(1))?;
    let ls = parse_ldst(h.get(2))?;
    (h.has_page_pair(false) && is_adrp(a) && add.rn == a & 0x1f && ls.base == add.rd)
        .then(|| (adrp_page(a, h.addr(0)) + add.imm, add, ls))
}

/// AdrpAddLdr: a load through adrp+add. From within 1 MiB of the
/// load, it becomes a literal load (ld64's T1); within 1 MiB of the
/// adrp, the address an adr (T4); else, if the load has no offset of
/// its own, the add folds into it (T2).
fn loh_adrp_add_ldr(h: &mut Hint) {
    let Some((addr, add, ld)) = adrp_add_ldst(h) else { return };
    let target = addr + ld.offset;
    if ld.has_literal_form() && target.is_multiple_of(4) && within_1mb(h.addr(2), target) {
        h.set(0, NOP);
        h.set(1, NOP);
        h.set(2, ld.to_literal(target, h.addr(2)));
    } else if within_1mb(h.addr(0), target) {
        h.set(0, adr(ld.base, target, h.addr(0)));
        h.set(1, NOP);
        h.set(2, ld.with_base(ld.base, 0));
    } else if addr.is_multiple_of(ld.size) && ld.offset == 0 {
        h.set(1, NOP);
        h.set(2, ld.with_base(add.rn, add.imm));
    }
}

/// AdrpAddStr: a store through adrp+add, as AdrpAddLdr but for the
/// literal form stores lack. (ld64's T2 keeps the add's destination
/// as the base; it is the adrp's register in compiled code.)
fn loh_adrp_add_str(h: &mut Hint) {
    let Some((addr, add, st)) = adrp_add_ldst(h) else { return };
    if !st.is_store {
        return;
    }
    let target = addr + st.offset;
    if within_1mb(h.addr(0), target) {
        h.set(0, adr(st.base, target, h.addr(0)));
        h.set(1, NOP);
        h.set(2, st.with_base(st.base, 0));
    } else if addr.is_multiple_of(st.size) && st.offset == 0 {
        h.set(1, NOP);
        h.set(2, st.with_base(add.rn, add.imm));
    }
}

/// The GOT load of the AdrpLdrGot kinds as it now stands: a load of
/// the slot at the address given, or an add computing the target
/// itself if the load was relaxed.
enum GotLoad {
    Slot(u64, LoadStore),
    Relaxed(u64, Add),
}

fn got_load(h: &Hint) -> Option<GotLoad> {
    let a = h.get(0);
    if !h.has_page_pair(true) || !is_adrp(a) {
        return None;
    }
    let page = adrp_page(a, h.addr(0));
    let b = h.get(1);
    if let Some(ld) = parse_ldst(b) {
        let ok = ld.size == 8 && !ld.is_float && !ld.is_store && ld.base == a & 0x1f;
        return ok.then(|| GotLoad::Slot(page + ld.offset, ld));
    }
    let add = parse_add(b)?;
    (add.rn == a & 0x1f).then(|| GotLoad::Relaxed(page + add.imm, add))
}

/// AdrpLdrGot: a GOT load. A slot within 1 MiB of the load is loaded
/// by a literal load (ld64's T5); relaxed, a target within 1 MiB of
/// the adrp is computed by an adr (T4).
fn loh_adrp_ldr_got(h: &mut Hint) {
    match got_load(h) {
        Some(GotLoad::Slot(slot, got)) if within_1mb(h.addr(1), slot) => {
            h.set(0, NOP);
            h.set(1, got.to_literal(slot, h.addr(1)));
        }
        Some(GotLoad::Relaxed(target, add)) if within_1mb(h.addr(0), target) => {
            h.set(0, adr(add.rd, target, h.addr(0)));
            h.set(1, NOP);
        }
        _ => {}
    }
}

/// T5 of AdrpLdrGotLdr and AdrpLdrGotStr: the GOT slot a load or store
/// goes through is loaded by a literal load. ld64 measures the reach
/// and the alignment to the slot plus the access's offset.
fn got_literal(h: &mut Hint, slot: u64, got: &LoadStore, ls: &LoadStore) {
    let end = slot + ls.offset;
    if ls.base == got.reg
        && end.is_multiple_of(4)
        && within_1mb(h.addr(1), end)
        && within_1mb(h.addr(1), slot)
    {
        h.set(0, NOP);
        h.set(1, got.to_literal(slot, h.addr(1)));
    }
}

/// AdrpLdrGotLdr: a load through a GOT load. Relaxed, the target
/// plus the load's offset within 1 MiB of the load becomes one literal
/// load (T1), a target within 1 MiB of the adrp an adr (T4), and the
/// add folds into the load otherwise (T2).
fn loh_adrp_ldr_got_ldr(h: &mut Hint) {
    let Some(ld) = parse_ldst(h.get(2)) else { return };
    match got_load(h) {
        Some(GotLoad::Slot(slot, got)) => got_literal(h, slot, &got, &ld),
        Some(GotLoad::Relaxed(target, add)) if add.rd == ld.base => {
            if ld.has_literal_form()
                && target.is_multiple_of(4)
                && within_1mb(h.addr(2), target + ld.offset)
            {
                h.set(0, NOP);
                h.set(1, NOP);
                h.set(2, ld.to_literal(target + ld.offset, h.addr(2)));
            } else if within_1mb(h.addr(0), target) {
                h.set(0, adr(ld.base, target, h.addr(0)));
                h.set(1, NOP);
            } else if target.is_multiple_of(ld.size) && add.imm + ld.offset < 4096 {
                h.set(1, NOP);
                h.set(2, ld.with_base(add.rn, add.imm + ld.offset));
            }
        }
        _ => {}
    }
}

/// AdrpLdrGotStr: a store through a GOT load, as AdrpLdrGotLdr but
/// for the literal form stores lack, and T2 only for a store with no
/// offset of its own.
fn loh_adrp_ldr_got_str(h: &mut Hint) {
    let Some(st) = parse_ldst(h.get(2)).filter(|st| st.is_store) else { return };
    match got_load(h) {
        Some(GotLoad::Slot(slot, got)) => got_literal(h, slot, &got, &st),
        Some(GotLoad::Relaxed(target, add)) if add.rd == st.base => {
            if within_1mb(h.addr(0), target) {
                h.set(0, adr(st.base, target, h.addr(0)));
                h.set(1, NOP);
            } else if target.is_multiple_of(st.size) && st.offset == 0 {
                h.set(1, NOP);
                h.set(2, st.with_base(add.rn, add.imm));
            }
        }
        _ => {}
    }
}

/// Finds a hint's instructions where ld64 takes them (see
/// ObjectFile::hint_subsec), in a subsection that made it to the
/// output. Returns the subsection's range in the output file with the
/// instructions. A copy folded into another subsection has its hints
/// dropped with it.
fn hint_insns<'a>(
    ctx: &'a Context<Arm64>,
    obj: &ObjectFile,
    addrs: &[u64],
) -> Option<(std::ops::Range<usize>, [HintInsn<'a>; 3])> {
    let id = obj.hint_subsec(&ctx.isecs, addrs)?;
    let isec = &ctx.isecs[id];
    if !isec.is_alive()
        || isec.offset == u32::MAX
        || isec.replacement != crate::input_sections::NO_REPLACEMENT
    {
        return None;
    }

    let hdr = ctx.chunk_header(isec.output_section()?);
    let rels = ctx.isec_relocs(id);
    let mut insns = [HintInsn::default(); 3];
    for (insn, &addr) in insns.iter_mut().zip(addrs) {
        let off = addr - isec.input_addr as u64;
        let i = rels.partition_point(|r| (r.offset as u64) < off);
        *insn = HintInsn {
            off: off as usize,
            addr: hdr.addr + isec.offset as u64 + off,
            rel: rels.get(i).filter(|r| r.offset as u64 == off),
        };
    }
    let start = (hdr.fileoff + isec.offset as u64) as usize;
    Some((start..start + isec.size as usize, insns))
}

fn apply_hints(ctx: &Context<Arm64>, buf: &mut [u8]) {
    struct BufPtr(*mut u8);
    unsafe impl Sync for BufPtr {}
    let bufp = BufPtr(buf.as_mut_ptr());
    let bufp = &bufp;

    ctx.objs.par_iter().filter(|obj| obj.is_alive).for_each(|obj| {
        // AdrpAdrp goes last, as ld64's second pass.
        for last in [false, true] {
            for (kind, addrs) in &obj.loh {
                if (*kind == LOH_ARM64_ADRP_ADRP) != last {
                    continue;
                }
                let (n, apply): (usize, fn(&mut Hint)) = match *kind {
                    LOH_ARM64_ADRP_ADRP => (2, loh_adrp_adrp),
                    LOH_ARM64_ADRP_LDR => (2, loh_adrp_ldr),
                    LOH_ARM64_ADRP_ADD_LDR => (3, loh_adrp_add_ldr),
                    LOH_ARM64_ADRP_LDR_GOT_LDR => (3, loh_adrp_ldr_got_ldr),
                    LOH_ARM64_ADRP_ADD_STR => (3, loh_adrp_add_str),
                    LOH_ARM64_ADRP_LDR_GOT_STR => (3, loh_adrp_ldr_got_str),
                    LOH_ARM64_ADRP_ADD => (2, loh_adrp_add),
                    LOH_ARM64_ADRP_LDR_GOT => (2, loh_adrp_ldr_got),
                    _ => continue,
                };
                if addrs.len() != n {
                    continue;
                }
                let Some((range, insns)) = hint_insns(ctx, obj, addrs) else {
                    continue;
                };

                // SAFETY: a hint rewrites only its own subsection, one
                // no other object's hints name.
                let buf =
                    unsafe { std::slice::from_raw_parts_mut(bufp.0.add(range.start), range.len()) };
                apply(&mut Hint { buf, insns, n });
            }
        }
    });
}

impl Target for Arm64 {
    const NAME: &'static str = "arm64";
    const CPUTYPE: u32 = CPU_TYPE_ARM64;
    const CPUSUBTYPE: u32 = CPU_SUBTYPE_ARM64_ALL;
    const PAGE_SIZE: u64 = 16384;
    const STUB_SIZE: u64 = 12;
    const STUB_HELPER_HEADER_SIZE: u64 = 24;
    const STUB_HELPER_ENTRY_SIZE: u64 = 12;
    const STUB_HELPER_ENTRY_PADDING: u64 = 0;
    const UNWIND_MODE_DWARF: u32 = UNWIND_ARM64_MODE_DWARF;
    const OBJC_STUB_SIZE: u64 = 32;
    const BRANCH_RANGE: u64 = 1 << 28;
    const THUNK_SIZE: u64 = 12;
    const RELOC_UNSIGNED: u8 = ARM64_RELOC_UNSIGNED;
    const RELOC_SUBTRACTOR: u8 = ARM64_RELOC_SUBTRACTOR;
    const RELOC_GOTPC: u8 = ARM64_RELOC_POINTER_TO_GOT;
    const RELOCATABLE_GOTPC_CELL: Option<u32> = Some(4);
    const RELOC_ADDEND: u8 = ARM64_RELOC_ADDEND;
    // ARM_THREAD_STATE64: x0..x28, fp, lr, sp, then pc.
    const THREAD_STATE_FLAVOR: u32 = 6;
    const THREAD_STATE_COUNT: u32 = 68;
    const THREAD_STATE_PC_OFFSET: usize = 32 * 8;

    fn relocatable_needs_addend(r_type: u8) -> bool {
        // Instruction-patching relocations can't embed an addend; data
        // relocations keep it in the relocated bytes.
        !matches!(r_type, ARM64_RELOC_UNSIGNED | ARM64_RELOC_SUBTRACTOR)
    }

    // Both halves of an adrp+ldr GOT load relax together: the adrp
    // keeps its shape and the ldr becomes an add. So the page half is
    // always relaxable, and the offset half is if it is an ldr (of 64
    // or 32 bits; ld-prime relaxes both); an add under it takes the
    // slot's address and keeps the slot.
    fn got_load_form(r_type: u8) -> Option<u8> {
        match r_type {
            ARM64_RELOC_PAGE21 => Some(ARM64_RELOC_GOT_LOAD_PAGE21),
            ARM64_RELOC_PAGEOFF12 => Some(ARM64_RELOC_GOT_LOAD_PAGEOFF12),
            _ => None,
        }
    }

    fn can_relax_got_load(data: &[u8], offset: u32, r_type: u8) -> bool {
        r_type != ARM64_RELOC_GOT_LOAD_PAGEOFF12 || is_ldr_imm(read32(&data[offset as usize..]))
    }

    fn page_pair_half(r_type: u8) -> Option<bool> {
        match r_type {
            ARM64_RELOC_PAGE21 | ARM64_RELOC_GOT_LOAD_PAGE21 => Some(true),
            ARM64_RELOC_PAGEOFF12 | ARM64_RELOC_GOT_LOAD_PAGEOFF12 => Some(false),
            _ => None,
        }
    }

    // ld64 applies no hints to a dylib eligible for the dyld shared
    // cache: on arm64 it records split-seg info v2 for one, which lets
    // the cache builder move segments apart, out of the 1 MiB reach a
    // rewrite relies on.
    fn apply_optimization_hints(ctx: &Context<Self>, buf: &mut [u8]) {
        if !ctx.args.ignore_optimization_hints && !crate::passes::shared_region_eligible(ctx) {
            apply_hints(ctx, buf);
        }
    }

    fn classify_reloc(r_type: u8) -> crate::target::RelocClass {
        use crate::target::RelocClass;
        match r_type {
            ARM64_RELOC_BRANCH26 => RelocClass::Branch,
            ARM64_RELOC_GOT_LOAD_PAGE21 | ARM64_RELOC_GOT_LOAD_PAGEOFF12 => RelocClass::GotLoad,
            ARM64_RELOC_POINTER_TO_GOT => RelocClass::Got,
            ARM64_RELOC_TLVP_LOAD_PAGE21 | ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => RelocClass::Tlv,
            _ => RelocClass::Plain,
        }
    }

    fn write_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        for (i, &sym) in ctx.stubs.symbols.iter().enumerate() {
            let ent = &mut buf[i * 12..];
            let ent_addr = addr + i as u64 * 12;
            let ptr_addr = ctx.stub_ptr_addr(i, sym);

            // adrp x16, $ptr@PAGE; ldr x16, [x16, $ptr@PAGEOFF]; br x16
            write32(&mut ent[0..], 0x9000_0010 | page_offset(ptr_addr, ent_addr));
            write32(&mut ent[4..], 0xf940_0210 | (bits(ptr_addr, 11, 3) as u32) << 10);
            write32(&mut ent[8..], 0xd61f_0200);
        }
    }

    fn write_stub_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        // The header, as ld64 emits it:
        //   adrp x17, __dyld_private@PAGE
        //   add  x17, x17, __dyld_private@PAGEOFF
        //   stp  x16, x17, [sp, #-16]!
        //   adrp x16, dyld_stub_binder@GOTPAGE
        //   ldr  x16, [x16, dyld_stub_binder@GOTPAGEOFF]
        //   br   x16
        let private = ctx.isec_addr(ctx.stub_helper.dyld_private_isec as usize);
        let binder = ctx.sym_got_addr(ctx.stub_helper.dyld_stub_binder.unwrap());
        write32(&mut buf[0..], 0x9000_0011 | page_offset(private, addr));
        write32(&mut buf[4..], 0x9100_0231 | ((private as u32 & 0xfff) << 10));
        write32(&mut buf[8..], 0xa9bf_47f0);
        write32(&mut buf[12..], 0x9000_0010 | page_offset(binder, addr + 12));
        write32(&mut buf[16..], 0xf940_0210 | (bits(binder, 11, 3) as u32) << 10);
        write32(&mut buf[20..], 0xd61f_0200);
        // Each entry: ldr w16, #8 (the lazy-bind offset that follows);
        // b header; .long offset.
        let lazy_offsets = &ctx.lazy_bind_info.offsets[..ctx.stubs.lazy.len()];
        for (i, &lazy_off) in lazy_offsets.iter().enumerate() {
            let off = 24 + i * 12;
            let ent_addr = addr + off as u64;
            write32(&mut buf[off..], 0x1800_0050);
            let rel = addr.wrapping_sub(ent_addr + 4) as i64 >> 2;
            write32(&mut buf[off + 4..], 0x1400_0000 | (rel as u32 & 0x03ff_ffff));
            write32(&mut buf[off + 8..], lazy_off);
        }
    }

    fn write_objc_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let msgsend_got = ctx.objc_msgsend_got_addr();

        for i in 0..ctx.objc_stubs.symbols.len() {
            let ent = &mut buf[i * 32..];
            let ent_addr = addr + i as u64 * 32;
            let sel_addr = ctx.objc_selref_addr(i);

            // adrp x1, sel@PAGE; ldr x1, [x1, sel@PAGEOFF]
            // adrp x16, _objc_msgSend@GOTPAGE; ldr x16, [...]; br x16
            write32(&mut ent[0..], 0x9000_0001 | page_offset(sel_addr, ent_addr));
            write32(&mut ent[4..], 0xf940_0021 | (bits(sel_addr, 11, 3) as u32) << 10);
            write32(&mut ent[8..], 0x9000_0010 | page_offset(msgsend_got, ent_addr + 8));
            write32(&mut ent[12..], 0xf940_0210 | (bits(msgsend_got, 11, 3) as u32) << 10);
            write32(&mut ent[16..], 0xd61f_0200);
            write32(&mut ent[20..], 0xd420_0020);
            write32(&mut ent[24..], 0xd420_0020);
            write32(&mut ent[28..], 0xd420_0020);
        }
    }

    fn write_thunk(
        ctx: &Context<Self>,
        addr: u64,
        syms: &[crate::symbol::SymbolId],
        buf: &mut [u8],
    ) {
        for (i, &sym) in syms.iter().enumerate() {
            let ent = &mut buf[i * 12..];
            let ent_addr = addr + i as u64 * 12;
            let target = ctx.sym_addr(sym);

            // adrp x16, target@PAGE; add x16, x16, target@PAGEOFF; br x16
            write32(&mut ent[0..], 0x9000_0010 | page_offset(target, ent_addr));
            write32(&mut ent[4..], 0x9100_0210 | (bits(target, 11, 0) as u32) << 10);
            write32(&mut ent[8..], 0xd61f_0200);
        }
    }

    fn read_relocs(
        file_name: &Path,
        sections: &[MachSection],
        hdr: &MachSection,
        file_data: &[u8],
        rels: &[MachRel],
    ) -> Vec<Reloc> {
        let mut vec = Vec::with_capacity(rels.len());
        let mut i = 0;

        while i < rels.len() {
            // Diagnostics spell the path lossily.
            let file_name = file_name.display();
            let mut addend: i64 = 0;

            // A Mach-O relocation doesn't contain an addend. UNSIGNED
            // relocs have addends in the relocated field. Addends for
            // other types of relocations are specified by prepending an
            // ADDEND reloc.
            match rels[i].r_type() {
                ARM64_RELOC_UNSIGNED => {
                    let off = hdr.offset as usize + rels[i].r_address as usize;
                    match 1 << rels[i].r_length() {
                        4 => {
                            let val = &file_data[off..off + 4];
                            addend = i32::from_le_bytes(val.try_into().unwrap()) as i64;
                        }
                        8 => {
                            let val = &file_data[off..off + 8];
                            addend = i64::from_le_bytes(val.try_into().unwrap());
                        }
                        _ => fatal!("{file_name}: bad relocation size"),
                    }
                }
                ARM64_RELOC_ADDEND => {
                    addend = sign_extend(rels[i].r_symbolnum() as u64, 24);
                    i += 1;
                }
                _ => {}
            }

            let r = &rels[i];
            let is_subtracted = i > 0 && rels[i - 1].r_type() == ARM64_RELOC_SUBTRACTOR;

            // A relocation refers to either a symbol or a section.
            let (target, addend) = if r.is_extern() {
                (RelocTarget::Sym(r.r_symbolnum()), addend)
            } else {
                let addr = if r.is_pcrel() {
                    (hdr.addr + r.r_address as u64).wrapping_add_signed(addend)
                } else {
                    addend as u64
                };
                let Some(idx) =
                    crate::target::nonextern_target_section(sections, r.r_symbolnum(), addr)
                else {
                    fatal!("{file_name}: bad relocation: {}", r.r_address);
                };
                let target = RelocTarget::Section(idx as u32);
                (target, (addr - sections[idx].addr) as i64)
            };

            vec.push(Reloc {
                offset: r.r_address,
                r_type: r.r_type(),
                size: 1 << r.r_length(),
                is_pcrel: r.is_pcrel(),
                is_subtracted,
                target: target.pack(),
                addend,
            });
            i += 1;
        }

        vec
    }

    fn apply_relocs(
        ctx: &Context<Self>,
        rels: &[Reloc],
        isec_id: usize,
        base: u64,
        buf: &mut [u8],
    ) {
        let obj = ctx.isecs[isec_id].file as usize;
        let mut i = 0;
        while i < rels.len() {
            let r = &rels[i];
            let loc = &mut buf[r.offset as usize..];
            let s = ctx.reloc_target_addr(obj, r);
            let a = r.addend;
            let p = base + r.offset as u64;

            match r.r_type {
                ARM64_RELOC_UNSIGNED => {
                    // An imported symbol's address is written by dyld,
                    // via a bind record.
                    let imported = ctx
                        .reloc_target_sym(obj, r)
                        .is_some_and(|id| ctx.symbols[id].is_imported());
                    if imported {
                        // The slot is filled by dyld.
                    } else if ctx.reloc_target_is_tls(obj, r) {
                        // __thread_vars holds thread-pointer-relative
                        // offsets into the TLS initialization image.
                        write64(loc, s.wrapping_add_signed(a) - ctx.tls_begin);
                    } else if r.size == 4 {
                        // A 32-bit absolute address (.long sym); ld64
                        // rejects one that does not fit.
                        let val = s.wrapping_add_signed(a);
                        if val > u32::MAX as u64 {
                            fatal!(
                                "{}: 32-bit absolute address out of range ({val:#x})",
                                ctx.objs[obj].mf.name.display()
                            );
                        }
                        write32(loc, val as u32);
                    } else {
                        write64(loc, s.wrapping_add_signed(a));
                    }
                }
                ARM64_RELOC_SUBTRACTOR => {
                    // A SUBTRACTOR relocation is always followed by an
                    // UNSIGNED relocation. They work as a pair to
                    // materialize a relative address between two locations.
                    i += 1;
                    debug_assert!(rels[i].r_type == ARM64_RELOC_UNSIGNED);
                    let val = ctx
                        .reloc_target_addr(obj, &rels[i])
                        .wrapping_add_signed(rels[i].addend)
                        .wrapping_sub(s);
                    match r.size {
                        4 => write32(loc, val as u32),
                        8 => write64(loc, val),
                        _ => fatal!("bad SUBTRACTOR relocation size"),
                    }
                }
                ARM64_RELOC_BRANCH26 => {
                    let s = match ctx.reloc_target_sym(obj, r) {
                        Some(id) => ctx.branch_target_addr(id),
                        None => s,
                    };
                    let mut val = s.wrapping_add_signed(a).wrapping_sub(p) as i64;
                    if !(-(1 << 27)..1 << 27).contains(&val) {
                        // Out of reach: branch through one of the
                        // symbol's thunk entries that is within reach of
                        // here (mold's thunk_addrs lookup).
                        let thunk = ctx.reloc_target_sym(obj, r).and_then(|sym| {
                            crate::thunks::reachable_thunk_addr::<Self>(ctx, sym, p)
                        });
                        match thunk {
                            Some(t) => val = t.wrapping_sub(p) as i64,
                            None => error!("branch target out of range: {val:x}"),
                        }
                    }
                    write32(loc, read32(loc) | bits(val as u64, 27, 2) as u32);
                }
                // A TLV load of a thread-local nothing binds at run time
                // relaxes like a GOT load: the adrp retargets to the
                // __thread_vars descriptor's page and the ldr becomes an
                // add. Others load the descriptor's address from __got.
                ARM64_RELOC_TLVP_LOAD_PAGE21 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    let target = if ctx.can_relax_got(id) { s } else { ctx.sym_got_addr(id) };
                    let val = read32(loc) | page_offset(target.wrapping_add_signed(a), p);
                    write32(loc, val);
                }
                ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    if !ctx.can_relax_got(id) {
                        let t = ctx.sym_got_addr(id);
                        write_add_ldst(loc, t.wrapping_add_signed(a));
                    } else {
                        let insn = read32(loc);
                        if insn & 0xffc0_0000 != 0xf940_0000 {
                            fatal!("unexpected instruction under TLVP_LOAD_PAGEOFF12");
                        }
                        let target = s.wrapping_add_signed(a);
                        let add = 0x9100_0000 | (insn & 0x3ff) | ((target as u32 & 0xfff) << 10);
                        write32(loc, add);
                    }
                }
                ARM64_RELOC_PAGE21 => {
                    let val = read32(loc) | page_offset(s.wrapping_add_signed(a), p);
                    write32(loc, val);
                }
                ARM64_RELOC_PAGEOFF12 => {
                    write_add_ldst(loc, s.wrapping_add_signed(a));
                }
                // A GOT load of a local symbol relaxes to computing
                // the address directly: the adrp retargets from the
                // slot's page to the symbol's, and the ldr becomes
                // "add Xn, Xm, #pageoff".
                ARM64_RELOC_GOT_LOAD_PAGE21 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    let target = if !ctx.can_relax_got(id) { ctx.sym_got_addr(id) } else { s };
                    let val = read32(loc) | page_offset(target.wrapping_add_signed(a), p);
                    write32(loc, val);
                }
                ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    if !ctx.can_relax_got(id) {
                        let g = ctx.sym_got_addr(id);
                        write_add_ldst(loc, g.wrapping_add_signed(a));
                    } else {
                        let insn = read32(loc);
                        if !is_ldr_imm(insn) {
                            fatal!("unexpected instruction under GOT_LOAD_PAGEOFF12");
                        }
                        let target = s.wrapping_add_signed(a);
                        let add = 0x9100_0000 | (insn & 0x3ff) | ((target as u32 & 0xfff) << 10);
                        write32(loc, add);
                    }
                }
                ARM64_RELOC_POINTER_TO_GOT => {
                    let g = ctx.sym_got_addr(ctx.reloc_target_sym(obj, r).unwrap());
                    debug_assert!(r.size == 4);
                    write32(loc, g.wrapping_add_signed(a).wrapping_sub(p) as u32);
                }
                _ => fatal!("unsupported relocation type: {}", r.r_type),
            }
            i += 1;
        }
    }
}
