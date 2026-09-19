//! The ARM64 (AArch64) target.

use crate::error;
use crate::fatal;
use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::files::FileName;
use crate::macho::format::*;
use crate::macho::input_sections::{Reloc, RelocTarget};
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

    write32(loc, insn | ((bits(val, 11, scale as u32) as u32) << 10));
}

impl Arch for Arm64 {
    const NAME: &'static str = "arm64";
    const CPUTYPE: u32 = CPU_TYPE_ARM64;
    const CPUSUBTYPE: u32 = CPU_SUBTYPE_ARM64_ALL;
    const PAGE_SIZE: u64 = 16384;
    const STUB_SIZE: u64 = 12;
    const STUB_HELPER_HEADER_SIZE: u64 = 24;
    const STUB_HELPER_ENTRY_SIZE: u64 = 12;
    const UNWIND_MODE_DWARF: u32 = UNWIND_ARM64_MODE_DWARF;
    const OBJC_STUB_SIZE: u64 = 32;
    const BRANCH_RANGE: u64 = 1 << 28;
    const THUNK_SIZE: u64 = 12;
    const RELOC_UNSIGNED: u8 = ARM64_RELOC_UNSIGNED;
    const RELOC_SUBTRACTOR: u8 = ARM64_RELOC_SUBTRACTOR;
    const RELOC_GOTPC: u8 = ARM64_RELOC_POINTER_TO_GOT;
    const RELOC_ADDEND: u8 = ARM64_RELOC_ADDEND;

    fn relocatable_needs_addend(r_type: u8) -> bool {
        // Instruction-patching relocations can't embed an addend; data
        // relocations keep it in the relocated bytes.
        !matches!(r_type, ARM64_RELOC_UNSIGNED | ARM64_RELOC_SUBTRACTOR)
    }

    // Both halves of an adrp+ldr GOT load relax together: the adrp
    // keeps its shape and the ldr becomes an add, so the pair-wide
    // answer is always yes (the ldr's shape is verified when the
    // rewrite happens - compilers emit nothing else for these
    // relocations).
    fn got_load_form(r_type: u8) -> Option<u8> {
        match r_type {
            ARM64_RELOC_PAGE21 => Some(ARM64_RELOC_GOT_LOAD_PAGE21),
            ARM64_RELOC_PAGEOFF12 => Some(ARM64_RELOC_GOT_LOAD_PAGEOFF12),
            _ => None,
        }
    }

    fn can_relax_got_load(_data: &[u8], _offset: u32, _r_type: u8) -> bool {
        true
    }

