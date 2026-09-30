//! The x86-64 target.

use std::path::Path;

use crate::context::Context;
use crate::input_sections::{Reloc, RelocTarget};
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::{
    BadReloc, RelocError, SplitRef, Target, check_reloc_index, check_reloc_place, has_reloc_form,
    reloc_form,
};
use crate::{error, fatal};

#[derive(Clone, Copy, Default)]
pub struct X86_64;

fn write32(loc: &mut [u8], val: u32) {
    loc[..4].copy_from_slice(&val.to_le_bytes());
}

fn write64(loc: &mut [u8], val: u64) {
    loc[..8].copy_from_slice(&val.to_le_bytes());
}

/// The SIGNED_K relocation types describe a pcrel field followed by K
/// more instruction bytes; the extra distance is folded into the addend
/// when reading and taken back out when writing.
fn reloc_bias(r_type: u8) -> i64 {
    match r_type {
        X86_64_RELOC_SIGNED_1 => 1,
        X86_64_RELOC_SIGNED_2 => 2,
        X86_64_RELOC_SIGNED_4 => 4,
        _ => 0,
    }
}

/// Whether ld-prime takes a record's pcrel, length and extern fields
/// for its type.
#[inline]
fn is_supported(r: &MachRel) -> bool {
    let forms = match r.r_type() {
        X86_64_RELOC_UNSIGNED => {
            reloc_form(false, 2, true)
                | reloc_form(false, 3, true)
                | reloc_form(false, 2, false)
                | reloc_form(false, 3, false)
        }
        X86_64_RELOC_SUBTRACTOR => reloc_form(false, 2, true) | reloc_form(false, 3, true),
        X86_64_RELOC_SIGNED
        | X86_64_RELOC_SIGNED_1
        | X86_64_RELOC_SIGNED_2
        | X86_64_RELOC_SIGNED_4 => reloc_form(true, 2, true) | reloc_form(true, 2, false),
        // A one-byte branch (jmp rel8) must name a symbol.
        X86_64_RELOC_BRANCH => {
            reloc_form(true, 2, true) | reloc_form(true, 2, false) | reloc_form(true, 0, true)
        }
        X86_64_RELOC_GOT_LOAD | X86_64_RELOC_GOT | X86_64_RELOC_TLV => reloc_form(true, 2, true),
        _ => 0,
    };
    has_reloc_form(r, forms)
}

/// Checks record `i` as ld-prime does when it reads it: its type must
/// take its pcrel, length and extern fields, and a SUBTRACTOR must pair
/// with an UNSIGNED of its size at its address. ld-prime checks no
/// instructions here.
#[inline(always)]
fn check_reloc(rels: &[MachRel], i: usize) -> Result<(), BadReloc> {
    let r = &rels[i];
    if !is_supported(r) {
        return Err(BadReloc::new(r, RelocError::Unsupported));
    }
    if r.r_type() == X86_64_RELOC_SUBTRACTOR {
        let Some(u) = rels.get(i + 1).filter(|u| {
            u.r_type() == X86_64_RELOC_UNSIGNED && is_supported(u) && u.r_length() == r.r_length()
        }) else {
            return Err(BadReloc::new(r, RelocError::Unsupported));
        };
        if u.r_address != r.r_address {
            return Err(BadReloc::new(
                u,
                RelocError::Invalid(
                    "X86_64_RELOC_SUBTRACTOR preceeding X86_64_RELOC_UNSIGNED must have same r_address",
                ),
            ));
        }
    }
    Ok(())
}

/// The displacement a 32-bit pc-relative fixup, relocation `r` of
/// subsection `isec` at `p`, holds to reach `t`: from the end of the
/// instruction, past the field and the immediate after it a SIGNED_1/2/4
/// counts. One that doesn't fit is a fixup error of ld-prime's `kind`,
/// naming the target `name`.
fn rip32_displacement(
    ctx: &Context<X86_64>,
    isec: usize,
    r: &Reloc,
    kind: &str,
    p: u64,
    t: u64,
    name: &str,
) -> u32 {
    let disp = t.wrapping_sub(p + 4).wrapping_sub(reloc_bias(r.r_type) as u64) as i64;
    if i32::try_from(disp).is_err() {
        let msg = format_args!(
            "32-bit RIP-relative reference out of range (displacement={disp}, max is +/-2GB), \
             from 0x{p:08X} to 0x{t:08X} ('{name}')"
        );
        ctx.fixup_error(isec, r.offset, kind, msg);
    }
    disp as u32
}

