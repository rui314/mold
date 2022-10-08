#include "mold.h"

namespace mold::elf {

using E = PPC64V1;

static u64 lo(u64 x)       { return x & 0xffff; }
static u64 hi(u64 x)       { return x >> 16; }
static u64 ha(u64 x)       { return (x + 0x8000) >> 16; }
static u64 high(u64 x)     { return (x >> 16) & 0xffff; }
static u64 higha(u64 x)    { return ((x + 0x8000) >> 16) & 0xffff; }
static u64 higher(u64 x)   { return (x >> 32) & 0xffff; }
static u64 highera(u64 x)  { return ((x + 0x8000) >> 32) & 0xffff; }
static u64 highest(u64 x)  { return x >> 48; }
static u64 highesta(u64 x) { return (x + 0x8000) >> 48; }

// .plt is used only for lazy symbol resolution on PPC64. All PLT
// calls are made via range extension thunks even if they are within
// reach. Thunks read addresses from .got.plt and jump there.
// Therefore, once PLT symbols are resolved and final addresses are
// written to .got.plt, thunks just skip .plt and directly jump to the
// resolved addresses.
template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  static const ub32 insn[] = {
    0x7d88'02a6, // mflr    r12
    0x429f'0005, // bcl     1f
    0x7d68'02a6, // 1: mflr r11
    0xe84b'0024, // ld      r2,36(r11)
    0x7d88'03a6, // mtlr    r12
    0x7d62'5a14, // add     r11,r2,r11
    0xe98b'0000, // ld      r12,0(r11)
    0xe84b'0008, // ld      r2,8(r11)
    0x7d89'03a6, // mtctr   r12
    0xe96b'0010, // ld      r11,16(r11)
    0x4e80'0420, // bctr
    // .quad .got.plt - .plt - 8
    0x0000'0000,
    0x0000'0000,
  };

  static_assert(sizeof(insn) == E::plt_hdr_size);
  memcpy(buf, insn, sizeof(insn));

