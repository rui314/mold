// RISC-V is a clean RISC ISA. It supports PC-relative load/store for
// position-independent code. Its 32-bit and 64-bit ISAs are almost
// identical. That is, you can think RV32 as a RV64 without 64-bit
// operations. In this file, we support both RV64 and RV32.
//
// From the linker's point of view, the RISC-V's psABI is unique because
// sections in input object files can be shrunk while being copied to the
// output file. That is contrary to other psABIs in which sections are an
// atomic unit of copying. Let me explain it in more details.
//
// Since RISC-V instructions are 16-bit or 32-bit long, there's no way to
// embed a very large immediate into a branch instruction. In fact, JAL
// (jump and link) instruction can jump to only within PC ± 1 MiB because
// its immediate is only 21 bits long. If the destination is out of its
// reach, we need to use two instructions instead; the first instruction
// being AUIPC which sets upper 20 bits to a register and the second being
// JALR with a 12-bit immediate and the register. Combined, they specify a
// 32 bits displacement.
//
// Other RISC ISAs have the same limitation, and they solved the problem by
// letting the linker create so-called "range extension thunks". It works as
// follows: the compiler optimistically emits single jump instructions for
// function calls. If the linker finds that a branch target is unreachable,
// it emits a small piece of machine code near the branch that constructs a
// full 32-bit address in a register and jump to the destination. Then the
// linker redirects the unreachable branch to that linker-synthesized piece
// of code. That code is called "range extension thunks" or just "thunks".
//
// The RISC-V psABI is unique that it works the other way around. That is,
// for RISC-V, the compiler always emits two instructions (AUIPC + JAL) for
// function calls. If the linker finds the destination is reachable with a
// single instruction, it replaces the two instructions with the one and
// shrink the section size by one instruction length, instead of filling the
// gap with a nop.
//
// With the presence of this relaxation, sections can no longer be
// considered as an atomic unit. If we delete 4 bytes form the middle of a
// section, all contents after that point needs to be shifted by 4. Symbol
// values and relocation offsets have to be adjusted accordingly if they
// refer past the deleted bytes.
//
// In mold, we use `r_deltas` to memorize how many bytes have be adjusted
// for relocations. For symbols, we directly mutate their `value` member.
//
// RISC-V object files tend to have way more relocations than those for
// other targets. This is because all branches, including ones that jump
// within the same section, are explicitly annotated with relocations. Here
// is why we need them: all control-flow statements such as `if` or `for`
// are implemented using branch instructions. For other targets, the
// compiler doesn't emit relocations for such branches because they know
// at compile-time exactly how many bytes has to be skipped. That's not true
// to RISC-V because the linker may delete bytes between a branch and its
// destination. Therefore, all branches including in-section ones have to
// be explicitly annotated with relocations.
//
// Note that this mechanism only shrink sections and never enlarge, as
// the compiler always emits the longest instruction sequence. This
// makes the linker implementation a bit simpler because we don't need
// to worry about oscillation.
//
// https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc

#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

static u32 itype(u32 val) {
  return val << 20;
}

static u32 stype(u32 val) {
  return bits(val, 11, 5) << 25 | bits(val, 4, 0) << 7;
}

static u32 btype(u32 val) {
  return bit(val, 12) << 31 | bits(val, 10, 5) << 25 |
         bits(val, 4, 1) << 8 | bit(val, 11) << 7;
}

static u32 utype(u32 val) {
  // U-type instructions are used in combination with I-type
  // instructions. U-type insn sets an immediate to the upper 20-bits
  // of a register. I-type insn sign-extends a 12-bits immediate and
  // add it to a register value to construct a complete value. 0x800
  // is added here to compensate for the sign-extension.
  return (val + 0x800) & 0xffff'f000;
}

static u32 jtype(u32 val) {
  return bit(val, 20) << 31 | bits(val, 10, 1)  << 21 |
         bit(val, 11) << 20 | bits(val, 19, 12) << 12;
}

