#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

using E = RISCV64;

static u32 bit(u32 val, i64 pos) {
  return (val >> pos) & 1;
}

// Returns [hi:lo] bits of val.
static u32 bits(u32 val, i64 hi, i64 lo) {
  return (val >> lo) & ((1LL << (hi - lo + 1)) - 1);
}

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
  return bits(val + 0x800, 31, 12) << 12;
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

static void write_itype(u32 *loc, u32 val) {
  u32 mask = 0b000000'00000'11111'111'11111'1111111;
  *loc = (*loc & mask) | itype(val);
}

static void write_stype(u32 *loc, u32 val) {
  u32 mask = 0b000000'11111'11111'111'00000'1111111;
  *loc = (*loc & mask) | stype(val);
}

static void write_btype(u32 *loc, u32 val) {
  u32 mask = 0b000000'11111'11111'111'00000'1111111;
  *loc = (*loc & mask) | btype(val);
}

static void write_utype(u32 *loc, u32 val) {
  u32 mask = 0b000000'00000'00000'000'11111'1111111;
  *loc = (*loc & mask) | utype(val);
}

static void write_jtype(u32 *loc, u32 val) {
  u32 mask = 0b000000'00000'00000'000'11111'1111111;
  *loc = (*loc & mask) | jtype(val);
}

static void write_cbtype(u16 *loc, u32 val) {
  u32 mask = 0b1110001110000011;
  *loc = (*loc & mask) | cbtype(val);
}

static void write_cjtype(u16 *loc, u32 val) {
  u32 mask = 0b1110000000000011;
  *loc = (*loc & mask) | cjtype(val);
}

static void write_plt_header(Context<E> &ctx) {
  u32 *buf = (u32 *)(ctx.buf + ctx.plt->shdr.sh_offset);

  static const u32 plt0[] = {
    0x00000397, // auipc  t2, %pcrel_hi(.got.plt)
    0x41c30333, // sub    t1, t1, t3               # .plt entry + hdr + 12
    0x0003be03, // ld     t3, %pcrel_lo(1b)(t2)    # _dl_runtime_resolve
    0xfd430313, // addi   t1, t1, -44              # .plt entry
    0x00038293, // addi   t0, t2, %pcrel_lo(1b)    # &.got.plt
    0x00135313, // srli   t1, t1, 1                # .plt entry offset
    0x0082b283, // ld     t0, 8(t0)                # link map
    0x000e0067, // jr     t3
  };

  u64 gotplt = ctx.gotplt->shdr.sh_addr;
  u64 plt = ctx.plt->shdr.sh_addr;

  memcpy(buf, plt0, sizeof(plt0));
  write_utype(buf, gotplt - plt);
  write_itype(buf + 2, gotplt - plt);
  write_itype(buf + 4, gotplt - plt);
}

static void write_plt_entry(Context<E> &ctx, Symbol<E> &sym) {
  u32 *ent = (u32 *)(ctx.buf + ctx.plt->shdr.sh_offset + ctx.plt_hdr_size +
                     sym.get_plt_idx(ctx) * ctx.plt_size);

  static const u32 data[] = {
    0x00000e17, // auipc   t3, %pcrel_hi(function@.got.plt)
    0x000e3e03, // ld      t3, %pcrel_lo(1b)(t3)
    0x000e0367, // jalr    t1, t3
    0x00000013, // nop
  };

  u64 gotplt = sym.get_gotplt_addr(ctx);
  u64 plt = sym.get_plt_addr(ctx);

  memcpy(ent, data, sizeof(data));
  write_utype(ent, gotplt - plt);
  write_itype(ent + 1, gotplt - plt);
}

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  write_plt_header(ctx);
  for (Symbol<E> *sym : symbols)
    write_plt_entry(ctx, *sym);
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u32 *buf = (u32 *)(ctx.buf + this->shdr.sh_offset);

  static const u32 data[] = {
    0x00000e17, // auipc   t3, %pcrel_hi(function@.got.plt)
    0x000e3e03, // ld      t3, %pcrel_lo(1b)(t3)
    0x000e0367, // jalr    t1, t3
    0x00000013, // nop
  };

  for (Symbol<E> *sym : symbols) {
    u32 *ent = buf + sym->get_pltgot_idx(ctx) * 4;
    u64 got = sym->get_got_addr(ctx);
    u64 plt = sym->get_plt_addr(ctx);

    memcpy(ent, data, sizeof(data));
    write_utype(ent, got - plt);
    write_itype(ent + 1, got - plt);
  }
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_RISCV_ADD32:
    *(u32 *)loc += val;
    return;
  case R_RISCV_SUB8:
    *loc -= val;
    return;
  case R_RISCV_SUB16:
    *(u16 *)loc -= val;
    return;
  case R_RISCV_SUB32:
    *(u32 *)loc -= val;
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
    *(u16 *)loc = val;
    return;
  case R_RISCV_32_PCREL:
    *(u32 *)loc = val - this->shdr.sh_addr - offset;
    return;
  }
  Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  ElfRel<E> *dynrel = nullptr;
  std::span<ElfRel<E>> rels = get_rels(ctx);
  std::span<i32> r_deltas = get_r_deltas();

  i64 frag_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                           file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_RISCV_NONE || rel.r_type == R_RISCV_RELAX)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    i64 r_offset = rel.r_offset + r_deltas[i];
    u8 *loc = base + r_offset;

    const SectionFragmentRef<E> *frag_ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      frag_ref = &rel_fragments[frag_idx++];