  *(ub64 *)(buf + 44) = ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 8;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  i64 offset = ctx.plt->shdr.sh_addr - sym.get_plt_addr(ctx) - 4;
  ub32 *loc = (ub32 *)buf;
  loc[0] = 0x3800'0000 | sym.get_plt_idx(ctx);   // li %r0, PLT_INDEX
  loc[1] = 0x4b00'0000 | (offset & 0x00ff'ffff); // b plt0
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  // No one uses .got.plt at runtime because all calls to .got.plt are
  // made via range extension thunks. Range extension thunks directly
  // calls the final destination by reading a .got entry. Here, we just
  // set a dummy instruction.
  *(ub32 *)buf = 0x6000'0000; // nop
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_PPC64_ADDR64:
    *(ub64 *)loc = val;
    break;
  case R_PPC64_REL32:
    *(ub32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  case R_PPC64_REL64:
    *(ub64 *)loc = val - this->shdr.sh_addr - offset;
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

#define S   sym.get_addr(ctx)
#define A   rel.r_addend
#define P   (get_addr() + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_PPC64_ADDR64:
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_PPC64_TOC:
      apply_abs_dyn_rel(ctx, *ctx.TOC, rel, loc, ctx.TOC->value, A, P, dynrel);
      break;
    case R_PPC64_TOC16_HA:
      *(ub16 *)loc = ha(S + A - ctx.TOC->value);
      break;
    case R_PPC64_TOC16_LO:
      *(ub16 *)loc = S + A - ctx.TOC->value;
      break;
    case R_PPC64_TOC16_DS:
    case R_PPC64_TOC16_LO_DS:
      *(ub16 *)loc |= (S + A - ctx.TOC->value) & 0xfffc;
      break;
    case R_PPC64_REL24: {
      i64 addr = S;

      if (ctx.opd) {
        i64 offset = addr - ctx.opd->shdr.sh_addr;
        if (0 <= offset && offset < ctx.opd->shdr.sh_size)
          addr = *(ub64 *)(ctx.buf + ctx.opd->shdr.sh_offset + offset);
      }

      i64 val = addr + A - P;

      if (sym.has_plt(ctx) || sign_extend(val, 25) != val) {
        RangeExtensionRef ref = extra.range_extn[i];
        assert(ref.thunk_idx != -1);
        val = output_section->thunks[ref.thunk_idx]->get_addr(ref.sym_idx) + A - P;

        // If the callee saves r2 to the caller's r2 save slot to clobber
        // r2, we need to restore r2 after function return. To do so,
        // there's usually a NOP as a placeholder after a BL. 0x6000'0000 is
        // a NOP.
        if (*(ub32 *)(loc + 4) == 0x6000'0000)
          *(ub32 *)(loc + 4) = 0xe841'0018; // ld r2, 24(r1)
      }

      check(val, -(1 << 25), 1 << 25);
      *(ub32 *)loc |= bits(val, 25, 2) << 2;
      break;
    }
    case R_PPC64_REL64:
      *(ub64 *)loc = S + A - P;
      break;
    case R_PPC64_REL16_HA:
      *(ub16 *)loc = ha(S + A - P);
      break;
    case R_PPC64_REL16_LO:
      *(ub16 *)loc = S + A - P;
      break;
    case R_PPC64_PLT16_HA:
      *(ub16 *)loc = ha(G + GOT - ctx.TOC->value);
      break;
    case R_PPC64_PLT16_HI:
      *(ub16 *)loc = hi(G + GOT - ctx.TOC->value);
      break;
    case R_PPC64_PLT16_LO:
      *(ub16 *)loc = lo(G + GOT - ctx.TOC->value);
      break;
    case R_PPC64_PLT16_LO_DS:
      *(ub16 *)loc |= (G + GOT - ctx.TOC->value) & 0xfffc;
      break;
    case R_PPC64_GOT_TPREL16_HA:
      *(ub16 *)loc = ha(sym.get_gottp_addr(ctx) - ctx.TOC->value);
      break;
    case R_PPC64_GOT_TLSGD16_HA:
      *(ub16 *)loc = ha(sym.get_tlsgd_addr(ctx) - ctx.TOC->value);
      break;
    case R_PPC64_GOT_TLSGD16_LO:
      *(ub16 *)loc = sym.get_tlsgd_addr(ctx) - ctx.TOC->value;
      break;
    case R_PPC64_GOT_TLSLD16_HA:
      *(ub16 *)loc = ha(ctx.got->get_tlsld_addr(ctx) - ctx.TOC->value);
      break;
    case R_PPC64_GOT_TLSLD16_LO:
      *(ub16 *)loc = ctx.got->get_tlsld_addr(ctx) - ctx.TOC->value;
      break;
    case R_PPC64_DTPREL16_HA:
      *(ub16 *)loc = ha(S + A - ctx.tls_begin - E::tls_dtv_offset);
      break;
    case R_PPC64_TPREL16_HA:
      *(ub16 *)loc = ha(S + A - ctx.tp_addr);
      break;
    case R_PPC64_DTPREL16_LO:
      *(ub16 *)loc = S + A - ctx.tls_begin - E::tls_dtv_offset;
      break;
    case R_PPC64_TPREL16_LO:
      *(ub16 *)loc = S + A - ctx.tp_addr;
      break;
    case R_PPC64_GOT_TPREL16_LO_DS:
      *(ub16 *)loc |= (sym.get_gottp_addr(ctx) - ctx.TOC->value) & 0xfffc;
      break;
    case R_PPC64_PLTSEQ:
    case R_PPC64_PLTCALL:
    case R_PPC64_TLS:
    case R_PPC64_TLSGD:
    case R_PPC64_TLSLD:
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

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? frag_addend : (i64)rel.r_addend)

    switch (rel.r_type) {
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

    if (sym.is_ifunc())
      sym.flags |= (NEEDS_GOT | NEEDS_PLT);

    switch (rel.r_type) {
    case R_PPC64_ADDR64:
      scan_abs_dyn_rel(ctx, sym, rel);
      break;
    case R_PPC64_TOC:
      scan_abs_dyn_rel(ctx, *ctx.TOC, rel);
      break;
    case R_PPC64_GOT_TPREL16_HA:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_PPC64_REL24:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_PPC64_PLT16_HA:
      sym.flags |= NEEDS_GOT;
      break;
    case R_PPC64_GOT_TLSGD16_HA:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_PPC64_GOT_TLSLD16_HA:
      ctx.needs_tlsld = true;
      break;
    case R_PPC64_REL64:
    case R_PPC64_TOC16_HA:
    case R_PPC64_TOC16_LO:
    case R_PPC64_TOC16_LO_DS:
    case R_PPC64_TOC16_DS:
    case R_PPC64_REL16_HA:
    case R_PPC64_REL16_LO:
    case R_PPC64_PLT16_HI:
    case R_PPC64_PLT16_LO:
    case R_PPC64_PLT16_LO_DS:
    case R_PPC64_PLTSEQ:
    case R_PPC64_PLTCALL:
    case R_PPC64_TPREL16_HA:
    case R_PPC64_TPREL16_LO:
    case R_PPC64_GOT_TPREL16_LO_DS:
    case R_PPC64_GOT_TLSGD16_LO:
    case R_PPC64_GOT_TLSLD16_LO:
    case R_PPC64_TLS:
    case R_PPC64_TLSGD:
    case R_PPC64_TLSLD:
    case R_PPC64_DTPREL16_HA:
    case R_PPC64_DTPREL16_LO:
      break;
    default:
      Fatal(ctx) << *this << ": scan_relocations: " << rel;
    }
  }
}

