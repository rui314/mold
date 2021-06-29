#include "mold.h"

enum {
  R_TLS_GOTIE = R_END + 1,
  R_TLS_LE,
  R_TLS_GD,
  R_TLS_LD,
  R_TLS_GOTDESC,
  R_TLS_GOTDESC_RELAX,
  R_TLS_DESC_CALL_RELAX,
  R_TPOFF,
};

template <>
void PltSection<I386>::copy_buf(Context<I386> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  // Write PLT header
  if (ctx.arg.pic) {
    static const u8 plt0[] = {
      0xff, 0xb3, 0x04, 0, 0, 0, // pushl 4(%ebx)
      0xff, 0xa3, 0x08, 0, 0, 0, // jmp *8(%ebx)
      0x90, 0x90, 0x90, 0x90,    // nop
    };
    memcpy(buf, plt0, sizeof(plt0));
  } else {
    static const u8 plt0[] = {
      0xff, 0x35, 0, 0, 0, 0, // pushl (GOTPLT+4)
      0xff, 0x25, 0, 0, 0, 0, // jmp *(GOTPLT+8)
      0x90, 0x90, 0x90, 0x90, // nop
    };
    memcpy(buf, plt0, sizeof(plt0));
    *(u32 *)(buf + 2) = ctx.gotplt->shdr.sh_addr + 4;
    *(u32 *)(buf + 8) = ctx.gotplt->shdr.sh_addr + 8;
  }

  // Write PLT entries
  i64 relplt_idx = 0;

  for (Symbol<I386> *sym : symbols) {
    u8 *ent = buf + sym->get_plt_idx(ctx) * I386::plt_size;

    if (ctx.arg.pic) {
      static const u8 data[] = {
        0xff, 0xa3, 0, 0, 0, 0, // jmp *foo@GOT(%ebx)
        0x68, 0,    0, 0, 0,    // pushl $reloc_offset
        0xe9, 0,    0, 0, 0,    // jmp .PLT0@PC
      };
      memcpy(ent, data, sizeof(data));
      *(u32 *)(ent + 2) = sym->get_gotplt_addr(ctx) - ctx.gotplt->shdr.sh_addr;
    } else {
      static const u8 data[] = {
        0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
        0x68, 0,    0, 0, 0,    // pushl $reloc_offset
        0xe9, 0,    0, 0, 0,    // jmp .PLT0@PC
      };
      memcpy(ent, data, sizeof(data));
      *(u32 *)(ent + 2) = sym->get_gotplt_addr(ctx);
    }

    *(u32 *)(ent + 7) = relplt_idx++ * sizeof(ElfRel<I386>);
    *(u32 *)(ent + 12) = this->shdr.sh_addr - sym->get_plt_addr(ctx) - 16;
  }
}

template <>
void PltGotSection<I386>::copy_buf(Context<I386> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  if (ctx.arg.pic) {
    static const u8 data[] = {
      0xff, 0xa3, 0, 0, 0, 0, // jmp   *foo@GOT(%ebx)
      0x66, 0x90,             // nop
    };

    for (i64 i = 0; i < symbols.size(); i++) {
      u8 *ent = buf + i * sizeof(data);
      memcpy(ent, data, sizeof(data));
      *(u32 *)(ent + 2) = symbols[i]->get_got_addr(ctx) - ctx.gotplt->shdr.sh_addr;
    }
  } else {
    static const u8 data[] = {
      0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOT
      0x66, 0x90,             // nop
    };

    for (i64 i = 0; i < symbols.size(); i++) {
      u8 *ent = buf + i * sizeof(data);
      memcpy(ent, data, sizeof(data));
      *(u32 *)(ent + 2) = symbols[i]->get_got_addr(ctx);
    }
  }
}

template <>
void EhFrameSection<I386>::apply_reloc(Context<I386> &ctx,
                                       ElfRel<I386> &rel,
                                       u64 loc, u64 val) {
  u8 *base = ctx.buf + this->shdr.sh_offset;

  switch (rel.r_type) {
  case R_386_NONE:
    return;
  case R_386_32:
    *(u32 *)(base + loc) = val;
    return;
  case R_386_PC32:
    *(u32 *)(base + loc) = val - this->shdr.sh_addr - loc;
    return;
  }
  unreachable(ctx);
}

