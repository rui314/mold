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
// just feels like a 64-bit i386 in a parallel universe.
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
// TLS is supported on s390x in the same way as it is on other targets
// with one exeption. On other targets, __tls_get_addr is used to get an
// address of a thread-local variable. On s390x, __tls_get_offset is used
// instead. The difference is __tls_get_offset returns an address of a
// thread-local variable as an offset from TP. So we need to add TP to a
// return value before use. I don't know why it is different, but that is
// the way it is.
//
// https://github.com/IBM/s390x-abi/releases/download/v1.6/lzsabi_s390x.pdf

#include "mold.h"

namespace mold::elf {

using E = S390X;

static void write_low12(u8 *loc, u64 val) {
  *(ub16 *)loc &= 0xf000;
  *(ub16 *)loc |= val & 0x0fff;
}

static void write_mid20(u8 *loc, u64 val) {
  *(ub32 *)loc &= 0xf000'00ff;
  *(ub32 *)loc |= (bits(val, 11, 0) << 16) | (bits(val, 19, 12) << 8);
}

static void write_low24(u8 *loc, u64 val) {
  *(ub32 *)loc &= 0xff00'0000;
  *(ub32 *)loc |= val & 0x00ff'ffff;
}

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static u8 insn[] = {
    0xe3, 0x00, 0xf0, 0x38, 0x00, 0x24, // stg   %r0, 56(%r15)
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOTPLT_OFFSET
    0xd2, 0x07, 0xf0, 0x30, 0x10, 0x08, // mvc   48(8, %r15), 8(%r1)
    0xe3, 0x10, 0x10, 0x10, 0x00, 0x04, // lg    %r1, 16(%r1)
    0x07, 0xf1,                         // br    %r1
    0x07, 0x00, 0x07, 0x00, 0x07, 0x00, // nopr; nopr; nopr
  };

  memcpy(buf, insn, sizeof(insn));
  *(ub32 *)(buf + 8) = (ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 6) >> 1;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static u8 insn[] = {
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOTPLT_ENTRY_OFFSET
    0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, (%r1)
    0xc0, 0x01, 0, 0, 0, 0,             // lgfi  %r0, PLT_INDEX
    0x07, 0xf1,                         // br    %r1
    0x07, 0x00, 0x07, 0x00, 0x07, 0x00, // nopr; nopr; nopr
    0x07, 0x00, 0x07, 0x00, 0x07, 0x00, // nopr; nopr; nopr
  };

  memcpy(buf, insn, sizeof(insn));
  *(ub32 *)(buf + 2) = (sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx)) >> 1;
  *(ub32 *)(buf + 14) = sym.get_plt_idx(ctx) * sizeof(ElfRel<E>);
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
  *(ub32 *)(buf + 2) = (sym.get_got_addr(ctx) - sym.get_plt_addr(ctx)) >> 1;
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
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

  ElfRel<E> *dynrel = nullptr;
  if (ctx.reldyn)
    dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                           file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

    auto check_dbl = [&](i64 val, i64 lo, i64 hi) {
      check(val, lo, hi);

      // R_390_*DBL relocs should never refer a symbol at an odd address
      if (val & 1)
        Error(ctx) << *this << ": misaligned symbol " << sym
                   << " for relocation " << rel;
    };

