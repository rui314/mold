#include "mold.h"

namespace mold::elf {

using E = SPARC64;

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  memset(buf, 0, this->shdr.sh_size);

  for (i64 i = 0; i < symbols.size(); i++) {
    Symbol<E> &sym = *symbols[i];
    ub32 *ent = (ub32 *)(buf + E::plt_hdr_size + i * E::plt_size);
    u64 addr = sym.get_plt_addr(ctx);

    // sethi (. - .PLT0), %g1
    ent[0] = 0x3000'0000 | ((ctx.plt->shdr.sh_addr - addr) >> 10);

    // ba,a  %xcc, .PLT1
    ent[1] = 0x3040'0000 | ((addr - ctx.plt->shdr.sh_addr + E::plt_size) >> 10);
  }
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    return;
  default:
    Fatal(ctx) << "unknown relocation in ehframe: " << rel;
    return;
  }
  unreachable();
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
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<E> *frag_ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      frag_ref = &rel_fragments[frag_idx++];

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   (frag_ref ? frag_ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag_ref ? frag_ref->addend : this->get_addend(rel))
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_SPARC_64:
    case R_SPARC_UA64:
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_SPARC_5:
    case R_SPARC_7:
    case R_SPARC_6:
    case R_SPARC_8:
    case R_SPARC_10:
    case R_SPARC_11:
    case R_SPARC_16:
    case R_SPARC_32:
    case R_SPARC_UA16:
    case R_SPARC_UA32:
    case R_SPARC_22:
    case R_SPARC_13:
    case R_SPARC_HI22:
    case R_SPARC_HH22:
    case R_SPARC_HM10:
    case R_SPARC_LM22:
    case R_SPARC_LO10:
    case R_SPARC_OLO10:
    case R_SPARC_HIX22:
    case R_SPARC_LOX10:
    case R_SPARC_H44:
    case R_SPARC_M44:
    case R_SPARC_L44:
    case R_SPARC_REGISTER:
    case R_SPARC_GOT10:
    case R_SPARC_GOT13:
    case R_SPARC_GOT22:
    case R_SPARC_PLT32:
    case R_SPARC_WPLT30:
    case R_SPARC_HIPLT22:
    case R_SPARC_LOPLT10:
    case R_SPARC_PCPLT32:
    case R_SPARC_PCPLT22:
    case R_SPARC_PCPLT10:
    case R_SPARC_PLT64:
    case R_SPARC_TLS_GD_HI22:
    case R_SPARC_TLS_LDM_HI22:
    case R_SPARC_TLS_IE_HI22:
    case R_SPARC_TLS_IE_LD:
    case R_SPARC_TLS_IE_LDX:
    case R_SPARC_TLS_IE_ADD:
    case R_SPARC_TLS_LE_HIX22:
    case R_SPARC_DISP8:
    case R_SPARC_DISP16:
    case R_SPARC_DISP32:
    case R_SPARC_PC10:
    case R_SPARC_PC22:
    case R_SPARC_WDISP30:
    case R_SPARC_WDISP22:
    case R_SPARC_PC_HH22:
    case R_SPARC_PC_HM10:
    case R_SPARC_PC_LM22:
    case R_SPARC_WDISP16:
    case R_SPARC_WDISP19:
    case R_SPARC_DISP64:
    case R_SPARC_TLS_GD_LO10:
    case R_SPARC_TLS_GD_ADD:
    case R_SPARC_TLS_GD_CALL:
    case R_SPARC_TLS_LDM_LO10:
    case R_SPARC_TLS_LDM_ADD:
    case R_SPARC_TLS_LDM_CALL:
    case R_SPARC_TLS_IE_LO10:
    case R_SPARC_TLS_LE_LOX10:
    case R_SPARC_TLS_LDO_HIX22:
    case R_SPARC_TLS_LDO_ADD:
    case R_SPARC_TLS_LDO_LOX10:
    case R_SPARC_GOTDATA_OP_HIX22:
    case R_SPARC_GOTDATA_OP_LOX10:
    case R_SPARC_GOTDATA_OP:
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
    i64 addend;
    std::tie(frag, addend) = get_fragment(ctx, rel);

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag ? addend : this->get_addend(rel))

    switch (rel.r_type) {
    case R_NONE:
      break;
    case R_SPARC_64:
      if (!frag) {
        if (std::optional<u64> val = get_tombstone(sym)) {
          *(ul64 *)loc = *val;
          break;
        }
      }
      *(ul64 *)loc = S + A;
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

    if (sym.get_type() == STT_GNU_IFUNC)
      sym.flags |= (NEEDS_GOT | NEEDS_PLT);

    switch (rel.r_type) {
    case R_SPARC_64:
    case R_SPARC_UA64:
      scan_abs_dyn_rel(ctx, sym, rel);
      break;
    case R_SPARC_5:
    case R_SPARC_7:
    case R_SPARC_6:
    case R_SPARC_8:
    case R_SPARC_10:
    case R_SPARC_11:
    case R_SPARC_16:
    case R_SPARC_32:
    case R_SPARC_UA16:
    case R_SPARC_UA32:
    case R_SPARC_22:
    case R_SPARC_13:
    case R_SPARC_HI22:
    case R_SPARC_HH22:
    case R_SPARC_HM10:
    case R_SPARC_LM22:
    case R_SPARC_LO10:
    case R_SPARC_OLO10:
    case R_SPARC_HIX22:
    case R_SPARC_LOX10:
    case R_SPARC_H44:
    case R_SPARC_M44:
    case R_SPARC_L44:
    case R_SPARC_REGISTER:
      scan_abs_rel(ctx, sym, rel);
      break;
    case R_SPARC_GOT10:
    case R_SPARC_GOT13:
    case R_SPARC_GOT22:
      sym.flags |= NEEDS_GOT;
      break;
    case R_SPARC_PLT32:
    case R_SPARC_WPLT30:
    case R_SPARC_HIPLT22:
    case R_SPARC_LOPLT10:
    case R_SPARC_PCPLT32:
    case R_SPARC_PCPLT22:
    case R_SPARC_PCPLT10:
    case R_SPARC_PLT64:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_SPARC_TLS_GD_HI22:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_SPARC_TLS_LDM_HI22:
      ctx.needs_tlsld = true;
      break;
    case R_SPARC_TLS_IE_HI22:
    case R_SPARC_TLS_IE_LD:
    case R_SPARC_TLS_IE_LDX:
    case R_SPARC_TLS_IE_ADD:
    case R_SPARC_TLS_LE_HIX22:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_SPARC_DISP8:
    case R_SPARC_DISP16:
    case R_SPARC_DISP32:
    case R_SPARC_PC10:
    case R_SPARC_PC22:
    case R_SPARC_WDISP30:
    case R_SPARC_WDISP22:
    case R_SPARC_PC_HH22:
    case R_SPARC_PC_HM10:
    case R_SPARC_PC_LM22:
    case R_SPARC_WDISP16:
    case R_SPARC_WDISP19:
    case R_SPARC_DISP64:
    case R_SPARC_TLS_GD_LO10:
    case R_SPARC_TLS_GD_ADD:
    case R_SPARC_TLS_GD_CALL:
    case R_SPARC_TLS_LDM_LO10:
    case R_SPARC_TLS_LDM_ADD:
    case R_SPARC_TLS_LDM_CALL:
    case R_SPARC_TLS_IE_LO10:
    case R_SPARC_TLS_LE_LOX10:
    case R_SPARC_TLS_LDO_HIX22:
    case R_SPARC_TLS_LDO_ADD:
    case R_SPARC_TLS_LDO_LOX10:
    case R_SPARC_GOTDATA_OP_HIX22:
    case R_SPARC_GOTDATA_OP_LOX10:
    case R_SPARC_GOTDATA_OP:
      break;
    default:
      Fatal(ctx) << *this << ": scan_relocations: " << rel;
    }
  }
}

} // namespace mold::elf
