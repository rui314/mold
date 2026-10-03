//! The ARM64 (AArch64) target.

use std::path::Path;

use rayon::prelude::*;

use crate::branch_shims;
use crate::chunks::delay_init::{DelayCode, DelayTarget, DelayUse};
use crate::chunks::lazy_helpers::{LazyTarget, LazyUse};
use crate::context::Context;
use crate::dtrace::SiteKind;
use crate::error::RawPath;
use crate::fatal;
use crate::input_files::ObjectFile;
use crate::input_sections::{Reloc, RelocTarget};
use crate::macho::*;
use crate::target::{SplitRef, Target, has_reloc_form, load_helper, reloc_form};
use crate::util::{bits, sign_extend};

#[derive(Clone, Copy, Default)]
pub struct Arm64;

// The immediate fields relocations fill: B/BL's imm26, ADRP's
// immlo:immhi, and the imm12 of ADD, LDR and STR.
const B_IMM: u32 = 0x03ff_ffff;
const ADRP_IMM: u32 = 0x60ff_ffe0;
const IMM12: u32 = 0x003f_fc00;

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

/// Points the ADRP at `loc`, whose address is `lo`, at `hi`'s page. The
/// immediate the object left is replaced: under an ADDEND record,
/// ld-prime ignores it.
fn write_adrp(loc: &mut [u8], hi: u64, lo: u64) {
    write32(loc, (read32(loc) & !ADRP_IMM) | page_offset(hi, lo));
}

/// Whether an ADRP at `lo` reaches `hi`'s page: its signed 21-bit
/// immediate counts 4 KiB pages, from 4 GiB below up to 4 GiB above.
fn adrp_reaches(hi: u64, lo: u64) -> bool {
    let delta = page(hi).wrapping_sub(page(lo)) as i64;
    (-(1 << 32)..1 << 32).contains(&delta)
}

/// Checks that the ADRP of relocation `r` of subsection `isec`, at `p`,
/// reaches the page of `t`, its target or the GOT slot it loads.
fn check_adrp(ctx: &Context<Arm64>, isec: usize, r: &Reloc, p: u64, t: u64) {
    if adrp_reaches(t, p) {
        return;
    }
    let name = ctx.reloc_target_name(ctx.isecs[isec].file as usize, r);
    let name = crate::error::raw(&name);
    let msg = format_args!("ADRP out of range, from 0x{p:08X} to 0x{t:08X} ('{name}')");
    ctx.fixup_error(isec, r.offset, msg);
}

/// Whether a GOT load from subsection `isec` of symbol `id` relaxes to
/// computing the symbol's address, as ld-prime relaxes one unless dyld
/// fills the slot or the symbol is 4 GiB away (see branch_shims).
fn relaxes_got_load(ctx: &Context<Arm64>, isec: usize, id: crate::symbol::SymbolId) -> bool {
    ctx.can_relax_got(id) && !branch_shims::is_far(ctx, isec, id)
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

/// Writes an immediate to an ADD, LDR or STR instruction. Fails with
/// the access size of an LDR or STR whose target it doesn't divide.
fn write_add_ldst(loc: &mut [u8], val: u64) -> Result<(), u32> {
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
        return Err(1 << scale);
    }

    // Bits [21:10] hold the 12-bit immediate. Compilers usually leave
    // them zero, but OR-ing without clearing would mix a leftover
    // placeholder with the final page offset.
    let imm = (bits(val, 11, scale as u32) as u32) << 10;
    write32(loc, (insn & !IMM12) | imm);
    Ok(())
}

/// Reports an LDR or STR, relocation `r` of subsection `isec`, whose
/// target (or the GOT slot it loads) its access size doesn't divide.
fn report_ldst_alignment(ctx: &Context<Arm64>, isec: usize, r: &Reloc, size: u32) {
    let target = ctx.reloc_target_name(ctx.isecs[isec].file as usize, r);
    let target = crate::error::raw(&target);
    let msg = format_args!(
        "target '{target}' not {size}-byte aligned, which is required by LDR/STR instruction"
    );
    ctx.fixup_error(isec, r.offset, msg);
}

// Linker optimization hints (LC_LINKER_OPTIMIZATION_HINT). A compiler
// can't know how far a symbol will land, so it materializes addresses
// with adrp and leaves hints naming the instructions of each sequence,
// for the linker to shorten once addresses are final. ld-prime ignores
// them; they are applied here as ld64 does, under its conditions, with
// the target address read back from the relocated instructions.

const NOP: u32 = 0xd503_201f;

