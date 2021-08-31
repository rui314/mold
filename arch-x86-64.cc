#include "mold.h"

template <>
void GotPltSection<X86_64>::copy_buf(Context<X86_64> &ctx) {
  u64 *buf = (u64 *)(ctx.buf + this->shdr.sh_offset);

  // The first slot of .got.plt points to _DYNAMIC, as requested by
  // the x86-64 psABI. The second and the third slots are reserved by
  // the psABI.
  buf[0] = ctx.dynamic ? ctx.dynamic->shdr.sh_addr : 0;
  buf[1] = 0;
  buf[2] = 0;

  for (Symbol<X86_64> *sym : ctx.plt->symbols)
    buf[sym->get_gotplt_idx(ctx)] = sym->get_plt_addr(ctx) + 6;
}

template <>
void PltSection<X86_64>::copy_buf(Context<X86_64> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  // Write PLT header
  static const u8 plt0[] = {
    0xff, 0x35, 0, 0, 0, 0, // pushq GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
    0x0f, 0x1f, 0x40, 0x00, // nop
  };

  memcpy(buf, plt0, sizeof(plt0));
  *(u32 *)(buf + 2) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr + 2;
  *(u32 *)(buf + 8) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr + 4;

  // Write PLT entries
  i64 relplt_idx = 0;

  static const u8 data[] = {
    0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOTPLT
    0x68, 0,    0, 0, 0,    // push  $index_in_relplt
    0xe9, 0,    0, 0, 0,    // jmp   PLT[0]
  };

  for (Symbol<X86_64> *sym : symbols) {
    u8 *ent = buf + sym->get_plt_idx(ctx) * X86_64::plt_size;
    memcpy(ent, data, sizeof(data));
    *(u32 *)(ent + 2) = sym->get_gotplt_addr(ctx) - sym->get_plt_addr(ctx) - 6;
    *(u32 *)(ent + 7) = relplt_idx++;
    *(u32 *)(ent + 12) = this->shdr.sh_addr - sym->get_plt_addr(ctx) - 16;
  }
}

template <>
void PltGotSection<X86_64>::copy_buf(Context<X86_64> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  static const u8 data[] = {
    0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOT
    0x66, 0x90,             // nop
  };

  for (Symbol<X86_64> *sym : symbols) {
    u8 *ent = buf + sym->get_pltgot_idx(ctx) * X86_64::pltgot_size;
    memcpy(ent, data, sizeof(data));
    *(u32 *)(ent + 2) = sym->get_got_addr(ctx) - sym->get_plt_addr(ctx) - 6;
  }
}

template <>
void EhFrameSection<X86_64>::apply_reloc(Context<X86_64> &ctx,
                                         ElfRel<X86_64> &rel,
                                         u64 loc, u64 val) {
  u8 *base = ctx.buf + this->shdr.sh_offset;

  switch (rel.r_type) {
  case R_X86_64_NONE:
    return;
  case R_X86_64_32:
    *(u32 *)(base + loc) = val;
    return;
  case R_X86_64_64:
    *(u64 *)(base + loc) = val;
    return;
  case R_X86_64_PC32:
    *(u32 *)(base + loc) = val - this->shdr.sh_addr - loc;
    return;
  case R_X86_64_PC64:
    *(u64 *)(base + loc) = val - this->shdr.sh_addr - loc;
    return;
  }
  unreachable(ctx);
}

static u32 relax_gotpcrelx(u8 *loc) {
  switch ((loc[0] << 8) | loc[1]) {
  case 0xff15: return 0x90e8; // call *0(%rip) -> call 0
  case 0xff25: return 0x90e9; // jmp  *0(%rip) -> jmp  0
  }
  return 0;
}