/// Writes a one-byte branch (jmp rel8) at `loc`, relocation `r` of
/// subsection `isec` whose address is `p`, to `t`, the address of
/// `sym`. It reaches only a definition near it in the image: ld-prime
/// gives it no stub.
fn write_branch8(
    ctx: &Context<X86_64>,
    isec: usize,
    r: &Reloc,
    sym: SymbolId,
    t: u64,
    p: u64,
    loc: &mut [u8],
) {
    let sym = &ctx.symbols[sym];
    let val = t.wrapping_sub(p + 1) as i64;
    if sym.is_imported() {
        let msg = format_args!("target '{sym}' does not have address");
        ctx.fixup_error(isec, r.offset, "x86_64_branch8", msg);
    } else if !(-128..128).contains(&val) {
        let msg = format_args!(
            "8-bit branch out of range (displacement={val}, max is +/-127), \
             from 0x{p:X} to 0x{t:X} ('{sym}')"
        );
        ctx.fixup_error(isec, r.offset, "x86_64_branch8", msg);
    }
    loc[0] = val as u8;
}

impl Target for X86_64 {
    const NAME: &'static str = "x86_64";
    const CPUTYPE: u32 = CPU_TYPE_X86_64;
    const CPUSUBTYPE: u32 = CPU_SUBTYPE_X86_64_ALL;
    const PAGE_SIZE: u64 = 4096;
    const STUB_SIZE: u64 = 6;
    const STUB_HELPER_HEADER_SIZE: u64 = 16;
    // ld64 keeps the entries 4-byte aligned: 10 bytes of push and jmp,
    // then 2 zero bytes.
    const STUB_HELPER_ENTRY_SIZE: u64 = 12;
    const STUB_HELPER_ENTRY_PADDING: u64 = 2;
    const UNWIND_MODE_DWARF: u32 = UNWIND_X86_64_MODE_DWARF;
    const OBJC_STUB_SIZE: u64 = 16;
    // A 32-bit pcrel branch covers 4 GiB; x86-64 outputs never need
    // thunks.
    const BRANCH_RANGE: u64 = 1 << 32;
    const THUNK_SIZE: u64 = 0;
    const RELOC_UNSIGNED: u8 = X86_64_RELOC_UNSIGNED;
    const RELOC_SUBTRACTOR: u8 = X86_64_RELOC_SUBTRACTOR;
    const RELOC_GOTPC: u8 = X86_64_RELOC_GOT;
    const RELOCATABLE_GOTPC_CELL: Option<u32> = None;
    // x86-64 embeds every addend in the relocated field.
    const RELOC_ADDEND: u8 = 0xff;
    const SPLIT_PCREL_KINDS: &'static [u8] = &[DYLD_CACHE_ADJ_V2_DELTA_32];
    const STUB_REF_OFF: u64 = 2;
    const STUB_HELPER_REF_OFFS: [u64; 2] = [3, 11];
    const OBJC_STUB_REF_OFFS: [u64; 2] = [3, 9];
    // x86_THREAD_STATE64: rax..r15, then rip.
    const THREAD_STATE_FLAVOR: u32 = 4;
    const THREAD_STATE_COUNT: u32 = 42;
    const THREAD_STATE_SP_OFFSET: usize = 7 * 8;
    const THREAD_STATE_PC_OFFSET: usize = 16 * 8;

    fn relocatable_needs_addend(_r_type: u8) -> bool {
        false
    }

    fn reloc_bias(r_type: u8) -> i64 {
        reloc_bias(r_type)
    }

    // GOT_LOAD marks "movq sym@GOTPCREL(%rip), %reg" (opcode 0x8b,
    // REX prefix before it); with a local target the load of the
    // slot's content is the same as computing the address, so the
    // opcode becomes lea (0x8d). A leaq of a class-reference slot
    // takes the slot's address, and keeps the slot.
    fn got_load_form(r_type: u8) -> Option<u8> {
        match r_type {
            X86_64_RELOC_SIGNED => Some(X86_64_RELOC_GOT_LOAD),
            _ => None,
        }
    }

    fn can_relax_got_load(data: &[u8], offset: u32, _r_type: u8) -> bool {
        offset >= 2 && data.get(offset as usize - 2) == Some(&0x8b)
    }

