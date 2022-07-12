// ARM32 is a bit special from the linker's viewpoint because ARM
// processors support two different instruction encodings: Thumb and
// ARM (in a narrower sense). Thumb instructions are either 16 bits or
// 32 bits, while ARM instructions are all 32 bits. Feature-wise,
// thumb is a subset of ARM, so not all ARM instructions are
// representable in Thumb.
//
// ARM processors originally supported only ARM instructions. Thumb
// instructions were later added to increase code density.
//
// ARM processors runs in either ARM mode or Thumb mode. The mode can
// be switched using BX (branch and mode exchange)-family instructions.
// We need to use such instructions to, for example, call a function
// encoded in Thumb from a function encoded in ARM. Sometimes, the
// linker even has to emit an interworking thunk code to switch from
// Thumb to ARM.
//
// ARM instructions are aligned to 4 byte boundaries. Thumb are to 2
// byte boundaries.
//
// You can distinguish Thumb functions from ARM functions by looking
// at the least significant bit (LSB) of its "address". If LSB is 0,
// it's ARM; otherwise, Thumb. LSB is not a part of its real address.
// For example, if a symbol `foo` is of type STT_FUNC and has value
// 0x2001, then `foo` is a function using Thumb instructions whose
// address is 0x2000 (not 0x2001).

#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