static u32 relax_rex_gotpcrelx(u8 *loc) {
  switch ((loc[0] << 16) | (loc[1] << 8) | loc[2]) {
  case 0x488b05: return 0x488d05; // mov 0(%rip), %rax -> lea 0(%rip), %rax
  case 0x488b0d: return 0x488d0d; // mov 0(%rip), %rcx -> lea 0(%rip), %rcx
  case 0x488b15: return 0x488d15; // mov 0(%rip), %rdx -> lea 0(%rip), %rdx
  case 0x488b1d: return 0x488d1d; // mov 0(%rip), %rbx -> lea 0(%rip), %rbx
  case 0x488b25: return 0x488d25; // mov 0(%rip), %rsp -> lea 0(%rip), %rsp
  case 0x488b2d: return 0x488d2d; // mov 0(%rip), %rbp -> lea 0(%rip), %rbp
  case 0x488b35: return 0x488d35; // mov 0(%rip), %rsi -> lea 0(%rip), %rsi
  case 0x488b3d: return 0x488d3d; // mov 0(%rip), %rdi -> lea 0(%rip), %rdi
  case 0x4c8b05: return 0x4c8d05; // mov 0(%rip), %r8  -> lea 0(%rip), %r8
  case 0x4c8b0d: return 0x4c8d0d; // mov 0(%rip), %r9  -> lea 0(%rip), %r9
  case 0x4c8b15: return 0x4c8d15; // mov 0(%rip), %r10 -> lea 0(%rip), %r10
  case 0x4c8b1d: return 0x4c8d1d; // mov 0(%rip), %r11 -> lea 0(%rip), %r11
  case 0x4c8b25: return 0x4c8d25; // mov 0(%rip), %r12 -> lea 0(%rip), %r12
  case 0x4c8b2d: return 0x4c8d2d; // mov 0(%rip), %r13 -> lea 0(%rip), %r13
  case 0x4c8b35: return 0x4c8d35; // mov 0(%rip), %r14 -> lea 0(%rip), %r14
  case 0x4c8b3d: return 0x4c8d3d; // mov 0(%rip), %r15 -> lea 0(%rip), %r15
  }
  return 0;
}

static u32 relax_gottpoff(u8 *loc) {
  switch ((loc[0] << 16) | (loc[1] << 8) | loc[2]) {
  case 0x488b05: return 0x48c7c0; // mov 0(%rip), %rax -> mov $0, %rax
  case 0x488b0d: return 0x48c7c1; // mov 0(%rip), %rcx -> mov $0, %rcx
  case 0x488b15: return 0x48c7c2; // mov 0(%rip), %rdx -> mov $0, %rdx
  case 0x488b1d: return 0x48c7c3; // mov 0(%rip), %rbx -> mov $0, %rbx
  case 0x488b25: return 0x48c7c4; // mov 0(%rip), %rsp -> mov $0, %rsp
  case 0x488b2d: return 0x48c7c5; // mov 0(%rip), %rbp -> mov $0, %rbp
  case 0x488b35: return 0x48c7c6; // mov 0(%rip), %rsi -> mov $0, %rsi
  case 0x488b3d: return 0x48c7c7; // mov 0(%rip), %rdi -> mov $0, %rdi
  case 0x4c8b05: return 0x49c7c0; // mov 0(%rip), %r8  -> mov $0, %r8
  case 0x4c8b0d: return 0x49c7c1; // mov 0(%rip), %r9  -> mov $0, %r9
  case 0x4c8b15: return 0x49c7c2; // mov 0(%rip), %r10 -> mov $0, %r10
  case 0x4c8b1d: return 0x49c7c3; // mov 0(%rip), %r11 -> mov $0, %r11
  case 0x4c8b25: return 0x49c7c4; // mov 0(%rip), %r12 -> mov $0, %r12
  case 0x4c8b2d: return 0x49c7c5; // mov 0(%rip), %r13 -> mov $0, %r13
  case 0x4c8b35: return 0x49c7c6; // mov 0(%rip), %r14 -> mov $0, %r14
  case 0x4c8b3d: return 0x49c7c7; // mov 0(%rip), %r15 -> mov $0, %r15
  }
  return 0;
}

