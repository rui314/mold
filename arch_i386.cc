#include "mold.h"

template <>
std::string rel_to_string<I386>(u32 r_type) {
  switch (r_type) {
  case R_386_NONE: return "R_386_NONE";
  case R_386_32: return "R_386_32";
  case R_386_PC32: return "R_386_PC32";
  case R_386_GOT32: return "R_386_GOT32";
  case R_386_PLT32: return "R_386_PLT32";
  case R_386_COPY: return "R_386_COPY";
  case R_386_GLOB_DAT: return "R_386_GLOB_DAT";
  case R_386_JUMP_SLOT: return "R_386_JUMP_SLOT";
  case R_386_RELATIVE: return "R_386_RELATIVE";
  case R_386_GOTOFF: return "R_386_GOTOFF";
  case R_386_GOTPC: return "R_386_GOTPC";
  case R_386_32PLT: return "R_386_32PLT";
  case R_386_TLS_TPOFF: return "R_386_TLS_TPOFF";
  case R_386_TLS_IE: return "R_386_TLS_IE";
  case R_386_TLS_GOTIE: return "R_386_TLS_GOTIE";
  case R_386_TLS_LE: return "R_386_TLS_LE";
  case R_386_TLS_GD: return "R_386_TLS_GD";
  case R_386_TLS_LDM: return "R_386_TLS_LDM";
  case R_386_16: return "R_386_16";
  case R_386_PC16: return "R_386_PC16";
  case R_386_8: return "R_386_8";
  case R_386_PC8: return "R_386_PC8";
  case R_386_TLS_GD_32: return "R_386_TLS_GD_32";
  case R_386_TLS_GD_PUSH: return "R_386_TLS_GD_PUSH";
  case R_386_TLS_GD_CALL: return "R_386_TLS_GD_CALL";
  case R_386_TLS_GD_POP: return "R_386_TLS_GD_POP";
  case R_386_TLS_LDM_32: return "R_386_TLS_LDM_32";
  case R_386_TLS_LDM_PUSH: return "R_386_TLS_LDM_PUSH";
  case R_386_TLS_LDM_CALL: return "R_386_TLS_LDM_CALL";
  case R_386_TLS_LDM_POP: return "R_386_TLS_LDM_POP";
  case R_386_TLS_LDO_32: return "R_386_TLS_LDO_32";
  case R_386_TLS_IE_32: return "R_386_TLS_IE_32";
  case R_386_TLS_LE_32: return "R_386_TLS_LE_32";
  case R_386_TLS_DTPMOD32: return "R_386_TLS_DTPMOD32";
  case R_386_TLS_DTPOFF32: return "R_386_TLS_DTPOFF32";
  case R_386_TLS_TPOFF32: return "R_386_TLS_TPOFF32";
  case R_386_TLS_GOTDESC: return "R_386_TLS_GOTDESC";
  case R_386_TLS_DESC_CALL: return "R_386_TLS_DESC_CALL";
  case R_386_TLS_DESC: return "R_386_TLS_DESC";
  case R_386_IRELATIVE: return "R_386_IRELATIVE";
  case R_386_GOT32X: return "R_386_GOT32X";
  }
  return "unknown (" + std::to_string(r_type) + ")";
}

static void write_val(Context<I386> &ctx, u64 r_type, u8 *loc, u64 val,
                      bool overwrite) {
  switch (r_type) {
  case R_386_NONE:
    return;
  case R_386_8:
  case R_386_PC8:
    if (overwrite)
      *loc = val;
    else
      *loc += val;
    return;
  case R_386_16:
  case R_386_PC16:
    if (overwrite)
      *(u16 *)loc = val;
    else
      *(u16 *)loc += val;
    return;
  case R_386_32:
  case R_386_PC32:
  case R_386_GOT32:
  case R_386_GOT32X:
  case R_386_PLT32:
  case R_386_GOTOFF:
  case R_386_GOTPC:
  case R_386_SIZE32:
    if (overwrite)
      *(u32 *)loc = val;
    else
      *(u32 *)loc += val;
    return;
  }
  unreachable(ctx);
}

template <>
void InputSection<I386>::apply_reloc_alloc(Context<I386> &ctx, u8 *base) {
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

    auto write = [&](u64 val, bool overwrite) {
      write_val(ctx, rel.r_type, loc, val, overwrite);
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
        write(ref->frag->get_addr(ctx) + ref->addend, true);
      else
        write(sym.get_addr(ctx), false);
      break;
    case R_386_SIZE32:
      write(sym.esym->st_size, false);
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
    case R_386_TLS_TPOFF:
    case R_386_TLS_IE:
    case R_386_TLS_GOTIE:
    case R_386_TLS_LE:
    case R_386_TLS_GD:
    case R_386_TLS_LDM:
    case R_386_TLS_GD_32:
    case R_386_TLS_GD_PUSH:
    case R_386_TLS_GD_CALL:
    case R_386_TLS_GD_POP:
    case R_386_TLS_LDM_32:
    case R_386_TLS_LDM_PUSH:
    case R_386_TLS_LDM_CALL:
    case R_386_TLS_LDM_POP:
    case R_386_TLS_LDO_32:
    case R_386_TLS_IE_32:
    case R_386_TLS_LE_32:
    case R_386_TLS_DTPMOD32:
    case R_386_TLS_DTPOFF32:
    case R_386_TLS_TPOFF32:
      Fatal(ctx) << "TLS reloc is not supported yet";
    case R_386_SIZE32:
      rel_types[i] = R_SIZE;
      break;
    case R_386_TLS_GOTDESC:
    case R_386_TLS_DESC_CALL:
    case R_386_TLS_DESC:
      Fatal(ctx) << "TLS reloc is not supported yet";
    }
  }
}
