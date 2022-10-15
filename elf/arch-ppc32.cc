// This file supports PowerPC 32-bit ISA. For 64-bit PowerPC, see
// arch-ppc64v1.cpp and arch-ppc64v2.cpp.
//
// PPC32 is a RISC ISA. It has 32 general-purpose registers (GPRs).
// r0, r11 and r12 are reserved for static linkers, so we can use these
// registers in PLTs and range extension thunks.
//
// Besides GPRs, PowerPC a few special registesr. Notable ones are LR
// which holds a return address and CTR which is used for branching.
//
// PowerPC generally lacks PC-relative load/store instructions, so it is
// not straightforward to support position-independent code. A position-
// independent function contains code like this in its prologue to obtain
// its own address
//
//   mflr    r0        // save the current return address to %r0
//   bcl     20, 31, 4 // call the next instruction as if it were a function
//   mtlr    r12       // save the return address to %r12
//   mtlr    r0        // restore the original return address
//
// , and then the function computes its .got2 address (or .got2+0x800) to
// %r30. The rule for the %r30 value is complicated, so we essentially
// ignore that part of the ABI. Our PLT and range extension thunks don't
// depend on %r30 value and position-independent by themselves.

#include "mold.h"

namespace mold::elf {

using E = PPC32;

static u64 lo(u64 x)       { return x & 0xffff; }
static u64 hi(u64 x)       { return x >> 16; }
static u64 ha(u64 x)       { return (x + 0x8000) >> 16; }
static u64 high(u64 x)     { return (x >> 16) & 0xffff; }
static u64 higha(u64 x)    { return ((x + 0x8000) >> 16) & 0xffff; }
static u64 higher(u64 x)   { return (x >> 32) & 0xffff; }
static u64 highera(u64 x)  { return ((x + 0x8000) >> 32) & 0xffff; }
static u64 highest(u64 x)  { return x >> 48; }
static u64 highesta(u64 x) { return (x + 0x8000) >> 48; }

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static const ub32 insn[] = {
    // Get the address of this thunk
    0x7c08'02a6, //    mflr    r0
    0x429f'0005, //    bcl     20, 31, 4
    0x7d88'02a6, // 1: mflr    r12
    0x7c08'03a6, //    mtlr    r0

    // Compute the runtime address of GOTPLT+12
    0x3d8c'0000, //    addis   r12, r12, (GOTPLT - 1b)@higha
    0x398c'0000, //    addi    r12, r12, (GOTPLT - 1b)@lo

    // Compute the PLT entry offset
    0x7d6c'5850, //    sub     r11, r11, r12
    0x1d6b'0003, //    mulli   r11, r11, 3

    // Load GOTPLT[2] and branch to GOTPLT[1]
    0x800c'fff8, //    lwz     r0,  -8(r12)
    0x7c09'03a6, //    mtctr   r0
    0x818c'fffc, //    lwz     r12, -4(r12)
    0x4e80'0420, //    bctr
    0x6000'0000, //    nop
    0x6000'0000, //    nop
    0x6000'0000, //    nop
    0x6000'0000, //    nop
  };

  static_assert(sizeof(insn) == E::plt_hdr_size);
  memcpy(buf, insn, sizeof(insn));

  ub32 *loc = (ub32 *)buf;
  loc[4] |= higha(ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr + 4);
  loc[5] |= lo(ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr + 4);
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const ub32 insn[] = {
    // Get the address of this PLT entry
    0x7c08'02a6, // mflr    r0
    0x429f'0005, // bcl     20, 31, 4
    0x7d88'02a6, // mflr    r12
    0x7c08'03a6, // mtlr    r0

    // Load an address from the GOTPLT entry and jump to that address
    0x3d6c'0000, // addis   r11, r12, OFFSET@higha
    0x396b'0000, // addi    r11, r11, OFFSET@lo
    0x818b'0000, // lwz     r12, 0(r11)
    0x7d89'03a6, // mtctr   r12
    0x4e80'0420, // bctr
  };