// Apply relocations to SHF_ALLOC sections (i.e. sections that are
// mapped to memory at runtime) based on the result of
// scan_relocations().
template <>
void InputSection<X86_64>::apply_reloc_alloc(Context<X86_64> &ctx, u8 *base) {
  ElfRel<X86_64> *dynrel = nullptr;
  std::span<ElfRel<X86_64>> rels = get_rels(ctx);
  i64 frag_idx = 0;

  if (ctx.reldyn)
    dynrel = (ElfRel<X86_64> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                                file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<X86_64> &rel = rels[i];
    if (rel.r_type == R_X86_64_NONE)
      continue;

    Symbol<X86_64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<X86_64> *ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      ref = &rel_fragments[frag_idx++];

    auto overflow_check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

    auto write8 = [&](u64 val) {
      overflow_check(val, 0, 1 << 8);
      *loc = val;
    };

    auto write8s = [&](u64 val) {
      overflow_check(val, -(1 << 7), 1 << 7);
      *loc = val;
    };

    auto write16 = [&](u64 val) {
      overflow_check(val, 0, 1 << 16);
      *(u16 *)loc = val;
    };

    auto write16s = [&](u64 val) {
      overflow_check(val, -(1 << 15), 1 << 15);
      *(u16 *)loc = val;
    };

    auto write32 = [&](u64 val) {
      overflow_check(val, 0, (i64)1 << 32);
      *(u32 *)loc = val;
    };

    auto write32s = [&](u64 val) {
      overflow_check(val, -((i64)1 << 31), (i64)1 << 31);
      *(u32 *)loc = val;
    };

#define S   (ref ? ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (ref ? ref->addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel_exprs[i]) {
    case R_BASEREL:
      *dynrel++ = {P, R_X86_64_RELATIVE, 0, (i64)(S + A)};
      *(u64 *)loc = S + A;
      continue;
    case R_DYN:
      *dynrel++ = {P, R_X86_64_64, (u32)sym.get_dynsym_idx(ctx), A};
      *(u64 *)loc = A;
      continue;
    }

    switch (rel.r_type) {
    case R_X86_64_8:
      write8(S + A);
      continue;
    case R_X86_64_16:
      write16(S + A);
      continue;
    case R_X86_64_32:
      write32(S + A);
      continue;
    case R_X86_64_32S:
      write32s(S + A);
      continue;
    case R_X86_64_64:
      *(u64 *)loc = S + A;
      continue;
    case R_X86_64_PC8:
      write8s(S + A - P);
      continue;
    case R_X86_64_PC16:
      write16s(S + A - P);
      continue;
    case R_X86_64_PC32:
      write32s(S + A - P);
      continue;
    case R_X86_64_PC64:
      *(u64 *)loc = S + A - P;
      continue;
    case R_X86_64_PLT32:
      write32s(S + A - P);
      continue;
    case R_X86_64_GOT32:
      write32s(G + A);
      continue;
    case R_X86_64_GOT64:
      *(u64 *)loc = G + A;
      continue;
    case R_X86_64_GOTPC32:
      write32s(GOT + A - P);
      continue;
    case R_X86_64_GOTPC64:
      *(u64 *)loc = GOT + A - P;
      continue;
    case R_X86_64_GOTPCREL:
      write32s(G + GOT + A - P);
      continue;
    case R_X86_64_GOTPCREL64:
      *(u64 *)loc = G + GOT + A - P;
      continue;
    case R_X86_64_GOTPCRELX:
      if (sym.get_got_idx(ctx) == -1) {
        u32 insn = relax_gotpcrelx(loc - 2);
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        write32s(S + A - P);
      } else {
        write32s(G + GOT + A - P);
      }
      continue;
    case R_X86_64_REX_GOTPCRELX:
      if (sym.get_got_idx(ctx) == -1) {
        u32 insn = relax_rex_gotpcrelx(loc - 3);
        loc[-3] = insn >> 16;
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        write32s(S + A - P);
      } else {
        write32s(G + GOT + A - P);
      }
      continue;
    case R_X86_64_TLSGD:
      if (sym.get_tlsgd_idx(ctx) == -1) {
        // Relax GD to LE
        static const u8 insn[] = {
          0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
          0x48, 0x8d, 0x80, 0,    0,    0, 0,       // lea 0(%rax), %rax
        };
        memcpy(loc - 4, insn, sizeof(insn));

        i64 val = S - ctx.tls_end + A + 4;
        overflow_check(val, -((i64)1 << 31), (i64)1 << 31);
        *(u32 *)(loc + 8) = val;
        i++;
      } else {
        write32s(sym.get_tlsgd_addr(ctx) + A - P);
      }
      continue;
    case R_X86_64_TLSLD:
      if (ctx.got->tlsld_idx == -1) {
        // Relax LD to LE
        static const u8 insn[] = {
          0x66, 0x66, 0x66,                         // (padding)
          0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
        };
        memcpy(loc - 3, insn, sizeof(insn));
        i++;
      } else {
        write32s(ctx.got->get_tlsld_addr(ctx) + A - P);
      }
      continue;
    case R_X86_64_DTPOFF32:
      if (ctx.arg.relax && !ctx.arg.shared)
        write32s(S + A - ctx.tls_end);
      else
        write32s(S + A - ctx.tls_begin);
      continue;
    case R_X86_64_DTPOFF64:
      if (ctx.arg.relax && !ctx.arg.shared)
        *(u64 *)loc = S + A - ctx.tls_end;
      else
        *(u64 *)loc = S + A - ctx.tls_begin;
      continue;
    case R_X86_64_TPOFF32:
      write32s(S + A - ctx.tls_end);
      continue;
    case R_X86_64_TPOFF64:
      *(u64 *)loc = S + A - ctx.tls_end;
      continue;
    case R_X86_64_GOTTPOFF:
      if (sym.get_gottp_idx(ctx) == -1) {
        u32 insn = relax_gottpoff(loc - 3);
        loc[-3] = insn >> 16;
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        write32s(S + A - ctx.tls_end + 4);
      } else {
        write32s(sym.get_gottp_addr(ctx) + A - P);
      }
      continue;
    case R_X86_64_GOTPC32_TLSDESC:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        static const u8 insn[] = {
          0x48, 0xc7, 0xc0, 0, 0, 0, 0, // mov $0, %rax
        };
        memcpy(loc - 3, insn, sizeof(insn));
        write32s(S + A - ctx.tls_end + 4);
      } else {
        write32s(sym.get_tlsdesc_addr(ctx) + A - P);
      }
      continue;
    case R_X86_64_SIZE32:
      write32(sym.esym().st_size + A);
      continue;
    case R_X86_64_SIZE64:
      *(u64 *)loc = sym.esym().st_size + A;
      continue;
    case R_X86_64_TLSDESC_CALL:
      if (ctx.relax_tlsdesc && !sym.is_imported) {
        // call *(%rax) -> nop
        loc[0] = 0x66;
        loc[1] = 0x90;
      }
      continue;
    default:
      unreachable(ctx);
    }