#define S   sym.get_addr(ctx)
#define A   rel.r_addend
#define P   (get_addr() + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_390_64:
      apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_390_8: {
      i64 val = S + A;
      check(val, 0, 1 << 8);
      *loc = val;
      break;
    }
    case R_390_12: {
      i64 val = S + A;
      check(val, 0, 1 << 12);
      write_low12(loc, val);
      break;
    }
    case R_390_16: {
      i64 val = S + A;
      check(val, 0, 1 << 16);
      *(ub16 *)loc = val;
      break;
    }
    case R_390_20: {
      i64 val = S + A;
      check(val, 0, 1 << 20);
      write_mid20(loc, val);
      break;
    }
    case R_390_32:
    case R_390_PLT32: {
      i64 val = S + A;
      check(val, 0, 1LL << 32);
      *(ub32 *)loc = val;
      break;
    }
    case R_390_PLT64:
      *(ub64 *)loc = S + A;
      break;
    case R_390_PC12DBL:
    case R_390_PLT12DBL: {
      i64 val = S + A - P;
      check_dbl(val, -(1 << 12), 1 << 12);
      write_low12(loc, val >> 1);
      break;
    }
    case R_390_PC16: {
      i64 val = S + A - P;
      check(val, -(1 << 15), 1 << 15);
      *(ub16 *)loc = val;
      break;
    }
    case R_390_PC32: {
      i64 val = S + A - P;
      check(val, -(1LL << 31), 1LL << 31);
      *(ub32 *)loc = val;
      break;
    }
    case R_390_PC64:
      *(ub64 *)loc = S + A - P;
      break;
    case R_390_PC16DBL:
    case R_390_PLT16DBL: {
      i64 val = S + A - P;
      check_dbl(val, -(1 << 16), 1 << 16);
      *(ub16 *)loc = val >> 1;
      break;
    }
    case R_390_PC24DBL:
    case R_390_PLT24DBL: {
      i64 val = S + A - P;
      check_dbl(val, -(1 << 24), 1 << 24);
      write_low24(loc, val >> 1);
      break;
    }
    case R_390_PC32DBL:
    case R_390_PLT32DBL:
      if (ctx.is_static && &sym == ctx.tls_get_offset) {
        // __tls_get_offset() in libc.a is stub code that calls abort().
        // So we provide a replacement function.
        *(ub32 *)loc = (ctx.s390x_tls_get_offset->shdr.sh_addr - P) >> 1;
      } else {
        i64 val = S + A - P;
        check_dbl(val, -(1LL << 32), 1LL << 32);
        *(ub32 *)loc = val >> 1;
      }
      break;
    case R_390_GOT12:
    case R_390_GOTPLT12: {
      i64 val = G + A;
      check(val, 0, 1 << 12);
      write_low12(loc, val);
      break;
    }
    case R_390_GOT16:
    case R_390_GOTPLT16: {
      i64 val = G + A;
      check(val, 0, 1 << 16);
      *(ub16 *)loc = val;
      break;
    }
    case R_390_GOT20:
    case R_390_GOTPLT20: {
      i64 val = G + A;
      check(val, 0, 1 << 20);
      write_mid20(loc, val);
      break;
    }
    case R_390_GOT32:
    case R_390_GOTPLT32: {
      i64 val = G + A;
      check(val, 0, 1LL << 32);
      *(ub32 *)loc = val;
      break;
    }
    case R_390_GOT64:
    case R_390_GOTPLT64:
      *(ub64 *)loc = G + A;
      break;
    case R_390_GOTOFF16:
    case R_390_PLTOFF16: {
      i64 val = S + A - GOT;
      check(val, -(1 << 15), 1 << 15);
      *(ub16 *)loc = val;
      break;
    }
    case R_390_GOTOFF32:
    case R_390_PLTOFF32: {
      i64 val = S + A - GOT;
      check(val, -(1LL << 31), 1LL << 31);
      *(ub32 *)loc = val;
      break;
    }
    case R_390_GOTOFF64:
    case R_390_PLTOFF64:
      *(ub64 *)loc = S + A - GOT;
      break;
    case R_390_GOTPC:
      *(ub64 *)loc = GOT + A - P;
      break;
    case R_390_GOTPCDBL: {
      i64 val = GOT + A - P;
      check_dbl(val, -(1LL << 32), 1LL << 32);
      *(ub32 *)loc = val >> 1;
      break;
    }
    case R_390_GOTENT: {
      i64 val = GOT + G + A - P;
      check(val, -(1LL << 32), 1LL << 32);
      *(ub32 *)loc = val >> 1;
      break;
    }
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
      else
        *(ub32 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_GD64:
      if (sym.has_tlsgd(ctx))
        *(ub64 *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      else
        *(ub64 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_GDCALL:
      if (!sym.has_tlsgd(ctx)) {
        static u8 nop[] = { 0xc0, 0x04, 0x00, 0x00, 0x00, 0x00 };
        memcpy(loc, nop, sizeof(nop));
      }
      break;
    case R_390_TLS_LDM32:
      if (ctx.got->has_tlsld(ctx))
        *(ub32 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      break;
    case R_390_TLS_LDM64:
      if (ctx.got->has_tlsld(ctx))
        *(ub64 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      break;
    case R_390_TLS_LDO32:
      if (ctx.got->has_tlsld(ctx))
        *(ub32 *)loc = S + A - ctx.tls_begin;
      else
        *(ub32 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_LDO64:
      if (ctx.got->has_tlsld(ctx))
        *(ub64 *)loc = S + A - ctx.tls_begin;
      else
        *(ub64 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_LDCALL:
      if (!ctx.got->has_tlsld(ctx)) {
        static u8 nop[] = { 0xc0, 0x04, 0x00, 0x00, 0x00, 0x00 };
        memcpy(loc, nop, sizeof(nop));
      }
      break;
    default:
      unreachable();
    }

#undef S
#undef A
#undef P
#undef G
#undef GOT
  }
}

template <>
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      record_undef_error(ctx, rel);
      continue;
    }

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? frag_addend : (i64)rel.r_addend)

    switch (rel.r_type) {
    case R_390_32: {
      i64 val = S + A;
      check(val, 0, 1LL << 32);
      *(ub32 *)loc = val;
      break;
    }
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
        *(ub64 *)loc = S + A - ctx.tls_begin;
      break;
    default:
      Fatal(ctx) << *this << ": apply_reloc_nonalloc: " << rel;
    }

#undef S
#undef A
  }
}