static u32 cbtype(u32 val) {
  return bit(val, 8) << 12 | bit(val, 4) << 11 | bit(val, 3) << 10 |
         bit(val, 7) << 6  | bit(val, 6) << 5  | bit(val, 2) << 4  |
         bit(val, 1) << 3  | bit(val, 5) << 2;
}

static u32 cjtype(u32 val) {
  return bit(val, 11) << 12 | bit(val, 4)  << 11 | bit(val, 9) << 10 |
         bit(val, 8)  << 9  | bit(val, 10) << 8  | bit(val, 6) << 7  |
         bit(val, 7)  << 6  | bit(val, 3)  << 5  | bit(val, 2) << 4  |
         bit(val, 1)  << 3  | bit(val, 5)  << 2;
}

static void write_itype(u8 *loc, u32 val) {
  u32 mask = 0b000000'00000'11111'111'11111'1111111;
  *(ul32 *)loc = (*(ul32 *)loc & mask) | itype(val);
}

static void write_stype(u8 *loc, u32 val) {
  u32 mask = 0b000000'11111'11111'111'00000'1111111;
  *(ul32 *)loc = (*(ul32 *)loc & mask) | stype(val);
}

static void write_btype(u8 *loc, u32 val) {
  u32 mask = 0b000000'11111'11111'111'00000'1111111;
  *(ul32 *)loc = (*(ul32 *)loc & mask) | btype(val);
}

static void write_utype(u8 *loc, u32 val) {
  u32 mask = 0b000000'00000'00000'000'11111'1111111;
  *(ul32 *)loc = (*(ul32 *)loc & mask) | utype(val);
}

static void write_jtype(u8 *loc, u32 val) {
  u32 mask = 0b000000'00000'00000'000'11111'1111111;
  *(ul32 *)loc = (*(ul32 *)loc & mask) | jtype(val);
}

static void write_cbtype(u8 *loc, u32 val) {
  u32 mask = 0b111'000'111'00000'11;
  *(ul16 *)loc = (*(ul16 *)loc & mask) | cbtype(val);
}

static void write_cjtype(u8 *loc, u32 val) {
  u32 mask = 0b111'00000000000'11;
  *(ul16 *)loc = (*(ul16 *)loc & mask) | cjtype(val);
}

// Returns the rd register of an R/I/U/J-type instruction.
static u32 get_rd(u32 val) {
  return bits(val, 11, 7);
}

static void set_rs1(u8 *loc, u32 rs1) {
  assert(rs1 < 32);
  *(ul32 *)loc &= 0b111111'11111'00000'111'11111'1111111;
  *(ul32 *)loc |= rs1 << 15;
}

template <typename E>
static void write_plt_header(Context<E> &ctx) {
  u8 *buf = ctx.buf + ctx.plt->shdr.sh_offset;

  static const ul32 plt0_64[] = {
    0x0000'0397, // auipc  t2, %pcrel_hi(.got.plt)
    0x41c3'0333, // sub    t1, t1, t3               # .plt entry + hdr + 12
    0x0003'be03, // ld     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
    0xfd43'0313, // addi   t1, t1, -44              # .plt entry
    0x0003'8293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
    0x0013'5313, // srli   t1, t1, 1                # .plt entry offset
    0x0082'b283, // ld     t0, 8(t0)                # link map
    0x000e'0067, // jr     t3
  };

  static const ul32 plt0_32[] = {
    0x0000'0397, // auipc  t2, %pcrel_hi(.got.plt)
    0x41c3'0333, // sub    t1, t1, t3               # .plt entry + hdr + 12
    0x0003'ae03, // lw     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
    0xfd43'0313, // addi   t1, t1, -44              # .plt entry
    0x0003'8293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
    0x0023'5313, // srli   t1, t1, 2                # .plt entry offset
    0x0042'a283, // lw     t0, 4(t0)                # link map
    0x000e'0067, // jr     t3
  };

  if constexpr (E::is_64)
    memcpy(buf, plt0_64, sizeof(plt0_64));
  else
    memcpy(buf, plt0_32, sizeof(plt0_32));

  u64 gotplt = ctx.gotplt->shdr.sh_addr;
  u64 plt = ctx.plt->shdr.sh_addr;

  write_utype(buf, gotplt - plt);
  write_itype(buf + 8, gotplt - plt);
  write_itype(buf + 16, gotplt - plt);
}

