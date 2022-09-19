// SPARC is a RISC ISA developed by Sun Microsystems.
//
// The byte order of the processor is big-endian. Anything larger than a
// byte is stored in the "reverse" order compared to little-endian
// processors such as x86-64.
//
// All instructions are 4 bytes long and aligned to 4 bytes boundaries.
//
// A notable feature of SPARC is that, unlike other RISC ISAs, it doesn't
// need range extension thunks. It is because the SPARC's CALL instruction
// contains a 30 bits immediate. The processor scales it by 4 to extend it
// to 32 bits (this is doable because all instructions are aligned to 4
// bytes boundaries, so the least significant two bits are always zero).
// That means CALL's reach is PC ± 2 GiB, elinating the need of range
// extension thunks. It comes with the cost that the CALL instruction alone
// takes 1/4th of the instruction encoding space, though.
//
// https://docs.oracle.com/cd/E36784_01/html/E36857/chapter6-62988.html
// https://docs.oracle.com/cd/E19120-01/open.solaris/819-0690/chapter8-40/index.html

#include "mold.h"

namespace mold::elf {

using E = SPARC64;

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  memset(buf, 0, this->shdr.sh_size);

  static ub32 plt[] = {
    0x0300'0000, // sethi (. - .PLT0), %g1
    0x3068'0000, // ba,a  %xcc, .PLT1
    0x0100'0000, // nop
    0x0100'0000, // nop
    0x0100'0000, // nop
    0x0100'0000, // nop
    0x0100'0000, // nop
    0x0100'0000, // nop
  };

  static_assert(sizeof(plt) == E::plt_size);

  for (i64 i = 0; i < symbols.size(); i++) {
    Symbol<E> &sym = *symbols[i];
    u8 *loc = buf + E::plt_hdr_size + i * E::plt_size;
    memcpy(loc, plt, sizeof(plt));

    u64 plt0 = ctx.plt->shdr.sh_addr;
    u64 plt1 = ctx.plt->shdr.sh_addr + E::plt_size;
    u64 ent_addr = sym.get_plt_addr(ctx);

    *(ub32 *)(loc + 0) |= bits(ent_addr - plt0, 21, 0);
    *(ub32 *)(loc + 4) |= bits(plt1 - ent_addr - 4, 20, 2);
  }
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  memset(buf, 0, this->shdr.sh_size);

  static ub32 entry[] = {
    0x8a10'000f, // mov  %o7, %g5
    0x4000'0002, // call . + 4
    0xc25b'e014, // ldx  [ %o7 + 20 ], %g1
    0xc25b'c001, // ldx  [ %o7 + %g1 ], %g1
    0x81c0'4000, // jmp  %g1
    0x9e10'0005, // mov  %g5, %o7
    0x0000'0000, // .quad PLT - GOT
    0x0000'0000,
  };

  static_assert(sizeof(entry) == E::pltgot_size);

