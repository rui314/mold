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
// function calls. If the linker finds that a branch target is out of reach,
// it emits a small piece of machine code near the branch instruction and
// redirect the branch to the linker-synthesized code. The code constructs a
// full 32-bit address in a register and jump to the destination. That
// linker-synthesized code is called "range extension thunks" or just
// "thunks".
//
// The RISC-V psABI is unique that it works the other way around. That is,
// for RISC-V, the compiler always emits two instructions (AUIPC + JAL) for
// function calls. If the linker finds the destination is reachable with a
// single instruction, it replaces the two instructions with the one and
// shrink the section size by one instruction length, instead of filling the
// gap with a nop.
//
// With the presence of this relaxation, sections can no longer be
// considered as an atomic unit. If we delete 4 bytes from the middle of a
// section, all contents after that point needs to be shifted by 4. Symbol
// values and relocation offsets have to be adjusted accordingly if they
// refer to past the deleted bytes.
//
// In mold, we use `r_deltas` to memorize how many bytes have be adjusted
// for relocations. For symbols, we directly mutate their `value` member.
//
// RISC-V object files tend to have way more relocations than those for
// other targets. This is because all branches, including ones that jump
// within the same section, are explicitly expressed with relocations.
// Here is why we need them: all control-flow statements such as `if` or
// `for` are implemented using branch instructions. For other targets, the
// compiler doesn't emit relocations for such branches because they know
// at compile-time exactly how many bytes has to be skipped. That's not
// true to RISC-V because the linker may delete bytes between a branch and
// its destination. Therefore, all branches including in-section ones have
// to be explicitly expressed with relocations.
//
// Note that this mechanism only shrink sections and never enlarge, as
// the compiler always emits the longest instruction sequence. This
// makes the linker implementation a bit simpler because we don't need
// to worry about oscillation.
//
// https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc

#if MOLD_RV64LE || MOLD_RV64BE || MOLD_RV32LE || MOLD_RV32BE

#include "elf.h"
#include "mold.h"

#include <regex>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

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
  val += 0x800;
  *(ul16 *)loc &= 0b111'0'11111'00000'11;
  *(ul16 *)loc |= bit(val, 17) << 12 | bits(val, 16, 12) << 2;
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

// Returns the rd register of an R/I/U/J-type instruction.
static u32 get_rd(const char *loc) {
  return bits(*(u32 *)loc, 11, 7);
}

static void set_rs1(u8 *loc, u32 rs1) {
  assert(rs1 < 32);
  *(ul32 *)loc &= 0b111111'11111'00000'111'11111'1111111;
  *(ul32 *)loc |= rs1 << 15;
}