static const ul32 plt_entry_64[] = {
  0x0000'0e17, // auipc   t3, %pcrel_hi(function@.got.plt)
  0x000e'3e03, // ld      t3, %pcrel_lo(1b)(t3)
  0x000e'0367, // jalr    t1, t3
  0x0000'0013, // nop
};

static const ul32 plt_entry_32[] = {
  0x0000'0e17, // auipc   t3, %pcrel_hi(function@.got.plt)
  0x000e'2e03, // lw      t3, %pcrel_lo(1b)(t3)
  0x000e'0367, // jalr    t1, t3
  0x0000'0013, // nop
};

template <typename E>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + ctx.plt->shdr.sh_offset;

  write_plt_header(ctx);

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + E::plt_hdr_size + sym->get_plt_idx(ctx) * E::plt_size;
    u64 gotplt = sym->get_gotplt_addr(ctx);
    u64 plt = sym->get_plt_addr(ctx);

    if constexpr (E::is_64)
      memcpy(ent, plt_entry_64, sizeof(plt_entry_64));
    else
      memcpy(ent, plt_entry_32, sizeof(plt_entry_32));

    write_utype(ent, gotplt - plt);
    write_itype(ent + 4, gotplt - plt);
  }
}

template <typename E>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + sym->get_pltgot_idx(ctx) * E::pltgot_size;
    u64 got = sym->get_got_addr(ctx);
    u64 plt = sym->get_plt_addr(ctx);

    if constexpr (E::is_64)
      memcpy(ent, plt_entry_64, sizeof(plt_entry_64));
    else
      memcpy(ent, plt_entry_32, sizeof(plt_entry_32));

    write_utype(ent, got - plt);
    write_itype(ent + 4, got - plt);
  }
}

template <typename E>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_RISCV_ADD32:
    *(ul32 *)loc += val;
    return;
  case R_RISCV_SUB8:
    *loc -= val;
    return;
  case R_RISCV_SUB16:
    *(ul16 *)loc -= val;
    return;
  case R_RISCV_SUB32:
    *(ul32 *)loc -= val;
    return;
  case R_RISCV_SUB6:
    *loc = (*loc & 0b1100'0000) | ((*loc - val) & 0b0011'1111);
    return;
  case R_RISCV_SET6:
    *loc = (*loc & 0b1100'0000) | (val & 0b0011'1111);
    return;
  case R_RISCV_SET8:
    *loc = val;
    return;
  case R_RISCV_SET16:
    *(ul16 *)loc = val;
    return;
  case R_RISCV_SET32:
    *(ul32 *)loc = val;
    return;
  case R_RISCV_32_PCREL:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    return;
  }
  Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
}

template <typename E>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  ElfRel<E> *dynrel = nullptr;
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  if (ctx.reldyn)
    dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                           file.reldyn_offset + this->reldyn_offset);

  auto get_r_delta = [&](i64 idx) {
    return extra.r_deltas.empty() ? 0 : extra.r_deltas[idx];
  };

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || rel.r_type == R_RISCV_RELAX)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    i64 r_offset = rel.r_offset - get_r_delta(i);
    i64 removed_bytes = get_r_delta(i + 1) - get_r_delta(i);
    u8 *loc = base + r_offset;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   sym.get_addr(ctx)
