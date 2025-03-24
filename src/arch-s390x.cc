// This file contains code for the IBM z/Architecture 64-bit ISA, which is
// commonly referred to as "s390x" on Linux.
//
// z/Architecture is a 64-bit CISC ISA developed by IBM around 2000 for
// IBM's "big iron" mainframe computers. The computers are direct
// descendents of IBM System/360 all the way back in 1966. I've never
// actually seen a mainframe, and you probaly haven't either, but it looks
// like the mainframe market is still large enough to sustain its ecosystem.
// Ubuntu for example provides the official support for s390x as of 2022.
// Since they are being actively maintained, we need to support them.
//
// As an instruction set, s390x isn't particularly odd. It has 16 general-
// purpose registers. Instructions are 2, 4 or 6 bytes long and always
// aligned to 2 bytes boundaries. Despite unfamiliarty, I found that it
// just feels like an x86-64 in a parallel universe.
//
// Here is the register usage in this ABI:
//
//   r0-r1: reserved as scratch registers so we can use them in our PLT
//   r2:    parameter passing and return values
//   r3-r6: parameter passing
//   r12:   address of GOT if position-independent code
//   r14:   return address
//   r15:   stack pointer
//   a1:    upper 32 bits of TP (thread pointer)
//   a2:    lower 32 bits of TP (thread pointer)
//
// Thread-local storage (TLS) is supported on s390x in the same way as it
// is on other targets with one exeption. On other targets, __tls_get_addr
// is used to get an address of a thread-local variable. On s390x,
// __tls_get_offset is used instead. The difference is __tls_get_offset
// returns an address of a thread-local variable as an offset from TP. So
// we need to add TP to a return value before use. I don't know why it is
// different, but that is the way it is.
//
// https://github.com/IBM/s390x-abi/releases/download/v1.6.1/lzsabi_s390x.pdf

#include "mold.h"

namespace mold {

using E = S390X;

static void write_mid20(u8 *loc, u64 val) {
  *(ub32 *)loc |= (bits(val, 11, 0) << 16) | (bits(val, 19, 12) << 8);
}

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static u8 insn[] = {
    // Compute PLT_INDEX
    0xb9, 0x09, 0x00, 0x01,             // sgr   %r0, %r1
    0xa7, 0x0b, 0xff, 0xc2,             // aghi  %r0, -62
    0xeb, 0x10, 0x00, 0x01, 0x00, 0x0c, // srlg  %r1, %r0, 1
    0xb9, 0x08, 0x00, 0x01,             // agr   %r0, %r1
    0xe3, 0x00, 0xf0, 0x38, 0x00, 0x24, // stg   %r0, 56(%r15)
    // Branch to _dl_runtime_resolve
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOTPLT_OFFSET
    0xd2, 0x07, 0xf0, 0x30, 0x10, 0x08, // mvc   48(8, %r15), 8(%r1)
    0xe3, 0x10, 0x10, 0x10, 0x00, 0x04, // lg    %r1, 16(%r1)
    0x07, 0xf1,                         // br    %r1
    0x07, 0x00, 0x07, 0x00,             // nopr; nopr
  };

  memcpy(buf, insn, sizeof(insn));
  *(ub32 *)(buf + 26) =
    (ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 24) >> 1;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static u8 insn[] = {
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOTPLT_ENTRY_OFFSET
    0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, (%r1)
    0x0d, 0x01,                         // basr  %r0, %r1
    0x07, 0x00,                         // nopr
  };

  memcpy(buf, insn, sizeof(insn));
  *(ub32 *)(buf + 2) = (sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx)) >> 1;
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static u8 insn[] = {
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOT_ENTRY_OFFSET
    0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, (%r1)
    0x07, 0xf1,                         // br    %r1
    0x07, 0x00,                         // nopr
  };

  memcpy(buf, insn, sizeof(insn));
  *(ub32 *)(buf + 2) = (sym.get_got_pltgot_addr(ctx) - sym.get_plt_addr(ctx)) >> 1;
}