#define S   (frag_ref ? frag_ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag_ref ? frag_ref->addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_RISCV_32:
      *(u32 *)loc = S + A;
      break;
    case R_RISCV_64:
      if (sym.is_absolute() || !ctx.arg.pic) {
        *(u64 *)loc = S + A;
      } else if (sym.is_imported) {
        *dynrel++ = {P, R_RISCV_64, (u32)sym.get_dynsym_idx(ctx), A};
        *(u64 *)loc = A;
      } else {
        if (!is_relr_reloc(ctx, rel))
          *dynrel++ = {P, R_RISCV_RELATIVE, 0, (i64)(S + A)};
        *(u64 *)loc = S + A;
      }
      break;
    case R_RISCV_TLS_DTPMOD32:
    case R_RISCV_TLS_DTPMOD64:
    case R_RISCV_TLS_DTPREL32:
    case R_RISCV_TLS_DTPREL64:
    case R_RISCV_TLS_TPREL32:
    case R_RISCV_TLS_TPREL64:
      Error(ctx) << *this << ": unsupported relocation: " << rel;
      break;
    case R_RISCV_BRANCH:
      write_btype((u32 *)loc, S + A - P);
      break;
    case R_RISCV_JAL:
      write_jtype((u32 *)loc, S + A - P);
      break;
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT: {
      if (r_deltas[i + 1] - r_deltas[i] != 0) {
        // auipc + jalr -> jal
        assert(r_deltas[i + 1] - r_deltas[i] == -4);
        u32 jalr = *(u32 *)&contents[rels[i].r_offset + 4];
        *(u32 *)loc = (0b11111'000000 & jalr) | 0b101111;
        write_jtype((u32 *)loc, S + A - P);
      } else {
        u64 val = sym.esym().is_undef_weak() ? 0 : S + A - P;
        write_utype((u32 *)loc, val);
        write_itype((u32 *)(loc + 4), val);
      }
      break;
    }
    case R_RISCV_GOT_HI20:
      *(u32 *)loc = G + GOT + A - P;
      break;
    case R_RISCV_TLS_GOT_HI20:
      *(u32 *)loc = sym.get_gottp_addr(ctx) + A - P;
      break;
    case R_RISCV_TLS_GD_HI20:
      *(u32 *)loc = sym.get_tlsgd_addr(ctx) + A - P;
      break;
    case R_RISCV_PCREL_HI20:
      if (sym.esym().is_undef_weak()) {
        // Calling an undefined weak symbol does not make sense.
        // We make such call into an infinite loop. This should
        // help debugging of a faulty program.
        *(u32 *)loc = P;
      } else {
        *(u32 *)loc = S + A - P;
      }
      break;
    case R_RISCV_PCREL_LO12_I:
      assert(sym.get_input_section() == this);
      assert(sym.value < r_offset);
      write_itype((u32 *)loc, *(u32 *)(base + sym.value));
      break;
    case R_RISCV_LO12_I:
    case R_RISCV_TPREL_LO12_I:
      write_itype((u32 *)loc, S + A);
      break;
    case R_RISCV_PCREL_LO12_S:
      assert(sym.get_input_section() == this);
      assert(sym.value < r_offset);
      write_stype((u32 *)loc, *(u32 *)(base + sym.value));
      break;
    case R_RISCV_LO12_S:
    case R_RISCV_TPREL_LO12_S:
      write_stype((u32 *)loc, S + A);
      break;
    case R_RISCV_HI20:
      write_utype((u32 *)loc, S + A);
      break;
    case R_RISCV_TPREL_HI20:
      write_utype((u32 *)loc, S + A - ctx.tls_begin);
      break;
    case R_RISCV_TPREL_ADD:
      break;
    case R_RISCV_ADD8:
      loc += S + A;
      break;
    case R_RISCV_ADD16:
      *(u16 *)loc += S + A;
      break;
    case R_RISCV_ADD32:
      *(u32 *)loc += S + A;
      break;
    case R_RISCV_ADD64:
      *(u64 *)loc += S + A;
      break;
    case R_RISCV_SUB8:
      loc -= S + A;
      break;
    case R_RISCV_SUB16:
      *(u16 *)loc -= S + A;
      break;
    case R_RISCV_SUB32:
      *(u32 *)loc -= S + A;
      break;
    case R_RISCV_SUB64:
      *(u64 *)loc -= S + A;
      break;
    case R_RISCV_ALIGN:
      break;
    case R_RISCV_RVC_BRANCH:
      write_cbtype((u16 *)loc, S + A - P);
      break;
    case R_RISCV_RVC_JUMP:
      write_cjtype((u16 *)loc, S + A - P);
      break;
    case R_RISCV_RVC_LUI:
      Error(ctx) << *this << ": unsupported relocation: " << rel;
      break;
    case R_RISCV_RELAX:
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
      *(u16 *)loc = S + A;
      break;
    case R_RISCV_SET32:
      *(u32 *)loc = S + A;
      break;
    case R_RISCV_32_PCREL:
      *(u32 *)loc = S + A - P;
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }

