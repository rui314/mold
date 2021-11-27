#include "mold.h"

namespace mold::elf {

static void write_adr(u8 *buf, u64 val) {
  u32 hi = (val & 0x1ffffc) << 3;
  u32 lo = (val & 3) << 29;
  *(u32 *)buf = (*(u32 *)buf & 0x9f00001f) | hi | lo;
}

// Returns [hi:lo] bits of val.
static u64 bits(u64 val, u64 hi, u64 lo) {
  return (val >> lo) & (((u64)1 << (hi - lo + 1)) - 1);
}

static u64 page(u64 val) {
  return val & ~(u64)0xfff;
}

template <>
void GotPltSection<AARCH64>::copy_buf(Context<AARCH64> &ctx) {
  u64 *buf = (u64 *)(ctx.buf + this->shdr.sh_offset);

  // The first slot of .got.plt points to _DYNAMIC.
  buf[0] = ctx.dynamic ? ctx.dynamic->shdr.sh_addr : 0;
  buf[1] = 0;
  buf[2] = 0;

  for (Symbol<AARCH64> *sym : ctx.plt->symbols)
    buf[sym->get_gotplt_idx(ctx)] = ctx.plt->shdr.sh_addr;
}

static void write_plt_header(Context<AARCH64> &ctx, u8 *buf) {
  // Write PLT header
  static const u8 plt0[] = {
    0xf0, 0x7b, 0xbf, 0xa9, // stp    x16, x30, [sp,#-16]!
    0x10, 0x00, 0x00, 0x90, // adrp   x16, .got.plt[2]
    0x11, 0x02, 0x40, 0xf9, // ldr    x17, [x16, .got.plt[2]]
    0x10, 0x02, 0x00, 0x91, // add    x16, x16, .got.plt[2]
    0x20, 0x02, 0x1f, 0xd6, // br     x17
    0x1f, 0x20, 0x03, 0xd5, // nop
    0x1f, 0x20, 0x03, 0xd5, // nop
    0x1f, 0x20, 0x03, 0xd5, // nop
  };

  u64 gotplt = ctx.gotplt->shdr.sh_addr + 16;
  u64 plt = ctx.plt->shdr.sh_addr;

  memcpy(buf, plt0, sizeof(plt0));
  write_adr(buf + 4, bits(page(gotplt) - page(plt + 4), 32, 12));
  *(u32 *)(buf + 8) |= bits(gotplt, 11, 3) << 10;
  *(u32 *)(buf + 12) |= ((gotplt) & 0xfff) << 10;
}

static void write_plt_entry(Context<AARCH64> &ctx, u8 *buf, Symbol<AARCH64> &sym) {
  u8 *ent = buf + sym.get_plt_idx(ctx) * AARCH64::plt_size;

  static const u8 data[] = {
    0x10, 0x00, 0x00, 0x90, // adrp x16, .got.plt[n]
    0x11, 0x02, 0x40, 0xf9, // ldr  x17, [x16, .got.plt[n]]
    0x10, 0x02, 0x00, 0x91, // add  x16, x16, .got.plt[n]
    0x20, 0x02, 0x1f, 0xd6, // br   x17
  };

  u64 gotplt = sym.get_gotplt_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(ent, data, sizeof(data));
  write_adr(ent, bits(page(gotplt) - page(plt), 32, 12));
  *(u32 *)(ent + 4) |= bits(gotplt, 11, 3) << 10;
  *(u32 *)(ent + 8) |= (gotplt & 0xfff) << 10;
}

template <>
void PltSection<AARCH64>::copy_buf(Context<AARCH64> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  write_plt_header(ctx, buf);
  for (Symbol<AARCH64> *sym : symbols)
    write_plt_entry(ctx, buf, *sym);
}

template <>
void PltGotSection<AARCH64>::copy_buf(Context<AARCH64> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  for (Symbol<AARCH64> *sym : symbols) {
    u8 *ent = buf + sym->get_pltgot_idx(ctx) * AARCH64::pltgot_size;

    static const u8 data[] = {
      0x10, 0x00, 0x00, 0x90, // adrp x16, GOT[n]
      0x11, 0x02, 0x40, 0xf9, // ldr  x17, [x16, GOT[n]]
      0x20, 0x02, 0x1f, 0xd6, // br   x17
      0x1f, 0x20, 0x03, 0xd5, // nop
    };

    u64 got = sym->get_got_addr(ctx);
    u64 plt = sym->get_plt_addr(ctx);

    memcpy(ent, data, sizeof(data));
    write_adr(ent, bits(page(got) - page(plt), 32, 12));
    *(u32 *)(ent + 4) |= bits(got, 11, 3) << 10;
  }
}

template <>
void EhFrameSection<AARCH64>::apply_reloc(Context<AARCH64> &ctx,
                                          ElfRel<AARCH64> &rel,
                                          u64 loc, u64 val) {
  u8 *base = ctx.buf + this->shdr.sh_offset;

  switch (rel.r_type) {
  case R_AARCH64_ABS64:
    *(u64 *)(base + loc) = val;
    return;
  case R_AARCH64_PREL32:
    *(u32 *)(base + loc) = val - this->shdr.sh_addr - loc;
    return;
  case R_AARCH64_PREL64:
    *(u64 *)(base + loc) = val - this->shdr.sh_addr - loc;
    return;
  }
  Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
}

template <>
void InputSection<AARCH64>::apply_reloc_alloc(Context<AARCH64> &ctx, u8 *base) {
  ElfRel<AARCH64> *dynrel = nullptr;
  std::span<ElfRel<AARCH64>> rels = get_rels(ctx);
  i64 subsec_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<AARCH64> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                                 file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<AARCH64> &rel = rels[i];
    if (rel.r_type == R_AARCH64_NONE)
      continue;

    Symbol<AARCH64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SubsectionRef<AARCH64> *ref = nullptr;
    if (rel_subsections && rel_subsections[subsec_idx].idx == i)
      ref = &rel_subsections[subsec_idx++];

    auto overflow_check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   (ref ? ref->subsec->get_addr(ctx) : sym.get_addr(ctx))
#define A   (ref ? ref->addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    if (needs_dynrel[i]) {
      *dynrel++ = {P, R_AARCH64_ABS64, (u32)sym.get_dynsym_idx(ctx), A};
      *(u64 *)loc = A;
      continue;
    }