#undef S
#undef A
#undef P
#undef G
#undef GOT
  }
}

// This function is responsible for applying relocations against
// non-SHF_ALLOC sections (i.e. sections that are not mapped to memory
// at runtime).
//
// Relocations against non-SHF_ALLOC sections are much easier to
// handle than that against SHF_ALLOC sections. It is because, since
// they are not mapped to memory, they don't contain any variable or
// function and never need PLT or GOT. Non-SHF_ALLOC sections are
// mostly debug info sections.
//
// Relocations against non-SHF_ALLOC sections are not scanned by
// scan_relocations.
template <>
void InputSection<X86_64>::apply_reloc_nonalloc(Context<X86_64> &ctx, u8 *base) {
  std::span<ElfRel<X86_64>> rels = get_rels(ctx);
  i64 frag_idx = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<X86_64> &rel = rels[i];
    if (rel.r_type == R_X86_64_NONE)
      continue;

    Symbol<X86_64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      report_undef(ctx, sym);
      continue;
    }

    const SectionFragmentRef<X86_64> *ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      ref = &rel_fragments[frag_idx++];

    auto overflow_check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

    auto write8 = [&](u64 val) {
      overflow_check(val, 0, 1 << 8);
      *loc = val;
    };

    auto write8s = [&](u64 val) {
      overflow_check(val, -(1 << 7), 1 << 7);
      *loc = val;
    };

    auto write16 = [&](u64 val) {
      overflow_check(val, 0, 1 << 16);
      *(u16 *)loc = val;
    };

    auto write16s = [&](u64 val) {
      overflow_check(val, -(1 << 15), 1 << 15);
      *(u16 *)loc = val;
    };

    auto write32 = [&](u64 val) {
      overflow_check(val, 0, (i64)1 << 32);
      *(u32 *)loc = val;
    };

    auto write32s = [&](u64 val) {
      overflow_check(val, -((i64)1 << 31), (i64)1 << 31);
      *(u32 *)loc = val;
    };

