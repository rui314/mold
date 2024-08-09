// LoongArch is a new RISC ISA announced in 2021 by Loongson. The ISA
// feels like a modernized MIPS with a hint of RISC-V flavor, although
// it's not compatible with either one.
//
// While LoongArch is a fresh and clean ISA, its technological advantage
// over other modern RISC ISAs such as RISC-V doesn't seem to be very
// significant. It appears that the real selling point of LoongArch is
// that the ISA is developed and controlled by a Chinese company,
// reflecting a desire for domestic CPUs. Loongson is actively working on
// bootstrapping the entire ecosystem for LoongArch, sending patches to
// Linux, GCC, LLVM, etc.
//
// Speaking of the ISA, all instructions are 4 byte long and aligned to 4
// byte boundaries in LoongArch. It has 32 general-purpose registers.
// Among these, $t0 - $t8 (aliases for $r12 - $r20) are temporary
// registers that we can use in our PLT.
//
// Just like RISC-V, LoongArch supports section-shrinking relaxations.
// That is, it allows linkers to rewrite certain instruction sequences to
// shorter ones. Sections are not an atomic unit of copying.
//
// https://github.com/loongson/la-abi-specs/blob/release/laelf.adoc

#if MOLD_LOONGARCH64 || MOLD_LOONGARCH32

#include "mold.h"

