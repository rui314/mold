#include "mold.h"

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
    u8 *ent = buf + sym->plt_idx * I386::plt_size;

    if (ctx.arg.pic) {
      static const u8 data[] = {
        0xff, 0xa3, 0, 0, 0, 0, // jmp *foo@GOT(%ebx)
        0x68, 0,    0, 0, 0,    // pushl $reloc_offset
        0xe9, 0,    0, 0, 0,    // jmp .PLT0@PC
      };
      memcpy(ent, data, sizeof(data));
      *(u32 *)(ent + 2) = sym->get_gotplt_addr(ctx) - sym->get_plt_addr(ctx);
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
  case R_386_TLS_GOTIE:
  case R_386_TLS_LE:
  case R_386_SIZE32:
    *(u32 *)loc += val;
    return;
  }
  unreachable(ctx);
}

template <>
void InputSection<I386>::apply_reloc_alloc(Context<I386> &ctx, u8 *base) {
  i64 ref_idx = 0;
  ElfRel<I386> *dynrel = nullptr;

  if (ctx.reldyn)
    dynrel = (ElfRel<I386> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                              file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<I386> &rel = rels[i];
    Symbol<I386> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<I386> *ref = nullptr;
    if (has_fragments[i])
      ref = &rel_fragments[ref_idx++];

    auto write = [&](u64 val) {
      write_val(ctx, rel.r_type, loc, val);
    };

#define S   (ref ? ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (ref ? ref->addend : 0)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

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
      *dynrel++ = {P, R_386_32, sym.dynsym_idx};
      *(u32 *)loc += A;
      break;
    case R_PC:
      write(S + A - P);
      break;
    case R_GOT:
      write(G + A);
      break;
    case R_GOTOFF:
      write(S + A - GOT);
      break;
    case R_GOTPC:
      write(GOT + A - P);
      break;
    case R_GOTPCREL:
      write(G + GOT + A - P);
      break;
    case R_GOTTP:
      write(sym.get_gottpoff_addr(ctx) + A);
      break;
    case R_NTPOFF:
      write(sym.get_gottpoff_addr(ctx) + A - GOT);
      break;
    case R_SIZE:
      write(sym.esym->st_size + A);
      break;
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
void InputSection<I386>::apply_reloc_nonalloc(Context<I386> &ctx, u8 *base) {
  i64 ref_idx = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<I386> &rel = rels[i];
    Symbol<I386> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      Error(ctx) << "undefined symbol: " << file << ": " << sym;
      continue;
    }

    const SectionFragmentRef<I386> *ref = nullptr;
    if (has_fragments[i])
      ref = &rel_fragments[ref_idx++];

    auto write = [&](u64 val) {
      write_val(ctx, rel.r_type, loc, val);
    };

    switch (rel.r_type) {
    case R_386_NONE:
      return;
    case R_386_8:
    case R_386_16:
    case R_386_32:
    case R_386_PC8:
    case R_386_PC16:
    case R_386_PC32:
      if (ref)
        write(ref->frag->get_addr(ctx) + ref->addend);
      else
        write(sym.get_addr(ctx));
      break;
    case R_386_SIZE32:
      write(sym.esym->st_size);
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

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<I386> &rel = rels[i];
    Symbol<I386> &sym = *file.symbols[rel.r_sym];
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (!sym.file) {
      Error(ctx) << "undefined symbol: " << file << ": " << sym;
      continue;
    }

    if (sym.esym->st_type == STT_GNU_IFUNC)
      sym.flags |= NEEDS_PLT;

    switch (rel.r_type) {
    case R_386_NONE:
      rel_types[i] = R_NONE;
      break;
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
        {  NONE,     NONE,    COPYREL,       PLT    },     // PDE
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
    case R_386_TLS_IE:
      Error(ctx) << "TLS reloc is not supported yet: "
                 << rel_to_string<I386>(rel.r_type);
    case R_386_TLS_GOTIE:
      sym.flags |= NEEDS_GOTTPOFF;
      rel_types[i] = R_GOTTP;
      break;
    case R_386_TLS_LE:
      sym.flags |= NEEDS_GOTTPOFF;
      rel_types[i] = R_NTPOFF;
      break;
    case R_386_TLS_GD:
    case R_386_TLS_LDM:
    case R_386_TLS_LDO_32:
    case R_386_TLS_LE_32:
      Error(ctx) << "TLS reloc is not supported yet: "
                 << rel_to_string<I386>(rel.r_type);
    case R_386_SIZE32:
      rel_types[i] = R_SIZE;
      break;
    case R_386_TLS_GOTDESC:
    case R_386_TLS_DESC_CALL:
    case R_386_TLS_DESC:
      Error(ctx) << "tlsdesc reloc is not supported yet: "
                 << rel_to_string<I386>(rel.r_type);
    }
  }
}