  static_assert(E::plt_size == sizeof(insn));
  memcpy(buf, insn, sizeof(insn));

  ub32 *loc = (ub32 *)buf;
  i64 offset = sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx) - 8;
  loc[4] |= higha(offset);
  loc[5] |= lo(offset);
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const ub32 insn[] = {
    // Get the address of this PLT entry
    0x7c08'02a6, // mflr    r0
    0x429f'0005, // bcl     20, 31, 4
    0x7d88'02a6, // mflr    r12
    0x7c08'03a6, // mtlr    r0

    // Load an address from the GOT entry and jump to that address
    0x3d6c'0000, // addis   r11, r12, 0
    0x396b'0000, // addi    r11, r11, 0
    0x818b'0000, // lwz     r12, 0(r11)
    0x7d89'03a6, // mtctr   r12
    0x4e80'0420, // bctr
  };

  static_assert(E::pltgot_size == sizeof(insn));
  memcpy(buf, insn, sizeof(insn));

  ub32 *loc = (ub32 *)buf;
  i64 offset = sym.get_got_addr(ctx) - sym.get_plt_addr(ctx) - 8;
  loc[4] |= higha(offset);
  loc[5] |= lo(offset);
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_PPC_ADDR32:
    *(ub32 *)loc = val;
    break;
  case R_PPC_REL32:
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

  u64 GP = 0;
  if (file.ppc32_got2)
    GP = file.ppc32_got2->output_section->shdr.sh_addr + file.ppc32_got2->offset;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    auto get_thunk_addr = [&] {
      RangeExtensionRef ref = extra.range_extn[i];
      assert(ref.thunk_idx != -1);
      return output_section->thunks[ref.thunk_idx]->get_addr(ref.sym_idx);
    };