#define A   this->get_addend(rel)
#define P   (get_addr() + r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_RISCV_32:
      if constexpr (E::is_64)
        *(ul32 *)loc = S + A;
      else
        apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_RISCV_64:
      assert(E::is_64);
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_RISCV_BRANCH: {
      i64 val = S + A - P;
      check(val, -(1 << 12), 1 << 12);
      write_btype(loc, val);
      break;
    }
    case R_RISCV_JAL: {
      i64 val = S + A - P;
      check(val, -(1 << 20), 1 << 20);
      write_jtype(loc, val);
      break;
    }
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT: {
      u32 rd = get_rd(*(ul32 *)(contents.data() + rel.r_offset + 4));

      if (removed_bytes == 4) {
        // auipc + jalr -> jal
        *(ul32 *)loc = (rd << 7) | 0b1101111;
        write_jtype(loc, S + A - P);
      } else if (removed_bytes == 6 && rd == 0) {
        // auipc + jalr -> c.j
        *(ul16 *)loc = 0b101'00000000000'01;
        write_cjtype(loc, S + A - P);
      } else if (removed_bytes == 6 && rd == 1) {
        // auipc + jalr -> c.jal
        assert(!E::is_64);
        *(ul16 *)loc = 0b001'00000000000'01;
        write_cjtype(loc, S + A - P);
      } else {
        assert(removed_bytes == 0);
        u64 val = sym.esym().is_undef_weak() ? 0 : S + A - P;
        check(val, -(1LL << 31), 1LL << 31);
        write_utype(loc, val);
        write_itype(loc + 4, val);
      }
      break;
    }
    case R_RISCV_GOT_HI20:
      *(ul32 *)loc = G + GOT + A - P;
      break;
    case R_RISCV_TLS_GOT_HI20:
      *(ul32 *)loc = sym.get_gottp_addr(ctx) + A - P;
      break;
    case R_RISCV_TLS_GD_HI20:
      *(ul32 *)loc = sym.get_tlsgd_addr(ctx) + A - P;
      break;
    case R_RISCV_PCREL_HI20:
      if (sym.esym().is_undef_weak()) {
        // Calling an undefined weak symbol does not make sense.
        // We make such call into an infinite loop. This should
        // help debugging of a faulty program.
        *(ul32 *)loc = 0;
      } else {
        *(ul32 *)loc = S + A - P;
      }
      break;
    case R_RISCV_HI20: {
      i64 val = S + A;
      if (removed_bytes == 0) {
        check(val, -(1LL << 31), 1LL << 31);
        write_utype(loc, val);
      } else {
        assert(removed_bytes == 4);
        assert(sign_extend(val, 11) == val);
      }
      break;
    }
    case R_RISCV_LO12_I:
    case R_RISCV_LO12_S: {
      i64 val = S + A;
      if (rel.r_type == R_RISCV_LO12_I)
        write_itype(loc, val);
      else
        write_stype(loc, val);

      // Rewrite `lw t1, 0(t0)` with `lw t1, 0(x0)` if the address is
      // accessible relative to the zero register. If the upper 20 bits
      // are all zero, the corresponding LUI might have been removed.
      if (sign_extend(val, 11) == val)
        set_rs1(loc, 0);
      break;
    }
    case R_RISCV_TPREL_HI20:
      assert(removed_bytes == 0 || removed_bytes == 4);
      if (removed_bytes == 0)
        write_utype(loc, S + A - ctx.tp_addr);
      break;
    case R_RISCV_TPREL_ADD:
      break;
    case R_RISCV_TPREL_LO12_I:
    case R_RISCV_TPREL_LO12_S: {
      i64 val = S + A - ctx.tp_addr;
      if (rel.r_type == R_RISCV_TPREL_LO12_I)
        write_itype(loc, val);
      else
        write_stype(loc, val);

      // Rewrite `lw t1, 0(t0)` with `lw t1, 0(tp)` if the address is
      // directly accessible using tp. tp is x4.
      if (sign_extend(val, 11) == val)
        set_rs1(loc, 4);
      break;
    }
    case R_RISCV_ADD8:
      loc += S + A;
      break;
    case R_RISCV_ADD16:
      *(ul16 *)loc += S + A;
      break;
    case R_RISCV_ADD32:
      *(ul32 *)loc += S + A;
      break;
    case R_RISCV_ADD64:
      *(ul64 *)loc += S + A;
      break;
    case R_RISCV_SUB8:
      loc -= S + A;
      break;
    case R_RISCV_SUB16:
      *(ul16 *)loc -= S + A;
      break;
    case R_RISCV_SUB32:
      *(ul32 *)loc -= S + A;
      break;
    case R_RISCV_SUB64:
      *(ul64 *)loc -= S + A;
      break;
    case R_RISCV_ALIGN: {
      // A R_RISCV_ALIGN is followed by a NOP sequence. We need to remove
      // zero or more bytes so that the instruction after R_RISCV_ALIGN is
      // aligned to a given alignment boundary.
      //
      // We need to guarantee that the NOP sequence is valid after byte
      // removal (e.g. we can't remove 2 bytes from a 4-byte NOP). For the
      // sake of simplicity, we always rewrite the entire NOP sequence.
      i64 padding_size = align_to(P, bit_ceil(rel.r_addend + 1)) - P;
      assert(padding_size % 2 == 0);

      i64 i = 0;
      for (; i <= padding_size - 4; i += 4)
        *(ul32 *)(loc + i) = 0x0000'0013; // nop
      if (i != padding_size)
        *(ul16 *)(loc + i) = 0x0001;     // c.nop
      break;
    }
    case R_RISCV_RVC_BRANCH: {
      i64 val = S + A - P;
      check(val, -(1 << 8), 1 << 8);
      write_cbtype(loc, val);
      break;
    }
    case R_RISCV_RVC_JUMP: {
      i64 val = S + A - P;
      check(val, -(1 << 11), 1 << 11);
      write_cjtype(loc, val);
      break;
    }
    case R_RISCV_SUB6:
      *loc = (*loc & 0b1100'0000) | ((*loc - (S + A)) & 0b0011'1111);
      break;
    case R_RISCV_SET6:
      *loc = (*loc & 0b1100'0000) | ((S + A) & 0b0011'1111);
      break;
    case R_RISCV_SET8:
      *loc = S + A;
      break;
    case R_RISCV_SET16:
      *(ul16 *)loc = S + A;
      break;
    case R_RISCV_SET32:
      *(ul32 *)loc = S + A;
      break;
    case R_RISCV_32_PCREL:
      *(ul32 *)loc = S + A - P;
      break;
    case R_RISCV_PCREL_LO12_I:
    case R_RISCV_PCREL_LO12_S:
      // These relocations are handled in the next loop.
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

  // Handle PC-relative LO12 relocations. In the above loop, pcrel HI20
  // relocations overwrote instructions with full 32-bit values to allow
  // their corresponding pcrel LO12 relocations to read their values.
  for (i64 i = 0; i < rels.size(); i++) {
    switch (rels[i].r_type)
    case R_RISCV_PCREL_LO12_I:
    case R_RISCV_PCREL_LO12_S: {
      Symbol<E> &sym = *file.symbols[rels[i].r_sym];
      assert(sym.get_input_section() == this);

      u8 *loc = base + rels[i].r_offset - get_r_delta(i);
      u32 val = *(ul32 *)(base + sym.value);

      if (rels[i].r_type == R_RISCV_PCREL_LO12_I)
        write_itype(loc, val);
      else
        write_stype(loc, val);
    }
  }

  // Restore the original instructions pcrel HI20 relocations overwrote.
  for (i64 i = 0; i < rels.size(); i++) {
    switch (rels[i].r_type) {
    case R_RISCV_GOT_HI20:
    case R_RISCV_PCREL_HI20:
    case R_RISCV_TLS_GOT_HI20:
    case R_RISCV_TLS_GD_HI20: {
      u8 *loc = base + rels[i].r_offset - get_r_delta(i);
      u32 val = *(ul32 *)loc;
      *(ul32 *)loc = *(ul32 *)(contents.data() + rels[i].r_offset);
      write_utype(loc, val);
    }
    }
  }
}

