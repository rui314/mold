// HP/PA (Hewlett-Packard Precision Architecture), also known as PA-RISC,
// is a RISC ISA developed by HP in the '80s. It was replaced by Itanium
// which was a joint project between Intel and HP in the early 2000s. No
// new HP/PA processors have been created after 2005.
//
// Thread pointer (TP) is stored to the control register cr27, which is an
// alias for tr3.
//
// $ltp (linkage table pointer) is $r19
// $dp is $r27


#include "mold.h"

namespace mold::elf {

using E = HPPA32;

static u64 RND(u64 x) { return (x + 0x1000) & ~(u64)0x1fff; }
static u64 L(u64 x) { return x & 0xffff'f800; }
static u64 R(u64 x) { return x & 0x0000'07ff; }
static u64 LR(u64 x, u64 a) { return L(x + RND(a)); }
static u64 RR(u64 x, u64 a) { return R(x + RND(a)) + a - RND(a); }

static u64 get_gp(Context<E> &ctx) {
  if (ctx.arg.pic)
    return ctx.gotplt->shdr.sh_addr;
  return ctx.extra.global->value;
}

static u32 dis_assemble_17(u32 val) {
  return ((val & 0x10000) >> 16) |
         ((val & 0x0f800) << 5)  |
         ((val & 0x00400) >> 8)  |
         ((val & 0x003ff) << 3);
}

static u32 dis_assemble_21(u32 val) {
  return (bits(val, 6, 2) << 16) | (bits(val, 8, 7) << 14) |
         (bits(val, 1, 0) << 12) | (bits(val, 19, 9) << 1) |
         bit(val, 20);
}

static u32 dis_low_sign_ext(u32 val, u32 len) {
  return (bits(val, len - 2, 0) << 1) | bit(val, len - 1);
}

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_PARISC_PCREL32:
    *(ub32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  case R_PARISC_SEGREL32:
    *(ub32 *)loc = val;
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

  u64 SB = 0;
  u64 GP = get_gp(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

#define S   sym.get_addr(ctx)
#define A   get_addend(loc, rel)
#define P   (get_addr() + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_PARISC_DIR32:
      apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_PARISC_DIR21L:
      *(ub32 *)loc |= dis_assemble_21(bits(LR(S, A), 31, 11));
      break;
    case R_PARISC_DIR14R:
      *(ub32 *)loc |= dis_low_sign_ext(RR(S, A), 14);
      break;
    case R_PARISC_PCREL32: {
      u64 addr = has_thunk(i) ? get_thunk_addr(i) : S;
      *(ub32 *)loc = addr + A - P - 8;
      break;
    }
    case R_PARISC_PCREL21L: {
      u64 addr = has_thunk(i) ? get_thunk_addr(i) : S;
      *(ub32 *)loc |= dis_assemble_21(bits(addr + A - P - 8, 31, 11));
      break;
    }
    case R_PARISC_PCREL17F: {
      u64 addr = has_thunk(i) ? get_thunk_addr(i) : S;
      *(ub32 *)loc |= dis_assemble_17((addr + A - P - 8) >> 2);
      break;
    }
    case R_PARISC_PCREL14R: {
      u64 addr = has_thunk(i) ? get_thunk_addr(i) : S;
      *(ub32 *)loc |= dis_low_sign_ext(R(addr - P - 8 + A), 14);
      break;
    }
    case R_PARISC_DPREL21L:
      *(ub32 *)loc |= dis_assemble_21(bits(LR(S - GP, A), 31, 11));
      break;
    case R_PARISC_DPREL14R:
      *(ub32 *)loc |= dis_low_sign_ext(RR(S - GP, A), 14);
      break;
    case R_PARISC_DLTIND21L:
      assert(A == 0);
      *(ub32 *)loc |= dis_assemble_21(bits(G + GOT - GP, 31, 11));
      break;
    case R_PARISC_DLTIND14R:
      assert(A == 0);
      *(ub32 *)loc |= dis_low_sign_ext(R(G + GOT - GP), 14);
      break;
    case R_PARISC_SEGREL32:
      *(ub32 *)loc = S + A - SB;
      break;
    case R_PARISC_PLABEL32:
      if (sym.is_remaining_undef_weak()) {
        *(ub32 *)loc = 0;
      } else if (ctx.arg.pic) {
        if (sym.is_imported)
          *dynrel++ = ElfRel<E>(P, R_PARISC_PLABEL32, sym.get_dynsym_idx(ctx), 0);
        else
          *dynrel++ = ElfRel<E>(P, R_PARISC_PLABEL32, 0, sym.get_opd_addr(ctx) + 2);
        *(ub32 *)loc = 0;
      } else {
        *(ub32 *)loc = sym.get_opd_addr(ctx) + 2;
      }
      break;
    case R_PARISC_TPREL21L:
      *(ub32 *)loc |= dis_assemble_21(bits(LR(S - ctx.tp_addr, A), 31, 11));
      break;
    case R_PARISC_TPREL14R:
      *(ub32 *)loc |= dis_low_sign_ext(RR(S - ctx.tp_addr, A), 14);
      break;
    case R_PARISC_LTOFF_TP21L:
      assert(A == 0);
      *(ub32 *)loc |=
        dis_assemble_21(bits(LR(sym.get_gottp_addr(ctx) - GP, A), 31, 11));
      break;
    case R_PARISC_LTOFF_TP14R:
      assert(A == 0);
      *(ub32 *)loc |= dis_low_sign_ext(R(sym.get_gottp_addr(ctx) - GP), 14);
      break;
    case R_PARISC_TLS_GD21L:
      assert(A == 0);
      *(ub32 *)loc |=
        dis_assemble_21(bits(LR(sym.get_tlsgd_addr(ctx) - GP, A), 31, 11));
      break;
    case R_PARISC_TLS_GD14R:
      assert(A == 0);
      *(ub32 *)loc |= dis_low_sign_ext(R(sym.get_tlsgd_addr(ctx) - GP), 14);
      break;
    case R_PARISC_TLS_LDM21L:
      assert(A == 0);
      *(ub32 *)loc |=
        dis_assemble_21(bits(LR(ctx.got->get_tlsld_addr(ctx) - GP, A), 31, 11));
      break;
    case R_PARISC_TLS_LDM14R:
      assert(A == 0);
      *(ub32 *)loc |= dis_low_sign_ext(R(ctx.got->get_tlsld_addr(ctx) - GP), 14);
      break;
    case R_PARISC_TLS_LDO21L:
      assert(A == 0);
      *(ub32 *)loc |= dis_assemble_21(bits(LR(S - ctx.dtp_addr, A), 31, 11));
      break;
    case R_PARISC_TLS_LDO14R:
      assert(A == 0);
      *(ub32 *)loc |= dis_low_sign_ext(R(S - ctx.dtp_addr), 14);
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

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? frag_addend : get_addend(loc, rel))

    switch (rel.r_type) {
    case R_PARISC_DIR32:
    case R_PARISC_SEGREL32:
      *(ub32 *)loc = S + A;
      break;
    default:
      Error(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
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
      Error(ctx) << sym << ": GNU ifunc symbol is not supported on PA-RISC";

    if (sym.is_func()) {
      switch (rel.r_type) {
      case R_PARISC_PCREL32:
      case R_PARISC_PCREL21L:
      case R_PARISC_PCREL17F:
      case R_PARISC_PCREL14R:
      case R_PARISC_PLABEL32:
        break;
      default:
        Error(ctx) << *this << ": " << rel
                   << " may not refer to a function symbol " << sym;
      }
    }

    switch (rel.r_type) {
    case R_PARISC_DIR32:
      scan_dyn_absrel(ctx, sym, rel);
      break;
    case R_PARISC_DIR21L:
    case R_PARISC_DIR14R:
      scan_absrel(ctx, sym, rel);
      break;
    case R_PARISC_PCREL32:
    case R_PARISC_PCREL21L:
    case R_PARISC_PCREL17F:
    case R_PARISC_PCREL14R:
    case R_PARISC_DPREL21L:
    case R_PARISC_DPREL14R:
      if (sym.is_func() || sym.esym().is_undef_weak())
        sym.flags.fetch_or(NEEDS_OPD, std::memory_order_relaxed);
      else
        scan_pcrel(ctx, sym, rel);
      break;
    case R_PARISC_DLTIND21L:
    case R_PARISC_DLTIND14R:
      sym.flags.fetch_or(NEEDS_GOT, std::memory_order_relaxed);
      break;
    case R_PARISC_PLABEL32:
      if (!sym.is_remaining_undef_weak()) {
        if (ctx.arg.pic)
          file.num_dynrel++;
        sym.flags.fetch_or(NEEDS_OPD, std::memory_order_relaxed);
      }
      break;
    case R_PARISC_LTOFF_TP21L:
    case R_PARISC_LTOFF_TP14R:
      sym.flags.fetch_or(NEEDS_GOTTP, std::memory_order_relaxed);
      break;
    case R_PARISC_TLS_GD21L:
    case R_PARISC_TLS_GD14R:
      sym.flags.fetch_or(NEEDS_TLSGD, std::memory_order_relaxed);
      break;
    case R_PARISC_TLS_LDM21L:
    case R_PARISC_TLS_LDM14R:
      ctx.needs_tlsld.store(true, std::memory_order_relaxed);
      break;
    case R_PARISC_SEGREL32:
    case R_PARISC_TPREL21L:
    case R_PARISC_TPREL14R:
    case R_PARISC_TLS_LDO21L:
    case R_PARISC_TLS_LDO14R:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <>
void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;
  u64 gp = get_gp(ctx);

  static const ub32 opd_pic_entry[] = {
    0x2a60'0000, // addil LR'<OFFSET>, r19, r1
    0x3436'0000, // ldo   RR'<OFFSET>(r1), r22
    0x0ec0'1095, // ldw   0(r22), r21
    0xeaa0'c000, // bv    r0(r21)
    0x0ec8'1093, // ldw   4(r22), r19
  };

  static const ub32 opd_nopic_entry[] = {
    0x2b60'0000, // addil LR'<OFFSET>, dp, r1
    0x3436'0000, // ldo   RR'<OFFSET>(r1), r22
    0x0ec0'1095, // ldw   0(r22), r21
    0xeaa0'c000, // bv    r0(r21)
    0x0ec8'1093, // ldw   4(r22), r19
  };

  static const ub32 local_pic_entry[] = {
    0x2a60'0000, // addil L%0, r19, r1
    0x3435'0000, // ldo   0(r1), r21
    0xeaa0'c000, // bv    r0(r21)
    0x0800'0240, // nop
    0x0800'0240, // nop
  };

  static const ub32 local_nopic_entry[] = {
    0x2b60'0000, // addil L%0, dp, r1
    0x3435'0000, // ldo   0(r1), r21
    0xeaa0'c000, // bv    r0(r21)
    0x0800'0240, // nop
    0x0800'0240, // nop
  };

  static_assert(E::thunk_size == sizeof(opd_pic_entry));
  static_assert(E::thunk_size == sizeof(opd_nopic_entry));
  static_assert(E::thunk_size == sizeof(local_pic_entry));
  static_assert(E::thunk_size == sizeof(local_nopic_entry));

  for (i64 i = 0; i < symbols.size(); i++) {
    ub32 *loc = (ub32 *)(buf + i * E::thunk_size);
    Symbol<E> &sym = *symbols[i];
    i64 val;

    if (sym.has_opd(ctx)) {
      val = sym.get_opd_addr(ctx) - gp;
      if (ctx.arg.pic)
        memcpy(loc, opd_pic_entry, E::thunk_size);
      else
        memcpy(loc, opd_nopic_entry, E::thunk_size);
    } else {
      assert(!sym.is_imported);
      val = sym.get_addr(ctx) - gp;
      if (ctx.arg.pic)
        memcpy(loc, local_pic_entry, E::thunk_size);
      else
        memcpy(loc, local_nopic_entry, E::thunk_size);
    }

    loc[0] |= dis_assemble_21(bits(LR(val, 0), 31, 11));
    loc[1] |= dis_low_sign_ext(RR(val, 0), 14);
  }
}

void HppaOpdSection::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  sym->set_opd_idx(ctx, symbols.size());
  symbols.push_back(sym);
}

void HppaOpdSection::update_shdr(Context<E> &ctx) {
  if (!symbols.empty())
    this->shdr.sh_size = symbols.size() * ENTRY_SIZE + TRAILER_SIZE;
  this->shdr.sh_link = ctx.extra.opd->shndx;
}

void HppaOpdSection::copy_buf(Context<E> &ctx) {
  ub32 *buf = (ub32 *)(ctx.buf + this->shdr.sh_offset);
  u64 gp = get_gp(ctx);

  memset(buf, 0, this->shdr.sh_size);

  for (i64 i = 0; i < symbols.size(); i++) {
    Symbol<E> &sym = *symbols[i];
    if (!ctx.arg.pic && !sym.is_imported) {
      buf[i * 2] = sym.get_addr(ctx, NO_OPD);
      buf[i * 2 + 1] = gp;
    }
  }
}

void HppaRelOpdSection::update_shdr(Context<E> &ctx) {
  this->shdr.sh_link = ctx.extra.opd->shndx;

  if (ctx.arg.pic) {
    this->shdr.sh_size = ctx.extra.opd->symbols.size() * sizeof(ElfRel<E>);
  } else {
    this->shdr.sh_size = 0;
    for (Symbol<E> *sym : ctx.extra.opd->symbols)
      if (sym->is_imported)
        this->shdr.sh_size += sizeof(ElfRel<E>);
  }
}

void HppaRelOpdSection::copy_buf(Context<E> &ctx) {
  ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + this->shdr.sh_offset);

  for (Symbol<E> *sym : ctx.extra.opd->symbols) {
    if (ctx.arg.pic || sym->is_imported) {
      memset(rel, 0, sizeof(*rel));
      rel->r_offset = sym->get_opd_addr(ctx);
      rel->r_type = R_PARISC_IPLT;
      if (sym->is_imported)
        rel->r_sym = sym->get_dynsym_idx(ctx);
      else
        rel->r_addend = sym->get_addr(ctx, NO_OPD);
      rel++;
    }
  }

  // This code stub must immediately follow the OPD element that the last
  // .rela.plt relocation refers to, because that's how the runtime finds
  // this piece of code on process startup.
  //
  // The code stub loads the _dl_runtime_resolve address to $r20 call the
  // function with its GP in $r19. The magic bytes, 0xc0ffee and 0xdeadbeef,
  // are used by the runtime to identify this code stub, so they must be
  // exactly that bytes.
  //
  // HPPA's runtime assumes that this stub is immediately followed by .got.
  // The files created by mold don't actually satisfy that constraint
  // because we don't intermix executable code and data. So, instead, we
  // reserve three padding words after the code stub and set
  // _GLOBAL_OFFSET_TABLE_ there as if that particular place is the
  // beginning of a .got. This trick works because the runtime only uses
  // _GLOBAL_OFFSET_TABLE_ to access GOT[1],
  //
  // Due to the existence of this code stub, .opd must be readable,
  // writable and executable. Writable executable segment is bad from the
  // security standpoint, but we have no choice other than doing it.
  //
  // https://sourceware.org/git/?p=glibc.git;f=sysdeps/hppa/dl-machine.h;h=1d5194856601e025c#l223
  ub32 insn[] = {
    0x0e80'1095, // 1: ldw 0(r20), r21
    0xeaa0'c000, //    bv r0(r21)
    0x0e88'1095, //    ldw 4(r20), r21
    0xea9f'1fdd, //    b,l 1b, r20
    0xd680'1c1e, //    depwi 0, 31, 2, r20
    0x00c0'ffee, //    (_dl_runtime_resolve's address)
    0xdead'beef, //    (%r19 for _dl_runtime_resolve)
    0x0000'0000, //    (_GLOBAL_OFFSET_TABLE_ is set to here)
    0x0000'0000, //    (runtime uses this word to identify this ELF module)
    0x0000'0000, //    (padding)
  };

  memcpy(ctx.buf + this->shdr.sh_offset +
         ctx.extra.opd->symbols.size() * sizeof(ElfRel<E>),
         insn, sizeof(insn));
}

} // namespace mold::elf
