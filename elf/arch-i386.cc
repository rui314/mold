// i386 is similar to x86-64 but lacks PC-relative memory access
// instructions. So it's not straightforward to support position-
// independent code (PIC) on that target.
//
// If an object file is compiled with -fPIC, a function that needs to load
// a value from memory first obtains its own address with the following
// code
//
//   call __x86.get_pc_thunk.bx
//
// where __x86.get_pc_thunk.bx is defined as
//
//   __x86.get_pc_thunk.bx:
//     mov (%esp), %ebx  # move the return address to %ebx
//     ret
//
// . With the function's own address (or, more precisely, the address
// immediately after the call instruction), the function can compute an
// absolute address of a variable with its address + link-time constant.
//
// Executing call-mov-ret isn't very cheap, and allocating one register to
// store PC isn't cheap too, especially given that i386 has only 8
// general-purpose registers. But that's the cost of PIC on i386. You need
// to pay it when creating a .so and a position-independent executable.
//
// When a position-independent function calls another function, it sets
// %ebx to the address of .got. Position-independent PLT entries use that
// register to load values from .got.plt/.got.
//
// If we are creating a position-dependent executable (PDE), we can't
// assume that %ebx is set to .got. For PDE, we need to create position-
// dependent PLT entries which don't use %ebx.
//
// https://github.com/rui314/mold/wiki/i386-psabi.pdf

#include "mold.h"

