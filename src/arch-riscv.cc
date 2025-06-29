// RISC-V is a clean RISC ISA. It supports PC-relative load/store for
// position-independent code. Its 32-bit and 64-bit ISAs are almost
// identical. That is, you can think RV32 as a RV64 without 64-bit
// operations. In this file, we support both RV64 and RV32.
//
// RISC-V is essentially little-endian, but the big-endian version is
// available as an extension. GCC supports `-mbig-endian` to generate
// big-endian code. Even in big-endian mode, machine instructions are
// defined to be encoded in little-endian, though. Only the behavior of
// load/store instructions are different between LE RISC-V and BE RISC-V.
//
// From the linker's point of view, the RISC-V's psABI is unique because
// sections in input object files can be shrunk while being copied to the
// output file. That is contrary to other psABIs in which sections are an
// atomic unit of copying. See file comments in shrink-sections.cc for
// details.
//
// https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc

#if MOLD_RV64LE || MOLD_RV64BE || MOLD_RV32LE || MOLD_RV32BE

#include "mold.h"

#include <regex>

namespace mold {

using E = MOLD_TARGET;

static void write_itype(u8 *loc, u32 val) {
  *(ul32 *)loc &= 0b000000'00000'11111'111'11111'1111111;
  *(ul32 *)loc |= bits(val, 11, 0) << 20;
}

static void write_stype(u8 *loc, u32 val) {
  *(ul32 *)loc &= 0b000000'11111'11111'111'00000'1111111;
  *(ul32 *)loc |= bits(val, 11, 5) << 25 | bits(val, 4, 0) << 7;
}

static void write_btype(u8 *loc, u32 val) {
  *(ul32 *)loc &= 0b000000'11111'11111'111'00000'1111111;
  *(ul32 *)loc |= bit(val, 12) << 31   | bits(val, 10, 5) << 25 |
                  bits(val, 4, 1) << 8 | bit(val, 11) << 7;
}

static void write_utype(u8 *loc, u32 val) {
  *(ul32 *)loc &= 0b000000'00000'00000'000'11111'1111111;

  // U-type instructions are used in combination with I-type
  // instructions. U-type insn sets an immediate to the upper 20-bits
  // of a register. I-type insn sign-extends a 12-bits immediate and
  // adds it to a register value to construct a complete value. 0x800
  // is added here to compensate for the sign-extension.
  *(ul32 *)loc |= (val + 0x800) & 0xffff'f000;
}

static void write_jtype(u8 *loc, u32 val) {
  *(ul32 *)loc &= 0b000000'00000'00000'000'11111'1111111;
  *(ul32 *)loc |= bit(val, 20) << 31 | bits(val, 10, 1)  << 21 |
                  bit(val, 11) << 20 | bits(val, 19, 12) << 12;
}

static void write_citype(u8 *loc, u32 val) {
  *(ul16 *)loc &= 0b111'0'11111'00000'11;
  *(ul16 *)loc |= bit(val, 5) << 12 | bits(val, 4, 0) << 2;
}

static void write_cbtype(u8 *loc, u32 val) {
  *(ul16 *)loc &= 0b111'000'111'00000'11;
  *(ul16 *)loc |= bit(val, 8) << 12 | bit(val, 4) << 11 | bit(val, 3) << 10 |
                  bit(val, 7) << 6  | bit(val, 6) << 5  | bit(val, 2) << 4  |
                  bit(val, 1) << 3  | bit(val, 5) << 2;
}

static void write_cjtype(u8 *loc, u32 val) {
  *(ul16 *)loc &= 0b111'00000000000'11;
  *(ul16 *)loc |= bit(val, 11) << 12 | bit(val, 4)  << 11 | bit(val, 9) << 10 |
                  bit(val, 8)  << 9  | bit(val, 10) << 8  | bit(val, 6) << 7  |
                  bit(val, 7)  << 6  | bit(val, 3)  << 5  | bit(val, 2) << 4  |
                  bit(val, 1)  << 3  | bit(val, 5)  << 2;
}

static void set_rs1(u8 *loc, u32 rs1) {
  assert(rs1 < 32);
  *(ul32 *)loc &= 0b111111'11111'00000'111'11111'1111111;
  *(ul32 *)loc |= rs1 << 15;
}

static u32 get_rd(u8 *loc) {
  return bits(*(u32 *)loc, 11, 7);
};

template <>
void write_plt_header<E>(Context<E> &ctx, u8 *buf) {
  constexpr ul32 insn_64[] = {
    0x0000'0397, // auipc  t2, %pcrel_hi(.got.plt)
    0x41c3'0333, // sub    t1, t1, t3               # .plt entry + hdr + 12
    0x0003'be03, // ld     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
    0xfd43'0313, // addi   t1, t1, -44              # .plt entry
    0x0003'8293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
    0x0013'5313, // srli   t1, t1, 1                # .plt entry offset
    0x0082'b283, // ld     t0, 8(t0)                # link map
    0x000e'0067, // jr     t3
  };

  constexpr ul32 insn_32[] = {
    0x0000'0397, // auipc  t2, %pcrel_hi(.got.plt)
    0x41c3'0333, // sub    t1, t1, t3               # .plt entry + hdr + 12
    0x0003'ae03, // lw     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
    0xfd43'0313, // addi   t1, t1, -44              # .plt entry
    0x0003'8293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
    0x0023'5313, // srli   t1, t1, 2                # .plt entry offset
    0x0042'a283, // lw     t0, 4(t0)                # link map
    0x000e'0067, // jr     t3
  };

  u64 gotplt = ctx.gotplt->shdr.sh_addr;
  u64 plt = ctx.plt->shdr.sh_addr;

  memcpy(buf, E::is_64 ? insn_64 : insn_32, E::plt_hdr_size);
  write_utype(buf, gotplt - plt);
  write_itype(buf + 8, gotplt - plt);
  write_itype(buf + 16, gotplt - plt);
}

static constexpr ul32 plt_entry_64[] = {
  0x0000'0e17, // auipc   t3, %pcrel_hi(function@.got.plt)
  0x000e'3e03, // ld      t3, %pcrel_lo(1b)(t3)
  0x000e'0367, // jalr    t1, t3
  0x0010'0073, // ebreak
};

static constexpr ul32 plt_entry_32[] = {
  0x0000'0e17, // auipc   t3, %pcrel_hi(function@.got.plt)
  0x000e'2e03, // lw      t3, %pcrel_lo(1b)(t3)
  0x000e'0367, // jalr    t1, t3
  0x0010'0073, // ebreak
};

template <>
void write_plt_entry<E>(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  u64 gotplt = sym.get_gotplt_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(buf, E::is_64 ? plt_entry_64 : plt_entry_32, E::plt_size);
  write_utype(buf, gotplt - plt);
  write_itype(buf + 4, gotplt - plt);
}

template <>
void write_pltgot_entry<E>(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  u64 got = sym.get_got_pltgot_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(buf, E::is_64 ? plt_entry_64 : plt_entry_32, E::plt_size);
  write_utype(buf, got - plt);
  write_itype(buf + 4, got - plt);
}

template <>
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                       u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_RISCV_ADD32:
    *(U32<E> *)loc += val;
    break;
  case R_RISCV_SUB8:
    *loc -= val;
    break;
  case R_RISCV_SUB16:
    *(U16<E> *)loc -= val;
    break;
  case R_RISCV_SUB32:
    *(U32<E> *)loc -= val;
    break;
  case R_RISCV_SUB6:
    *loc = (*loc & 0b1100'0000) | ((*loc - val) & 0b0011'1111);
    break;
  case R_RISCV_SET6:
    *loc = (*loc & 0b1100'0000) | (val & 0b0011'1111);
    break;
  case R_RISCV_SET8:
    *loc = val;
    break;
  case R_RISCV_SET16:
    *(U16<E> *)loc = val;
    break;
  case R_RISCV_SET32:
    *(U32<E> *)loc = val;
    break;
  case R_RISCV_32_PCREL:
    *(U32<E> *)loc = val - this->shdr.sh_addr - offset;
    break;
  default:
    Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
  }
}