namespace mold::elf {

using E = ARM32;

static void write_mov_imm(u8 *loc, u32 val) {
  u32 imm12 = bits(val, 11, 0);
  u32 imm4 = bits(val, 15, 12);
  *(ul32 *)loc = (*(ul32 *)loc & 0xfff0f000) | (imm4 << 16) | imm12;
}

static void write_thm_b_imm(u8 *loc, u32 val) {
  // https://developer.arm.com/documentation/ddi0406/cb/Application-Level-Architecture/Instruction-Details/Alphabetical-list-of-instructions/BL--BLX--immediate-
  u32 sign = bit(val, 24);
  u32 I1 = bit(val, 23);
  u32 I2 = bit(val, 22);
  u32 J1 = !I1 ^ sign;
  u32 J2 = !I2 ^ sign;
  u32 imm10 = bits(val, 21, 12);
  u32 imm11 = bits(val, 11, 1);

  *(ul16 *)loc = (*(ul16 *)loc & 0xf800) | (sign << 10) | imm10;
  *(ul16 *)(loc + 2) =
    (*(ul16 *)(loc + 2) & 0xd000) | (J1 << 13) | (J2 << 11) | imm11;
}

static void write_thm_mov_imm(u8 *loc, u32 val) {
  // https://developer.arm.com/documentation/ddi0406/cb/Application-Level-Architecture/Instruction-Details/Alphabetical-list-of-instructions/MOVT
  u32 imm4 = bits(val, 15, 12);
  u32 i = bit(val, 11);
  u32 imm3 = bits(val, 10, 8);
  u32 imm8 = bits(val, 7, 0);
  *(ul16 *)loc = (*(ul16 *)loc & 0b1111'1011'1111'0000) | (i << 10) | imm4;
  *(ul16 *)(loc + 2) =
    ((*(ul16 *)(loc + 2)) & 0b1000'1111'0000'0000) | (imm3 << 12) | imm8;
}

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  static const u32 plt0[] = {
    0xe52de004, // push {lr}
    0xe59fe004, // ldr lr, 2f
    0xe08fe00e, // 1: add lr, pc, lr
    0xe5bef008, // ldr pc, [lr, #8]!
    0x00000000, // 2: .word .got.plt - 1b - 8
    0xe320f000, // nop
    0xe320f000, // nop
    0xe320f000, // nop
  };

  memcpy(buf, plt0, sizeof(plt0));
  *(ul32 *)(buf + 16) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr - 16;

  for (Symbol<E> *sym : symbols) {
    static const u32 plt[] = {
      0xe59fc004, // 1: ldr ip, 2f
      0xe08cc00f, // add ip, ip, pc
      0xe59cf000, // ldr pc, [ip]
      0x00000000, // 2: .word sym@PLTGOT - 1b
    };

    u8 *ent = buf + sizeof(plt0) + sym->get_plt_idx(ctx) * sizeof(plt);
    memcpy(ent, plt, sizeof(plt));
    *(ul32 *)(ent + 12) = sym->get_gotplt_addr(ctx) - sym->get_plt_addr(ctx) - 12;
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
    *(ul32 *)(ent + 12) = sym->get_got_addr(ctx) - sym->get_plt_addr(ctx) - 12;
  }
}

// ARM does not use .eh_frame for exception handling. Instead, it uses
// .ARM.exidx and .ARM.extab. So this function is empty.
template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  ElfRel<E> *dynrel = nullptr;
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  i64 frag_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                           file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_ARM_NONE || rel.r_type == R_ARM_V4BX)
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
    case R_ARM_TARGET1:
      if (sym.is_absolute() || !ctx.arg.pic) {
        *(ul32 *)loc = S + A;
      } else if (sym.is_imported) {
        *dynrel++ = {P, R_ARM_ABS32, (u32)sym.get_dynsym_idx(ctx)};
      } else {
        if (!is_relr_reloc(ctx, rel))
          *dynrel++ = {P, R_ARM_RELATIVE, 0};
        *(ul32 *)loc = S + A;
      }
      continue;
    case R_ARM_REL32:
      *(ul32 *)loc = S + A - P;
      continue;
    case R_ARM_THM_CALL:
      // THM_CALL relocation refers either BL or BLX instruction.
      // They are different in only one bit. We need to use BL if
      // the jump target is Thumb. Otherwise, use BLX.
      if (sym.esym().is_undef_weak()) {
        // On ARM, calling an weak undefined symbol jumps to the
        // next instruction.
        write_thm_b_imm(loc, 4);
        *(ul16 *)(loc + 2) |= (1 << 12); // rewrite with BL
      } else if (T) {
        write_thm_b_imm(loc, S + A - P);
        *(ul16 *)(loc + 2) |= (1 << 12); // rewrite with BL
      } else {
        write_thm_b_imm(loc, align_to(S + A - P, 4));
        *(ul16 *)(loc + 2) &= ~(1 << 12); // rewrite with BLX
      }
      continue;
    case R_ARM_BASE_PREL:
      *(ul32 *)loc = GOT + A - P;
      continue;
    case R_ARM_GOT_PREL:
      *(ul32 *)loc = G + A - P;
      continue;
    case R_ARM_GOT_BREL:
      *(ul32 *)loc = G + A;
      continue;
    case R_ARM_TARGET2:
      *(ul32 *)loc = GOT + G + A - P;
      continue;
    case R_ARM_CALL:
    case R_ARM_JUMP24: {
      u32 val;

      if (sym.esym().is_undef_weak()) {
        // On ARM, calling an weak undefined symbol jumps to the
        // next instruction.
        val = 4;
      } else {
        val = S + A - P;
      }

      *(ul32 *)loc = (*(ul32 *)loc & 0xff00'0000) | ((val >> 2) & 0x00ff'ffff);
      continue;
    }
    case R_ARM_THM_JUMP11: {
      assert(T);
      u32 val = (S + A - P) >> 1;
      *(ul16 *)loc = (*(ul16 *)loc & 0xf800) | (val & 0x07ff);
      continue;
    }
    case R_ARM_THM_JUMP24:
      if (T) {
        write_thm_b_imm(loc, S + A - P);
      } else {
        assert(sym.extra.thumb_to_arm_thunk_idx != -1);
        u64 thunk_addr =
          ctx.thumb_to_arm->shdr.sh_addr +
          sym.extra.thumb_to_arm_thunk_idx * ThumbToArmSection::ENTRY_SIZE;
        write_thm_b_imm(loc, thunk_addr - P - 4);
      }
      continue;
    case R_ARM_MOVW_PREL_NC:
      write_mov_imm(loc, ((S + A) | T) - P);
      continue;
    case R_ARM_MOVW_ABS_NC:
      write_mov_imm(loc, (S + A) | T);
      continue;
    case R_ARM_THM_MOVW_PREL_NC:
      write_thm_mov_imm(loc, ((S + A) | T) - P);
      continue;
    case R_ARM_PREL31: {
      u32 val = S + A - P;
      *(ul32 *)loc = (*(ul32 *)loc & 0x8000'0000) | (val & 0x7fff'ffff);
      continue;
    }
    case R_ARM_THM_MOVW_ABS_NC:
      write_thm_mov_imm(loc, (S + A) | T);
      continue;
    case R_ARM_MOVT_PREL:
      write_mov_imm(loc, (S + A - P) >> 16);
      continue;
    case R_ARM_THM_MOVT_PREL:
      write_thm_mov_imm(loc, (S + A - P) >> 16);
      continue;
    case R_ARM_MOVT_ABS:
      write_mov_imm(loc, (S + A) >> 16);
      continue;
    case R_ARM_THM_MOVT_ABS:
      write_thm_mov_imm(loc, (S + A) >> 16);
      continue;
    case R_ARM_TLS_GD32:
      *(ul32 *)loc = sym.get_tlsgd_addr(ctx) + A - P;
      continue;
    case R_ARM_TLS_LDM32:
      *(ul32 *)loc = ctx.got->get_tlsld_addr(ctx) + A - P;
      continue;
    case R_ARM_TLS_LDO32:
      *(ul32 *)loc = S + A - ctx.tls_begin;
      continue;
    case R_ARM_TLS_IE32:
      *(ul32 *)loc = sym.get_gottp_addr(ctx) + A - P;
      continue;
    case R_ARM_TLS_LE32:
      *(ul32 *)loc = S + A - ctx.tls_begin + 8;
      continue;
    case R_ARM_TLS_GOTDESC:
      if (sym.get_tlsdesc_idx(ctx) == -1)
        *(ul32 *)loc = S - ctx.tls_begin + 8;
      else
        *(ul32 *)loc = sym.get_tlsdesc_addr(ctx) + A - P - 6;
      continue;
    case R_ARM_THM_TLS_CALL:
      if (sym.get_tlsdesc_idx(ctx) == -1) {
        // BL -> NOP
        *(ul32 *)loc = 0x8000f3af;
      } else {
        u64 addr = ctx.tls_trampoline->shdr.sh_addr;
        write_thm_b_imm(loc, align_to(addr - P - 4, 4));
        *(ul16 *)(loc + 2) &= ~(1 << 12); // rewrite BL with BLX
      }
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
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_ARM_NONE)
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
    case R_ARM_ABS32:
      if (!frag) {
        if (std::optional<u64> val = get_tombstone(sym)) {
          *(ul32 *)loc = *val;
          break;
        }
      }
      *(ul32 *)loc = S + A;
      break;
    case R_ARM_TLS_LDO32:
      if (std::optional<u64> val = get_tombstone(sym))
        *(ul32 *)loc = *val;
      else
        *(ul32 *)loc = S + A - ctx.tls_begin;
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
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_ARM_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];

    if (!sym.file) {
      record_undef_error(ctx, rel);
      continue;
    }

    if (sym.get_type() == STT_GNU_IFUNC) {
      sym.flags |= NEEDS_GOT;
      sym.flags |= NEEDS_PLT;
    }

    switch (rel.r_type) {
    case R_ARM_ABS32:
    case R_ARM_MOVT_ABS:
    case R_ARM_THM_MOVT_ABS:
    case R_ARM_TARGET1: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    COPYREL,       CPLT   },     // PDE
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
    case R_ARM_GOT_PREL:
    case R_ARM_GOT_BREL:
    case R_ARM_TARGET2:
      sym.flags |= NEEDS_GOT;
      break;
    case R_ARM_CALL:
    case R_ARM_JUMP24:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_ARM_THM_JUMP24:
      if (sym.is_imported || sym.get_type() == STT_GNU_IFUNC)
        sym.flags |= NEEDS_PLT | NEEDS_THUMB_TO_ARM_THUNK;
      else if (sym.esym().st_value % 2 == 0)
        sym.flags |= NEEDS_THUMB_TO_ARM_THUNK;
      break;
    case R_ARM_MOVT_PREL:
    case R_ARM_THM_MOVT_PREL:
    case R_ARM_PREL31: {
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  ERROR,    NONE,    ERROR,         PLT   },      // DSO
        {  ERROR,    NONE,    COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,    COPYREL,       PLT   },      // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_ARM_TLS_GD32:
      sym.flags |= NEEDS_TLSGD;
      break;
    case R_ARM_TLS_LDM32:
      ctx.needs_tlsld = true;
      break;
    case R_ARM_TLS_IE32:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_ARM_TLS_GOTDESC:
      if (!ctx.relax_tlsdesc || sym.is_imported)
        sym.flags |= NEEDS_TLSDESC;
      break;
    case R_ARM_THM_JUMP11:
    case R_ARM_MOVW_PREL_NC:
    case R_ARM_MOVW_ABS_NC:
    case R_ARM_THM_MOVW_PREL_NC:
    case R_ARM_THM_MOVW_ABS_NC:
    case R_ARM_TLS_LDO32:
    case R_ARM_TLS_LE32:
    case R_ARM_THM_TLS_CALL:
    case R_ARM_V4BX:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

