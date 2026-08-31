//! This file contains code for the 64-bit PowerPC ELFv1 ABI that is
//! commonly used for big-endian PPC systems. Modern PPC systems that use
//! the processor in the little-endian mode use the ELFv2 ABI instead. For
//! ELFv2, see arch-ppc64v2.cc.
//!
//! Even though they are similiar, ELFv1 isn't only different from ELFv2 in
//! endianness. The most notable difference is, in ELFv1, a function
//! pointer doesn't directly refer to the entry point of a function but
//! instead refers to a data structure so-called "function descriptor".
//!
//! The function descriptor is essentially a pair of a function entry point
//! address and a value that should be set to %r2 before calling that
//! function. There is also a third member for "the environment pointer for
//! languages such as Pascal and PL/1" according to the psABI, but it looks
//! like no one acutally uses it. In total, the function descriptor is 24
//! bytes long. Here is why we need it.
//!
//! PPC generally lacks PC-relative data access instructions. Position-
//! independent code sets GOT + 0x8000 to %r2 and access global variables
//! relative to %r2.
//!
//! Each ELF file has its own GOT. If a function calls another function in
//! the same ELF file, it doesn't have to reset %r2. However, if it is in
//! other file (e.g. other .so), it has to set a new value to %r2 so that
//! the register contains the callee's GOT + 0x8000.
//!
//! In this way, you can't call a function just by knowing the function's
//! entry point address. You also need to know a proper %r2 value for the
//! function. This is why a function pointer refers to a tuple of an
//! address and a %r2 value.
//!
//! If a function call is made through PLT, PLT takes care of restoring %r2.
//! Therefore, the caller has to restore %r2 only for function calls
//! through function pointers.
//!
//! .opd (short for "official procedure descriptors") contains function
//! descriptors.
//!
//! You can think OPD as this: even in other targets, a function can have a
//! few different addresses for different purposes. It may not only have an
//! entry point address but may also have PLT and/or GOT addresses.
//! In PPCV1, it may have an OPD address in addition to these. OPD address
//! is used for relocations that refers to the address of a function as a
//! function pointer.
//!
//! https://github.com/rui314/psabi/blob/main/ppc64v1.pdf

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::arch::{Arch, Family, ThunkLayout};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::input_sections::{check_tlsle, InputSection, SectionRef};
use crate::output_chunks::eh_frame;
use crate::symbol::{
    AddrFlags, Symbol, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT, NEEDS_PPC_OPD, NEEDS_TLSGD,
};
use crate::thunks::Thunk;
use crate::util::{bits, is_int};
use crate::{error, fatal};

#[derive(Clone, Copy, Debug, Default)]
pub struct Ppc64V1;

impl Layout for Ppc64V1 {
    type Endian = BigEndian;
    const IS_64: bool = true;
    const IS_RELA: bool = true;
}

fn lo(x: u64) -> u64 {
    x & 0xffff
}

fn hi(x: u64) -> u64 {
    x >> 16
}

fn ha(x: u64) -> u64 {
    x.wrapping_add(0x8000) >> 16
}

fn high(x: u64) -> u64 {
    hi(x) & 0xffff
}

