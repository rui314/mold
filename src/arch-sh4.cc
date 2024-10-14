// SH-4 (SuperH 4) is a 32-bit RISC ISA developed by Hitachi in the early
// '90s. Some relatively powerful systems were developed with SH-4.
// A notable example is Sega's Dreamcast game console which debuted in 1998.
// Hitachi later spun off its semiconductor division as an independent
// company, Renesas, and Renesas is still selling SH-4 processors for the
// embedded market. It has never been as popular as ARM is, and its
// popularity continues to decline though.
//
// SH-4's most distinctive feature compared to other RISC ISAs is that its
// instructions are 16 bits in length instead of more common 32 bits for
// better code density. This difference affects various aspects of its
// instruction set as shown below:
//
//  - SH-4 has 16 general-purpose registers (GPRs) instead of the most
//    commmon 32 GPR configuration to save one bit to specify a register.
//
//  - Binary instructions such as ADD normally take three register in
//    RISC ISAs (e.g. x ← y ⊕ z where x, y and z are registers), but
//    SH-4's instructions take only two registers. The result of an
//    operation is written to one of the source registers (e.g. x ← x ⊕ y).
//
//  - Usual RISC ISAs have "load high" and "load low" instructions to set
//    an immediate to most significant and least significant bits in a
//    register to construct a full 32-bit value in a register. This
//    technique is hard to use in SH-4, as 16 bit instructions are too
//    small to contain large immediates. On SH-4, large immediates are
//    loaded from memory using `mov.l` PC-relative load instruction.
//
//  - Many RISC ISAs are, despite their name, actually fairly complex.
//    They tend to have hundreds if not thousands of different instructions.
//    SH-4 doesn't really have that many instructions because its 16-bit
//    machine code simply can't encode many different opcodes. As a
//    result, the number of relocations the linker has to support is also
//    small.
//
// Beside these, SH-4 has a delay branch slot just like contemporary MIPS
// and SPARC. That is, one instruction after a branch instruction will
// always be executed even if the branch is taken. Delay branch slot allows
// a pipelined CPU to start and finish executing an instruction after a
// branch regardless of the branch's condition, simplifying the processor's
// implementation. It's considered a bad premature optimization nowadays,
// though. Modern RISC processors don't have it.
//
// Here are notes about the SH-4 psABI:
//
//  - If a source file is compiled with -fPIC, each function starts
//    with a piece of code to store the address of .got to %r12.
//    We can use the register in our PLT for position-independent output.
//
//  - Even though it uses the RELA-type relocations, relocation addends
//    are stored not to the r_addend field but to the relocated section
//    contents for some reason. Therefore, it's effectively REL.
//
//  - It looks like the ecosystem has bit-rotted. Some tests, especially
//    one using C++ exceptions, don't pass even with GNU ld.
//
//  - GCC/SH4 tends to write dynamically-relocated data into .text, so the
//    output from the linker contains lots of text relocations. That's not
//    a problem with embedded programming, I guess.

#if MOLD_SH4LE || MOLD_SH4BE

#include "mold.h"

