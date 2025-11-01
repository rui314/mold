// Supporting x86-64 is straightforward. Unlike its predecessor, i386,
// x86-64 supports PC-relative addressing for position-independent code.
// Being CISC, its instructions are variable in size. Branch instructions
// take 4 bytes offsets, so we don't need range extension thunks.
//
// The psABI specifies %r11 as neither caller- nor callee-saved. It's
// intentionally left out so that we can use it as a scratch register in
// PLT.
//
// Thread Pointer (TP) is stored not to a general-purpose register but to
// FS segment register. Segment register is a 64-bits register which can
// be used as a base address for memory access. Each thread has a unique
// FS value, and they access their thread-local variables relative to FS
// as %fs:offset_from_tp.
//
// The value of a segment register itself is not generally readable from
// the user space. As a workaround, libc initializes %fs:0 (the first word
// referenced by FS) to the value of %fs itself. So we can obtain TP just
// by `mov %fs:0, %rax` if we need it.
//
// For historical reasons, TP points past the end of the TLS block on x86.
// This is contrary to other psABIs which usually use the beginning of the
// TLS block as TP (with some addend). As a result, offsets from TP to
// thread-local variables (TLVs) in the main executable are all negative.
//
// https://gitlab.com/x86-psABIs/x86-64-ABI

#if MOLD_X86_64

#include "mold.h"
#include <tbb/parallel_for_each.h>

namespace mold {

using E = X86_64;

// This is a security-enhanced version of the regular PLT. The PLT
// header and each PLT entry starts with endbr64 for the Intel's
// control-flow enforcement security mechanism.
//
// Note that our IBT-enabled PLT instruction sequence is different
// from the one used in GNU ld. GNU's IBTPLT implementation uses two
// separate sections (.plt and .plt.sec) in which one PLT entry takes
// 32 bytes in total. Our IBTPLT consists of just .plt and each entry
// is 16 bytes long.
//
// Our PLT entry clobbers %r11, but that's fine because the resolver
// function (_dl_runtime_resolve) clobbers %r11 anyway.
template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static const u8 insn[] = {
    0xf3, 0x0f, 0x1e, 0xfa, // endbr64
    0x41, 0x53,             // push %r11
    0xff, 0x35, 0, 0, 0, 0, // push GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
    0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    0xcc, 0xcc,             // (padding)
  };

  memcpy(buf, insn, sizeof(insn));
  *(ul32 *)(buf + 8) = ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 4;
  *(ul32 *)(buf + 14) = ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 2;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  // Only a canonical PLT can be address-taken; there's no way to take
  // an address of a non-canonical PLT. Therefore, a non-canonical PLT
  // doesn't have to start with an endbr64.
  if (sym.is_canonical) {
    static const u8 insn[] = {
      0xf3, 0x0f, 0x1e, 0xfa, // endbr64
      0x41, 0xbb, 0, 0, 0, 0, // mov $index_in_relplt, %r11d
      0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOTPLT
    };

    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 6) = sym.get_plt_idx(ctx);
    *(ul32 *)(buf + 12) = sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx) - 16;
  } else {
    static const u8 insn[] = {
      0x41, 0xbb, 0, 0, 0, 0, // mov $index_in_relplt, %r11d
      0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOTPLT
      0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    };

    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 2) = sym.get_plt_idx(ctx);
    *(ul32 *)(buf + 8) = sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx) - 12;
  }
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const u8 insn[] = {
    0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
    0xcc, 0xcc,             // (padding)
  };

  memcpy(buf, insn, sizeof(insn));
  *(ul32 *)(buf + 2) = sym.get_got_pltgot_addr(ctx) - sym.get_plt_addr(ctx) - 6;
}

template <>
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                       u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_X86_64_32:
    *(ul32 *)loc = val;
    break;
  case R_X86_64_64:
    *(ul64 *)loc = val;
    break;
  case R_X86_64_PC32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  case R_X86_64_PC64:
    *(ul64 *)loc = val - this->shdr.sh_addr - offset;
    break;
  default:
    Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
  }
}

