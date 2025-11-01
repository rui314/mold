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
// https://github.com/rui314/psabi/blob/main/i386.pdf

#if MOLD_I386

#include "mold.h"

namespace mold {

using E = I386;

template <>
i64 get_addend(u8 *loc, const ElfRel<E> &rel) {
  switch (rel.r_type) {
  case R_386_8:
  case R_386_PC8:
    return *loc;
  case R_386_16:
  case R_386_PC16:
    return *(ul16 *)loc;
  case R_386_32:
  case R_386_PC32:
  case R_386_GOT32:
  case R_386_GOT32X:
  case R_386_PLT32:
  case R_386_GOTOFF:
  case R_386_GOTPC:
  case R_386_TLS_LDM:
  case R_386_TLS_GOTIE:
  case R_386_TLS_LE:
  case R_386_TLS_IE:
  case R_386_TLS_GD:
  case R_386_TLS_LDO_32:
  case R_386_SIZE32:
  case R_386_TLS_GOTDESC:
    return *(ul32 *)loc;
  default:
    return 0;
  }
}

template <>
void write_addend(u8 *loc, i64 val, const ElfRel<E> &rel) {
  switch (rel.r_type) {
  case R_386_NONE:
    break;
  case R_386_8:
  case R_386_PC8:
    *loc = val;
    break;
  case R_386_16:
  case R_386_PC16:
    *(ul16 *)loc = val;
    break;
  case R_386_32:
  case R_386_PC32:
  case R_386_GOT32:
  case R_386_GOT32X:
  case R_386_PLT32:
  case R_386_GOTOFF:
  case R_386_GOTPC:
  case R_386_TLS_LDM:
  case R_386_TLS_GOTIE:
  case R_386_TLS_LE:
  case R_386_TLS_IE:
  case R_386_TLS_GD:
  case R_386_TLS_LDO_32:
  case R_386_SIZE32:
  case R_386_TLS_GOTDESC:
    *(ul32 *)loc = val;
    break;
  default:
    unreachable();
  }
}

template <>
void write_plt_header(Context<E> &ctx, u8 *buf) {
  if (ctx.arg.pic) {
    static const u8 insn[] = {
      0x51,                   // push   %ecx
      0x8d, 0x8b, 0, 0, 0, 0, // lea    GOTPLT+4(%ebx), %ecx
      0xff, 0x31,             // push   (%ecx)
      0xff, 0x61, 0x04,       // jmp    *0x4(%ecx)
      0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    };
    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 3) = ctx.gotplt->shdr.sh_addr - ctx.got->shdr.sh_addr + 4;
  } else {
    static const u8 insn[] = {
      0x51,                         // push   %ecx
      0xb9, 0, 0, 0, 0,             // mov    GOTPLT+4, %ecx
      0xff, 0x31,                   // push   (%ecx)
      0xff, 0x61, 0x04,             // jmp    *0x4(%ecx)
      0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    };
    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 2) = ctx.gotplt->shdr.sh_addr + 4;
  }
}

template <>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  if (ctx.arg.pic) {
    static const u8 insn[] = {
      0xb9, 0, 0, 0, 0,             // mov $reloc_offset, %ecx
      0xff, 0xa3, 0, 0, 0, 0,       // jmp *foo@GOT(%ebx)
      0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    };
    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 1) = sym.get_plt_idx(ctx) * sizeof(ElfRel<E>);
    *(ul32 *)(buf + 7) = sym.get_gotplt_addr(ctx) - ctx.got->shdr.sh_addr;
  } else {
    static const u8 insn[] = {
      0xb9, 0, 0, 0, 0,             // mov $reloc_offset, %ecx
      0xff, 0x25, 0, 0, 0, 0,       // jmp *foo@GOT
      0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
    };
    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 1) = sym.get_plt_idx(ctx) * sizeof(ElfRel<E>);
    *(ul32 *)(buf + 7) = sym.get_gotplt_addr(ctx);
  }
}

template <>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym) {
  if (ctx.arg.pic) {
    static const u8 insn[] = {
      0xff, 0xa3, 0, 0, 0, 0, // jmp *foo@GOT(%ebx)
      0xcc, 0xcc,             // (padding)
    };
    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 2) = sym.get_got_pltgot_addr(ctx) - ctx.got->shdr.sh_addr;
  } else {
    static const u8 insn[] = {
      0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOT
      0xcc, 0xcc,             // (padding)
    };
    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 2) = sym.get_got_pltgot_addr(ctx);
  }
}