#define S   (ref ? ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (ref ? ref->addend : rel.r_addend)

    switch (rel.r_type) {
    case R_X86_64_8:
      write8(S + A);
      break;
    case R_X86_64_16:
      write16(S + A);
      break;
    case R_X86_64_32:
      write32(S + A);
      break;
    case R_X86_64_32S:
      write32s(S + A);
      break;
    case R_X86_64_64:
      *(u64 *)loc = S + A;
      break;
    case R_X86_64_DTPOFF32:
      write32s(S + A - ctx.tls_begin);
      break;
    case R_X86_64_DTPOFF64:
      *(u64 *)loc = S + A - ctx.tls_begin;
      break;
    case R_X86_64_SIZE32:
      write32(sym.esym().st_size + A);
      break;
    case R_X86_64_SIZE64:
      *(u64 *)loc = sym.esym().st_size + A;
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

// Linker has to create data structures in an output file to apply
// some type of relocations. For example, if a relocation refers a GOT
// or a PLT entry of a symbol, linker has to create an entry in .got
// or in .plt for that symbol. In order to fix the file layout, we
// need to scan relocations.
template <>
void InputSection<X86_64>::scan_relocations(Context<X86_64> &ctx) {
  ASSERT(shdr.sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<X86_64>);
  std::span<ElfRel<X86_64>> rels = get_rels(ctx);
  bool is_writable = (shdr.sh_flags & SHF_WRITE);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<X86_64> &rel = rels[i];
    if (rel.r_type == R_X86_64_NONE)
      continue;

    Symbol<X86_64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (!sym.file) {
      report_undef(ctx, sym);
      continue;
    }

    if (sym.get_type() == STT_GNU_IFUNC) {
      sym.flags |= NEEDS_GOT;
      sym.flags |= NEEDS_PLT;
    }

    switch (rel.r_type) {
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S: {
      // Dynamic linker does not support 8, 16 or 32-bit dynamic
      // relocations for these types of relocations. We report an
      // error if we cannot relocate them even at load-time.
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  NONE,     ERROR, ERROR,         ERROR },      // DSO
        {  NONE,     ERROR, ERROR,         ERROR },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };
      dispatch(ctx, table, i);
      break;
    }
    case R_X86_64_64: {
      // Unlike the above, we can use R_X86_64_RELATIVE and R_86_64_64
      // relocations.
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    DYNREL,        DYNREL },     // PDE
      };
      dispatch(ctx, table, i);
      break;
    }
    case R_X86_64_PC8:
    case R_X86_64_PC16:
    case R_X86_64_PC32: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  ERROR,    NONE,  ERROR,         ERROR },      // DSO
        {  ERROR,    NONE,  COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };
      dispatch(ctx, table, i);
      break;
    }
    case R_X86_64_PC64: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  BASEREL,  NONE,  ERROR,         ERROR },      // DSO
        {  BASEREL,  NONE,  COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };
      dispatch(ctx, table, i);
      break;
    }
    case R_X86_64_GOT32:
    case R_X86_64_GOT64:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPC64:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCREL64:
      sym.flags |= NEEDS_GOT;
      break;
    case R_X86_64_GOTPCRELX: {
      if (rel.r_addend != -4)
        Fatal(ctx) << *this << ": bad r_addend for R_X86_64_GOTPCRELX";

      bool do_relax = ctx.arg.relax && !sym.is_imported &&
                      sym.is_relative(ctx) && relax_gotpcrelx(loc - 2);
      if (!do_relax)
        sym.flags |= NEEDS_GOT;
      break;
    }
    case R_X86_64_REX_GOTPCRELX: {
      if (rel.r_addend != -4)
        Fatal(ctx) << *this << ": bad r_addend for R_X86_64_REX_GOTPCRELX";

      bool do_relax = ctx.arg.relax && !sym.is_imported &&
                      sym.is_relative(ctx) && relax_rex_gotpcrelx(loc - 3);
      if (!do_relax)
        sym.flags |= NEEDS_GOT;
      break;
    }
    case R_X86_64_PLT32:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_X86_64_TLSGD:
      if (i + 1 == rels.size())
        Fatal(ctx) << *this
                   << ": TLSGD reloc must be followed by PLT32 or GOTPCREL";

      if (ctx.arg.relax && !ctx.arg.shared && !sym.is_imported)
        i++;
      else
        sym.flags |= NEEDS_TLSGD;
      break;
    case R_X86_64_TLSLD:
      if (i + 1 == rels.size())
        Fatal(ctx) << *this
                   << ": TLSGD reloc must be followed by PLT32 or GOTPCREL";
      if (sym.is_imported)
        Fatal(ctx) << *this << ": TLSLD reloc refers external symbol " << sym;

      if (ctx.arg.relax && !ctx.arg.shared)
        i++;
      else
        sym.flags |= NEEDS_TLSLD;
      break;
    case R_X86_64_DTPOFF32:
    case R_X86_64_DTPOFF64:
      if (sym.is_imported)
        Fatal(ctx) << *this << ": DTPOFF reloc refers external symbol " << sym;
      break;
    case R_X86_64_GOTTPOFF: {
      ctx.has_gottp_rel = true;

      bool do_relax = ctx.arg.relax && !ctx.arg.shared &&
                      !sym.is_imported && relax_gottpoff(loc - 3);
      if (!do_relax)
        sym.flags |= NEEDS_GOTTP;
      break;
    }
    case R_X86_64_GOTPC32_TLSDESC: {
      if (memcmp(loc - 3, "\x48\x8d\x05", 3))
        Fatal(ctx) << *this << ": GOTPC32_TLSDESC relocation is used"
                   << " against an invalid code sequence";

      bool do_relax = ctx.relax_tlsdesc && !sym.is_imported;
      if (!do_relax)
        sym.flags |= NEEDS_TLSDESC;
      break;
    }
    case R_X86_64_TPOFF32:
    case R_X86_64_TPOFF64:
    case R_X86_64_SIZE32:
    case R_X86_64_SIZE64:
    case R_X86_64_TLSDESC_CALL:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}
