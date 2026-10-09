//! The x86-64 target.

use std::path::Path;

use mold_common::endian::{write_ul32, write_ul64};

use crate::arch::{SplitRef, Target, has_reloc_form, reloc_form};
use crate::chunks::delay_init::{DelayCode, DelayTarget, DelayUse};
use crate::chunks::lazy_helpers::{LazyTarget, LazyUse};
use crate::chunks::{delay_init, lazy_helpers, objc_stubs, stub_helper, stubs};
use crate::context::Context;
use crate::dtrace::SiteKind;
use crate::input_files::{ObjectFile, section_target};
use crate::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho::*;
use crate::symbol::{NEEDS_GOT, NEEDS_STUB, SymbolId};
use crate::{error, fatal};

#[derive(Clone, Copy, Default)]
pub struct X86_64;

/// The start of a delay-init stub or load helper at `base`, which
/// makes sure the dylib of dlopen helper `dlopen` is initialized:
/// cmpl $0, flag(%rip); jne 1f; push %rbp; mov %rsp, %rbp; call the
/// dlopen helper; pop %rbp; 1:
fn write_delay_check(ctx: &Context<X86_64>, ent: &mut [u8], base: u64, dlopen: u32) {
    let flag = ctx.isecs[ctx.delay_init.dlopens[dlopen as usize].flag as usize].addr(ctx);
    let helper = ctx.delay_init.dlopen_helper_addr(dlopen as usize);
    ent[..19].copy_from_slice(&[
        0x83, 0x3d, 0, 0, 0, 0, 0, 0x75, 0x0a, 0x55, 0x48, 0x89, 0xe5, 0xe8, 0, 0, 0, 0, 0x5d,
    ]);
    write_ul32(&mut ent[2..], flag.wrapping_sub(base + 7) as u32);
    write_ul32(&mut ent[14..], helper.wrapping_sub(base + 18) as u32);
}

/// "movq disp32(%rip), %reg" up to its displacement: REX.W (and REX.R
/// for %r8-%r15), the opcode, and a ModRM of `reg` and RIP-relative.
fn movq_rip(reg: u8) -> [u8; 3] {
    [0x48 | (reg >> 3) << 2, 0x8b, 0x05 | (reg & 7) << 3]
}

/// A dlopen helper, but for the displacements of the leaq of the
/// install name (at 68), the call of dlopen (75) and the xchgl of the
/// flag (86).
const DLOPEN_HELPER: [u8; 153] = [
    0x55, // push %rbp
    0x48, 0x89, 0xe5, // mov %rsp, %rbp
    0x50, 0x51, 0x52, 0x53, 0x56, 0x57, 0x41, 0x50, 0x41, 0x51, // push %rax ... %r9
    0x48, 0x83, 0xc4, 0x80, // add $-0x80, %rsp
    0xf3, 0x0f, 0x7f, 0x04, 0x24, // movdqu %xmm0, (%rsp)
    0xf3, 0x0f, 0x7f, 0x4c, 0x24, 0x10, // movdqu %xmm1, 0x10(%rsp)
    0xf3, 0x0f, 0x7f, 0x54, 0x24, 0x20, 0xf3, 0x0f, 0x7f, 0x5c, 0x24, 0x30, 0xf3, 0x0f, 0x7f, 0x64,
    0x24, 0x40, 0xf3, 0x0f, 0x7f, 0x6c, 0x24, 0x50, 0xf3, 0x0f, 0x7f, 0x74, 0x24, 0x60, 0xf3, 0x0f,
    0x7f, 0x7c, 0x24, 0x70, // movdqu %xmm7, 0x70(%rsp)
    0x48, 0x8d, 0x3d, 0, 0, 0, 0, // leaq name(%rip), %rdi
    0x31, 0xf6, // xorl %esi, %esi
    0xe8, 0, 0, 0, 0, // call dlopen
    0xb8, 0x01, 0, 0, 0, // movl $1, %eax
    0x87, 0x05, 0, 0, 0, 0, // xchgl %eax, flag(%rip)
    0xf3, 0x0f, 0x6f, 0x04, 0x24, // movdqu (%rsp), %xmm0
    0xf3, 0x0f, 0x6f, 0x4c, 0x24, 0x10, 0xf3, 0x0f, 0x6f, 0x54, 0x24, 0x20, 0xf3, 0x0f, 0x6f, 0x5c,
    0x24, 0x30, 0xf3, 0x0f, 0x6f, 0x64, 0x24, 0x40, 0xf3, 0x0f, 0x6f, 0x6c, 0x24, 0x50, 0xf3, 0x0f,
    0x6f, 0x74, 0x24, 0x60, 0xf3, 0x0f, 0x6f, 0x7c, 0x24, 0x70, // movdqu 0x70(%rsp), %xmm7
    0x48, 0x83, 0xec, 0x80, // sub $-0x80, %rsp
    0x41, 0x59, 0x41, 0x58, 0x5f, 0x5e, 0x5b, 0x5a, 0x59, 0x58, // pop %r9 ... %rax
    0x5d, // pop %rbp
    0xc3, // ret
];