static u32 relax_gotpcrelx(u8 *loc, const ElfRel<E> &rel) {
  if (rel.r_type == R_X86_64_GOTPCRELX) {
    switch ((loc[-2] << 8) | loc[-1]) {
    case 0xff15: return 0x40e8; // call *0(%rip) -> call 0
    case 0xff25: return 0x40e9; // jmp  *0(%rip) -> jmp  0
    }
  } else {
    assert(rel.r_type == R_X86_64_REX_GOTPCRELX ||
           rel.r_type == R_X86_64_CODE_4_GOTPCRELX);
    switch ((loc[-3] << 16) | (loc[-2] << 8) | loc[-1]) {
    case 0x488b05: return 0x8d05; // mov 0(%rip), %rax -> lea 0(%rip), %rax
    case 0x488b0d: return 0x8d0d; // mov 0(%rip), %rcx -> lea 0(%rip), %rcx
    case 0x488b15: return 0x8d15; // mov 0(%rip), %rdx -> lea 0(%rip), %rdx
    case 0x488b1d: return 0x8d1d; // mov 0(%rip), %rbx -> lea 0(%rip), %rbx
    case 0x488b25: return 0x8d25; // mov 0(%rip), %rsp -> lea 0(%rip), %rsp
    case 0x488b2d: return 0x8d2d; // mov 0(%rip), %rbp -> lea 0(%rip), %rbp
    case 0x488b35: return 0x8d35; // mov 0(%rip), %rsi -> lea 0(%rip), %rsi
    case 0x488b3d: return 0x8d3d; // mov 0(%rip), %rdi -> lea 0(%rip), %rdi
    case 0x4c8b05: return 0x8d05; // mov 0(%rip), %r8  -> lea 0(%rip), %r8
    case 0x4c8b0d: return 0x8d0d; // mov 0(%rip), %r9  -> lea 0(%rip), %r9
    case 0x4c8b15: return 0x8d15; // mov 0(%rip), %r10 -> lea 0(%rip), %r10
    case 0x4c8b1d: return 0x8d1d; // mov 0(%rip), %r11 -> lea 0(%rip), %r11
    case 0x4c8b25: return 0x8d25; // mov 0(%rip), %r12 -> lea 0(%rip), %r12
    case 0x4c8b2d: return 0x8d2d; // mov 0(%rip), %r13 -> lea 0(%rip), %r13
    case 0x4c8b35: return 0x8d35; // mov 0(%rip), %r14 -> lea 0(%rip), %r14
    case 0x4c8b3d: return 0x8d3d; // mov 0(%rip), %r15 -> lea 0(%rip), %r15
    }
  }
  return 0;
}

static u32 relax_gottpoff(u8 *loc, const ElfRel<E> &rel) {
  if (rel.r_type == R_X86_64_GOTTPOFF) {
    switch ((loc[-3] << 16) | (loc[-2] << 8) | loc[-1]) {
    case 0x488b05: return 0x48c7c0; // mov 0(%rip), %rax -> mov $0, %rax
    case 0x488b0d: return 0x48c7c1; // mov 0(%rip), %rcx -> mov $0, %rcx
    case 0x488b15: return 0x48c7c2; // mov 0(%rip), %rdx -> mov $0, %rdx
    case 0x488b1d: return 0x48c7c3; // mov 0(%rip), %rbx -> mov $0, %rbx
    case 0x488b25: return 0x48c7c4; // mov 0(%rip), %rsp -> mov $0, %rsp
    case 0x488b2d: return 0x48c7c5; // mov 0(%rip), %rbp -> mov $0, %rbp
    case 0x488b35: return 0x48c7c6; // mov 0(%rip), %rsi -> mov $0, %rsi
    case 0x488b3d: return 0x48c7c7; // mov 0(%rip), %rdi -> mov $0, %rdi
    case 0x4c8b05: return 0x49c7c0; // mov 0(%rip), %r8  -> mov $0, %r8
    case 0x4c8b0d: return 0x49c7c1; // mov 0(%rip), %r9  -> mov $0, %r9
    case 0x4c8b15: return 0x49c7c2; // mov 0(%rip), %r10 -> mov $0, %r10
    case 0x4c8b1d: return 0x49c7c3; // mov 0(%rip), %r11 -> mov $0, %r11
    case 0x4c8b25: return 0x49c7c4; // mov 0(%rip), %r12 -> mov $0, %r12
    case 0x4c8b2d: return 0x49c7c5; // mov 0(%rip), %r13 -> mov $0, %r13
    case 0x4c8b35: return 0x49c7c6; // mov 0(%rip), %r14 -> mov $0, %r14
    case 0x4c8b3d: return 0x49c7c7; // mov 0(%rip), %r15 -> mov $0, %r15
    }
  } else {
    assert(rel.r_type == R_X86_64_CODE_4_GOTTPOFF);
    switch ((loc[-3] << 16) | (loc[-2] << 8) | loc[-1]) {
    case 0x488b05: return 0x18c7c0; // mov 0(%rip), %r16 -> mov $0, %r16
    case 0x488b0d: return 0x18c7c1; // mov 0(%rip), %r17 -> mov $0, %r17
    case 0x488b15: return 0x18c7c2; // mov 0(%rip), %r18 -> mov $0, %r18
    case 0x488b1d: return 0x18c7c3; // mov 0(%rip), %r19 -> mov $0, %r19
    case 0x488b25: return 0x18c7c4; // mov 0(%rip), %r20 -> mov $0, %r20
    case 0x488b2d: return 0x18c7c5; // mov 0(%rip), %r21 -> mov $0, %r21
    case 0x488b35: return 0x18c7c6; // mov 0(%rip), %r22 -> mov $0, %r22
    case 0x488b3d: return 0x18c7c7; // mov 0(%rip), %r23 -> mov $0, %r23
    case 0x4c8b05: return 0x19c7c0; // mov 0(%rip), %r24 -> mov $0, %r24
    case 0x4c8b0d: return 0x19c7c1; // mov 0(%rip), %r25 -> mov $0, %r25
    case 0x4c8b15: return 0x19c7c2; // mov 0(%rip), %r26 -> mov $0, %r26
    case 0x4c8b1d: return 0x19c7c3; // mov 0(%rip), %r27 -> mov $0, %r27
    case 0x4c8b25: return 0x19c7c4; // mov 0(%rip), %r28 -> mov $0, %r28
    case 0x4c8b2d: return 0x19c7c5; // mov 0(%rip), %r29 -> mov $0, %r29
    case 0x4c8b35: return 0x19c7c6; // mov 0(%rip), %r30 -> mov $0, %r30
    case 0x4c8b3d: return 0x19c7c7; // mov 0(%rip), %r31 -> mov $0, %r31
    }
  }
  return 0;
}

