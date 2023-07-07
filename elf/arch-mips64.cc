#include "mold.h"

namespace mold::elf {

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
        *(U32<E> *)loc |= ((-val + 0x8000) >> 16) & 0xffff;
      } else if (rel.r_type2 == R_MIPS_SUB && rel.r_type3 == R_MIPS_LO16) {
        *(U32<E> *)loc |= -val & 0xffff;
      } else {
        Error(ctx) << *this << ": unsupported relocation combination: "
                   << rel_to_string<E>(rel.r_type) << " "
                   << rel_to_string<E>(rel.r_type2) << " "
                   << rel_to_string<E>(rel.r_type3);
      }
    };

    auto write_hi16 = [&](u64 val) {
      if (rel.r_type2 == R_NONE && rel.r_type3 == R_NONE)
        *(U32<E> *)loc |= ((val + 0x8000) >> 16) & 0xffff;
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
    case R_MIPS_GPREL16:
      if (sym.is_local(ctx))
        write_lo16(S + A + GP0 - GP);
      else
        write_lo16(S + A - GP);
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
    case R_MIPS_GPREL16:
    case R_MIPS_GOT_OFST:
    case R_MIPS_JALR:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

static constexpr i64 PAGE_HALF = 0x8000;

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
static bool compare(const typename MipsGotSection<E>::Entry &a,
                    const typename MipsGotSection<E>::Entry &b) {
  return std::tuple(a.sym->file->priority, a.sym->sym_idx, a.addend) <
         std::tuple(b.sym->file->priority, b.sym->sym_idx, b.addend);
}

template <typename E>
void MipsGotSection<E>::finalize(Context<E> &ctx) {
  // Finalize got_syms
  sort(got_syms.begin(), got_syms.end(), compare<E>);

  remove_duplicates(got_syms);

  // Finalize gotpage_syms
  for (Entry &ent : gotpage_syms)
    ent.addr = ent.sym->get_addr(ctx) + ent.addend;

  sort(gotpage_syms.begin(), gotpage_syms.end(),
       [](const Entry &a, const Entry &b) {
    return a.addr < b.addr;
  });

  i64 old_size = rel_addrs.size();
  abs_addrs.clear();
  rel_addrs.clear();

  for (Entry &ent : gotpage_syms) {
    if (ent.sym->is_absolute()) {
      abs_addrs.push_back(ent.addr);
    } else if (rel_addrs.empty() || rel_addrs.back() + PAGE_HALF < ent.addr) {
      rel_addrs.push_back(ent.addr + PAGE_HALF);
    }
  }

  // We want to shrink this section monotonically to prevent oscillation.
  if (old_size != 0 && rel_addrs.size() < old_size)
    rel_addrs.resize(old_size);

  i64 num_entries = got_syms.size() + abs_addrs.size() + rel_addrs.size();
  this->shdr.sh_size = num_entries * sizeof(Word<E>);
}

template <typename E>
u64 MipsGotSection<E>::get_got_addr(Context<E> &ctx, Symbol<E> &sym, i64 addend) {
  auto it = std::lower_bound(got_syms.begin(), got_syms.end(),
                             Entry{&sym, addend}, compare<E>);
  assert(it != got_syms.end());
  return this->shdr.sh_addr + (it - got_syms.begin()) * sizeof(Word<E>);
}

template <typename E>
u64 MipsGotSection<E>::get_gotpage_got_addr(Context<E> &ctx, Symbol<E> &sym,
                                            i64 addend) {
  std::span<u64> vec = sym.is_absolute() ? abs_addrs : rel_addrs;

  u64 addr = sym.get_addr(ctx) + addend;
  auto it = std::lower_bound(vec.begin(), vec.end(), addr, [](u64 x, u64 y) {
    return x + PAGE_HALF < y;
  });
  assert(*it - PAGE_HALF <= addr && addr < *it + PAGE_HALF);

  i64 idx;
  if (sym.is_absolute())
    idx = it - vec.begin() + got_syms.size();
  else
    idx = it - vec.begin() + got_syms.size() + abs_addrs.size();
  return this->shdr.sh_addr + idx * sizeof(Word<E>);
}

template <typename E>
u64 MipsGotSection<E>::get_gotpage_page_addr(Context<E> &ctx, Symbol<E> &sym,
                                            i64 addend) {
  std::span<u64> vec = sym.is_absolute() ? abs_addrs : rel_addrs;

  u64 addr = sym.get_addr(ctx) + addend;
  auto it = std::lower_bound(vec.begin(), vec.end(), addr,
                             [](u64 page_addr, u64 addr) {
    return page_addr + PAGE_HALF < addr;
  });
  assert(*it - PAGE_HALF <= addr && addr < *it + PAGE_HALF);
  return *it;
}

template <typename E>
i64 MipsGotSection<E>::get_reldyn_size(Context<E> &ctx) const {
  i64 n = 0;
  for (const Entry &ent : got_syms)
    if (ent.sym->is_imported || (ctx.arg.pic && ent.sym->is_relative()))
      n++;
  return n + rel_addrs.size();
}

template <typename E>
void MipsGotSection<E>::copy_buf(Context<E> &ctx) {
  U64<E> *buf = (U64<E> *)(ctx.buf + this->shdr.sh_offset);
  memset(buf, 0, this->shdr.sh_size);

  ElfRel<E> *dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                                    this->reldyn_offset);

  i64 i = 0;
  for (; i < got_syms.size(); i++) {
    Symbol<E> &sym = *got_syms[i].sym;
    u64 loc = this->shdr.sh_addr + i * sizeof(Word<E>);
    i64 A = got_syms[i].addend;

    if (sym.is_imported) {
      *dynrel++ = ElfRel<E>(loc, E::R_GLOB_DAT, sym.get_dynsym_idx(ctx), A);
    } else if (ctx.arg.pic && sym.is_relative()) {
      u64 val = sym.get_addr(ctx, NO_PLT) + A;
      *dynrel++ = ElfRel<E>(loc, E::R_RELATIVE, sym.get_dynsym_idx(ctx), val);
      if (ctx.arg.apply_dynamic_relocs)
        buf[i] = val;
    } else {
      buf[i] = sym.get_addr(ctx, NO_PLT) + A;
    }
  }

  for (u64 addr : abs_addrs)
    buf[i++] = addr;

  for (u64 addr : rel_addrs) {
    if (ctx.arg.pic)
      *dynrel++ = ElfRel<E>(this->shdr.sh_addr + i * sizeof(Word<E>),
                            E::R_RELATIVE, 0, addr);
    buf[i++] = addr;
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
