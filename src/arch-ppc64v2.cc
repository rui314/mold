// This file implements the PowerPC ELFv2 ABI which was standardized in
// 2014. Modern little-endian PowerPC systems are based on this ABI.
// The ABI is often referred to as "ppc64le". This shouldn't be confused
// with "ppc64" which refers to the original, big-endian PowerPC systems.
//
// PPC64 is a bit tricky to support because PC-relative load/store
// instructions hadn't been available until Power10 which debuted in 2021.
// Prior to Power10, it wasn't trivial for position-independent code (PIC)
// to load a value from, for example, .got, as we can't do that with [PC +
// the offset to the .got entry].
//
// In the following, I'll explain how PIC is supported on pre-Power10
// systems first and then explain what has changed with Power10.
//
//
// Position-independent code on Power9 or earlier:
//
// We can get the program counter on older PPC64 systems with the
// following four instructions
//
//   mflr  r1  // save the current link register to r1
//   bl    .+4 // branch to the next instruction as if it were a function
//   mflr  r0  // copy the return address to r0
//   mtlr  r1  // restore the original link register value
//
// , but it's too expensive to do if we do this for each load/store.
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
// A function compiled for pre-Power10 usually has two entry points,
// global and local. The global entry point usually 8 bytes precedes
// the local entry point. In between is the following instructions:
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
//
// Position-independent code on Power10:
//
// Power10 added 8-bytes-long instructions to the ISA. Some of them are
// PC-relative load/store instructions that take 34 bits offsets.
// Functions compiled with `-mcpu=power10` use these instructions for PIC.
// r2 does not have a special meaning in such fucntions.
//
// When a fucntion compiled for Power10 calls a function that uses the TOC
// pointer, we need to compute a correct value for TOC and set it to r2
// before transferring the control to the callee. Thunks are responsible
// for doing it.
//
// `_NOTOC` relocations such as `R_PPC64_REL24_NOTOC` indicate that the
// callee does not use TOC (i.e. compiled with `-mcpu=power10`). If a
// function using TOC is referenced via a `_NOTOC` relocation, that call
// is made through a range extension thunk.
//
//
// Note on section names: the PPC64 psABI uses a weird naming convention
// which calls .got.plt .plt. We ignored that part because it's just
// confusing. Since the runtime only cares about segments, we should be
// able to name sections whatever we want.
//
// https://github.com/rui314/psabi/blob/main/ppc64v2.pdf

#include "mold.h"