    // LC_LINKER_OPTIMIZATION_HINT: compilers can't know how far a
    // symbol will land, so they emit the conservative two-instruction
    // materializations and leave hints naming the instructions, for
    // the linker to shorten once addresses are final. Everything here
    // is a peephole guarded by instruction-shape checks, so hints
    // invalidated by other rewrites (or by unexpected code) are
    // silently skipped - hints are advisory by design.
    fn apply_optimization_hints(ctx: &Context<Self>, buf: &mut [u8]) {
        if ctx.args.ignore_optimization_hints {
            return;
        }

        const NOP: u32 = 0xd503_201f;
        let is_adrp = |i: u32| i & 0x9f00_0000 == 0x9000_0000;
        let is_add = |i: u32| i & 0xffc0_0000 == 0x9100_0000;
        // LDR (immediate, unsigned offset), 32- or 64-bit integer.
        let ldr_size = |i: u32| match i & 0xffc0_0000 {
            0xb940_0000 => Some(4u64),
            0xf940_0000 => Some(8u64),
            _ => None,
        };
        let adrp_target = |i: u32, pc: u64| -> u64 {
            let imm = ((i >> 29) & 3) as u64 | (((i >> 5) & 0x7_ffff) as u64) << 2;
            let imm = (imm << 43) as i64 >> 31; // sign-extend 21 bits, <<12
            (pc & !0xfff).wrapping_add_signed(imm)
        };
        let in_adr_range = |target: u64, pc: u64| -> bool {
            (target.wrapping_sub(pc) as i64).unsigned_abs() < (1 << 20)
        };
        let make_adr = |target: u64, pc: u64, rd: u32| -> u32 {
            let d = target.wrapping_sub(pc);
            0x1000_0000 | ((d as u32 & 3) << 29) | (((d >> 2) as u32 & 0x7_ffff) << 5) | rd
        };
        let make_ldr_lit = |target: u64, pc: u64, rt: u32, size: u64| -> u32 {
            let opc = if size == 8 { 0x5800_0000 } else { 0x1800_0000 };
            opc | ((target.wrapping_sub(pc) as u32 >> 2) & 0x7_ffff) << 5 | rt
        };

        use rayon::prelude::*;
        struct BufPtr(*mut u8);
        unsafe impl Sync for BufPtr {}
        let bufp = BufPtr(buf.as_mut_ptr());
        let bufp = &bufp;
        let buf_len = buf.len();
        // Objects rewrite their own instructions only, so their hint
        // lists process in parallel.
        ctx.objs.par_iter().for_each(|obj| {
            // SAFETY: every hint writes within its object's own
            // subsections; different objects' subsections are
            // disjoint ranges of the output.
            let buf = unsafe { std::slice::from_raw_parts_mut(bufp.0, buf_len) };
            if !obj.is_alive {
                return;
            }
            'hint: for (kind, addrs) in &obj.loh {
                // Map input addresses to (file offset, address).
                let mut locs: Vec<(usize, u64)> = Vec::with_capacity(addrs.len());
                for &addr in addrs {
                    let Some((isec, off)) =
                        crate::macho::input_files::find_subsec(&ctx.isecs, &obj.subsecs, addr)
                    else {
                        continue 'hint;
                    };
                    let isec = &ctx.isecs[ctx.resolve_isec(isec)];
                    if !isec.is_alive() || isec.offset == u32::MAX {
                        continue 'hint;
                    }
                    let hdr = ctx.chunk_header(isec.output_section().unwrap());
                    locs.push((
                        (hdr.fileoff + isec.offset as u64 + off) as usize,
                        hdr.addr + isec.offset as u64 + off,
                    ));
                }
                let insn = |buf: &[u8], i: usize| read32(&buf[locs[i].0..]);
                let put = |buf: &mut [u8], i: usize, v: u32| {
                    write32(&mut buf[locs[i].0..locs[i].0 + 4], v)
                };

                match (kind, locs.len()) {
                    // Two adrp of the same page into the same register:
                    // the second is redundant.
                    (1, 2) => {
                        let (a, b) = (insn(buf, 0), insn(buf, 1));
                        if is_adrp(a)
                            && is_adrp(b)
                            && a & 0x1f == b & 0x1f
                            && adrp_target(a, locs[0].1) == adrp_target(b, locs[1].1)
                        {
                            put(buf, 1, NOP);
                        }
                    }
                    // adrp+ldr loading a nearby location: a single
                    // pc-relative literal load. Kind 8 is the same
                    // pair when the ldr reads a GOT slot; if the GOT
                    // relaxation already turned that ldr into an add,
                    // fall through to the adr rewrite below.
                    (2 | 8, 2) => {
                        let (a, l) = (insn(buf, 0), insn(buf, 1));
                        if is_adrp(a) && (l >> 5) & 0x1f == a & 0x1f {
                            if let Some(size) = ldr_size(l) {
                                let target =
                                    adrp_target(a, locs[0].1) + (((l >> 10) & 0xfff) as u64) * size;
                                if size == 8 && target % 4 == 0 && in_adr_range(target, locs[1].1) {
                                    put(buf, 0, NOP);
                                    put(buf, 1, make_ldr_lit(target, locs[1].1, l & 0x1f, size));
                                }
                            } else if *kind == 8 && is_add(l) {
                                let target = adrp_target(a, locs[0].1) + ((l >> 10) & 0xfff) as u64;
                                if in_adr_range(target, locs[1].1) {
                                    put(buf, 0, NOP);
                                    put(buf, 1, make_adr(target, locs[1].1, l & 0x1f));
                                }
                            }
                        }
                    }
                    // adrp+add materializing a nearby address: one adr.
                    (7, 2) => {
                        let (a, d) = (insn(buf, 0), insn(buf, 1));
                        if is_adrp(a) && is_add(d) && (d >> 5) & 0x1f == a & 0x1f {
                            let target = adrp_target(a, locs[0].1) + ((d >> 10) & 0xfff) as u64;
                            if in_adr_range(target, locs[1].1) {
                                put(buf, 0, NOP);
                                put(buf, 1, make_adr(target, locs[1].1, d & 0x1f));
                            }
                        }
                    }
                    // adrp+add+ldr: load through a computed address.
                    // Nearby: fold everything into one literal load;
                    // else shorten the address computation to adr.
                    (3, 3) => {
                        let (a, d, l) = (insn(buf, 0), insn(buf, 1), insn(buf, 2));
                        if !is_adrp(a) || !is_add(d) || (d >> 5) & 0x1f != a & 0x1f {
                            continue;
                        }
                        let base = adrp_target(a, locs[0].1) + ((d >> 10) & 0xfff) as u64;
                        if let Some(size) = ldr_size(l) {
                            if (l >> 5) & 0x1f == d & 0x1f {
                                let target = base + (((l >> 10) & 0xfff) as u64) * size;
                                if size == 8 && target % 4 == 0 && in_adr_range(target, locs[2].1) {
                                    put(buf, 0, NOP);
                                    put(buf, 1, NOP);
                                    put(buf, 2, make_ldr_lit(target, locs[2].1, l & 0x1f, size));
                                    continue;
                                }
                            }
                        }
                        if in_adr_range(base, locs[1].1) {
                            put(buf, 0, NOP);
                            put(buf, 1, make_adr(base, locs[1].1, d & 0x1f));
                        }
                    }
                    // Other kinds (GOT-load triples, stores) are left
                    // as compiled; hints are advisory.
                    _ => {}
                }
            }
        });
    }

    fn classify_reloc(r_type: u8) -> crate::macho::arch::RelocClass {
        use crate::macho::arch::RelocClass;
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
        for i in 0..ctx.stubs.symbols.len() {
            let off = 24 + i * 12;
            let ent_addr = addr + off as u64;
            write32(&mut buf[off..], 0x1800_0050);
            let rel = addr.wrapping_sub(ent_addr + 4) as i64 >> 2;
            write32(&mut buf[off + 4..], 0x1400_0000 | (rel as u32 & 0x03ff_ffff));
            write32(&mut buf[off + 8..], ctx.lazy_bind_info.offsets[i]);
        }
    }

    fn write_objc_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let msgsend_got = ctx.sym_got_addr(ctx.objc_stubs.msgsend_sym.unwrap());

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
        syms: &[crate::macho::symbol::SymbolId],
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
        file_name: &str,
        sections: &[MachSection],
        hdr: &MachSection,
        file_data: &[u8],
        rels: &[MachRel],
    ) -> Vec<Reloc> {
        let mut vec = Vec::with_capacity(rels.len());
        let mut i = 0;

        while i < rels.len() {
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
                    addend = sign_extend(rels[i].r_symbolnum() as u64, 23);
                    i += 1;
                }
                _ => {}
            }

            let r = &rels[i];
            let is_subtracted = i > 0 && rels[i - 1].r_type() == ARM64_RELOC_SUBTRACTOR;

            // A relocation refers to either a symbol or a section.
            let (target, addend) = if r.is_extern() {
                (RelocTarget::Sym(r.r_symbolnum() as u32), addend)
            } else {
                let addr = if r.is_pcrel() {
                    (hdr.addr + r.r_address as u64).wrapping_add_signed(addend)
                } else {
                    addend as u64
                };
                // The address may be one past a section's end: a
                // DWARF range end or high_pc, or a label after the
                // last instruction.
                let Some(idx) = sections
                    .iter()
                    .position(|sec| sec.addr <= addr && addr < sec.addr + sec.size)
                    .or_else(|| sections.iter().position(|sec| addr == sec.addr + sec.size))
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
                                ctx.objs[obj].mf.name_str()
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
                        // here (mold-rust's thunk_addrs lookup).
                        let thunk = ctx.reloc_target_sym(obj, r).and_then(|sym| {
                            crate::macho::thunks::reachable_thunk_addr::<Self>(ctx, sym, p)
                        });
                        match thunk {
                            Some(t) => val = t.wrapping_sub(p) as i64,
                            None => error!("branch target out of range: {val:x}"),
                        }
                    }
                    write32(loc, read32(loc) | bits(val as u64, 27, 2) as u32);
                }
                // A local thread-local's TLV load relaxes like a GOT
                // load: the adrp retargets to the __thread_vars
                // descriptor's page and the ldr becomes an add.
                ARM64_RELOC_TLVP_LOAD_PAGE21 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    let target =
                        if ctx.symbols[id].is_imported() { ctx.sym_tlv_ptr_addr(id) } else { s };
                    let val = read32(loc) | page_offset(target.wrapping_add_signed(a), p);
                    write32(loc, val);
                }
                ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => {
                    let id = ctx.reloc_target_sym(obj, r).unwrap();
                    if ctx.symbols[id].is_imported() {
                        let t = ctx.sym_tlv_ptr_addr(id);
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
                        if insn & 0xffc0_0000 != 0xf940_0000 {
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
