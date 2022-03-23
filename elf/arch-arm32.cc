#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

using E = ARM32;

static u32 bit(u32 val, i64 pos) {
  return (val >> pos) & 1;
}

// Returns [hi:lo] bits of val.
static u32 bits(u32 val, i64 hi, i64 lo) {
  return (val >> lo) & ((1LL << (hi - lo + 1)) - 1);
}

static void write_thm_bl_imm(u8 *loc, u32 val, bool is_blx) {
  // https://developer.arm.com/documentation/ddi0406/cb/Application-Level-Architecture/Instruction-Details/Alphabetical-list-of-instructions/BL--BLX--immediate-
  u32 sign = bit(val, 24);
  u32 I1 = bit(val, 23);
  u32 I2 = bit(val, 22);
  u32 J1 = !I1 ^ sign;
  u32 J2 = !I2 ^ sign;
  u32 imm10H = bits(val, 21, 12);
  u32 imm10L = bits(val, 11, 2);
  u32 imm11 = bits(val, 11, 1);

  *(u16 *)loc = (*(u16 *)loc & 0xf800) | (sign << 10) | imm10H;

  // R_ARM_THUM_CALL is used for BL or BLX instructions. BL and BLX
  // differ only at bit 12. We need to use BLX if we are switching
  // from THUMB to ARM.
  if (is_blx)
    *(u16 *)(loc + 2) = 0xc000 | (J1 << 13) | (J2 << 11) | (imm10L << 1);
  else
    *(u16 *)(loc + 2) = 0xd000 | (J1 << 13) | (1 << 12) | (J2 << 11) | imm11;
}