namespace mold {

using E = PPC64V2;

static u64 lo(u64 x)    { return x & 0xffff; }
static u64 hi(u64 x)    { return x >> 16; }
static u64 ha(u64 x)    { return (x + 0x8000) >> 16; }
static u64 high(u64 x)  { return (x >> 16) & 0xffff; }
static u64 higha(u64 x) { return ((x + 0x8000) >> 16) & 0xffff; }

static void write34(u8 *loc, u64 x) {
  ul32 *buf = (ul32 *)loc;
  buf[0] = (buf[0] & 0xfffc'0000) | bits(x, 33, 16);
  buf[1] = (buf[1] & 0xffff'0000) | bits(x, 15, 0);
}

// .plt is used only for lazy symbol resolution on PPC64. All PLT
// calls are made via range extension thunks even if they are within
// reach. Thunks read addresses from .got.plt and jump there.
// Therefore, once PLT symbols are resolved and final addresses are
// written to .got.plt, thunks just skip .plt and directly jump to the
// resolved addresses.
template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  constexpr ul32 insn[] = {
    // Get PC
    0x7c08'02a6, // mflr    r0
    0x429f'0005, // bcl     20, 31, 4 // obtain PC
    0x7d68'02a6, // mflr    r11
    0x7c08'03a6, // mtlr    r0

    // Compute the PLT entry index
    0x398c'ffd4, // addi    r12, r12, -44
    0x7c0b'6050, // subf    r0, r11, r12
    0x7800'f082, // rldicl  r0, r0, 62, 2

    // Compute the address of .got.plt
    0x3d6b'0000, // addis   r11, r11, GOTPLT_OFFSET@ha
    0x396b'0000, // addi    r11, r11, GOTPLT_OFFSET@lo

    // Load .got.plt[0] and .got.plt[1] and branch to .got.plt[0]
    0xe98b'0000, // ld      r12, 0(r11)
    0x7d89'03a6, // mtctr   r12
    0xe96b'0008, // ld      r11, 8(r11)
    0x4e80'0420, // bctr
  };

  memcpy(buf, insn, sizeof(insn));

  i64 val = ctx.gotplt->shdr.sh_addr - ctx.plt->shdr.sh_addr - 8;
  *(ul32 *)(buf + 28) |= higha(val);
  *(ul32 *)(buf + 32) |= lo(val);
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
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
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
  i64 val = sym.esym().ppc64_local_entry;
  assert(val <= 7);
  if (val == 7)
    Fatal(ctx) << sym << ": local entry offset 7 is reserved";

  if (val == 0 || val == 1)
    return 0;
  return 1 << val;
}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + rel.r_offset;
    u64 G = sym.get_got_idx(ctx) * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;
    u64 TOC = ctx.extra.TOC->value;

    auto r2save_thunk_addr = [&] { return sym.get_thunk_addr(ctx, P); };
    auto no_r2save_thunk_addr = [&] { return sym.get_thunk_addr(ctx, P) + 8; };

    switch (rel.r_type) {
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
    case R_PPC64_REL24:
      if (sym.has_plt(ctx) || !sym.esym().ppc64_preserves_r2()) {
        i64 val = r2save_thunk_addr() + A - P;
        *(ul32 *)loc |= bits(val, 25, 2) << 2;

        // The thunk saves %r2 to the caller's r2 save slot. We need to
        // restore it after function return. To do so, there's usually a
        // NOP as a placeholder after a BL. 0x6000'0000 is a NOP.
        if (*(ul32 *)(loc + 4) == 0x6000'0000)
          *(ul32 *)(loc + 4) = 0xe841'0018; // ld r2, 24(r1)
      } else {
        i64 val = S + get_local_entry_offset(ctx, sym) + A - P;
        if (int_cast(val, 26) != val)
          val = no_r2save_thunk_addr() + A - P;
        *(ul32 *)loc |= bits(val, 25, 2) << 2;
      }
      break;
    case R_PPC64_REL24_NOTOC:
      if (sym.has_plt(ctx) || sym.esym().ppc64_uses_toc()) {
        i64 val = no_r2save_thunk_addr() + A - P;
        *(ul32 *)loc |= bits(val, 25, 2) << 2;
      } else {
        i64 val = S + A - P;
        if (int_cast(val, 26) != val)
          val = no_r2save_thunk_addr() + A - P;
        *(ul32 *)loc |= bits(val, 25, 2) << 2;
      }
      break;
    case R_PPC64_REL32:
      *(ul32 *)loc = S + A - P;
      break;
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
    case R_PPC64_PLT_PCREL34:
    case R_PPC64_PLT_PCREL34_NOTOC:
    case R_PPC64_GOT_PCREL34:
      write34(loc, G + GOT - P);
      break;
    case R_PPC64_PCREL34:
      write34(loc, S + A - P);
      break;
    case R_PPC64_GOT_TPREL16_HA:
      *(ul16 *)loc = ha(sym.get_gottp_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TPREL16_LO_DS:
      *(ul16 *)loc |= (sym.get_gottp_addr(ctx) - TOC) & 0xfffc;
      break;
    case R_PPC64_GOT_TPREL_PCREL34:
      write34(loc, sym.get_gottp_addr(ctx) - P);
      break;
    case R_PPC64_GOT_TLSGD16_HA:
      *(ul16 *)loc = ha(sym.get_tlsgd_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSGD16_LO:
      *(ul16 *)loc = lo(sym.get_tlsgd_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSGD_PCREL34:
      write34(loc, sym.get_tlsgd_addr(ctx) - P);
      break;
    case R_PPC64_GOT_TLSLD16_HA:
      *(ul16 *)loc = ha(ctx.got->get_tlsld_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSLD16_LO:
      *(ul16 *)loc = lo(ctx.got->get_tlsld_addr(ctx) - TOC);
      break;
    case R_PPC64_GOT_TLSLD_PCREL34:
      write34(loc, ctx.got->get_tlsld_addr(ctx) - P);
      break;
    case R_PPC64_DTPREL16_HA:
      *(ul16 *)loc = ha(S + A - ctx.dtp_addr);
      break;
    case R_PPC64_DTPREL16_LO:
      *(ul16 *)loc = lo(S + A - ctx.dtp_addr);
      break;
    case R_PPC64_DTPREL16_LO_DS:
      *(ul16 *)loc |= (S + A - ctx.dtp_addr) & 0xfffc;
      break;
    case R_PPC64_DTPREL34:
      write34(loc, S + A - ctx.dtp_addr);
      break;
    case R_PPC64_TPREL16_HA:
      *(ul16 *)loc = ha(S + A - ctx.tp_addr);
      break;
    case R_PPC64_TPREL16_LO:
      *(ul16 *)loc = lo(S + A - ctx.tp_addr);
      break;
    case R_PPC64_TPREL16_LO_DS:
      *(ul16 *)loc |= (S + A - ctx.tp_addr) & 0xfffc;
      break;
    case R_PPC64_TPREL34:
      write34(loc, S + A - ctx.tp_addr);
      break;
    case R_PPC64_ADDR64:
    case R_PPC64_PLTSEQ:
    case R_PPC64_PLTSEQ_NOTOC:
    case R_PPC64_PLTCALL:
    case R_PPC64_PLTCALL_NOTOC:
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
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

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
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (sym.is_ifunc())
      sym.flags |= NEEDS_GOT | NEEDS_PLT;

    switch (rel.r_type) {
    case R_PPC64_GOT_TPREL16_HA:
    case R_PPC64_GOT_TPREL_PCREL34:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_PPC64_REL24:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_PPC64_REL24_NOTOC:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      ctx.extra.is_power10 = true;
      break;
    case R_PPC64_PLT16_HA:
    case R_PPC64_PLT_PCREL34:
    case R_PPC64_PLT_PCREL34_NOTOC:
    case R_PPC64_GOT_PCREL34:
      sym.flags |= NEEDS_GOT;
      break;
    case R_PPC64_GOT_TLSGD16_HA:
    case R_PPC64_GOT_TLSGD_PCREL34:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_PPC64_GOT_TLSLD16_HA:
    case R_PPC64_GOT_TLSLD_PCREL34:
      ctx.needs_tlsld = true;
      break;
    case R_PPC64_TPREL16_HA:
    case R_PPC64_TPREL16_LO:
    case R_PPC64_TPREL16_LO_DS:
    case R_PPC64_TPREL34:
      check_tlsle(ctx, sym, rel);
      break;
    case R_PPC64_ADDR64:
    case R_PPC64_REL32:
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
    case R_PPC64_PCREL34:
    case R_PPC64_PLTSEQ:
    case R_PPC64_PLTSEQ_NOTOC:
    case R_PPC64_PLTCALL:
    case R_PPC64_PLTCALL_NOTOC:
    case R_PPC64_GOT_TPREL16_LO_DS:
    case R_PPC64_GOT_TLSGD16_LO:
    case R_PPC64_GOT_TLSLD16_LO:
    case R_PPC64_TLS:
    case R_PPC64_TLSGD:
    case R_PPC64_TLSLD:
    case R_PPC64_DTPREL16_HA:
    case R_PPC64_DTPREL16_LO:
    case R_PPC64_DTPREL16_LO_DS:
    case R_PPC64_DTPREL34:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <>
void Thunk<E>::copy_buf(Context<E> &ctx) {
  // If the destination is PLT, we read an address from .got.plt or .got
  // and jump there.
  constexpr ul32 plt_thunk[] = {
    0xf841'0018, // std   r2, 24(r1)
    0x6000'0000, // nop
    0x3d82'0000, // addis r12, r2, foo@gotplt@toc@ha
    0xe98c'0000, // ld    r12, foo@gotplt@toc@lo(r12)
    0x7d89'03a6, // mtctr r12
    0x4e80'0420, // bctr
  };

  constexpr ul32 plt_thunk_power10[] = {
    0xf841'0018, // std   r2, 24(r1)
    0x6000'0000, // nop
    0x0410'0000, // pld   r12, foo@gotplt@pcrel
    0xe580'0000,
    0x7d89'03a6, // mtctr r12
    0x4e80'0420, // bctr
  };

  // If the destination is a non-imported function, we directly jump
  // to its local entry point.
  constexpr ul32 local_thunk[] = {
    0xf841'0018, // std   r2, 24(r1)
    0x6000'0000, // nop
    0x3d82'0000, // addis r12, r2,  foo@toc@ha
    0x398c'0000, // addi  r12, r12, foo@toc@lo
    0x7d89'03a6, // mtctr r12
    0x4e80'0420, // bctr
  };

  constexpr ul32 local_thunk_power10[] = {
    0xf841'0018, // std   r2, 24(r1)
    0x6000'0000, // nop
    0x0610'0000, // pla   r12, foo@pcrel
    0x3980'0000,
    0x7d89'03a6, // mtctr r12
    0x4e80'0420, // bctr
  };

  static_assert(E::thunk_size == sizeof(plt_thunk));
  static_assert(E::thunk_size == sizeof(plt_thunk_power10));
  static_assert(E::thunk_size == sizeof(local_thunk));
  static_assert(E::thunk_size == sizeof(local_thunk_power10));

  u8 *buf = ctx.buf + output_section.shdr.sh_offset + offset;
  u64 P = output_section.shdr.sh_addr + offset;
  u64 TOC = ctx.extra.TOC->value;

  for (Symbol<E> *sym : symbols) {
    if (sym->has_plt(ctx)) {
      u64 got =
        sym->has_got(ctx) ? sym->get_got_addr(ctx) : sym->get_gotplt_addr(ctx);

      if (ctx.extra.is_power10) {
        memcpy(buf, plt_thunk_power10, E::thunk_size);
        write34(buf + 8, got - P - 8);
      } else {
        memcpy(buf, plt_thunk, E::thunk_size);
        *(ul32 *)(buf + 8) |= higha(got - TOC);
        *(ul32 *)(buf + 12) |= lo(got - TOC);
      }
    } else {
      u64 S = sym->get_addr(ctx);
      if (ctx.extra.is_power10) {
        memcpy(buf, local_thunk_power10, E::thunk_size);
        write34(buf + 8, S - P - 8);
      } else {
        memcpy(buf, local_thunk, E::thunk_size);
        *(ul32 *)(buf + 8) |= higha(S - TOC);
        *(ul32 *)(buf + 12) |= lo(S - TOC);
      }
    }

    buf += E::thunk_size;
    P += E::thunk_size;
  }
}

// GCC may emit references to the following functions in function prologue
// and epiilogue if -Os is specified. For some reason, these functions are
// not in libgcc.a and expected to be synthesized by the linker.
const std::vector<std::pair<std::string_view, u32>>
ppc64_save_restore_insns = {
  { "_savegpr0_14", 0xf9c1ff70 }, // std r14,-144(r1)
  { "_savegpr0_15", 0xf9e1ff78 }, // std r15,-136(r1)
  { "_savegpr0_16", 0xfa01ff80 }, // std r16,-128(r1)
  { "_savegpr0_17", 0xfa21ff88 }, // std r17,-120(r1)
  { "_savegpr0_18", 0xfa41ff90 }, // std r18,-112(r1)
  { "_savegpr0_19", 0xfa61ff98 }, // std r19,-104(r1)
  { "_savegpr0_20", 0xfa81ffa0 }, // std r20,-96(r1)
  { "_savegpr0_21", 0xfaa1ffa8 }, // std r21,-88(r1)
  { "_savegpr0_22", 0xfac1ffb0 }, // std r22,-80(r1)
  { "_savegpr0_23", 0xfae1ffb8 }, // std r23,-72(r1)
  { "_savegpr0_24", 0xfb01ffc0 }, // std r24,-64(r1)
  { "_savegpr0_25", 0xfb21ffc8 }, // std r25,-56(r1)
  { "_savegpr0_26", 0xfb41ffd0 }, // std r26,-48(r1)
  { "_savegpr0_27", 0xfb61ffd8 }, // std r27,-40(r1)
  { "_savegpr0_28", 0xfb81ffe0 }, // std r28,-32(r1)
  { "_savegpr0_29", 0xfba1ffe8 }, // std r29,-24(r1)
  { "_savegpr0_30", 0xfbc1fff0 }, // std r30,-16(r1)
  { "_savegpr0_31", 0xfbe1fff8 }, // std r31,-8(r1)
  { "",             0xf8010010 }, // std r0,16(r1)
  { "",             0x4e800020 }, // blr

  { "_restgpr0_14", 0xe9c1ff70 }, // ld r14,-144(r1)
  { "_restgpr0_15", 0xe9e1ff78 }, // ld r15,-136(r1)
  { "_restgpr0_16", 0xea01ff80 }, // ld r16,-128(r1)
  { "_restgpr0_17", 0xea21ff88 }, // ld r17,-120(r1)
  { "_restgpr0_18", 0xea41ff90 }, // ld r18,-112(r1)
  { "_restgpr0_19", 0xea61ff98 }, // ld r19,-104(r1)
  { "_restgpr0_20", 0xea81ffa0 }, // ld r20,-96(r1)
  { "_restgpr0_21", 0xeaa1ffa8 }, // ld r21,-88(r1)
  { "_restgpr0_22", 0xeac1ffb0 }, // ld r22,-80(r1)
  { "_restgpr0_23", 0xeae1ffb8 }, // ld r23,-72(r1)
  { "_restgpr0_24", 0xeb01ffc0 }, // ld r24,-64(r1)
  { "_restgpr0_25", 0xeb21ffc8 }, // ld r25,-56(r1)
  { "_restgpr0_26", 0xeb41ffd0 }, // ld r26,-48(r1)
  { "_restgpr0_27", 0xeb61ffd8 }, // ld r27,-40(r1)
  { "_restgpr0_28", 0xeb81ffe0 }, // ld r28,-32(r1)
  { "_restgpr0_29", 0xe8010010 }, // ld r0,16(r1)
  { "",             0xeba1ffe8 }, // ld r29,-24(r1)
  { "",             0x7c0803a6 }, // mtlr r0
  { "",             0xebc1fff0 }, // ld r30,-16(r1)
  { "",             0xebe1fff8 }, // ld r31,-8(r1)
  { "",             0x4e800020 }, // blr
  { "_restgpr0_30", 0xebc1fff0 }, // ld r30,-16(r1)
  { "_restgpr0_31", 0xe8010010 }, // ld r0,16(r1)
  { "",             0xebe1fff8 }, // ld r31,-8(r1)
  { "",             0x7c0803a6 }, // mtlr r0
  { "",             0x4e800020 }, // blr

  { "_savegpr1_14", 0xf9ccff70 }, // std r14,-144(r12)
  { "_savegpr1_15", 0xf9ecff78 }, // std r15,-136(r12)
  { "_savegpr1_16", 0xfa0cff80 }, // std r16,-128(r12)
  { "_savegpr1_17", 0xfa2cff88 }, // std r17,-120(r12)
  { "_savegpr1_18", 0xfa4cff90 }, // std r18,-112(r12)
  { "_savegpr1_19", 0xfa6cff98 }, // std r19,-104(r12)
  { "_savegpr1_20", 0xfa8cffa0 }, // std r20,-96(r12)
  { "_savegpr1_21", 0xfaacffa8 }, // std r21,-88(r12)
  { "_savegpr1_22", 0xfaccffb0 }, // std r22,-80(r12)
  { "_savegpr1_23", 0xfaecffb8 }, // std r23,-72(r12)
  { "_savegpr1_24", 0xfb0cffc0 }, // std r24,-64(r12)
  { "_savegpr1_25", 0xfb2cffc8 }, // std r25,-56(r12)
  { "_savegpr1_26", 0xfb4cffd0 }, // std r26,-48(r12)
  { "_savegpr1_27", 0xfb6cffd8 }, // std r27,-40(r12)
  { "_savegpr1_28", 0xfb8cffe0 }, // std r28,-32(r12)
  { "_savegpr1_29", 0xfbacffe8 }, // std r29,-24(r12)
  { "_savegpr1_30", 0xfbccfff0 }, // std r30,-16(r12)
  { "_savegpr1_31", 0xfbecfff8 }, // std r31,-8(r12)
  { "",             0x4e800020 }, // blr

  { "_restgpr1_14", 0xe9ccff70 }, // ld r14,-144(r12)
  { "_restgpr1_15", 0xe9ecff78 }, // ld r15,-136(r12)
  { "_restgpr1_16", 0xea0cff80 }, // ld r16,-128(r12)
  { "_restgpr1_17", 0xea2cff88 }, // ld r17,-120(r12)
  { "_restgpr1_18", 0xea4cff90 }, // ld r18,-112(r12)
  { "_restgpr1_19", 0xea6cff98 }, // ld r19,-104(r12)
  { "_restgpr1_20", 0xea8cffa0 }, // ld r20,-96(r12)
  { "_restgpr1_21", 0xeaacffa8 }, // ld r21,-88(r12)
  { "_restgpr1_22", 0xeaccffb0 }, // ld r22,-80(r12)
  { "_restgpr1_23", 0xeaecffb8 }, // ld r23,-72(r12)
  { "_restgpr1_24", 0xeb0cffc0 }, // ld r24,-64(r12)
  { "_restgpr1_25", 0xeb2cffc8 }, // ld r25,-56(r12)
  { "_restgpr1_26", 0xeb4cffd0 }, // ld r26,-48(r12)
  { "_restgpr1_27", 0xeb6cffd8 }, // ld r27,-40(r12)
  { "_restgpr1_28", 0xeb8cffe0 }, // ld r28,-32(r12)
  { "_restgpr1_29", 0xebacffe8 }, // ld r29,-24(r12)
  { "_restgpr1_30", 0xebccfff0 }, // ld r30,-16(r12)
  { "_restgpr1_31", 0xebecfff8 }, // ld r31,-8(r12)
  { "",             0x4e800020 }, // blr
};

void PPC64SaveRestoreSection::copy_buf(Context<E> &ctx) {
  ul32 *buf = (ul32 *)(ctx.buf + this->shdr.sh_offset);
  for (auto [label, insn] : ppc64_save_restore_insns)
    *buf++ = insn;
}

template <>
u64 get_eflags(Context<E> &ctx) {
  return 2;
}

} // namespace mold
