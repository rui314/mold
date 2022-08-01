#include "mold.h"

namespace mold::elf {

using E = X86_64;

// This is a security-enhanced version of the regular PLT. The PLT
// header and each PLT entry starts with endbr64 for the Intel's
// control-flow enforcement security mechanism.
//
// Note that our IBT-enabled PLT instruction sequence is different
// from the one used in GNU ld. GNU's IBTPLT implementation uses two
// separate sections (.plt and .plt.sec) in which one PLT entry takes
// 32 bytes in total. Our IBTPLT consists of just .plt and each entry
// is 16 bytes long.
//
// Our PLT entry clobbers %r11, but that's fine because the resolver
// function (_dl_runtime_resolve) clobbers %r11 anyway.
template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  memset(buf, 0xcc, this->shdr.sh_size);

  // Write PLT header
  static const u8 plt0[] = {
    0xf3, 0x0f, 0x1e, 0xfa, // endbr64
    0x41, 0x53,             // push %r11
    0xff, 0x35, 0, 0, 0, 0, // push GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
  };

  memcpy(buf, plt0, sizeof(plt0));
  *(ul32 *)(buf + 8) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr - 4;
  *(ul32 *)(buf + 14) = ctx.gotplt->shdr.sh_addr - this->shdr.sh_addr - 2;

  // Write PLT entries
  static const u8 data[] = {
    0xf3, 0x0f, 0x1e, 0xfa, // endbr64
    0x41, 0xbb, 0, 0, 0, 0, // mov $index_in_relplt, %r11d
    0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOTPLT
  };

  for (Symbol<E> *sym : symbols) {
    i64 idx = sym->get_plt_idx(ctx);
    u8 *ent = buf + E::plt_hdr_size + idx * E::plt_size;
    memcpy(ent, data, sizeof(data));
    *(ul32 *)(ent + 6) = idx;
    *(ul32 *)(ent + 12) = sym->get_gotplt_addr(ctx) - sym->get_plt_addr(ctx) - 16;
  }
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  memset(buf, 0xcc, this->shdr.sh_size);

  static const u8 data[] = {
    0xf3, 0x0f, 0x1e, 0xfa, // endbr64
    0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
  };

  for (Symbol<E> *sym : symbols) {
    u8 *ent = buf + sym->get_pltgot_idx(ctx) * E::pltgot_size;
    memcpy(ent, data, sizeof(data));
    *(ul32 *)(ent + 6) = sym->get_got_addr(ctx) - sym->get_plt_addr(ctx) - 10;
  }
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_X86_64_NONE:
    return;
  case R_X86_64_32:
    *(ul32 *)loc = val;
    return;
  case R_X86_64_64:
    *(ul64 *)loc = val;
    return;
  case R_X86_64_PC32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    return;
  case R_X86_64_PC64:
    *(ul64 *)loc = val - this->shdr.sh_addr - offset;
    return;
  }
  unreachable();
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

static u32 relax_gotpc32_tlsdesc(u8 *loc) {
  switch ((loc[0] << 16) | (loc[1] << 8) | loc[2]) {
  case 0x488d05: return 0x48c7c0; // lea 0(%rip), %rax -> mov $0, %rax
  case 0x488d0d: return 0x48c7c1; // lea 0(%rip), %rcx -> mov $0, %rcx
  case 0x488d15: return 0x48c7c2; // lea 0(%rip), %rdx -> mov $0, %rdx
  case 0x488d1d: return 0x48c7c3; // lea 0(%rip), %rbx -> mov $0, %rbx
  case 0x488d25: return 0x48c7c4; // lea 0(%rip), %rsp -> mov $0, %rsp
  case 0x488d2d: return 0x48c7c5; // lea 0(%rip), %rbp -> mov $0, %rbp
  case 0x488d35: return 0x48c7c6; // lea 0(%rip), %rsi -> mov $0, %rsi
  case 0x488d3d: return 0x48c7c7; // lea 0(%rip), %rdi -> mov $0, %rdi
  case 0x4c8d05: return 0x49c7c0; // lea 0(%rip), %r8  -> mov $0, %r8
  case 0x4c8d0d: return 0x49c7c1; // lea 0(%rip), %r9  -> mov $0, %r9
  case 0x4c8d15: return 0x49c7c2; // lea 0(%rip), %r10 -> mov $0, %r10
  case 0x4c8d1d: return 0x49c7c3; // lea 0(%rip), %r11 -> mov $0, %r11
  case 0x4c8d25: return 0x49c7c4; // lea 0(%rip), %r12 -> mov $0, %r12
  case 0x4c8d2d: return 0x49c7c5; // lea 0(%rip), %r13 -> mov $0, %r13
  case 0x4c8d35: return 0x49c7c6; // lea 0(%rip), %r14 -> mov $0, %r14
  case 0x4c8d3d: return 0x49c7c7; // lea 0(%rip), %r15 -> mov $0, %r15
  }
  return 0;
}

