// This file implements the PowerPC ELFv2 ABI which was standardized in
// 2014. Modern little-endian PowerPC systems are based on this ABI.
// The ABI is often referred to as "ppc64le". This shouldn't be confused
// with "ppc64" which refers the original, big-endian PowerPC systems.
//
// PPC64 is a bit tricky to support because PC-relative load/store
// instructions are generally not available. Therefore, it's not easy
// for position-independent code to load a value from, for example,
// .got, as we can't do that with [PC + the offset to the .got entry].
//
// We can get the program counter by the following four instructions
//
//   mflr  r1  // save the current link register to r1
//   bl    .+4 // branch to the next instruction as if it were a function
//   mflr  r0  // copy the return address to r0
//   mtlr  r1  // restore the original link register value
//
// , but that's too expensive to do if we do this for each load/store.
//
// As a workaround, most functions are compiled in such a way that r2 is
// assumed to always contain the address of .got + 0x8000. With this, we
// can for example load the first entry of .got with a single instruction
// `lw r0, -0x8000(r2)`. r2 is called the TOC pointer.
//
// There's only one .got for each ELF module. Therefore, if a callee is in
// the same ELF module, r2 doesn't have to be recomputed. Most function
// calls are usually within the same ELF module, so this mechanism is
// efficient.
//
// In PPC64, a function usually have two entry points, global and local.
// The global entry point is usually 8 bytes precedes the local entry
// point. In between is the following instructions:
//
//   addis r2, r12, .TOC.@ha
//   addi  r2, r2,  .TOC.@lo + 4;
//
// The global entry point assumes that the address of itself is in r12,
// and it computes its own TOC pointer from r12. It's easy to do so for
// the callee because the offset between its .got + 0x8000 and the
// function is known at link-time. The above code sequence then falls
// through to the local entry point that assumes r2 is .got + 0x8000.
//
// So, if a callee's TOC pointer is different from the current one
// (e.g. calling a function in another .so), we first load the callee's
// address to r12 (e.g. from .got.plt with a r2-relative load) and branch
// to that address. Then the callee computes its own TOC pointer using
// r12.
//
// Note on section names: the PPC64 psABI uses a weird naming convention
// which calls .got.plt .plt. We ignored that part because it's just
// confusing. Since the runtime only cares about segments, we should be
// able to name sections whatever we want.
//
// https://openpowerfoundation.org/specifications/64bitelfabi/

#include "mold.h"

namespace mold::elf {

using E = PPC64V2;

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
  static const ul32 insn[] = {
    // Get PC
    0x7c08'02a6, // mflr    r0
    0x429f'0005, // bcl     20, 31, 4 // obtain PC
    0x7d68'02a6, // mflr    r11
    0x7c08'03a6, // mtlr    r0

    // Compute the PLT entry index
    0xe80b'002c, // ld      r0, 44(r11)
    0x7d8b'6050, // subf    r12, r11, r12
    0x7d60'5a14, // add     r11, r0, r11
    0x380c'ffcc, // addi    r0, r12, -52
    0x7800'f082, // rldicl  r0, r0, 62, 2

    // Load .got.plt[0] and .got.plt[1] and branch to .got.plt[0]
    0xe98b'0000, // ld      r12, 0(r11)
    0x7d89'03a6, // mtctr   r12
    0xe96b'0008, // ld      r11, 8(r11)
    0x4e80'0420, // bctr

    // .quad .got.plt - .plt - 8
    0x0000'0000,
    0x0000'0000,
  };

  memcpy(buf, insn, sizeof(insn));
  *(ul64 *)(buf + 52) = ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 8;
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  // When the control is transferred to a PLT entry, the PLT entry's
  // address is already set to %r12 by the caller.
  i64 offset = ctx.plt->shdr.sh_addr - sym.get_plt_addr(ctx);
  *(ul32 *)buf = 0x4b00'0000 | (offset & 0x00ff'ffff);        // b plt0
}