static u32 relax_tlsdesc_to_ie(u8 *loc, const ElfRel<E> &rel) {
  if (rel.r_type == R_X86_64_GOTPC32_TLSDESC) {
    switch ((loc[-3] << 16) | (loc[-2] << 8) | loc[-1]) {
    case 0x488d05: return 0x488b05; // lea 0(%rip), %rax -> mov 0(%rip), %rax
    case 0x488d0d: return 0x488b0d; // lea 0(%rip), %rcx -> mov 0(%rip), %rcx
    case 0x488d15: return 0x488b15; // lea 0(%rip), %rdx -> mov 0(%rip), %rdx
    case 0x488d1d: return 0x488b1d; // lea 0(%rip), %rbx -> mov 0(%rip), %rbx
    case 0x488d25: return 0x488b25; // lea 0(%rip), %rsp -> mov 0(%rip), %rsp
    case 0x488d2d: return 0x488b2d; // lea 0(%rip), %rbp -> mov 0(%rip), %rbp
    case 0x488d35: return 0x488b35; // lea 0(%rip), %rsi -> mov 0(%rip), %rsi
    case 0x488d3d: return 0x488b3d; // lea 0(%rip), %rdi -> mov 0(%rip), %rdi
    case 0x4c8d05: return 0x4c8b05; // lea 0(%rip), %r8  -> mov 0(%rip), %r8
    case 0x4c8d0d: return 0x4c8b0d; // lea 0(%rip), %r9  -> mov 0(%rip), %r9
    case 0x4c8d15: return 0x4c8b15; // lea 0(%rip), %r10 -> mov 0(%rip), %r10
    case 0x4c8d1d: return 0x4c8b1d; // lea 0(%rip), %r11 -> mov 0(%rip), %r11
    case 0x4c8d25: return 0x4c8b25; // lea 0(%rip), %r12 -> mov 0(%rip), %r12
    case 0x4c8d2d: return 0x4c8b2d; // lea 0(%rip), %r13 -> mov 0(%rip), %r13
    case 0x4c8d35: return 0x4c8b35; // lea 0(%rip), %r14 -> mov 0(%rip), %r14
    case 0x4c8d3d: return 0x4c8b3d; // lea 0(%rip), %r15 -> mov 0(%rip), %r15
    }
  } else {
    assert(rel.r_type == R_X86_64_CODE_4_GOTPC32_TLSDESC);
    switch ((loc[-3] << 16) | (loc[-2] << 8) | loc[-1]) {
    case 0x488d05: return 0x488b05; // lea 0(%rip), %r16 -> mov 0(%rip), %r16
    case 0x488d0d: return 0x488b0d; // lea 0(%rip), %r17 -> mov 0(%rip), %r17
    case 0x488d15: return 0x488b15; // lea 0(%rip), %r18 -> mov 0(%rip), %r18
    case 0x488d1d: return 0x488b1d; // lea 0(%rip), %r19 -> mov 0(%rip), %r19
    case 0x488d25: return 0x488b25; // lea 0(%rip), %r20 -> mov 0(%rip), %r20
    case 0x488d2d: return 0x488b2d; // lea 0(%rip), %r21 -> mov 0(%rip), %r21
    case 0x488d35: return 0x488b35; // lea 0(%rip), %r22 -> mov 0(%rip), %r22
    case 0x488d3d: return 0x488b3d; // lea 0(%rip), %r23 -> mov 0(%rip), %r23
    case 0x4c8d05: return 0x4c8b05; // lea 0(%rip), %r24 -> mov 0(%rip), %r24
    case 0x4c8d0d: return 0x4c8b0d; // lea 0(%rip), %r25 -> mov 0(%rip), %r25
    case 0x4c8d15: return 0x4c8b15; // lea 0(%rip), %r26 -> mov 0(%rip), %r26
    case 0x4c8d1d: return 0x4c8b1d; // lea 0(%rip), %r27 -> mov 0(%rip), %r27
    case 0x4c8d25: return 0x4c8b25; // lea 0(%rip), %r28 -> mov 0(%rip), %r28
    case 0x4c8d2d: return 0x4c8b2d; // lea 0(%rip), %r29 -> mov 0(%rip), %r29
    case 0x4c8d35: return 0x4c8b35; // lea 0(%rip), %r30 -> mov 0(%rip), %r30
    case 0x4c8d3d: return 0x4c8b3d; // lea 0(%rip), %r31 -> mov 0(%rip), %r31
    }
  }
  return 0;
}