/// What the call or jmp of a DTrace probe site becomes, from its opcode
/// on, if relocation `r` is one (see dtrace): a nop and a 4-byte nop,
/// or for an is-enabled test "xorl %eax, %eax" (its result false) and
/// nops.
fn dtrace_site_code(
    ctx: &Context<X86_64>,
    file: &ObjectFile,
    r: &Reloc,
) -> Option<&'static [u8; 5]> {
    let id = r.sym(file)?;
    match crate::dtrace::site_kind(ctx, id)? {
        SiteKind::Probe => Some(&[0x90, 0x0f, 0x1f, 0x40, 0x00]),
        SiteKind::IsEnabled => Some(&[0x33, 0xc0, 0x90, 0x90, 0x90]),
    }
}

/// The SIGNED_K relocation types describe a pcrel field followed by K
/// more instruction bytes; the extra distance is folded into the addend
/// when reading and taken back out when writing.
fn reloc_bias(ty: u8) -> i64 {
    match ty {
        X86_64_RELOC_SIGNED_1 => 1,
        X86_64_RELOC_SIGNED_2 => 2,
        X86_64_RELOC_SIGNED_4 => 4,
        _ => 0,
    }
}

/// Whether a record's pcrel, p2size and extern fields are ones its type
/// takes. An assembler writes other forms for `.short sym` or
/// `.quad sym@GOTPCREL`.
#[inline]
fn is_supported(r: &MachRel) -> bool {
    let forms = match r.ty() {
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

/// The displacement a 32-bit pc-relative fixup, relocation `r` of
/// subsection `isec` at `p`, holds to reach `t` (its target, or the
/// target's stub or GOT slot): from the end of the instruction, past the
/// field and the immediate after it a SIGNED_1/2/4 counts. One that
/// doesn't fit is an error.
fn rip32_displacement(
    ctx: &Context<X86_64>,
    isec: &InputSection,
    r: &Reloc,
    p: u64,
    t: u64,
) -> u32 {
    let disp = t.wrapping_sub(p + 4).wrapping_sub(reloc_bias(r.ty) as u64) as i64;
    if i32::try_from(disp).is_err() {
        let name = r.target_name(ctx, &ctx.objs[isec.file as usize]);
        let name = crate::error::raw(&name);
        let msg = format_args!(
            "32-bit RIP-relative reference out of range (displacement={disp}, max is +/-2GB), \
             from 0x{p:08X} to 0x{t:08X} ('{name}')"
        );
        isec.fixup_error(ctx, r.offset, msg);
    }
    disp as u32
}

/// Writes a one-byte branch (jmp rel8) at `loc`, relocation `r` of
/// subsection `isec` whose address is `p`, to `t`, the address of
/// `sym`. It reaches only a definition near it in the image: ld-prime
/// gives it no stub.
fn write_branch8(
    ctx: &Context<X86_64>,
    isec: &InputSection,
    r: &Reloc,
    sym: SymbolId,
    t: u64,
    p: u64,
    loc: &mut [u8],
) {
    let sym = &ctx.symbols[sym];
    let val = t.wrapping_sub(p + 1) as i64;
    if sym.is_imported() {
        isec.fixup_error(ctx, r.offset, format_args!("target '{sym}' does not have address"));
    } else if !(-128..128).contains(&val) {
        let msg = format_args!(
            "8-bit branch out of range (displacement={val}, max is +/-127), \
             from 0x{p:X} to 0x{t:X} ('{sym}')"
        );
        isec.fixup_error(ctx, r.offset, msg);
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
    const UNWIND_MODE_DWARF: u32 = UNWIND_X86_64_MODE_DWARF;
    const OBJC_STUB_SIZE: u64 = 13;
    // ld-prime has no small form of x86-64's.
    const OBJC_SMALL_STUB_SIZE: u64 = 13;
    const LAZY_HELPERS_P2ALIGN: u32 = 0;
    const LAZY_CALL_OWN_SLOT: bool = true;
    const DELAY_STUB_SIZE: u64 = 25;
    const DLOPEN_HELPER_SIZE: u32 = DLOPEN_HELPER.len() as u32;
    const DELAY_P2ALIGN: u32 = 0;
    // A 32-bit pcrel branch covers 4 GiB; x86-64 outputs never need
    // thunks.
    const BRANCH_RANGE: u64 = 1 << 32;
    const THUNK_SIZE: u64 = 0;
    const RELOC_UNSIGNED: u8 = X86_64_RELOC_UNSIGNED;
    const RELOC_SUBTRACTOR: u8 = X86_64_RELOC_SUBTRACTOR;
    const RELOC_GOTPC: u8 = X86_64_RELOC_GOT;
    const RELOC_BRANCH: u8 = X86_64_RELOC_BRANCH;
    const RELOC_GOT_LOADS: &'static [u8] = &[X86_64_RELOC_GOT_LOAD, X86_64_RELOC_TLV];
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

    fn relocatable_needs_addend(_ty: u8) -> bool {
        false
    }

    fn reloc_bias(ty: u8) -> i64 {
        reloc_bias(ty)
    }

    fn split_ref(ty: u8) -> SplitRef {
        match ty {
            X86_64_RELOC_UNSIGNED => SplitRef::Pointer,
            X86_64_RELOC_SUBTRACTOR => SplitRef::Subtractor,
            _ => SplitRef::PcRel32,
        }
    }

    // GOT_LOAD marks "movq sym@GOTPCREL(%rip), %reg" (opcode 0x8b,
    // after a REX prefix), which a local target relaxes to lea (0x8d).
    // A leaq of a slot takes its address.
    fn got_load_form(ty: u8, data: &[u8], offset: u32) -> Option<u8> {
        let mov = offset >= 2 && data.get(offset as usize - 2) == Some(&0x8b);
        (ty == X86_64_RELOC_SIGNED && mov).then_some(X86_64_RELOC_GOT_LOAD)
    }

    fn write_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        for (i, &sym) in ctx.stubs.symbols.iter().enumerate() {
            let off = stubs::entry_offset::<Self>(i as u32);
            let ent = &mut buf[off as usize..];
            let ent_addr = addr + off;
            let ptr_addr = ctx.symbols[sym].stub_ptr_addr(ctx, i);
            let disp = ptr_addr.wrapping_sub(ent_addr + 6) as i64;
            if i32::try_from(disp).is_err() {
                let p = ent_addr + 2;
                crate::error!(
                    "stub for {}: 32-bit RIP-relative reference out of range (displacement={disp}, \
                     max is +/-2GB), from 0x{p:08X} to its pointer at 0x{ptr_addr:08X}",
                    ctx.symbols[sym]
                );
            }

            // jmp *ptr(%rip)
            ent[0] = 0xff;
            ent[1] = 0x25;
            write_ul32(&mut ent[2..], disp as u32);
        }
    }

    fn write_stub_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        // The header, as ld64 emits it:
        //   lea  __dyld_private(%rip), %r11
        //   push %r11
        //   jmp  *dyld_stub_binder@GOTPCREL(%rip)
        //   nop
        let private = ctx.isecs[ctx.stub_helper.dyld_private_isec as usize].addr(ctx);
        let binder = ctx.symbols[ctx.stub_helper.dyld_stub_binder.unwrap()].got_addr(ctx);
        buf[0..3].copy_from_slice(&[0x4c, 0x8d, 0x1d]);
        write_ul32(&mut buf[3..], private.wrapping_sub(addr + 7) as u32);
        buf[7..9].copy_from_slice(&[0x41, 0x53]);
        buf[9..11].copy_from_slice(&[0xff, 0x25]);
        write_ul32(&mut buf[11..], binder.wrapping_sub(addr + 15) as u32);
        buf[15] = 0x90;
        // Each entry: push $offset; jmp header; the zero padding.
        let lazy_offsets = &ctx.lazy_bind_info.offsets[..ctx.stubs.lazy.len()];
        for (i, &lazy_off) in lazy_offsets.iter().enumerate() {
            let off = stub_helper::entry_offset(ctx, i as u32) as usize;
            let ent_addr = addr + off as u64;
            buf[off] = 0x68;
            write_ul32(&mut buf[off + 1..], lazy_off);
            buf[off + 5] = 0xe9;
            write_ul32(&mut buf[off + 6..], addr.wrapping_sub(ent_addr + 10) as u32);
            buf[off + 10..off + 12].fill(0);
        }
    }

    /// Each entry hands dyld_stub_binding_helper the address of its lazy
    /// pointer, which dyld binds by the indirect symbol table.
    ///   lea  lazy_ptr(%rip), %r11
    ///   jmp  dyld_stub_binding_helper
    fn write_legacy_stub_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let helper = ctx.stub_helper.binding_helper.map(|id| ctx.symbols[id].addr(ctx));
        if helper.is_none() {
            crate::error!("stub helper: target 'dyld_stub_binding_helper' does not have address");
        }
        for i in 0..ctx.stubs.lazy.len() {
            let off = stub_helper::entry_offset(ctx, i as u32);
            let ent = &mut buf[off as usize..];
            let ent_addr = addr + off;
            let ptr = ctx.lazy_ptrs.slot_addr(i);
            ent[0..3].copy_from_slice(&[0x4c, 0x8d, 0x1d]);
            write_ul32(&mut ent[3..], ptr.wrapping_sub(ent_addr + 7) as u32);
            ent[7] = 0xe9;
            write_ul32(&mut ent[8..], helper.unwrap_or(0).wrapping_sub(ent_addr + 12) as u32);
        }
    }

    fn write_objc_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let msgsend_got = ctx.objc_msgsend_got_addr();

        for i in 0..ctx.objc_stubs.symbols.len() {
            let off = objc_stubs::entry_offset(ctx, i as u32);
            let ent = &mut buf[off as usize..];
            let ent_addr = addr + off;
            let sel_addr = ctx.objc_stubs.selref_addr(ctx, i);

            // mov sel(%rip), %rsi; jmp *_objc_msgSend@GOT(%rip), packed
            // back to back as ld-prime lays them out.
            ent[..13].copy_from_slice(&[0x48, 0x8b, 0x35, 0, 0, 0, 0, 0xff, 0x25, 0, 0, 0, 0]);
            write_ul32(&mut ent[3..], sel_addr.wrapping_sub(ent_addr + 7) as u32);
            write_ul32(&mut ent[9..], msgsend_got.wrapping_sub(ent_addr + 13) as u32);
        }
    }

    fn write_lazy_helpers(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let lazy_load = ctx.symbols[ctx.lazy_helpers.dyld_lazy_load.unwrap()].stub_addr(ctx);
        let header = ctx.mach_header.hdr.addr;
        for h in &ctx.lazy_helpers.helpers {
            let size = Self::lazy_helper_size(h.kind) as usize;
            let ent = &mut buf[h.offset as usize..h.offset as usize + size];
            let base = addr + h.offset as u64;
            let flag = ctx.isecs[h.flag as usize].addr(ctx);
            let slot = ctx.lazy_load_got.slot_addr(h.slot);
            // The call of __dyld_lazy_load(&flag, mach header), by which
            // dyld finds the dylib's record, with the argument registers
            // saved: push %rbp; mov %rsp, %rbp; push %rsi; push %rdi;
            // lea flag(%rip), %rdi; lea header(%rip), %rsi; call; pop
            // %rdi; pop %rsi; pop %rbp.
            let call: [u8; 28] = [
                0x55, 0x48, 0x89, 0xe5, 0x56, 0x57, 0x48, 0x8d, 0x3d, 0, 0, 0, 0, 0x48, 0x8d, 0x35,
                0, 0, 0, 0, 0xe8, 0, 0, 0, 0, 0x5f, 0x5e, 0x5d,
            ];
            // cmpl $0, flag(%rip)
            ent[..7].copy_from_slice(&[0x83, 0x3d, 0, 0, 0, 0, 0]);
            write_ul32(&mut ent[2..], flag.wrapping_sub(base + 7) as u32);
            let start = match h.kind {
                // je 1f; 2: jmp *slot(%rip); 1: (the call); jmp 2b
                LazyUse::Call => {
                    ent[7..15].copy_from_slice(&[0x74, 0x06, 0xff, 0x25, 0, 0, 0, 0]);
                    write_ul32(&mut ent[11..], slot.wrapping_sub(base + 15) as u32);
                    ent[43..45].copy_from_slice(&[0xeb, 0xdc]);
                    15
                }
                // jne 1f; (the call); 1: movq slot(%rip), %reg; ret
                LazyUse::Load { reg, .. } => {
                    ent[7..9].copy_from_slice(&[0x75, 0x1c]);
                    ent[37..40].copy_from_slice(&movq_rip(reg));
                    write_ul32(&mut ent[40..], slot.wrapping_sub(base + 44) as u32);
                    ent[44] = 0xc3;
                    9
                }
                // jne 1f; (the call); 1: cmpq $0, slot(%rip); ret
                LazyUse::Cmp => {
                    ent[7..9].copy_from_slice(&[0x75, 0x1c]);
                    ent[37..45].copy_from_slice(&[0x48, 0x83, 0x3d, 0, 0, 0, 0, 0]);
                    write_ul32(&mut ent[40..], slot.wrapping_sub(base + 45) as u32);
                    ent[45] = 0xc3;
                    9
                }
            };
            let at = base + start as u64;
            ent[start..start + call.len()].copy_from_slice(&call);
            write_ul32(&mut ent[start + 9..], flag.wrapping_sub(at + 13) as u32);
            write_ul32(&mut ent[start + 16..], header.wrapping_sub(at + 20) as u32);
            write_ul32(&mut ent[start + 21..], lazy_load.wrapping_sub(at + 25) as u32);
        }
    }

    fn lazy_helper_size(kind: LazyUse) -> u32 {
        if kind == LazyUse::Cmp { 46 } else { 45 }
    }

    // cmpl $0, flag(%rip); jne 1f
    // push %rbp; mov %rsp, %rbp; call dlopen helper; pop %rbp
    // 1: jmp *slot(%rip)
    fn write_delay_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        for (i, stub) in ctx.delay_init.stubs.iter().enumerate() {
            let at = delay_init::stub_offset::<Self>(i as u32) as usize;
            let ent = &mut buf[at..at + Self::DELAY_STUB_SIZE as usize];
            let base = addr + at as u64;
            write_delay_check(ctx, ent, base, stub.dlopen);
            ent[19..21].copy_from_slice(&[0xff, 0x25]);
            let slot = ctx.got.slot_addr(stub.got as usize);
            write_ul32(&mut ent[21..], slot.wrapping_sub(base + 25) as u32);
        }
    }

    // (the check as a stub's) 1: movq slot(%rip), %reg; ret, or
    // cmpq $0, slot(%rip); ret.
    //
    // A dlopen helper calls dlopen(install name, 0) with the argument
    // registers saved, then sets the flag by an xchgl. ld-prime's saves
    // the general-purpose ones alone, which lets dlopen() clobber the
    // floating-point arguments of the call that first reaches the
    // dylib (addf(1.0, 2.0, 3.0, 4.0) returns 0.5): this one saves
    // xmm0-xmm7 too.
    fn write_delay_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]) {
        let delay = &ctx.delay_init;
        for h in &delay.helpers {
            let size = Self::delay_helper_size(h.kind) as usize;
            let ent = &mut buf[h.offset as usize..h.offset as usize + size];
            let base = addr + h.offset as u64;
            write_delay_check(ctx, ent, base, h.dlopen);
            let slot = ctx.symbols[h.sym].got_addr(ctx);
            match h.kind {
                DelayUse::Load { reg, .. } => {
                    ent[19..22].copy_from_slice(&movq_rip(reg));
                    write_ul32(&mut ent[22..], slot.wrapping_sub(base + 26) as u32);
                    ent[26] = 0xc3;
                }
                DelayUse::Cmp => {
                    ent[19..22].copy_from_slice(&[0x48, 0x83, 0x3d]);
                    write_ul32(&mut ent[22..], slot.wrapping_sub(base + 27) as u32);
                    ent[26..28].copy_from_slice(&[0, 0xc3]);
                }
            }
        }
        let dlopen = ctx.symbols[delay.dlopen_sym.unwrap()].stub_addr(ctx);
        for d in &delay.dlopens {
            let size = Self::DLOPEN_HELPER_SIZE as usize;
            let ent = &mut buf[d.offset as usize..d.offset as usize + size];
            let base = addr + d.offset as u64;
            ent.copy_from_slice(&DLOPEN_HELPER);
            let (name, flag) =
                (ctx.isecs[d.string as usize].addr(ctx), ctx.isecs[d.flag as usize].addr(ctx));
            write_ul32(&mut ent[68..], name.wrapping_sub(base + 72) as u32);
            write_ul32(&mut ent[75..], dlopen.wrapping_sub(base + 79) as u32);
            write_ul32(&mut ent[86..], flag.wrapping_sub(base + 90) as u32);
        }
    }

    fn delay_helper_size(kind: DelayUse) -> u32 {
        if kind == DelayUse::Cmp { 28 } else { 27 }
    }

    // Each a 32-bit displacement.
    fn delay_refs(code: DelayCode) -> Vec<(u32, u8, DelayTarget)> {
        use DelayTarget::*;
        let refs = match code {
            DelayCode::Stub => vec![(2, Flag), (14, DlopenHelper), (21, Slot)],
            DelayCode::Helper(_) => vec![(2, Flag), (14, DlopenHelper), (22, Slot)],
            DelayCode::Dlopen => vec![(68, Name), (75, Dlopen), (86, Flag)],
        };
        refs.into_iter().map(|(off, to)| (off, DYLD_CACHE_ADJ_V2_DELTA_32, to)).collect()
    }

    // Each a 32-bit displacement: the flag's in the cmpl, the lea's of
    // the call's arguments and the call's; and the jmp's or the final
    // movq's or cmpq's of the slot.
    fn lazy_helper_refs(kind: LazyUse) -> Vec<(u32, u8, LazyTarget)> {
        use LazyTarget::*;
        let call = |k: u32| [(k + 9, Flag), (k + 16, Header), (k + 21, LazyLoad)];
        let mut refs = vec![(2, Flag)];
        match kind {
            LazyUse::Call => {
                refs.push((11, Slot));
                refs.extend(call(15));
            }
            _ => {
                refs.extend(call(9));
                refs.push((40, Slot));
            }
        }
        refs.into_iter().map(|(off, to)| (off, DYLD_CACHE_ADJ_V2_DELTA_32, to)).collect()
    }

    fn lazy_ref(r: &Reloc, data: &[u8]) -> crate::arch::LazyRef {
        use crate::arch::LazyRef;
        let off = r.offset as usize;
        match r.ty {
            X86_64_RELOC_BRANCH if r.size == 4 => LazyRef::Call,
            X86_64_RELOC_GOT_LOAD => LazyRef::Load,
            // cmpq $0, sym@GOTPCREL(%rip)
            X86_64_RELOC_GOT
                if off >= 3
                    && data[off - 3..off] == [0x48, 0x83, 0x3d]
                    && data.get(off + 4) == Some(&0) =>
            {
                LazyRef::Cmp
            }
            _ => LazyRef::Unsupported,
        }
    }

    // The movq's destination: ModRM's reg field, extended by REX.R.
    // The helper returns (by ret), so any code may call it.
    fn lazy_load_site(data: &[u8], offset: u32) -> (u8, bool) {
        let off = offset as usize;
        let reg = (data[off - 3] >> 2 & 1) << 3 | (data[off - 1] >> 3 & 7);
        (reg, false)
    }

    // (ld-prime names both %rsp and %rbp "xxx".)
    fn lazy_register_name(reg: u8) -> String {
        const NAMES: [&str; 16] = [
            "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11",
            "r12", "r13", "r14", "r15",
        ];
        NAMES[reg as usize & 15].to_string()
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
    ) -> Vec<Reloc> {
        let mut vec = Vec::with_capacity(rels.len());

        for (i, r) in rels.iter().enumerate() {
            if !is_supported(r) {
                crate::arch::bad_reloc(file_name, hdr, r, "unsupported relocation");
            }

            // On x86-64 every relocation's addend is embedded in the
            // relocated field.
            let loc = &contents[r.offset as usize..];
            let embedded = match r.p2size() {
                0 => loc[0] as i8 as i64,
                2 => i32::from_le_bytes(loc[..4].try_into().unwrap()) as i64,
                _ => i64::from_le_bytes(loc[..8].try_into().unwrap()),
            };
            let addend = embedded + reloc_bias(r.ty());
            let is_subtracted = i > 0 && rels[i - 1].ty() == X86_64_RELOC_SUBTRACTOR;

            // A non-extern record's field holds the address it points
            // at, a pcrel one as a displacement from the field's end.
            let (target, addend) = if r.is_extern() {
                (RelocTarget::Sym(r.idx()), addend)
            } else if r.is_pcrel() {
                let addr = (hdr.addr + r.offset as u64 + 4).wrapping_add_signed(addend);
                section_target(file_name, sections, r, addr)
            } else {
                section_target(file_name, sections, r, addend as u64)
            };

            vec.push(Reloc {
                offset: r.offset,
                ty: r.ty(),
                size: 1 << r.p2size(),
                is_pcrel: r.is_pcrel(),
                is_subtracted,
                target: target.pack(),
                addend,
            });
        }
        vec
    }

    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection) {
        let file = &ctx.objs[isec.file as usize];
        for rel in isec.rels(file) {
            let Some(id) = rel.sym(file) else { continue };
            let sym = &ctx.symbols[id];
            // A lazy dylib's symbols take no stub or GOT slot; the image
            // reaches them through the helpers of
            // lazy_load::create_lazy_loads.
            if sym.is_lazy_import(ctx) {
                continue;
            }

            // A TLV load must load a thread-local, and only a TLV load
            // may, as on arm64.
            if (rel.ty == X86_64_RELOC_TLV) != sym.is_tlv(ctx) {
                error!("illegal thread local variable reference to regular symbol `{sym}`");
            }

            match rel.ty {
                // A one-byte branch (jmp rel8) reaches only code near it,
                // so it takes no stub: one to an import is a fixup error,
                // as in ld-prime.
                X86_64_RELOC_BRANCH if rel.size == 1 => {}
                // A kext calls an import directly, unless
                // -kexts_use_stubs: kmutil fills in the call by an
                // external relocation, or the stub's GOT slot. What dyld
                // resolves by weak lookup is called through a stub all
                // the same.
                X86_64_RELOC_BRANCH
                    if ctx.args.is_kext()
                        && !ctx.args.kexts_use_stubs
                        && !sym.binds_weak_lookup(ctx) => {}
                // Otherwise, as on arm64.
                X86_64_RELOC_BRANCH => {
                    if !sym.is_delay_import(ctx)
                        && (sym.binds_as_import(ctx) || sym.binds_weak_lookup(ctx))
                    {
                        sym.add_flags(NEEDS_STUB);
                    }
                }
                X86_64_RELOC_GOT_LOAD | X86_64_RELOC_TLV => {
                    if !sym.can_relax_got(ctx) {
                        sym.add_flags(NEEDS_GOT);
                    }
                }
                X86_64_RELOC_GOT => sym.add_flags(NEEDS_GOT),
                _ => {}
            }
        }
    }

    fn apply_reloc_alloc(
        ctx: &Context<Self>,
        rels: &[Reloc],
        isec_id: usize,
        base: u64,
        buf: &mut [u8],
    ) {
        let isec = &ctx.isecs[isec_id];
        let file = &ctx.objs[isec.file as usize];
        let mut i = 0;
        while i < rels.len() {
            let r = &rels[i];
            // A GOT load of a local symbol relaxes: the movq that
            // reads the slot (opcode 0x8b) becomes a leaq (0x8d) of
            // the target itself, and a leaq is taken as one already.
            // ld-prime refuses any other instruction. The opcode sits
            // before the fixup, so it is rewritten before the slice
            // below is taken.
            let relaxed_got_load = matches!(r.ty, X86_64_RELOC_GOT_LOAD | X86_64_RELOC_TLV)
                && r.sym(file).is_some_and(|id| ctx.symbols[id].can_relax_got(ctx));
            if relaxed_got_load {
                match r.offset.checked_sub(2).map(|i| &mut buf[i as usize]) {
                    Some(op) if *op == 0x8b => *op = 0x8d,
                    Some(op) if *op == 0x8d => {}
                    _ => {
                        let msg =
                            format_args!("GOT load fixup does not point to a movq instruction");
                        isec.fixup_error(ctx, r.offset, msg);
                    }
                }
            }
            // A GOT load or compare of a lazy or delay-init dylib's
            // symbol becomes a call of its helper, nops filling the rest
            // of the movq or cmpq (see LazyUse and DelayUse).
            if matches!(r.ty, X86_64_RELOC_GOT_LOAD | X86_64_RELOC_GOT)
                && let Some((helper, _)) = lazy_helpers::load_helper(ctx, isec_id, r)
                    .or_else(|| delay_init::load_helper(ctx, isec_id, r))
            {
                let at = r.offset as usize - 3;
                let disp = helper.wrapping_sub(base + at as u64 + 5);
                buf[at] = 0xe8;
                write_ul32(&mut buf[at + 1..], disp as u32);
                let end = r.offset as usize + if r.ty == X86_64_RELOC_GOT { 5 } else { 4 };
                buf[at + 5..end].fill(0x90);
                i += 1;
                continue;
            }
            // A DTrace probe site does nothing (see dtrace).
            if r.ty == X86_64_RELOC_BRANCH
                && r.size == 4
                && let Some(code) = dtrace_site_code(ctx, file, r)
            {
                let at = r.offset as usize - 1;
                buf[at..at + 5].copy_from_slice(code);
                i += 1;
                continue;
            }
            let loc = &mut buf[r.offset as usize..];
            let s = r.addr(ctx, file);
            let a = r.addend;
            let p = base + r.offset as u64;

            match r.ty {
                X86_64_RELOC_UNSIGNED if r.size == 4 => {
                    // A 32-bit pointer (.long sym) can be neither slid
                    // nor bound, so ld-prime takes one only in an image
                    // no dyld or kmutil loads, where it must fit
                    // ("oveflow" sic). Elsewhere one where a pointer
                    // would be a text relocation is one, and any other
                    // an error (see passes::report_text_relocs).
                    let val = s.wrapping_add_signed(a);
                    if ctx.args.static_link {
                        if val > u32::MAX as u64 {
                            isec.fixup_error(
                                ctx,
                                r.offset,
                                format_args!("32-bit pointer overflow"),
                            );
                        }
                    } else if ctx.text_reloc_ranges.iter().any(|range| range.contains(&p)) {
                        isec.check_text_reloc(ctx, isec_id, rels, i, p);
                    } else {
                        ctx.pointers32.lock().unwrap().push((isec_id as u32, r.offset));
                    }
                    write_ul32(loc, val as u32);
                }
                X86_64_RELOC_UNSIGNED => {
                    isec.check_text_reloc(ctx, isec_id, rels, i, p);
                    let imported = r.sym(file).is_some_and(|id| ctx.symbols[id].binds_pointer(ctx));
                    if imported {
                        // The slot is filled by dyld. It keeps the
                        // addend, to which a legacy LINKEDIT external
                        // relocation has dyld add the symbol's address.
                    } else if r.refers_to_tls(ctx, file) {
                        write_ul64(loc, s.wrapping_add_signed(a).wrapping_sub(ctx.tls_begin));
                    } else {
                        write_ul64(loc, s.wrapping_add_signed(a));
                    }
                }
                // The assembler pairs it with an UNSIGNED of its size.
                X86_64_RELOC_SUBTRACTOR => {
                    i += 1;
                    let val =
                        rels[i].addr(ctx, file).wrapping_add_signed(rels[i].addend).wrapping_sub(s);
                    if r.size == 4 {
                        write_ul32(loc, val as u32);
                    } else {
                        write_ul64(loc, val);
                    }
                }
                X86_64_RELOC_BRANCH if r.size == 1 => {
                    let sym = r.sym(file).unwrap();
                    write_branch8(ctx, isec, r, sym, s.wrapping_add_signed(a), p, loc);
                }
                // A kext's call to an import, without a stub, keeps
                // its addend for kmutil's external relocation.
                X86_64_RELOC_BRANCH
                    if r.sym(file).is_some_and(|id| {
                        let sym = &ctx.symbols[id];
                        sym.is_imported() && !sym.has_stub(&ctx.symbols)
                    }) =>
                {
                    write_ul32(loc, a as u32);
                }
                // A pc-relative fixup that can't reach is an error.
                X86_64_RELOC_BRANCH => {
                    let s = match r.sym(file) {
                        Some(id) => ctx.symbols[id].branch_target_addr(ctx),
                        None => s,
                    };
                    let t = s.wrapping_add_signed(a);
                    write_ul32(loc, rip32_displacement(ctx, isec, r, p, t));
                }
                X86_64_RELOC_SIGNED
                | X86_64_RELOC_SIGNED_1
                | X86_64_RELOC_SIGNED_2
                | X86_64_RELOC_SIGNED_4 => {
                    if isec.target_has_address(ctx, r) {
                        let t = s.wrapping_add_signed(a);
                        write_ul32(loc, rip32_displacement(ctx, isec, r, p, t));
                    }
                }
                // A local thread-local's TLV load relaxes just like a
                // GOT load: the movq of the descriptor's GOT slot
                // becomes a leaq of the __thread_vars descriptor itself.
                X86_64_RELOC_GOT_LOAD | X86_64_RELOC_TLV if relaxed_got_load => {
                    let t = s.wrapping_add_signed(a);
                    write_ul32(loc, rip32_displacement(ctx, isec, r, p, t));
                }
                X86_64_RELOC_GOT_LOAD | X86_64_RELOC_GOT | X86_64_RELOC_TLV => {
                    let g = ctx.symbols[r.sym(file).unwrap()].got_addr(ctx);
                    let t = g.wrapping_add_signed(a);
                    write_ul32(loc, rip32_displacement(ctx, isec, r, p, t));
                }
                _ => fatal!("unsupported relocation type: {}", r.ty),
            }
            i += 1;
        }
    }
}
