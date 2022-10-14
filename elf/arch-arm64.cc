// This file contains ARM64-specific code. Being new, the ARM64's ELF
// psABI doesn't have anything peculiar. ARM64 is a clean RISC
// instruction set that supports PC-relative load/store instructions.
//
// Unlike ARM32, instructions length doesn't vary. All ARM64
// instructions are 4 bytes long.
//
// Branch instructions used for function call can jump within ±128 MiB.
// We need to create range extension thunks to support binaries whose
// .text is larger than that.
//
// Unlike most other targets, the TLSDESC access model is used by default
// for -fPIC to access thread-local variables instead of the less
// efficient GD model. You can still enable GD but it needs the
// -mtls-dialect=trad flag. Since GD is used rarely, we don't need to
// implement GD → LE relaxation.
//
// https://github.com/ARM-software/abi-aa/blob/main/aaelf64/aaelf64.rst

#include "mold.h"

namespace mold::elf {

using E = ARM64;

static void write_adrp(u8 *buf, u64 val) {
  u32 hi = bits(val, 32, 14);
  u32 lo = bits(val, 13, 12);
  *(ul32 *)buf &= 0b1001'1111'0000'0000'0000'0000'0001'1111;
  *(ul32 *)buf |= (lo << 29) | (hi << 5);
}

static void write_adr(u8 *buf, u64 val) {
  u32 hi = bits(val, 20, 2);
  u32 lo = bits(val, 1, 0);
  *(ul32 *)buf &= 0b1001'1111'0000'0000'0000'0000'0001'1111;
  *(ul32 *)buf |= (lo << 29) | (hi << 5);
}

static u64 page(u64 val) {
  return val & 0xffff'ffff'ffff'f000;
}

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static const ul32 insn[] = {
    0xa9bf'7bf0, // stp  x16, x30, [sp,#-16]!
    0x9000'0010, // adrp x16, .got.plt[2]
    0xf940'0211, // ldr  x17, [x16, .got.plt[2]]
    0x9100'0210, // add  x16, x16, .got.plt[2]
    0xd61f'0220, // br   x17
    0xd503'201f, // nop
    0xd503'201f, // nop
    0xd503'201f, // nop
  };

  u64 gotplt = ctx.gotplt->shdr.sh_addr + 16;
  u64 plt = ctx.plt->shdr.sh_addr;

  memcpy(buf, insn, sizeof(insn));
  write_adrp(buf + 4, page(gotplt) - page(plt + 4));
  *(ul32 *)(buf + 8) |= bits(gotplt, 11, 3) << 10;
  *(ul32 *)(buf + 12) |= (gotplt & 0xfff) << 10;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const ul32 insn[] = {
    0x9000'0010, // adrp x16, .got.plt[n]
    0xf940'0211, // ldr  x17, [x16, .got.plt[n]]
    0x9100'0210, // add  x16, x16, .got.plt[n]
    0xd61f'0220, // br   x17
  };

  u64 gotplt = sym.get_gotplt_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(buf, insn, sizeof(insn));
  write_adrp(buf, page(gotplt) - page(plt));
  *(ul32 *)(buf + 4) |= bits(gotplt, 11, 3) << 10;
  *(ul32 *)(buf + 8) |= (gotplt & 0xfff) << 10;
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const ul32 insn[] = {
    0x9000'0010, // adrp x16, GOT[n]
    0xf940'0211, // ldr  x17, [x16, GOT[n]]
    0xd61f'0220, // br   x17
    0xd503'201f, // nop
  };