template <typename E>
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

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? addend : this->get_addend(rel))

    switch (rel.r_type) {
    case R_RISCV_32:
      *(ul32 *)loc = S + A;
      break;
    case R_RISCV_64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A;
      break;
    case R_RISCV_ADD8:
      *loc += S + A;
      break;
    case R_RISCV_ADD16:
      *(ul16 *)loc += S + A;
      break;
    case R_RISCV_ADD32:
      *(ul32 *)loc += S + A;
      break;
    case R_RISCV_ADD64:
      *(ul64 *)loc += S + A;
      break;
    case R_RISCV_SUB8:
      *loc -= S + A;
      break;
    case R_RISCV_SUB16:
      *(ul16 *)loc -= S + A;
      break;
    case R_RISCV_SUB32:
      *(ul32 *)loc -= S + A;
      break;
    case R_RISCV_SUB64:
      *(ul64 *)loc -= S + A;
      break;
    case R_RISCV_SUB6:
      *loc = (*loc & 0b1100'0000) | ((*loc - (S + A)) & 0b0011'1111);
      break;
    case R_RISCV_SET6:
      *loc = (*loc & 0b1100'0000) | ((S + A) & 0b0011'1111);
      break;
    case R_RISCV_SET8:
      *loc = S + A;
      break;
    case R_RISCV_SET16:
      *(ul16 *)loc = S + A;
      break;
    case R_RISCV_SET32:
      *(ul32 *)loc = S + A;
      break;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
      break;
    }