namespace mold::elf {

using E = I386;

static void write_plt_header(Context<E> &ctx, u8 *buf) {
  if (ctx.arg.pic) {
    static const u8 plt0[] = {
      0xff, 0xb3, 0, 0, 0, 0, // pushl GOTPLT+4(%ebx)
      0xff, 0xa3, 0, 0, 0, 0, // jmp *GOTPLT+8(%ebx)
      0x90, 0x90, 0x90, 0x90, // nop
    };
    memcpy(buf, plt0, sizeof(plt0));
    *(ul32 *)(buf + 2) = ctx.gotplt->shdr.sh_addr - ctx.got->shdr.sh_addr + 4;
    *(ul32 *)(buf + 8) = ctx.gotplt->shdr.sh_addr - ctx.got->shdr.sh_addr + 8;
  } else {
    static const u8 plt0[] = {
      0xff, 0x35, 0, 0, 0, 0, // pushl GOTPLT+4
      0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+8
      0x90, 0x90, 0x90, 0x90, // nop
    };
    memcpy(buf, plt0, sizeof(plt0));
    *(ul32 *)(buf + 2) = ctx.gotplt->shdr.sh_addr + 4;
    *(ul32 *)(buf + 8) = ctx.gotplt->shdr.sh_addr + 8;
  }
}

static void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym,
                            i64 idx) {
  u8 *ent = buf + E::plt_hdr_size + sym.get_plt_idx(ctx) * E::plt_size;

  if (ctx.arg.pic) {
    static const u8 data[] = {
      0xff, 0xa3, 0, 0, 0, 0, // jmp *foo@GOT(%ebx)
      0x68, 0,    0, 0, 0,    // pushl $reloc_offset
      0xe9, 0,    0, 0, 0,    // jmp .PLT0@PC
    };
    memcpy(ent, data, sizeof(data));
    *(ul32 *)(ent + 2) = sym.get_gotplt_addr(ctx) - ctx.got->shdr.sh_addr;
  } else {
    static const u8 data[] = {
      0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
      0x68, 0,    0, 0, 0,    // pushl $reloc_offset
      0xe9, 0,    0, 0, 0,    // jmp .PLT0@PC
    };
    memcpy(ent, data, sizeof(data));
    *(ul32 *)(ent + 2) = sym.get_gotplt_addr(ctx);
  }

  *(ul32 *)(ent + 7) = idx * sizeof(ElfRel<E>);
  *(ul32 *)(ent + 12) = ctx.plt->shdr.sh_addr - sym.get_plt_addr(ctx) - 16;
}

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;
  write_plt_header(ctx, buf);

  for (i64 i = 0; i < symbols.size(); i++)
    write_plt_entry(ctx, buf, *symbols[i], i);
}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->shdr.sh_offset;

  if (ctx.arg.pic) {
    static const u8 data[] = {
      0xff, 0xa3, 0, 0, 0, 0, // jmp   *foo@GOT(%ebx)
      0x66, 0x90,             // nop
    };

    for (i64 i = 0; i < symbols.size(); i++) {
      u8 *ent = buf + i * sizeof(data);
      memcpy(ent, data, sizeof(data));
      *(ul32 *)(ent + 2) = symbols[i]->get_got_addr(ctx) - ctx.got->shdr.sh_addr;
    }
  } else {
    static const u8 data[] = {
      0xff, 0x25, 0, 0, 0, 0, // jmp   *foo@GOT
      0x66, 0x90,             // nop
    };

    for (i64 i = 0; i < symbols.size(); i++) {
      u8 *ent = buf + i * sizeof(data);
      memcpy(ent, data, sizeof(data));
      *(ul32 *)(ent + 2) = symbols[i]->get_got_addr(ctx);
    }
  }
}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    return;
  case R_386_32:
    *(ul32 *)loc = val;
    return;
  case R_386_PC32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    return;
  }
  unreachable();
}

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
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<E> *frag_ref = nullptr;
    if (rel_fragments && rel_fragments[frag_idx].idx == i)
      frag_ref = &rel_fragments[frag_idx++];

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   (frag_ref ? frag_ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag_ref ? frag_ref->addend : this->get_addend(rel))
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_386_8: {
      i64 val = S + A;
      check(val, 0, 1 << 8);
      *loc = val;
      break;
    }
    case R_386_16: {
      i64 val = S + A;
      check(val, 0, 1 << 16);
      *(ul16 *)loc = val;
      break;
    }
    case R_386_32:
      apply_abs_dyn_rel(ctx, sym, rel, loc, S, A, P, dynrel);
      break;
    case R_386_PC8: {
      i64 val = S + A - P;
      check(val, -(1 << 7), 1 << 7);
      *loc = val;
      break;
    };
    case R_386_PC16: {
      i64 val = S + A - P;
      check(val, -(1 << 15), 1 << 15);
      *(ul16 *)loc = val;
      break;
    }
    case R_386_PC32:
    case R_386_PLT32:
      *(ul32 *)loc = S + A - P;
      break;
    case R_386_GOT32:
    case R_386_GOT32X:
      *(ul32 *)loc = G + A;
      break;
    case R_386_GOTOFF:
      *(ul32 *)loc = S + A - GOT;
      break;
    case R_386_GOTPC:
      *(ul32 *)loc = GOT + A - P;
      break;
    case R_386_TLS_GOTIE:
      *(ul32 *)loc = sym.get_gottp_addr(ctx) + A - GOT;
      break;
    case R_386_TLS_LE:
      *(ul32 *)loc = S + A - ctx.tp_addr;
      break;
    case R_386_TLS_IE:
      *(ul32 *)loc = sym.get_gottp_addr(ctx) + A;
      break;
    case R_386_TLS_GD:
      if (sym.get_tlsgd_idx(ctx) == -1) {
        // Relax GD to LE
        switch (rels[i + 1].r_type) {
        case R_386_PLT32: {
          static const u8 insn[] = {
            0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %rax
            0x81, 0xe8, 0, 0, 0, 0, // add $0, %rax
          };
          memcpy(loc - 3, insn, sizeof(insn));
          *(ul32 *)(loc + 5) = ctx.tp_addr - S - A;
          break;
        }
        case R_386_GOT32: {
          static const u8 insn[] = {
            0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %rax
            0x81, 0xe8, 0, 0, 0, 0, // add $0, %rax
          };
          memcpy(loc - 2, insn, sizeof(insn));
          *(ul32 *)(loc + 6) = ctx.tp_addr - S - A;
          break;
        }
        default:
          unreachable();
        }

        i++;
      } else {
        *(ul32 *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      }
      break;
    case R_386_TLS_LDM:
      if (ctx.got->tlsld_idx == -1) {
        // Relax LD to LE
        switch (rels[i + 1].r_type) {
        case R_386_PLT32: {
          static const u8 insn[] = {
            0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
            0x8d, 0x74, 0x26, 0x00, // lea (%esi,1), %esi
            0x90,                   // nop
          };
          memcpy(loc - 2, insn, sizeof(insn));
          break;
        }
        case R_386_GOT32: {
          static const u8 insn[] = {
            0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
            0x8d, 0x74, 0x26, 0x00, // lea (%esi,1), %esi
            0x66, 0x90,
          };
          memcpy(loc - 2, insn, sizeof(insn));
          break;
        }
        default:
          unreachable();
        }

        i++;
      } else {
        *(ul32 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      }
      break;
    case R_386_TLS_LDO_32:
      if (ctx.got->tlsld_idx == -1)
        *(ul32 *)loc = S + A - ctx.tp_addr;
      else
        *(ul32 *)loc = S + A - ctx.tls_begin;
      break;
    case R_386_SIZE32:
      *(ul32 *)loc = sym.esym().st_size + A;
      break;
    case R_386_TLS_GOTDESC:
      if (sym.get_tlsdesc_idx(ctx) == -1) {
        static const u8 insn[] = {
          0x8d, 0x05, 0, 0, 0, 0, // lea 0, %eax
        };
        memcpy(loc - 2, insn, sizeof(insn));
        *(ul32 *)loc = S + A - ctx.tp_addr;
      } else {
        *(ul32 *)loc = sym.get_tlsdesc_addr(ctx) + A - GOT;
      }
      break;
    case R_386_TLS_DESC_CALL:
      if (ctx.arg.relax && !ctx.arg.shared) {
        // call *(%rax) -> nop
        loc[0] = 0x66;
        loc[1] = 0x90;
      }
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
    i64 addend;
    std::tie(frag, addend) = get_fragment(ctx, rel);

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (val < lo || hi <= val)
        Error(ctx) << *this << ": relocation " << rel << " against "
                   << sym << " out of range: " << val << " is not in ["
                   << lo << ", " << hi << ")";
    };

#define S   (frag ? frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (frag ? addend : this->get_addend(rel))
#define G   (sym.get_got_idx(ctx) * sizeof(Word<E>))
#define GOT ctx.got->shdr.sh_addr

    switch (rel.r_type) {
    case R_386_8: {
      i64 val = S + A;
      check(val, 0, 1 << 8);
      *loc = val;
      break;
    }
    case R_386_16: {
      i64 val = S + A;
      check(val, 0, 1 << 16);
      *(ul16 *)loc = val;
      break;
    }
    case R_386_32:
      if (!frag) {
        if (std::optional<u64> val = get_tombstone(sym)) {
          *(ul32 *)loc = *val;
          break;
        }
      }
      *(ul32 *)loc = S + A;
      break;
    case R_386_PC8: {
      i64 val = S + A;
      check(val, -(1 << 7), 1 << 7);
      *loc = val;
      break;
    }
    case R_386_PC16: {
      i64 val = S + A;
      check(val, -(1 << 15), 1 << 15);
      *(ul16 *)loc = val;
      break;
    };
    case R_386_PC32:
      *(ul32 *)loc = S + A;
      break;
    case R_386_GOTPC:
      *(ul32 *)loc = GOT + A;
      break;
    case R_386_GOTOFF:
      *(ul32 *)loc = S + A - GOT;
      break;
    case R_386_TLS_LDO_32:
      if (std::optional<u64> val = get_tombstone(sym))
        *(ul32 *)loc = *val;
      else
        *(ul32 *)loc = S + A - ctx.tls_begin;
      break;
    case R_386_SIZE32:
      *(ul32 *)loc = sym.esym().st_size + A;
      break;
    default:
      unreachable();
    }

#undef S
#undef A
#undef GOT
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
    case R_386_8:
    case R_386_16:
      scan_abs_rel(ctx, sym, rel);
      break;
    case R_386_32:
      scan_abs_dyn_rel(ctx, sym, rel);
      break;
    case R_386_PC8:
    case R_386_PC16:
    case R_386_PC32:
      scan_pcrel_rel(ctx, sym, rel);
      break;
    case R_386_GOT32:
    case R_386_GOT32X:
    case R_386_GOTPC:
      sym.flags |= NEEDS_GOT;
      break;
    case R_386_PLT32:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_386_TLS_GOTIE:
    case R_386_TLS_LE:
    case R_386_TLS_IE:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_386_TLS_GD:
      if (i + 1 == rels.size())
        Fatal(ctx) << *this << ": TLS_GD reloc must be followed by PLT or GOT32";

      if (u32 ty = rels[i + 1].r_type;
          ty != R_386_PLT32 && ty != R_386_GOT32)
        Fatal(ctx) << *this << ": TLS_GD reloc must be followed by PLT or GOT32";

      if (ctx.arg.relax && !ctx.arg.shared && !sym.is_imported)
        i++;
      else
        sym.flags |= NEEDS_TLSGD;
      break;
    case R_386_TLS_LDM:
      if (i + 1 == rels.size())
        Fatal(ctx) << *this << ": TLS_LDM reloc must be followed by PLT or GOT32";

      if (u32 ty = rels[i + 1].r_type;
          ty != R_386_PLT32 && ty != R_386_GOT32)
        Fatal(ctx) << *this << ": TLS_LDM reloc must be followed by PLT or GOT32";

      if (ctx.arg.relax && !ctx.arg.shared)
        i++;
      else
        ctx.needs_tlsld = true;
      break;
    case R_386_TLS_GOTDESC:
      if (!ctx.arg.relax || ctx.arg.shared)
        sym.flags |= NEEDS_TLSDESC;
      break;
    case R_386_GOTOFF:
    case R_386_TLS_LDO_32:
    case R_386_SIZE32:
    case R_386_TLS_DESC_CALL:
      break;
    default:
      Error(ctx) << *this << ": unknown relocation: " << rel;
    }
  }
}

} // namespace mold::elf