// RISC-V generally uses the AUIPC + ADDI/LW/SW/etc instruction pair
// to access the AUIPC's address ± 2 GiB. AUIPC materializes the most
// significant 52 bits in a PC-relative manner, and the following
// instruction specifies the remaining least significant 12 bits.
// There are several HI20 and LO12 relocation types for them.
//
// LO12 relocations need to materialize an address relative to AUIPC's
// address, not relative to the instruction that the relocation
// directly refers to.
//
// The problem here is that the instruction pair may not always be
// adjacent. We need a mechanism to find a paired AUIPC for a given
// LO12 relocation. For this purpose, the compiler creates a local
// symbol for each location to which HI20 refers, and the LO12
// relocation refers to that symbol.
//
// This function returns a paired HI20 relocation for a given LO12.
// Since the instructions are typically adjacent, we do a linear
// search.
static const ElfRel<E> &
find_paired_reloc(Context<E> &ctx, InputSection<E> &isec,
                  std::span<const ElfRel<E>> rels,
                  Symbol<E> &sym, i64 i) {
  auto is_hi20 = [](u32 ty) {
    return ty == R_RISCV_GOT_HI20 || ty == R_RISCV_TLS_GOT_HI20 ||
           ty == R_RISCV_TLS_GD_HI20 || ty == R_RISCV_PCREL_HI20 ||
           ty == R_RISCV_TLSDESC_HI20;
  };

  u64 value = sym.esym().st_value;

  if (value <= rels[i].r_offset) {
    for (i64 j = i - 1; j >= 0; j--)
      if (is_hi20(rels[j].r_type) && value == rels[j].r_offset)
        return rels[j];
  } else {
    for (i64 j = i + 1; j < rels.size(); j++)
      if (is_hi20(rels[j].r_type) && value == rels[j].r_offset)
        return rels[j];
  }
  Fatal(ctx) << isec << ": paired relocation is missing: " << i;
}