/// What the bl or b of a DTrace probe site becomes, if relocation `r`
/// is one (see dtrace): a nop, or for an is-enabled test "movz x0, #0",
/// its result false.
fn dtrace_site_insn(ctx: &Context<Arm64>, obj: usize, r: &Reloc) -> Option<u32> {
    let id = ctx.reloc_target_sym(obj, r)?;
    match crate::dtrace::site_kind(ctx, id)? {
        SiteKind::Probe => Some(NOP),
        SiteKind::IsEnabled => Some(0xd280_0000),
    }
}

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

/// Whether a record's pcrel, length and extern fields are ones its type
/// takes. Only an UNSIGNED may be section-relative. An assembler writes
/// other forms for `.short sym` or `.quad sym@GOT`.
#[inline]
fn is_supported(r: &MachRel) -> bool {
    let forms = match r.r_type() {
        ARM64_RELOC_UNSIGNED => {
            reloc_form(false, 2, true)
                | reloc_form(false, 3, true)
                | reloc_form(false, 2, false)
                | reloc_form(false, 3, false)
        }
        ARM64_RELOC_SUBTRACTOR => reloc_form(false, 2, true) | reloc_form(false, 3, true),
        ARM64_RELOC_BRANCH26
        | ARM64_RELOC_PAGE21
        | ARM64_RELOC_GOT_LOAD_PAGE21
        | ARM64_RELOC_TLVP_LOAD_PAGE21
        | ARM64_RELOC_POINTER_TO_GOT => reloc_form(true, 2, true),
        ARM64_RELOC_PAGEOFF12
        | ARM64_RELOC_GOT_LOAD_PAGEOFF12
        | ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => reloc_form(false, 2, true),
        ARM64_RELOC_ADDEND => reloc_form(false, 2, false),
        _ => 0,
    };
    has_reloc_form(r, forms)
}

