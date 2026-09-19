//! The x86-64 target.

use crate::fatal;
use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::files::FileName;
use crate::macho::format::*;
use crate::macho::input_sections::{Reloc, RelocTarget};

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

impl Arch for X86_64 {
    const NAME: &'static str = "x86_64";
    const CPUTYPE: u32 = CPU_TYPE_X86_64;
    const CPUSUBTYPE: u32 = CPU_SUBTYPE_X86_64_ALL;
    const PAGE_SIZE: u64 = 4096;
    const STUB_SIZE: u64 = 6;
    const STUB_HELPER_HEADER_SIZE: u64 = 16;
    const STUB_HELPER_ENTRY_SIZE: u64 = 10;
    const UNWIND_MODE_DWARF: u32 = UNWIND_X86_64_MODE_DWARF;
    const OBJC_STUB_SIZE: u64 = 16;
    // A 32-bit pcrel branch covers 4 GiB; x86-64 outputs never need
    // thunks.
    const BRANCH_RANGE: u64 = 1 << 32;
    const THUNK_SIZE: u64 = 0;
    const RELOC_UNSIGNED: u8 = X86_64_RELOC_UNSIGNED;
    const RELOC_SUBTRACTOR: u8 = X86_64_RELOC_SUBTRACTOR;
    const RELOC_GOTPC: u8 = X86_64_RELOC_GOT;
    // x86-64 embeds every addend in the relocated field.
    const RELOC_ADDEND: u8 = 0xff;

    fn relocatable_needs_addend(_r_type: u8) -> bool {
        false
    }

    fn reloc_bias(r_type: u8) -> i64 {
        reloc_bias(r_type)
    }

    // GOT_LOAD marks "movq sym@GOTPCREL(%rip), %reg" (opcode 0x8b,
    // REX prefix before it); with a local target the load of the
    // slot's content is the same as computing the address, so the
    // opcode becomes lea (0x8d). Anything else keeps the GOT.
    fn got_load_form(r_type: u8) -> Option<u8> {
        match r_type {
            X86_64_RELOC_SIGNED => Some(X86_64_RELOC_GOT_LOAD),
            _ => None,
        }
    }

    fn can_relax_got_load(data: &[u8], offset: u32, _r_type: u8) -> bool {
        offset >= 2 && data.get(offset as usize - 2) == Some(&0x8b)
    }

    fn classify_reloc(r_type: u8) -> crate::macho::arch::RelocClass {
        use crate::macho::arch::RelocClass;
        match r_type {
            X86_64_RELOC_BRANCH => RelocClass::Branch,
            X86_64_RELOC_GOT_LOAD => RelocClass::GotLoad,
            X86_64_RELOC_GOT => RelocClass::Got,
            X86_64_RELOC_TLV => RelocClass::Tlv,
            _ => RelocClass::Plain,
        }
    }

    fn write_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        for (i, &sym) in ctx.stubs.symbols.iter().enumerate() {
            let ent = &mut buf[i * 6..];
            let ent_addr = addr + i as u64 * 6;
            let ptr_addr = ctx.stub_ptr_addr(i, sym);

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
        // Each entry: push $offset; jmp header.
        for i in 0..ctx.stubs.symbols.len() {
            let off = 16 + i * 10;
            let ent_addr = addr + off as u64;
            buf[off] = 0x68;
            write32(&mut buf[off + 1..], ctx.lazy_bind_info.offsets[i]);
            buf[off + 5] = 0xe9;
            write32(&mut buf[off + 6..], addr.wrapping_sub(ent_addr + 10) as u32);
        }
    }