#undef S
#undef A
#undef P
#undef G
#undef GOT
  }

  // In the above loop, PC-relative HI20 relocations overwrote
  // instructions with full 32-bit values to allow their corresponding
  // PCREL_LO12 relocations to read their values. This loop restore
  // the original instructions.
  for (i64 i = 0; i < rels.size(); i++) {
    switch (rels[i].r_type) {
    case R_RISCV_GOT_HI20:
    case R_RISCV_PCREL_HI20:
    case R_RISCV_TLS_GOT_HI20:
    case R_RISCV_TLS_GD_HI20: {
      u32 *loc = (u32 *)(base + rels[i].r_offset + r_deltas[i]);
      u32 val = *loc;
      *loc = *(u32 *)&contents[rels[i].r_offset];
      write_utype(loc, val);
    }
    }
  }
}

template <>
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {
  std::span<ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_RISCV_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      report_undef(ctx, file, sym);
      continue;
    }

    SectionFragment<E> *frag;
    i64 addend;
    std::tie(frag, addend) = get_fragment(ctx, rel);

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? addend : rel.r_addend)

    switch (rel.r_type) {
    case R_RISCV_32:
      *(u32 *)loc = S + A;
      break;
    case R_RISCV_64:
      if (std::optional<u64> val = get_tombstone(sym))
        *(u64 *)loc = *val;
      else
        *(u64 *)loc = S + A;
      break;
    case R_RISCV_ADD8:
      *loc += S + A;
      break;
    case R_RISCV_ADD16:
      *(u16 *)loc += S + A;
      break;
    case R_RISCV_ADD32:
      *(u32 *)loc += S + A;
      break;
    case R_RISCV_ADD64:
      *(u64 *)loc += S + A;
      break;
    case R_RISCV_SUB8:
      *loc -= S + A;
      break;
    case R_RISCV_SUB16:
      *(u16 *)loc -= S + A;
      break;
    case R_RISCV_SUB32:
      *(u32 *)loc -= S + A;
      break;
    case R_RISCV_SUB64:
      *(u64 *)loc -= S + A;
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
      *(u16 *)loc = S + A;
      break;
    case R_RISCV_SET32:
      *(u32 *)loc = S + A;
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

template <>
void InputSection<E>::copy_contents_riscv(Context<E> &ctx, u8 *buf) {
  // A non-alloc section isn't relaxed, so just copy it as one big chunk.
  if (!(shdr().sh_flags & SHF_ALLOC)) {
    if (is_compressed())
      uncompress(ctx, buf);
    else
      memcpy(buf, contents.data(), contents.size());
    return;
  }

  // Memory-allocated sections may be relaxed, so copy each segment
  // individually.
  std::span<ElfRel<E>> rels = get_rels(ctx);
  std::span<i32> r_deltas = get_r_deltas();
  i64 pos = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    i64 delta = r_deltas[i + 1] - r_deltas[i];
    if (delta == 0)
      continue;

    const ElfRel<E> &r = rels[i];
    memcpy(buf, contents.data() + pos, r.r_offset - pos);
    buf += r.r_offset - pos;
    pos = r.r_offset;

    if (delta < 0) {
      pos -= delta;
    } else {
      memset(buf, 0, delta);
      buf += delta;
    }
  }

  memcpy(buf, contents.data() + pos, contents.size() - pos);
}