/// Fails the link on record `i` of section `hdr` if it is one the
/// linker can't apply, `loc` being the bytes it applies to: one of a
/// form its type doesn't take, a 4-byte UNSIGNED but as the second half
/// of a SUBTRACTOR pair (`.long sym` makes one, a 32-bit pointer no
/// 64-bit image can hold), or the page offset of a GOT load on anything
/// but an add or an 8-byte load, or of a TLV load on anything but a
/// load. The assembler writes those for `ldr w0, [x0, sym@GOTPAGEOFF]`
/// or a store, and relaxing such a load to an add would compute
/// something else.
#[inline]
fn check_reloc(file: &Path, hdr: &MachSection, rels: &[MachRel], i: usize, loc: &[u8]) {
    let r = &rels[i];
    let pointer32 = r.r_type() == ARM64_RELOC_UNSIGNED
        && r.r_length() == 2
        && (i == 0 || rels[i - 1].r_type() != ARM64_RELOC_SUBTRACTOR);
    if !is_supported(r) || pointer32 {
        crate::target::bad_reloc(file, hdr, r, "unsupported relocation");
    }
    let load = || parse_ldst(read32(loc)).filter(|ls| !ls.is_store);
    let ok = match r.r_type() {
        ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
            // "add Wd|Xd, Wn|Xn, #imm" with an unshifted immediate
            read32(loc) & 0x7fc0_0000 == 0x1100_0000 || load().is_some_and(|ls| ls.size == 8)
        }
        ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => load().is_some(),
        _ => true,
    };
    if !ok {
        crate::target::bad_reloc(file, hdr, r, "relocation on an invalid instruction");
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

/// Encodes the instructions of a lazy or delay-init helper or stub at
/// `base` that refer to other places, by the index k of the
/// instruction (at base + 4k).
struct HelperInsn {
    base: u64,
}

impl HelperInsn {
    fn pc(&self, k: usize) -> u64 {
        self.base + k as u64 * 4
    }

    /// adrp xRD, target@PAGE
    fn adrp(&self, k: usize, rd: u32, target: u64) -> u32 {
        0x9000_0000 | rd | page_offset(target, self.pc(k))
    }

    /// add xRD, xRD, target@PAGEOFF
    fn add(&self, rd: u32, target: u64) -> u32 {
        0x9100_0000 | rd << 5 | rd | (target as u32 & 0xfff) << 10
    }

    /// ldr xRT, [xRT, target@PAGEOFF]
    fn ldr(&self, rt: u32, target: u64) -> u32 {
        0xf940_0000 | rt << 5 | rt | (bits(target, 11, 3) as u32) << 10
    }

    /// ldr wRT, [xRT, target@PAGEOFF]
    fn ldr_w(&self, rt: u32, target: u64) -> u32 {
        0xb940_0000 | rt << 5 | rt | (bits(target, 11, 2) as u32) << 10
    }

    /// b target
    fn b(&self, k: usize, target: u64) -> u32 {
        0x1400_0000 | (target.wrapping_sub(self.pc(k)) >> 2) as u32 & B_IMM
    }

    /// bl target
    fn bl(&self, k: usize, target: u64) -> u32 {
        0x9400_0000 | (target.wrapping_sub(self.pc(k)) >> 2) as u32 & B_IMM
    }

    /// A load helper's last instruction, instruction k: ret, or in a
    /// site's own helper a branch back past the site's adrp (see
    /// LazyUse::Load).
    fn ret_or_back(&self, ctx: &Context<Arm64>, k: usize, site: Option<(u32, u32)>) -> u32 {
        match site {
            None => 0xd65f_03c0,
            Some((isec, off)) => self.b(k, ctx.isec_addr(isec as usize) + off as u64 + 4),
        }
    }
}

/// Writes instructions one after another from the start of `loc`.
fn write_code(loc: &mut [u8], code: &[u32]) {
    for (loc, &insn) in loc.as_chunks_mut::<4>().0.iter_mut().zip(code) {
        *loc = insn.to_le_bytes();
    }
}

/// A dlopen helper, but for instructions 16-17 (adrp/add x0 of the
/// install name), 19 (bl dlopen) and 20-21 (adrp/add x1 of the flag).
const DLOPEN_HELPER: [u32; 40] = [
    0xd104_83ff, // sub sp, sp, #0x120
    0xa900_03e1, // stp x1, x0, [sp]
    0xa901_0be3, // stp x3, x2, [sp, #0x10]
    0xa902_13e5,
    0xa903_1be7,
    0xa904_23e9,
    0xa905_2beb,
    0xa906_33ed,
    0xa907_3bef,
    0xa908_43f1, // stp x17, x16, [sp, #0x80]
    0xad04_83e1, // stp q1, q0, [sp, #0x90]
    0xad05_8be3,
    0xad06_93e5,
    0xad07_9be7, // stp q7, q6, [sp, #0xf0]
    0xa911_7bfd, // stp x29, x30, [sp, #0x110]
    0x9104_83fd, // add x29, sp, #0x120
    0,
    0,
    0xd280_0001, // mov x1, #0
    0,
    0,
    0,
    0x5280_0020, // mov w0, #1
    0x889f_fc20, // stlr w0, [x1]
    0xad47_9be7, // ldp q7, q6, [sp, #0xf0]
    0xad46_93e5,
    0xad45_8be3,
    0xad44_83e1,
    0xa948_43f1, // ldp x17, x16, [sp, #0x80]
    0xa947_3bef,
    0xa946_33ed,
    0xa945_2beb,
    0xa944_23e9,
    0xa943_1be7,
    0xa942_13e5,
    0xa941_0be3,
    0xa940_03e1, // ldp x1, x0, [sp]
    0xa951_7bfd, // ldp x29, x30, [sp, #0x110]
    0x9104_83ff, // add sp, sp, #0x120
    0xd65f_03c0, // ret
];

impl Target for Arm64 {
    const NAME: &'static str = "arm64";
    const CPUTYPE: u32 = CPU_TYPE_ARM64;
    const CPUSUBTYPE: u32 = CPU_SUBTYPE_ARM64_ALL;
    const PAGE_SIZE: u64 = 16384;
    const STUB_SIZE: u64 = 12;
    const STUB_HELPER_HEADER_SIZE: u64 = 24;
    const STUB_HELPER_ENTRY_SIZE: u64 = 12;
    const UNWIND_MODE_DWARF: u32 = UNWIND_ARM64_MODE_DWARF;
    const OBJC_STUB_SIZE: u64 = 32;
    const OBJC_SMALL_STUB_SIZE: u64 = 12;
    const LAZY_HELPERS_P2ALIGN: u32 = 2;
    const LAZY_CALL_OWN_SLOT: bool = false;
    const DELAY_STUB_SIZE: u64 = 40;
    const DLOPEN_HELPER_SIZE: u32 = 160;
    const DELAY_P2ALIGN: u32 = 2;
    const BRANCH_RANGE: u64 = 1 << 28;
    const THUNK_SIZE: u64 = 12;
    const RELOC_UNSIGNED: u8 = ARM64_RELOC_UNSIGNED;
    const RELOC_SUBTRACTOR: u8 = ARM64_RELOC_SUBTRACTOR;
    const RELOC_GOTPC: u8 = ARM64_RELOC_POINTER_TO_GOT;
    const RELOC_ADDEND: u8 = ARM64_RELOC_ADDEND;
    const SPLIT_PCREL_KINDS: &'static [u8] =
        &[DYLD_CACHE_ADJ_V2_ARM64_ADRP, DYLD_CACHE_ADJ_V2_ARM64_OFF12];
    const STUB_REF_OFF: u64 = 0;
    const STUB_HELPER_REF_OFFS: [u64; 2] = [0, 12];
    const OBJC_STUB_REF_OFFS: [u64; 2] = [0, 8];
    // ARM_THREAD_STATE64: x0..x28, fp, lr, sp, then pc.
    const THREAD_STATE_FLAVOR: u32 = 6;
    const THREAD_STATE_COUNT: u32 = 68;
    const THREAD_STATE_SP_OFFSET: usize = 31 * 8;
    const THREAD_STATE_PC_OFFSET: usize = 32 * 8;

    fn relocatable_needs_addend(r_type: u8) -> bool {
        // Instruction-patching relocations can't embed an addend; data
        // relocations keep it in the relocated bytes.
        !matches!(r_type, ARM64_RELOC_UNSIGNED | ARM64_RELOC_SUBTRACTOR)
    }

    // ld64 applies no hints to an image bound for the dyld shared cache
    // (resolve_shared_region sets -ignore_optimization_hints): on arm64
    // it records split-seg info v2 for one, which lets the cache builder
    // move segments apart, out of the 1 MiB reach a rewrite relies on.
    fn apply_optimization_hints(ctx: &Context<Self>, buf: &mut [u8]) {
        if !ctx.args.ignore_optimization_hints {
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

    fn split_ref(r_type: u8) -> SplitRef {
        match r_type {
            ARM64_RELOC_SUBTRACTOR => SplitRef::Subtractor,
            ARM64_RELOC_PAGE21 | ARM64_RELOC_GOT_LOAD_PAGE21 | ARM64_RELOC_TLVP_LOAD_PAGE21 => {
                SplitRef::Page
            }
            ARM64_RELOC_PAGEOFF12
            | ARM64_RELOC_GOT_LOAD_PAGEOFF12
            | ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => SplitRef::PageOff,
            ARM64_RELOC_BRANCH26 => SplitRef::Branch26,
            ARM64_RELOC_POINTER_TO_GOT => SplitRef::PcRel32,
            _ => SplitRef::Pointer,
        }
    }

    fn write_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        for (i, &sym) in ctx.stubs.symbols.iter().enumerate() {
            let ent = &mut buf[i * 12..];
            let ent_addr = addr + i as u64 * 12;
            let ptr_addr = ctx.stub_ptr_addr(i, sym);
            if !adrp_reaches(ptr_addr, ent_addr) {
                crate::error!(
                    "stub for {}: ADRP out of range, from 0x{ent_addr:08X} to its pointer at 0x{ptr_addr:08X}",
                    ctx.symbols[sym]
                );
            }

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
        if ctx.args.objc_stubs_small {
            let msgsend = ctx.branch_target_addr(ctx.objc_stubs.msgsend_sym.unwrap());
            for i in 0..ctx.objc_stubs.symbols.len() {
                let ent = &mut buf[i * 12..];
                let ent_addr = addr + i as u64 * 12;
                let sel_addr = ctx.objc_selref_addr(i);

                // adrp x1, sel@PAGE; ldr x1, [x1, sel@PAGEOFF]
                // b _objc_msgSend
                write32(&mut ent[0..], 0x9000_0001 | page_offset(sel_addr, ent_addr));
                write32(&mut ent[4..], 0xf940_0021 | (bits(sel_addr, 11, 3) as u32) << 10);
                let disp = msgsend.wrapping_sub(ent_addr + 8);
                write32(&mut ent[8..], 0x1400_0000 | (disp >> 2) as u32 & B_IMM);
            }
            return;
        }

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

    fn write_lazy_helpers(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let lazy_load = ctx.sym_stub_addr(ctx.lazy_helpers.dyld_lazy_load.unwrap());
        let header = ctx.mach_header.hdr.addr;
        for h in &ctx.lazy_helpers.helpers {
            let insn = HelperInsn { base: addr + h.offset as u64 };
            let flag = ctx.isec_addr(h.flag as usize);
            let slot = ctx.lazy_load_got.slot_addr(h.slot);
            // From instruction k: __dyld_lazy_load(&flag, mach header),
            // by which dyld finds the dylib's record.
            let call = |k: usize| {
                [
                    insn.adrp(k, 0, flag),
                    insn.add(0, flag),
                    insn.adrp(k + 2, 1, header),
                    insn.add(1, header),
                    insn.bl(k + 4, lazy_load),
                ]
            };
            let code: [u32; 16] = match h.kind {
                //    adrp x16, flag@PAGE; ldr w16, [x16, flag@PAGEOFF]
                //    cbz w16, 1f
                // 2: adrp x16, slot@PAGE; ldr x16, [x16, slot@PAGEOFF]
                //    br x16
                // 1: stp x1, x0, [sp, #-16]!; stp x29, x30, [sp, #-16]!
                //    (the call); ldp x29, x30, [sp], #16
                //    ldp x1, x0, [sp], #16; b 2b
                LazyUse::Call => {
                    let [c0, c1, c2, c3, c4] = call(8);
                    [
                        insn.adrp(0, 16, flag),
                        insn.ldr_w(16, flag),
                        0x3400_0090,
                        insn.adrp(3, 16, slot),
                        insn.ldr(16, slot),
                        0xd61f_0200,
                        0xa9bf_03e1,
                        0xa9bf_7bfd,
                        c0,
                        c1,
                        c2,
                        c3,
                        c4,
                        0xa8c1_7bfd,
                        0xa8c1_03e1,
                        insn.b(15, insn.pc(3)),
                    ]
                }
                //    adrp xN, flag@PAGE; ldr wN, [xN, flag@PAGEOFF]
                //    cbnz wN, 1f
                //    stp x1, x0, [sp, #-16]!; stp x16, x17, [sp, #-16]!
                //    stp x29, x30, [sp, #-16]!; (the call)
                //    ldp x29, x30, [sp], #16; ldp x16, x17, [sp], #16
                //    ldp x1, x0, [sp], #16
                // 1: adrp xN, slot@PAGE; ret (or b back past the adrp)
                LazyUse::Load { reg, site } => {
                    let [c0, c1, c2, c3, c4] = call(6);
                    let rd = reg as u32;
                    [
                        insn.adrp(0, rd, flag),
                        insn.ldr_w(rd, flag),
                        0x3500_0180 | rd,
                        0xa9bf_03e1,
                        0xa9bf_47f0,
                        0xa9bf_7bfd,
                        c0,
                        c1,
                        c2,
                        c3,
                        c4,
                        0xa8c1_7bfd,
                        0xa8c1_47f0,
                        0xa8c1_03e1,
                        insn.adrp(14, rd, slot),
                        insn.ret_or_back(ctx, 15, site),
                    ]
                }
                LazyUse::Cmp => unreachable!(),
            };
            write_code(&mut buf[h.offset as usize..], &code);
        }
    }

    fn lazy_helper_size(_kind: LazyUse) -> u32 {
        64
    }

    //    adrp x16, flag@PAGE; add x16, x16, flag@PAGEOFF; ldar w16, [x16]
    //    cbnz w16, 1f
    //    stp x29, x30, [sp, #-16]!; bl dlopen helper; ldp x29, x30, [sp], #16
    // 1: adrp x16, slot@PAGE; ldr x16, [x16, slot@PAGEOFF]; br x16
    fn write_delay_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        for (i, stub) in ctx.delay_init.stubs.iter().enumerate() {
            let off = i * Self::DELAY_STUB_SIZE as usize;
            let insn = HelperInsn { base: addr + off as u64 };
            let flag = ctx.isec_addr(ctx.delay_init.dlopens[stub.dlopen as usize].flag as usize);
            let helper = ctx.dlopen_helper_addr(stub.dlopen as usize);
            let slot = ctx.got.slot_addr(stub.got as usize);
            let code = [
                insn.adrp(0, 16, flag),
                insn.add(16, flag),
                0x88df_fe10,
                0x3500_0090,
                0xa9bf_7bfd,
                insn.bl(5, helper),
                0xa8c1_7bfd,
                insn.adrp(7, 16, slot),
                insn.ldr(16, slot),
                0xd61f_0200,
            ];
            write_code(&mut buf[off..], &code);
        }
    }

    //    adrp xN, flag@PAGE; add xN, xN, flag@PAGEOFF; ldar wN, [xN]
    //    cbnz wN, 1f
    //    stp x29, x30, [sp, #-16]!; bl dlopen helper; ldp x29, x30, [sp], #16
    // 1: adrp xN, slot@PAGE; ret (or b back past the adrp)
    //
    // A dlopen helper calls dlopen(install name, 0) with the argument
    // and scratch registers x0-x17 and q0-q7 saved, then sets the flag
    // with a store-release.
    fn write_delay_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let delay = &ctx.delay_init;
        for h in &delay.helpers {
            let DelayUse::Load { reg, site } = h.kind else { unreachable!() };
            let (insn, rd) = (HelperInsn { base: addr + h.offset as u64 }, reg as u32);
            let flag = ctx.isec_addr(delay.dlopens[h.dlopen as usize].flag as usize);
            let helper = ctx.dlopen_helper_addr(h.dlopen as usize);
            let slot = ctx.sym_got_addr(h.sym);
            let code = [
                insn.adrp(0, rd, flag),
                insn.add(rd, flag),
                0x88df_fc00 | rd << 5 | rd,
                0x3500_0080 | rd,
                0xa9bf_7bfd,
                insn.bl(5, helper),
                0xa8c1_7bfd,
                insn.adrp(7, rd, slot),
                insn.ret_or_back(ctx, 8, site),
            ];
            write_code(&mut buf[h.offset as usize..], &code);
        }
        for d in &delay.dlopens {
            let insn = HelperInsn { base: addr + d.offset as u64 };
            let (name, flag) = (ctx.isec_addr(d.string as usize), ctx.isec_addr(d.flag as usize));
            let dlopen = ctx.sym_stub_addr(delay.dlopen_sym.unwrap());
            let mut code = DLOPEN_HELPER;
            code[16] = insn.adrp(16, 0, name);
            code[17] = insn.add(0, name);
            code[19] = insn.bl(19, dlopen);
            code[20] = insn.adrp(20, 1, flag);
            code[21] = insn.add(1, flag);
            write_code(&mut buf[d.offset as usize..], &code);
        }
    }

    fn delay_helper_size(_kind: DelayUse) -> u32 {
        36
    }

    fn delay_refs(code: DelayCode) -> Vec<(u32, u8, DelayTarget)> {
        use DelayTarget::*;
        let (adrp, off12, br26) = (
            DYLD_CACHE_ADJ_V2_ARM64_ADRP,
            DYLD_CACHE_ADJ_V2_ARM64_OFF12,
            DYLD_CACHE_ADJ_V2_ARM64_BR26,
        );
        match code {
            DelayCode::Stub => vec![
                (0, adrp, Flag),
                (4, off12, Flag),
                (20, br26, DlopenHelper),
                (28, adrp, Slot),
                (32, off12, Slot),
            ],
            DelayCode::Helper(_) => vec![
                (0, adrp, Flag),
                (4, off12, Flag),
                (20, br26, DlopenHelper),
                (28, adrp, Slot),
                (32, br26, Site),
            ],
            DelayCode::Dlopen => vec![
                (64, adrp, Name),
                (68, off12, Name),
                (76, br26, Dlopen),
                (80, adrp, Flag),
                (84, off12, Flag),
            ],
        }
    }

    fn lazy_helper_refs(kind: LazyUse) -> Vec<(u32, u8, LazyTarget)> {
        use LazyTarget::*;
        let (adrp, off12, br26) = (
            DYLD_CACHE_ADJ_V2_ARM64_ADRP,
            DYLD_CACHE_ADJ_V2_ARM64_OFF12,
            DYLD_CACHE_ADJ_V2_ARM64_BR26,
        );
        let mut refs = vec![(0, adrp, Flag), (4, off12, Flag)];
        let call = |k: u32| {
            let k = k * 4;
            [(k, adrp, Flag), (k + 4, off12, Flag), (k + 8, adrp, Header), (k + 12, off12, Header)]
                .into_iter()
                .chain([(k + 16, br26, LazyLoad)])
        };
        match kind {
            LazyUse::Call => {
                refs.extend([(12, adrp, Slot), (16, off12, Slot)]);
                refs.extend(call(8));
            }
            LazyUse::Load { site, .. } => {
                refs.extend(call(6));
                refs.push((56, adrp, Slot));
                if site.is_some() {
                    refs.push((60, br26, Site));
                }
            }
            LazyUse::Cmp => unreachable!(),
        }
        refs
    }

    fn lazy_ref(r: &Reloc, _data: &[u8]) -> crate::target::LazyRef {
        use crate::target::LazyRef;
        match r.r_type {
            ARM64_RELOC_BRANCH26 => LazyRef::Call,
            ARM64_RELOC_GOT_LOAD_PAGE21 => LazyRef::Load,
            ARM64_RELOC_GOT_LOAD_PAGEOFF12 => LazyRef::Slot,
            _ => LazyRef::Unsupported,
        }
    }

    // The adrp's register. ld-prime takes code whose link register is
    // saved - a frame record stored (stp x29, x30, [sp...], of any
    // addressing form) before the adrp in its subsection - to be free to
    // call the helper; other code branches to a helper of its own.
    fn lazy_load_site(data: &[u8], offset: u32) -> (u8, bool) {
        let reg = read32(&data[offset as usize..]) & 0x1f;
        let (insns, _) = data[..offset as usize].as_chunks::<4>();
        let framed =
            insns.iter().any(|&insn| u32::from_le_bytes(insn) & 0x3c40_7fff == 0x2800_7bfd);
        (reg as u8, !framed)
    }

    fn lazy_register_name(reg: u8) -> String {
        format!("x{reg}")
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
            let target = ctx.branch_target_addr(sym);

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
        contents: &[u8],
        rels: &[MachRel],
    ) -> Vec<Reloc> {
        let mut vec = Vec::with_capacity(rels.len());
        let mut i = 0;

        while i < rels.len() {
            // A Mach-O relocation doesn't contain an addend. UNSIGNED
            // relocs have addends in the relocated field. Addends for
            // other types of relocations are specified by prepending an
            // ADDEND reloc, whose address ld-prime takes for the pair's.
            let offset = rels[i].r_address;
            let loc = &contents[offset as usize..];
            let mut addend = 0;
            if rels[i].r_type() == ARM64_RELOC_ADDEND {
                addend = sign_extend(rels[i].r_symbolnum() as u64, 24);
                i += 1;
            }

            let r = &rels[i];
            check_reloc(file_name, hdr, rels, i, loc);
            if r.r_type() == ARM64_RELOC_UNSIGNED {
                addend = match r.r_length() {
                    2 => i32::from_le_bytes(loc[..4].try_into().unwrap()) as i64,
                    _ => i64::from_le_bytes(loc[..8].try_into().unwrap()),
                };
            }
            let is_subtracted = i > 0 && rels[i - 1].r_type() == ARM64_RELOC_SUBTRACTOR;

            // A relocation refers to either a symbol or a section. Only
            // an UNSIGNED can be section-relative, and it holds the
            // target's address.
            let (target, addend) = if r.is_extern() {
                (RelocTarget::Sym(r.r_symbolnum()), addend)
            } else {
                let addr = addend as u64;
                let Some(idx) = crate::target::nonextern_target_section(sections, r.r_section())
                else {
                    fatal!("{}: bad relocation: {}", file_name.raw(), r.r_address);
                };
                let target = RelocTarget::Section(idx as u32);
                (target, addr.wrapping_sub(sections[idx].addr) as i64)
            };

            vec.push(Reloc {
                offset,
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
                    ctx.check_text_reloc(isec_id, rels, i, p);
                    // An imported symbol's address is written by dyld,
                    // via a bind record (an interposable export's too).
                    let imported =
                        ctx.reloc_target_sym(obj, r).is_some_and(|id| ctx.binds_as_import(id));
                    if imported {
                        // The slot is filled by dyld.
                    } else if ctx.reloc_target_is_tls(obj, r) {
                        // __thread_vars holds thread-pointer-relative
                        // offsets into the TLS initialization image.
                        write64(loc, s.wrapping_add_signed(a).wrapping_sub(ctx.tls_begin));
                    } else {
                        // Only a SUBTRACTOR's pair is 4 bytes long.
                        write64(loc, s.wrapping_add_signed(a));
                    }
                }
                ARM64_RELOC_SUBTRACTOR => {
                    // A SUBTRACTOR relocation is always followed by an
                    // UNSIGNED relocation of its size. They work as a
                    // pair to materialize a relative address between two
                    // locations.
                    i += 1;
                    let val = ctx
                        .reloc_target_addr(obj, &rels[i])
                        .wrapping_add_signed(rels[i].addend)
                        .wrapping_sub(s);
                    if r.size == 4 {
                        write32(loc, val as u32);
                    } else {
                        write64(loc, val);
                    }
                }
                ARM64_RELOC_BRANCH26 => {
                    // A DTrace probe site does nothing (see dtrace).
                    if let Some(insn) = dtrace_site_insn(ctx, obj, r) {
                        write32(loc, insn);
                        i += 1;
                        continue;
                    }
                    // A branch from 4 GiB away goes through its target's
                    // shim (see branch_shims).
                    let sym = ctx.reloc_target_sym(obj, r);
                    let shim = sym.filter(|&id| {
                        a == 0 && ctx.has_branch_shim(id) && branch_shims::is_far(ctx, isec_id, id)
                    });
                    let s = match (shim, sym) {
                        (Some(id), _) => ctx.sym_stub_addr(id),
                        (None, Some(id)) => ctx.branch_target_addr(id),
                        (None, None) => s,
                    };
                    let t = s.wrapping_add_signed(a);
                    let mut val = t.wrapping_sub(p) as i64;
                    if !(-(1 << 27)..1 << 27).contains(&val) {
                        // Out of reach: branch through one of the
                        // symbol's thunk entries that is within reach of
                        // here (mold's thunk_addrs lookup) - if its ADRP
                        // reaches the target. ld-prime has no island
                        // either for a target more than 4 GiB away. An
                        // entry jumps to its symbol, so a branch with an
                        // addend can't take one (ld-prime's branches to
                        // its island plus the addend, past the island).
                        let thunk = sym.filter(|_| a == 0 && shim.is_none()).and_then(|sym| {
                            crate::thunks::reachable_thunk_addr::<Self>(ctx, sym, p)
                        });
                        match thunk {
                            Some(thunk) if adrp_reaches(t, thunk) => {
                                val = thunk.wrapping_sub(p) as i64
                            }
                            _ => {
                                let name = ctx.reloc_target_name(obj, r);
                                let name = crate::error::raw(&name);
                                let msg = format_args!(
                                    "B/BL out of range (displacement={val}, max is +/-128MB), \
                                     from 0x{p:08X} to 0x{t:08X} ('{name}')"
                                );
                                ctx.fixup_error(isec_id, r.offset, msg);
                            }
                        }
                    }
                    write32(loc, (read32(loc) & !B_IMM) | bits(val as u64, 27, 2) as u32);
                }
                // A TLV load of a thread-local nothing binds at run time
                // relaxes like a GOT load: the adrp retargets to the
                // __thread_vars descriptor's page and the ldr becomes an
                // add. Others load the descriptor's address from __got.
                ARM64_RELOC_TLVP_LOAD_PAGE21 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    let target = if ctx.can_relax_got(id) { s } else { ctx.sym_got_addr(id) };
                    check_adrp(ctx, isec_id, r, p, target.wrapping_add_signed(a));
                    write_adrp(loc, target.wrapping_add_signed(a), p);
                }
                ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    if !ctx.can_relax_got(id) {
                        let t = ctx.sym_got_addr(id);
                        if let Err(size) = write_add_ldst(loc, t.wrapping_add_signed(a)) {
                            report_ldst_alignment(ctx, isec_id, r, size);
                        }
                    } else {
                        // ld-prime relaxes an ldr of either width.
                        let insn = read32(loc);
                        if is_ldr_imm(insn) {
                            let target = s.wrapping_add_signed(a);
                            let add =
                                0x9100_0000 | (insn & 0x3ff) | ((target as u32 & 0xfff) << 10);
                            write32(loc, add);
                        } else {
                            ctx.fixup_error(isec_id, r.offset, format_args!("non-LDR instruction"));
                        }
                    }
                }
                ARM64_RELOC_PAGE21 => {
                    if ctx.target_has_address(obj, isec_id, r) {
                        check_adrp(ctx, isec_id, r, p, s.wrapping_add_signed(a));
                        write_adrp(loc, s.wrapping_add_signed(a), p);
                    }
                }
                // (ld-prime checks the alignment only of an offset
                // whose adrp it hasn't paired with it, and truncates
                // the others.)
                ARM64_RELOC_PAGEOFF12 => {
                    if ctx.target_has_address(obj, isec_id, r)
                        && let Err(size) = write_add_ldst(loc, s.wrapping_add_signed(a))
                    {
                        report_ldst_alignment(ctx, isec_id, r, size);
                    }
                }
                // A GOT load of a lazy or delay-init dylib's symbol calls
                // its load helper in place of the adrp, or in frameless
                // code branches to one of its own (see LazyUse::Load and
                // DelayUse::Load). The ldr then loads from the lazy
                // helper's slot, or the symbol's __got slot.
                ARM64_RELOC_GOT_LOAD_PAGE21
                    if let Some((helper, own)) = load_helper(ctx, isec_id, r) =>
                {
                    let op = if own { 0x1400_0000 } else { 0x9400_0000 };
                    write32(loc, op | bits(helper.wrapping_sub(p), 27, 2) as u32);
                }
                // A GOT load of a local symbol relaxes to computing
                // the address directly (see relaxes_got_load): the adrp
                // retargets from the slot's page to the symbol's, and
                // the ldr becomes "add Xn, Xm, #pageoff". ld-prime takes
                // a 64-bit add as one already, and refuses any other
                // instruction.
                ARM64_RELOC_GOT_LOAD_PAGE21 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    let target =
                        if relaxes_got_load(ctx, isec_id, id) { s } else { ctx.sym_got_addr(id) };
                    check_adrp(ctx, isec_id, r, p, target.wrapping_add_signed(a));
                    write_adrp(loc, target.wrapping_add_signed(a), p);
                }
                ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    if !relaxes_got_load(ctx, isec_id, id) {
                        let g = ctx.sym_got_addr(id);
                        if let Err(size) = write_add_ldst(loc, g.wrapping_add_signed(a)) {
                            report_ldst_alignment(ctx, isec_id, r, size);
                        }
                    } else {
                        let insn = read32(loc);
                        if is_ldr_imm(insn) || insn & 0xffc0_0000 == 0x9100_0000 {
                            let target = s.wrapping_add_signed(a);
                            let add =
                                0x9100_0000 | (insn & 0x3ff) | ((target as u32 & 0xfff) << 10);
                            write32(loc, add);
                        } else {
                            ctx.fixup_error(isec_id, r.offset, format_args!("non-LDR instruction"));
                        }
                    }
                }
                ARM64_RELOC_POINTER_TO_GOT => {
                    let g = ctx.sym_got_addr(ctx.reloc_target_sym(obj, r).unwrap());
                    write32(loc, g.wrapping_add_signed(a).wrapping_sub(p) as u32);
                }
                _ => fatal!("unsupported relocation type: {}", r.r_type),
            }
            i += 1;
        }
    }
}