    fn classify_reloc(r_type: u8) -> crate::target::RelocClass {
        use crate::target::RelocClass;
        match r_type {
            X86_64_RELOC_BRANCH => RelocClass::Branch,
            X86_64_RELOC_GOT_LOAD => RelocClass::GotLoad,
            X86_64_RELOC_GOT => RelocClass::Got,
            X86_64_RELOC_TLV => RelocClass::Tlv,
            _ => RelocClass::Plain,
        }
    }

    fn split_ref(r_type: u8) -> SplitRef {
        match r_type {
            X86_64_RELOC_UNSIGNED => SplitRef::Pointer,
            X86_64_RELOC_SUBTRACTOR => SplitRef::Subtractor,
            _ => SplitRef::PcRel32,
        }
    }

    fn write_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let mut reported = false;
        for (i, &sym) in ctx.stubs.symbols.iter().enumerate() {
            let ent = &mut buf[i * 6..];
            let ent_addr = addr + i as u64 * 6;
            let ptr_addr = ctx.stub_ptr_addr(i, sym);
            let disp = ptr_addr.wrapping_sub(ent_addr + 6) as i64;
            if !reported && i32::try_from(disp).is_err() {
                let p = ent_addr + 2;
                let msg = format_args!(
                    "32-bit RIP-relative reference out of range (displacement={disp}, max is \
                     +/-2GB), from 0x{p:08X} to 0x{ptr_addr:08X} ('')"
                );
                ctx.stub_fixup_error(i, 2, "x86_64_rip", msg);
                reported = true;
            }

            // jmp *ptr(%rip)
            ent[0] = 0xff;
            ent[1] = 0x25;
            write32(&mut ent[2..], ptr_addr.wrapping_sub(ent_addr + 6) as u32);
        }
    }

    fn write_stub_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        // The header, as ld64 emits it:
        //   lea  __dyld_private(%rip), %r11
        //   push %r11
        //   jmp  *dyld_stub_binder@GOTPCREL(%rip)
        //   nop
        let private = ctx.isec_addr(ctx.stub_helper.dyld_private_isec as usize);
        let binder = ctx.sym_got_addr(ctx.stub_helper.dyld_stub_binder.unwrap());
        buf[0..3].copy_from_slice(&[0x4c, 0x8d, 0x1d]);
        write32(&mut buf[3..], private.wrapping_sub(addr + 7) as u32);
        buf[7..9].copy_from_slice(&[0x41, 0x53]);
        buf[9..11].copy_from_slice(&[0xff, 0x25]);
        write32(&mut buf[11..], binder.wrapping_sub(addr + 15) as u32);
        buf[15] = 0x90;
        // Each entry: push $offset; jmp header; the zero padding.
        let lazy_offsets = &ctx.lazy_bind_info.offsets[..ctx.stubs.lazy.len()];
        for (i, &lazy_off) in lazy_offsets.iter().enumerate() {
            let off =
                (Self::STUB_HELPER_HEADER_SIZE + i as u64 * Self::STUB_HELPER_ENTRY_SIZE) as usize;
            let ent_addr = addr + off as u64;
            buf[off] = 0x68;
            write32(&mut buf[off + 1..], lazy_off);
            buf[off + 5] = 0xe9;
            write32(&mut buf[off + 6..], addr.wrapping_sub(ent_addr + 10) as u32);
            if i + 1 < lazy_offsets.len() {
                buf[off + 10..off + 12].fill(0);
            }
        }
    }

    fn write_objc_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let msgsend_got = ctx.objc_msgsend_got_addr();

        for i in 0..ctx.objc_stubs.symbols.len() {
            let ent = &mut buf[i * 16..];
            let ent_addr = addr + i as u64 * 16;
            let sel_addr = ctx.objc_selref_addr(i);

            // mov sel(%rip), %rsi; jmp *_objc_msgSend@GOT(%rip); int3 x3
            ent[..16].copy_from_slice(&[
                0x48, 0x8b, 0x35, 0, 0, 0, 0, 0xff, 0x25, 0, 0, 0, 0, 0xcc, 0xcc, 0xcc,
            ]);
            write32(&mut ent[3..], sel_addr.wrapping_sub(ent_addr + 7) as u32);
            write32(&mut ent[9..], msgsend_got.wrapping_sub(ent_addr + 13) as u32);
        }
    }

    fn write_thunk(
        _ctx: &Context<Self>,
        _addr: u64,
        _syms: &[crate::symbol::SymbolId],
        _buf: &mut [u8],
    ) {
        unreachable!("x86-64 branches never need thunks");
    }

    fn read_relocs(
        file_name: &Path,
        sections: &[MachSection],
        hdr: &MachSection,
        contents: &[u8],
        rels: &[MachRel],
        nsyms: usize,
    ) -> Result<Vec<Reloc>, BadReloc> {
        let mut vec = Vec::with_capacity(rels.len());

        for (i, r) in rels.iter().enumerate() {
            // Diagnostics spell the path lossily.
            let file_name = file_name.display();
            check_reloc_place(r, contents)?;
            check_reloc(rels, i)?;
            check_reloc_index(r, sections.len(), nsyms)?;

            // On x86-64 every relocation's addend is embedded in the
            // relocated field.
            let loc = &contents[r.r_address as usize..];
            let embedded = match r.r_length() {
                0 => loc[0] as i8 as i64,
                2 => i32::from_le_bytes(loc[..4].try_into().unwrap()) as i64,
                _ => i64::from_le_bytes(loc[..8].try_into().unwrap()),
            };
            let addend = embedded + reloc_bias(r.r_type());
            let is_subtracted = i > 0 && rels[i - 1].r_type() == X86_64_RELOC_SUBTRACTOR;

            let (target, addend) = if r.is_extern() {
                (RelocTarget::Sym(r.r_symbolnum()), addend)
            } else {
                let addr = if r.is_pcrel() {
                    (hdr.addr + r.r_address as u64 + 4).wrapping_add_signed(addend)
                } else {
                    addend as u64
                };
                let Some(idx) =
                    crate::target::nonextern_target_section(sections, r.r_section(), addr)
                else {
                    fatal!("{file_name}: bad relocation: {}", r.r_address);
                };
                (RelocTarget::Section(idx as u32), addr.wrapping_sub(sections[idx].addr) as i64)
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
        }
        Ok(vec)
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
            // A GOT load of a local symbol relaxes: the movq that
            // reads the slot (opcode 0x8b) becomes a leaq (0x8d) of
            // the target itself, and a leaq is taken as one already.
            // ld-prime refuses any other instruction. The opcode sits
            // before the fixup, so it is rewritten before the slice
            // below is taken.
            let relaxed_got_load = matches!(r.r_type, X86_64_RELOC_GOT_LOAD | X86_64_RELOC_TLV)
                && ctx.reloc_target_sym(obj, r).is_some_and(|id| ctx.can_relax_got(id));
            if relaxed_got_load {
                match r.offset.checked_sub(2).map(|i| &mut buf[i as usize]) {
                    Some(op) if *op == 0x8b => *op = 0x8d,
                    Some(op) if *op == 0x8d => {}
                    _ => {
                        let kind = if r.r_type == X86_64_RELOC_TLV {
                            "x86_64_was_rip_tlv_elide_got"
                        } else {
                            "x86_64_was_rip_got_load_elide_got"
                        };
                        let msg =
                            format_args!("GOT load fixup does not point to a movq instruction");
                        ctx.fixup_error(isec_id, r.offset, kind, msg);
                    }
                }
            }
            let loc = &mut buf[r.offset as usize..];
            let s = ctx.reloc_target_addr(obj, r);
            let a = r.addend;
            let p = base + r.offset as u64;

            match r.r_type {
                X86_64_RELOC_UNSIGNED if r.size == 4 => {
                    // A 32-bit pointer (.long sym) can be neither slid
                    // nor bound, so ld-prime takes one only in an image
                    // no dyld or kmutil loads, where it must fit
                    // ("oveflow" sic).
                    let val = s.wrapping_add_signed(a);
                    if !ctx.args.static_link {
                        let at = ctx.atom_ref(isec_id, r.offset);
                        error!("32-bit pointer used in 64-bit code in {at}");
                    } else if val > u32::MAX as u64 {
                        let msg = format_args!("32-bit pointer oveflow");
                        ctx.fixup_error(isec_id, r.offset, "ptr32", msg);
                    }
                    write32(loc, val as u32);
                }
                X86_64_RELOC_UNSIGNED => {
                    ctx.check_text_reloc(isec_id, rels, i, p);
                    let imported = ctx
                        .reloc_target_sym(obj, r)
                        .is_some_and(|id| ctx.symbols[id].is_imported());
                    if imported {
                        // The slot is filled by dyld.
                    } else if ctx.reloc_target_is_tls(obj, r) {
                        write64(loc, s.wrapping_add_signed(a) - ctx.tls_begin);
                    } else {
                        write64(loc, s.wrapping_add_signed(a));
                    }
                }
                // read_relocs has paired it with an UNSIGNED of its size.
                X86_64_RELOC_SUBTRACTOR => {
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
                X86_64_RELOC_BRANCH if r.size == 1 => {
                    let sym = ctx.reloc_target_sym(obj, r).unwrap();
                    write_branch8(ctx, isec_id, r, sym, s.wrapping_add_signed(a), p, loc);
                }
                // A kext's call to an import, without a stub, keeps
                // its addend for kmutil's external relocation.
                X86_64_RELOC_BRANCH
                    if ctx.reloc_target_sym(obj, r).is_some_and(|id| {
                        ctx.symbols[id].is_imported()
                            && ctx.sym_aux(id).stub_idx == crate::symbol::NO_IDX
                    }) =>
                {
                    write32(loc, a as u32);
                }
                // A pc-relative fixup that can't reach is an error
                // named after what ld-prime makes of the reference: a
                // call, a plain one, or a GOT or TLV load that it
                // relaxed ("elide") or not. It names a GOT slot ''.
                X86_64_RELOC_BRANCH => {
                    let s = match ctx.reloc_target_sym(obj, r) {
                        Some(id) => ctx.branch_target_addr(id),
                        None => s,
                    };
                    let name = ctx.fixup_target_name(obj, r);
                    let t = s.wrapping_add_signed(a);
                    write32(loc, rip32_displacement(ctx, isec_id, r, "x86_64_call", p, t, name));
                }
                X86_64_RELOC_SIGNED
                | X86_64_RELOC_SIGNED_1
                | X86_64_RELOC_SIGNED_2
                | X86_64_RELOC_SIGNED_4 => {
                    let kind = match r.r_type {
                        X86_64_RELOC_SIGNED => "x86_64_rip",
                        X86_64_RELOC_SIGNED_1 => "x86_64_rip1",
                        X86_64_RELOC_SIGNED_2 => "x86_64_rip2",
                        _ => "x86_64_rip4",
                    };
                    let (t, name) = (s.wrapping_add_signed(a), ctx.fixup_target_name(obj, r));
                    write32(loc, rip32_displacement(ctx, isec_id, r, kind, p, t, name));
                }
                X86_64_RELOC_GOT_LOAD if relaxed_got_load => {
                    let kind = "x86_64_was_rip_got_load_elide_got";
                    let (t, name) = (s.wrapping_add_signed(a), ctx.fixup_target_name(obj, r));
                    write32(loc, rip32_displacement(ctx, isec_id, r, kind, p, t, name));
                }
                X86_64_RELOC_GOT_LOAD | X86_64_RELOC_GOT => {
                    let g = ctx.sym_got_addr(ctx.reloc_target_sym(obj, r).unwrap());
                    let kind = if r.r_type == X86_64_RELOC_GOT {
                        "x86_64_rip_got"
                    } else {
                        "x86_64_was_rip_got_load_load_got"
                    };
                    let t = g.wrapping_add_signed(a);
                    write32(loc, rip32_displacement(ctx, isec_id, r, kind, p, t, ""));
                }
                // A local thread-local's TLV load relaxes just like a
                // GOT load: the movq of the descriptor's GOT slot
                // becomes a leaq of the __thread_vars descriptor itself.
                X86_64_RELOC_TLV if relaxed_got_load => {
                    let kind = "x86_64_was_rip_tlv_elide_got";
                    let (t, name) = (s.wrapping_add_signed(a), ctx.fixup_target_name(obj, r));
                    write32(loc, rip32_displacement(ctx, isec_id, r, kind, p, t, name));
                }
                X86_64_RELOC_TLV => {
                    let g = ctx.sym_got_addr(ctx.reloc_target_sym(obj, r).unwrap());
                    let kind = "x86_64_was_rip_tlv_load_got";
                    let t = g.wrapping_add_signed(a);
                    write32(loc, rip32_displacement(ctx, isec_id, r, kind, p, t, ""));
                }
                _ => fatal!("unsupported relocation type: {}", r.r_type),
            }
            i += 1;
        }
    }
}