// If a function referenced by a Thumb B (branch) instruction is
// resovled to a non-thumb function, we can't directly jump from the
// thumb function to the ARM function. We can't rewrite B with BX
// because there's no such BX instruction that takes an immediate.
// BX takes only a register.
//
// In order to support such branch, we insert a small piece of code to
// the resulting executable which switches the processor mode from
// Thumb to ARM. This section contains such code.
void ThumbToArmSection::add_symbol(Context<E> &ctx, Symbol<E> *sym) {
  if (sym->extra.thumb_to_arm_thunk_idx == -1) {
    sym->extra.thumb_to_arm_thunk_idx = symbols.size();
    symbols.push_back(sym);
  }
}

void ThumbToArmSection::update_shdr(Context<E> &ctx) {
  this->shdr.sh_size = symbols.size() * ENTRY_SIZE;
}

void ThumbToArmSection::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  i64 offset = 0;

  static u8 insn[] = {
    0x40, 0xf2, 0x00, 0x0c, // movw ip, $0
    0xc0, 0xf2, 0x00, 0x0c, // movt ip, $0
    0xfc, 0x44,             // add  ip, pc
    0x60, 0x47,             // bx   ip
  };

  static_assert(sizeof(insn) == ENTRY_SIZE);

  for (Symbol<E> *sym : symbols) {
    memcpy(buf + offset, insn, sizeof(insn));

    u32 val = sym->get_addr(ctx) - this->shdr.sh_addr - offset - 12;
    write_thm_mov_imm(buf + offset, val);
    write_thm_mov_imm(buf + offset + 4, val >> 16);
    offset += sizeof(insn);
  }
}