// Returns true if isec's i'th relocation refers to the following
// GOT-load instructioon pair, which is an expeanded form of
// `la t0, foo` pseudo assembly instruction.
//
// .L0
//   auipc t0, 0      # R_RISCV_GOT_HI20(foo),     R_RISCV_RELAX
//   ld    t0, 0(t0)  # R_RISCV_PCREL_LO12_I(.L0), R_RISCV_RELAX
static bool is_got_load_pair(Context<E> &ctx, InputSection<E> &isec,
                             std::span<const ElfRel<E>> rels, i64 i) {
  u8 *buf = (u8 *)isec.contents.data();
  return i + 3 < rels.size() &&
         rels[i].r_type == R_RISCV_GOT_HI20 &&
         rels[i + 1].r_type == R_RISCV_RELAX &&
         rels[i + 2].r_type == R_RISCV_PCREL_LO12_I &&
         rels[i + 3].r_type == R_RISCV_RELAX &&
         rels[i].r_offset == rels[i + 2].r_offset - 4 &&
         rels[i].r_offset == isec.file.symbols[rels[i + 2].r_sym]->value &&
         get_rd(buf + rels[i].r_offset) == get_rd(buf + rels[i + 2].r_offset);
}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);
  std::span<RelocDelta> deltas = extra.r_deltas;
  i64 k = 0;
  u8 *buf = (u8 *)contents.data();
  RelocationsStats rels_stats;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE || rel.r_type == R_RISCV_RELAX)
      continue;

    i64 removed_bytes = 0;
    i64 r_delta = 0;

    if (!deltas.empty()) {
      while (k < deltas.size() && deltas[k].offset < rel.r_offset)
        k++;
      if (k < deltas.size() && deltas[k].offset == rel.r_offset)
        removed_bytes = get_removed_bytes(deltas, k);
      if (k > 0)
        r_delta = deltas[k - 1].delta;
    }

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    i64 r_offset = rel.r_offset - r_delta;
    u8 *loc = base + r_offset;

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + r_offset;
    u64 G = sym.get_got_idx(ctx) * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (ctx.arg.stats)
        update_relocation_stats(rels_stats, i, val, lo, hi);
      check_range(ctx, i, val, lo, hi);
    };

    auto utype = [&](i64 val) {
      check(val, -(1LL << 31) - 0x800, (1LL << 31) - 0x800);
      write_utype(loc, val);
    };

    switch (rel.r_type) {
    case R_RISCV_32:
      if (E::is_64)
        *(U32<E> *)loc = S + A;
      break;
    case R_RISCV_64:
      break;
    case R_RISCV_BRANCH:
      check(S + A - P, -(1 << 12), 1 << 12);
      write_btype(loc, S + A - P);
      break;
    case R_RISCV_JAL:
      check(S + A - P, -(1 << 20), 1 << 20);
      write_jtype(loc, S + A - P);
      break;
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT: {
      i64 val = S + A - P;
      i64 rd = get_rd(buf + rel.r_offset + 4);

      if (removed_bytes == 4) {
        // auipc + jalr -> jal
        *(ul32 *)loc = (rd << 7) | 0b1101111;
        write_jtype(loc, val);
      } else if (removed_bytes == 6 && rd == 0) {
        // auipc + jalr -> c.j
        *(ul16 *)loc = 0b101'00000000000'01;
        write_cjtype(loc, val);
      } else if (removed_bytes == 6 && rd == 1) {
        // auipc + jalr -> c.jal
        assert(!E::is_64);
        *(ul16 *)loc = 0b001'00000000000'01;
        write_cjtype(loc, val);
      } else {
        assert(removed_bytes == 0);
        utype(val);
        write_itype(loc + 4, val);
      }
      break;
    }
    case R_RISCV_GOT_HI20: {
      // This relocation usually refers to an AUIPC + LD instruction
      // pair to load a symbol value from the GOT. If the symbol value
      // is actually a link-time constant, we can materialize the value
      // directly into a register to eliminate a memory load.
      i64 rd = get_rd(buf + rel.r_offset);

      if (removed_bytes == 6) {
        // c.li <rd>, val
        *(ul16 *)loc = 0b010'0'00000'00000'01 | (rd << 7);
        write_citype(loc, sym.get_addr(ctx));
        i += 3;
      } else if (removed_bytes == 4) {
        // addi <rd>, zero, val
        *(ul32 *)loc = 0b0010011 | (rd << 7);
        write_itype(loc, sym.get_addr(ctx));
        i += 3;
      } else {
        assert(removed_bytes == 0);

        i64 val = S + A - P;
        if (ctx.arg.relax && sym.is_pcrel_linktime_const(ctx) &&
            is_got_load_pair(ctx, *this, rels, i) && is_int(val, 32)) {
          // auipc <rd>, %hi20(val)
          utype(val);

          // addi <rd>, <rd>, %lo12(val)
          *(ul32 *)(loc + 4) = 0b0010011 | (rd << 15) | (rd << 7);
          write_itype(loc + 4, val);
          i += 3;
        } else {
          utype(G + GOT + A - P);
        }
      }
      break;
    }
    case R_RISCV_TLS_GOT_HI20:
      utype(sym.get_gottp_addr(ctx) + A - P);
      break;
    case R_RISCV_TLS_GD_HI20:
      utype(sym.get_tlsgd_addr(ctx) + A - P);
      break;
    case R_RISCV_PCREL_HI20:
      utype(S + A - P);
      break;
    case R_RISCV_PCREL_LO12_I:
    case R_RISCV_PCREL_LO12_S: {
      const ElfRel<E> &rel2 = find_paired_reloc(ctx, *this, rels, sym, i);
      Symbol<E> &sym2 = *file.symbols[rel2.r_sym];

      auto write =
        (rel.r_type == R_RISCV_PCREL_LO12_I) ? write_itype : write_stype;

      u64 S = sym2.get_addr(ctx);
      u64 A = rel2.r_addend;
      u64 P = get_addr() + rel2.r_offset - get_r_delta(*this, rel2.r_offset);
      u64 G = sym2.get_got_idx(ctx) * sizeof(Word<E>);

      switch (rel2.r_type) {
      case R_RISCV_GOT_HI20:
        write(loc, G + GOT + A - P);
        break;
      case R_RISCV_TLS_GOT_HI20:
        write(loc, sym2.get_gottp_addr(ctx) + A - P);
        break;
      case R_RISCV_TLS_GD_HI20:
        write(loc, sym2.get_tlsgd_addr(ctx) + A - P);
        break;
      case R_RISCV_PCREL_HI20:
        write(loc, S + A - P);
        break;
      }
      break;
    }
    case R_RISCV_HI20:
      if (removed_bytes == 2) {
        // Rewrite LUI with C.LUI
        i64 rd = get_rd(buf + rel.r_offset);
        *(ul16 *)loc = 0b011'0'00000'00000'01 | (rd << 7);
        write_citype(loc, (S + A + 0x800) >> 12);
      } else if (removed_bytes == 0) {
        utype(S + A);
      }
      break;
    case R_RISCV_LO12_I:
    case R_RISCV_LO12_S:
      if (rel.r_type == R_RISCV_LO12_I)
        write_itype(loc, S + A);
      else
        write_stype(loc, S + A);

      // Rewrite `lw t1, 0(t0)` with `lw t1, 0(x0)` if the address is
      // accessible relative to the zero register because if that's the
      // case, corresponding LUI might have been removed by relaxation.
      if (is_int(S + A, 12))
        set_rs1(loc, 0);
      break;
    case R_RISCV_TPREL_HI20:
      assert(removed_bytes == 0 || removed_bytes == 4);
      if (removed_bytes == 0)
        utype(S + A - ctx.tp_addr);
      break;
    case R_RISCV_TPREL_ADD:
      // This relocation just annotates an ADD instruction that can be
      // removed when a TPREL is relaxed. No value is needed to be
      // written.
      assert(removed_bytes == 0 || removed_bytes == 4);
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
      if (is_int(val, 12))
        set_rs1(loc, 4);
      break;
    }
    case R_RISCV_TLSDESC_HI20:
      // RISC-V TLSDESC uses the following code sequence to materialize
      // a TP-relative address in a0.
      //
      //   .L0:
      //   auipc  tX, 0
      //       R_RISCV_TLSDESC_HI20         foo
      //   l[d|w] tY, tX, 0
      //       R_RISCV_TLSDESC_LOAD_LO12_I  .L0
      //   addi   a0, tX, 0
      //       R_RISCV_TLSDESC_ADD_LO12_I   .L0
      //   jalr   t0, tY
      //       R_RISCV_TLSDESC_CALL         .L0
      //
      // For non-dlopen'd DSO, we may relax the instructions to the following:
      //
      //   <deleted>
      //   <deleted>
      //   auipc  a0, %gottp_hi(a0)
      //   l[d|w] a0, %gottp_lo(a0)
      //
      // For executable, if the TP offset is small enough, we'll relax
      // it to the following:
      //
      //   <deleted>
      //   <deleted>
      //   <deleted>
      //   addi   a0, zero, %tpoff_lo(a0)
      //
      // Otherwise, the following sequence is used:
      //
      //   <deleted>
      //   <deleted>
      //   lui    a0, %tpoff_hi(a0)
      //   addi   a0, a0, %tpoff_lo(a0)
      //
      // If the code-shrinking relaxation is disabled, we may leave
      // original useless instructions instead of deleting them, but we
      // accept that because relaxations are enabled by default.
      if (sym.has_tlsdesc(ctx) && removed_bytes == 0)
        utype(sym.get_tlsdesc_addr(ctx) + A - P);
      break;
    case R_RISCV_TLSDESC_LOAD_LO12:
    case R_RISCV_TLSDESC_ADD_LO12:
    case R_RISCV_TLSDESC_CALL: {
      if (removed_bytes == 4)
        break;

      const ElfRel<E> &rel2 = find_paired_reloc(ctx, *this, rels, sym, i);
      Symbol<E> &sym2 = *file.symbols[rel2.r_sym];

      u64 S = sym2.get_addr(ctx);
      u64 A = rel2.r_addend;
      u64 P = get_addr() + rel2.r_offset - get_r_delta(*this, rel2.r_offset);

      switch (rel.r_type) {
      case R_RISCV_TLSDESC_LOAD_LO12:
        if (sym2.has_tlsdesc(ctx))
          write_itype(loc, sym2.get_tlsdesc_addr(ctx) + A - P);
        else
          *(ul32 *)loc = 0x13; // nop
        break;
      case R_RISCV_TLSDESC_ADD_LO12:
        if (sym2.has_tlsdesc(ctx)) {
          write_itype(loc, sym2.get_tlsdesc_addr(ctx) + A - P);
        } else if (sym2.has_gottp(ctx)) {
          *(ul32 *)loc = 0x517; // auipc a0,<hi20>
          utype(sym2.get_gottp_addr(ctx) + A - P);
        } else {
          *(ul32 *)loc = 0x537; // lui a0,<hi20>
          utype(S + A - ctx.tp_addr);
        }
        break;
      case R_RISCV_TLSDESC_CALL:
        if (sym2.has_tlsdesc(ctx)) {
          // Do nothing
        } else if (sym2.has_gottp(ctx)) {
          // l[d|w] a0,<lo12>
          *(ul32 *)loc = E::is_64 ? 0x53503 : 0x52503;
          write_itype(loc, sym2.get_gottp_addr(ctx) + A - P);
        } else {
          i64 val = S + A - ctx.tp_addr;
          if (is_int(val, 12))
            *(ul32 *)loc = 0x513;   // addi a0,zero,<lo12>
          else
            *(ul32 *)loc = 0x50513; // addi a0,a0,<lo12>
          write_itype(loc, val);
        }
        break;
      }
      break;
    }
    case R_RISCV_ADD8:
      loc += S + A;
      break;
    case R_RISCV_ADD16:
      *(U16<E> *)loc += S + A;
      break;
    case R_RISCV_ADD32:
      *(U32<E> *)loc += S + A;
      break;
    case R_RISCV_ADD64:
      *(U64<E> *)loc += S + A;
      break;
    case R_RISCV_SUB8:
      loc -= S + A;
      break;
    case R_RISCV_SUB16:
      *(U16<E> *)loc -= S + A;
      break;
    case R_RISCV_SUB32:
      *(U32<E> *)loc -= S + A;
      break;
    case R_RISCV_SUB64:
      *(U64<E> *)loc -= S + A;
      break;
    case R_RISCV_ALIGN: {
      // A R_RISCV_ALIGN is followed by a NOP sequence. We need to remove
      // zero or more bytes so that the instruction after R_RISCV_ALIGN is
      // aligned to a given alignment boundary.
      //
      // We need to guarantee that the NOP sequence is valid after byte
      // removal (e.g. we can't remove the first 2 bytes of a 4-byte NOP).
      // For the sake of simplicity, we always rewrite the entire NOP sequence.
      i64 padding_bytes = rel.r_addend - removed_bytes;
      assert((padding_bytes & 1) == 0);

      i64 i = 0;
      for (; i <= padding_bytes - 4; i += 4)
        *(ul32 *)(loc + i) = 0x0000'0013; // nop
      if (i < padding_bytes)
        *(ul16 *)(loc + i) = 0x0001;      // c.nop
      break;
    }
    case R_RISCV_RVC_BRANCH:
      check(S + A - P, -(1 << 8), 1 << 8);
      write_cbtype(loc, S + A - P);
      break;
    case R_RISCV_RVC_JUMP:
      check(S + A - P, -(1 << 11), 1 << 11);
      write_cjtype(loc, S + A - P);
      break;
    case R_RISCV_SUB6:
      *loc = (*loc & 0b1100'0000) | ((*loc - S - A) & 0b0011'1111);
      break;
    case R_RISCV_SET6:
      *loc = (*loc & 0b1100'0000) | ((S + A) & 0b0011'1111);
      break;
    case R_RISCV_SET8:
      *loc = S + A;
      break;
    case R_RISCV_SET16:
      *(U16<E> *)loc = S + A;
      break;
    case R_RISCV_SET32:
      *(U32<E> *)loc = S + A;
      break;
    case R_RISCV_PLT32:
    case R_RISCV_32_PCREL:
      *(U32<E> *)loc = S + A - P;
      break;
    case R_RISCV_SET_ULEB128:
      overwrite_uleb(loc, S + A);
      break;
    case R_RISCV_SUB_ULEB128:
      overwrite_uleb(loc, read_uleb(loc) - S - A);
      break;
    default:
      unreachable();
    }
  }
  if (ctx.arg.stats)
    save_relocation_stats<E>(ctx, *this, rels_stats);
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
    u64 A = frag ? frag_addend : (i64)rel.r_addend;

    switch (rel.r_type) {
    case R_RISCV_32:
      *(U32<E> *)loc = S + A;
      break;
    case R_RISCV_64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(U64<E> *)loc = *val;
      else
        *(U64<E> *)loc = S + A;
      break;
    case R_RISCV_ADD8:
      *loc += S + A;
      break;
    case R_RISCV_ADD16:
      *(U16<E> *)loc += S + A;
      break;
    case R_RISCV_ADD32:
      *(U32<E> *)loc += S + A;
      break;
    case R_RISCV_ADD64:
      *(U64<E> *)loc += S + A;
      break;
    case R_RISCV_SUB8:
      *loc -= S + A;
      break;
    case R_RISCV_SUB16:
      *(U16<E> *)loc -= S + A;
      break;
    case R_RISCV_SUB32:
      *(U32<E> *)loc -= S + A;
      break;
    case R_RISCV_SUB64:
      *(U64<E> *)loc -= S + A;
      break;
    case R_RISCV_SUB6:
      *loc = (*loc & 0b1100'0000) | ((*loc - S - A) & 0b0011'1111);
      break;
    case R_RISCV_SET6:
      *loc = (*loc & 0b1100'0000) | ((S + A) & 0b0011'1111);
      break;
    case R_RISCV_SET8:
      *loc = S + A;
      break;
    case R_RISCV_SET16:
      *(U16<E> *)loc = S + A;
      break;
    case R_RISCV_SET32:
      *(U32<E> *)loc = S + A;
      break;
    case R_RISCV_SET_ULEB128:
      overwrite_uleb(loc, S + A);
      break;
    case R_RISCV_SUB_ULEB128:
      overwrite_uleb(loc, read_uleb(loc) - S - A);
      break;
    default:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel;
      break;
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
    case R_RISCV_32:
      if constexpr (E::is_64)
        scan_absrel(ctx, sym, rel);
      break;
    case R_RISCV_HI20:
      scan_absrel(ctx, sym, rel);
      break;
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT:
    case R_RISCV_PLT32:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_RISCV_GOT_HI20:
      sym.flags |= NEEDS_GOT;
      break;
    case R_RISCV_TLS_GOT_HI20:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_RISCV_TLS_GD_HI20:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_RISCV_TLSDESC_HI20:
      scan_tlsdesc(ctx, sym);
      break;
    case R_RISCV_32_PCREL:
    case R_RISCV_PCREL_HI20:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_RISCV_TPREL_HI20:
      check_tlsle(ctx, sym, rel);
      break;
    case R_RISCV_64:
    case R_RISCV_BRANCH:
    case R_RISCV_JAL:
    case R_RISCV_PCREL_LO12_I:
    case R_RISCV_PCREL_LO12_S:
    case R_RISCV_LO12_I:
    case R_RISCV_LO12_S:
    case R_RISCV_TPREL_LO12_I:
    case R_RISCV_TPREL_LO12_S:
    case R_RISCV_TPREL_ADD:
    case R_RISCV_TLSDESC_LOAD_LO12:
    case R_RISCV_TLSDESC_ADD_LO12:
    case R_RISCV_TLSDESC_CALL:
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
    case R_RISCV_SET_ULEB128:
    case R_RISCV_SUB_ULEB128:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <>
u64 get_eflags(Context<E> &ctx) {
  std::vector<ObjectFile<E> *> objs = ctx.objs;
  std::erase(objs, ctx.internal_obj);

  if (objs.empty())
    return 0;

  u32 ret = objs[0]->get_eflags();
  for (i64 i = 1; i < objs.size(); i++) {
    u32 flags = objs[i]->get_eflags();
    if (flags & EF_RISCV_RVC)
      ret |= EF_RISCV_RVC;

    if ((flags & EF_RISCV_FLOAT_ABI) != (ret & EF_RISCV_FLOAT_ABI))
      Error(ctx) << *objs[i] << ": cannot link object files with different"
                 << " floating-point ABI from " << *objs[0];

    if ((flags & EF_RISCV_RVE) != (ret & EF_RISCV_RVE))
      Error(ctx) << *objs[i] << ": cannot link object files with different"
                 << " EF_RISCV_RVE from " << *objs[0];
  }
  return ret;
}

// Scan relocations to shrink a given section.
template <>
void shrink_section(Context<E> &ctx, InputSection<E> &isec) {
  std::span<const ElfRel<E>> rels = isec.get_rels(ctx);
  std::vector<RelocDelta> &deltas = isec.extra.r_deltas;
  i64 r_delta = 0;
  u8 *buf = (u8 *)isec.contents.data();

  // True if we can use 2-byte instructions. This is usually true on
  // Unix because RV64GC is generally considered the baseline hardware.
  bool use_rvc = isec.file.get_eflags() & EF_RISCV_RVC;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &r = rels[i];
    Symbol<E> &sym = *isec.file.symbols[r.r_sym];

    auto remove = [&](i64 d) {
      r_delta += d;
      deltas.push_back(RelocDelta{r.r_offset, r_delta});
    };

    // Handling R_RISCV_ALIGN is mandatory.
    //
    // R_RISCV_ALIGN refers to NOP instructions. We need to eliminate some
    // or all of the instructions so that the instruction that immediately
    // follows the NOPs is aligned to a specified alignment boundary.
    if (r.r_type == R_RISCV_ALIGN) {
      // The total bytes of NOPs is stored to r_addend, so the next
      // instruction is r_addend away.
      u64 P = isec.get_addr() + r.r_offset - r_delta;
      u64 desired = align_to(P, bit_ceil(r.r_addend));
      u64 actual = P + r.r_addend;
      if (desired != actual)
        remove(actual - desired);
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
      // These relocations refer to an AUIPC + JALR instruction pair to
      // allow to jump to anywhere in PC ± 2 GiB. If the jump target is
      // close enough to PC, we can use C.J, C.JAL or JAL instead.
      i64 dist = compute_distance(ctx, sym, isec, r);
      if (dist & 1)
        break;

      i64 rd = get_rd(buf + r.r_offset + 4);

      if (use_rvc && rd == 0 && is_int(dist, 12)) {
        // If rd is x0 and the jump target is within ±2 KiB, we can use
        // C.J, saving 6 bytes.
        remove(6);
      } else if (use_rvc && !E::is_64 && rd == 1 && is_int(dist, 12)) {
        // If rd is x1 and the jump target is within ±2 KiB, we can use
        // C.JAL. This is RV32 only because C.JAL is RV32-only instruction.
        remove(6);
      } else if (is_int(dist, 21)) {
        // If the jump target is within ±1 MiB, we can use JAL.
        remove(4);
      }
      break;
    }
    case R_RISCV_GOT_HI20:
      // A GOT_HI20 followed by a PCREL_LO12_I is used to load a value from
      // GOT. If the loaded value is a link-time constant, we can rewrite
      // the instructions to directly materialize the value, eliminating a
      // memory load.
      if (sym.is_absolute() && is_got_load_pair(ctx, isec, rels, i)) {
        u64 val = sym.get_addr(ctx) + r.r_addend;
        if (use_rvc && is_int(val, 6) && get_rd(buf + r.r_offset) != 0) {
          // Replace AUIPC + LD with C.LI.
          remove(6);
        } else if (is_int(val, 12)) {
          // Replace AUIPC + LD with ADDI.
          remove(4);
        }
      }
      break;
    case R_RISCV_HI20: {
      u64 val = sym.get_addr(ctx) + r.r_addend;
      i64 rd = get_rd(buf + r.r_offset);

      if (is_int(val, 12)) {
        // We can replace `lui t0, %hi(foo)` and `add t0, t0, %lo(foo)`
        // instruction pair with `add t0, x0, %lo(foo)` if foo's bits
        // [32:11] are all one or all zero.
        remove(4);
      } else if (use_rvc && rd != 0 && rd != 2 && is_int(val + 0x800, 18)) {
        // If the upper 20 bits can actually be represented in 6 bits,
        // we can use C.LUI instead of LUI.
        remove(2);
      }
      break;
    }
    case R_RISCV_TPREL_HI20:
    case R_RISCV_TPREL_ADD:
      // These relocations are used to add a high 20-bit value to the
      // thread pointer. The following two instructions materializes
      // TP + %tprel_hi20(foo) in %t0, for example.
      //
      //  lui  t0, %tprel_hi(foo)         # R_RISCV_TPREL_HI20
      //  add  t0, t0, tp                 # R_RISCV_TPREL_ADD
      //
      // Then thread-local variable `foo` is accessed with the low
      // 12-bit offset like this:
      //
      //  sw   t0, %tprel_lo(foo)(t0)     # R_RISCV_TPREL_LO12_S
      //
      // However, if the variable is at TP ± 2 KiB, TP + %tprel_hi20(foo)
      // is the same as TP, so we can instead access the thread-local
      // variable directly using TP like this:
      //
      //  sw   t0, %tprel_lo(foo)(tp)
      //
      // Here, we remove `lui` and `add` if the offset is within ±2 KiB.
      if (i64 val = sym.get_addr(ctx) + r.r_addend - ctx.tp_addr;
          is_int(val, 12))
        remove(4);
      break;
    case R_RISCV_TLSDESC_HI20:
      if (!sym.has_tlsdesc(ctx))
        remove(4);
      break;
    case R_RISCV_TLSDESC_LOAD_LO12:
    case R_RISCV_TLSDESC_ADD_LO12: {
      const ElfRel<E> &rel2 = find_paired_reloc(ctx, isec, rels, sym, i);
      Symbol<E> &sym2 = *isec.file.symbols[rel2.r_sym];

      if (r.r_type == R_RISCV_TLSDESC_LOAD_LO12) {
        if (!sym2.has_tlsdesc(ctx))
          remove(4);
      } else {
        assert(r.r_type == R_RISCV_TLSDESC_ADD_LO12);
        if (!sym2.has_tlsdesc(ctx) && !sym2.has_gottp(ctx))
          if (i64 val = sym2.get_addr(ctx) + rel2.r_addend - ctx.tp_addr;
              is_int(val, 12))
            remove(4);
      }
      break;
    }
    }
  }

  isec.sh_size -= r_delta;
}