template <>
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<E>);
  std::span<ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_RISCV_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (!sym.file) {
      report_undef(ctx, file, sym);
      continue;
    }

    if (sym.get_type() == STT_GNU_IFUNC) {
      sym.flags |= NEEDS_GOT;
      sym.flags |= NEEDS_PLT;
    }

    switch (rel.r_type) {
    case R_RISCV_32:
    case R_RISCV_HI20: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     ERROR,   ERROR,         ERROR },      // DSO
        {  NONE,     ERROR,   ERROR,         ERROR },      // PIE
        {  NONE,     NONE,    COPYREL,       CPLT  },      // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_RISCV_64: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    COPYREL,       CPLT   },     // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_RISCV_TLS_DTPMOD32:
    case R_RISCV_TLS_DTPMOD64:
    case R_RISCV_TLS_DTPREL32:
    case R_RISCV_TLS_DTPREL64:
    case R_RISCV_TLS_TPREL32:
    case R_RISCV_TLS_TPREL64:
      Error(ctx) << *this << ": unsupported relocation: " << rel;
      break;
    case R_RISCV_BRANCH:
    case R_RISCV_JAL:
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
      break;
    case R_RISCV_RVC_BRANCH:
    case R_RISCV_RVC_JUMP:
      break;
    case R_RISCV_RVC_LUI:
      Error(ctx) << *this << ": unsupported relocation: " << rel;
      break;
    case R_RISCV_RELAX:
    case R_RISCV_SUB6:
    case R_RISCV_SET6:
    case R_RISCV_SET8:
    case R_RISCV_SET16:
    case R_RISCV_SET32:
      break;
    case R_RISCV_32_PCREL: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  ERROR,    NONE,  ERROR,         ERROR },      // DSO
        {  ERROR,    NONE,  COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

static bool is_resizable(Context<E> &ctx, InputSection<E> *isec) {
  return isec && (isec->shdr().sh_flags & SHF_ALLOC);
}

// Initializes r_deltas and sorted_symbols.
static void initialize_storage(Context<E> &ctx) {
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->r_deltas.resize(file->sections.size());
    for (std::unique_ptr<InputSection<E>> &isec : file->sections)
      if (is_resizable(ctx, isec.get()))
        isec->get_r_deltas().resize(isec->get_rels(ctx).size() + 1);

    file->sorted_symbols.resize(file->sections.size());

    for (Symbol<E> *sym : file->symbols)
      if (sym->file == file)
        if (InputSection<E> *isec = sym->get_input_section())
          file->sorted_symbols[isec->shndx].push_back(sym);

    for (std::vector<Symbol<E> *> &vec : file->sorted_symbols)
      sort(vec, [](Symbol<E> *a, Symbol<E> *b) { return a->value < b->value; });
  });
}

// Interpret R_RISCV_ALIGN relocations and align them if necessary.
// This function may enlarge input sections but never shrinks.
static void align_contents(Context<E> &ctx, InputSection<E> &isec) {
  std::span<i32> r_deltas = isec.get_r_deltas();
  std::span<Symbol<E> *> syms = isec.get_sorted_symbols();
  i64 delta = 0;

  std::span<ElfRel<E>> rels = isec.get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &r = rels[i];
    r_deltas[i] = delta;

    if (r.r_type != R_RISCV_ALIGN)
      continue;

    i64 delta2 = align_to(r.r_offset, r.r_addend) - r.r_offset;
    if (delta2 == 0)
      continue;

    while (!syms.empty() && syms[0]->value <= r.r_offset) {
      syms[0]->value += delta;
      syms = syms.subspan(1);
    }

    delta += delta2;
  }

  for (Symbol<E> *sym : syms)
    sym->value += delta;
  r_deltas[rels.size()] = delta;

  isec.sh_size += delta;
}