static void write_val(Context<I386> &ctx, u64 r_type, u8 *loc, u64 val) {
  switch (r_type) {
  case R_386_NONE:
    return;
  case R_386_8:
  case R_386_PC8:
    *loc += val;
    return;
  case R_386_16:
  case R_386_PC16:
    *(u16 *)loc += val;
    return;
  case R_386_32:
  case R_386_PC32:
  case R_386_GOT32:
  case R_386_GOT32X:
  case R_386_PLT32:
  case R_386_GOTOFF:
  case R_386_GOTPC:
  case R_386_TLS_LDM:
  case R_386_TLS_GOTIE:
  case R_386_TLS_LE:
  case R_386_TLS_GD:
  case R_386_TLS_LDO_32:
  case R_386_SIZE32:
  case R_386_TLS_GOTDESC:
    *(u32 *)loc += val;
    return;
  }
  unreachable(ctx);
}

template <>
void InputSection<I386>::apply_reloc_alloc(Context<I386> &ctx, u8 *base) {
  ElfRel<I386> *dynrel = nullptr;
  std::span<ElfRel<I386>> rels = get_rels(ctx);
  i64 frag_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<I386> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                              file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<I386> &rel = rels[i];
    Symbol<I386> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<I386> *ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      ref = &rel_fragments[frag_idx++];

    auto write = [&](u64 val) {
      write_val(ctx, rel.r_type, loc, val);
    };

#define S      (ref ? ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A      (ref ? ref->addend : 0)
#define P      (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G      (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOTPLT ctx.gotplt->shdr.sh_addr

    switch (rel_types[i]) {
    case R_NONE:
      break;
    case R_ABS:
      write(S + A);
      break;
    case R_BASEREL:
      *dynrel++ = {P, R_386_RELATIVE, 0};
      *(u32 *)loc += S + A;
      break;
    case R_DYN:
      *dynrel++ = {P, R_386_32, (u32)sym.get_dynsym_idx(ctx)};
      *(u32 *)loc += A;
      break;
    case R_PC:
      write(S + A - P);
      break;
    case R_GOT:
      write(sym.get_got_addr(ctx) + A - GOTPLT);
      break;
    case R_GOTOFF:
      write(S + A - GOTPLT);
      break;
    case R_GOTPC:
      write(GOTPLT + A - P);
      break;
    case R_GOTPCREL:
      write(G + GOTPLT + A - P);
      break;
    case R_TLS_GOTIE:
      write(sym.get_gottp_addr(ctx) + A - GOTPLT);
      break;
    case R_TLS_LE:
      write(S + A - ctx.tls_end);
      break;
    case R_TLS_GD:
      write(sym.get_tlsgd_addr(ctx) + A - GOTPLT);
      break;
    case R_TLS_LD:
      write(ctx.got->get_tlsld_addr(ctx) + A - GOTPLT);
      break;
    case R_TPOFF:
      write(S + A - ctx.tls_begin);
      break;
    case R_TLS_GOTDESC:
      write(sym.get_tlsdesc_addr(ctx) + A - GOTPLT);
      break;
    case R_TLS_GOTDESC_RELAX: {
      static const u8 insn[] = {
        0x8d, 0x05, 0, 0, 0, 0, // lea 0, %eax
      };
      memcpy(loc - 2, insn, sizeof(insn));
      write(S + A - ctx.tls_end);
      break;
    }
    case R_TLS_DESC_CALL_RELAX:
      // call *(%rax) -> nop
      loc[0] = 0x66;
      loc[1] = 0x90;
      break;
    case R_SIZE:
      write(sym.esym().st_size + A);
      break;
    default:
      unreachable(ctx);
    }

#undef S
#undef A
#undef P
#undef G
#undef GOTPLT
  }
}