    if (needs_baserel[i]) {
      *dynrel++ = {P, R_AARCH64_RELATIVE, 0, (i64)(S + A)};
      *(u64 *)loc = S + A;
      continue;
    }

    switch (rel.r_type) {
    case R_AARCH64_ABS64:
      *(u64 *)loc = S + A;
      continue;
    case R_AARCH64_LDST8_ABS_LO12_NC:
      *(u32 *)loc |= bits(S + A, 11, 0) << 10;
      continue;
    case R_AARCH64_LDST16_ABS_LO12_NC:
      *(u32 *)loc |= bits(S + A, 11, 1) << 10;
      continue;
    case R_AARCH64_LDST32_ABS_LO12_NC:
      *(u32 *)loc |= bits(S + A, 11, 2) << 10;
      continue;
    case R_AARCH64_LDST64_ABS_LO12_NC:
      *(u32 *)loc |= bits(S + A, 11, 3) << 10;
      continue;
    case R_AARCH64_LDST128_ABS_LO12_NC:
      *(u32 *)loc |= bits(S + A, 11, 4) << 10;
      continue;
    case R_AARCH64_ADD_ABS_LO12_NC:
      *(u32 *)loc |= bits(S + A, 11, 0) << 10;
      continue;
    case R_AARCH64_MOVW_UABS_G0_NC:
      *(u32 *)loc |= bits(S + A, 15, 0) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G1_NC:
      *(u32 *)loc |= bits(S + A, 31, 16) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G2_NC:
      *(u32 *)loc |= bits(S + A, 47, 32) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G3:
      *(u32 *)loc |= bits(S + A, 63, 48) << 5;
      continue;
    case R_AARCH64_ADR_GOT_PAGE: {
      i64 val = page(G + GOT + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adr(loc, bits(val, 32, 12));
      continue;
    }
    case R_AARCH64_ADR_PREL_PG_HI21: {
      i64 val = page(S + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adr(loc, bits(val, 32, 12));
      continue;
    }
    case R_AARCH64_ADR_PREL_LO21: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 20), (i64)1 << 20);
      write_adr(loc, val);
      continue;
    }
    case R_AARCH64_CALL26:
    case R_AARCH64_JUMP26:
      if (!sym.esym().is_undef_weak()) {
        i64 val = S + A - P;
        overflow_check(val, -((i64)1 << 26), (i64)1 << 26);
        *(u32 *)loc |= (val >> 2) & 0x3ffffff;
      } else {
        // On ARM, calling an weak undefined symbol jumps to the
        // next instruction.
        *(u32 *)loc |= 1;
      }
      continue;
    case R_AARCH64_CONDBR19: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 20), (i64)1 << 20);
      *(u32 *)loc |= bits(val, 20, 2) << 5;
      continue;
    }
    case R_AARCH64_PREL16: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 15), (i64)1 << 15);
      *(u16 *)loc = val;
      continue;
    }
    case R_AARCH64_PREL32: {
      i64 val = S + A - P;
      overflow_check(val, -((i64)1 << 31), (i64)1 << 32);
      *(u32 *)loc = val;
      continue;
    }
    case R_AARCH64_PREL64:
      *(u64 *)loc = S + A - P;
      continue;
    case R_AARCH64_LD64_GOT_LO12_NC:
      *(u32 *)loc |= bits(G + GOT + A, 11, 3) << 10;
      continue;
    case R_AARCH64_LD64_GOTPAGE_LO15: {
      i64 val = G + GOT + A - page(GOT);
      overflow_check(val, 0, 1 << 15);
      *(u32 *)loc |= bits(val, 14, 3) << 10;
      continue;
    }
    case R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21: {
      i64 val = page(sym.get_gottp_addr(ctx) + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adr(loc, bits(val, 32, 12));
      continue;
    }
    case R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC:
      *(u32 *)loc |= bits(sym.get_gottp_addr(ctx) + A, 11, 3) << 10;
      continue;
    case R_AARCH64_TLSLE_ADD_TPREL_HI12: {
      i64 val = S + A - ctx.tls_begin + 16;
      overflow_check(val, 0, (i64)1 << 24);
      *(u32 *)loc |= bits(val, 23, 12) << 10;
      continue;
    }
    case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC:
      *(u32 *)loc |= bits(S + A - ctx.tls_begin + 16, 11, 0) << 10;
      continue;
    case R_AARCH64_TLSGD_ADR_PAGE21: {
      i64 val = page(sym.get_tlsgd_addr(ctx) + A) - page(P);
      overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
      write_adr(loc, bits(val, 32, 12));
      continue;
    }
    case R_AARCH64_TLSGD_ADD_LO12_NC:
      *(u32 *)loc |= bits(sym.get_tlsgd_addr(ctx) + A, 11, 0) << 10;
      continue;
    case R_AARCH64_TLSDESC_ADR_PAGE21: {
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // adrp x0, 0 -> movz x0, #tls_ofset_hi, lsl #16
        i64 val = (S + A - ctx.tls_begin + 16);
        overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
        *(u32 *)loc = 0xd2a00000 | (bits(val, 32, 16) << 5);
      } else {
        i64 val = page(sym.get_tlsdesc_addr(ctx) + A) - page(P);
        overflow_check(val, -((i64)1 << 32), (i64)1 << 32);
        write_adr(loc, bits(val, 32, 12));
      }
      continue;
    }
    case R_AARCH64_TLSDESC_LD64_LO12:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // ldr x2, [x0] -> movk x0, #tls_ofset_lo
        u32 offset_lo = (S + A - ctx.tls_begin + 16) & 0xffff;
        *(u32 *)loc = 0xf2800000 | (offset_lo << 5);
      } else {
        *(u32 *)loc |= bits(sym.get_tlsdesc_addr(ctx) + A, 11, 3) << 10;
      }
      continue;
    case R_AARCH64_TLSDESC_ADD_LO12:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // add x0, x0, #0 -> nop
        *(u32 *)loc = 0xd503201f;
      } else {
        *(u32 *)loc |= bits(sym.get_tlsdesc_addr(ctx) + A, 11, 0) << 10;
      }
      continue;
    case R_AARCH64_TLSDESC_CALL:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // blr x2 -> nop
        *(u32 *)loc = 0xd503201f;
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
void InputSection<AARCH64>::apply_reloc_nonalloc(Context<AARCH64> &ctx, u8 *base) {
  std::span<ElfRel<AARCH64>> rels = get_rels(ctx);
  i64 subsec_idx = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<AARCH64> &rel = rels[i];
    if (rel.r_type == R_AARCH64_NONE)
      continue;

    Symbol<AARCH64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      report_undef(ctx, sym);
      continue;
    }

    const SubsectionRef<AARCH64> *ref = nullptr;
    if (rel_subsections && rel_subsections[subsec_idx].idx == i)
      ref = &rel_subsections[subsec_idx++];