#undef S
#undef A
  }
}

template <typename E>
void InputSection<E>::copy_contents_riscv(Context<E> &ctx, u8 *buf) {
  // If a section is not relaxed, we can copy it as a one big chunk.
  if (extra.r_deltas.empty()) {
    uncompress_to(ctx, buf);
    return;
  }

  // Memory-allocated sections may be relaxed, so copy each segment
  // individually.
  std::span<const ElfRel<E>> rels = get_rels(ctx);
  i64 pos = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    i64 delta = extra.r_deltas[i + 1] - extra.r_deltas[i];
    if (delta == 0)
      continue;
    assert(delta > 0);

    const ElfRel<E> &r = rels[i];
    memcpy(buf, contents.data() + pos, r.r_offset - pos);
    buf += r.r_offset - pos;
    pos = r.r_offset + delta;
  }

  memcpy(buf, contents.data() + pos, contents.size() - pos);
}

template <typename E>
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
    case R_RISCV_32:
      if constexpr (E::is_64)
        scan_abs_rel(ctx, sym, rel);
      else
        scan_abs_dyn_rel(ctx, sym, rel);
      break;
    case R_RISCV_HI20:
      scan_abs_rel(ctx, sym, rel);
      break;
    case R_RISCV_64:
      if constexpr (!E::is_64)
        Fatal(ctx) << *this << ": R_RISCV_64 cannot be used on RV32";
      scan_abs_dyn_rel(ctx, sym, rel);
      break;
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_RISCV_GOT_HI20:
      sym.flags |= NEEDS_GOT;
      break;
    case R_RISCV_TLS_GOT_HI20:
      ctx.has_gottp_rel = true;
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_RISCV_TLS_GD_HI20:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_RISCV_32_PCREL:
      scan_pcrel_rel(ctx, sym, rel);
      break;
    case R_RISCV_BRANCH:
    case R_RISCV_JAL:
    case R_RISCV_PCREL_HI20:
    case R_RISCV_PCREL_LO12_I:
    case R_RISCV_PCREL_LO12_S:
    case R_RISCV_LO12_I:
    case R_RISCV_LO12_S:
    case R_RISCV_TPREL_HI20:
    case R_RISCV_TPREL_LO12_I:
    case R_RISCV_TPREL_LO12_S:
    case R_RISCV_TPREL_ADD:
    case R_RISCV_ADD8:
    case R_RISCV_ADD16:
    case R_RISCV_ADD32:
    case R_RISCV_ADD64:
    case R_RISCV_SUB8:
    case R_RISCV_SUB16:
    case R_RISCV_SUB32:
    case R_RISCV_SUB64:
    case R_RISCV_ALIGN:
    case R_RISCV_RVC_BRANCH:
    case R_RISCV_RVC_JUMP:
    case R_RISCV_RELAX:
    case R_RISCV_SUB6:
    case R_RISCV_SET6:
    case R_RISCV_SET8:
    case R_RISCV_SET16:
    case R_RISCV_SET32:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <typename E>