template <>
void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;

  // If the destination is PLT, we save the current r2, read a function
  // address and a new r2 from .got.plt and jump to the function.
  static const ub32 plt_thunk[] = {
    0xf841'0028, // std   %r2, 40(%r1)
    0x3d82'0000, // addis %r12, %r2,  foo@gotplt@toc@ha
    0x398c'0000, // addi  %r12, %r12, foo@gotplt@toc@lo
    0xe84c'0008, // ld    %r2,  8(%r12)
    0xe98c'0000, // ld    %r12, 0(%r12)
    0x7d89'03a6, // mtctr %r12
    0x4e80'0420, // bctr
  };


  // If the destination is a non-imported function, we directly jump
  // to that address.
  static const ub32 local_thunk[] = {
    0x3d82'0000, // addis r12, r2,  foo@toc@ha
    0x398c'0000, // addi  r12, r12, foo@toc@lo
    0x7d89'03a6, // mtctr r12
    0x4e80'0420, // bctr
    0x6000'0000, // nop
    0x6000'0000, // nop
    0x6000'0000, // nop
  };

  static_assert(E::thunk_size == sizeof(plt_thunk));
  static_assert(E::thunk_size == sizeof(local_thunk));

  for (i64 i = 0; i < symbols.size(); i++) {
    Symbol<E> &sym = *symbols[i];
    ub32 *loc = (ub32 *)(buf + i * E::thunk_size);

    if (sym.has_plt(ctx)) {
      memcpy(loc , plt_thunk, sizeof(plt_thunk));
      u64 got = sym.has_got(ctx) ? sym.get_got_addr(ctx) : sym.get_gotplt_addr(ctx);
      i64 val = got - ctx.TOC->value;
      loc[1] |= higha(val);
      loc[2] |= lo(val);
    } else {
      memcpy(loc , local_thunk, sizeof(local_thunk));
      i64 val = sym.get_addr(ctx) - ctx.TOC->value;
      loc[0] |= higha(val);
      loc[1] |= lo(val);
    }
  }
}

} // namespace mold::elf