static u32 relax_tlsdesc_to_le(u8 *loc, const ElfRel<E> &rel) {
  if (rel.r_type == R_X86_64_GOTPC32_TLSDESC) {
    switch ((loc[-3] << 16) | (loc[-2] << 8) | loc[-1]) {
    case 0x488d05: return 0x48c7c0; // lea 0(%rip), %rax -> mov $0, %rax
    case 0x488d0d: return 0x48c7c1; // lea 0(%rip), %rcx -> mov $0, %rcx
    case 0x488d15: return 0x48c7c2; // lea 0(%rip), %rdx -> mov $0, %rdx
    case 0x488d1d: return 0x48c7c3; // lea 0(%rip), %rbx -> mov $0, %rbx
    case 0x488d25: return 0x48c7c4; // lea 0(%rip), %rsp -> mov $0, %rsp
    case 0x488d2d: return 0x48c7c5; // lea 0(%rip), %rbp -> mov $0, %rbp
    case 0x488d35: return 0x48c7c6; // lea 0(%rip), %rsi -> mov $0, %rsi
    case 0x488d3d: return 0x48c7c7; // lea 0(%rip), %rdi -> mov $0, %rdi
    case 0x4c8d05: return 0x49c7c0; // lea 0(%rip), %r8  -> mov $0, %r8
    case 0x4c8d0d: return 0x49c7c1; // lea 0(%rip), %r9  -> mov $0, %r9
    case 0x4c8d15: return 0x49c7c2; // lea 0(%rip), %r10 -> mov $0, %r10
    case 0x4c8d1d: return 0x49c7c3; // lea 0(%rip), %r11 -> mov $0, %r11
    case 0x4c8d25: return 0x49c7c4; // lea 0(%rip), %r12 -> mov $0, %r12
    case 0x4c8d2d: return 0x49c7c5; // lea 0(%rip), %r13 -> mov $0, %r13
    case 0x4c8d35: return 0x49c7c6; // lea 0(%rip), %r14 -> mov $0, %r14
    case 0x4c8d3d: return 0x49c7c7; // lea 0(%rip), %r15 -> mov $0, %r15
    }
  } else {
    assert(rel.r_type == R_X86_64_CODE_4_GOTPC32_TLSDESC);
    switch ((loc[-3] << 16) | (loc[-2] << 8) | loc[-1]) {
    case 0x488d05: return 0x18c7c0; // lea 0(%rip), %r16 -> mov $0, %r16
    case 0x488d0d: return 0x18c7c1; // lea 0(%rip), %r17 -> mov $0, %r17
    case 0x488d15: return 0x18c7c2; // lea 0(%rip), %r18 -> mov $0, %r18
    case 0x488d1d: return 0x18c7c3; // lea 0(%rip), %r19 -> mov $0, %r19
    case 0x488d25: return 0x18c7c4; // lea 0(%rip), %r20 -> mov $0, %r20
    case 0x488d2d: return 0x18c7c5; // lea 0(%rip), %r21 -> mov $0, %r21
    case 0x488d35: return 0x18c7c6; // lea 0(%rip), %r22 -> mov $0, %r22
    case 0x488d3d: return 0x18c7c7; // lea 0(%rip), %r23 -> mov $0, %r23
    case 0x4c8d05: return 0x19c7c0; // lea 0(%rip), %r24 -> mov $0, %r24
    case 0x4c8d0d: return 0x19c7c1; // lea 0(%rip), %r25 -> mov $0, %r25
    case 0x4c8d15: return 0x19c7c2; // lea 0(%rip), %r26 -> mov $0, %r26
    case 0x4c8d1d: return 0x19c7c3; // lea 0(%rip), %r27 -> mov $0, %r27
    case 0x4c8d25: return 0x19c7c4; // lea 0(%rip), %r28 -> mov $0, %r28
    case 0x4c8d2d: return 0x19c7c5; // lea 0(%rip), %r29 -> mov $0, %r29
    case 0x4c8d35: return 0x19c7c6; // lea 0(%rip), %r30 -> mov $0, %r30
    case 0x4c8d3d: return 0x19c7c7; // lea 0(%rip), %r31 -> mov $0, %r31
    }
  }
  return 0;
}

// Rewrite a function call to __tls_get_addr to a cheaper instruction
// sequence. We can do this when we know the thread-local variable's TP-
// relative address at link-time.
static void relax_gd_to_le(u8 *loc, const ElfRel<E> &rel, u64 val) {
  switch (rel.r_type) {
  case R_X86_64_PLT32:
  case R_X86_64_PC32:
  case R_X86_64_GOTPCREL:
  case R_X86_64_GOTPCRELX: {
    // The original instructions are the following:
    //
    //  66 48 8d 3d 00 00 00 00    lea  foo@tlsgd(%rip), %rdi
    //  66 66 48 e8 00 00 00 00    call __tls_get_addr
    //
    // or
    //
    //  66 48 8d 3d 00 00 00 00    lea foo@tlsgd(%rip), %rdi
    //  66 48 ff 15 00 00 00 00    call *__tls_get_addr@GOT(%rip)
    static const u8 insn[] = {
      0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
      0x48, 0x81, 0xc0, 0, 0, 0, 0,             // add $tp_offset, %rax
    };
    memcpy(loc - 4, insn, sizeof(insn));
    *(ul32 *)(loc + 8) = val;
    break;
  }
  case R_X86_64_PLTOFF64: {
    // The original instructions are the following:
    //
    //  48 8d 3d 00 00 00 00           lea    foo@tlsgd(%rip), %rdi
    //  48 b8 00 00 00 00 00 00 00 00  movabs __tls_get_addr, %rax
    //  48 01 d8                       add    %rbx, %rax
    //  ff d0                          call   *%rax
    static const u8 insn[] = {
      0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
      0x48, 0x81, 0xc0, 0, 0, 0, 0,             // add $tp_offset, %rax
      0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00,       // nop
    };
    memcpy(loc - 3, insn, sizeof(insn));
    *(ul32 *)(loc + 9) = val;
    break;
  }
  default:
    unreachable();
  }
}