static void write_thm_mov_imm(u8 *loc, u32 val) {
  // https://developer.arm.com/documentation/ddi0406/cb/Application-Level-Architecture/Instruction-Details/Alphabetical-list-of-instructions/MOVT
  u32 imm4 = bits(val, 15, 12);
  u32 i = bit(val, 11);
  u32 imm3 = bits(val, 10, 8);
  u32 imm8 = bits(val, 7, 0);
  *(u16 *)loc = (*(u16 *)loc & 0b1111'1011'1111'0000) | (i << 10) | imm4;
  *(u16 *)(loc + 2) =
    ((*(u16 *)(loc + 2)) & 0b1000'1111'0000'0000) | (imm3 << 12) | imm8;
}

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  static const u32 plt0[] = {
    0xe52de004, // 1: push {lr}
    0xe59fe004, // ldr     lr, [pc, #4]
    0xe08fe00e, // add     lr, pc, lr
    0xe5bef008, // ldr     pc, [lr, #8]!
    0x00000000, // .word   .got.plt - 1b - 16
  };

  memcpy(buf, plt0, sizeof(plt0));
  *(u32 *)(buf + 16) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr - 16;

  for (Symbol<E> *sym : symbols) {
    static const u32 plt[] = {
      0xe59fc004, // 1: ldr ip, 2f
      0xe08cc00f, // add ip, ip, pc
      0xe59cf000, // ldr pc, [ip]
      0x00000000, // 2: .word sym@PLTGOT - 1b
    };

    u8 *ent = buf + sizeof(plt0) + sym->get_plt_idx(ctx) * sizeof(plt);
    memcpy(ent, plt, sizeof(plt));
    *(u32 *)(ent + 12) = sym->get_gotplt_addr(ctx) - sym->get_plt_addr(ctx) - 12;
  }
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  for (Symbol<E> *sym : symbols) {
    static const u32 plt[] = {
      0xe59fc004, // 1: ldr ip, 2f
      0xe08cc00f, // add ip, ip, pc
      0xe59cf000, // ldr pc, [ip]
      0x00000000, // 2: .word sym@GOT - 1b
    };

    u8 *ent = buf + sym->get_pltgot_idx(ctx) * sizeof(plt);
    memcpy(ent, plt, sizeof(plt));
    *(u32 *)(ent + 12) = sym->get_got_addr(ctx) - sym->get_plt_addr(ctx) - 12;
  }
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, ElfRel<E> &rel,
                                    u64 offset, u64 val) {}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  ElfRel<E> *dynrel = nullptr;
  std::span<ElfRel<E>> rels = get_rels(ctx);

  i64 frag_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                           file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_ARM_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<E> *frag_ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      frag_ref = &rel_fragments[frag_idx++];

#define S   (frag_ref ? frag_ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag_ref ? frag_ref->addend : this->get_addend(rel))
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define T   (sym.get_addr(ctx) & 1)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_ARM_ABS32:
      if (sym.is_absolute() || !ctx.arg.pic) {
        *(u32 *)loc = S + A;
      } else if (sym.is_imported) {
        *dynrel++ = {P, R_ARM_ABS32, (u32)sym.get_dynsym_idx(ctx)};
        *(u32 *)loc = A;
      } else {
        if (!is_relr_reloc(ctx, rel))
          *dynrel++ = {P, R_ARM_RELATIVE, 0};
        *(u32 *)loc = S + A;
      }
      continue;
    case R_ARM_REL32:
      *(u32 *)loc = S + A - P;
      continue;
    case R_ARM_THM_CALL:
      if (sym.esym().is_undef_weak()) {
        // On ARM, calling an weak undefined symbol jumps to the
        // next instruction.
        write_thm_bl_imm(loc, 4, false);
      } else if (T) {
        write_thm_bl_imm(loc, S + A - P, false);
      } else {
        write_thm_bl_imm(loc, align_to(S + A - P, 4), true);
      }
      continue;
    case R_ARM_BASE_PREL:
      *(u32 *)loc = GOT + A - P;
      continue;
    case R_ARM_GOT_BREL:
      *(u32 *)loc = G + A;
      continue;
    case R_ARM_CALL:
    case R_ARM_JUMP24: {
      u32 val;

      if (sym.esym().is_undef_weak()) {
        // On ARM, calling an weak undefined symbol jumps to the
        // next instruction.
        val = 4;
      } else {
        val = ((S + A) | T) - P;
      }

      *(u32 *)loc = (*(u32 *)loc & 0xff00'0000) | ((val >> 2) & 0x00ff'ffff);
      continue;
    }
    case R_ARM_PREL31: {
      u32 val = S + A - P;
      *(u32 *)loc = (*(u32 *)loc & 0x8000'0000) | (val & 0x7fff'ffff);
      continue;
    }
    case R_ARM_THM_MOVW_ABS_NC:
      write_thm_mov_imm(loc, (S + A) | T);
      continue;
    case R_ARM_THM_MOVT_ABS:
      write_thm_mov_imm(loc, (S + A) >> 16);
      continue;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }

#undef S
#undef A
#undef P
#undef T
#undef G
#undef GOT
  }
}

template <>
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {
  std::span<ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_ARM_NONE)
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
#define A (frag ? addend : this->get_addend(rel))

    switch (rel.r_type) {
    case R_ARM_ABS32:
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
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<E>);
  std::span<ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_ARM_NONE)
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
    case R_ARM_ABS32: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    COPYREL,       PLT    },     // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_ARM_REL32:
    case R_ARM_BASE_PREL:
      break;
    case R_ARM_THM_CALL: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  NONE,     NONE,  PLT,           PLT    },     // DSO
        {  NONE,     NONE,  PLT,           PLT    },     // PIE
        {  NONE,     NONE,  PLT,           PLT    },     // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_ARM_GOT_BREL:
      sym.flags |= NEEDS_GOT;
      break;
    case R_ARM_CALL:
    case R_ARM_JUMP24:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_ARM_PREL31:
    case R_ARM_THM_MOVW_ABS_NC:
      break;
    case R_ARM_THM_MOVT_ABS: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  ERROR,    NONE,    ERROR,         ERROR },      // DSO
        {  ERROR,    NONE,    COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,    COPYREL,       PLT   },      // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

} // namespace mold::elf