namespace mold::elf {

using E = MOLD_TARGET;

static u64 page(u64 val) {
  return val & 0xffff'ffff'ffff'f000;
}

static u64 hi20(u64 val, u64 pc) {
  // A PC-relative address with a 32 bit offset is materialized in a
  // register with the following instructions:
  //
  //   pcalau12i $rN, %pc_hi20(sym)
  //   addi.d    $rN, $rN, %lo12(sym)
  //
  // PCALAU12I materializes bits [63:12] by computing (pc + imm << 12)
  // and zero-clear [11:0]. ADDI.D sign-extends its 12 bit immediate and
  // add it to the register. To compensate the sign-extension, PCALAU12I
  // needs to materialize a 0x1000 larger value than the desired [63:12]
  // if [11:0] is sign-extended.
  //
  // This is similar but different from RISC-V because RISC-V's AUIPC
  // doesn't zero-clear [11:0].
  return bits(page(val + 0x800) - page(pc), 31, 12);
}

static u64 hi64(u64 val, u64 pc) {
  // A PC-relative 64-bit address is materialized with the following
  // instructions for the large code model:
  //
  //   pcalau12i $rN, %pc_hi20(sym)
  //   addi.d    $rM, $zero, %lo12(sym)
  //   lu32i.d   $rM, %pc64_lo20(sym)
  //   lu52i.d   $rM, $r12, %pc64_hi12(sym)
  //   add.d     $rN, $rN, $rM
  //
  // PCALAU12I computes (pc + imm << 12) to materialize a 64-bit value.
  // ADDI.D adds a sign-extended 12 bit value to a register. LU32I.D and
  // LU52I.D simply set bits to [51:31] and to [63:53], respectively.
  //
  // Compensating all the sign-extensions is a bit complicated.
  u64 x = page(val) - page(pc);
  if (val & 0x800)
    x += 0x1000 - 0x1'0000'0000;
  if (x & 0x8000'0000)
    x += 0x1'0000'0000;
  return x;
}

static u64 higher20(u64 val, u64 pc) {
  return bits(hi64(val, pc), 51, 32);
}

static u64 highest12(u64 val, u64 pc) {
  return bits(hi64(val, pc), 63, 52);
}

static void write_k12(u8 *loc, u32 val) {
  // opcode, [11:0], rj, rd
  *(ul32 *)loc &= 0b1111111111'000000000000'11111'11111;
  *(ul32 *)loc |= bits(val, 11, 0) << 10;
}

static void write_k16(u8 *loc, u32 val) {
  // opcode, [15:0], rj, rd
  *(ul32 *)loc &= 0b111111'0000000000000000'11111'11111;
  *(ul32 *)loc |= bits(val, 15, 0) << 10;
}

static void write_j20(u8 *loc, u32 val) {
  // opcode, [19:0], rd
  *(ul32 *)loc &= 0b1111111'00000000000000000000'11111;
  *(ul32 *)loc |= bits(val, 19, 0) << 5;
}

static void write_d5k16(u8 *loc, u32 val) {
  // opcode, [15:0], rj, [20:16]
  *(ul32 *)loc &= 0b111111'0000000000000000'11111'00000;
  *(ul32 *)loc |= bits(val, 15, 0) << 10;
  *(ul32 *)loc |= bits(val, 20, 16);
}

static void write_d10k16(u8 *loc, u32 val) {
  // opcode, [15:0], [25:16]
  *(ul32 *)loc &= 0b111111'0000000000000000'0000000000;
  *(ul32 *)loc |= bits(val, 15, 0) << 10;
  *(ul32 *)loc |= bits(val, 25, 16);
}

static u32 get_rd(u32 insn) {
  return bits(insn, 4, 0);
}

static u32 get_rj(u32 insn) {
  return bits(insn, 9, 5);
}

static void set_rj(u8 *loc, u32 rj) {
  assert(rj < 32);
  *(ul32 *)loc &= 0b111111'1111111111111111'00000'11111;
  *(ul32 *)loc |= rj << 5;
}

// Returns true if isec's i'th relocation refers to the following
// relaxable instructioon pair.
//
//   pcalau12i $t0, 0         # R_LARCH_GOT_PC_HI20
//   ld.d      $t0, $t0, 0    # R_LARCH_GOT_PC_LO12
static bool is_relaxable_got_load(Context<E> &ctx, InputSection<E> &isec, i64 i) {
  std::span<const ElfRel<E>> rels = isec.get_rels(ctx);
  Symbol<E> &sym = *isec.file.symbols[rels[i].r_sym];

  if (ctx.arg.relax &&
      sym.is_pcrel_linktime_const(ctx) &&
      i + 3 < rels.size() &&
      rels[i + 2].r_type == R_LARCH_GOT_PC_LO12 &&
      rels[i + 2].r_offset == rels[i].r_offset + 4 &&
      rels[i + 3].r_type == R_LARCH_RELAX) {
    u32 insn1 = *(ul32 *)(isec.contents.data() + rels[i].r_offset);
    u32 insn2 = *(ul32 *)(isec.contents.data() + rels[i].r_offset + 4);
    bool is_ld_d = (insn2 & 0xffc0'0000) == 0x28c0'0000;
    return get_rd(insn1) == get_rd(insn2) && get_rd(insn2) == get_rj(insn2) &&
           is_ld_d;
  }
  return false;
}

template <>
void write_plt_header<E>(Context<E> &ctx, u8 *buf) {
  constexpr ul32 insn_64[] = {
    0x1a00'000e, // pcalau12i $t2, %pc_hi20(.got.plt)
    0x0011'bdad, // sub.d     $t1, $t1, $t3
    0x28c0'01cf, // ld.d      $t3, $t2, %lo12(.got.plt) # _dl_runtime_resolve
    0x02ff'51ad, // addi.d    $t1, $t1, -44             # .plt entry
    0x02c0'01cc, // addi.d    $t0, $t2, %lo12(.got.plt) # &.got.plt
    0x0045'05ad, // srli.d    $t1, $t1, 1               # .plt entry offset
    0x28c0'218c, // ld.d      $t0, $t0, 8               # link map
    0x4c00'01e0, // jr        $t3
  };

  constexpr ul32 insn_32[] = {
    0x1a00'000e, // pcalau12i $t2, %pc_hi20(.got.plt)
    0x0011'3dad, // sub.w     $t1, $t1, $t3
    0x2880'01cf, // ld.w      $t3, $t2, %lo12(.got.plt) # _dl_runtime_resolve
    0x02bf'51ad, // addi.w    $t1, $t1, -44             # .plt entry
    0x0280'01cc, // addi.w    $t0, $t2, %lo12(.got.plt) # &.got.plt
    0x0044'89ad, // srli.w    $t1, $t1, 2               # .plt entry offset
    0x2880'118c, // ld.w      $t0, $t0, 4               # link map
    0x4c00'01e0, // jr        $t3
  };

  u64 gotplt = ctx.gotplt->shdr.sh_addr;
  u64 plt = ctx.plt->shdr.sh_addr;

  memcpy(buf, E::is_64 ? insn_64 : insn_32, E::plt_hdr_size);
  write_j20(buf, hi20(gotplt, plt));
  write_k12(buf + 8, gotplt);
  write_k12(buf + 16, gotplt);
}

constexpr ul32 plt_entry_64[] = {
  0x1a00'000f, // pcalau12i $t3, %pc_hi20(func@.got.plt)
  0x28c0'01ef, // ld.d      $t3, $t3, %lo12(func@.got.plt)
  0x4c00'01ed, // jirl      $t1, $t3, 0
  0x002a'0000, // break
};

constexpr ul32 plt_entry_32[] = {
  0x1a00'000f, // pcalau12i $t3, %pc_hi20(func@.got.plt)
  0x2880'01ef, // ld.w      $t3, $t3, %lo12(func@.got.plt)
  0x4c00'01ed, // jirl      $t1, $t3, 0
  0x002a'0000, // break
};

template <>
void write_plt_entry<E>(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  u64 gotplt = sym.get_gotplt_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(buf, E::is_64 ? plt_entry_64 : plt_entry_32, E::plt_size);
  write_j20(buf, hi20(gotplt, plt));
  write_k12(buf + 4, gotplt);
}

template <>
void write_pltgot_entry<E>(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  u64 got = sym.get_got_pltgot_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(buf, E::is_64 ? plt_entry_64 : plt_entry_32, E::plt_size);
  write_j20(buf, hi20(got, plt));
  write_k12(buf + 4, got);
}

template <>
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                       u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_LARCH_ADD6:
    *loc = (*loc & 0b1100'0000) | ((*loc + val) & 0b0011'1111);
    break;
  case R_LARCH_ADD8:
    *loc += val;
    break;
  case R_LARCH_ADD16:
    *(ul16 *)loc += val;
    break;
  case R_LARCH_ADD32:
    *(ul32 *)loc += val;
    break;
  case R_LARCH_ADD64:
    *(ul64 *)loc += val;
    break;
  case R_LARCH_SUB6:
    *loc = (*loc & 0b1100'0000) | ((*loc - val) & 0b0011'1111);
    break;
  case R_LARCH_SUB8:
    *loc -= val;
    break;
  case R_LARCH_SUB16:
    *(ul16 *)loc -= val;
    break;
  case R_LARCH_SUB32:
    *(ul32 *)loc -= val;
    break;
  case R_LARCH_SUB64:
    *(ul64 *)loc -= val;
    break;
  case R_LARCH_32_PCREL:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  case R_LARCH_64_PCREL:
    *(ul64 *)loc = val - this->shdr.sh_addr - offset;
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

  auto get_r_delta = [&](i64 idx) {
    return extra.r_deltas.empty() ? 0 : extra.r_deltas[idx];
  };

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];

    if (rel.r_type == R_NONE || rel.r_type == R_LARCH_RELAX ||
        rel.r_type == R_LARCH_MARK_LA || rel.r_type == R_LARCH_MARK_PCREL ||
        rel.r_type == R_LARCH_ALIGN)
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

    auto check_branch = [&](i64 val, i64 lo, i64 hi) {
      if (val & 0b11)
        Error(ctx) << *this << ": misaligned symbol " << sym
                   << " for relocation " << rel;
      check(val, lo, hi);
    };

    // Unlike other psABIs, the LoongArch ABI uses the same relocation
    // types to refer to GOT entries for thread-local symbols and regular
    // ones. Therefore, G may refer to a TLSGD or a regular GOT slot
    // depending on the symbol type.
    //
    // Note that as of August 2023, both GCC and Clang treat TLSLD relocs
    // as if they were TLSGD relocs for LoongArch, which is a clear bug.
    // We need to handle TLSLD relocs as synonyms for TLSGD relocs for the
    // sake of bug compatibility.
    auto get_got_idx = [&] {
      if (sym.has_tlsgd(ctx))
        return sym.get_tlsgd_idx(ctx);
      return sym.get_got_idx(ctx);
    };

    u64 S = sym.get_addr(ctx);
    u64 A = rel.r_addend;
    u64 P = get_addr() + r_offset;
    u64 G = get_got_idx() * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;

    switch (rel.r_type) {
    case R_LARCH_32:
      if constexpr (E::is_64)
        *(ul32 *)loc = S + A;
      else
        apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, &dynrel);
      break;
    case R_LARCH_64:
      assert(E::is_64);
      apply_dyn_absrel(ctx, sym, rel, loc, S, A, P, &dynrel);
      break;
    case R_LARCH_B16:
      check_branch(S + A - P, -(1 << 17), 1 << 17);
      write_k16(loc, (S + A - P) >> 2);
      break;
    case R_LARCH_B21:
      check_branch(S + A - P, -(1 << 22), 1 << 22);
      write_d5k16(loc, (S + A - P) >> 2);
      break;
    case R_LARCH_B26:
      check_branch(S + A - P, -(1 << 27), 1 << 27);
      write_d10k16(loc, (S + A - P) >> 2);
      break;
    case R_LARCH_ABS_LO12:
      write_k12(loc, S + A);
      break;
    case R_LARCH_ABS_HI20:
      write_j20(loc, (S + A) >> 12);
      break;
    case R_LARCH_ABS64_LO20:
      write_j20(loc, (S + A) >> 32);
      break;
    case R_LARCH_ABS64_HI12:
      write_k12(loc, (S + A) >> 52);
      break;
    case R_LARCH_PCALA_LO12:
      // It looks like R_LARCH_PCALA_LO12 is sometimes used for JIRL even
      // though the instruction takes a 16 bit immediate rather than 12 bits.
      // It is contrary to the psABI document, but GNU ld has special
      // code to handle it, so we accept it too.
      if ((*(ul32 *)loc & 0xfc00'0000) == 0x4c00'0000)
        write_k16(loc, sign_extend(S + A, 11) >> 2);
      else
        write_k12(loc, S + A);
      break;
    case R_LARCH_PCALA_HI20:
      if (removed_bytes == 0) {
        write_j20(loc, hi20(S + A, P));
      } else {
        // Rewrite pcalau12i + addi.d with pcaddi
        assert(removed_bytes == 4);
        *(ul32 *)loc = 0x1800'0000 | get_rd(*(ul32 *)loc); // pcaddi
        write_j20(loc, (S + A - P) >> 2);
        i += 3;
      }
      break;
    case R_LARCH_PCALA64_LO20:
      write_j20(loc, higher20(S + A, P));
      break;
    case R_LARCH_PCALA64_HI12:
      write_k12(loc, highest12(S + A, P));
      break;
    case R_LARCH_GOT_PC_LO12:
      write_k12(loc, GOT + G + A);
      break;
    case R_LARCH_GOT_PC_HI20:
      if (removed_bytes == 0) {
        // If the PC-relative symbol address is known at link-time, we can
        // rewrite the following GOT load
        //
        //   pcalau12i $t0, 0         # R_LARCH_GOT_PC_HI20
        //   ld.d      $t0, $t0, 0    # R_LARCH_GOT_PC_LO12
        //
        // with the following address materialization
        //
        //   pcalau12i $t0, 0
        //   addi.d    $t0, $t0, 0
        if (is_relaxable_got_load(ctx, *this, i)) {
          i64 dist = compute_distance(ctx, sym, *this, rel);
          if (-(1LL << 31) <= dist && dist < (1LL << 31)) {
            u32 rd = get_rd(*(ul32 *)loc);
            *(ul32 *)(loc + 4) = 0x02c0'0000 | (rd << 5) | rd; // addi.d

            write_j20(loc, hi20(S + A, P));
            write_k12(loc + 4, S + A);
            i += 3;
            break;
          }
        }
        write_j20(loc, hi20(GOT + G + A, P));
      } else {
        // Rewrite pcalau12i + ld.d with pcaddi
        assert(removed_bytes == 4);
        *(ul32 *)loc = 0x1800'0000 | get_rd(*(ul32 *)loc); // pcaddi
        write_j20(loc, (S + A - P) >> 2);
        i += 3;
      }
      break;
    case R_LARCH_GOT64_PC_LO20:
      write_j20(loc, higher20(GOT + G + A, P));
      break;
    case R_LARCH_GOT64_PC_HI12:
      write_k12(loc, highest12(GOT + G + A, P));
      break;
    case R_LARCH_GOT_LO12:
      write_k12(loc, GOT + G + A);
      break;
    case R_LARCH_GOT_HI20:
      write_j20(loc, (GOT + G + A) >> 12);
      break;
    case R_LARCH_GOT64_LO20:
      write_j20(loc, (GOT + G + A) >> 32);
      break;
    case R_LARCH_GOT64_HI12:
      write_k12(loc, (GOT + G + A) >> 52);
      break;
    case R_LARCH_TLS_LE_LO12:
      write_k12(loc, S + A - ctx.tp_addr);
      break;
    case R_LARCH_TLS_LE_HI20:
      write_j20(loc, (S + A - ctx.tp_addr) >> 12);
      break;
    case R_LARCH_TLS_LE64_LO20:
      write_j20(loc, (S + A - ctx.tp_addr) >> 32);
      break;
    case R_LARCH_TLS_LE64_HI12:
      write_k12(loc, (S + A - ctx.tp_addr) >> 52);
      break;
    case R_LARCH_TLS_IE_PC_LO12:
      write_k12(loc, sym.get_gottp_addr(ctx) + A);
      break;
    case R_LARCH_TLS_IE_PC_HI20:
      write_j20(loc, hi20(sym.get_gottp_addr(ctx) + A, P));
      break;
    case R_LARCH_TLS_IE64_PC_LO20:
      write_j20(loc, higher20(sym.get_gottp_addr(ctx) + A, P));
      break;
    case R_LARCH_TLS_IE64_PC_HI12:
      write_k12(loc, highest12(sym.get_gottp_addr(ctx) + A, P));
      break;
    case R_LARCH_TLS_IE_LO12:
      write_k12(loc, sym.get_gottp_addr(ctx) + A);
      break;
    case R_LARCH_TLS_IE_HI20:
      write_j20(loc, (sym.get_gottp_addr(ctx) + A) >> 12);
      break;
    case R_LARCH_TLS_IE64_LO20:
      write_j20(loc, (sym.get_gottp_addr(ctx) + A) >> 32);
      break;
    case R_LARCH_TLS_IE64_HI12:
      write_k12(loc, (sym.get_gottp_addr(ctx) + A) >> 52);
      break;
    case R_LARCH_TLS_LD_PC_HI20:
    case R_LARCH_TLS_GD_PC_HI20:
      check(sym.get_tlsgd_addr(ctx) + A - P, -(1LL << 31), 1LL << 31);
      write_j20(loc, hi20(sym.get_tlsgd_addr(ctx) + A, P));
      break;
    case R_LARCH_TLS_LD_HI20:
    case R_LARCH_TLS_GD_HI20:
      write_j20(loc, (sym.get_tlsgd_addr(ctx) + A) >> 12);
      break;
    case R_LARCH_ADD6:
      *loc = (*loc & 0b1100'0000) | ((*loc + S + A) & 0b0011'1111);
      break;
    case R_LARCH_ADD8:
      *loc += S + A;
      break;
    case R_LARCH_ADD16:
      *(ul16 *)loc += S + A;
      break;
    case R_LARCH_ADD32:
      *(ul32 *)loc += S + A;
      break;
    case R_LARCH_ADD64:
      *(ul64 *)loc += S + A;
      break;
    case R_LARCH_SUB6:
      *loc = (*loc & 0b1100'0000) | ((*loc - S - A) & 0b0011'1111);
      break;
    case R_LARCH_SUB8:
      *loc -= S + A;
      break;
    case R_LARCH_SUB16:
      *(ul16 *)loc -= S + A;
      break;
    case R_LARCH_SUB32:
      *(ul32 *)loc -= S + A;
      break;
    case R_LARCH_SUB64:
      *(ul64 *)loc -= S + A;
      break;
    case R_LARCH_32_PCREL:
      *(ul32 *)loc = S + A - P;
      break;
    case R_LARCH_64_PCREL:
      *(ul64 *)loc = S + A - P;
      break;
    case R_LARCH_CALL36:
      if (removed_bytes == 0) {
        write_j20(loc, (S + A - P + 0x20000) >> 18);
        write_k16(loc + 4, (S + A - P) >> 2);
      } else {
        // Rewrite PCADDU18I + JIRL to B or BL
        assert(removed_bytes == 4);
        if (get_rd(*(ul32 *)(contents.data() + rel.r_offset + 4)) == 0)
          *(ul32 *)loc = 0x5000'0000; // B
        else
          *(ul32 *)loc = 0x5400'0000; // BL
        write_k16(loc, (S + A - P) >> 2);
      }
      break;
    case R_LARCH_ADD_ULEB128:
      overwrite_uleb(loc, read_uleb(loc) + S + A);
      break;
    case R_LARCH_SUB_ULEB128:
      overwrite_uleb(loc, read_uleb(loc) - S - A);
      break;
    case R_LARCH_TLS_DESC_PC_HI20:
      // LoongArch TLSDESC uses the following code sequence to materialize
      // a TP-relative address in a0.
      //
      //   pcalau12i $a0, 0
      //       R_LARCH_TLS_DESC_PC_HI20    foo
      //   addi.d    $a0, $a0, 0
      //       R_LARCH_TLS_DESC_PC_LO12    foo
      //   ld.d      $ra, $a0, 0
      //       R_LARCH_TLS_DESC_LD         foo
      //   jirl      $ra, $ra, 0
      //       R_LARCH_TLS_DESC_CALL       foo
      //
      // We may relax the instructions to the following if its TP-relative
      // address is known at link-time
      //
      //   <nop>
      //   <nop>
      //   lu12i.w   $a0, foo@TPOFF
      //   addi.w    $a0, $a0, foo@TPOFF
      //
      // or to the following if the TP-relative address is known at
      // process startup time.
      //
      //   <nop>
      //   <nop>
      //   pcalau12i $a0, foo@GOTTP
      //   ld.[dw]   $a0, $a0, foo@GOTTP
      //
      // Note that if section-shrinking relaxation is enabled, nop may be
      // completely deleted.
      if (removed_bytes == 0) {
        if (sym.has_tlsdesc(ctx))
          write_j20(loc, hi20(sym.get_tlsdesc_addr(ctx) + A, P));
        else
          *(ul32 *)loc = 0x0340'0000; // nop
      }
      break;
    case R_LARCH_TLS_DESC_PC_LO12:
      if (removed_bytes == 0) {
        if (sym.has_tlsdesc(ctx))
          write_k12(loc, sym.get_tlsdesc_addr(ctx) + A);
        else
          *(ul32 *)loc = 0x0340'0000; // nop
      }
      break;
    case R_LARCH_TLS_DESC_LD:
      if (sym.has_tlsdesc(ctx)) {
        // Do nothing
      } else if (sym.has_gottp(ctx)) {
        *(ul32 *)loc = 0x1a00'0004; // pcalau12i $a0, 0
        write_j20(loc, hi20(sym.get_gottp_addr(ctx) + A, P));
      } else {
        *(ul32 *)loc = 0x1400'0004; // lu12i.w   $a0, 0
        write_j20(loc, (S + A + 0x800 - ctx.tp_addr) >> 12);
      }
      break;
    case R_LARCH_TLS_DESC_CALL:
      if (sym.has_tlsdesc(ctx)) {
        // Do nothing
      } else if (sym.has_gottp(ctx)) {
        if (E::is_64)
          *(ul32 *)loc = 0x28c0'0084; // ld.d $a0, $a0, 0
        else
          *(ul32 *)loc = 0x2880'0084; // ld.w $a0, $a0, 0
        write_k12(loc, sym.get_gottp_addr(ctx) + A);
      } else {
        *(ul32 *)loc = 0x0280'0084;   // addi.w $a0, $a0, 0
        write_k12(loc, S + A - ctx.tp_addr);
      }
      break;
    case R_LARCH_TLS_LE_HI20_R:
      if (removed_bytes == 0)
        write_j20(loc, (S + A + 0x800 - ctx.tp_addr) >> 12);
      break;
    case R_LARCH_TLS_LE_LO12_R: {
      i64 val = S + A - ctx.tp_addr;
      write_k12(loc, val);

      // Rewrite `addi.d $t0, $t0, <offset>` with `addi.d $t0, $tp, <offset>`
      // if the offset is directly accessible using tp. tp is r2.
      if (sign_extend(val, 11) == val)
        set_rj(loc, 2);
      break;
    }
    case R_LARCH_TLS_LE_ADD_R:
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

    SectionFragment<E> *frag;
    i64 frag_addend;
    std::tie(frag, frag_addend) = get_fragment(ctx, rel);

    u64 S = frag ? frag->get_addr(ctx) : sym.get_addr(ctx);
    u64 A = frag ? frag_addend : (i64)rel.r_addend;

    switch (rel.r_type) {
    case R_LARCH_32:
      *(ul32 *)loc = S + A;
      break;
    case R_LARCH_64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A;
      break;
    case R_LARCH_ADD6:
      *loc = (*loc & 0b1100'0000) | ((*loc + S + A) & 0b0011'1111);
      break;
    case R_LARCH_ADD8:
      *loc += S + A;
      break;
    case R_LARCH_ADD16:
      *(ul16 *)loc += S + A;
      break;
    case R_LARCH_ADD32:
      *(ul32 *)loc += S + A;
      break;
    case R_LARCH_ADD64:
      *(ul64 *)loc += S + A;
      break;
    case R_LARCH_SUB6:
      *loc = (*loc & 0b1100'0000) | ((*loc - S - A) & 0b0011'1111);
      break;
    case R_LARCH_SUB8:
      *loc -= S + A;
      break;
    case R_LARCH_SUB16:
      *(ul16 *)loc -= S + A;
      break;
    case R_LARCH_SUB32:
      *(ul32 *)loc -= S + A;
      break;
    case R_LARCH_SUB64:
      *(ul64 *)loc -= S + A;
      break;
    case R_LARCH_TLS_DTPREL32:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul32 *)loc = *val;
      else
        *(ul32 *)loc = S + A - ctx.dtp_addr;
      break;
    case R_LARCH_TLS_DTPREL64:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A - ctx.dtp_addr;
      break;
    case R_LARCH_ADD_ULEB128:
      overwrite_uleb(loc, read_uleb(loc) + S + A);
      break;
    case R_LARCH_SUB_ULEB128:
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

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<E>);
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];