#define S   sym.get_addr(ctx)
#define A   rel.r_addend
#define P   (get_addr() + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_PPC_ADDR32:
    case R_PPC_UADDR32:
      apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_PPC_ADDR14:
      *(ub32 *)loc &= 0b1111'1111'1111'1111'0000'0000'0000'0011;
      *(ub32 *)loc |= bits(S + A, 15, 2) << 2;
      break;
    case R_PPC_ADDR16:
    case R_PPC_UADDR16:
    case R_PPC_ADDR16_LO:
      *(ub16 *)loc = lo(S + A);
      break;
    case R_PPC_ADDR16_HI:
      *(ub16 *)loc = hi(S + A);
      break;
    case R_PPC_ADDR16_HA:
      *(ub16 *)loc = ha(S + A);
      break;
    case R_PPC_ADDR24:
      *(ub32 *)loc &= 0b1111'1100'0000'0000'0000'0000'0000'0011;
      *(ub32 *)loc |= bits(S + A, 25, 2) << 2;
      break;
    case R_PPC_ADDR30:
      *(ub32 *)loc &= 0b0000'0000'0000'0000'0000'0000'0000'0011;
      *(ub32 *)loc |= bits(S + A, 31, 2) << 2;
      break;
    case R_PPC_PLT16_LO:
      *(ub16 *)loc = lo(G + GOT - GP - A);
      break;
    case R_PPC_PLT16_HI:
      *(ub16 *)loc = hi(G + GOT - GP - A);
      break;
    case R_PPC_PLT16_HA:
      *(ub16 *)loc = ha(G + GOT - GP - A);
      break;
    case R_PPC_PLT32:
      *(ub32 *)loc = G + GOT - GP - A;
      break;
    case R_PPC_REL14:
      *(ub32 *)loc &= 0b1111'1111'1111'1111'0000'0000'0000'0011;
      *(ub32 *)loc |= bits(S + A - P, 15, 2) << 2;
      break;
    case R_PPC_REL16:
    case R_PPC_REL16_LO:
      *(ub16 *)loc = S + A - P;
      break;
    case R_PPC_REL16_HI:
      *(ub16 *)loc = hi(S + A - P);
      break;
    case R_PPC_REL16_HA:
      *(ub16 *)loc = ha(S + A - P);
      break;
    case R_PPC_REL24: {
      i64 val = S + A - P;
      if (sign_extend(val, 25) != val)
        val = get_thunk_addr() - P;

      *(ub32 *)loc &= 0b1111'1100'0000'0000'0000'0000'0000'0011;
      *(ub32 *)loc |= bits(val, 25, 2) << 2;
      break;
    }
    case R_PPC_PLTREL24: {
      i64 val = S - P;
      if (sym.has_plt(ctx) || sign_extend(val, 25) != val)
        val = get_thunk_addr() - P;

      *(ub32 *)loc &= 0b1111'1100'0000'0000'0000'0000'0000'0011;
      *(ub32 *)loc |= bits(val, 25, 2) << 2;
      break;
    }
    case R_PPC_REL32:
    case R_PPC_PLTREL32:
      *(ub32 *)loc = S + A - P;
      break;
    case R_PPC_LOCAL24PC:
      assert(!sym.is_imported);
      *(ub32 *)loc &= 0b1111'1100'0000'0000'0000'0000'0000'0011;
      *(ub32 *)loc |= bits(S + A - P, 25, 2) << 2;
      break;
    case R_PPC_GOT16:
    case R_PPC_GOT16_LO:
      *(ub16 *)loc = G + A;
      break;
    case R_PPC_GOT16_HI:
      *(ub16 *)loc = hi(G + A);
      break;
    case R_PPC_GOT16_HA:
      *(ub16 *)loc = ha(G + A);
      break;
    case R_PPC_TPREL16_LO:
      *(ub16 *)loc = lo(S + A - ctx.tp_addr);
      break;
    case R_PPC_TPREL16_HI:
      *(ub16 *)loc = hi(S + A - ctx.tp_addr);
      break;
    case R_PPC_TPREL16_HA:
      *(ub16 *)loc = ha(S + A - ctx.tp_addr);
      break;
    case R_PPC_DTPREL16_LO:
      *(ub16 *)loc = lo(S + A - ctx.dtp_addr);
      break;
    case R_PPC_DTPREL16_HI:
      *(ub16 *)loc = hi(S + A - ctx.dtp_addr);
      break;
    case R_PPC_DTPREL16_HA:
      *(ub16 *)loc = ha(S + A - ctx.dtp_addr);
      break;
    case R_PPC_GOT_TLSGD16:
      *(ub16 *)loc = sym.get_tlsgd_addr(ctx) - GOT;
      break;
    case R_PPC_GOT_TLSLD16:
      *(ub16 *)loc = ctx.got->get_tlsld_addr(ctx) - GOT;
      break;
    case R_PPC_GOT_TPREL16:
      *(ub16 *)loc = sym.get_gottp_addr(ctx) - GOT;
      break;
    case R_PPC_TLS:
    case R_PPC_TLSGD:
    case R_PPC_TLSLD:
    case R_PPC_PLTSEQ:
    case R_PPC_PLTCALL:
      break;
    default:
      Fatal(ctx) << *this << ": apply_reloc_alloc relocation: " << rel;
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
    case R_PPC_ADDR32:
      *(ub32 *)loc = S + A;
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
    case R_PPC_ADDR32:
    case R_PPC_UADDR32:
      scan_dyn_absrel(ctx, sym, rel);
      break;
    case R_PPC_ADDR14:
    case R_PPC_ADDR16:
    case R_PPC_UADDR16:
    case R_PPC_ADDR16_LO:
    case R_PPC_ADDR16_HI:
    case R_PPC_ADDR16_HA:
    case R_PPC_ADDR24:
    case R_PPC_ADDR30:
      scan_absrel(ctx, sym, rel);
      break;
    case R_PPC_REL14:
    case R_PPC_REL16:
    case R_PPC_REL16_LO:
    case R_PPC_REL16_HI:
    case R_PPC_REL16_HA:
    case R_PPC_REL32:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_PPC_GOT16:
    case R_PPC_GOT16_LO:
    case R_PPC_GOT16_HI:
    case R_PPC_GOT16_HA:
    case R_PPC_PLT16_LO:
    case R_PPC_PLT16_HI:
    case R_PPC_PLT16_HA:
    case R_PPC_PLT32:
      sym.flags.fetch_or(NEEDS_GOT, std::memory_order_relaxed);
      break;
    case R_PPC_REL24:
    case R_PPC_PLTREL24:
    case R_PPC_PLTREL32:
      if (sym.is_imported)
        sym.flags.fetch_or(NEEDS_PLT, std::memory_order_relaxed);
      break;
    case R_PPC_GOT_TLSGD16:
      sym.flags.fetch_or(NEEDS_TLSGD, std::memory_order_relaxed);
      break;
    case R_PPC_GOT_TLSLD16:
      ctx.needs_tlsld.store(true, std::memory_order_relaxed);
      break;
    case R_PPC_GOT_TPREL16:
      sym.flags.fetch_or(NEEDS_GOTTP, std::memory_order_relaxed);
      break;
    case R_PPC_LOCAL24PC:
    case R_PPC_TLS:
    case R_PPC_TLSGD:
    case R_PPC_TLSLD:
    case R_PPC_TPREL16_LO:
    case R_PPC_TPREL16_HI:
    case R_PPC_TPREL16_HA:
    case R_PPC_DTPREL16_LO:
    case R_PPC_DTPREL16_HI:
    case R_PPC_DTPREL16_HA:
    case R_PPC_PLTSEQ:
    case R_PPC_PLTCALL:
      break;
    default:
      Fatal(ctx) << *this << ": scan_relocations: " << rel;
    }
  }
}