static void relax_gd_to_ie(u8 *loc, const ElfRel<E> &rel, u64 val) {
  switch (rel.r_type) {
  case R_X86_64_PLT32:
  case R_X86_64_PC32:
  case R_X86_64_GOTPCREL:
  case R_X86_64_GOTPCRELX: {
    static const u8 insn[] = {
      0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
      0x48, 0x03, 0x05, 0, 0, 0, 0,             // add foo@gottpoff(%rip), %rax
    };
    memcpy(loc - 4, insn, sizeof(insn));
    *(ul32 *)(loc + 8) = val - 12;
    break;
  }
  case R_X86_64_PLTOFF64: {
    static const u8 insn[] = {
      0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
      0x48, 0x03, 0x05, 0, 0, 0, 0,             // add foo@gottpoff(%rip), %rax
      0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00,       // nop
    };
    memcpy(loc - 3, insn, sizeof(insn));
    *(ul32 *)(loc + 9) = val - 13;
    break;
  }
  default:
    unreachable();
  }
}

// Rewrite a function call to __tls_get_addr to a cheaper instruction
// sequence. The difference from relax_gd_to_le is that we are materializing
// the address of the beginning of TLS block instead of an address of a
// particular thread-local variable.
static void relax_ld_to_le(u8 *loc, const ElfRel<E> &rel, i64 tls_size) {
  switch (rel.r_type) {
  case R_X86_64_PLT32:
  case R_X86_64_PC32: {
    // The original instructions are the following:
    //
    //  48 8d 3d 00 00 00 00    lea    foo@tlsld(%rip), %rdi
    //  e8 00 00 00 00          call   __tls_get_addr
    //
    // Because the original instruction sequence is so short that we need a
    // little bit of code golfing here. "mov %fs:0, %rax" is 9 byte long, so
    // xor + mov is shorter. Note that `xor %eax, %eax` zero-clears %eax.
    static const u8 insn[] = {
      0x31, 0xc0,                   // xor %eax, %eax
      0x64, 0x48, 0x8b, 0x00,       // mov %fs:(%rax), %rax
      0x48, 0x2d, 0, 0, 0, 0,       // sub $tls_size, %rax
    };
    memcpy(loc - 3, insn, sizeof(insn));
    *(ul32 *)(loc + 5) = tls_size;
    break;
  }
  case R_X86_64_GOTPCREL:
  case R_X86_64_GOTPCRELX: {
    // The original instructions are the following:
    //
    //  48 8d 3d 00 00 00 00    lea    foo@tlsld(%rip), %rdi
    //  ff 15 00 00 00 00       call   *__tls_get_addr@GOT(%rip)
    static const u8 insn[] = {
      0x48, 0x31, 0xc0,             // xor %rax, %rax
      0x64, 0x48, 0x8b, 0x00,       // mov %fs:(%rax), %rax
      0x48, 0x2d, 0, 0, 0, 0,       // sub $tls_size, %rax
    };
    memcpy(loc - 3, insn, sizeof(insn));
    *(ul32 *)(loc + 6) = tls_size;
    break;
  }
  case R_X86_64_PLTOFF64: {
    // The original instructions are the following:
    //
    //  48 8d 3d 00 00 00 00           lea    foo@tlsld(%rip), %rdi
    //  48 b8 00 00 00 00 00 00 00 00  movabs __tls_get_addr@GOTOFF, %rax
    //  48 01 d8                       add    %rbx, %rax
    //  ff d0                          call   *%rax
    static const u8 insn[] = {
      0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
      0x48, 0x2d, 0, 0, 0, 0,                   // sub $tls_size, %rax
      0x0f, 0x1f, 0x80, 0x00, 0x00, 0x00, 0x00, // nop
    };
    memcpy(loc - 3, insn, sizeof(insn));
    *(ul32 *)(loc + 8) = tls_size;
    break;
  }
  default:
    unreachable();
  }
}