  for (Symbol<E> *sym : symbols) {
    u8 *loc = buf + sym->get_pltgot_idx(ctx) * E::pltgot_size;
    memcpy(loc, entry, sizeof(entry));
    *(ub64 *)(loc + 24) = sym->get_got_addr(ctx) - sym->get_plt_addr(ctx) - 4;
  }
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    return;
  case R_SPARC_DISP32:
    *(ub32 *)loc = val - this->shdr.sh_addr - offset;
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

#define S   (frag_ref ? frag_ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag_ref ? frag_ref->addend : this->get_addend(rel))
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_SPARC_64:
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_SPARC_5:
      *(ub32 *)loc |= bits(S + A, 4, 0);
      break;
    case R_SPARC_6:
      *(ub32 *)loc |= bits(S + A, 5, 0);
      break;
    case R_SPARC_7:
      *(ub32 *)loc |= bits(S + A, 6, 0);
      break;
    case R_SPARC_8:
      *(u8 *)loc = S + A;
      break;
    case R_SPARC_10:
    case R_SPARC_LO10:
    case R_SPARC_LOPLT10:
      *(ub32 *)loc |= bits(S + A, 9, 0);
      break;
    case R_SPARC_11:
      *(ub32 *)loc |= bits(S + A, 10, 0);
      break;
    case R_SPARC_13:
      *(ub32 *)loc |= bits(S + A, 12, 0);
      break;
    case R_SPARC_22:
      *(ub32 *)loc |= bits(S + A, 21, 0);
      break;
    case R_SPARC_16:
    case R_SPARC_UA16:
      *(ub16 *)loc = S + A;
      break;
    case R_SPARC_32:
    case R_SPARC_UA32:
    case R_SPARC_PLT32:
      *(ub32 *)loc = S + A;
      break;
    case R_SPARC_DISP8:
      *(u8 *)loc = S + A - P;
      break;
    case R_SPARC_DISP16:
      *(ub16 *)loc = S + A - P;
      break;
    case R_SPARC_DISP32:
    case R_SPARC_PCPLT32:
      *(ub32 *)loc = S + A - P;
      break;
    case R_SPARC_WDISP22:
      *(ub32 *)loc |= bits(S + A - P, 23, 2);
      break;
    case R_SPARC_WDISP30:
    case R_SPARC_WPLT30:
      *(ub32 *)loc |= bits(S + A - P, 31, 2);
      break;
    case R_SPARC_HI22:
    case R_SPARC_HIPLT22:
    case R_SPARC_LM22:
      *(ub32 *)loc |= bits(S + A, 31, 10);
      break;
    case R_SPARC_GOT10:
      *(ub32 *)loc |= bits(G, 9, 0);
      break;
    case R_SPARC_GOT13:
      *(ub32 *)loc |= bits(G, 12, 0);
      break;
    case R_SPARC_GOT22:
      *(ub32 *)loc |= bits(G, 31, 10);
      break;
    case R_SPARC_GOTDATA_HIX22: {
      i64 val = S + A - GOT;
      *(ub32 *)loc |= bits((val >> 10) ^ (val >> 31), 21, 0);
      break;
    }
    case R_SPARC_GOTDATA_OP_HIX22:
      *(ub32 *)loc |= bits((G >> 10) ^ (G >> 31), 21, 0);
      break;
    case R_SPARC_GOTDATA_LOX10: {
      i64 val = S + A - GOT;
      *(ub32 *)loc |= bits((val & 0x3ff) | ((val >> 31) & 0x1c00), 12, 0);
      break;
    }
    case R_SPARC_GOTDATA_OP_LOX10:
      *(ub32 *)loc |= bits((G & 0x3ff) | ((G >> 31) & 0x1c00), 12, 0);
      break;
    case R_SPARC_GOTDATA_OP:
      break;
    case R_SPARC_PC10:
    case R_SPARC_PCPLT10:
      *(ub32 *)loc |= bits(S + A - P, 9, 0);
      break;
    case R_SPARC_PC22:
    case R_SPARC_PCPLT22:
    case R_SPARC_PC_LM22:
      *(ub32 *)loc |= bits(S + A - P, 31, 10);
      break;
    case R_SPARC_OLO10:
      *(ub32 *)loc |= bits(S + A, 9, 0); // + O
      break;
    case R_SPARC_HH22:
      *(ub32 *)loc |= bits(S + A, 63, 42);
      break;
    case R_SPARC_HM10:
      *(ub32 *)loc |= bits(S + A, 41, 32);
      break;
    case R_SPARC_PC_HH22:
      *(ub32 *)loc |= bits(S + A - P, 53, 42);
      break;
    case R_SPARC_PC_HM10:
      *(ub32 *)loc |= bits(S + A - P, 41, 32);
      break;
    case R_SPARC_WDISP16: {
      i64 val = S + A - P;
      *(ub16 *)loc |= (bit(val, 16) << 21) | bits(val, 15, 2);
      break;
    }
    case R_SPARC_WDISP19:
      *(ub32 *)loc |= bits(S + A - P, 20, 2);
      break;
    case R_SPARC_DISP64:
      *(ub64 *)loc = S + A - P;
      break;
    case R_SPARC_PLT64:
    case R_SPARC_UA64:
    case R_SPARC_REGISTER:
      *(ub64 *)loc = S + A;
      break;
    case R_SPARC_HIX22:
      *(ub32 *)loc |= bits(~(S + A), 31, 10);
      break;
    case R_SPARC_LOX10:
      *(ub32 *)loc |= bits(S + A, 9, 0) | 0b0001'1100'0000'0000;
      break;
    case R_SPARC_H44:
      *(ub32 *)loc |= bits(S + A, 43, 22);
      break;
    case R_SPARC_M44:
      *(ub32 *)loc |= bits(S + A, 21, 12);
      break;
    case R_SPARC_L44:
      *(ub32 *)loc |= bits(S + A, 11, 0);
      break;
    case R_SPARC_TLS_GD_HI22:
      *(ub32 *)loc |= bits(sym.get_tlsgd_addr(ctx) + A - GOT, 31, 10);
      break;
    case R_SPARC_TLS_GD_LO10:
      *(ub32 *)loc |= bits(sym.get_tlsgd_addr(ctx) + A - GOT, 9, 0);
      break;
    case R_SPARC_TLS_GD_CALL:
    case R_SPARC_TLS_LDM_CALL: {
      Symbol<E> *sym2 = get_symbol(ctx, "__tls_get_addr");
      *(ub32 *)loc |= bits(sym2->get_addr(ctx) + A - P, 31, 2);
      break;
    }
    case R_SPARC_TLS_LDM_HI22:
      *(ub32 *)loc |= bits(ctx.got->get_tlsld_addr(ctx) + A - GOT, 31, 10);
      break;
    case R_SPARC_TLS_LDM_LO10:
      *(ub32 *)loc |= bits(ctx.got->get_tlsld_addr(ctx) + A - GOT, 9, 0);
      break;
    case R_SPARC_TLS_LDO_HIX22:
      *(ub32 *)loc |= bits(S + A - ctx.tls_begin, 31, 10);
      break;
    case R_SPARC_TLS_LDO_LOX10:
      *(ub32 *)loc |= bits(S + A - ctx.tls_begin, 9, 0);
      break;
    case R_SPARC_TLS_IE_HI22:
      *(ub32 *)loc |= bits(sym.get_gottp_addr(ctx) + A - GOT, 31, 10);
      break;
    case R_SPARC_TLS_IE_LO10:
      *(ub32 *)loc |= bits(sym.get_gottp_addr(ctx) + A - GOT, 9, 0);
      break;
    case R_SPARC_TLS_LE_HIX22:
      *(ub32 *)loc |= bits(~(S + A - ctx.tp_addr), 31, 10);
      break;
    case R_SPARC_TLS_LE_LOX10:
      *(ub32 *)loc |= bits(S + A - ctx.tp_addr, 9, 0) | 0b0001'1100'0000'0000;
      break;
    case R_SPARC_SIZE32:
      *(ub32 *)loc = sym.esym().st_size + A;
      break;
    case R_SPARC_TLS_GD_ADD:
    case R_SPARC_TLS_LDM_ADD:
    case R_SPARC_TLS_LDO_ADD:
    case R_SPARC_TLS_IE_LD:
    case R_SPARC_TLS_IE_LDX:
    case R_SPARC_TLS_IE_ADD:
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

#define S   (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag ? addend : this->get_addend(rel))