namespace mold {

using E = MOLD_TARGET;

// Even though SH-4 uses RELA-type relocations, addends are stored to
// relocated places for some reason.
template <>
i64 get_addend(u8 *loc, const ElfRel<E> &rel) {
  switch (rel.r_type) {
  case R_SH_DIR32:
  case R_SH_REL32:
  case R_SH_TLS_GD_32:
  case R_SH_TLS_LD_32:
  case R_SH_TLS_LDO_32:
  case R_SH_TLS_IE_32:
  case R_SH_TLS_LE_32:
  case R_SH_TLS_DTPMOD32:
  case R_SH_TLS_DTPOFF32:
  case R_SH_TLS_TPOFF32:
  case R_SH_GOT32:
  case R_SH_PLT32:
  case R_SH_GOTOFF:
  case R_SH_GOTPC:
  case R_SH_GOTPLT32:
    return *(U32<E> *)loc;
  default:
    return 0;
  }
}

template <>
void write_addend(u8 *loc, i64 val, const ElfRel<E> &rel) {
  switch (rel.r_type) {
  case R_SH_DIR32:
  case R_SH_REL32:
  case R_SH_TLS_GD_32:
  case R_SH_TLS_LD_32:
  case R_SH_TLS_LDO_32:
  case R_SH_TLS_IE_32:
  case R_SH_TLS_LE_32:
  case R_SH_TLS_DTPMOD32:
  case R_SH_TLS_DTPOFF32:
  case R_SH_TLS_TPOFF32:
  case R_SH_GOT32:
  case R_SH_PLT32:
  case R_SH_GOTOFF:
  case R_SH_GOTPC:
  case R_SH_GOTPLT32:
    *(U32<E> *)loc = val;
  }
}

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  if (ctx.arg.pic) {
    constexpr U16<E> insn[] = {
      0xd202, //    mov.l   1f, r2
      0x32cc, //    add     r12, r2
      0x5022, //    mov.l   @(8, r2), r0
      0x5221, //    mov.l   @(4, r2), r2
      0x402b, //    jmp     @r0
      0xe000, //    mov     #0, r0
      0, 0,   // 1: .long GOTPLT
    };

    static_assert(sizeof(insn) == E::plt_hdr_size);
    memcpy(buf, insn, sizeof(insn));
    *(U32<E> *)(buf + 12) = ctx.gotplt->shdr.sh_addr - ctx.got->shdr.sh_addr;
  } else {
    constexpr U16<E> insn[] = {
      0xd202, //    mov.l   1f, r2
      0x5022, //    mov.l   @(8, r2), r0
      0x5221, //    mov.l   @(4, r2), r2
      0x402b, //    jmp     @r0
      0xe000, //    mov     #0, r0
      0x0009, //    nop
      0, 0,   // 1: .long GOTPLT
    };

    static_assert(sizeof(insn) == E::plt_hdr_size);
    memcpy(buf, insn, sizeof(insn));
    *(U32<E> *)(buf + 12) = ctx.gotplt->shdr.sh_addr;
  }
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  if (ctx.arg.pic) {
    constexpr U16<E> insn[] = {
      0xd001, //    mov.l   1f, r0
      0x00ce, //    mov.l   @(r0, r12), r0
      0x402b, //    jmp     @r0
      0xd101, //    mov.l   2f, r1
      0, 0,   // 1: .long GOTPLT_ENTRY
      0, 0,   // 2: .long INDEX_IN_RELPLT
    };

    static_assert(sizeof(insn) == E::plt_size);
    memcpy(buf, insn, sizeof(insn));
    *(U32<E> *)(buf + 8) = sym.get_gotplt_addr(ctx) - ctx.got->shdr.sh_addr;
    *(U32<E> *)(buf + 12) = sym.get_plt_idx(ctx) * sizeof(ElfRel<E>);
  } else {
    constexpr U16<E> insn[] = {
      0xd001, //    mov.l   1f, r0
      0x6002, //    mov.l   @r0, r0
      0x402b, //    jmp     @r0
      0xd101, //    mov.l   2f, r1
      0, 0,   // 1: .long GOTPLT_ENTRY
      0, 0,   // 2: .long INDEX_IN_RELPLT
    };

    static_assert(sizeof(insn) == E::plt_size);
    memcpy(buf, insn, sizeof(insn));
    *(U32<E> *)(buf + 8) = sym.get_gotplt_addr(ctx);
    *(U32<E> *)(buf + 12) = sym.get_plt_idx(ctx) * sizeof(ElfRel<E>);
  }
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  if (ctx.arg.pic) {
    constexpr U16<E> insn[] = {
      0xd001, //    mov.l   1f, r0
      0x00ce, //    mov.l   @(r0, r12), r0
      0x402b, //    jmp     @r0
      0x0009, //    nop
      0, 0,   // 1: .long GOT_ENTRY
    };

    static_assert(sizeof(insn) == E::pltgot_size);
    memcpy(buf, insn, sizeof(insn));
    *(U32<E> *)(buf + 8) = sym.get_got_pltgot_addr(ctx) - ctx.got->shdr.sh_addr;
  } else {
    constexpr U16<E> insn[] = {
      0xd001, //    mov.l   1f, r0
      0x6002, //    mov.l   @r0, r0
      0x402b, //    jmp     @r0
      0x0009, //    nop
      0, 0,   // 1: .long GOT_ENTRY
    };

    static_assert(sizeof(insn) == E::pltgot_size);
    memcpy(buf, insn, sizeof(insn));
    *(U32<E> *)(buf + 8) = sym.get_got_pltgot_addr(ctx);
  }
}

template <>
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                       u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_SH_DIR32:
    *(U32<E> *)loc = val;
    break;
  case R_SH_REL32:
    *(U32<E> *)loc = val - this->shdr.sh_addr - offset;
    break;
  default:
    Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
  }
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
    u64 A = get_addend(loc, rel);
    u64 P = get_addr() + rel.r_offset;
    u64 G = sym.get_got_idx(ctx) * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;

    switch (rel.r_type) {
    case R_SH_DIR32:
      break;
    case R_SH_REL32:
    case R_SH_PLT32:
      *(U32<E> *)loc = S + A - P;
      break;
    case R_SH_GOT32:
      *(U32<E> *)loc = G;
      break;
    case R_SH_GOTPC:
      *(U32<E> *)loc = GOT + A - P;
      break;
    case R_SH_GOTOFF:
      *(U32<E> *)loc = S + A - GOT;
      break;
    case R_SH_TLS_GD_32:
      *(U32<E> *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      break;
    case R_SH_TLS_LD_32:
      *(U32<E> *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      break;
    case R_SH_TLS_LDO_32:
      *(U32<E> *)loc = S + A - ctx.dtp_addr;
      break;
    case R_SH_TLS_IE_32:
      *(U32<E> *)loc = sym.get_gottp_addr(ctx) + A - GOT;
      break;
    case R_SH_TLS_LE_32:
      *(U32<E> *)loc = S + A - ctx.tp_addr;
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

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

    u64 S = frag ? frag->get_addr(ctx) : sym.get_addr(ctx);
    u64 A = frag ? frag_addend : get_addend(loc, rel);

    switch (rel.r_type) {
    case R_SH_DIR32:
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

template <>
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (sym.is_ifunc())
      Error(ctx) << sym << ": GNU ifunc symbol is not supported on sh4";

    switch (rel.r_type) {
    case R_SH_REL32:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_SH_GOT32:
      sym.flags |= NEEDS_GOT;
      break;
    case R_SH_PLT32:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_SH_TLS_GD_32:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_SH_TLS_LD_32:
      ctx.needs_tlsld = true;
      break;
    case R_SH_TLS_IE_32:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_SH_TLS_LE_32:
      check_tlsle(ctx, sym, rel);
      break;
    case R_SH_DIR32:
    case R_SH_GOTPC:
    case R_SH_GOTOFF:
    case R_SH_TLS_LDO_32:
      break;
    default:
      Fatal(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

} // namespace mold

#endif