// Apply relocations to SHF_ALLOC sections (i.e. sections that are
// mapped to memory at runtime) based on the result of
// scan_relocations().
template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);
  RelocationsStats rels_stats;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + rel.r_offset;
    u64 G = sym.get_got_addr(ctx) - ctx.gotplt->shdr.sh_addr;
    u64 GOT = ctx.gotplt->shdr.sh_addr;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (ctx.arg.stats)
        update_relocation_stats(rels_stats, i, val, lo, hi);
      check_range(ctx, i, val, lo, hi);
    };

    auto write32 = [&](u64 val) {
      check(val, 0, 1LL << 32);
      *(ul32 *)loc = val;
    };

    auto write32s = [&](u64 val) {
      check(val, -(1LL << 31), 1LL << 31);
      *(ul32 *)loc = val;
    };

    switch (rel.r_type) {
    case R_X86_64_8:
      check(S + A, 0, 1 << 8);
      *loc = S + A;
      break;
    case R_X86_64_16:
      check(S + A, 0, 1 << 16);
      *(ul16 *)loc = S + A;
      break;
    case R_X86_64_32:
      write32(S + A);
      break;
    case R_X86_64_32S:
      write32s(S + A);
      break;
    case R_X86_64_64:
      break;
    case R_X86_64_PC8:
      check(S + A - P, -(1 << 7), 1 << 7);
      *loc = S + A - P;
      break;
    case R_X86_64_PC16:
      check(S + A - P, -(1 << 15), 1 << 15);
      *(ul16 *)loc = S + A - P;
      break;
    case R_X86_64_PC32:
    case R_X86_64_PLT32:
      write32s(S + A - P);
      break;
    case R_X86_64_PC64:
      *(ul64 *)loc = S + A - P;
      break;
    case R_X86_64_GOT32:
      write32(G + A);
      break;
    case R_X86_64_GOT64:
      *(ul64 *)loc = G + A;
      break;
    case R_X86_64_GOTOFF64:
    case R_X86_64_PLTOFF64:
      *(ul64 *)loc = S + A - GOT;
      break;
    case R_X86_64_GOTPC32:
      write32s(GOT + A - P);
      break;
    case R_X86_64_GOTPC64:
      *(ul64 *)loc = GOT + A - P;
      break;
    case R_X86_64_GOTPCREL:
      write32s(G + GOT + A - P);
      break;
    case R_X86_64_GOTPCREL64:
      *(ul64 *)loc = G + GOT + A - P;
      break;
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
    case R_X86_64_CODE_4_GOTPCRELX:
      // We always want to relax GOTPCRELX relocs even if --no-relax
      // was given because some static PIE runtime code depends on these
      // relaxations.
      if (sym.is_pcrel_linktime_const(ctx) && is_int(S + A - P, 32)) {
        if (u32 insn = relax_gotpcrelx(loc, rel)) {
          loc[-2] = insn >> 8;
          loc[-1] = insn;
          *(ul32 *)loc = S + A - P;
          break;
        }
      }
      write32s(G + GOT + A - P);
      break;
    case R_X86_64_TLSGD:
      if (sym.has_tlsgd(ctx))
        write32s(sym.get_tlsgd_addr(ctx) + A - P);
      else if (sym.has_gottp(ctx))
        relax_gd_to_ie(loc, rels[++i], sym.get_gottp_addr(ctx) - P);
      else
        relax_gd_to_le(loc, rels[++i], S - ctx.tp_addr);
      break;
    case R_X86_64_TLSLD:
      if (ctx.got->has_tlsld(ctx))
        write32s(ctx.got->get_tlsld_addr(ctx) + A - P);
      else
        relax_ld_to_le(loc, rels[++i], ctx.tp_addr - ctx.tls_begin);
      break;
    case R_X86_64_DTPOFF32:
      write32s(S + A - ctx.dtp_addr);
      break;
    case R_X86_64_DTPOFF64:
      *(ul64 *)loc = S + A - ctx.dtp_addr;
      break;
    case R_X86_64_TPOFF32:
      write32s(S + A - ctx.tp_addr);
      break;
    case R_X86_64_TPOFF64:
      *(ul64 *)loc = S + A - ctx.tp_addr;
      break;
    case R_X86_64_GOTTPOFF:
    case R_X86_64_CODE_4_GOTTPOFF:
      if (sym.has_gottp(ctx)) {
        write32s(sym.get_gottp_addr(ctx) + A - P);
      } else {
        u32 insn = relax_gottpoff(loc, rel);
        loc[-3] = insn >> 16;
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        write32s(S - ctx.tp_addr);
      }
      break;
    case R_X86_64_CODE_6_GOTTPOFF:
      write32s(sym.get_gottp_addr(ctx) + A - P);
      break;
    case R_X86_64_GOTPC32_TLSDESC:
    case R_X86_64_CODE_4_GOTPC32_TLSDESC:
      // x86-64 TLSDESC uses the following code sequence to materialize
      // a TP-relative address in %rax.
      //
      //   lea    0(%rip), %rax
      //       R_X86_64_GOTPC32_TLSDESC    foo
      //   call   *(%rax)
      //       R_X86_64_TLSDESC_CALL       foo
      //
      // We may relax the instructions to the following if its TP-relative
      // address is known at link-time
      //
      //   mov     $foo@TPOFF, %rax
      //   nop
      //
      // or to the following if the TP-relative address is known at
      // process startup time.
      //
      //   mov     foo@GOTTPOFF(%rip), %rax
      //   nop
      //
      // We allow the following alternative code sequence too because
      // LLVM emits such code.
      //
      //   lea    0(%rip), %reg
      //       R_X86_64_GOTPC32_TLSDESC    foo
      //   mov    %reg, %rax
      //   call   *(%rax)
      //       R_X86_64_TLSDESC_CALL       foo
      if (sym.has_tlsdesc(ctx)) {
        write32s(sym.get_tlsdesc_addr(ctx) + A - P);
      } else if (sym.has_gottp(ctx)) {
        u32 insn = relax_tlsdesc_to_ie(loc, rel);
        if (!insn)
          Fatal(ctx) << *this << ": illegal instruction sequence for " << rel;
        loc[-3] = insn >> 16;
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        write32s(sym.get_gottp_addr(ctx) + A - P);
      } else {
        u32 insn = relax_tlsdesc_to_le(loc, rel);
        if (!insn)
          Fatal(ctx) << *this << ": illegal instruction sequence for " << rel;
        loc[-3] = insn >> 16;
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        write32s(S - ctx.tp_addr);
      }
      break;
    case R_X86_64_TLSDESC_CALL:
      if (!sym.has_tlsdesc(ctx)) {
        // call *(%rax) -> nop
        loc[0] = 0x66;
        loc[1] = 0x90;
      }
      break;
    case R_X86_64_SIZE32:
      write32(sym.esym().st_size + A);
      break;
    case R_X86_64_SIZE64:
      *(ul64 *)loc = sym.esym().st_size + A;
      break;
    default:
      unreachable();
    }
  }
  if (ctx.arg.stats)
    save_relocation_stats<E>(ctx, *this, rels_stats);
}