  u64 got = sym.get_got_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(buf, insn, sizeof(insn));
  write_adrp(buf, page(got) - page(plt));
  *(ul32 *)(buf + 4) |= bits(got, 11, 3) << 10;
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_AARCH64_ABS64:
    *(ul64 *)loc = val;
    break;
  case R_AARCH64_PREL32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  case R_AARCH64_PREL64:
    *(ul64 *)loc = val - this->shdr.sh_addr - offset;
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

#define S   sym.get_addr(ctx)
#define A   rel.r_addend
#define P   (get_addr() + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_AARCH64_ABS64:
      apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_AARCH64_LDST8_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 0) << 10;
      break;
    case R_AARCH64_LDST16_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 1) << 10;
      break;
    case R_AARCH64_LDST32_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 2) << 10;
      break;
    case R_AARCH64_LDST64_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 3) << 10;
      break;
    case R_AARCH64_LDST128_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 4) << 10;
      break;
    case R_AARCH64_ADD_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 0) << 10;
      break;
    case R_AARCH64_MOVW_UABS_G0: {
      i64 val = S + A;
      check(val, 0, 1 << 16);
      *(ul32 *)loc |= bits(val, 15, 0) << 5;
      break;
    }
    case R_AARCH64_MOVW_UABS_G0_NC:
      *(ul32 *)loc |= bits(S + A, 15, 0) << 5;
      break;
    case R_AARCH64_MOVW_UABS_G1: {
      i64 val = S + A;
      check(val, 0, 1LL << 32);
      *(ul32 *)loc |= bits(val, 31, 16) << 5;
      break;
    }
    case R_AARCH64_MOVW_UABS_G1_NC:
      *(ul32 *)loc |= bits(S + A, 31, 16) << 5;
      break;
    case R_AARCH64_MOVW_UABS_G2: {
      i64 val = S + A;
      check(val, 0, 1LL << 48);
      *(ul32 *)loc |= bits(val, 47, 32) << 5;
      break;
    }
    case R_AARCH64_MOVW_UABS_G2_NC:
      *(ul32 *)loc |= bits(S + A, 47, 32) << 5;
      break;
    case R_AARCH64_MOVW_UABS_G3:
      *(ul32 *)loc |= bits(S + A, 63, 48) << 5;
      break;
    case R_AARCH64_ADR_GOT_PAGE: {
      i64 val = page(G + GOT + A) - page(P);
      check(val, -(1LL << 32), 1LL << 32);
      write_adrp(loc, val);
      break;
    }
    case R_AARCH64_ADR_PREL_PG_HI21: {
      i64 val = page(S + A) - page(P);
      check(val, -(1LL << 32), 1LL << 32);
      write_adrp(loc, val);
      break;
    }
    case R_AARCH64_ADR_PREL_LO21: {
      i64 val = S + A - P;
      check(val, -(1LL << 20), 1LL << 20);
      write_adr(loc, val);
      break;
    }
    case R_AARCH64_CALL26:
    case R_AARCH64_JUMP26: {
      if (sym.is_remaining_undef_weak()) {
        // On ARM, calling an weak undefined symbol jumps to the
        // next instruction.
        *(ul32 *)loc = 0xd503'201f; // nop
        break;
      }

      i64 lo = -(1 << 27);
      i64 hi = 1 << 27;
      i64 val = S + A - P;

      if (val < lo || hi <= val) {
        RangeExtensionRef ref = extra.range_extn[i];
        val = output_section->thunks[ref.thunk_idx]->get_addr(ref.sym_idx) + A - P;
        assert(lo <= val && val < hi);
      }

      *(ul32 *)loc |= (val >> 2) & 0x03ff'ffff;
      break;
    }
    case R_AARCH64_CONDBR19:
    case R_AARCH64_LD_PREL_LO19: {
      i64 val = S + A - P;
      check(val, -(1LL << 20), 1LL << 20);
      *(ul32 *)loc |= bits(val, 20, 2) << 5;
      break;
    }
    case R_AARCH64_PREL16: {
      i64 val = S + A - P;
      check(val, -(1LL << 15), 1LL << 15);
      *(ul16 *)loc = val;
      break;
    }
    case R_AARCH64_PREL32: {
      i64 val = S + A - P;
      check(val, -(1LL << 31), 1LL << 32);
      *(ul32 *)loc = val;
      break;
    }
    case R_AARCH64_PREL64:
      *(ul64 *)loc = S + A - P;
      break;
    case R_AARCH64_LD64_GOT_LO12_NC:
      *(ul32 *)loc |= bits(G + GOT + A, 11, 3) << 10;
      break;
    case R_AARCH64_LD64_GOTPAGE_LO15: {
      i64 val = G + GOT + A - page(GOT);
      check(val, 0, 1 << 15);
      *(ul32 *)loc |= bits(val, 14, 3) << 10;
      break;
    }
    case R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21: {
      i64 val = page(sym.get_gottp_addr(ctx) + A) - page(P);
      check(val, -(1LL << 32), 1LL << 32);
      write_adrp(loc, val);
      break;
    }
    case R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC:
      *(ul32 *)loc |= bits(sym.get_gottp_addr(ctx) + A, 11, 3) << 10;
      break;
    case R_AARCH64_TLSLE_ADD_TPREL_HI12: {
      i64 val = S + A - ctx.tp_addr;
      check(val, 0, 1LL << 24);
      *(ul32 *)loc |= bits(val, 23, 12) << 10;
      break;
    }
    case R_AARCH64_TLSLE_ADD_TPREL_LO12:
    case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC:
      *(ul32 *)loc |= bits(S + A - ctx.tp_addr, 11, 0) << 10;
      break;
    case R_AARCH64_TLSGD_ADR_PAGE21: {
      i64 val = page(sym.get_tlsgd_addr(ctx) + A) - page(P);
      check(val, -(1LL << 32), 1LL << 32);
      write_adrp(loc, val);
      break;
    }
    case R_AARCH64_TLSGD_ADD_LO12_NC:
      *(ul32 *)loc |= bits(sym.get_tlsgd_addr(ctx) + A, 11, 0) << 10;
      break;
    case R_AARCH64_TLSDESC_ADR_PAGE21: {
      if (sym.has_tlsdesc(ctx)) {
        i64 val = page(sym.get_tlsdesc_addr(ctx) + A) - page(P);
        check(val, -(1LL << 32), 1LL << 32);
        write_adrp(loc, val);
      } else {
        // adrp x0, 0 -> movz x0, #tls_ofset_hi, lsl #16
        i64 val = (S + A - ctx.tp_addr);
        check(val, -(1LL << 32), 1LL << 32);
        *(ul32 *)loc = 0xd2a0'0000 | (bits(val, 32, 16) << 5);
      }
      break;
    }
    case R_AARCH64_TLSDESC_LD64_LO12:
      if (sym.has_tlsdesc(ctx)) {
        *(ul32 *)loc |= bits(sym.get_tlsdesc_addr(ctx) + A, 11, 3) << 10;
      } else {
        // ldr x2, [x0] -> movk x0, #tls_ofset_lo
        u32 offset_lo = (S + A - ctx.tp_addr) & 0xffff;
        *(ul32 *)loc = 0xf280'0000 | (offset_lo << 5);
      }
      break;
    case R_AARCH64_TLSDESC_ADD_LO12:
      if (sym.has_tlsdesc(ctx)) {
        *(ul32 *)loc |= bits(sym.get_tlsdesc_addr(ctx) + A, 11, 0) << 10;
      } else {
        // add x0, x0, #0 -> nop
        *(ul32 *)loc = 0xd503'201f;
      }
      break;
    case R_AARCH64_TLSDESC_CALL:
      if (!sym.has_tlsdesc(ctx)) {
        // blr x2 -> nop
        *(ul32 *)loc = 0xd503'201f;
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
    case R_AARCH64_ABS64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A;
      break;
    case R_AARCH64_ABS32: {
      i64 val = S + A;
      check(val, 0, 1LL << 32);
      *(ul32 *)loc = val;
      break;
    }
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
      break;
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
    case R_AARCH64_ABS64:
      scan_rel(ctx, sym, rel, dyn_absrel_table);
      break;
    case R_AARCH64_ADR_GOT_PAGE:
    case R_AARCH64_LD64_GOT_LO12_NC:
    case R_AARCH64_LD64_GOTPAGE_LO15:
      sym.flags |= NEEDS_GOT;
      break;
    case R_AARCH64_CALL26:
    case R_AARCH64_JUMP26:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21:
    case R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_AARCH64_ADR_PREL_PG_HI21:
      scan_rel(ctx, sym, rel, pcrel_table);
      break;
    case R_AARCH64_TLSGD_ADR_PAGE21:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_AARCH64_TLSDESC_ADR_PAGE21:
    case R_AARCH64_TLSDESC_LD64_LO12:
    case R_AARCH64_TLSDESC_ADD_LO12:
      if (!relax_tlsdesc(ctx, sym))
        sym.flags |= NEEDS_TLSDESC;
      break;
    case R_AARCH64_ADD_ABS_LO12_NC:
    case R_AARCH64_ADR_PREL_LO21:
    case R_AARCH64_CONDBR19:
    case R_AARCH64_LD_PREL_LO19:
    case R_AARCH64_LDST16_ABS_LO12_NC:
    case R_AARCH64_LDST32_ABS_LO12_NC:
    case R_AARCH64_LDST64_ABS_LO12_NC:
    case R_AARCH64_LDST128_ABS_LO12_NC:
    case R_AARCH64_LDST8_ABS_LO12_NC:
    case R_AARCH64_MOVW_UABS_G0:
    case R_AARCH64_MOVW_UABS_G0_NC:
    case R_AARCH64_MOVW_UABS_G1:
    case R_AARCH64_MOVW_UABS_G1_NC:
    case R_AARCH64_MOVW_UABS_G2:
    case R_AARCH64_MOVW_UABS_G2_NC:
    case R_AARCH64_MOVW_UABS_G3:
    case R_AARCH64_PREL16:
    case R_AARCH64_PREL32:
    case R_AARCH64_PREL64:
    case R_AARCH64_TLSLE_ADD_TPREL_HI12:
    case R_AARCH64_TLSLE_ADD_TPREL_LO12:
    case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC:
    case R_AARCH64_TLSGD_ADD_LO12_NC:
    case R_AARCH64_TLSDESC_CALL:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <>
void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;

  static const ul32 data[] = {
    0x9000'0010, // adrp x16, 0   # R_AARCH64_ADR_PREL_PG_HI21
    0x9100'0210, // add  x16, x16 # R_AARCH64_ADD_ABS_LO12_NC
    0xd61f'0200, // br   x16
  };

  static_assert(E::thunk_size == sizeof(data));

  for (i64 i = 0; i < symbols.size(); i++) {
    u64 S = symbols[i]->get_addr(ctx);
    u64 P = output_section.shdr.sh_addr + offset + i * E::thunk_size;

    u8 *loc = buf + i * E::thunk_size;
    memcpy(loc , data, sizeof(data));
    write_adrp(loc, page(S) - page(P));
    *(ul32 *)(loc + 4) |= bits(S, 11, 0) << 10;
  }
}

} // namespace mold::elf
