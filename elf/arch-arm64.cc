#include "mold.h"

namespace mold::elf {

using E = ARM64;

static void write_adrp(u8 *buf, u64 val) {
  u32 hi = bits(val, 32, 14);
  u32 lo = bits(val, 13, 12);
  u32 op = *(ul32 *)buf & 0b1001'1111'0000'0000'0000'0000'0001'1111;
  *(ul32 *)buf = (lo << 29) | (hi << 5) | op;
}

static void write_adr(u8 *buf, u64 val) {
  u32 hi = bits(val, 20, 2);
  u32 lo = bits(val, 1, 0);
  u32 op = *(ul32 *)buf & 0b1001'1111'0000'0000'0000'0000'0001'1111;
  *(ul32 *)buf = (lo << 29) | (hi << 5) | op;
}

static u64 page(u64 val) {
  return val & ~(u64)0xfff;
}

static void write_plt_header(Context<E> &ctx, u8 *buf) {
  // Write PLT header
  static const u32 plt0[] = {
    0xa9bf7bf0, // stp  x16, x30, [sp,#-16]!
    0x90000010, // adrp x16, .got.plt[2]
    0xf9400211, // ldr  x17, [x16, .got.plt[2]]
    0x91000210, // add  x16, x16, .got.plt[2]
    0xd61f0220, // br   x17
    0xd503201f, // nop
    0xd503201f, // nop
    0xd503201f, // nop
  };

  u64 gotplt = ctx.gotplt->shdr.sh_addr + 16;
  u64 plt = ctx.plt->shdr.sh_addr;

  memcpy(buf, plt0, sizeof(plt0));
  write_adrp(buf + 4, page(gotplt) - page(plt + 4));
  *(ul32 *)(buf + 8) |= bits(gotplt, 11, 3) << 10;
  *(ul32 *)(buf + 12) |= (gotplt & 0xfff) << 10;
}

static void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  u8 *ent = buf + E::plt_hdr_size + sym.get_plt_idx(ctx) * E::plt_size;

  static const u32 data[] = {
    0x90000010, // adrp x16, .got.plt[n]
    0xf9400211, // ldr  x17, [x16, .got.plt[n]]
    0x91000210, // add  x16, x16, .got.plt[n]
    0xd61f0220, // br   x17
  };

