// This file contains code for the IBM z/Architecture ISA, which is
// commonly referred to as "s390x" as a target name on Linux.
//
// z/Architecture is a 64-bit CISC ISA developed by IBM around 2000 for
// IBM's "big iron" mainframe computers. The computers are direct
// descendents of IBM System/360 all the way back in 1966. I've never
// actually seen a mainframe, and you probaly haven't either, but it looks
// like the mainframe market is still large enough to sustain its ecosystem.
// Ubuntu for example provides the official support for s390x as of 2022.
// Since they are being actively maintained, we need to support them.
//
// As an instruction set, s390x is actually straightforward to support.
// It has 16 general-purpose registers. Instructions vary in size but
// always be a multiple of 2 and always aligned to 2 bytes boundaries.
//
// Its psABI reserves %r0 and %r1 as scratch registers so we can use them
// in our PLT. %r2-%r6 are used for parameter passing. %r2 is also used to
// return a value. In position independent code, %r12 usually contains the
// address of GOT. %r14 usually contains a return address. %r15 is a stack
// pointer. Special registers %a0 and %a1 contain the upper 32 bits and
// the lower 32 bits of TP, respectively.
//
// https://uclibc.org/docs/psABI-s390x.pdf

#include "mold.h"

namespace mold::elf {

using E = S390;

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static u8 insn[] = {
    0xe3, 0x00, 0xf0, 0x38, 0x00, 0x24, // stg   %r0, 56(%r15)
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOT_OFFSET
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

#define S   sym.get_addr(ctx)
#define A   rel.r_addend
#define P   (get_addr() + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_390_64:
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_390_8:
      *loc = S + A;
      break;
    case R_390_12:
      *(ub16 *)loc &= 0xf000;
      *(ub16 *)loc |= (S + A) & 0x0fff;
      break;
    case R_390_16:
      *(ub16 *)loc = S + A;
      break;
    case R_390_32:
    case R_390_PLT32:
      *(ub32 *)loc = S + A;
      break;
    case R_390_PLT64:
      *(ub64 *)loc = S + A;
      break;
    case R_390_PC16:
      *(ub16 *)loc = S + A - P;
      break;
    case R_390_PC16DBL:
    case R_390_PLT16DBL:
      *(ub16 *)loc = (S + A - P) >> 1;
      break;
    case R_390_PC32:
      *(ub32 *)loc = S + A - P;
      break;
    case R_390_PC32DBL:
    case R_390_PLT32DBL:
      if (ctx.is_static && &sym == ctx.tls_get_offset) {
        // __tls_get_offset() in libc.a is stub code that calls abort().
        // So we provide a replacement function.
        *(ub32 *)loc = (ctx.s390_tls_get_offset->shdr.sh_addr - P) >> 1;
      } else {
        *(ub32 *)loc = (S + A - P) >> 1;
      }
      break;
    case R_390_PC64:
      *(ub64 *)loc = S + A - P;
      break;
    case R_390_GOT12:
      *(ub16 *)loc &= 0xf000;
      *(ub16 *)loc |= (G + GOT + A) & 0x0fff;
      break;
    case R_390_GOT16:
      *(ub16 *)loc = G + GOT + A;
      break;
    case R_390_GOT32:
      *(ub32 *)loc = G + GOT + A;
      break;
    case R_390_GOT64:
      *(ub64 *)loc = G + GOT + A;
      break;
    case R_390_GOTOFF16:
      *(ub16 *)loc = S + A - GOT;
      break;
    case R_390_GOTOFF64:
      *(ub64 *)loc = S + A - GOT;
      break;
    case R_390_GOTPC:
      *(ub64 *)loc = GOT + A - P;
      break;
    case R_390_GOTPCDBL:
      *(ub32 *)loc = (GOT + A - P) >> 1;
      break;
    case R_390_GOTENT:
      *(ub32 *)loc = (GOT + G + A - P) >> 1;
      break;
    case R_390_TLS_LE32:
      *(ub32 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_LE64:
      *(ub64 *)loc = S + A - ctx.tp_addr;
      break;
    case R_390_TLS_GOTIE20: {
      i64 val = sym.get_gottp_addr(ctx) + A - GOT;
      *(ub32 *)loc &= 0xf000'00ff;
      *(ub32 *)loc |= (bits(val, 11, 0) << 16) | (bits(val, 19, 12) << 8);
      break;
    }
    case R_390_TLS_IEENT:
      *(ub32 *)loc = (sym.get_gottp_addr(ctx) + A - P) >> 1;
      break;
    case R_390_TLS_GD32:
      if (sym.get_tlsgd_idx(ctx) == -1)
        *(ub32 *)loc = S + A - ctx.tp_addr;
      else
        *(ub32 *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      break;
    case R_390_TLS_GD64:
      if (sym.get_tlsgd_idx(ctx) == -1)
        *(ub64 *)loc = S + A - ctx.tp_addr;
      else
        *(ub64 *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      break;
    case R_390_TLS_GDCALL:
      if (sym.get_tlsgd_idx(ctx) == -1) {
        static const u8 nop[] = { 0xc0, 0x04, 0x00, 0x00, 0x00, 0x00 };
        memcpy(loc, nop, sizeof(nop));
      }
      break;
    case R_390_TLS_LDM32:
      if (ctx.got->tlsld_idx == -1)
        *(ub32 *)loc = 0;
      else
        *(ub32 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      break;
    case R_390_TLS_LDM64:
      if (ctx.got->tlsld_idx == -1)
        *(ub64 *)loc = 0;
      else
        *(ub64 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      break;
    case R_390_TLS_LDO32:
      if (ctx.got->tlsld_idx == -1)
        *(ub32 *)loc = S + A - ctx.tp_addr;
      else
        *(ub32 *)loc = S + A - ctx.tls_begin;
      break;
    case R_390_TLS_LDO64:
      if (ctx.got->tlsld_idx == -1)
        *(ub64 *)loc = S + A - ctx.tp_addr;
      else
        *(ub64 *)loc = S + A - ctx.tls_begin;
      break;
    case R_390_TLS_LDCALL:
      if (ctx.got->tlsld_idx == -1) {
        static const u8 nop[] = { 0xc0, 0x04, 0x00, 0x00, 0x00, 0x00 };
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

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? frag_addend : (i64)rel.r_addend)

    switch (rel.r_type) {
    case R_390_32:
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
      sym.flags |= (NEEDS_GOT | NEEDS_PLT);

    switch (rel.r_type) {
    case R_390_64:
      scan_abs_dyn_rel(ctx, sym, rel);
      break;
    case R_390_8:
    case R_390_12:
    case R_390_16:
    case R_390_32:
      scan_abs_rel(ctx, sym, rel);
      break;
    case R_390_PC16:
    case R_390_PC16DBL:
    case R_390_PC32:
    case R_390_PC32DBL:
    case R_390_PC64:
      scan_pcrel_rel(ctx, sym, rel);
      break;
    case R_390_GOT12:
    case R_390_GOT16:
    case R_390_GOT32:
    case R_390_GOT64:
    case R_390_GOTOFF16:
    case R_390_GOTOFF64:
    case R_390_GOTPC:
    case R_390_GOTPCDBL:
    case R_390_GOTENT:
      sym.flags |= NEEDS_GOT;
      break;
    case R_390_PLT16DBL:
    case R_390_PLT32:
    case R_390_PLT32DBL:
    case R_390_PLT64:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_390_TLS_LE32:
    case R_390_TLS_LE64:
    case R_390_TLS_GOTIE20:
    case R_390_TLS_IEENT:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_390_TLS_GD32:
    case R_390_TLS_GD64:
      if (!relax_tlsgd(ctx, sym))
        sym.flags |= NEEDS_TLSGD;
      break;
    case R_390_TLS_LDM32:
    case R_390_TLS_LDM64: {
      if (!relax_tlsld(ctx, sym))
        ctx.needs_tlsld = true;
      break;
    }
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

// __tls_get_offset() in libc.a just calls abort().
// This section provides a replacement.
void S390TlsGetOffsetSection::copy_buf(Context<E> &ctx) {
  static const u8 insn[] = {
    0xb9, 0x08, 0x00, 0x2c,             // agr  %r2, %r12
    0xe3, 0x20, 0x20, 0x08, 0x00, 0x04, // lg   %r2, 8(%r2)
    0xc0, 0x11, 0, 0, 0, 0,             // lgfi %r1, TLS_BLOCK_SIZE
    0xb9, 0x09, 0x00, 0x21,             // sgr  %r2, %r1
    0x07, 0xfe,                         // br   %r14
  };

  assert(this->shdr.sh_size == sizeof(insn));

  u8 *loc = ctx.buf + this->shdr.sh_offset;
  memcpy(loc, insn, sizeof(insn));
  *(ub32 *)(loc + 12) = ctx.tp_addr - ctx.tls_begin;
}

} // namespace mold::elf