#define S   (ref ? ref->subsec->get_addr(ctx) : sym.get_addr(ctx))
#define A   (ref ? ref->addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_AARCH64_ABS64:
      *(u64 *)loc = S + A;
      continue;
    case R_AARCH64_ABS32:
      *(u32 *)loc = S + A;
      continue;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
      break;
    }

#undef S
#undef A
#undef P
#undef G
#undef GOT
  }
}

template <>
void InputSection<AARCH64>::scan_relocations(Context<AARCH64> &ctx) {
  assert(shdr.sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<AARCH64>);
  std::span<ElfRel<AARCH64>> rels = get_rels(ctx);
  bool is_writable = (shdr.sh_flags & SHF_WRITE);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<AARCH64> &rel = rels[i];
    if (rel.r_type == R_AARCH64_NONE)
      continue;

    Symbol<AARCH64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (!sym.file) {
      report_undef(ctx, sym);
      continue;
    }

    if (sym.get_type() == STT_GNU_IFUNC) {
      sym.flags |= NEEDS_GOT;
      sym.flags |= NEEDS_PLT;
    }

    switch (rel.r_type) {
    case R_AARCH64_ABS64: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    COPYREL,       PLT    },     // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
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
    case R_AARCH64_ADR_PREL_PG_HI21: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     NONE,    ERROR,         ERROR },      // DSO
        {  NONE,     NONE,    ERROR,         PLT   },      // PIE
        {  NONE,     NONE,    COPYREL,       PLT   },      // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
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
    case R_AARCH64_LDST16_ABS_LO12_NC:
    case R_AARCH64_LDST32_ABS_LO12_NC:
    case R_AARCH64_LDST64_ABS_LO12_NC:
    case R_AARCH64_LDST128_ABS_LO12_NC:
    case R_AARCH64_LDST8_ABS_LO12_NC:
    case R_AARCH64_MOVW_UABS_G0_NC:
    case R_AARCH64_MOVW_UABS_G1_NC:
    case R_AARCH64_MOVW_UABS_G2_NC:
    case R_AARCH64_MOVW_UABS_G3:
    case R_AARCH64_PREL16:
    case R_AARCH64_PREL32:
    case R_AARCH64_PREL64:
    case R_AARCH64_TLSLE_ADD_TPREL_HI12:
    case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC:
    case R_AARCH64_TLSGD_ADD_LO12_NC:
    case R_AARCH64_TLSDESC_CALL:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

} // namespace mold::elf