// This function is responsible for applying relocations against
// non-SHF_ALLOC sections (i.e. sections that are not mapped to memory
// at runtime).
//
// Relocations against non-SHF_ALLOC sections are much easier to
// handle than that against SHF_ALLOC sections. It is because, since
// they are not mapped to memory, they don't contain any variable or
// function and never need PLT or GOT. Non-SHF_ALLOC sections are
// mostly debug info sections.
//
// Relocations against non-SHF_ALLOC sections are not scanned by
// scan_relocations.
template <>
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);
  RelocationsStats rels_stats;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

    u64 S = frag ? frag->get_addr(ctx) : sym.get_addr(ctx);
    u64 A = frag ? frag_addend : (i64)rel.r_addend;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (ctx.arg.stats)
        update_relocation_stats(rels_stats, i, val, lo, hi);
      check_range(ctx, i, val, lo, hi);
    };

    auto write32 = [&](u64 val) {
      check(val, 0, 1LL << 32);
      *(ul32 *)loc = val;
    };

    auto write32s = [&](u64 val) {
      check(val, -(1LL << 31), 1LL << 31);
      *(ul32 *)loc = val;
    };

    switch (rel.r_type) {
    case R_X86_64_8:
      check(S + A, 0, 1 << 8);
      *loc = S + A;
      break;
    case R_X86_64_16:
      check(S + A, 0, 1 << 16);
      *(ul16 *)loc = S + A;
      break;
    case R_X86_64_32:
      write32(S + A);
      break;
    case R_X86_64_32S:
      write32s(S + A);
      break;
    case R_X86_64_64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A;
      break;
    case R_X86_64_DTPOFF32:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul32 *)loc = *val;
      else
        write32s(S + A - ctx.dtp_addr);
      break;
    case R_X86_64_DTPOFF64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A - ctx.dtp_addr;
      break;
    case R_X86_64_GOTOFF64:
      *(ul64 *)loc = S + A - ctx.gotplt->shdr.sh_addr;
      break;
    case R_X86_64_GOTPC64:
      // PC-relative relocation doesn't make sense for non-memory-allocated
      // section, but GCC 6.3.0 seems to create this reloc for
      // _GLOBAL_OFFSET_TABLE_.
      *(ul64 *)loc = ctx.gotplt->shdr.sh_addr + A;
      break;
    case R_X86_64_SIZE32:
      write32(sym.esym().st_size + A);
      break;
    case R_X86_64_SIZE64:
      *(ul64 *)loc = sym.esym().st_size + A;
      break;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
      break;
    }
  }
  if (ctx.arg.stats)
    save_relocation_stats<E>(ctx, *this, rels_stats);
}