  u64 gotplt = sym.get_gotplt_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(ent, data, sizeof(data));
  write_adrp(ent, page(gotplt) - page(plt));
  *(ul32 *)(ent + 4) |= bits(gotplt, 11, 3) << 10;
  *(ul32 *)(ent + 8) |= (gotplt & 0xfff) << 10;
}

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  write_plt_header(ctx, buf);
  for (Symbol<E> *sym : symbols)
    write_plt_entry(ctx, buf, *sym);
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + sym->get_pltgot_idx(ctx) * ARM64::pltgot_size;

    static const u32 data[] = {
      0x90000010, // adrp x16, GOT[n]
      0xf9400211, // ldr  x17, [x16, GOT[n]]
      0xd61f0220, // br   x17
      0xd503201f, // nop
    };

    u64 got = sym->get_got_addr(ctx);
    u64 plt = sym->get_plt_addr(ctx);

    memcpy(ent, data, sizeof(data));
    write_adrp(ent, page(got) - page(plt));
    *(ul32 *)(ent + 4) |= bits(got, 11, 3) << 10;
  }
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_AARCH64_ABS64:
    *(ul64 *)loc = val;
    return;
  case R_AARCH64_PREL32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    return;
  case R_AARCH64_PREL64:
    *(ul64 *)loc = val - this->shdr.sh_addr - offset;
    return;
  }
  Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  ElfRel<E> *dynrel = nullptr;
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  i64 frag_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                           file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_AARCH64_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<E> *frag_ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      frag_ref = &rel_fragments[frag_idx++];

    auto overflow_check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   (frag_ref ? frag_ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag_ref ? (u64)frag_ref->addend : (u64)rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_AARCH64_ABS64:
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      continue;
    case R_AARCH64_LDST8_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 0) << 10;
      continue;
    case R_AARCH64_LDST16_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 1) << 10;
      continue;
    case R_AARCH64_LDST32_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 2) << 10;
      continue;
    case R_AARCH64_LDST64_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 3) << 10;
      continue;
    case R_AARCH64_LDST128_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 4) << 10;
      continue;
    case R_AARCH64_ADD_ABS_LO12_NC:
      *(ul32 *)loc |= bits(S + A, 11, 0) << 10;
      continue;
    case R_AARCH64_MOVW_UABS_G0:
    case R_AARCH64_MOVW_UABS_G0_NC:
      *(ul32 *)loc |= bits(S + A, 15, 0) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G1:
    case R_AARCH64_MOVW_UABS_G1_NC:
      *(ul32 *)loc |= bits(S + A, 31, 16) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G2:
    case R_AARCH64_MOVW_UABS_G2_NC:
      *(ul32 *)loc |= bits(S + A, 47, 32) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G3:
      *(ul32 *)loc |= bits(S + A, 63, 48) << 5;
      continue;
    case R_AARCH64_ADR_GOT_PAGE: {
      i64 val = page(G + GOT + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adrp(loc, val);
      continue;
    }
    case R_AARCH64_ADR_PREL_PG_HI21: {
      i64 val = page(S + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adrp(loc, val);
      continue;
    }
    case R_AARCH64_ADR_PREL_LO21: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 20), (i64)1 << 20);
      write_adr(loc, val);
      continue;
    }
    case R_AARCH64_CALL26:
    case R_AARCH64_JUMP26: {
      if (sym.is_remaining_undef_weak()) {
        // On ARM, calling an weak undefined symbol jumps to the
        // next instruction.
        *(ul32 *)loc = 0xd503201f; // nop
        continue;
      }

      i64 lo = -(1 << 27);
      i64 hi = 1 << 27;
      i64 val = S + A - P;

      if (val < lo || hi <= val) {
        RangeExtensionRef ref = extra.range_extn[i];
        val = output_section->thunks[ref.thunk_idx]->get_addr(ref.sym_idx) + A - P;
        assert(lo <= val && val < hi);
      }

      *(ul32 *)loc |= (val >> 2) & 0x3ffffff;
      continue;
    }
    case R_AARCH64_CONDBR19:
    case R_AARCH64_LD_PREL_LO19: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 20), (i64)1 << 20);
      *(ul32 *)loc |= bits(val, 20, 2) << 5;
      continue;
    }
    case R_AARCH64_PREL16: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 15), (i64)1 << 15);
      *(ul16 *)loc = val;
      continue;
    }
    case R_AARCH64_PREL32: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 31), (i64)1 << 32);
      *(ul32 *)loc = val;
      continue;
    }
    case R_AARCH64_PREL64:
      *(ul64 *)loc = S + A - P;
      continue;
    case R_AARCH64_LD64_GOT_LO12_NC:
      *(ul32 *)loc |= bits(G + GOT + A, 11, 3) << 10;
      continue;
    case R_AARCH64_LD64_GOTPAGE_LO15: {
      i64 val = G + GOT + A - page(GOT);
      overflow_check(val, 0, 1 << 15);
      *(ul32 *)loc |= bits(val, 14, 3) << 10;
      continue;
    }
    case R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21: {
      i64 val = page(sym.get_gottp_addr(ctx) + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adrp(loc, val);
      continue;
    }
    case R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC:
      *(ul32 *)loc |= bits(sym.get_gottp_addr(ctx) + A, 11, 3) << 10;
      continue;
    case R_AARCH64_TLSLE_ADD_TPREL_HI12: {
      i64 val = S + A - ctx.tls_begin + E::tls_tp_offset;
      overflow_check(val, 0, (i64)1 << 24);
      *(ul32 *)loc |= bits(val, 23, 12) << 10;
      continue;
    }
    case R_AARCH64_TLSLE_ADD_TPREL_LO12:
    case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC:
      *(ul32 *)loc |= bits(S + A - ctx.tls_begin + E::tls_tp_offset, 11, 0) << 10;
      continue;
    case R_AARCH64_TLSGD_ADR_PAGE21: {
      i64 val = page(sym.get_tlsgd_addr(ctx) + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adrp(loc, val);
      continue;
    }
    case R_AARCH64_TLSGD_ADD_LO12_NC:
      *(ul32 *)loc |= bits(sym.get_tlsgd_addr(ctx) + A, 11, 0) << 10;
      continue;
    case R_AARCH64_TLSDESC_ADR_PAGE21: {
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // adrp x0, 0 -> movz x0, #tls_ofset_hi, lsl #16
        i64 val = (S + A - ctx.tls_begin + E::tls_tp_offset);
        overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
        *(ul32 *)loc = 0xd2a00000 | (bits(val, 32, 16) << 5);
      } else {
        i64 val = page(sym.get_tlsdesc_addr(ctx) + A) - page(P);
        overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
        write_adrp(loc, val);
      }
      continue;
    }
    case R_AARCH64_TLSDESC_LD64_LO12:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // ldr x2, [x0] -> movk x0, #tls_ofset_lo
        u32 offset_lo = (S + A - ctx.tls_begin + E::tls_tp_offset) & 0xffff;
        *(ul32 *)loc = 0xf2800000 | (offset_lo << 5);
      } else {
        *(ul32 *)loc |= bits(sym.get_tlsdesc_addr(ctx) + A, 11, 3) << 10;
      }
      continue;
    case R_AARCH64_TLSDESC_ADD_LO12:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // add x0, x0, #0 -> nop
        *(ul32 *)loc = 0xd503201f;
      } else {
        *(ul32 *)loc |= bits(sym.get_tlsdesc_addr(ctx) + A, 11, 0) << 10;
      }
      continue;
    case R_AARCH64_TLSDESC_CALL:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // blr x2 -> nop
        *(ul32 *)loc = 0xd503201f;
      }
      continue;
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
    if (rel.r_type == R_AARCH64_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      record_undef_error(ctx, rel);
      continue;
    }

    SectionFragment<E> *frag;
    i64 addend;
    std::tie(frag, addend) = get_fragment(ctx, rel);

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? (u64)addend : (u64)rel.r_addend)

    switch (rel.r_type) {
    case R_AARCH64_ABS64:
      if (!frag) {
        if (std::optional<u64> val = get_tombstone(sym)) {
          *(ul64 *)loc = *val;
          break;
        }
      }
      *(ul64 *)loc = S + A;
      continue;
    case R_AARCH64_ABS32:
      *(ul32 *)loc = S + A;
      continue;
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
    if (rel.r_type == R_AARCH64_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (!sym.file) {
      record_undef_error(ctx, rel);
      continue;
    }

    if (sym.get_type() == STT_GNU_IFUNC)
      sym.flags |= (NEEDS_GOT | NEEDS_PLT);

    switch (rel.r_type) {
    case R_AARCH64_ABS64:
      scan_abs_dyn_rel(ctx, sym, rel);
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
      scan_pcrel_rel(ctx, sym, rel);
      break;
    case R_AARCH64_TLSGD_ADR_PAGE21:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_AARCH64_TLSDESC_ADR_PAGE21:
    case R_AARCH64_TLSDESC_LD64_LO12:
    case R_AARCH64_TLSDESC_ADD_LO12:
      if (!ctx.relax_tlsdesc || sym.is_imported)
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

// For range extension thunks
template <>
bool is_reachable(Context<E> &ctx, Symbol<E> &sym,
                  InputSection<E> &isec, const ElfRel<E> &rel) {
  // We pessimistically assume that PLT entries are unreacahble.
  if (sym.has_plt(ctx))
    return false;

  // We create thunks with a pessimistic assumption that all
  // out-of-section relocations would be out-of-range.
  InputSection<E> *isec2 = sym.get_input_section();
  if (!isec2 || isec.output_section != isec2->output_section)
    return false;

  if (isec2->offset == -1)
    return false;

  // Compute a distance between the relocated place and the symbol
  // and check if they are within reach.
  i64 S = sym.get_addr(ctx);
  i64 A = rel.r_addend;
  i64 P = isec.get_addr() + rel.r_offset;
  i64 val = S + A - P;
  return -(1 << 27) <= val && val < (1 << 27);
}

template <>
void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;

  static const u32 data[] = {
    0x90000010, // adrp x16, 0   # R_AARCH64_ADR_PREL_PG_HI21
    0x91000210, // add  x16, x16 # R_AARCH64_ADD_ABS_LO12_NC
    0xd61f0200, // br   x16
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
