#include "mold.h"

namespace mold::elf {

static constexpr i64 BIAS = 0x8000;

template <typename E>
void write_plt_header(Context<E> &ctx, u8 *buf) {}

template <typename E>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  static const U32<E> insn[] = {
    0xdf99'0000, // ld      t9, 0(gp)
    0x03e0'7825, // move    t3, ra
    0x0320'f809, // jalr    t9
    0x6418'0000, // daddiu  t8, zero, 0
  };

  memcpy(buf, insn, sizeof(insn));
  *(U32<E> *)buf |= sym.get_gotplt_addr(ctx) - sym.get_plt_addr(ctx);
  *(U32<E> *)(buf + 12) |= sym.get_dynsym_idx(ctx);
}

template <typename E>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {}

template <typename E>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_MIPS_64:
    *(U64<E> *)loc = val;
    break;
  default:
    Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
  }
}

template <typename E>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  ElfRel<E> *dynrel = nullptr;
  if (ctx.reldyn)
    dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                           file.reldyn_offset + this->reldyn_offset);

  u64 GP = ctx._gp->get_addr(ctx);
  u64 GP0 = file.mips_gp0;

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

    auto write_combined = [&](u64 val) {
      if (rel.r_type2 == R_MIPS_64 && rel.r_type3 == R_NONE) {
        *(U64<E> *)loc |= val;
      } else if (rel.r_type2 == R_MIPS_SUB && rel.r_type3 == R_MIPS_HI16) {
        *(U32<E> *)loc |= ((-val + BIAS) >> 16) & 0xffff;
      } else if (rel.r_type2 == R_MIPS_SUB && rel.r_type3 == R_MIPS_LO16) {
        *(U32<E> *)loc |= -val & 0xffff;
      } else {
        Error(ctx) << *this << ": unsupported relocation combination: "
                   << rel_to_string<E>(rel.r_type) << " "
                   << rel_to_string<E>(rel.r_type2) << " "
                   << rel_to_string<E>(rel.r_type3);
      }
    };

    auto write32 = [&](u64 val) {
      if (rel.r_type2 == R_NONE && rel.r_type3 == R_NONE)
        *(U32<E> *)loc = val;
      else
        write_combined(val);
    };

    auto write_hi16 = [&](u64 val) {
      if (rel.r_type2 == R_NONE && rel.r_type3 == R_NONE)
        *(U32<E> *)loc |= ((val + BIAS) >> 16) & 0xffff;
      else
        write_combined(val);
    };

    auto write_lo16 = [&](u64 val) {
      if (rel.r_type2 == R_NONE && rel.r_type3 == R_NONE)
        *(U32<E> *)loc |= val & 0xffff;
      else
        write_combined(val);
    };

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + rel.r_offset;

    switch (rel.r_type) {
    case R_MIPS_64:
      *(U64<E> *)loc = S + A;
      break;
    case R_MIPS_GPREL16:
      if (sym.is_local(ctx))
        write_lo16(S + A + GP0 - GP);
      else
        write_lo16(S + A - GP);
      break;
    case R_MIPS_GPREL32:
      write32(S + A + GP0 - GP);
      break;
    case R_MIPS_GOT_DISP:
    case R_MIPS_CALL16:
      write_lo16(ctx.extra.got->get_got_addr(ctx, sym, A) - GP);
      break;
    case R_MIPS_GOT_PAGE:
      write_lo16(ctx.extra.got->get_gotpage_got_addr(ctx, sym, A) - GP);
      break;
    case R_MIPS_GOT_OFST:
      write_lo16(S + A - ctx.extra.got->get_gotpage_page_addr(ctx, sym, A));
      break;
    case R_MIPS_JALR:
      break;
    case R_MIPS_TLS_TPREL_HI16:
      write_hi16(S + A - ctx.tp_addr);
      break;
    case R_MIPS_TLS_TPREL_LO16:
      write_lo16(S + A - ctx.tp_addr);
      break;
    case R_MIPS_TLS_GOTTPREL:
      write_lo16(ctx.extra.got->get_gottp_addr(ctx, sym) - GP);
      break;
    default:
      unreachable();
    }
  }
}