// ISA name handlers
//
// An example of ISA name is "rv64i2p1_m2p0_a2p1_f2p2_d2p2_c2p0_zicsr2p0".
// An ISA name starts with the base name (e.g. "rv64i2p1") followed by
// ISA extensions separated by underscores.
//
// There are lots of ISA extensions defined for RISC-V, and they are
// identified by name. Some extensions are of single-letter alphabet such
// as "m" or "q". Newer extension names start with "z" followed by one or
// more alphabets (i.e. "zicsr"). "s" and "x" prefixes are reserved
// for supervisor-level extensions and private extensions, respectively.
//
// Each extension consists of a name, a major version and a minor version.
// For example, "m2p0" indicates the "m" extension of version 2.0. "p" is
// just a separator. Versions are often omitted in documents, but they are
// mandatory in .riscv.attributes. Likewise, abbreviations such as "G"
// (which is short for "IMAFD") are not allowed in .riscv.attributes.
//
// Each RISC-V object file contains an ISA string enumerating extensions
// used by the object file. We need to merge input objects' ISA strings
// into a single ISA string.
//
// In order to guarantee string uniqueness, extensions have to be ordered
// in a specific manner. The exact rule is unfortunately a bit complicated.
//
// The following functions takes care of ISA strings.

namespace {
struct Extn {
  std::string name;
  i64 major;
  i64 minor;
};
}