static bool is_resizable(Context<E> &ctx, InputSection<E> *isec) {
  return isec && isec->is_alive && (isec->shdr().sh_flags & SHF_ALLOC) &&
         (isec->shdr().sh_flags & SHF_EXECINSTR);
}

// Returns the distance between a relocated place and a symbol.
template <typename E>
static i64 compute_distance(Context<E> &ctx, Symbol<E> &sym,
                            InputSection<E> &isec, const ElfRel<E> &rel) {
  // We handle absolute symbols as if they were infinitely far away
  // because `shrink_section` may increase a distance between a branch
  // instruction and an absolute symbol. Branching to an absolute
  // location is extremely rare in real code, though.
  if (sym.is_absolute())
    return INT32_MAX;

  // Likewise, relocations against weak undefined symbols won't be relaxed.
  if (sym.esym().is_undef_weak())
    return INT32_MAX;

  // Compute a distance between the relocated place and the symbol.
  i64 S = sym.get_addr(ctx);
  i64 A = rel.r_addend;
  i64 P = isec.get_addr() + rel.r_offset;
  return S + A - P;
}

// Scan relocations to shrink sections.
template <typename E>
static void shrink_section(Context<E> &ctx, InputSection<E> &isec, bool use_rvc) {
  std::span<const ElfRel<E>> rels = isec.get_rels(ctx);
  isec.extra.r_deltas.resize(rels.size() + 1);

  i64 delta = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &r = rels[i];
    Symbol<E> &sym = *isec.file.symbols[r.r_sym];
    isec.extra.r_deltas[i] = delta;

    // Handling R_RISCV_ALIGN is mandatory.
    //
    // R_RISCV_ALIGN refers NOP instructions. We need to eliminate some
    // or all of the instructions so that the instruction that immediately
    // follows the NOPs is aligned to a specified alignment boundary.
    if (r.r_type == R_RISCV_ALIGN) {
      // The total bytes of NOPs is stored to r_addend, so the next
      // instruction is r_addend away.
      u64 loc = isec.get_addr() + r.r_offset - delta;
      u64 next_loc = loc + r.r_addend;
      u64 alignment = bit_ceil(r.r_addend + 1);
      assert(alignment <= (1 << isec.p2align));
      delta += next_loc - align_to(loc, alignment);
      continue;
    }

    // Handling other relocations is optional.
    if (!ctx.arg.relax || i == rels.size() - 1 ||
        rels[i + 1].r_type != R_RISCV_RELAX)
      continue;

    // Linker-synthesized symbols haven't been assigned their final
    // values when we are shrinking sections because actual values can
    // be computed only after we fix the file layout. Therefore, we
    // assume that relocations against such symbols are always
    // non-relaxable.
    if (sym.file == ctx.internal_obj)
      continue;

    switch (r.r_type) {
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT: {
      // These relocations refer an AUIPC + JALR instruction pair to
      // allow to jump to anywhere in PC ± 2 GiB. If the jump target is
      // close enough to PC, we can use C.J, C.JAL or JAL instead.
      i64 dist = compute_distance(ctx, sym, isec, r);
      if (dist % 2)
        break;

      std::string_view contents = isec.contents;
      i64 rd = get_rd(*(ul32 *)(contents.data() + r.r_offset + 4));

      if (rd == 0 && sign_extend(dist, 11) == dist && use_rvc) {
        // If rd is x0 and the jump target is within ±2 KiB, we can use
        // C.J, saving 6 bytes.
        delta += 6;
      } else if (rd == 1 && sign_extend(dist, 11) == dist && use_rvc && !E::is_64) {
        // If rd is x1 and the jump target is within ±2 KiB, we can use
        // C.JAL. This is RV32 only because C.JAL is RV32-only instruction.
        delta += 6;
      } else if (sign_extend(dist, 20) == dist) {
        // If the jump target is within ±1 MiB, we can use JAL.
        delta += 4;
      }
      break;
    }
    case R_RISCV_HI20: {
      // If the upper 20 bits are all zero, we can remove LUI.
      // The corresponding instructions referred by LO12_I/LO12_S
      // relocations will use the zero register instead.
      i64 val = sym.get_addr(ctx);
      if (sign_extend(val, 11) == val)
        delta += 4;
      break;
    }
    case R_RISCV_TPREL_HI20:
    case R_RISCV_TPREL_ADD: {
      // These relocations are used to materialize the upper 20 bits of
      // an address relative to the thread pointer as follows:
      //
      //  lui  a5,%tprel_hi(foo)         # R_RISCV_TPREL_HI20 (symbol)
      //  add  a5,a5,tp,%tprel_add(foo)  # R_RISCV_TPREL_ADD (symbol)
      //
      // Then thread-local variable `foo` is accessed with a 12-bit offset
      // like this:
      //
      //  sw   t0,%tprel_lo(foo)(a5)     # R_RISCV_TPREL_LO12_S (symbol)
      //
      // However, if the offset is ±2 KiB, we don't need to materialize
      // the upper 20 bits in a register. We can instead access the
      // thread-local variable directly with TP like this:
      //
      //  sw   t0,%tprel_lo(foo)(tp)
      //
      // Here, we remove `lui` and `add` if the offset is within ±2 KiB.
      i64 val = sym.get_addr(ctx) + r.r_addend - ctx.tp_addr;
      if (sign_extend(val, 11) == val)
        delta += 4;
      break;
    }
    }
  }

  isec.extra.r_deltas[rels.size()] = delta;
  isec.sh_size -= delta;
}