    fn write_objc_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let msgsend_got = ctx.sym_got_addr(ctx.objc_stubs.msgsend_sym.unwrap());

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
        _syms: &[crate::macho::symbol::SymbolId],
        _buf: &mut [u8],
    ) {
        unreachable!("x86-64 branches never need thunks");
    }

    fn read_relocs(
        file_name: &str,
        sections: &[MachSection],
        hdr: &MachSection,
        file_data: &[u8],
        rels: &[MachRel],
    ) -> Vec<Reloc> {
        let mut vec = Vec::with_capacity(rels.len());

        for (i, r) in rels.iter().enumerate() {
            // On x86-64 every relocation's addend is embedded in the
            // relocated field.
            let off = hdr.offset as usize + r.r_address as usize;
            let embedded = match 1 << r.r_length() {
                4 => i32::from_le_bytes(file_data[off..off + 4].try_into().unwrap()) as i64,
                8 => i64::from_le_bytes(file_data[off..off + 8].try_into().unwrap()),
                _ => fatal!("{file_name}: bad relocation size"),
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
                (RelocTarget::Section(idx as u32), (addr - sections[idx].addr) as i64)
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
            // A GOT load of a local symbol relaxes: the movq that
            // reads the slot (opcode 0x8b) becomes a leaq (0x8d) of
            // the target itself. The opcode sits before the fixup, so
            // it is rewritten before the slice below is taken.
            let mut relaxed_got_load = false;
            if matches!(r.r_type, X86_64_RELOC_GOT_LOAD | X86_64_RELOC_TLV)
                && r.offset >= 2
                && buf[r.offset as usize - 2] == 0x8b
                && ctx.reloc_target_sym(obj, r).is_some_and(|id| ctx.can_relax_got(id))
            {
                buf[r.offset as usize - 2] = 0x8d;
                relaxed_got_load = true;
            }
            let loc = &mut buf[r.offset as usize..];
            let s = ctx.reloc_target_addr(obj, r);
            let a = r.addend;
            let p = base + r.offset as u64;

            match r.r_type {
                X86_64_RELOC_UNSIGNED => {
                    let imported = ctx
                        .reloc_target_sym(obj, r)
                        .is_some_and(|id| ctx.symbols[id].is_imported());
                    if imported {
                        // The slot is filled by dyld.
                    } else if ctx.reloc_target_is_tls(obj, r) {
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
                X86_64_RELOC_SUBTRACTOR => {
                    i += 1;
                    debug_assert!(rels[i].r_type == X86_64_RELOC_UNSIGNED);
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
                X86_64_RELOC_BRANCH => {
                    debug_assert!(r.size == 4);
                    let s = match ctx.reloc_target_sym(obj, r) {
                        Some(id) => ctx.branch_target_addr(id),
                        None => s,
                    };
                    let val = s.wrapping_add_signed(a).wrapping_sub(p + 4);
                    write32(loc, val as u32);
                }
                X86_64_RELOC_SIGNED
                | X86_64_RELOC_SIGNED_1
                | X86_64_RELOC_SIGNED_2
                | X86_64_RELOC_SIGNED_4 => {
                    debug_assert!(r.size == 4);
                    let val = s
                        .wrapping_add_signed(a)
                        .wrapping_sub(p + 4)
                        .wrapping_sub(reloc_bias(r.r_type) as u64);
                    write32(loc, val as u32);
                }
                X86_64_RELOC_GOT_LOAD if relaxed_got_load => {
                    debug_assert!(r.size == 4);
                    let val = s.wrapping_add_signed(a).wrapping_sub(p + 4);
                    write32(loc, val as u32);
                }
                X86_64_RELOC_GOT_LOAD | X86_64_RELOC_GOT => {
                    debug_assert!(r.size == 4);
                    let g = ctx.sym_got_addr(ctx.reloc_target_sym(obj, r).unwrap());
                    let val = g.wrapping_add_signed(a).wrapping_sub(p + 4);
                    write32(loc, val as u32);
                }
                // A local thread-local's TLV load relaxes just like a
                // GOT load: the movq of the __thread_ptrs slot becomes
                // a leaq of the __thread_vars descriptor itself.
                X86_64_RELOC_TLV if relaxed_got_load => {
                    debug_assert!(r.size == 4);
                    let val = s.wrapping_add_signed(a).wrapping_sub(p + 4);
                    write32(loc, val as u32);
                }
                X86_64_RELOC_TLV => {
                    debug_assert!(r.size == 4);
                    let t = ctx.sym_tlv_ptr_addr(ctx.reloc_target_sym(obj, r).unwrap());
                    let val = t.wrapping_add_signed(a).wrapping_sub(p + 4);
                    write32(loc, val as u32);
                }
                _ => fatal!("unsupported relocation type: {}", r.r_type),
            }
            i += 1;
        }
    }
}