// As per the RISC-V spec, the extension names must be sorted in a very
// specific way, and unfortunately that's not just an alphabetical order.
// For example, rv64imafd is a legal ISA string, whereas rv64iafdm is not.
// The exact rule is somewhat arbitrary.
//
// This function returns true if the first extension name should precede
// the second one as per the rule.
static bool extn_name_less(std::string_view x, std::string_view y) {
  auto get_single_letter_rank = [](char c) -> i64 {
    std::string_view exts = "iemafdqlcbkjtpvnh";
    size_t pos = exts.find_first_of(c);
    if (pos != exts.npos)
      return pos;
    return c - 'a' + exts.size();
  };

  auto get_rank = [&](std::string_view str) -> i64 {
    switch (str[0]) {
    case 'x':
      return 1 << 20;
    case 's':
      return 1 << 19;
    case 'z':
      return (1 << 18) + get_single_letter_rank(str[1]);
    default:
      return get_single_letter_rank(str[0]);
    }
  };

  return std::tuple{get_rank(x), x} < std::tuple{get_rank(y), y};
}

static std::vector<Extn> parse_arch_string(std::string_view str) {
  auto flags = std::regex_constants::optimize | std::regex_constants::ECMAScript;
  static std::regex re(R"(^([a-z]|[a-z][a-z0-9]*[a-z])(\d+)p(\d+)(_|$))", flags);

  std::vector<Extn> vec;

  for (;;) {
    std::cmatch m;
    if (!std::regex_search(str.data(), str.data() + str.size(), m, re))
      return {};

    vec.push_back(Extn{m[1], (i64)std::stoul(m[2]), (i64)std::stoul(m[3])});
    if (m[4].length() == 0)
      return vec;

    str = str.substr(m.length());
  }
}

