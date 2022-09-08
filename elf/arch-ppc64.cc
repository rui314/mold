// PPC64 is a bit tricky to support because PC-relative load/store
// instructions are generally not supported. Therefore, for example, it's
// not easy for position-independent code to load a value from .got, as we
// can't do that with <PC + the offset to the .got entry>.
//
// We can get the program counter by the following two instructions
//
//   bl .+4   // branch to the next instruction as if it were a function
//   mflr r0  // copy the return address to r0
//
// , but that's too expensive to do if we do this for each load/store.
//
// As a workaround, most functions are compiled in such a way that r2 is
// assumed to always contain the address of .got + 0x8000. With this, we
// can for example load the first entry of .got with a single instruction,
// `lw r0, -0x8000(r2)`. r2 is called the TOC pointer.
//
// There's only one .got for each ELF module. Therefore, if a callee is in
// the same ELF module, r2 doesn't have to recomputed. Most function calls
// are usually within the same ELF module, so this mechanism is efficient.
//
// In PPC64, a function usually have two entry points, global and local.
// The local entry point is usually 8 bytes past the global entry point.
// In between is the following instructions:
//
//   addis r2, r12, .TOC.@ha
//   addi  r2, r2,  .TOC.@lo + 4;
//
// The global entry point assumes that the address of the function being
// called is in r12, and it restores r2 from r12. So, if a callee's TOC
// pointer is different from the current one (e.g. calling a function in
// another .so), we first load the callee's address to r12 (e.g. from
// .got.plt with a r2-relative load) and branch to that address. Then the
// callee computes its own TOC pointer using r12.

#include "mold.h"

namespace mold::elf {

using E = PPC64;

static u64 lo(u64 x)       { return x & 0xffff; }
static u64 hi(u64 x)       { return x >> 16; }
static u64 ha(u64 x)       { return (x + 0x8000) >> 16; }
static u64 high(u64 x)     { return (x >> 16) & 0xffff; }
static u64 higha(u64 x)    { return ((x + 0x8000) >> 16) & 0xffff; }
static u64 higher(u64 x)   { return (x >> 32) & 0xffff; }
static u64 highera(u64 x)  { return ((x + 0x8000) >> 32) & 0xffff; }
static u64 highest(u64 x)  { return x >> 48; }
static u64 highesta(u64 x) { return (x + 0x8000) >> 48; }

static constexpr u32 plt_entry[] = {
  // Save %r2 to the caller's TOC save area
  0xf841'0018, // std     r2, 24(r1)

  // Set %r12 to this PLT entry's .got.plt value and jump there
  0x3d82'0000, // addis   r12, r2, 0
  0xe98c'0000, // ld      r12, 0(r12)
  0x7d89'03a6, // mtctr   r12
  0x4e80'0420, // bctr
  0x0000'0000, // padding
};

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + sym->get_plt_idx(ctx) * E::plt_size;
    memcpy(ent, plt_entry, sizeof(plt_entry));

    i64 disp = sym->get_gotplt_addr(ctx) - sym->get_plt_addr(ctx) - ctx.TOC->value;
    assert(disp == sign_extend(disp, 31));
    *(ul32 *)(ent + 4) |= bits(disp, 31, 16);
    *(ul32 *)(ent + 8) |= bits(disp, 15, 0);
  }
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + sym->get_pltgot_idx(ctx) * E::pltgot_size;
    memcpy(ent, plt_entry, sizeof(plt_entry));

    i64 disp = sym->get_got_addr(ctx) - sym->get_plt_addr(ctx) - ctx.TOC->value;
    assert(disp == sign_extend(disp, 31));
    *(ul32 *)(ent + 4) |= bits(disp, 31, 16);
    *(ul32 *)(ent + 8) |= bits(disp, 15, 0);
  }
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_PPC64_NONE:
    return;
  case R_PPC64_ADDR64:
    *(ul64 *)loc = val;
    return;
  case R_PPC64_REL32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    return;
  case R_PPC64_REL64:
    *(ul64 *)loc = val - this->shdr.sh_addr - offset;
    return;
  default:
    Fatal(ctx) << ": EhFrame: " << rel;
  }
  unreachable();
}

static u64 get_local_entry_offset(Context<E> &ctx, Symbol<E> &sym) {
  i64 val = sym.esym().ppc64_local_entry;
  if (val == 0 || val == 1)
    return 0;
  if (val == 7)
    Fatal(ctx) << sym << ": local entry offset 7 is reserved";
  return 1 << val;
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
    if (rel.r_type == R_386_NONE)
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
    case R_PPC64_ADDR64:
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_PPC64_TOC16_HA:
      *(ul16 *)loc = ha(S + A - ctx.TOC->value);
      break;
    case R_PPC64_TOC16_LO:
      *(ul16 *)loc = S + A - ctx.TOC->value;
      break;
    case R_PPC64_TOC16_DS:
    case R_PPC64_TOC16_LO_DS:
      *(ul16 *)loc |= (S + A - ctx.TOC->value) & 0xfffc;
      break;
    case R_PPC64_REL24: {
      i64 val;
      if (sym.has_plt(ctx)) {
        RangeExtensionRef ref = extra.range_extn[i];
        assert(ref.thunk_idx != -1);
        val = output_section->thunks[ref.thunk_idx]->get_addr(ref.sym_idx) + A - P;
      } else {
        val = S + A - P + get_local_entry_offset(ctx, sym);
      }

      check(val, -(1 << 23), 1 << 23);
      *(ul32 *)loc |= bits(val, 25, 2) << 2;
      break;
    }
    case R_PPC64_REL16_HA:
      *(ul16 *)loc = ha(S + A - P);
      break;
    case R_PPC64_REL16_LO:
      *(ul16 *)loc = S + A - P;
      break;
    case R_PPC64_GOT_TPREL16_HA:
      *(ul16 *)loc = ha(sym.get_gottp_addr(ctx) - ctx.TOC->value);
      break;
    case R_PPC64_GOT_TPREL16_LO_DS:
      *(ul16 *)loc |= (sym.get_gottp_addr(ctx) - ctx.TOC->value) & 0xfffc;
      break;
    case R_PPC64_TPREL16_HA:
      *(ul16 *)loc = ha(S + A - ctx.tp_addr);
      break;
    case R_PPC64_TPREL16_LO:
      *(ul16 *)loc = S + A - ctx.tp_addr;
      break;
    case R_PPC64_TLS:
      break;
    default:
      Fatal(ctx) << ": apply_reloc_alloc relocation: " << rel;
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
    if (rel.r_type == R_386_NONE)
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

    auto overflow_check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag ? addend : this->get_addend(rel))
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    default:
      Fatal(ctx) << ": apply_reloc_nonalloc: " << rel;
    }

