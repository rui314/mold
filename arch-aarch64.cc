#include "mold.h"

static void write_addr(u8 *buf, u64 val) {
  u32 hi = (val & 0x1ffffc) << 3;
  u32 lo = (val & 3) << 29;
  *(u32 *)buf = (*(u32 *)buf & 0x9f00001f) | hi | lo;
}

static u64 extract(u64 val, u64 hi, u64 lo) {
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
  write_addr(buf + 4, (page(gotplt) - page(plt + 4)) >> 12);
  *(u32 *)(buf + 8) |= extract(gotplt, 11, 3) << 10;
  *(u32 *)(buf + 12) |= ((gotplt) & 0xfff) << 10;
}

static void write_plt_entry(Context<AARCH64> &ctx, u8 *buf, Symbol<AARCH64> *sym) {
  u8 *ent = buf + sym->get_plt_idx(ctx) * AARCH64::plt_size;

  static const u8 data[] = {
    0x10, 0x00, 0x00, 0x90, // adrp x16, .got.plt[n]
    0x11, 0x02, 0x40, 0xf9, // ldr  x17, [x16, .got.plt[n]]
    0x10, 0x02, 0x00, 0x91, // add  x16, x16, .got.plt[n]
    0x20, 0x02, 0x1f, 0xd6, // br   x17
  };

  u64 gotplt = sym->get_gotplt_addr(ctx);
  u64 plt = sym->get_plt_addr(ctx);

  memcpy(ent, data, sizeof(data));
  write_addr(ent, (page(gotplt) - page(plt)) >> 12);
  *(u32 *)(ent + 4) |= extract(gotplt, 11, 3) << 10;
  *(u32 *)(ent + 8) |= (gotplt & 0xfff) << 10;
}

template <>
void PltSection<AARCH64>::copy_buf(Context<AARCH64> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  write_plt_header(ctx, buf);
  for (Symbol<AARCH64> *sym : symbols)
    write_plt_entry(ctx, buf, sym);
}

template <>
void PltGotSection<AARCH64>::copy_buf(Context<AARCH64> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  for (Symbol<AARCH64> *sym : symbols) {
    u8 *ent = buf + sym->get_pltgot_idx(ctx) * AARCH64::pltgot_size;

    static const u8 data[] = {
      0x10, 0x00, 0x00, 0x90, // adrp x16, GOT[n]
      0x11, 0x02, 0x40, 0xf9, // ldr  x17, [x16, GOT[n]]
      0x10, 0x02, 0x00, 0x91, // add  x16, x16, GOT[n]
      0x20, 0x02, 0x1f, 0xd6, // br   x17
    };

    u64 got = sym->get_got_addr(ctx);
    u64 plt = sym->get_plt_addr(ctx);

    memcpy(ent, data, sizeof(data));
    write_addr(ent, (page(got) - page(plt)) >> 12);
    *(u32 *)(ent + 4) |= extract(got, 11, 3) << 10;
    *(u32 *)(ent + 8) |= (got & 0xfff) << 10;
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
    *(u32 *)(base + loc) = val;
    return;
  }
  unreachable(ctx);
}