// Apply relocations to SHF_ALLOC sections (i.e. sections that are
// mapped to memory at runtime) based on the result of
// scan_relocations().
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
    if (rel.r_type == R_X86_64_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<E> *frag_ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      frag_ref = &rel_fragments[frag_idx++];

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
      *(ul16 *)loc = val;
    };

    auto write16s = [&](u64 val) {
      overflow_check(val, -(1 << 15), 1 << 15);
      *(ul16 *)loc = val;
    };

    auto write32 = [&](u64 val) {
      overflow_check(val, 0, (i64)1 << 32);
      *(ul32 *)loc = val;
    };

    auto write32s = [&](u64 val) {
      overflow_check(val, -((i64)1 << 31), (i64)1 << 31);
      *(ul32 *)loc = val;
    };

    auto write64 = [&](u64 val) {
      *(ul64 *)loc = val;
    };

#define S   (frag_ref ? frag_ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag_ref ? (u64)frag_ref->addend : (u64)rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.gotplt->shdr.sh_addr)
#define GOT ctx.gotplt->shdr.sh_addr

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
      if (sym.is_absolute() || !ctx.arg.pic) {
        write64(S + A);
      } else if (sym.is_imported) {
        *dynrel++ = {P, R_X86_64_64, (u32)sym.get_dynsym_idx(ctx), A};
        write64(A);
      } else {
        if (!is_relr_reloc(ctx, rel))
          *dynrel++ = {P, R_X86_64_RELATIVE, 0, (i64)(S + A)};
        write64(S + A);
      }
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
      if (sym.is_absolute() || !sym.is_imported || !ctx.arg.shared) {
        write64(S + A - P);
      } else {
        *dynrel++ = {P, R_X86_64_64, (u32)sym.get_dynsym_idx(ctx), A};
        write64(A);
      }
      continue;
    case R_X86_64_PLT32:
      write32s(S + A - P);
      continue;
    case R_X86_64_PLTOFF64:
      write64(S + A - GOT);
      break;
    case R_X86_64_GOT32:
      write32s(G + A);
      continue;
    case R_X86_64_GOT64:
      write64(G + A);
      continue;
    case R_X86_64_GOTOFF64:
      write64(S + A - GOT);
      continue;
    case R_X86_64_GOTPC32:
      write32s(GOT + A - P);
      continue;
    case R_X86_64_GOTPC64:
      write64(GOT + A - P);
      continue;
    case R_X86_64_GOTPCREL:
      write32s(G + GOT + A - P);
      continue;
    case R_X86_64_GOTPCREL64:
      write64(G + GOT + A - P);
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
        i64 val = S - ctx.tls_end + A + 4;
        overflow_check(val, -((i64)1 << 31), (i64)1 << 31);

        switch (rels[i + 1].r_type) {
        case R_X86_64_PLT32:
        case R_X86_64_GOTPCREL:
        case R_X86_64_GOTPCRELX: {
          static const u8 insn[] = {
            0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
            0x48, 0x8d, 0x80, 0,    0,    0, 0,       // lea 0(%rax), %rax
          };
          memcpy(loc - 4, insn, sizeof(insn));
          *(ul32 *)(loc + 8) = val;
          break;
        }
        case R_X86_64_PLTOFF64: {
          static const u8 insn[] = {
            0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
            0x48, 0x8d, 0x80, 0,    0,    0, 0,       // lea 0(%rax), %rax
            0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00,       // nop
          };
          memcpy(loc - 3, insn, sizeof(insn));
          *(ul32 *)(loc + 9) = val;
          break;
        }
        default:
          unreachable();
        }

        i++;
      } else {
        write32s(sym.get_tlsgd_addr(ctx) + A - P);
      }
      continue;
    case R_X86_64_TLSLD:
      if (ctx.got->tlsld_idx == -1) {
        // Relax LD to LE
        switch (rels[i + 1].r_type) {
        case R_X86_64_PLT32: {
          static const u8 insn[] = {
            0x66, 0x66, 0x66,                         // (padding)
            0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
          };
          memcpy(loc - 3, insn, sizeof(insn));
          break;
        }
        case R_X86_64_GOTPCREL:
        case R_X86_64_GOTPCRELX: {
          static const u8 insn[] = {
            0x66, 0x66, 0x66,                         // (padding)
            0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
            0x90,                                     // nop
          };
          memcpy(loc - 3, insn, sizeof(insn));
          break;
        }
        case R_X86_64_PLTOFF64: {
          static const u8 insn[] = {
            0x66, 0x66, 0x66,                         // (padding)
            0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
            0x66, 0x66, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00, // nop
          };
          memcpy(loc - 3, insn, sizeof(insn));
          break;
        }
        default:
          unreachable();
        }

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
        write64(S + A - ctx.tls_end);
      else
        write64(S + A - ctx.tls_begin);
      continue;
    case R_X86_64_TPOFF32:
      write32s(S + A - ctx.tls_end);
      continue;
    case R_X86_64_TPOFF64:
      write64(S + A - ctx.tls_end);
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
      if (sym.get_tlsdesc_idx(ctx) == -1) {
        u32 insn = relax_gotpc32_tlsdesc(loc - 3);
        loc[-3] = insn >> 16;
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        write32s(S + A - ctx.tls_end + 4);
      } else {
        write32s(sym.get_tlsdesc_addr(ctx) + A - P);
      }
      continue;
    case R_X86_64_SIZE32:
      write32(sym.esym().st_size + A);
      continue;
    case R_X86_64_SIZE64:
      write64(sym.esym().st_size + A);
      continue;
    case R_X86_64_TLSDESC_CALL:
      if (sym.get_tlsdesc_idx(ctx) == -1) {
        // call *(%rax) -> nop
        loc[0] = 0x66;
        loc[1] = 0x90;
      }
      continue;
    default:
      unreachable();
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
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_X86_64_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      add_undef(ctx, file, sym, this, rel.r_offset);
      continue;
    }

    SectionFragment<E> *frag;
    i64 addend;
    std::tie(frag, addend) = get_fragment(ctx, rel);

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

    auto write16 = [&](u64 val) {
      overflow_check(val, 0, 1 << 16);
      *(ul16 *)loc = val;
    };

    auto write32 = [&](u64 val) {
      overflow_check(val, 0, (i64)1 << 32);
      *(ul32 *)loc = val;
    };

    auto write32s = [&](u64 val) {
      overflow_check(val, -((i64)1 << 31), (i64)1 << 31);
      *(ul32 *)loc = val;
    };