#undef S
#undef A
#undef G
#undef GOT
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
    if (rel.r_type == R_386_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (!sym.file) {
      record_undef_error(ctx, rel);
      continue;
    }

    if (sym.get_type() == STT_GNU_IFUNC)
      sym.flags |= (NEEDS_GOT | NEEDS_PLT);

    switch (rel.r_type) {
    case R_PPC64_ADDR64:
      scan_abs_dyn_rel(ctx, sym, rel);
      break;
    case R_PPC64_GOT_TPREL16_HA:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_PPC64_REL24:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_PPC64_TOC16_HA:
    case R_PPC64_TOC16_LO:
    case R_PPC64_TOC16_LO_DS:
    case R_PPC64_TOC16_DS:
    case R_PPC64_REL16_HA:
    case R_PPC64_REL16_LO:
    case R_PPC64_TPREL16_HA:
    case R_PPC64_TPREL16_LO:
    case R_PPC64_GOT_TPREL16_LO_DS:
    case R_PPC64_TLS:
      break;
    default:
      Fatal(ctx) << *this << ": scan_relocations: " << rel;
    }
  }
}

void GlinkSection::update_shdr(Context<PPC64> &ctx) {
  this->shdr.sh_size = HEADER_SIZE + ctx.plt->symbols.size() * ENTRY_SIZE;
}

void GlinkSection::copy_buf(Context<PPC64> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  static constexpr u8 hdr[] = {
    0xa6, 0x02, 0x08, 0x7c, // mflr    r0
    0x05, 0x00, 0x9f, 0x42, // bcl     1f
    0xa6, 0x02, 0x68, 0x7d, // 1: mflr r11
    0xa6, 0x03, 0x08, 0x7c, // mtlr    r0
    0xf0, 0xff, 0x0b, 0xe8, // ld      r0, -16(r11)
    0x50, 0x60, 0x8b, 0x7d, // subf    r12, r11, r12
    0x14, 0x5a, 0x60, 0x7d, // add     r11, r0, r11
    0xd4, 0xff, 0x0c, 0x38, // addi    r0, r12, -44
    0x00, 0x00, 0x8b, 0xe9, // ld      r12, 0(r11)
    0x82, 0xf0, 0x00, 0x78, // rldicl  r0, r0, 62, 2
    0xa6, 0x03, 0x89, 0x7d, // mtctr   r12
    0x08, 0x00, 0x6b, 0xe9, // ld      r11, 8(r11)
    0x20, 0x04, 0x80, 0x4e, // bctr
  };

  static_assert(HEADER_SIZE == sizeof(hdr));
  memcpy(buf, hdr, sizeof(hdr));

  u32 *ent = (u32 *)(buf + sizeof(hdr));
  for (i64 i = 0; i < ctx.plt->symbols.size(); i++)
    *ent++ = 0x4b00'0000 | (-(HEADER_SIZE + i * ENTRY_SIZE) & 0x00ff'ffff);
}

// For range extension thunks
template <>
bool is_reachable(Context<E> &ctx, Symbol<E> &sym,
                  InputSection<E> &isec, const ElfRel<E> &rel) {
  // We always jump to a PLT entry through a thunk.
  return !sym.has_plt(ctx);
}

template <>
void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;

  static const u32 data[] = {
    // Save r2 to the r2 save slot reserved in the caller's stack frame
    0xf8410018, // std   r2, 24(r1)
    // Jump to a PLT entry
    0x3d820000, // addis r12, r2,  foo@gotplt@toc@ha
    0xe98c0000, // addi  r12, r12, foo@gotplt@toc@lo
    0x7d8903a6, // mtctr r12
    0x4e800420, // bctr  r12
  };

  static_assert(E::thunk_size == sizeof(data));

  for (i64 i = 0; i < symbols.size(); i++) {
    Symbol<E> &sym = *symbols[i];
    u8 *loc = buf + i * E::thunk_size;
    memcpy(loc , data, sizeof(data));

    u64 got = sym.has_got(ctx) ? sym.get_got_addr(ctx) : sym.get_gotplt_addr(ctx);
    i64 val = got - ctx.TOC->value;
    *(ul32 *)(loc + 4) |= ha(val);
    *(ul32 *)(loc + 8) |= lo(val);
  }
}

} // namespace mold::elf
