// r31 is blink. r30 is I believe a scratch register.
//
// Programs might use one of several branch, jump, and link instructions
// to control execution flow through direct and indirect function calls
// and branching. For function calling, the conditional branch-and-link
// instruction has a maximum branch range of ±1 MiB, and the target
// address is 32-bit-aligned. The unconditional branch-and-link format has
// a maximum branch range of ±16 MiB.
//
// https://github.com/foss-for-synopsys-dwc-arc-processors/arc-ABI-manual/blob/master/ARCv2_ABI.pdf

#include "mold.h"

namespace mold {

using E = ARC;

// An integer-like class for the mixed-endian encoding
class M32 {
public:
  operator u32() const {
    return (buf[1] << 24) | (buf[0] << 16) | (buf[3] << 8) | buf[2];
  }

  M32 &operator=(u32 x) {
    buf[0] = x >> 16;
    buf[1] = x >> 24;
    buf[2] = x;
    buf[3] = x >> 8;
    return *this;
  }

  M32 &operator&=(u32 x) { return *this = *this & x; }
  M32 &operator|=(u32 x) { return *this = *this | x; }
  u8 buf[4];
};

static void write_disp7u(u8 *loc, u32 val) {
  *(ul16 *)loc |= (bits(val, 6, 3) << 4) | bits(val, 2, 0);
}

static void write_disp9(u8 *loc, u32 val) {
  *(M32 *)loc |= bits(val, 8, 0);
}

static void write_disp9ls(u8 *loc, u32 val) {
  *(M32 *)loc |= (bits(val, 7, 0) << 16) | (bit(val, 8) << 15);
}

static void write_disp9s(u8 *loc, u32 val) {
  *(ul16 *)loc |= bits(val, 8, 0);
}

static void write_disp10u(u8 *loc, u32 val) {
  *(ul16 *)loc |= bits(val, 9, 0);
}

static void write_disp13s(u8 *loc, u32 val) {
  *(ul16 *)loc |= bits(val, 12, 0);
}

static void write_disp21h(u8 *loc, u32 val) {
  *(M32 *)loc |= (bits(val, 10, 1) << 17) | (bits(val, 20, 11) << 6);
}

static void write_disp21w(u8 *loc, u32 val) {
  *(M32 *)loc |= (bits(val, 10, 2) << 18) | (bits(val, 20, 11) << 6);
}

static void write_disp25h(u8 *loc, u32 val) {
  *(M32 *)loc |= (bits(val, 10, 1) << 17) | (bits(val, 20, 11) << 6) |
                  bits(val, 24, 21);
}

static void write_disp25w(u8 *loc, u32 val) {
  *(M32 *)loc |= (bits(val, 10, 2) << 18) | (bits(val, 20, 11) << 6) |
                  bits(val, 24, 21);
}

static void write_disps9(u8 *loc, u32 val) {
  *(ul16 *)loc |= bits(val, 10, 2);
}

static void write_disps12(u8 *loc, u32 val) {
  *(M32 *)loc |= (bits(val, 5, 0) << 6) | bits(val, 11, 6);
}

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static const ul16 insn[] = {
    0x2730, 0x7f8b, 0, 0, // ld r11, [pcl,0]
    0x2730, 0x7f8a, 0, 0, // ld r10, [pcl,0]
    0x2020, 0x0280,       // j       [r10]
    0, 0,                 // (address of GOTPLT)
  };

  memcpy(buf, insn, sizeof(insn));
  *(M32 *)(buf + 4) = ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr + 4;
  *(M32 *)(buf + 12) = ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr;
  *(M32 *)(buf + 20) = ctx.gotplt->shdr.sh_addr;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const ul16 insn[] = {
    0x2730, 0x7f8c, 0, 0, // ld  r12, [pcl,0]
    0x2021, 0x0300,       // j.d [r12]
    0x240a, 0x1fc0,       // mov r12,pcl
  };

  memcpy(buf, insn, sizeof(insn));
  *(M32 *)(buf + 4) = sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx);
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const ul16 insn[] = {
    0x2730, 0x7f8c, 0, 0, // ld  r12, [pcl,0]
    0x2021, 0x0300,       // j.d [r12]
    0x240a, 0x1fc0,       // mov r12,pcl
  };

  memcpy(buf, insn, sizeof(insn));
  *(M32 *)(buf + 4) = sym.get_got_pltgot_addr(ctx) - sym.get_plt_addr(ctx);
}

template <>
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                       u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_ARC_32:
    *(ul32 *)loc = val;
    break;
  case R_ARC_32_PCREL:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
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

    switch (rel.r_type) {
    case R_ARC_32:
      *(ul32 *)loc = S + A;
      break;
    case R_ARC_32_ME:
      *(M32 *)loc = S + A;
      break;
    case R_ARC_S25H_PCREL:
    case R_ARC_S25H_PCREL_PLT:
      write_disp25h(loc, align_to(S + A - P, 2));
      break;
    case R_ARC_S25W_PCREL:
    case R_ARC_S25W_PCREL_PLT:
      write_disp25w(loc, align_to(S + A - P, 4));
      break;
    case R_ARC_PC32:
      *(M32 *)loc = S + A - P;
      break;
    case R_ARC_32_PCREL:
      *(ul32 *)loc = S + A - P;
      break;
    case R_ARC_GOTPC32:
      *(M32 *)loc = GOT + G + A - P;
      break;
    case R_ARC_TLS_LE_32:
      *(M32 *)loc = S + A - ctx.tp_addr;
      break;
    case R_ARC_TLS_IE_GOT:
      *(M32 *)loc = sym.get_gottp_addr(ctx) + A - P;
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

    switch (rel.r_type) {
    case R_ARC_32:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul32 *)loc = *val;
      else
        *(ul32 *)loc = S + A;
      break;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
    }
  }
}

template <>
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (sym.is_ifunc())
      Error(ctx) << sym << ": GNU ifunc symbol is not supported on m68k";

    switch (rel.r_type) {
    case R_ARC_32:
      break;
    case R_ARC_32_ME:
      scan_absrel(ctx, sym, rel);
      break;
    case R_ARC_S25H_PCREL:
    case R_ARC_S25W_PCREL:
    case R_ARC_32_PCREL:
    case R_ARC_PC32:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_ARC_S25H_PCREL_PLT:
    case R_ARC_S25W_PCREL_PLT:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_ARC_GOTPC32:
      sym.flags |= NEEDS_GOT;
      break;
    case R_ARC_TLS_LE_32:
      check_tlsle(ctx, sym, rel);
      break;
    case R_ARC_TLS_IE_GOT:
      sym.flags |= NEEDS_GOTTP;
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <>
u64 get_eflags(Context<E> &ctx) {
  return 0x406;
}

} // namespace mold