template <>
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                       u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_390_PC32:
    *(ub32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  case R_390_64:
    *(ub64 *)loc = val;
    break;
  default:
    Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
  }
}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + rel.r_offset;
    u64 G = sym.get_got_idx(ctx) * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      check_range(ctx, i, val, lo, hi);
    };

    auto check_dbl = [&](i64 val, i64 lo, i64 hi) {
      // R_390_*DBL relocs should never refer to a symbol at an odd address
      check(val, lo, hi);
      if (val & 1)
        Error(ctx) << *this << ": misaligned symbol " << sym
                   << " for relocation " << rel;
    };

    switch (rel.r_type) {
    case R_390_64:
      break;
    case R_390_8:
      check(S + A, 0, 1 << 8);
      *loc = S + A;
      break;
    case R_390_12:
      check(S + A, 0, 1 << 12);
      *(ul16 *)loc |= bits(S + A, 11, 0);
      break;
    case R_390_16:
      check(S + A, 0, 1 << 16);
      *(ub16 *)loc = S + A;
      break;
    case R_390_20:
      check(S + A, 0, 1 << 20);
      write_mid20(loc, S + A);
      break;
    case R_390_32:
    case R_390_PLT32:
      check(S + A, 0, 1LL << 32);
      *(ub32 *)loc = S + A;
      break;
    case R_390_PC12DBL:
    case R_390_PLT12DBL:
      check_dbl(S + A - P, -(1 << 12), 1 << 12);
      *(ul16 *)loc |= bits(S + A - P, 12, 1);
      break;
    case R_390_PC16:
      check(S + A - P, -(1 << 15), 1 << 15);
      *(ub16 *)loc = S + A - P;
      break;
    case R_390_PC32:
      check(S + A - P, -(1LL << 31), 1LL << 31);
      *(ub32 *)loc = S + A - P;
      break;
    case R_390_PC64:
    case R_390_PLT64:
      *(ub64 *)loc = S + A - P;
      break;
    case R_390_PC16DBL:
    case R_390_PLT16DBL:
      check_dbl(S + A - P, -(1 << 16), 1 << 16);
      *(ub16 *)loc = (S + A - P) >> 1;
      break;
    case R_390_PC24DBL:
    case R_390_PLT24DBL:
      check_dbl(S + A - P, -(1 << 24), 1 << 24);
      *(ub32 *)loc |= bits(S + A - P, 24, 1);
      break;
    case R_390_PC32DBL:
    case R_390_PLT32DBL:
      check_dbl(S + A - P, -(1LL << 32), 1LL << 32);
      *(ub32 *)loc = (S + A - P) >> 1;
      break;
    case R_390_GOT12:
    case R_390_GOTPLT12:
      check(G + A, 0, 1 << 12);
      *(ul16 *)loc |= bits(G + A, 11, 0);
      break;
    case R_390_GOT16:
    case R_390_GOTPLT16:
      check(G + A, 0, 1 << 16);
      *(ub16 *)loc = G + A;
      break;
    case R_390_GOT20:
    case R_390_GOTPLT20:
      check(G + A, 0, 1 << 20);
      write_mid20(loc, G + A);
      break;
    case R_390_GOT32:
    case R_390_GOTPLT32:
      check(G + A, 0, 1LL << 32);
      *(ub32 *)loc = G + A;
      break;
    case R_390_GOT64:
    case R_390_GOTPLT64:
      *(ub64 *)loc = G + A;
      break;
    case R_390_GOTOFF16:
    case R_390_PLTOFF16:
      check(S + A - GOT, -(1 << 15), 1 << 15);
      *(ub16 *)loc = S + A - GOT;
      break;
    case R_390_GOTOFF32:
    case R_390_PLTOFF32:
      check(S + A - GOT, -(1LL << 31), 1LL << 31);
      *(ub32 *)loc = S + A - GOT;
      break;
    case R_390_GOTOFF64:
    case R_390_PLTOFF64:
      *(ub64 *)loc = S + A - GOT;
      break;
    case R_390_GOTPC:
      *(ub64 *)loc = GOT + A - P;
      break;
    case R_390_GOTPCDBL:
      check_dbl(GOT + A - P, -(1LL << 32), 1LL << 32);
      *(ub32 *)loc = (GOT + A - P) >> 1;
      break;
    case R_390_GOTENT:
      // If we can relax a GOT-loading LGRL to an address-materializing
      // LARL, do that. The format of LGRL is 0xc 0x4 <reg> 0x8 followed
      // by a 32-bit offset. LARL is 0xc 0x0 <reg> 0x0.
      if (ctx.arg.relax && sym.is_pcrel_linktime_const(ctx)) {
        u64 op = *(ub16 *)(loc - 2);
        u64 val = S + A - P;
        if ((op & 0xff0f) == 0xc408 && A == 2 && (val & 1) == 0 &&
            is_int(val, 33)) {
          *(ub16 *)(loc - 2) = 0xc000 | (op & 0x00f0);
          *(ub32 *)loc = val >> 1;
          break;
        }
      }
      check_dbl(GOT + G + A - P, -(1LL << 32), 1LL << 32);
      *(ub32 *)loc = (GOT + G + A - P) >> 1;
      break;
    case R_390_TLS_LE32:
      *(ub32 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_LE64:
      *(ub64 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_GOTIE20:
      write_mid20(loc, sym.get_gottp_addr(ctx) + A - GOT);
      break;
    case R_390_TLS_IEENT:
      *(ub32 *)loc = (sym.get_gottp_addr(ctx) + A - P) >> 1;
      break;
    case R_390_TLS_GD32:
      if (sym.has_tlsgd(ctx))
        *(ub32 *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      else if (sym.has_gottp(ctx))
        *(ub32 *)loc = sym.get_gottp_addr(ctx) + A - GOT;
      else
        *(ub32 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_GD64:
      if (sym.has_tlsgd(ctx))
        *(ub64 *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      else if (sym.has_gottp(ctx))
        *(ub64 *)loc = sym.get_gottp_addr(ctx) + A - GOT;
      else
        *(ub64 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_GDCALL:
      if (sym.has_tlsgd(ctx)) {
        // do nothing
      } else if (sym.has_gottp(ctx)) {
        // lg %r2, 0(%r2, %r12)
        static u8 insn[] = { 0xe3, 0x22, 0xc0, 0x00, 0x00, 0x04 };
        memcpy(loc, insn, sizeof(insn));
      } else {
        // nop
        static u8 insn[] = { 0xc0, 0x04, 0x00, 0x00, 0x00, 0x00 };
        memcpy(loc, insn, sizeof(insn));
      }
      break;
    case R_390_TLS_LDM32:
      if (ctx.got->has_tlsld(ctx))
        *(ub32 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      else
        *(ub32 *)loc = ctx.dtp_addr - ctx.tp_addr;
      break;
    case R_390_TLS_LDM64:
      if (ctx.got->has_tlsld(ctx))
        *(ub64 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      else
        *(ub64 *)loc = ctx.dtp_addr - ctx.tp_addr;
      break;
    case R_390_TLS_LDCALL:
      if (!ctx.got->has_tlsld(ctx)) {
        // nop
        static u8 insn[] = { 0xc0, 0x04, 0x00, 0x00, 0x00, 0x00 };
        memcpy(loc, insn, sizeof(insn));
      }
      break;
    case R_390_TLS_LDO32:
      *(ub32 *)loc = S + A - ctx.dtp_addr;
      break;
    case R_390_TLS_LDO64:
      *(ub64 *)loc = S + A - ctx.dtp_addr;
      break;
    default:
      unreachable();
    }
  }
}

template <>
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

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
      check_range(ctx, val, i, lo, hi);
    };

    switch (rel.r_type) {
    case R_390_32:
      check(S + A, 0, 1LL << 32);
      *(ub32 *)loc = S + A;
      break;
    case R_390_64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ub64 *)loc = *val;
      else
        *(ub64 *)loc = S + A;
      break;
    case R_390_TLS_LDO64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ub64 *)loc = *val;
      else
        *(ub64 *)loc = S + A - ctx.dtp_addr;
      break;
    default:
      Fatal(ctx) << *this << ": apply_reloc_nonalloc: " << rel;
    }
  }
}

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

    if (sym.is_ifunc())
      sym.flags |= NEEDS_GOT | NEEDS_PLT;

    switch (rel.r_type) {
    case R_390_8:
    case R_390_12:
    case R_390_16:
    case R_390_20:
    case R_390_32:
      scan_absrel(ctx, sym, rel);
      break;
    case R_390_PC12DBL:
    case R_390_PC16:
    case R_390_PC16DBL:
    case R_390_PC24DBL:
    case R_390_PC32:
    case R_390_PC32DBL:
    case R_390_PC64:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_390_GOT12:
    case R_390_GOT16:
    case R_390_GOT20:
    case R_390_GOT32:
    case R_390_GOT64:
    case R_390_GOTOFF16:
    case R_390_GOTOFF32:
    case R_390_GOTOFF64:
    case R_390_GOTPLT12:
    case R_390_GOTPLT16:
    case R_390_GOTPLT20:
    case R_390_GOTPLT32:
    case R_390_GOTPLT64:
    case R_390_GOTPC:
    case R_390_GOTPCDBL:
    case R_390_GOTENT:
      sym.flags |= NEEDS_GOT;
      break;
    case R_390_PLT12DBL:
    case R_390_PLT16DBL:
    case R_390_PLT24DBL:
    case R_390_PLT32:
    case R_390_PLT32DBL:
    case R_390_PLT64:
    case R_390_PLTOFF16:
    case R_390_PLTOFF32:
    case R_390_PLTOFF64:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_390_TLS_GOTIE20:
    case R_390_TLS_IEENT:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_390_TLS_GD32:
    case R_390_TLS_GD64:
      // We always want to relax calls to __tls_get_offset() in statically-
      // linked executables because __tls_get_offset() in libc.a just calls
      // abort().
      if (ctx.arg.static_ || (ctx.arg.relax && sym.is_tprel_linktime_const(ctx))) {
        // Do nothing
      } else if (ctx.arg.relax && sym.is_tprel_runtime_const(ctx)) {
        sym.flags |= NEEDS_GOTTP;
      } else {
        sym.flags |= NEEDS_TLSGD;
      }
      break;
    case R_390_TLS_LDM32:
    case R_390_TLS_LDM64:
      if (ctx.arg.static_ || (ctx.arg.relax && !ctx.arg.shared)) {
        // Do nothing
      } else {
        ctx.needs_tlsld = true;
      }
      break;
    case R_390_TLS_LE32:
    case R_390_TLS_LE64:
      check_tlsle(ctx, sym, rel);
      break;
    case R_390_64:
    case R_390_TLS_LDO32:
    case R_390_TLS_LDO64:
    case R_390_TLS_GDCALL:
    case R_390_TLS_LDCALL:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

} // namespace mold