template <>
void EhFrameSection<E>::apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                       u64 offset, u64 val) {
  u8 *loc = ctx.buf + this->shdr.sh_offset + offset;

  switch (rel.r_type) {
  case R_NONE:
    break;
  case R_386_32:
    *(ul32 *)loc = val;
    break;
  case R_386_PC32:
    *(ul32 *)loc = val - this->shdr.sh_addr - offset;
    break;
  default:
    Fatal(ctx) << "unsupported relocation in .eh_frame: " << rel;
  }
}

static u32 relax_got32x(u8 *loc) {
  // mov imm(%reg1), %reg2 -> lea imm(%reg1), %reg2
  if (loc[0] == 0x8b)
    return 0x8d00 | loc[1];
  return 0;
}

// Relax GD to LE
static void relax_gd_to_le(u8 *loc, ElfRel<E> rel, u64 val) {
  static const u8 insn[] = {
    0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
    0x81, 0xc0, 0, 0, 0, 0, // add $tp_offset, %eax
  };

  switch (rel.r_type) {
  case R_386_PLT32:
  case R_386_PC32:
    memcpy(loc - 3, insn, sizeof(insn));
    *(ul32 *)(loc + 5) = val;
    break;
  case R_386_GOT32:
  case R_386_GOT32X:
    memcpy(loc - 2, insn, sizeof(insn));
    *(ul32 *)(loc + 6) = val;
    break;
  default:
    unreachable();
  }
}

// Relax LD to LE
static void relax_ld_to_le(u8 *loc, ElfRel<E> rel, u64 tls_size) {
  switch (rel.r_type) {
  case R_386_PLT32:
  case R_386_PC32: {
    static const u8 insn[] = {
      0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
      0x2d, 0, 0, 0, 0,       // sub $tls_size, %eax
    };
    memcpy(loc - 2, insn, sizeof(insn));
    *(ul32 *)(loc + 5) = tls_size;
    break;
  }
  case R_386_GOT32:
  case R_386_GOT32X: {
    static const u8 insn[] = {
      0x65, 0xa1, 0, 0, 0, 0, // mov %gs:0, %eax
      0x81, 0xe8, 0, 0, 0, 0, // sub $tls_size, %eax
    };
    memcpy(loc - 2, insn, sizeof(insn));
    *(ul32 *)(loc + 6) = tls_size;
    break;
  }
  default:
    unreachable();
  }
}

static u32 relax_tlsdesc_to_ie(u8 *loc) {
  switch ((loc[0] << 8) | loc[1]) {
  case 0x8d83: return 0x8b83; // lea 0(%ebx), %eax -> mov 0(%ebx), %eax
  case 0x8d9b: return 0x8b9b; // lea 0(%ebx), %ebx -> mov 0(%ebx), %ebx
  case 0x8d8b: return 0x8b8b; // lea 0(%ebx), %ecx -> mov 0(%ebx), %ecx
  case 0x8d93: return 0x8b93; // lea 0(%ebx), %edx -> mov 0(%ebx), %edx
  case 0x8db3: return 0x8bb3; // lea 0(%ebx), %esi -> mov 0(%ebx), %esi
  case 0x8dbb: return 0x8bbb; // lea 0(%ebx), %edi -> mov 0(%ebx), %edi
  case 0x8da3: return 0x8ba3; // lea 0(%ebx), %esp -> mov 0(%ebx), %esp
  case 0x8dab: return 0x8bab; // lea 0(%ebx), %ebp -> mov 0(%ebx), %ebp
  }
  return 0;
}