template <>
void InputSection<I386>::apply_reloc_nonalloc(Context<I386> &ctx, u8 *base) {
  std::span<ElfRel<I386>> rels = get_rels(ctx);
  i64 frag_idx = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<I386> &rel = rels[i];
    if (rel.r_type == R_386_NONE)
      continue;

    Symbol<I386> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      report_undef(ctx, sym);
      continue;
    }

    const SectionFragmentRef<I386> *ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      ref = &rel_fragments[frag_idx++];

    auto write = [&](u64 val) {
      write_val(ctx, rel.r_type, loc, val);
    };

    switch (rel.r_type) {
    case R_386_8:
    case R_386_16:
    case R_386_32:
    case R_386_PC8:
    case R_386_PC16:
    case R_386_PC32:
    case R_386_GOTPC:
      if (ref)
        write(ref->frag->get_addr(ctx) + ref->addend);
      else
        write(sym.get_addr(ctx));
      break;
    case R_386_GOTOFF:
      write(sym.get_addr(ctx) - ctx.got->shdr.sh_addr);
      break;
    case R_386_TLS_LDO_32:
      write(sym.get_addr(ctx) - ctx.tls_begin);
      break;
    case R_386_SIZE32:
      write(sym.esym().st_size);
      break;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel_to_string<I386>(rel.r_type);
      break;
    }
  }
}

template <>
void InputSection<I386>::scan_relocations(Context<I386> &ctx) {
  assert(shdr.sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<I386>);
  std::span<ElfRel<I386>> rels = get_rels(ctx);
  bool is_writable = (shdr.sh_flags & SHF_WRITE);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<I386> &rel = rels[i];

    if (rel.r_type == R_386_NONE) {
      rel_types[i] = R_NONE;
      break;
    }

    Symbol<I386> &sym = *file.symbols[rel.r_sym];
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
    case R_386_8:
    case R_386_16: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  NONE,     ERROR, ERROR,         ERROR },      // DSO
        {  NONE,     ERROR, ERROR,         ERROR },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };

      dispatch(ctx, table, R_ABS, i);
      break;
    }
    case R_386_32: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    DYNREL,        PLT    },     // PDE
      };

      dispatch(ctx, table, R_ABS, i);
      break;
    }
    case R_386_PC8:
    case R_386_PC16: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  ERROR,    NONE,  ERROR,         ERROR },      // DSO
        {  ERROR,    NONE,  COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };

      dispatch(ctx, table, R_PC, i);
      break;
    }
    case R_386_PC32: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  BASEREL,  NONE,  ERROR,         ERROR },      // DSO
        {  BASEREL,  NONE,  COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };

      dispatch(ctx, table, R_PC, i);
      break;
    }
    case R_386_GOT32:
    case R_386_GOT32X:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOT;
      break;
    case R_386_PLT32:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      rel_types[i] = R_PC;
      break;
    case R_386_GOTOFF:
      rel_types[i] = R_GOTOFF;
      break;
    case R_386_GOTPC:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOTPC;
      break;
    case R_386_TLS_GOTIE:
      sym.flags |= NEEDS_GOTTP;
      rel_types[i] = R_TLS_GOTIE;
      break;
    case R_386_TLS_LE:
      sym.flags |= NEEDS_GOTTP;
      rel_types[i] = R_TLS_LE;
      break;
    case R_386_TLS_GD:
      sym.flags |= NEEDS_TLSGD;
      rel_types[i] = R_TLS_GD;
      break;
    case R_386_TLS_LDM:
      sym.flags |= NEEDS_TLSLD;
      rel_types[i] = R_TLS_LD;
      break;
    case R_386_TLS_LDO_32:
      rel_types[i] = R_TPOFF;
      break;
    case R_386_SIZE32:
      rel_types[i] = R_SIZE;
      break;
    case R_386_TLS_GOTDESC:
      if (ctx.arg.relax && !ctx.arg.shared) {
        rel_types[i] = R_TLS_GOTDESC_RELAX;
      } else {
        sym.flags |= NEEDS_TLSDESC;
        rel_types[i] = R_TLS_GOTDESC;
      }
      break;
    case R_386_TLS_DESC_CALL:
      if (ctx.arg.relax && !ctx.arg.shared)
        rel_types[i] = R_TLS_DESC_CALL_RELAX;
      else
        rel_types[i] = R_NONE;
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: "
                 << rel_to_string<I386>(rel.r_type);
    }
  }
}