// Returns the distance between a relocated place and a symbol.
static i64 compute_distance(Context<E> &ctx, Symbol<E> &sym,
                            InputSection<E> &isec, const ElfRel<E> &rel) {
  // We handle absolute symbols as if they were infinitely far away
  // because `relax_call` may increase a distance between a branch
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

// Relax R_RISCV_CALL and R_RISCV_CALL_PLT relocations.
static void relax_call(Context<E> &ctx, InputSection<E> &isec) {
  std::span<i32> r_deltas = isec.get_r_deltas();
  std::span<Symbol<E> *> syms = isec.get_sorted_symbols();
  i64 delta = 0;

  std::span<ElfRel<E>> rels = isec.get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &r = rels[i];
    i64 delta2 = 0;

    r_deltas[i] += delta;

    switch (r.r_type) {
    case R_RISCV_ALIGN:
      delta2 = align_to(r.r_offset, r.r_addend) - r.r_offset;
      break;
    case R_RISCV_CALL:
    case R_RISCV_CALL_PLT: {
      if (i == rels.size() - 1 || rels[i + 1].r_type != R_RISCV_RELAX)
        break;

      // If the jump target is within ±1 MiB, we can replace AUIPC+JALR
      // with JAL, saving 4 bytes.
      Symbol<E> &sym = *isec.file.symbols[r.r_sym];
      i64 dist = compute_distance(ctx, sym, isec, r);
      if (dist % 2 == 0 && -(1 << 20) <= dist && dist < (1 << 20))
        delta2 = -4;
    }
    }

    if (delta2 == 0)
      continue;

    while (!syms.empty() && syms[0]->value <= r.r_offset) {
      syms[0]->value += delta;
      syms = syms.subspan(1);
    }

    delta += delta2;
  }

  for (Symbol<E> *sym : syms)
    sym->value += delta;
  r_deltas[rels.size()] += delta;

  isec.sh_size += delta;
}

// RISC-V instructions are 16 or 32 bits long, so immediates encoded
// in instructions can't be 32 bits long. Therefore, branch and load
// instructions can't refer the 4 GiB address space unlike x86-64.
// In fact, JAL (jump and link) instruction can jump to only within
// ±1 MiB as their immediate is only 21 bits long.
//
// If you want to jump to somewhere further than that, you need to
// construct a full 32-bit offset using multiple instruction and
// branch to that place (e.g. AUIPC and JALR instead of JAL).
// In this comment, we refer instructions such as JAL as the short
// encoding and ones such as AUIPC+JALR as the long encoding.
//
// By default, compiler always uses the long encoding so that branch
// targets are always encodable. This is a safe bet for them but
// may result in inefficient code. Therefore, the RISC-V psABI defines
// a mechanism for the linker to replace long encoding instructions
// with short ones, shrinking the section and increasing the code
// density.
//
// This is contrary to the psABIs for the other RISC processors such as
// ARM64. Typically, they use short instructions by default, and a
// linker creates so-called "thunks" to extend ranges of short jumps.
// On RISC-V, instructions are in the long encoding by default, and
// the linker shrinks them if it can.
//
// When we shrink a section, we need to adjust relocation offsets and
// symbol values. For example, if we replace AUIPC+JALR with JAL
// (which saves 4 bytes), all relocations pointing to anywhere after
// that location need to be shifted by 4. In addition to that, any
// symbol that refers anywhere after that locatioin need to be shifted
// by 4 bytes as well.
//
// For relocations, we use `r_deltas` array to memorize how many bytes
// have be adjusted. For symbols, we directly mutate their `value`
// member.
//
// This operation seems to be optional, as by default instructions are
// using the long encoding, but calling this function is actually
// mandatory because of R_RISCV_ALIGN. R_RISCV_ALIGN relocation is a
// directive to the linker to align the location referred to by the
// relocation to a specified byte boundary. We at least have to
// interpret them satisfy the constraints imposed by R_RISCV_ALIGN,
// and that means we may change section sizes anyway.
i64 riscv_resize_sections(Context<E> &ctx) {
  Timer t(ctx, "riscv_resize_sections");

  initialize_storage(ctx);

  // First, interpret R_RISCV_ALIGN relocations. This may enlarge
  // sections.
  {
    Timer t(ctx, "align_contents");
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      for (std::unique_ptr<InputSection<E>> &isec : file->sections)
        if (is_resizable(ctx, isec.get()))
          align_contents(ctx, *isec);
    });
  }

  // Re-compute section offset.
  compute_section_sizes(ctx);
  set_osec_offsets(ctx);

  // Find R_RISCV_CALL AND R_RISCV_CALL_PLT that can be relaxed.
  // This step should only shrink sections.
  {
    Timer t(ctx, "relax_call");
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      for (std::unique_ptr<InputSection<E>> &isec : file->sections)
        if (is_resizable(ctx, isec.get()))
          relax_call(ctx, *isec);
    });
  }

  // Re-compute section offset again to finalize them.
  compute_section_sizes(ctx);
  return set_osec_offsets(ctx);
}

} // namespace mold::elf