static u32 relax_tlsdesc_to_le(u8 *loc) {
  switch ((loc[0] << 8) | loc[1]) {
  case 0x8d83: return 0x90b8; // lea 0(%ebx), %eax -> mov $0, %eax
  case 0x8d9b: return 0x90bb; // lea 0(%ebx), %ebx -> mov $0, %ebx
  case 0x8d8b: return 0x90b9; // lea 0(%ebx), %ecx -> mov $0, %ecx
  case 0x8d93: return 0x90ba; // lea 0(%ebx), %edx -> mov $0, %edx
  case 0x8db3: return 0x90be; // lea 0(%ebx), %esi -> mov $0, %esi
  case 0x8dbb: return 0x90bf; // lea 0(%ebx), %edi -> mov $0, %edi
  case 0x8da3: return 0x90bc; // lea 0(%ebx), %esp -> mov $0, %esp
  case 0x8dab: return 0x90bd; // lea 0(%ebx), %ebp -> mov $0, %ebp
  }
  return 0;
}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {
  std::span<const ElfRel<E>> rels = get_rels(ctx);
  RelocationsStats rels_stats;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (rel.r_type == R_NONE)
      continue;

    Symbol<E> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    u64 S = sym.get_addr(ctx);
    u64 A = get_addend(*this, rel);
    u64 P = get_addr() + rel.r_offset;
    u64 G = sym.get_got_idx(ctx) * sizeof(Word<E>);
    u64 GOT = ctx.got->shdr.sh_addr;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (ctx.arg.stats)
        update_relocation_stats(rels_stats, i, val, lo, hi);
      check_range(ctx, i, val, lo, hi);
    };

    switch (rel.r_type) {
    case R_386_8:
      check(S + A, 0, 1 << 8);
      *loc = S + A;
      break;
    case R_386_16:
      check(S + A, 0, 1 << 16);
      *(ul16 *)loc = S + A;
      break;
    case R_386_32:
      break;
    case R_386_PC8:
      check(S + A - P, -(1 << 7), 1 << 7);
      *loc = S + A - P;
      break;
    case R_386_PC16:
      check(S + A - P, -(1 << 15), 1 << 15);
      *(ul16 *)loc = S + A - P;
      break;
    case R_386_PC32:
    case R_386_PLT32:
      *(ul32 *)loc = S + A - P;
      break;
    case R_386_GOT32:
      *(ul32 *)loc = G + A;
      break;
    case R_386_GOT32X:
      if (sym.has_got(ctx)) {
        *(ul32 *)loc = G + A;
      } else {
        u32 insn = relax_got32x(loc - 2);
        assert(insn);
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        *(ul32 *)loc = S + A - GOT;
      }
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
      if (sym.has_tlsgd(ctx))
        *(ul32 *)loc = sym.get_tlsgd_addr(ctx) + A - GOT;
      else
        relax_gd_to_le(loc, rels[++i], S - ctx.tp_addr);
      break;
    case R_386_TLS_LDM:
      if (ctx.got->has_tlsld(ctx))
        *(ul32 *)loc = ctx.got->get_tlsld_addr(ctx) + A - GOT;
      else
        relax_ld_to_le(loc, rels[++i], ctx.tp_addr - ctx.tls_begin);
      break;
    case R_386_TLS_LDO_32:
      *(ul32 *)loc = S + A - ctx.dtp_addr;
      break;
    case R_386_SIZE32:
      *(ul32 *)loc = sym.esym().st_size + A;
      break;
    case R_386_TLS_GOTDESC:
      // i386 TLSDESC uses the following code sequence to materialize
      // a TP-relative address in %eax.
      //
      //   lea    0(%ebx), %eax
      //       R_386_TLS_GOTDESC   foo
      //   call   *(%eax)
      //       R_386_TLS_DESC_CALL foo
      //
      // We may relax the instructions to the following if its TP-relative
      // address is known at link-time
      //
      //   mov     $foo@TPOFF, %eax
      //   nop
      //
      // or to the following if the TP-relative address is known at
      // process startup time.
      //
      //   mov     foo@GOTTPOFF(%ebx), %eax
      //   nop
      //
      // We allow the following alternative code sequence too because
      // LLVM emits such code.
      //
      //   lea    0(%ebx), %reg
      //       R_386_TLS_GOTDESC   foo
      //   mov    %reg, %eax
      //   call   *(%eax)
      //       R_386_TLS_DESC_CALL foo
      //
      // Note that the compiler always uses the local-exec TLS model
      // for -fno-pic, so TLSDESC code is always PIC (i.e. uses %ebx to
      // store the address of GOT.)
      if (sym.has_tlsdesc(ctx)) {
        *(ul32 *)loc = sym.get_tlsdesc_addr(ctx) + A - GOT;
      } else if (sym.has_gottp(ctx)) {
        u32 insn = relax_tlsdesc_to_ie(loc - 2);
        if (!insn)
          Fatal(ctx) << *this << ": illegal instruction sequence for TLSDESC";
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        *(ul32 *)loc = sym.get_gottp_addr(ctx) + A - GOT;
      } else {
        u32 insn = relax_tlsdesc_to_le(loc - 2);
        if (!insn)
          Fatal(ctx) << *this << ": illegal instruction sequence for TLSDESC";
        loc[-2] = insn >> 8;
        loc[-1] = insn;
        *(ul32 *)loc = S + A - ctx.tp_addr;
      }
      break;
    case R_386_TLS_DESC_CALL:
      if (!sym.has_tlsdesc(ctx)) {
        // call *(%eax) -> nop
        loc[0] = 0x66;
        loc[1] = 0x90;
      }
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
  RelocationsStats rels_stats;

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
    u64 A = frag ? frag_addend : get_addend(*this, rel);
    u64 GOT = ctx.got->shdr.sh_addr;

    auto check = [&](i64 val, i64 lo, i64 hi) {
      if (ctx.arg.stats)
        update_relocation_stats(rels_stats, i, val, lo, hi);
      check_range(ctx, val, i, lo, hi);
    };

    switch (rel.r_type) {
    case R_386_8:
      check(S + A, 0, 1 << 8);
      *loc = S + A;
      break;
    case R_386_16:
      check(S + A, 0, 1 << 16);
      *(ul16 *)loc = S + A;
      break;
    case R_386_32:
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul32 *)loc = *val;
      else
        *(ul32 *)loc = S + A;
      break;
    case R_386_PC8:
      check(S + A, -(1 << 7), 1 << 7);
      *loc = S + A;
      break;
    case R_386_PC16:
      check(S + A, -(1 << 15), 1 << 15);
      *(ul16 *)loc = S + A;
      break;
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
      if (std::optional<u64> val = get_tombstone(sym, frag))
        *(ul32 *)loc = *val;
      else
        *(ul32 *)loc = S + A - ctx.dtp_addr;
      break;
    case R_386_SIZE32:
      *(ul32 *)loc = sym.esym().st_size + A;
      break;
    default:
      unreachable();
    }
  }
  if (ctx.arg.stats)
    save_relocation_stats<E>(ctx, *this, rels_stats);
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
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (sym.is_ifunc())
      sym.flags |= NEEDS_GOT | NEEDS_PLT;

    if (rel.r_type == R_386_TLS_GD || rel.r_type == R_386_TLS_LDM) {
      if (i + 1 == rels.size())
        Fatal(ctx) << *this << ": " << rel << " must be followed by PLT or GOT32";

      if (u32 ty = rels[i + 1].r_type;
          ty != R_386_PLT32 && ty != R_386_PC32 &&
          ty != R_386_GOT32 && ty != R_386_GOT32X)
        Fatal(ctx) << *this << ": " << rel << " must be followed by PLT or GOT32";
    }

    switch (rel.r_type) {
    case R_386_8:
    case R_386_16:
      scan_absrel(ctx, sym, rel);
      break;
    case R_386_PC8:
    case R_386_PC16:
    case R_386_PC32:
      scan_pcrel(ctx, sym, rel);
      break;
    case R_386_GOT32:
    case R_386_GOTPC:
      sym.flags |= NEEDS_GOT;
      break;
    case R_386_GOT32X:
      // We always want to relax GOT32X even if --no-relax is given
      // because static PIE doesn't work without it.
      if (sym.is_pcrel_linktime_const(ctx) && relax_got32x(loc - 2)) {
        // Do nothing
      } else {
        sym.flags |= NEEDS_GOT;
      }
      break;
    case R_386_PLT32:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      break;
    case R_386_TLS_GOTIE:
    case R_386_TLS_IE:
      sym.flags |= NEEDS_GOTTP;
      break;
    case R_386_TLS_GD:
      // We always relax if -static because libc.a doesn't contain
      // __tls_get_addr().
      if (ctx.arg.static_ || (ctx.arg.relax && sym.is_tprel_linktime_const(ctx)))
        i++;
      else
        sym.flags |= NEEDS_TLSGD;
      break;
    case R_386_TLS_LDM:
      // We always relax if -static because libc.a doesn't contain
      // __tls_get_addr().
      if (ctx.arg.static_ || (ctx.arg.relax && !ctx.arg.shared))
        i++;
      else
        ctx.needs_tlsld = true;
      break;
    case R_386_TLS_GOTDESC:
      scan_tlsdesc(ctx, sym);
      break;
    case R_386_TLS_LE:
      check_tlsle(ctx, sym, rel);
      break;
    case R_386_32:
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

} // namespace mold

#endif