fn higha(x: u64) -> u64 {
    ha(x) & 0xffff
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

fn or16(loc: &mut [u8], v: u64) {
    let cur = BigEndian::read_u16(loc);
    BigEndian::write_u16(loc, cur | v as u16);
}

fn or32(loc: &mut [u8], v: u64) {
    let cur = r32(loc);
    BigEndian::write_u32(loc, cur | v as u32);
}

fn write_insns(buf: &mut [u8], insns: &[u32]) {
    for (i, &insn) in insns.iter().enumerate() {
        BigEndian::write_u32(&mut buf[i * 4..], insn);
    }
}

/// The address of the TOC pointer, `.got + 0x8000`.
fn toc(ctx: &Context<Ppc64V1>) -> u64 {
    ctx.symbols[ctx.syms.toc.expect("PPC64 has a .TOC. symbol")].addr(ctx)
}

// Compiler creates an .opd entry for each function symbol. The intention
// is to make it possible to create an output .opd section just by linking
// input .opd sections in the same manner as we do to other normal input
// sections.
//
// However, in reality, .opd isn't a normal input section. It needs many
// special treatments as follows:
//
// 1. A function symbol refers to not a .text but an .opd. Its address
//    works fine for address-taking relocations such as R_PPC64_ADDR64.
//    However, R_PPC64_REL24 (which is used for branch instruction) needs
//    a function's real address instead of the function's .opd address.
//    We need to read .opd contents to find out a function entry point
//    address to apply R_PPC64_REL24.
//
// 2. Output .opd entries are needed only for functions whose addresses
//    are taken. Just copying input .opd sections to an output would
//    produces lots of dead .opd entries.
//
// 3. In this design, all function symbols refer to an .opd section, and
//    that doesn't work well with graph traversal optimizations such as
//    garbage collection or identical comdat folding. For example, garbage
//    collector would mark an .opd alive which in turn mark all functions
//    thatare referenced by .opd as alive, effectively keeping all
//    functions as alive.
//
// The problem is that the compiler creates a half-baked .opd section, and
// the linker has to figure out what all these .opd entries and
// relocations are trying to achieve. It's like the compiler would emit a
// half-baked .plt section in an object file and the linker has to deal
// with that. That's not a good design.
//
// So, in this function, we undo what the compiler did to .opd. We remove
// function symbols from .opd and reattach them to their function entry
// points. We also rewrite relocations that directly refer to an input
// .opd  section so that they refer to function symbols instead. We then
// mark input .opd sections as dead.
//
// After this function, we mark symbols with the NEEDS_PPC_OPD flag if the
// symbol needs an .opd entry. We then create an output .opd just like we
// do for .plt or .got.
pub fn rewrite_opd(ctx: &mut Context<Ppc64V1>) {
    let _t = ctx.timer("rewrite_opd");

    let obj_ids: Vec<_> = ctx.objs.iter().map(|file| file.id()).collect();
    for obj_id in obj_ids {
        let i = obj_id.index();
        let Some(opd) = ctx.objs[i]
            .input_sections()
            .find(|s| s.name(&ctx.objs[i]) == ".opd")
            .map(|s| SectionRef {
                file: s.file,
                shndx: s.shndx,
            })
        else {
            continue;
        };
        ctx.objs[i].kill_section(opd.shndx as usize);

        let local_symbols = ctx.objs[i].base.symbols.clone();
        let rels_at: HashMap<u64, ElfRel> = ctx.objs[i]
            .section_at(opd.shndx)
            .rels::<Ppc64V1>(&ctx.objs[i])
            .iter()
            .map(|r| (r.r_offset, r))
            .collect();

        // Move symbols from .opd to .text.
        let mut descriptors: Vec<(u64, u32)> = Vec::new(); // (offset in .opd, local symbol index)
        for (idx, &id) in local_symbols.iter().enumerate() {
            let sym = &ctx.symbols[id];
            if sym.file() != Some(FileId::Obj(opd.file))
                || sym.input_section() != Some(opd)
                || !matches!(sym.ty(), STT_FUNC | STT_GNU_IFUNC)
            {
                continue;
            }
            let Some(rel) = rels_at.get(&sym.value) else {
                fatal!(
                    ctx,
                    "{}: cannot find a relocation in .opd for {sym} at offset {:#x}",
                    ctx.objs[i],
                    sym.value
                );
            };
            let target = &ctx.symbols[local_symbols[rel.r_sym as usize]];
            if target.ty() != STT_SECTION {
                fatal!(
                    ctx,
                    "{}: bad relocation in .opd referring to {target}",
                    ctx.objs[i]
                );
            }
            let origin = target.origin_state();
            descriptors.push((sym.value, idx as u32));
            let sym = &mut ctx.symbols[id];
            sym.set_origin_state(origin);
            sym.value = rel.r_addend as u64;
        }
        // Sort symbols so that get_opd_sym_at() can do binary search.
        descriptors.sort_by_key(|&(offset, _)| offset);

        // Rewrite relocations so that they directly refer to .opd.
        let refers_to_opd: Vec<bool> = local_symbols
            .iter()
            .map(|&id| ctx.symbols[id].input_section() == Some(opd))
            .collect();
        let mut unresolved = None;
        let mut rewrites = Vec::new();
        for isec in ctx.objs[i]
            .input_sections()
            .filter(|s| s.is_alive() && s.shndx != opd.shndx)
        {
            let mut rels: Vec<ElfRel> = isec.rels::<Ppc64V1>(&ctx.objs[i]).iter().collect();
            let mut redirected = false;
            for rel in &mut rels {
                if !refers_to_opd[rel.r_sym as usize] {
                    continue;
                }
                match descriptors
                    .binary_search_by_key(&(rel.r_addend as u64), |&(offset, _)| offset)
                {
                    Ok(n) => {
                        rel.r_sym = descriptors[n].1;
                        rel.r_addend = 0;
                        redirected = true;
                    }
                    Err(_) => unresolved = unresolved.or(Some((isec.name(&ctx.objs[i]), *rel))),
                }
            }
            if redirected {
                rewrites.push((isec.shndx, rels));
            }
        }
        for (shndx, rels) in rewrites {
            ctx.objs[i].rels_mut::<Ppc64V1>(shndx).set_all(&rels);
        }
        if let Some((name, rel)) = unresolved {
            fatal!(
                ctx,
                "{}:({name}): cannot find a symbol in .opd for {} at offset {:#x}",
                ctx.objs[i],
                rel.type_name::<Ppc64V1>(),
                rel.r_addend
            );
        }
    }
}

// When a function is exported, the dynamic symbol for the function should
// refers to the function's .opd entry. This function marks such symbols
// with NEEDS_PPC_OPD.
pub fn scan_symbols(ctx: &mut Context<Ppc64V1>) {
    let _t = ctx.timer("scan_symbols");
    let needs_descriptor = |sym: &Symbol| sym.add_flags(NEEDS_PPC_OPD);

    for id in ctx.symbols.global_ids() {
        let sym = &ctx.symbols[id];
        if matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_exported()
            && matches!(sym.ty(), STT_FUNC | STT_GNU_IFUNC)
        {
            needs_descriptor(sym);
        }
    }
    // Functions referenced by the ELF header also have to have .opd entries.
    for id in [ctx.syms.entry, ctx.syms.init, ctx.syms.fini] {
        let sym = &ctx.symbols[id];
        if !sym.is_imported() {
            needs_descriptor(sym);
        }
    }
}

impl Arch for Ppc64V1 {
    const NAME: &'static str = "ppc64v1";
    const FAMILY: Family = Family::Ppc64V1;
    const PAGE_SIZE: u64 = 65536;
    const E_MACHINE: u32 = EM_PPC64;
    const PLT_HDR_SIZE: u64 = 44;
    const PLT_SIZE: u64 = 8;
    const PLTGOT_SIZE: u64 = 0;
    const THUNK: Option<ThunkLayout> = Some(ThunkLayout {
        header_size: 0,
        entry_size: 28,
    });
    const TRAP: &'static [u8] = &[0x7f, 0xe0, 0x00, 0x08]; // trap

    const R_COPY: u32 = R_PPC64_COPY;
    const R_GLOB_DAT: u32 = R_PPC64_GLOB_DAT;
    const R_JUMP_SLOT: u32 = R_PPC64_JMP_SLOT;
    const R_ABS: u32 = R_PPC64_ADDR64;
    const R_RELATIVE: u32 = R_PPC64_RELATIVE;
    const R_IRELATIVE: Option<u32> = Some(R_PPC64_IRELATIVE);
    const R_DTPOFF: u32 = R_PPC64_DTPREL64;
    const R_TPOFF: u32 = R_PPC64_TPREL64;
    const R_DTPMOD: u32 = R_PPC64_DTPMOD64;
    const R_FUNCALL: &'static [u32] = &[R_PPC64_REL24, R_PPC64_REL24_NOTOC];

    fn rel_to_string(r_type: u32) -> String {
        ppc64_rel_to_string(r_type)
    }

    fn rewrite_input_sections(ctx: &mut Context<Self>) {
        rewrite_opd(ctx);
    }

    fn scan_symbols(ctx: &mut Context<Self>) {
        scan_symbols(ctx);
    }

    // .plt is used only for lazy symbol resolution on PPC64. All PLT
    // calls are made via range extension thunks even if they are within
    // reach. Thunks read addresses from .got.plt and jump there.
    // Therefore, once PLT symbols are resolved and final addresses are
    // written to .got.plt, thunks just skip .plt and directly jump to the
    // resolved addresses.
    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]) {
        const INSN: [u32; 11] = [
            0x7d88_02a6, // mflr    r12
            0x429f_0005, // bcl     20, 31, 4 // obtain PC
            0x7d68_02a6, // mflr    r11
            0x7d88_03a6, // mtlr    r12
            0x3d6b_0000, // addis   r11, r11, GOTPLT_OFFSET@ha
            0x396b_0000, // addi    r11, r11, GOTPLT_OFFSET@lo
            0xe98b_0000, // ld      r12,0(r11)
            0xe84b_0008, // ld      r2,8(r11)
            0x7d89_03a6, // mtctr   r12
            0xe96b_0010, // ld      r11,16(r11)
            0x4e80_0420, // bctr
        ];
        write_insns(buf, &INSN);
        let val = ctx
            .gotplt
            .hdr
            .shdr
            .sh_addr
            .wrapping_sub(ctx.plt.hdr.shdr.sh_addr)
            .wrapping_sub(8);
        or32(&mut buf[16..], higha(val));
        or32(&mut buf[20..], lo(val));
    }

    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol) {
        let idx = sym.plt_idx(&ctx.symbols).unwrap() as u64;
        let plt0 = ctx.plt.hdr.shdr.sh_addr;

        // The PPC64 ELFv1 ABI requires PLT entries to be vary in size depending
        // on their indices. Unlike other targets, .got.plt is filled not by us
        // but by the loader, so we don't have a control over where the initial
        // call to the PLT entry jumps to. So we need to strictly follow the PLT
        // section layout as the loader expect it to be.
        if idx < 0x8000 {
            write_insns(buf, &[0x3800_0000, 0x4b00_0000]); // li r0, PLT_INDEX; b plt0
            or32(buf, idx);
            or32(
                &mut buf[4..],
                plt0.wrapping_sub(sym.plt_addr(ctx)).wrapping_sub(4) & 0x00ff_ffff,
            );
        } else {
            // lis r0, PLT_INDEX@high; ori r0, r0, PLT_INDEX@lo; b plt0
            write_insns(buf, &[0x3c00_0000, 0x6000_0000, 0x4b00_0000]);
            or32(buf, high(idx));
            or32(&mut buf[4..], lo(idx));
            or32(
                &mut buf[8..],
                plt0.wrapping_sub(sym.plt_addr(ctx)).wrapping_sub(8) & 0x00ff_ffff,
            );
        }
    }

    // .plt.got is not necessary on PPC64 because range extension thunks
    // directly read GOT entries and jump there.
    fn write_pltgot_entry(_ctx: &Context<Self>, _buf: &mut [u8], _sym: &Symbol) {}

    fn apply_eh_reloc(
        ctx: &Context<Self>,
        isec: &InputSection,
        rel: &ElfRel,
        loc: &mut [u8],
        p: u64,
        val: u64,
    ) {
        match rel.r_type {
            R_NONE => {}
            R_PPC64_ADDR64 => w64(loc, val),
            R_PPC64_REL32 => {
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
            R_PPC64_REL64 => w64(loc, val.wrapping_sub(p)),
            _ => eh_frame::unsupported(ctx, rel),
        }
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        debug_assert!(isec.is_alloc());
        let file = &ctx.objs[isec.file.index()];

        // Scan relocations
        for rel in isec.rels::<Self>(file) {
            if rel.r_type == R_NONE || isec.record_undef_error(ctx, &rel) {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.is_ifunc() {
                sym.add_flags(NEEDS_GOT | NEEDS_PLT | NEEDS_PPC_OPD);
            }

            // Any relocation except R_PPC64_REL24 is considered as an
            // address-taking relocation.
            if rel.r_type != R_PPC64_REL24 && sym.ty() == STT_FUNC {
                sym.add_flags(NEEDS_PPC_OPD);
            }

            match rel.r_type {
                R_PPC64_GOT_TPREL16_HA => sym.add_flags(NEEDS_GOTTP),
                R_PPC64_REL24 => {
                    if sym.is_imported() {
                        sym.add_flags(NEEDS_PLT);
                    }
                }
                R_PPC64_GOT16 | R_PPC64_PLT16_HA => sym.add_flags(NEEDS_GOT),
                R_PPC64_GOT_TLSGD16_HA => sym.add_flags(NEEDS_TLSGD),
                R_PPC64_GOT_TLSLD16_HA => ctx.needs_tlsld.store(true, Ordering::Relaxed),
                R_PPC64_TPREL16_HA | R_PPC64_TPREL16_LO | R_PPC64_TPREL16_LO_DS => {
                    check_tlsle(ctx, isec, sym, &rel)
                }
                R_PPC64_ADDR64
                | R_PPC64_TOC
                | R_PPC64_REL32
                | R_PPC64_REL64
                | R_PPC64_TOC16_HA
                | R_PPC64_TOC16_LO
                | R_PPC64_TOC16_LO_DS
                | R_PPC64_TOC16_DS
                | R_PPC64_REL16_HA
                | R_PPC64_REL16_LO
                | R_PPC64_PLT16_HI
                | R_PPC64_PLT16_LO
                | R_PPC64_PLT16_LO_DS
                | R_PPC64_PLTSEQ
                | R_PPC64_PLTCALL
                | R_PPC64_GOT_TPREL16_LO_DS
                | R_PPC64_GOT_TLSGD16_LO
                | R_PPC64_GOT_TLSLD16_LO
                | R_PPC64_TLS
                | R_PPC64_TLSGD
                | R_PPC64_TLSLD
                | R_PPC64_DTPREL16_HA
                | R_PPC64_DTPREL16_LO
                | R_PPC64_DTPREL16_LO_DS => {}
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
        let toc = toc(ctx);
        let got = ctx.got.hdr.shdr.sh_addr;

        for (i, rel) in isec.rels::<Self>(file).iter().enumerate() {
            if rel.r_type == R_NONE {
                continue;
            }
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
            if sym.ty() == STT_TLS && sym.is_remaining_undef_weak() {
                continue;
            }

            let s = sym.addr(ctx);
            let a = rel.r_addend as u64;
            let p = isec.addr(ctx) + rel.r_offset;
            let g = || sym.got_addr(ctx).wrapping_sub(got);
            let sa = s.wrapping_add(a);
            let pcrel = sa.wrapping_sub(p);
            let loc = &mut buf[rel.r_offset as usize..];

            match rel.r_type {
                R_PPC64_TOC => {}
                R_PPC64_TOC16_HA => w16(loc, ha(sa.wrapping_sub(toc))),
                R_PPC64_TOC16_LO => w16(loc, lo(sa.wrapping_sub(toc))),
                R_PPC64_TOC16_DS => {
                    isec.check_range(ctx, i, sa.wrapping_sub(toc) as i64, -(1 << 15), 1 << 15);
                    or16(loc, sa.wrapping_sub(toc) & 0xfffc);
                }
                R_PPC64_TOC16_LO_DS => or16(loc, sa.wrapping_sub(toc) & 0xfffc),
                R_PPC64_REL24 => {
                    let code = sym.addr_with(ctx, AddrFlags::NO_OPD);
                    let mut val = code.wrapping_add(a).wrapping_sub(p) as i64;
                    if sym.has_plt(&ctx.symbols) || !is_int(val, 26) {
                        val = sym.thunk_addr(ctx, p).wrapping_add(a).wrapping_sub(p) as i64;
                    }
                    isec.check_range(ctx, i, val, -(1 << 25), 1 << 25);
                    or32(loc, bits(val as u64, 25, 2) << 2);

                    // If a callee is an external function, PLT saves %r2 to the
                    // caller's r2 save slot. We need to restore it after function
                    // return. To do so, there's usually a NOP as a placeholder
                    // after a BL. 0x6000'0000 is a NOP.
                    if sym.has_plt(&ctx.symbols) && loc.len() >= 8 && r32(&loc[4..]) == 0x6000_0000
                    {
                        w32(&mut loc[4..], 0xe841_0028); // ld r2, 40(r1)
                    }
                }
                R_PPC64_REL32 => w32(loc, pcrel),
                R_PPC64_REL64 => w64(loc, pcrel),
                R_PPC64_REL16_HA => w16(loc, ha(pcrel)),
                R_PPC64_REL16_LO => w16(loc, lo(pcrel)),
                R_PPC64_GOT16 => w16(loc, g().wrapping_sub(toc)),
                R_PPC64_PLT16_HA => w16(loc, ha(sym.got_addr(ctx).wrapping_sub(toc))),
                R_PPC64_PLT16_HI => w16(loc, hi(sym.got_addr(ctx).wrapping_sub(toc))),
                R_PPC64_PLT16_LO => w16(loc, lo(sym.got_addr(ctx).wrapping_sub(toc))),
                R_PPC64_PLT16_LO_DS => or16(loc, sym.got_addr(ctx).wrapping_sub(toc) & 0xfffc),
                R_PPC64_GOT_TPREL16_HA => w16(loc, ha(sym.gottp_addr(ctx).wrapping_sub(toc))),
                R_PPC64_GOT_TPREL16_LO_DS => {
                    or16(loc, sym.gottp_addr(ctx).wrapping_sub(toc) & 0xfffc)
                }
                R_PPC64_GOT_TLSGD16_HA => w16(loc, ha(sym.tlsgd_addr(ctx).wrapping_sub(toc))),
                R_PPC64_GOT_TLSGD16_LO => w16(loc, lo(sym.tlsgd_addr(ctx).wrapping_sub(toc))),
                R_PPC64_GOT_TLSLD16_HA => {
                    w16(loc, ha(ctx.got.tlsld_addr::<Self>().wrapping_sub(toc)))
                }
                R_PPC64_GOT_TLSLD16_LO => {
                    w16(loc, lo(ctx.got.tlsld_addr::<Self>().wrapping_sub(toc)))
                }
                R_PPC64_DTPREL16_HA => w16(loc, ha(sa.wrapping_sub(ctx.dtp_addr))),
                R_PPC64_DTPREL16_LO => w16(loc, lo(sa.wrapping_sub(ctx.dtp_addr))),
                R_PPC64_DTPREL16_LO_DS => or16(loc, sa.wrapping_sub(ctx.dtp_addr) & 0xfffc),
                R_PPC64_TPREL16_HA => w16(loc, ha(sa.wrapping_sub(ctx.tp_addr))),
                R_PPC64_TPREL16_LO => w16(loc, lo(sa.wrapping_sub(ctx.tp_addr))),
                R_PPC64_TPREL16_LO_DS => or16(loc, sa.wrapping_sub(ctx.tp_addr) & 0xfffc),
                R_PPC64_ADDR64 | R_PPC64_PLTSEQ | R_PPC64_PLTCALL | R_PPC64_TLS | R_PPC64_TLSGD
                | R_PPC64_TLSLD => {}
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

            match rel.r_type {
                R_PPC64_ADDR64 => w64(
                    loc,
                    isec.tombstone(ctx, sym, frag.map(|(f, _)| f)).unwrap_or(sa),
                ),
                R_PPC64_ADDR32 => {
                    isec.check_range(ctx, i, sa as i64, 0, 1 << 32);
                    w32(loc, sa);
                }
                R_PPC64_DTPREL64 => w64(loc, sa.wrapping_sub(ctx.dtp_addr)),
                _ => fatal!(
                    ctx,
                    "{}: invalid relocation for non-allocated sections: {}",
                    isec.display(file),
                    rel.type_name::<Self>()
                ),
            }
        }
    }

    fn always_needs_thunk(ctx: &Context<Self>, sym: &Symbol, _rel: &ElfRel) -> bool {
        sym.has_plt(&ctx.symbols)
    }

    fn write_thunk(ctx: &Context<Self>, thunk: &Thunk, _addr: u64, buf: &mut [u8]) {
        // If the destination is .plt.got, we save the current r2, read an
        // address of a function descriptor from .got, restore %r2 and jump
        // to the function.
        const PLTGOT_THUNK: [u32; 7] = [
            // Store the caller's %r2
            0xf841_0028, // std   %r2, 40(%r1)
            // Load an address of a function descriptor
            0x3d82_0000, // addis %r12, %r2,  foo@got@toc@ha
            0xe98c_0000, // ld    %r12, foo@got@toc@lo(%r12)
            // Restore the callee's %r2
            0xe84c_0008, // ld    %r2,  8(%r12)
            // Jump to the function
            0xe98c_0000, // ld    %r12, 0(%r12)
            0x7d89_03a6, // mtctr %r12
            0x4e80_0420, // bctr
        ];

        // If the destination is .plt, read a function descriptor from .got.plt.
        const PLT_THUNK: [u32; 7] = [
            // Store the caller's %r2
            0xf841_0028, // std   %r2, 40(%r1)
            // Materialize an address of a function descriptor
            0x3d82_0000, // addis %r12, %r2,  foo@gotplt@toc@ha
            0x398c_0000, // addi  %r12, %r12, foo@gotplt@toc@lo
            // Restore the callee's %r2
            0xe84c_0008, // ld    %r2,  8(%r12)
            // Jump to the function
            0xe98c_0000, // ld    %r12, 0(%r12)
            0x7d89_03a6, // mtctr %r12
            0x4e80_0420, // bctr
        ];

        // If the destination is a non-imported function, we directly jump
        // to the function entry address.
        const LOCAL_THUNK: [u32; 7] = [
            0x3d82_0000, // addis r12, r2,  foo@toc@ha
            0x398c_0000, // addi  r12, r12, foo@toc@lo
            0x7d89_03a6, // mtctr r12
            0x4e80_0420, // bctr
            0x6000_0000, // nop
            0x6000_0000, // nop
            0x6000_0000, // nop
        ];

        let toc = toc(ctx);

        for (i, &id) in thunk.symbols.iter().enumerate() {
            let sym = &ctx.symbols[id];
            let entry = &mut buf[thunk.offsets[i] as usize..][..28];

            let (insns, target, at) = if sym.has_got(&ctx.symbols) {
                (&PLTGOT_THUNK, sym.got_addr(ctx), 4)
            } else if sym.has_plt(&ctx.symbols) {
                (&PLT_THUNK, sym.gotplt_addr(ctx), 4)
            } else {
                (&LOCAL_THUNK, sym.addr_with(ctx, AddrFlags::NO_OPD), 0)
            };
            write_insns(entry, insns);
            let val = target.wrapping_sub(toc);
            or32(&mut entry[at..], higha(val));
            or32(&mut entry[at + 4..], lo(val));
        }
    }
}