    switch (rel.r_type) {
    case R_NONE:
      break;
    case R_SPARC_64:
    case R_SPARC_UA64:
      if (!frag) {
        if (std::optional<u64> val = get_tombstone(sym)) {
          *(ub64 *)loc = *val;
          break;
        }
      }
      *(ub64 *)loc = S + A;
      break;
    case R_SPARC_32:
    case R_SPARC_UA32:
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

    if (sym.get_type() == STT_GNU_IFUNC)
      sym.flags |= (NEEDS_GOT | NEEDS_PLT);

    switch (rel.r_type) {
    case R_SPARC_8:
    case R_SPARC_10:
    case R_SPARC_11:
    case R_SPARC_13:
    case R_SPARC_16:
    case R_SPARC_22:
    case R_SPARC_32:
    case R_SPARC_REGISTER:
    case R_SPARC_UA16:
    case R_SPARC_UA32:
    case R_SPARC_UA64:
    case R_SPARC_PC_HM10:
    case R_SPARC_OLO10:
    case R_SPARC_LOX10:
    case R_SPARC_HM10:
    case R_SPARC_M44:
    case R_SPARC_HIX22:
    case R_SPARC_5:
    case R_SPARC_6:
    case R_SPARC_LO10:
    case R_SPARC_7:
    case R_SPARC_L44:
    case R_SPARC_LM22:
    case R_SPARC_HI22:
    case R_SPARC_H44:
    case R_SPARC_HH22:
      scan_abs_rel(ctx, sym, rel);
      break;
    case R_SPARC_64:
      scan_abs_dyn_rel(ctx, sym, rel);
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
    case R_SPARC_GOT13:
    case R_SPARC_GOT10:
    case R_SPARC_GOT22:
    case R_SPARC_GOTDATA_HIX22:
    case R_SPARC_GOTDATA_LOX10:
    case R_SPARC_GOTDATA_OP_HIX22:
    case R_SPARC_GOTDATA_OP_LOX10:
    case R_SPARC_GOTDATA_OP:
      sym.flags |= NEEDS_GOT;
      break;
    case R_SPARC_DISP16:
    case R_SPARC_DISP32:
    case R_SPARC_DISP64:
    case R_SPARC_DISP8:
    case R_SPARC_PC10:
    case R_SPARC_PC22:
    case R_SPARC_PC_LM22:
    case R_SPARC_WDISP16:
    case R_SPARC_WDISP19:
    case R_SPARC_WDISP22:
    case R_SPARC_WDISP30:
    case R_SPARC_PC_HH22:
      scan_pcrel_rel(ctx, sym, rel);
      break;
    case R_SPARC_TLS_GD_HI22:
    case R_SPARC_TLS_GD_LO10:
    case R_SPARC_TLS_GD_ADD:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_SPARC_TLS_LDM_HI22:
    case R_SPARC_TLS_LDM_LO10:
    case R_SPARC_TLS_LDM_ADD:
    case R_SPARC_TLS_LDO_HIX22:
    case R_SPARC_TLS_LDO_LOX10:
    case R_SPARC_TLS_LDO_ADD:
      ctx.needs_tlsld = true;
      break;
    case R_SPARC_TLS_IE_HI22:
    case R_SPARC_TLS_IE_LO10:
    case R_SPARC_TLS_LE_HIX22:
    case R_SPARC_TLS_LE_LOX10:
    case R_SPARC_TLS_IE_LD:
    case R_SPARC_TLS_IE_LDX:
    case R_SPARC_TLS_IE_ADD:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_SPARC_TLS_GD_CALL:
    case R_SPARC_TLS_LDM_CALL: {
      Symbol<E> *sym2 = get_symbol(ctx, "__tls_get_addr");
      if (sym2->is_imported)
        sym2->flags |= NEEDS_PLT;
      break;
    }
    case R_SPARC_SIZE32:
    default:
      Fatal(ctx) << *this << ": scan_relocations: " << rel;
    }
  }
}

} // namespace mold::elf