template <>
void write_plt_header<E>(Context<E> &ctx, u8 *buf) {
  static const ul32 insn_64[] = {
    0x0000'0397, // auipc  t2, %pcrel_hi(.got.plt)
    0x41c3'0333, // sub    t1, t1, t3               # .plt entry + hdr + 12
    0x0003'be03, // ld     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
    0xfd43'0313, // addi   t1, t1, -44              # .plt entry
    0x0003'8293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
    0x0013'5313, // srli   t1, t1, 1                # .plt entry offset
    0x0082'b283, // ld     t0, 8(t0)                # link map
    0x000e'0067, // jr     t3
  };

  static const ul32 insn_32[] = {
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
  u64 got = sym.get_got_addr(ctx);
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

static inline bool is_hi20(const ElfRel<E> &rel) {
  u32 ty = rel.r_type;
  return ty == R_RISCV_GOT_HI20 || ty == R_RISCV_TLS_GOT_HI20 ||
         ty == R_RISCV_TLS_GD_HI20 || ty == R_RISCV_PCREL_HI20 ||
         ty == R_RISCV_TLSDESC_HI20;
}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  ElfRel<E> *dynrel = nullptr;
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

    auto find_paired_reloc = [&] {
      if (sym.value <= rels[i].r_offset - get_r_delta(i)) {
        for (i64 j = i - 1; j >= 0; j--)
          if (is_hi20(rels[j]) && sym.value == rels[j].r_offset - get_r_delta(j))
            return j;
      } else {
        for (i64 j = i + 1; j < rels.size(); j++)
          if (is_hi20(rels[j]) && sym.value == rels[j].r_offset - get_r_delta(j))
            return j;
      }

      Fatal(ctx) << *this << ": paired relocation is missing: " << i;
    };

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + r_offset;
    u64 G = sym.get_got_idx(ctx) * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;

    switch (rel.r_type) {
    case R_RISCV_32:
      if constexpr (E::is_64)
        *(U32<E> *)loc = S + A;
      else
        apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, &dynrel);
      break;
    case R_RISCV_64:
      assert(E::is_64);
      apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, &dynrel);
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
      i64 rd = get_rd(contents.data() + rel.r_offset + 4);

      // Calling an undefined weak symbol does not make sense.
      // We make such call into an infinite loop. This should
      // help debugging of a faulty program.
      if (sym.esym().is_undef_weak())
        val = 0;

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
        check(val, -(1LL << 31), 1LL << 31);
        write_utype(loc, val);
        write_itype(loc + 4, val);
      }
      break;
    }
    case R_RISCV_GOT_HI20:
      write_utype(loc, G + GOT + A - P);
      break;
    case R_RISCV_TLS_GOT_HI20:
      write_utype(loc, sym.get_gottp_addr(ctx) + A - P);
      break;
    case R_RISCV_TLS_GD_HI20:
      write_utype(loc, sym.get_tlsgd_addr(ctx) + A - P);
      break;
    case R_RISCV_PCREL_HI20:
      write_utype(loc, S + A - P);
      break;
    case R_RISCV_PCREL_LO12_I:
    case R_RISCV_PCREL_LO12_S: {
      i64 idx2 = find_paired_reloc();
      const ElfRel<E> &rel2 = rels[idx2];
      Symbol<E> &sym2 = *file.symbols[rel2.r_sym];

      u64 S = sym2.get_addr(ctx);
      u64 A = rel2.r_addend;
      u64 P = get_addr() + rel2.r_offset - get_r_delta(idx2);
      u64 G = sym2.get_got_idx(ctx) * sizeof(Word<E>);
      u64 val;

      switch (rel2.r_type) {
      case R_RISCV_GOT_HI20:
        val = G + GOT + A - P;
        break;
      case R_RISCV_TLS_GOT_HI20:
        val = sym2.get_gottp_addr(ctx) + A - P;
        break;
      case R_RISCV_TLS_GD_HI20:
        val = sym2.get_tlsgd_addr(ctx) + A - P;
        break;
      case R_RISCV_PCREL_HI20:
        val = S + A - P;
        break;
      default:
        unreachable();
      }

      if (rel.r_type == R_RISCV_PCREL_LO12_I)
        write_itype(loc, val);
      else
        write_stype(loc, val);
      break;
    }
    case R_RISCV_HI20:
      if (removed_bytes == 2) {
        // Rewrite LUI with C.LUI
        i64 rd = get_rd(contents.data() + rel.r_offset);
        *(ul16 *)loc = 0b011'0'00000'00000'01 | (rd << 7);
        write_citype(loc, S + A);
      } else if (removed_bytes == 0) {
        check(S + A, -(1LL << 31), 1LL << 31);
        write_utype(loc, S + A);
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
      if (sign_extend(S + A, 11) == S + A)
        set_rs1(loc, 0);
      break;
    case R_RISCV_TPREL_HI20:
      assert(removed_bytes == 0 || removed_bytes == 4);
      if (removed_bytes == 0)
        write_utype(loc, S + A - ctx.tp_addr);
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
      if (sign_extend(val, 11) == val)
        set_rs1(loc, 4);
      break;
    }
    case R_RISCV_TLSDESC_HI20:
      if (removed_bytes == 0)
        write_utype(loc, sym.get_tlsdesc_addr(ctx) + A - P);
      break;
    case R_RISCV_TLSDESC_LOAD_LO12:
    case R_RISCV_TLSDESC_ADD_LO12:
    case R_RISCV_TLSDESC_CALL: {
      i64 idx2 = find_paired_reloc();
      const ElfRel<E> &rel2 = rels[idx2];
      Symbol<E> &sym2 = *file.symbols[rel2.r_sym];

      u64 S = sym2.get_addr(ctx);
      u64 A = rel2.r_addend;
      u64 P = get_addr() + rel2.r_offset - get_r_delta(idx2);

      switch (rel.r_type) {
      case R_RISCV_TLSDESC_LOAD_LO12:
        if (sym2.has_tlsdesc(ctx))
          write_itype(loc, sym2.get_tlsdesc_addr(ctx) + A - P);
        break;
      case R_RISCV_TLSDESC_ADD_LO12:
        if (sym2.has_tlsdesc(ctx)) {
          write_itype(loc, sym2.get_tlsdesc_addr(ctx) + A - P);
        } else if (sym2.has_gottp(ctx)) {
          *(ul32 *)loc = 0x517;   // auipc a0,<hi20>
          write_utype(loc, sym2.get_gottp_addr(ctx) + A - P);
        } else {
          if (removed_bytes == 0) {
            *(ul32 *)loc = 0x537; // lui a0,<hi20>
            write_utype(loc, S + A - ctx.tp_addr);
          }
        }
        break;
      case R_RISCV_TLSDESC_CALL:
        if (sym2.has_tlsdesc(ctx)) {
          // Do nothing
        } else if (sym2.has_gottp(ctx)) {
          // {ld,lw} a0, <lo12>(a0)
          *(ul32 *)loc = E::is_64 ? 0x53503 : 0x52503;
          write_itype(loc, sym2.get_gottp_addr(ctx) + A - P);
        } else {
          i64 val = S + A - ctx.tp_addr;
          if (sign_extend(val, 11) == val)
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
void InputSection<E>::copy_contents_riscv(Context<E> &ctx, u8 *buf) {
  // If a section is not relaxed, we can copy it as a one big chunk.
  if (extra.r_deltas.empty()) {
    uncompress_to(ctx, buf);
    return;
  }

  // A relaxed section is copied piece-wise.
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

template <>
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<E>);
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
      else
        scan_dyn_absrel(ctx, sym, rel);
      break;
    case R_RISCV_HI20:
      scan_absrel(ctx, sym, rel);
      break;
    case R_RISCV_64:
      if constexpr (!E::is_64)
        Fatal(ctx) << *this << ": R_RISCV_64 cannot be used on RV32";
      scan_dyn_absrel(ctx, sym, rel);
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
    case R_RISCV_TPREL_LO12_I:
    case R_RISCV_TPREL_LO12_S:
    case R_RISCV_TPREL_ADD:
      check_tlsle(ctx, sym, rel);
      break;
    case R_RISCV_BRANCH:
    case R_RISCV_JAL:
    case R_RISCV_PCREL_LO12_I:
    case R_RISCV_PCREL_LO12_S:
    case R_RISCV_LO12_I:
    case R_RISCV_LO12_S:
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

  u32 ret = objs[0]->get_ehdr().e_flags;
  for (i64 i = 1; i < objs.size(); i++) {
    u32 flags = objs[i]->get_ehdr().e_flags;
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

static bool is_resizable(Context<E> &ctx, InputSection<E> *isec) {
  return isec && isec->is_alive && (isec->shdr().sh_flags & SHF_ALLOC) &&
         (isec->shdr().sh_flags & SHF_EXECINSTR);
}

// Returns the distance between a relocated place and a symbol.
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
    // R_RISCV_ALIGN refers to NOP instructions. We need to eliminate some
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

    auto find_paired_reloc = [&] {
      if (sym.value <= rels[i].r_offset) {
        for (i64 j = i - 1; j >= 0; j--)
          if (is_hi20(rels[j]) && sym.value == rels[j].r_offset)
            return j;
      } else {
        for (i64 j = i + 1; j < rels.size(); j++)
          if (is_hi20(rels[j]) && sym.value == rels[j].r_offset)
            return j;
      }

      Fatal(ctx) << isec << ": paired relocation is missing: " << i;
    };

    switch (r.r_type) {
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT: {
      // These relocations refer to an AUIPC + JALR instruction pair to
      // allow to jump to anywhere in PC ± 2 GiB. If the jump target is
      // close enough to PC, we can use C.J, C.JAL or JAL instead.
      i64 dist = compute_distance(ctx, sym, isec, r);
      if (dist & 1)
        break;

      i64 rd = get_rd(isec.contents.data() + r.r_offset + 4);

      if (use_rvc && rd == 0 && sign_extend(dist, 11) == dist) {
        // If rd is x0 and the jump target is within ±2 KiB, we can use
        // C.J, saving 6 bytes.
        delta += 6;
      } else if (use_rvc && !E::is_64 && rd == 1 && sign_extend(dist, 11) == dist) {
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
      u64 val = sym.get_addr(ctx) + r.r_addend;
      i64 rd = get_rd(isec.contents.data() + r.r_offset);

      if (sign_extend(val, 11) == val) {
        // We can replace `lui t0, %hi(foo)` and `add t0, t0, %lo(foo)`
        // instruction pair with `add t0, x0, %lo(foo)` if foo's bits
        // [32:11] are all one or all zero.
        delta += 4;
      } else if (use_rvc && rd != 0 && rd != 2 && sign_extend(val, 17) == val) {
        // If the upper 20 bits can actually be represented in 6 bits,
        // we can use C.LUI instead of LUI.
        delta += 2;
      }
      break;
    }
    case R_RISCV_TPREL_HI20:
    case R_RISCV_TPREL_ADD:
      // These relocations are used to add a high 20-bit value to the
      // thread pointer. The following two instructions materializes
      // TP + HI20(foo) in %r5, for example.
      //
      //  lui  a5,%tprel_hi(foo)         # R_RISCV_TPREL_HI20 (symbol)
      //  add  a5,a5,tp,%tprel_add(foo)  # R_RISCV_TPREL_ADD (symbol)
      //
      // Then thread-local variable `foo` is accessed with a low 12-bit
      // offset like this:
      //
      //  sw   t0,%tprel_lo(foo)(a5)     # R_RISCV_TPREL_LO12_S (symbol)
      //
      // However, if the variable is at TP ±2 KiB, TP + HI20(foo) is the
      // same as TP, so we can instead access the thread-local variable
      // directly using TP like this:
      //
      //  sw   t0,%tprel_lo(foo)(tp)
      //
      // Here, we remove `lui` and `add` if the offset is within ±2 KiB.
      if (i64 val = sym.get_addr(ctx) + r.r_addend - ctx.tp_addr;
          sign_extend(val, 11) == val)
        delta += 4;
      break;
    case R_RISCV_TLSDESC_HI20:
      if (!sym.has_tlsdesc(ctx))
        delta += 4;
      break;
    case R_RISCV_TLSDESC_LOAD_LO12:
    case R_RISCV_TLSDESC_ADD_LO12: {
      const ElfRel<E> &rel2 = rels[find_paired_reloc()];
      Symbol<E> &sym2 = *isec.file.symbols[rel2.r_sym];

      if (r.r_type == R_RISCV_TLSDESC_LOAD_LO12) {
        if (!sym2.has_tlsdesc(ctx))
          delta += 4;
      } else {
        assert(r.r_type == R_RISCV_TLSDESC_ADD_LO12);
        if (!sym2.has_tlsdesc(ctx) && !sym2.has_gottp(ctx))
          if (i64 val = sym2.get_addr(ctx) + rel2.r_addend - ctx.tp_addr;
              sign_extend(val, 11) == val)
            delta += 4;
      }
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
// linker to align the location referred to by the relocation to a
// specified byte boundary. We at least have to interpret them to satisfy
// the alignment constraints.
template <>
i64 riscv_resize_sections<E>(Context<E> &ctx) {
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
// mandatory in .riscv.attributes. Likewise, abbreviations as "g" (which
// is short for "IMAFD") are not allowed in .riscv.attributes.
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
static bool extn_name_less(const Extn &e1, const Extn &e2) {
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

  return std::tuple{get_rank(e1.name), e1.name} <
         std::tuple{get_rank(e2.name), e2.name};
}

static bool extn_version_less(const Extn &e1, const Extn &e2) {
  return std::tuple{e1.major, e1.minor} <
         std::tuple{e2.major, e2.minor};
}

static std::optional<Extn> read_extn_string(std::string_view &str) {
  auto flags = std::regex_constants::optimize | std::regex_constants::ECMAScript;
  static std::regex re(R"(^([a-z]+)(\d+)p(\d+))", flags);

  std::cmatch m;
  if (std::regex_search(str.data(), str.data() + str.size(), m, re)) {
    str = str.substr(m.length());
    return Extn{m[1], (i64)std::stoul(m[2]), (i64)std::stoul(m[3])};
  }
  return {};
}

static std::vector<Extn> parse_arch_string(std::string_view str) {
  if (str.size() < 5)
    return {};

  // Parse the base part
  std::string_view base = str.substr(0, 5);
  if (base != "rv32i" && base != "rv32e" && base != "rv64i" && base != "rv64e")
    return {};
  str = str.substr(4);

  std::optional<Extn> extn = read_extn_string(str);
  if (!extn)
    return {};

  std::vector<Extn> vec;
  extn->name = base;
  vec.push_back(*extn);

  // Parse extensions
  while (!str.empty()) {
    if (str[0] != '_')
      return {};
    str = str.substr(1);

    std::optional<Extn> extn = read_extn_string(str);
    if (!extn)
      return {};
    vec.push_back(*extn);
  }
  return vec;
}

static std::vector<Extn> merge_extensions(std::span<Extn> x, std::span<Extn> y) {
  std::vector<Extn> vec;

  // The base part (i.e. "rv64i" or "rv32i") must match.
  if (x[0].name != y[0].name)
    return {};

  // Merge ISA extension strings
  while (!x.empty() && !y.empty()) {
    if (x[0].name == y[0].name) {
      vec.push_back(extn_version_less(x[0], y[0]) ? y[0] : x[0]);
      x = x.subspan(1);
      y = y.subspan(1);
    } else if (extn_name_less(x[0], y[0])) {
      vec.push_back(x[0]);
      x = x.subspan(1);
    } else {
      vec.push_back(y[0]);
      y = y.subspan(1);
    }
  }

  vec.insert(vec.end(), x.begin(), x.end());
  vec.insert(vec.end(), y.begin(), y.end());
  return vec;
}

static std::string to_string(std::span<Extn> v) {
  std::string str = v[0].name + std::to_string(v[0].major) + "p" +
                    std::to_string(v[0].minor);

  for (i64 i = 1; i < v.size(); i++)
    str += "_" + v[i].name + std::to_string(v[i].major) + "p" +
           std::to_string(v[i].minor);
  return str;
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
  memcpy(ctx.buf + this->shdr.sh_offset, contents.data(), contents.size());
}

} // namespace mold::elf

#endif