    if (rel.r_type == R_NONE || rel.r_type == R_LARCH_RELAX ||
        rel.r_type == R_LARCH_MARK_LA || rel.r_type == R_LARCH_MARK_PCREL ||
        rel.r_type == R_LARCH_ALIGN)
      continue;

    if (record_undef_error(ctx, rel))
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (sym.is_ifunc())
      sym.flags |= NEEDS_GOT | NEEDS_PLT;

    switch (rel.r_type) {
    case R_LARCH_32:
      if constexpr (E::is_64)
        scan_absrel(ctx, sym, rel);
      else
        scan_dyn_absrel(ctx, sym, rel);
      break;
    case R_LARCH_64:
      assert(E::is_64);
      scan_dyn_absrel(ctx, sym, rel);
      break;
    case R_LARCH_B26:
    case R_LARCH_PCALA_HI20:
    case R_LARCH_CALL36:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_LARCH_GOT_HI20:
    case R_LARCH_GOT_PC_HI20:
      sym.flags |= NEEDS_GOT;
      break;
    case R_LARCH_TLS_IE_HI20:
    case R_LARCH_TLS_IE_PC_HI20:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_LARCH_TLS_LD_PC_HI20:
    case R_LARCH_TLS_GD_PC_HI20:
    case R_LARCH_TLS_LD_HI20:
    case R_LARCH_TLS_GD_HI20:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_LARCH_32_PCREL:
    case R_LARCH_64_PCREL:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_LARCH_TLS_LE_HI20:
    case R_LARCH_TLS_LE_LO12:
    case R_LARCH_TLS_LE64_LO20:
    case R_LARCH_TLS_LE64_HI12:
    case R_LARCH_TLS_LE_HI20_R:
    case R_LARCH_TLS_LE_LO12_R:
      check_tlsle(ctx, sym, rel);
      break;
    case R_LARCH_TLS_DESC_CALL:
      scan_tlsdesc(ctx, sym);
      break;
    case R_LARCH_B16:
    case R_LARCH_B21:
    case R_LARCH_ABS_HI20:
    case R_LARCH_ABS_LO12:
    case R_LARCH_ABS64_LO20:
    case R_LARCH_ABS64_HI12:
    case R_LARCH_PCALA_LO12:
    case R_LARCH_PCALA64_LO20:
    case R_LARCH_PCALA64_HI12:
    case R_LARCH_GOT_PC_LO12:
    case R_LARCH_GOT64_PC_LO20:
    case R_LARCH_GOT64_PC_HI12:
    case R_LARCH_GOT_LO12:
    case R_LARCH_GOT64_LO20:
    case R_LARCH_GOT64_HI12:
    case R_LARCH_TLS_IE_PC_LO12:
    case R_LARCH_TLS_IE64_PC_LO20:
    case R_LARCH_TLS_IE64_PC_HI12:
    case R_LARCH_TLS_IE_LO12:
    case R_LARCH_TLS_IE64_LO20:
    case R_LARCH_TLS_IE64_HI12:
    case R_LARCH_ADD6:
    case R_LARCH_SUB6:
    case R_LARCH_ADD8:
    case R_LARCH_SUB8:
    case R_LARCH_ADD16:
    case R_LARCH_SUB16:
    case R_LARCH_ADD32:
    case R_LARCH_SUB32:
    case R_LARCH_ADD64:
    case R_LARCH_SUB64:
    case R_LARCH_ADD_ULEB128:
    case R_LARCH_SUB_ULEB128:
    case R_LARCH_TLS_DESC_PC_HI20:
    case R_LARCH_TLS_DESC_PC_LO12:
    case R_LARCH_TLS_DESC_LD:
    case R_LARCH_TLS_LE_ADD_R:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

template <>
void shrink_section(Context<E> &ctx, InputSection<E> &isec, bool use_rvc) {
  std::span<const ElfRel<E>> rels = isec.get_rels(ctx);
  isec.extra.r_deltas.resize(rels.size() + 1);
  i64 delta = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &r = rels[i];
    Symbol<E> &sym = *isec.file.symbols[r.r_sym];
    isec.extra.r_deltas[i] = delta;

    // A R_LARCH_ALIGN relocation refers to the beginning of a nop
    // sequence. We need to remove some or all of them so that the
    // instruction that immediately follows that is aligned to a specified
    // boundary. To allow that, a R_LARCH_ALIGN relocation that requests
    // 2^n alignment refers to 2^n - 4 bytes of nop instructions.
    if (r.r_type == R_LARCH_ALIGN) {
      // The actual rule for storing the alignment size is a bit weird.
      // In particular, the most significant 56 bits of r_addend is
      // sometimes used to store the upper limit of the alignment,
      // allowing the instruction that follows nops _not_ to be aligned at
      // all. I think that's a spec bug, so we don't want to support that.
      i64 alignment;
      if (r.r_sym) {
        if (r.r_addend >> 8)
          Fatal(ctx) << isec << ": ternary R_LARCH_ALIGN is not supported: " << i;
        alignment = 1 << r.r_addend;
      } else {
        if (!has_single_bit(r.r_addend + 4))
          Fatal(ctx) << isec << ": R_LARCH_ALIGN: invalid alignment requirement: "
                     << i;
        alignment = r.r_addend + 4;
      }

      u64 loc = isec.get_addr() + r.r_offset - delta;
      u64 next_loc = loc + alignment - 4;
      delta += next_loc - align_to(loc, alignment);
      continue;
    }

    // Handling other relocations is optional.
    if (!ctx.arg.relax || i == rels.size() - 1 ||
        rels[i + 1].r_type != R_LARCH_RELAX)
      continue;

    // Skip linker-synthesized symbols because their final addresses
    // are not fixed yet.
    if (sym.file == ctx.internal_obj)
      continue;

    switch (r.r_type) {
    case R_LARCH_TLS_LE_HI20_R:
    case R_LARCH_TLS_LE_ADD_R:
      // LoongArch uses the following three instructions to access
      // TP ± 2 GiB.
      //
      //  lu12i.w $t0, 0           # R_LARCH_TLS_LE_HI20_R
      //  add.d   $t0, $t0, $tp    # R_LARCH_TLS_LE_ADD_R
      //  addi.d  $t0, $t0, 0      # R_LARCH_TLS_LE_LO12_R
      //
      // If the thread-local variable is within TP ± 2 KiB, we can
      // relax them into the following single instruction.
      //
      //  addi.d  $t0, $tp, <tp-offset>
      if (i64 val = sym.get_addr(ctx) + r.r_addend - ctx.tp_addr;
          sign_extend(val, 11) == val)
        delta += 4;
      break;
    case R_LARCH_PCALA_HI20:
      // The following two instructions are used to materialize a
      // PC-relative address with a 32 bit displacement.
      //
      //   pcalau12i $t0, 0         # R_LARCH_PCALA_HI20
      //   addi.d    $t0, $t0, 0    # R_LARCH_PCALA_LO12
      //
      // If the displacement is within ±2 MiB, we can relax them to
      // the following instruction.
      //
      //   pcaddi    $t0, <offset>
      if (i + 3 < rels.size() &&
          rels[i + 2].r_type == R_LARCH_PCALA_LO12 &&
          rels[i + 2].r_offset == rels[i].r_offset + 4 &&
          rels[i + 3].r_type == R_LARCH_RELAX) {
        i64 dist = compute_distance(ctx, sym, isec, r);
        u32 insn1 = *(ul32 *)(isec.contents.data() + rels[i].r_offset);
        u32 insn2 = *(ul32 *)(isec.contents.data() + rels[i].r_offset + 4);
        bool is_addi_d = (insn2 & 0xffc0'0000) == 0x02c0'0000;

        if (dist % 4 == 0 && -(1 << 21) <= dist && dist < (1 << 21) &&
            is_addi_d && get_rd(insn1) == get_rd(insn2) &&
            get_rd(insn2) == get_rj(insn2))
          delta += 4;
      }
      break;
    case R_LARCH_CALL36:
      // A CALL36 relocation referes to the following instruction pair
      // to jump to PC ± 128 GiB.
      //
      //   pcaddu18i $t0,       0         # R_LARCH_CALL36
      //   jirl      $zero/$ra, $t0, 0
      //
      // If the displacement is PC ± 128 MiB, we can use B or BL instead.
      // Note that $zero is $r0 and $ra is $r1.
      if (i64 dist = compute_distance(ctx, sym, isec, r);
          -(1 << 27) <= dist && dist < (1 << 27))
        if (u32 jirl = *(ul32 *)(isec.contents.data() + rels[i].r_offset + 4);
            get_rd(jirl) == 0 || get_rd(jirl) == 1)
          delta += 4;
      break;
    case R_LARCH_GOT_PC_HI20:
      // The following two instructions are used to load a symbol address
      // from the GOT.
      //
      //   pcalau12i $t0, 0         # R_LARCH_GOT_PC_HI20
      //   ld.d      $t0, $t0, 0    # R_LARCH_GOT_PC_LO12
      //
      // If the PC-relative symbol address is known at link-time, we can
      // relax them to the following instruction.
      //
      //   pcaddi    $t0, <offset>
      if (is_relaxable_got_load(ctx, isec, i)) {
        i64 dist = compute_distance(ctx, sym, isec, r);
        if (dist % 4 == 0 && -(1 << 21) <= dist && dist < (1 << 21))
          delta += 4;
      }
      break;
    case R_LARCH_TLS_DESC_PC_HI20:
    case R_LARCH_TLS_DESC_PC_LO12:
      if (!sym.has_tlsdesc(ctx))
        delta += 4;
      break;
    }
  }

  isec.extra.r_deltas[rels.size()] = delta;
  isec.sh_size -= delta;
}

} // namespace mold::elf

#endif