template <typename E>
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

    u64 S = frag ? frag->get_addr(ctx) : sym.get_addr(ctx);
    u64 A = frag ? frag_addend : get_addend(loc, rel);

    switch (rel.r_type) {
    case R_MIPS_32:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(U32<E> *)loc = *val;
      else
        *(U32<E> *)loc = S + A;
      break;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
    }
  }
}

template <typename E>
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<E>);
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    switch (rel.r_type) {
    case R_MIPS_GOT_DISP:
    case R_MIPS_CALL16:
      ctx.extra.got->add_got_symbol(sym, rel.r_addend);
      break;
    case R_MIPS_GOT_PAGE:
      ctx.extra.got->add_gotpage_symbol(sym, rel.r_addend);
      break;
    case R_MIPS_TLS_GOTTPREL:
      assert(rel.r_addend == 0);
      ctx.extra.got->add_gottp_symbol(sym);
      break;
    case R_MIPS_TLS_TPREL_HI16:
    case R_MIPS_TLS_TPREL_LO16:
      check_tlsle(ctx, sym, rel);
      break;
    case R_MIPS_64:
    case R_MIPS_GPREL16:
    case R_MIPS_GPREL32:
    case R_MIPS_GOT_OFST:
    case R_MIPS_JALR:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <typename E>
void MipsGotSection<E>::add_got_symbol(Symbol<E> &sym, i64 addend) {
  std::scoped_lock lock(mu);
  got_syms.push_back({&sym, addend});
}

template <typename E>
void MipsGotSection<E>::add_gotpage_symbol(Symbol<E> &sym, i64 addend) {
  std::scoped_lock lock(mu);
  gotpage_syms.push_back({&sym, addend});
}

template <typename E>
void MipsGotSection<E>::add_gottp_symbol(Symbol<E> &sym) {
  std::scoped_lock lock(mu);
  gottp_syms.push_back(&sym);
}

template <typename E>
bool MipsGotSection<E>::SymbolAddend::operator<(const SymbolAddend &other) const {
  return std::tuple(sym->file->priority, sym->sym_idx, addend) <
         std::tuple(other.sym->file->priority, other.sym->sym_idx, other.addend);
};

template <typename E>
static bool compare(const Symbol<E> *a, const Symbol<E> *b) {
  return std::tuple(a->file->priority, a->sym_idx) <
         std::tuple(b->file->priority, b->sym_idx);
};

template <typename E>
u64 MipsGotSection<E>::SymbolAddend::get_addr(Context<E> &ctx, i64 flags) const {
  return sym->get_addr(ctx, flags) + addend;
}

template <typename E>
u64
MipsGotSection<E>::get_got_addr(Context<E> &ctx, Symbol<E> &sym, i64 addend) const {
  auto it = std::lower_bound(got_syms.begin(), got_syms.end(),
                             SymbolAddend{&sym, addend});
  assert(it != got_syms.end());
  return this->shdr.sh_addr + (it - got_syms.begin()) * sizeof(Word<E>);
}

template <typename E>
u64 MipsGotSection<E>::get_gotpage_got_addr(Context<E> &ctx, Symbol<E> &sym,
                                            i64 addend) const {
  auto it = std::lower_bound(gotpage_syms.begin(), gotpage_syms.end(),
                             SymbolAddend{&sym, addend});
  assert(it != gotpage_syms.end());
  i64 idx = it - gotpage_syms.begin();
  return this->shdr.sh_addr + (got_syms.size() + idx) * sizeof(Word<E>);
}

template <typename E>
u64 MipsGotSection<E>::get_gotpage_page_addr(Context<E> &ctx, Symbol<E> &sym,
                                            i64 addend) const {
  auto it = std::lower_bound(gotpage_syms.begin(), gotpage_syms.end(),
                             SymbolAddend{&sym, addend});
  assert(it != gotpage_syms.end());
  return it->get_addr(ctx);
}

template <typename E>
u64 MipsGotSection<E>::get_gottp_addr(Context<E> &ctx, Symbol<E> &sym) const {
  auto it = std::lower_bound(gottp_syms.begin(), gottp_syms.end(),
                             &sym, compare<E>);
  assert(it != gottp_syms.end());
  i64 idx = it - gottp_syms.begin();
  return this->shdr.sh_addr +
         (got_syms.size() + gotpage_syms.size() + idx) * sizeof(Word<E>);
}

template <typename E>
std::vector<typename MipsGotSection<E>::GotEntry>
MipsGotSection<E>::get_got_entries(Context<E> &ctx) const {
  std::vector<GotEntry> entries;

  // Create GOT entries for ordinary symbols
  for (const SymbolAddend &ent : got_syms) {
    // If a symbol is imported, let the dynamic linker to resolve it.
    if (ent.sym->is_imported) {
      entries.push_back({0, E::R_GLOB_DAT, ent.sym});
      continue;
    }

    // If we know an address at link-time, fill that GOT entry now.
    // It may need a base relocation, though.
    if (ctx.arg.pic && ent.sym->is_relative())
      entries.push_back({ent.get_addr(ctx, NO_PLT), E::R_RELATIVE});
    else
      entries.push_back({ent.get_addr(ctx, NO_PLT)});
  }

  // Create GOT entries for GOT_PAGE and GOT_OFST relocs
  for (const SymbolAddend &ent : gotpage_syms) {
    if (ctx.arg.pic && ent.sym->is_relative())
      entries.push_back({ent.get_addr(ctx), E::R_RELATIVE});
    else
      entries.push_back({ent.get_addr(ctx)});
  }

  for (Symbol<E> *sym : gottp_syms) {
    // If we know nothing about the symbol, let the dynamic linker
    // to fill the GOT entry.
    if (sym->is_imported) {
      entries.push_back({0, E::R_TPOFF, sym});
      continue;
    }

    // If we know the offset within the current thread vector,
    // let the dynamic linker to adjust it.
    if (ctx.arg.shared) {
      entries.push_back({sym->get_addr(ctx) - ctx.tls_begin, E::R_TPOFF});
      continue;
    }

    // Otherwise, we know the offset from the thread pointer (TP) at
    // link-time, so we can fill the GOT entry directly.
    entries.push_back({sym->get_addr(ctx) - ctx.tp_addr});
  }

  return entries;
}

template <typename E>
void MipsGotSection<E>::update_shdr(Context<E> &ctx) {
  // Finalize got_syms
  sort(got_syms);
  remove_duplicates(got_syms);
  this->shdr.sh_size = got_syms.size() * sizeof(Word<E>);

  // Finalize gotpage_syms
  sort(gotpage_syms);
  remove_duplicates(gotpage_syms);
  this->shdr.sh_size += gotpage_syms.size() * sizeof(Word<E>);

  // Finalize gottp_syms
  sort(gottp_syms, compare<E>);
  remove_duplicates(gottp_syms);
  this->shdr.sh_size += gottp_syms.size() * sizeof(Word<E>);
}

template <typename E>
i64 MipsGotSection<E>::get_reldyn_size(Context<E> &ctx) const {
  i64 n = 0;
  for (GotEntry &ent : get_got_entries(ctx))
    if (ent.r_type != R_NONE)
      n++;
  return n;
}

template <typename E>
void MipsGotSection<E>::copy_buf(Context<E> &ctx) {
  U64<E> *buf = (U64<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  ElfRel<E> *dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                                    this->reldyn_offset);

  for (i64 i = 0; GotEntry &ent : get_got_entries(ctx)) {
    if (ent.r_type != R_NONE)
      *dynrel++ = ElfRel<E>(this->shdr.sh_addr + i * sizeof(Word<E>),
                            ent.r_type,
                            ent.sym ? ent.sym->get_dynsym_idx(ctx) : 0,
                            ent.val);
    buf[i++] = ent.val;
  }
}

#define INSTANTIATE(E)                                                       \
  template void write_plt_header(Context<E> &, u8 *);                        \
  template void write_plt_entry(Context<E> &, u8 *, Symbol<E> &);            \
  template void write_pltgot_entry(Context<E> &, u8 *, Symbol<E> &);         \
  template void                                                              \
  EhFrameSection<E>::apply_reloc(Context<E> &, const ElfRel<E> &, u64, u64); \
  template void InputSection<E>::apply_reloc_alloc(Context<E> &, u8 *);      \
  template void InputSection<E>::apply_reloc_nonalloc(Context<E> &, u8 *);   \
  template void InputSection<E>::scan_relocations(Context<E> &);             \
  template class MipsGotSection<E>;

INSTANTIATE(MIPS64LE);
INSTANTIATE(MIPS64BE);

} // namespace mold::elf