void TlsTrampolineSection::copy_buf(Context<E> &ctx) {
  // Trampoline code for TLSDESC
  static u32 insn[] = {
    0xe08e0000, // add r0, lr, r0
    0xe5901004, // ldr r1, [r0, #4]
    0xe12fff11, // bx  r1
  };
  memcpy(ctx.buf + this->shdr.sh_offset, insn, sizeof(insn));
}

template <typename E>
static OutputSection<E> *find_exidx_section(Context<E> &ctx) {
  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
    if (osec->shdr.sh_type == SHT_ARM_EXIDX)
      return osec.get();
  return nullptr;
}

// ARM executables use an .ARM.exidx section to look up an exception
// handling record for the current instruction pointer. The table needs
// to be sorted by their addresses.
//
// Other target uses .eh_frame_hdr instead for the same purpose.
// I don't know why only ARM uses the different mechanism, but it's
// likely that it's due to some historical reason.
//
// This function sorts .ARM.exidx records.
void sort_arm_exidx(Context<E> &ctx) {
  Timer t(ctx, "sort_arm_exidx");

  OutputSection<E> *osec = find_exidx_section(ctx);
  if (!osec)
    return;

  // .ARM.exidx records consists of a signed 31-bit relative address
  // and a 32-bit value. The relative address indicates the start
  // address of a function that the record covers. The value is one of
  // the followings:
  //
  // 1. CANTUNWIND indicating that there's no unwinding info for the function,
  // 2. a compact unwinding record encoded into a 32-bit value, or
  // 3. a 31-bit relative address which points to a larger record in
  //    the .ARM.extab section.
  //
  // CANTUNWIND is value 1. The most significant bit is set in (2) but
  // not in (3). So we can distinguished them just by looking at a value.
  const u32 EXIDX_CANTUNWIND = 1;

  struct Entry {
    ul32 addr;
    ul32 val;
  };

  if (osec->shdr.sh_size % sizeof(Entry))
    Fatal(ctx) << "invalid .ARM.exidx section size";

  Entry *ent = (Entry *)(ctx.buf + osec->shdr.sh_offset);
  i64 num_entries = osec->shdr.sh_size / sizeof(Entry);

  // Entry's addresses are relative to the beginning of their entries.
  // We first translate them so that they are relative to the
  // beginning of the section. We then sort the records and then
  // translate them back to be relative to each record.

  auto is_relative = [](u32 val) {
    return val != EXIDX_CANTUNWIND && !(val & 0x8000'0000);
  };

  tbb::parallel_for((i64)0, num_entries, [&](i64 i) {
    i64 offset = sizeof(Entry) * i;
    ent[i].addr = sign_extend(ent[i].addr, 30) - offset;
    if (is_relative(ent[i].val))
      ent[i].val = 0x7fff'ffff & (sign_extend(ent[i].val, 30) - offset);
  });

  tbb::parallel_sort(ent, ent + num_entries, [](const Entry &a, const Entry &b) {
    return a.addr < b.addr;
  });

  // Write back the sorted records while adjusting relative addresses
  tbb::parallel_for((i64)0, num_entries, [&](i64 i) {
    i64 offset = sizeof(Entry) * i;
    ent[i].addr = 0x7fff'ffff & (ent[i].addr + offset);
    if (is_relative(ent[i].val))
      ent[i].val = 0x7fff'ffff & (ent[i].val + offset);
  });
}

} // namespace mold::elf