template <>
void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;

  static const ub32 plt_thunk[] = {
    // Get this thunk's address
    0x7c08'02a6, // mflr    r0
    0x429f'0005, // bcl     20, 31, 4
    0x7d88'02a6, // mflr    r12
    0x7c08'03a6, // mtlr    r0

    // Load an address from the GOT/GOTPLT entry and jump to that address
    0x3d6c'0000, // addis   r11, r12, OFFSET@higha
    0x396b'0000, // addi    r11, r11, OFFSET@lo
    0x818b'0000, // lwz     r12, 0(r11)
    0x7d89'03a6, // mtctr   r12
    0x4e80'0420, // bctr
  };

  static const ub32 local_thunk[] = {
    // Get this thunk's address
    0x7c08'02a6, // mflr    r0
    0x429f'0005, // bcl     20, 31, 4
    0x7d88'02a6, // mflr    r12
    0x7c08'03a6, // mtlr    r0

    // Materialize the destination's address in %r11 and jump to that address
    0x3d6c'0000, // addis   r11, r12, OFFSET@higha
    0x396b'0000, // addi    r11, r11, OFFSET@lo
    0x7d69'03a6, // mtctr   r11
    0x4e80'0420, // bctr
    0x6000'0000, // nop
  };

  static_assert(E::thunk_size == sizeof(plt_thunk));
  static_assert(E::thunk_size == sizeof(local_thunk));

  for (i64 i = 0; i < symbols.size(); i++) {
    ub32 *loc = (ub32 *)(buf + i * E::thunk_size);
    Symbol<E> &sym = *symbols[i];

    if (sym.has_got(ctx)) {
      memcpy(loc, plt_thunk, sizeof(plt_thunk));
      i64 val = sym.get_got_addr(ctx) - get_addr(i) - 8;
      loc[4] |= higha(val);
      loc[5] |= lo(val);
    } else if (sym.has_plt(ctx)) {
      memcpy(loc, plt_thunk, sizeof(plt_thunk));
      i64 val = sym.get_gotplt_addr(ctx) - get_addr(i) - 8;
      loc[4] |= higha(val);
      loc[5] |= lo(val);
    } else {
      memcpy(loc, local_thunk, sizeof(local_thunk));
      i64 val = sym.get_addr(ctx) - get_addr(i) - 8;
      loc[4] |= higha(val);
      loc[5] |= lo(val);
    }
  }
}

} // namespace mold::elf