template <>
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<E>);
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (!sym.file) {
      record_undef_error(ctx, rel);
      continue;
    }

    if (sym.is_ifunc())
      sym.flags.fetch_or(NEEDS_GOT | NEEDS_PLT, std::memory_order_relaxed);

    switch (rel.r_type) {
    case R_390_64:
      scan_dyn_absrel(ctx, sym, rel);
      break;
    case R_390_8:
    case R_390_12:
    case R_390_16:
    case R_390_20:
    case R_390_32:
      scan_absrel(ctx, sym, rel);
      break;
    case R_390_PC16:
    case R_390_PC16DBL:
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
      sym.flags.fetch_or(NEEDS_GOT, std::memory_order_relaxed);
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
        sym.flags.fetch_or(NEEDS_PLT, std::memory_order_relaxed);
      break;
    case R_390_TLS_GOTIE20:
    case R_390_TLS_IEENT:
      sym.flags.fetch_or(NEEDS_GOTTP, std::memory_order_relaxed);
      break;
    case R_390_TLS_GD32:
    case R_390_TLS_GD64:
      if (!relax_tlsgd(ctx, sym))
        sym.flags.fetch_or(NEEDS_TLSGD, std::memory_order_relaxed);
      break;
    case R_390_TLS_LDM32:
    case R_390_TLS_LDM64:
      if (!relax_tlsld(ctx))
        ctx.needs_tlsld = true;
      break;
    case R_390_TLS_LE32:
    case R_390_TLS_LE64:
    case R_390_TLS_LDO32:
    case R_390_TLS_LDO64:
    case R_390_TLS_GDCALL:
    case R_390_TLS_LDCALL:
      break;
    default:
      Fatal(ctx) << *this << ": scan_relocations: " << rel;
    }
  }
}

// __tls_get_offset() in libc.a just calls abort(), assuming that the
// linker always relaxes TLS calls for statically-linkd executables.
// We don't always do that because we believe --relax and --static
// should be orthogonal.
//
// This section provides a replacement for __tls_get_offset() in libc.a.
void S390XTlsGetOffsetSection::copy_buf(Context<E> &ctx) {
  static const u8 insn[] = {
    0xc0, 0x10, 0, 0, 0, 0,             // larl %r1, GOT
    0xb9, 0x08, 0x00, 0x21,             // agr  %r2, %r1
    0xe3, 0x20, 0x20, 0x08, 0x00, 0x04, // lg   %r2, 8(%r2)
    0xc0, 0x11, 0, 0, 0, 0,             // lgfi %r1, TLS_BLOCK_SIZE
    0xb9, 0x09, 0x00, 0x21,             // sgr  %r2, %r1
    0x07, 0xfe,                         // br   %r14
  };

  assert(this->shdr.sh_size == sizeof(insn));

  u8 *loc = ctx.buf + this->shdr.sh_offset;
  memcpy(loc, insn, sizeof(insn));
  *(ub32 *)(loc + 2) = (ctx.got->shdr.sh_addr - this->shdr.sh_addr) >> 1;
  *(ub32 *)(loc + 18) = ctx.tp_addr - ctx.tls_begin;
}

} // namespace mold::elf
