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
    0x07, 0x00,                         // nopr
    0x07, 0x00,                         // nopr
    0x07, 0x00,                         // nopr
  };

  memcpy(buf, insn, sizeof(insn));
  *(ub32 *)(buf + 8) = (ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 6) >> 1;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static u8 insn[] = {
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOT_ENTRY_OFFSET
    0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, 0(%r1)
    0xc0, 0x01, 0, 0, 0, 0,             // lgfi  %r0, PLT_INDEX
    0x07, 0xf1,                         // br    %r1
    0x07, 0x00, 0x07, 0x00, 0x07, 0x00, // nopr
    0x07, 0x00, 0x07, 0x00, 0x07, 0x00, // nopr
  };

  memcpy(buf, insn, sizeof(insn));
  *(ub32 *)(buf + 2) = (sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx)) >> 1;
  *(ub32 *)(buf + 14) = sym.get_plt_idx(ctx) * 24;
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static u8 insn[] = {
    0xc0, 0x10, 0, 0, 0, 0,             // larl  %r1, GOT_ENTRY_OFFSET
    0xe3, 0x10, 0x10, 0x00, 0x00, 0x04, // lg    %r1, 0(%r1)
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
      *(ub16 *)loc = (*(ub16 *)loc & 0xf00) | (S + A) & 0x0fff;
      break;
    case R_390_16:
      *(ub16 *)loc = S + A;
      break;
    case R_390_32:
      *(ub32 *)loc = S + A;
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
      *(ub32 *)loc = (S + A - P) >> 1;
      break;
    case R_390_PC64:
      *(ub64 *)loc = S + A - P;
      break;
    case R_390_GOT12:
      *(ub16 *)loc = (*(ub16 *)loc & 0xf00) | (G + GOT + A) & 0x0fff;
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
    case R_390_GOTOFF:
      *(ub32 *)loc = S + A - GOT;
      break;
    case R_390_GOTPC:
      *(ub32 *)loc = GOT + A - P;
      break;
    case R_390_GOTPCDBL:
      *(ub32 *)loc = (GOT + A - P) >> 1;
      break;
    case R_390_GOTENT:
      *(ub32 *)loc = (GOT + G + A - P) >> 1;
      break;
    case R_390_PLT32:
      *(ub32 *)loc = S + A;
      break;
    case R_390_PLT64:
      *(ub64 *)loc = S + A;
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
  Symbol<E> &tls_get_addr = *get_symbol(ctx, "__tls_get_addr");

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
    case R_390_GOTOFF:
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
    default:
      Fatal(ctx) << *this << ": scan_relocations: " << rel;
    }
  }
}

} // namespace mold::elf