static std::vector<Extn> merge_extensions(std::span<Extn> x, std::span<Extn> y) {
  std::vector<Extn> vec;

  // The base part (i.e. "rv64i" or "rv32i") must match.
  if (x[0].name != y[0].name)
    return {};

  // Merge ISA extension strings
  while (!x.empty() && !y.empty()) {
    if (x[0].name == y[0].name) {
      if (std::tuple{x[0].major, x[0].minor} < std::tuple{y[0].major, y[0].minor})
        vec.push_back(y[0]);
      else
        vec.push_back(x[0]);
      x = x.subspan(1);
      y = y.subspan(1);
    } else if (extn_name_less(x[0].name, y[0].name)) {
      vec.push_back(x[0]);
      x = x.subspan(1);
    } else {
      vec.push_back(y[0]);
      y = y.subspan(1);
    }
  }

  append(vec, x);
  append(vec, y);
  return vec;
}

static std::string to_string(std::span<Extn> v) {
  std::ostringstream os;
  os << v[0].name << v[0].major << 'p' << v[0].minor;
  for (Extn &e : v.subspan(1))
    os << '_' << e.name << e.major << 'p' << e.minor;
  return os.str();
}

//
// Output .riscv.attributes class
//

template <>
void RiscvAttributesSection<E>::update_shdr(Context<E> &ctx) {
  if (!contents.empty())
    return;

  i64 stack = -1;
  std::vector<Extn> arch;
  bool unaligned = false;

  for (ObjectFile<E> *file : ctx.objs) {
    if (file->extra.stack_align) {
      i64 val = *file->extra.stack_align;
      if (stack != -1 && stack != val)
        Error(ctx) << *file << ": stack alignment requirement mistmatch";
      stack = val;
    }

    if (file->extra.arch) {
      std::vector<Extn> arch2 = parse_arch_string(*file->extra.arch);
      if (arch2.empty())
        Error(ctx) << *file << ": corrupted .riscv.attributes ISA string: "
                   << *file->extra.arch;

      if (arch.empty()) {
        arch = arch2;
      } else {
        arch = merge_extensions(arch, arch2);
        if (arch.empty())
          Error(ctx) << *file << ": incompatible .riscv.attributes ISA string: "
                     << *file->extra.arch;
      }
    }

    if (file->extra.unaligned_access)
      unaligned = true;
  }

  if (arch.empty())
    return;

  std::string arch_str = to_string(arch);
  contents.resize(arch_str.size() + 100);

  u8 *p = (u8 *)contents.data();
  *p++ = 'A';                             // Format version
  U32<E> *sub_sz = (U32<E> *)p;           // Sub-section length
  p += 4;
  p += write_string(p, "riscv");          // Vendor name
  u8 *sub_sub_start = p;
  *p++ = ELF_TAG_FILE;                    // Sub-section tag
  U32<E> *sub_sub_sz = (U32<E> *)p;       // Sub-sub-section length
  p += 4;

  if (stack != -1) {
    p += write_uleb(p, ELF_TAG_RISCV_STACK_ALIGN);
    p += write_uleb(p, stack);
  }

  p += write_uleb(p, ELF_TAG_RISCV_ARCH);
  p += write_string(p, arch_str);

  if (unaligned) {
    p += write_uleb(p, ELF_TAG_RISCV_UNALIGNED_ACCESS);
    p += write_uleb(p, 1);
  }

  i64 sz = p - (u8 *)contents.data();
  *sub_sz = sz - 1;
  *sub_sub_sz = p - sub_sub_start;
  contents.resize(sz);
  this->shdr.sh_size = sz;
}

template <>
void RiscvAttributesSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->shdr.sh_offset, contents);
}

} // namespace mold

#endif