#define S (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A (frag ? (u64)addend : (u64)rel.r_addend)

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
      if (!frag) {
        if (std::optional<u64> val = get_tombstone(sym)) {
          *(ul64 *)loc = *val;
          break;
        }
      }
      *(ul64 *)loc = S + A;
      break;
    case R_X86_64_DTPOFF32:
      if (std::optional<u64> val = get_tombstone(sym))
        *(ul32 *)loc = *val;
      else
        write32s(S + A - ctx.tls_begin);
      break;
    case R_X86_64_DTPOFF64:
      if (std::optional<u64> val = get_tombstone(sym))
        *(ul64 *)loc = *val;
      else
        *(ul64 *)loc = S + A - ctx.tls_begin;
      break;
    case R_X86_64_SIZE32:
      write32(sym.esym().st_size + A);
      break;
    case R_X86_64_SIZE64:
      *(ul64 *)loc = sym.esym().st_size + A;
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
void InputSection<E>::scan_relocations(Context<E> &ctx) {
  assert(shdr().sh_flags & SHF_ALLOC);

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<E>);
  std::span<const ElfRel<E>> rels = get_rels(ctx);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_X86_64_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (!sym.file) {
      add_undef(ctx, file, sym, this, rel.r_offset);
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
        {  NONE,     NONE,  COPYREL,       CPLT  },      // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_X86_64_64: {
      // Unlike the above, we can use R_X86_64_RELATIVE and R_86_64_64
      // relocations.
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    COPYREL,       CPLT   },     // PDE
      };
      dispatch(ctx, table, i, rel, sym);
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
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_X86_64_PC64: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  ERROR,    NONE,  DYNREL,        DYNREL },     // DSO
        {  ERROR,    NONE,  COPYREL,       PLT    },     // PIE
        {  NONE,     NONE,  COPYREL,       PLT    },     // PDE
      };
      dispatch(ctx, table, i, rel, sym);
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
                      sym.is_relative() && relax_gotpcrelx(loc - 2);
      if (!do_relax)
        sym.flags |= NEEDS_GOT;
      break;
    }
    case R_X86_64_REX_GOTPCRELX: {
      if (rel.r_addend != -4)
        Fatal(ctx) << *this << ": bad r_addend for R_X86_64_REX_GOTPCRELX";

      bool do_relax = ctx.arg.relax && !sym.is_imported &&
                      sym.is_relative() && relax_rex_gotpcrelx(loc - 3);
      if (!do_relax)
        sym.flags |= NEEDS_GOT;
      break;
    }
    case R_X86_64_PLT32:
    case R_X86_64_PLTOFF64: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  NONE,     NONE,  PLT,           PLT    },     // DSO
        {  NONE,     NONE,  PLT,           PLT    },     // PIE
        {  NONE,     NONE,  PLT,           PLT    },     // PDE
      };
      dispatch(ctx, table, i, rel, sym);
      break;
    }
    case R_X86_64_TLSGD: {
      if (i + 1 == rels.size())
        Fatal(ctx) << *this << ": TLSGD reloc must be followed by PLT or GOTPCREL";

      if (u32 ty = rels[i + 1].r_type;
          ty != R_X86_64_PLT32 && ty != R_X86_64_PLTOFF64 &&
          ty != R_X86_64_GOTPCREL && ty != R_X86_64_GOTPCRELX)
        Fatal(ctx) << *this << ": TLSGD reloc must be followed by PLT or GOTPCREL";

      if (ctx.arg.relax && !ctx.arg.shared && !sym.is_imported)
        i++;
      else
        sym.flags |= NEEDS_TLSGD;
      break;
    }
    case R_X86_64_TLSLD: {
      if (i + 1 == rels.size())
        Fatal(ctx) << *this << ": TLSLD reloc must be followed by PLT or GOTPCREL";

      if (u32 ty = rels[i + 1].r_type;
          ty != R_X86_64_PLT32 && ty != R_X86_64_PLTOFF64 &&
          ty != R_X86_64_GOTPCREL && ty != R_X86_64_GOTPCRELX)
        Fatal(ctx) << *this << ": TLSLD reloc must be followed by PLT or GOTPCREL";

      if (ctx.arg.relax && !ctx.arg.shared)
        i++;
      else
        ctx.needs_tlsld = true;
      break;
    }
    case R_X86_64_GOTTPOFF: {
      ctx.has_gottp_rel = true;

      bool do_relax = ctx.arg.relax && !ctx.arg.shared &&
                      !sym.is_imported && relax_gottpoff(loc - 3);
      if (!do_relax)
        sym.flags |= NEEDS_GOTTP;
      break;
    }
    case R_X86_64_GOTPC32_TLSDESC: {
      if (relax_gotpc32_tlsdesc(loc - 3) == 0)
        Fatal(ctx) << *this << ": GOTPC32_TLSDESC relocation is used"
                   << " against an invalid code sequence";

      bool do_relax = ctx.relax_tlsdesc && !sym.is_imported;
      if (!do_relax)
        sym.flags |= NEEDS_TLSDESC;
      break;
    }
    case R_X86_64_GOTOFF64:
    case R_X86_64_DTPOFF32:
    case R_X86_64_DTPOFF64:
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

} // namespace mold::elf