// .plt.got is not necessary on PPC64 because range extension thunks
// directly read GOT entries and jump there.
template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_PPC64_ADDR64:
    *(ul64 *)loc = val;
    break;
  case R_PPC64_REL32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  case R_PPC64_REL64:
    *(ul64 *)loc = val - this->shdr.sh_addr - offset;
    break;
  default:
    Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
  }
}

static u64 get_local_entry_offset(Context<E> &ctx, Symbol<E> &sym) {
  i64 val = sym.esym().ppc_local_entry;
  assert(val <= 7);
  if (val == 0 || val == 1)
    return 0;
  if (val == 7)
    Fatal(ctx) << sym << ": local entry offset 7 is reserved";
  return 1 << val;
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

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + rel.r_offset;
    u64 G = sym.get_got_idx(ctx) * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;
    u64 TOC = ctx.extra.TOC->value;

    switch (rel.r_type) {
    case R_PPC64_ADDR64:
      if (name() == ".toc")
        apply_toc_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      else
        apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_PPC64_TOC16_HA:
      *(ul16 *)loc = ha(S + A - TOC);
      break;
    case R_PPC64_TOC16_LO:
      *(ul16 *)loc = lo(S + A - TOC);
      break;
    case R_PPC64_TOC16_DS:
    case R_PPC64_TOC16_LO_DS:
      *(ul16 *)loc |= (S + A - TOC) & 0xfffc;
      break;
    case R_PPC64_REL24: {
      i64 val = S + A - P + get_local_entry_offset(ctx, sym);
      if (sym.has_plt(ctx) || sign_extend(val, 25) != val)
        val = get_thunk_addr(i) + A - P;

      check(val, -(1 << 25), 1 << 25);
      *(ul32 *)loc |= bits(val, 25, 2) << 2;

      // If a callee is an external function, PLT saves %r2 to the
      // caller's r2 save slot. We need to restore it after function
      // return. To do so, there's usually a NOP as a placeholder
      // after a BL. 0x6000'0000 is a NOP.
      if (sym.has_plt(ctx) && *(ul32 *)(loc + 4) == 0x6000'0000)
        *(ul32 *)(loc + 4) = 0xe841'0018; // ld r2, 24(r1)
      break;
    }
    case R_PPC64_REL64:
      *(ul64 *)loc = S + A - P;
      break;
    case R_PPC64_REL16_HA:
      *(ul16 *)loc = ha(S + A - P);
      break;
    case R_PPC64_REL16_LO:
      *(ul16 *)loc = lo(S + A - P);
      break;
    case R_PPC64_PLT16_HA:
      *(ul16 *)loc = ha(G + GOT - TOC);
      break;
    case R_PPC64_PLT16_HI:
      *(ul16 *)loc = hi(G + GOT - TOC);
      break;
    case R_PPC64_PLT16_LO:
      *(ul16 *)loc = lo(G + GOT - TOC);
      break;
    case R_PPC64_PLT16_LO_DS:
      *(ul16 *)loc |= (G + GOT - TOC) & 0xfffc;
      break;
    case R_PPC64_GOT_TPREL16_HA:
      *(ul16 *)loc = ha(sym.get_gottp_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSGD16_HA:
      *(ul16 *)loc = ha(sym.get_tlsgd_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSGD16_LO:
      *(ul16 *)loc = lo(sym.get_tlsgd_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSLD16_HA:
      *(ul16 *)loc = ha(ctx.got->get_tlsld_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSLD16_LO:
      *(ul16 *)loc = lo(ctx.got->get_tlsld_addr(ctx) - TOC);
      break;
    case R_PPC64_DTPREL16_HA:
      *(ul16 *)loc = ha(S + A - ctx.dtp_addr);
      break;
    case R_PPC64_TPREL16_HA:
      *(ul16 *)loc = ha(S + A - ctx.tp_addr);
      break;
    case R_PPC64_DTPREL16_LO:
      *(ul16 *)loc = lo(S + A - ctx.dtp_addr);
      break;
    case R_PPC64_TPREL16_LO:
      *(ul16 *)loc = lo(S + A - ctx.tp_addr);
      break;
    case R_PPC64_GOT_TPREL16_LO_DS:
      *(ul16 *)loc |= (sym.get_gottp_addr(ctx) - TOC) & 0xfffc;
      break;
    case R_PPC64_PLTSEQ:
    case R_PPC64_PLTCALL:
    case R_PPC64_TLS:
    case R_PPC64_TLSGD:
    case R_PPC64_TLSLD:
      break;
    default:
      unreachable();
    }
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

    u64 S = frag ? frag->get_addr(ctx) : sym.get_addr(ctx);
    u64 A = frag ? frag_addend : (i64)rel.r_addend;

    switch (rel.r_type) {
    case R_PPC64_ADDR64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A;
      break;
    case R_PPC64_ADDR32: {
      i64 val = S + A;
      check(val, 0, 1LL << 32);
      *(ul32 *)loc = val;
      break;
    }
    case R_PPC64_DTPREL64:
      *(ul64 *)loc = S + A - ctx.dtp_addr;
      break;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
    }
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
      sym.flags.fetch_or(NEEDS_GOT | NEEDS_PLT, std::memory_order_relaxed);

    switch (rel.r_type) {
    case R_PPC64_ADDR64:
      if (name() == ".toc")
        scan_toc_rel(ctx, sym, rel);
      else
        scan_dyn_absrel(ctx, sym, rel);
      break;
    case R_PPC64_GOT_TPREL16_HA:
      sym.flags.fetch_or(NEEDS_GOTTP, std::memory_order_relaxed);
      break;
    case R_PPC64_REL24:
      if (sym.is_imported)
        sym.flags.fetch_or(NEEDS_PLT, std::memory_order_relaxed);
      break;
    case R_PPC64_PLT16_HA:
      sym.flags.fetch_or(NEEDS_GOT, std::memory_order_relaxed);
      break;
    case R_PPC64_GOT_TLSGD16_HA:
      sym.flags.fetch_or(NEEDS_TLSGD, std::memory_order_relaxed);
      break;
    case R_PPC64_GOT_TLSLD16_HA:
      ctx.needs_tlsld.store(true, std::memory_order_relaxed);
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
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <>
void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;

  // If the destination is PLT, we read an address from .got.plt or .got
  // and jump there.
  static const ul32 plt_thunk[] = {
    // Save r2 to the r2 save slot reserved in the caller's stack frame
    0xf841'0018, // std   r2, 24(r1)

    // Jump to a PLT entry
    0x3d82'0000, // addis r12, r2, foo@gotplt@toc@ha
    0xe98c'0000, // ld    r12, foo@gotplt@toc@lo(r12)
    0x7d89'03a6, // mtctr r12
    0x4e80'0420, // bctr
  };

  // If the destination is a non-imported function, we directly jump
  // to its local entry point.
  static const ul32 local_thunk[] = {
    // Jump to a local entry point
    0x3d82'0000, // addis r12, r2,  foo@toc@ha
    0x398c'0000, // addi  r12, r12, foo@toc@lo
    0x7d89'03a6, // mtctr r12
    0x4e80'0420, // bctr
    0x6000'0000, // nop
  };

  static_assert(E::thunk_size == sizeof(plt_thunk));
  static_assert(E::thunk_size == sizeof(local_thunk));

  for (i64 i = 0; i < symbols.size(); i++) {
    Symbol<E> &sym = *symbols[i];
    ul32 *loc = (ul32 *)(buf + i * E::thunk_size);

    if (sym.has_plt(ctx)) {
      memcpy(loc, plt_thunk, sizeof(plt_thunk));
      u64 got = sym.has_got(ctx) ? sym.get_got_addr(ctx) : sym.get_gotplt_addr(ctx);
      i64 val = got - ctx.extra.TOC->value;
      loc[1] |= higha(val);
      loc[2] |= lo(val);
    } else {
      memcpy(loc, local_thunk, sizeof(local_thunk));
      i64 val = sym.get_addr(ctx) + get_local_entry_offset(ctx, sym) -
                ctx.extra.TOC->value;
      loc[0] |= higha(val);
      loc[1] |= lo(val);
    }
  }
}

} // namespace mold::elf