template <>
void InputSection<AARCH64>::apply_reloc_alloc(Context<AARCH64> &ctx, u8 *base) {
  ElfRel<AARCH64> *dynrel = nullptr;
  std::span<ElfRel<AARCH64>> rels = get_rels(ctx);
  i64 frag_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<AARCH64> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                                 file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<AARCH64> &rel = rels[i];
    if (rel.r_type == R_AARCH64_NONE)
      continue;

    Symbol<AARCH64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<AARCH64> *ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      ref = &rel_fragments[frag_idx++];

#define S   (ref ? ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (ref ? ref->addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel_exprs[i]) {
    case R_BASEREL:
      *dynrel++ = {P, R_AARCH64_RELATIVE, 0, (i64)(S + A)};
      *(u64 *)loc = S + A;
      continue;
    case R_DYN:
      *dynrel++ = {P, R_AARCH64_ABS64, (u32)sym.get_dynsym_idx(ctx), A};
      *(u64 *)loc = A;
      continue;
    }

    switch (rel.r_type) {
    case R_AARCH64_ABS64:
      *(u64 *)loc = S + A;
      continue;
    case R_AARCH64_LDST8_ABS_LO12_NC:
      *(u32 *)loc |= extract(S + A, 11, 0) << 10;
      continue;
    case R_AARCH64_LDST32_ABS_LO12_NC:
      *(u32 *)loc |= extract(S + A, 11, 2) << 10;
      continue;
    case R_AARCH64_LDST64_ABS_LO12_NC:
      *(u32 *)loc |= extract(S + A, 11, 3) << 10;
      continue;
    case R_AARCH64_ADD_ABS_LO12_NC:
      *(u32 *)loc |= extract(S + A, 11, 0) << 10;
      continue;
    case R_AARCH64_MOVW_UABS_G0_NC:
      *(u32 *)loc |= extract(S + A, 15, 0) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G1_NC:
      *(u32 *)loc |= extract(S + A, 31, 16) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G2_NC:
      *(u32 *)loc |= extract(S + A, 47, 32) << 5;
      continue;
    case R_AARCH64_MOVW_UABS_G3:
      *(u32 *)loc |= extract(S + A, 63, 48) << 5;
      continue;
    case R_AARCH64_ADR_GOT_PAGE:
      write_addr(loc, extract(page(GOT + A) - page(P), 32, 12));
      continue;
    case R_AARCH64_ADR_PREL_PG_HI21:
      write_addr(loc, extract(page(S + A) - page(P), 32, 12));
      continue;
    case R_AARCH64_CALL26:
    case R_AARCH64_JUMP26:
      if (sym.esym().is_undef_weak())
        *(u32 *)loc |= 1;
      else
        *(u32 *)loc |= ((S + A - P) >> 2) & 0x3ffffff;
      continue;
    case R_AARCH64_PREL32:
      *(u32 *)loc = S + A - P;
      continue;
    case R_AARCH64_LD64_GOT_LO12_NC:
      *(u32 *)loc |= extract(G + GOT + A, 11, 3) << 10;
      continue;
    case R_AARCH64_LD64_GOTPAGE_LO15:
      *(u32 *)loc |= extract(G + GOT + A - page(GOT), 14, 3) << 10;
      continue;
    case R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21:
      write_addr(loc, (page(sym.get_gottp_addr(ctx) + A) - page(P)) >> 12);
      continue;
    case R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC:
      *(u32 *)loc |= extract(sym.get_gottp_addr(ctx) + A, 11, 3) << 10;
      continue;
    case R_AARCH64_TLSLE_ADD_TPREL_HI12:
      *(u32 *)loc |= extract(S + A - ctx.tls_begin + 16, 23, 12) << 10;
      continue;
    case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC:
      *(u32 *)loc |= extract(S + A - ctx.tls_begin + 16, 11, 0) << 10;
      continue;
    case R_AARCH64_TLSDESC_ADR_PAGE21:
      write_addr(loc, (page(sym.get_tlsdesc_addr(ctx) + A) - page(P)) >> 12);
      continue;
    case R_AARCH64_TLSDESC_LD64_LO12:
      *(u32 *)loc |= extract(sym.get_tlsdesc_addr(ctx) + A, 11, 3) << 10;
      continue;
    case R_AARCH64_TLSDESC_ADD_LO12:
      *(u32 *)loc |= extract(sym.get_tlsdesc_addr(ctx) + A, 11, 0) << 10;
      continue;
    case R_AARCH64_TLSDESC_CALL:
      continue;
    default:
      unreachable(ctx);
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
  i64 frag_idx = 0;

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

    const SectionFragmentRef<AARCH64> *ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      ref = &rel_fragments[frag_idx++];

#define S   (ref ? ref->frag->get_addr(ctx) : sym.get_addr(ctx))
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
                 << rel_to_string<X86_64>(rel.r_type);
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
  ASSERT(shdr.sh_flags & SHF_ALLOC);

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
        {  NONE,     NONE,    DYNREL,        DYNREL },     // PDE
      };
      dispatch(ctx, table, i);
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
        {  NONE,     NONE,    ERROR,         PLT   },     // PIE
        {  NONE,     NONE,    COPYREL,       PLT   },       // PDE
      };
      dispatch(ctx, table, i);
      break;
    }
    case R_AARCH64_TLSDESC_ADR_PAGE21:
    case R_AARCH64_TLSDESC_LD64_LO12:
    case R_AARCH64_TLSDESC_ADD_LO12:
      sym.flags |= NEEDS_TLSDESC;
      break;
    case R_AARCH64_ADD_ABS_LO12_NC:
    case R_AARCH64_LDST64_ABS_LO12_NC:
    case R_AARCH64_LDST32_ABS_LO12_NC:
    case R_AARCH64_LDST8_ABS_LO12_NC:
    case R_AARCH64_MOVW_UABS_G0_NC:
    case R_AARCH64_MOVW_UABS_G1_NC:
    case R_AARCH64_MOVW_UABS_G2_NC:
    case R_AARCH64_MOVW_UABS_G3:
    case R_AARCH64_PREL32:
    case R_AARCH64_TLSLE_ADD_TPREL_HI12:
    case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC:
    case R_AARCH64_TLSDESC_CALL:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: "
                 << rel_to_string<AARCH64>(rel.r_type);
    }
  }
}