// Linker has to create data structures in an output file to apply
// some type of relocations. For example, if a relocation refers a GOT
// or a PLT entry of a symbol, linker has to create an entry in .got
// or in .plt for that symbol. In order to fix the file layout, we
// need to scan relocations.
template <>
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (sym.is_ifunc())
      sym.flags |= NEEDS_GOT | NEEDS_PLT;

    if (rel.r_type == R_X86_64_TLSGD || rel.r_type == R_X86_64_TLSLD) {
      if (i + 1 == rels.size())
        Fatal(ctx) << *this << ": " << rel
                   << " must be followed by PLT or GOTPCREL";

      if (u32 ty = rels[i + 1].r_type;
          ty != R_X86_64_PLT32 && ty != R_X86_64_PC32 &&
          ty != R_X86_64_PLTOFF64 && ty != R_X86_64_GOTPCREL &&
          ty != R_X86_64_GOTPCRELX)
        Fatal(ctx) << *this << ": " << rel
                   << " must be followed by PLT or GOTPCREL";
    }

    switch (rel.r_type) {
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S:
      scan_absrel(ctx, sym, rel);
      break;
    case R_X86_64_PC8:
    case R_X86_64_PC16:
    case R_X86_64_PC32:
    case R_X86_64_PC64:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_X86_64_GOT32:
    case R_X86_64_GOT64:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPC64:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCREL64:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
    case R_X86_64_CODE_4_GOTPCRELX:
      sym.flags |= NEEDS_GOT;
      break;
    case R_X86_64_PLT32:
    case R_X86_64_PLTOFF64:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_X86_64_TLSGD:
      if (ctx.arg.static_ || (ctx.arg.relax && sym.is_tprel_linktime_const(ctx))) {
        // We always relax if -static because libc.a doesn't contain
        // __tls_get_addr().
        i++;
      } else if (ctx.arg.relax && sym.is_tprel_runtime_const(ctx)) {
        sym.flags |= NEEDS_GOTTP;
        i++;
      } else {
        sym.flags |= NEEDS_TLSGD;
      }
      break;
    case R_X86_64_TLSLD:
      // We always relax if -static because libc.a doesn't contain
      // __tls_get_addr().
      if (ctx.arg.static_ || (ctx.arg.relax && !ctx.arg.shared))
        i++;
      else
        ctx.needs_tlsld = true;
      break;
    case R_X86_64_GOTTPOFF:
    case R_X86_64_CODE_4_GOTTPOFF:
      if (!ctx.arg.relax || !sym.is_tprel_linktime_const(ctx) ||
          !relax_gottpoff(loc, rel))
        sym.flags |= NEEDS_GOTTP;
      break;
    case R_X86_64_CODE_6_GOTTPOFF:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_X86_64_TLSDESC_CALL:
      scan_tlsdesc(ctx, sym);
      break;
    case R_X86_64_TPOFF32:
    case R_X86_64_TPOFF64:
      check_tlsle(ctx, sym, rel);
      break;
    case R_X86_64_64:
    case R_X86_64_GOTOFF64:
    case R_X86_64_DTPOFF32:
    case R_X86_64_DTPOFF64:
    case R_X86_64_SIZE32:
    case R_X86_64_SIZE64:
    case R_X86_64_GOTPC32_TLSDESC:
    case R_X86_64_CODE_4_GOTPC32_TLSDESC:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

// Intel CET is a relatively new CPU feature to enhance security by
// protecting control flow integrity. If the feature is enabled, indirect
// branches (i.e. branch instructions that take a register instead of an
// immediate) must land on a "landing pad" instruction, or a CPU-level fault
// will raise. That prevents an attacker to branch to a middle of a random
// function, making ROP or JOP much harder to conduct.
//
// On x86-64, the landing pad instruction is ENDBR64. That is actually a
// repurposed NOP instruction to provide binary compatibility with older
// hardware that doesn't support CET.
//
// The problem here is that the compiler always emits a landing pad at the
// beginning fo a global function because it doesn't know whether or not the
// function's address is taken in other translation units. As a result, the
// resulting binary contains more landing pads than necessary.
//
// This function rewrites a landing pad with a nop if the function's address
// was not actually taken. We can do what the compiler cannot because we
// know about all translation units.
void rewrite_endbr(Context<E> &ctx) {
  Timer t(ctx, "rewrite_endbr");

  constexpr u8 endbr64[] = {0xf3, 0x0f, 0x1e, 0xfa};
  constexpr u8 nop[] = {0x0f, 0x1f, 0x40, 0x00};

  // Rewrite all endbr64 instructions referred to by function symbols with
  // NOPs. We handle only global symbols because the compiler doesn't emit
  // an endbr64 for a file-scoped function in the first place if its address
  // is not taken within the file.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->get_global_syms()) {
      if (sym->file == file && sym->esym().st_type == STT_FUNC) {
        if (InputSection<E> *isec = sym->get_input_section();
            isec && (isec->shdr().sh_flags & SHF_EXECINSTR)) {
          if (OutputSection<E> *osec = isec->output_section) {
            u8 *buf = ctx.buf + osec->shdr.sh_offset + isec->offset + sym->value;
            if (memcmp(buf, endbr64, 4) == 0)
              memcpy(buf, nop, 4);
          }
        }
      }
    }
  });

  auto write_back = [&](InputSection<E> *isec, i64 offset) {
    // If isec has an endbr64 at a given offset, copy that instruction to
    // the output buffer, possibly overwriting a nop written in the above
    // loop.
    if (isec && isec->output_section &&
        (isec->shdr().sh_flags & SHF_EXECINSTR) &&
        0 <= offset && offset <= isec->contents.size() - 4 &&
        memcmp(isec->contents.data() + offset, endbr64, 4) == 0)
      memcpy(ctx.buf + isec->output_section->shdr.sh_offset + isec->offset + offset,
             endbr64, 4);
  };

  // Write back endbr64 instructions if they are referred to by address-taking
  // relocations.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (isec && isec->is_alive && (isec->shdr().sh_flags & SHF_ALLOC)) {
        for (const ElfRel<E> &rel : isec->get_rels(ctx)) {
          if (!is_func_call_rel(rel)) {
            Symbol<E> *sym = file->symbols[rel.r_sym];
            if (sym->esym().st_type == STT_SECTION)
              write_back(sym->get_input_section(), rel.r_addend);
            else
              write_back(sym->get_input_section(), sym->value);
          }
        }
      }
    }
  });

  // We record addresses of some symbols in the ELF header, .dynamic or in
  // .dynsym. We need to retain endbr64s for such symbols.
  auto keep = [&](Symbol<E> *sym) {
    if (sym)
      write_back(sym->get_input_section(), sym->value);
  };

  keep(ctx.arg.entry);
  keep(ctx.arg.init);
  keep(ctx.arg.fini);

  if (ctx.dynsym)
    for (Symbol<E> *sym : ctx.dynsym->symbols)
      if (sym && sym->is_exported)
        keep(sym);
}

} // namespace mold

#endif