// Shrink sections by interpreting relocations.
//
// This operation seems to be optional, because by default longest
// instructions are being used. However, calling this function is actually
// mandatory because of R_RISCV_ALIGN. R_RISCV_ALIGN is a directive to the
// linker to align the location referred to by the relocation to a specified
// byte boundary. We at least have to interpret them satisfy the constraints
// imposed by R_RISCV_ALIGN relocations.
template <typename E>
i64 riscv_resize_sections(Context<E> &ctx) {
  Timer t(ctx, "riscv_resize_sections");

  // True if we can use the 2-byte instructions. This is usually true on
  // Unix because RV64GC is generally considered the baseline hardware.
  bool use_rvc = get_eflags(ctx) & EF_RISCV_RVC;

  // Find all the relocations that can be relaxed.
  // This step should only shrink sections.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections)
      if (is_resizable(ctx, isec.get()))
        shrink_section(ctx, *isec, use_rvc);
  });

  // Fix symbol values.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->symbols) {
      if (sym->file != file)
        continue;

      InputSection<E> *isec = sym->get_input_section();
      if (!isec || isec->extra.r_deltas.empty())
        continue;

      std::span<const ElfRel<E>> rels = isec->get_rels(ctx);
      auto it = std::lower_bound(rels.begin(), rels.end(), sym->value,
                                 [&](const ElfRel<E> &r, u64 val) {
        return r.r_offset < val;
      });

      sym->value -= isec->extra.r_deltas[it - rels.begin()];
    }
  });

  // Re-compute section offset again to finalize them.
  compute_section_sizes(ctx);
  return set_osec_offsets(ctx);
}

#define INSTANTIATE_RISCV(E)                                                 \
  template void PltSection<E>::copy_buf(Context<E> &);                       \
  template void PltGotSection<E>::copy_buf(Context<E> &);                    \
  template void                                                              \
  EhFrameSection<E>::apply_reloc(Context<E> &, const ElfRel<E> &, u64, u64); \
  template void InputSection<E>::apply_reloc_alloc(Context<E> &, u8 *);      \
  template void InputSection<E>::apply_reloc_nonalloc(Context<E> &, u8 *);   \
  template void InputSection<E>::copy_contents_riscv(Context<E> &, u8 *);    \
  template void InputSection<E>::scan_relocations(Context<E> &);             \
  template i64 riscv_resize_sections(Context<E> &);

INSTANTIATE_RISCV(RISCV64);
INSTANTIATE_RISCV(RISCV32);

} // namespace mold::elf
