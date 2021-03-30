#include "mold.h"

template <>
std::string rel_to_string(Context<X86_64> &ctx, u32 r_type) {
  switch (r_type) {
  case R_X86_64_NONE: return "R_X86_64_NONE";
  case R_X86_64_8: return "R_X86_64_8";
  case R_X86_64_16: return "R_X86_64_16";
  case R_X86_64_32: return "R_X86_64_32";
  case R_X86_64_32S: return "R_X86_64_32S";
  case R_X86_64_64: return "R_X86_64_64";
  case R_X86_64_PC8: return "R_X86_64_PC8";
  case R_X86_64_PC16: return "R_X86_64_PC16";
  case R_X86_64_PC32: return "R_X86_64_PC32";
  case R_X86_64_PC64: return "R_X86_64_PC64";
  case R_X86_64_GOT32: return "R_X86_64_GOT32";
  case R_X86_64_GOTPC32: return "R_X86_64_GOTPC32";
  case R_X86_64_GOT64: return "R_X86_64_GOT64";
  case R_X86_64_GOTPCREL64: return "R_X86_64_GOTPCREL64";
  case R_X86_64_GOTPC64: return "R_X86_64_GOTPC64";
  case R_X86_64_GOTPCREL: return "R_X86_64_GOTPCREL";
  case R_X86_64_GOTPCRELX: return "R_X86_64_GOTPCRELX";
  case R_X86_64_REX_GOTPCRELX: return "R_X86_64_REX_GOTPCRELX";
  case R_X86_64_PLT32: return "R_X86_64_PLT32";
  case R_X86_64_TLSGD: return "R_X86_64_TLSGD";
  case R_X86_64_TLSLD: return "R_X86_64_TLSLD";
  case R_X86_64_TPOFF32: return "R_X86_64_TPOFF32";
  case R_X86_64_DTPOFF32: return "R_X86_64_DTPOFF32";
  case R_X86_64_TPOFF64: return "R_X86_64_TPOFF64";
  case R_X86_64_DTPOFF64: return "R_X86_64_DTPOFF64";
  case R_X86_64_GOTTPOFF: return "R_X86_64_GOTTPOFF";
  }
  unreachable(ctx);
}

template <typename E>
static void overflow_check(Context<E> &ctx, InputSection<E> *sec,
                           Symbol<E> &sym, u64 r_type, u64 val) {
  switch (r_type) {
  case R_X86_64_8:
    if (val != (u8)val)
      Error(ctx) << *sec << ": relocation R_X86_64_8 against " << sym
                 << " out of range: " << val << " is not in [0, 255]";
    return;
  case R_X86_64_PC8:
    if (val != (i8)val)
      Error(ctx) << *sec << ": relocation R_X86_64_PC8 against " << sym
                 << " out of range: " << (i64)val << " is not in [-128, 127]";
    return;
  case R_X86_64_16:
    if (val != (u16)val)
      Error(ctx) << *sec << ": relocation R_X86_64_16 against " << sym
                 << " out of range: " << val << " is not in [0, 65535]";
    return;
  case R_X86_64_PC16:
    if (val != (i16)val)
      Error(ctx) << *sec << ": relocation R_X86_64_PC16 against " << sym
                 << " out of range: " << (i64)val << " is not in [-32768, 32767]";
    return;
  case R_X86_64_32:
    if (val != (u32)val)
      Error(ctx) << *sec << ": relocation R_X86_64_32 against " << sym
                 << " out of range: " << val << " is not in [0, 4294967296]";
    return;
  case R_X86_64_32S:
  case R_X86_64_PC32:
  case R_X86_64_GOT32:
  case R_X86_64_GOTPC32:
  case R_X86_64_GOTPCREL:
  case R_X86_64_GOTPCRELX:
  case R_X86_64_REX_GOTPCRELX:
  case R_X86_64_PLT32:
  case R_X86_64_TLSGD:
  case R_X86_64_TLSLD:
  case R_X86_64_TPOFF32:
  case R_X86_64_DTPOFF32:
  case R_X86_64_GOTTPOFF:
  case R_X86_64_GOTPC32_TLSDESC:
  case R_X86_64_SIZE32:
  case R_X86_64_TLSDESC_CALL:
    if (val != (i32)val)
      Error(ctx) << *sec << ": relocation " << rel_to_string(ctx, r_type)
                 << " against " << sym << " out of range: " << (i64)val
                 << " is not in [-2147483648, 2147483647]";
    return;
  case R_X86_64_NONE:
  case R_X86_64_64:
  case R_X86_64_PC64:
  case R_X86_64_TPOFF64:
  case R_X86_64_DTPOFF64:
  case R_X86_64_GOT64:
  case R_X86_64_GOTPCREL64:
  case R_X86_64_GOTPC64:
  case R_X86_64_SIZE64:
    return;
  }
  unreachable(ctx);
}

static void write_val(Context<X86_64> &ctx, u64 r_type, u8 *loc, u64 val) {
  switch (r_type) {
  case R_X86_64_NONE:
    return;
  case R_X86_64_8:
  case R_X86_64_PC8:
    *loc = val;
    return;
  case R_X86_64_16:
  case R_X86_64_PC16:
    *(u16 *)loc = val;
    return;
  case R_X86_64_32:
  case R_X86_64_32S:
  case R_X86_64_PC32:
  case R_X86_64_GOT32:
  case R_X86_64_GOTPC32:
  case R_X86_64_GOTPCREL:
  case R_X86_64_GOTPCRELX:
  case R_X86_64_REX_GOTPCRELX:
  case R_X86_64_PLT32:
  case R_X86_64_TLSGD:
  case R_X86_64_TLSLD:
  case R_X86_64_TPOFF32:
  case R_X86_64_DTPOFF32:
  case R_X86_64_GOTTPOFF:
  case R_X86_64_GOTPC32_TLSDESC:
  case R_X86_64_SIZE32:
  case R_X86_64_TLSDESC_CALL:
    *(u32 *)loc = val;
    return;
  case R_X86_64_64:
  case R_X86_64_PC64:
  case R_X86_64_TPOFF64:
  case R_X86_64_DTPOFF64:
  case R_X86_64_GOT64:
  case R_X86_64_GOTPCREL64:
  case R_X86_64_GOTPC64:
  case R_X86_64_SIZE64:
    *(u64 *)loc = val;
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
  i64 ref_idx = 0;
  ElfRel<X86_64> *dynrel = nullptr;

  if (ctx.reldyn)
    dynrel = (ElfRel<X86_64> *)(ctx.buf + ctx.reldyn->shdr.sh_offset +
                         file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<X86_64> &rel = rels[i];
    Symbol<X86_64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef<X86_64> *ref = nullptr;
    if (has_fragments[i])
      ref = &rel_fragments[ref_idx++];

    auto write = [&](u64 val) {
      overflow_check(ctx, this, sym, rel.r_type, val);
      write_val(ctx, rel.r_type, loc, val);
    };

#define S   (ref ? ref->frag->get_addr(ctx) : sym.get_addr(ctx))
#define A   (ref ? ref->addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr(ctx) - ctx.got->shdr.sh_addr)
#define GOT ctx.got->shdr.sh_addr

    switch (rel_types[i]) {
    case R_NONE:
      break;
    case R_ABS:
      write(S + A);
      break;
    case R_BASEREL:
      *dynrel++ = {P, R_X86_64_RELATIVE, 0, (i64)(S + A)};
      break;
    case R_DYN:
      *dynrel++ = {P, R_X86_64_64, sym.dynsym_idx, A};
      break;
    case R_PC:
      write(S + A - P);
      break;
    case R_GOT:
      write(G + A);
      break;
    case R_GOTPC:
      write(GOT + A - P);
      break;
    case R_GOTPCREL:
      write(G + GOT + A - P);
      break;
    case R_GOTPCRELX_RELAX: {
      u32 insn = relax_gotpcrelx(loc - 2);
      loc[-2] = insn >> 8;
      loc[-1] = insn;
      write(S + A - P);
      break;
    }
    case R_REX_GOTPCRELX_RELAX: {
      u32 insn = relax_rex_gotpcrelx(loc - 3);
      loc[-3] = insn >> 16;
      loc[-2] = insn >> 8;
      loc[-1] = insn;
      write(S + A - P);
      break;
    }
    case R_TLSGD:
      write(sym.get_tlsgd_addr(ctx) + A - P);
      break;
    case R_TLSGD_RELAX_LE: {
      // Relax GD to LE
      static const u8 insn[] = {
        0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
        0x48, 0x8d, 0x80, 0,    0,    0, 0,       // lea 0(%rax), %rax
      };
      memcpy(loc - 4, insn, sizeof(insn));
      *(u32 *)(loc + 8) = S - ctx.tls_end + A + 4;
      i++;
      break;
    }
    case R_TLSLD:
      write(ctx.got->get_tlsld_addr(ctx) + A - P);
      break;
    case R_TLSLD_RELAX_LE: {
      // Relax LD to LE
      static const u8 insn[] = {
        0x66, 0x66, 0x66,                         // (padding)
        0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
      };
      memcpy(loc - 3, insn, sizeof(insn));
      i++;
      break;
    }
    case R_DTPOFF:
      write(S + A - ctx.tls_begin);
      break;
    case R_DTPOFF_RELAX:
      write(S + A - ctx.tls_end);
      break;
    case R_TPOFF:
      write(S + A - ctx.tls_end);
      break;
    case R_GOTTPOFF:
      write(sym.get_gottpoff_addr(ctx) + A - P);
      break;
    case R_GOTTPOFF_RELAX: {
      u32 insn = relax_gottpoff(loc - 3);
      loc[-3] = insn >> 16;
      loc[-2] = insn >> 8;
      loc[-1] = insn;
      write(S + A - ctx.tls_end + 4);
      break;
    }
    case R_GOTPC_TLSDESC:
      write(sym.get_tlsdesc_addr(ctx) + A - P);
      break;
    case R_GOTPC_TLSDESC_RELAX_LE: {
      static const u8 insn[] = {
        0x48, 0xc7, 0xc0, 0, 0, 0, 0, // mov $0, %rax
      };
      memcpy(loc - 3, insn, sizeof(insn));
      write(S + A - ctx.tls_end + 4);
      break;
    }
    case R_SIZE:
      write(sym.esym->st_size + A);
      break;
    case R_TLSDESC_CALL_RELAX:
      // call *(%rax) -> nop
      loc[0] = 0x66;
      loc[1] = 0x90;
      break;
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
  static Counter counter("reloc_nonalloc");
  counter += rels.size();

  i64 ref_idx = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<X86_64> &rel = rels[i];
    Symbol<X86_64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      Error(ctx) << "undefined symbol: " << file << ": " << sym;
      continue;
    }

    const SectionFragmentRef<X86_64> *ref = nullptr;
    if (has_fragments[i])
      ref = &rel_fragments[ref_idx++];

    auto write = [&](u64 val) {
      overflow_check(ctx, this, sym, rel.r_type, val);
      write_val(ctx, rel.r_type, loc, val);
    };

    switch (rel.r_type) {
    case R_X86_64_NONE:
      break;
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S:
    case R_X86_64_64:
      if (ref)
        write(ref->frag->get_addr(ctx) + ref->addend);
      else
        write(sym.get_addr(ctx) + rel.r_addend);
      break;
    case R_X86_64_DTPOFF32:
    case R_X86_64_DTPOFF64:
      write(sym.get_addr(ctx) + rel.r_addend - ctx.tls_begin);
      break;
    case R_X86_64_SIZE32:
    case R_X86_64_SIZE64:
      write(sym.esym->st_size + rel.r_addend);
      break;
    case R_X86_64_PC8:
    case R_X86_64_PC16:
    case R_X86_64_PC32:
    case R_X86_64_PC64:
    case R_X86_64_GOT32:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
    case R_X86_64_PLT32:
    case R_X86_64_TLSGD:
    case R_X86_64_TLSLD:
    case R_X86_64_TPOFF32:
    case R_X86_64_TPOFF64:
    case R_X86_64_GOTTPOFF:
    case R_X86_64_GOT64:
    case R_X86_64_GOTPCREL64:
    case R_X86_64_GOTPC64:
    case R_X86_64_GOTPC32_TLSDESC:
      Fatal(ctx) << *this << ": invalid relocation for non-allocated sections: "
                 << rel.r_type;
      break;
    default:
      Fatal(ctx) << *this << ": unknown relocation: " << rel.r_type;
    }
  }
}

// Linker has to create data structures in an output file to apply
// some type of relocations. For example, if a relocation refers a GOT
// or a PLT entry of a symbol, linker has to create an entry in .got
// or in .plt for that symbol. In order to fix the file layout, we
// need to scan relocations.
template <>
void InputSection<X86_64>::scan_relocations(Context<X86_64> &ctx) {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  static Counter counter("reloc_alloc");
  counter += rels.size();

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRel<X86_64>);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<X86_64> &rel = rels[i];
    Symbol<X86_64> &sym = *file.symbols[rel.r_sym];
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (!sym.file) {
      Error(ctx) << "undefined symbol: " << file << ": " << sym;
      continue;
    }

    if (sym.esym->st_type == STT_GNU_IFUNC)
      sym.flags |= NEEDS_PLT;

    switch (rel.r_type) {
    case R_X86_64_NONE:
      rel_types[i] = R_NONE;
      break;
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

      dispatch(ctx, table, R_ABS, i);
      break;
    }
    case R_X86_64_64: {
      // Unlike the above, we can use R_X86_64_RELATIVE and R_86_64_64
      // relocations.
      Action table[][4] = {
        // Absolute  Local    Imported data  Imported code
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // DSO
        {  NONE,     BASEREL, DYNREL,        DYNREL },     // PIE
        {  NONE,     NONE,    COPYREL,       PLT    },     // PDE
      };

      dispatch(ctx, table, R_ABS, i);
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

      dispatch(ctx, table, R_PC, i);
      break;
    }
    case R_X86_64_PC64: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  BASEREL,  NONE,  ERROR,         ERROR },      // DSO
        {  BASEREL,  NONE,  COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };

      dispatch(ctx, table, R_PC, i);
      break;
    }
    case R_X86_64_GOT32:
    case R_X86_64_GOT64:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOT;
      break;
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPC64:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOTPC;
      break;
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCREL64:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOTPCREL;
      break;
    case R_X86_64_GOTPCRELX: {
      if (rel.r_addend != -4)
        Fatal(ctx) << *this << ": bad r_addend for R_X86_64_GOTPCRELX";

      if (ctx.arg.relax && !sym.is_imported && sym.is_relative(ctx) &&
          relax_gotpcrelx(loc - 2)) {
        rel_types[i] = R_GOTPCRELX_RELAX;
      } else {
        sym.flags |= NEEDS_GOT;
        rel_types[i] = R_GOTPCREL;
      }
      break;
    }
    case R_X86_64_REX_GOTPCRELX:
      if (rel.r_addend != -4)
        Fatal(ctx) << *this << ": bad r_addend for R_X86_64_REX_GOTPCRELX";

      if (ctx.arg.relax && !sym.is_imported && sym.is_relative(ctx) &&
          relax_rex_gotpcrelx(loc - 3)) {
        rel_types[i] = R_REX_GOTPCRELX_RELAX;
      } else {
        sym.flags |= NEEDS_GOT;
        rel_types[i] = R_GOTPCREL;
      }
      break;
    case R_X86_64_PLT32:
      if (sym.is_imported)
        sym.flags |= NEEDS_PLT;
      rel_types[i] = R_PC;
      break;
    case R_X86_64_TLSGD: {
      if (i + 1 == rels.size())
        Fatal(ctx) << *this
                   << ": TLSGD reloc must be followed by PLT32 or GOTPCREL";

      if (ctx.arg.relax && !ctx.arg.shared && !sym.is_imported) {
        rel_types[i++] = R_TLSGD_RELAX_LE;
      } else {
        sym.flags |= NEEDS_TLSGD;
        rel_types[i] = R_TLSGD;
      }
      break;
    }
    case R_X86_64_TLSLD:
      if (i + 1 == rels.size())
        Fatal(ctx) << *this
                   << ": TLSGD reloc must be followed by PLT32 or GOTPCREL";
      if (sym.is_imported)
        Fatal(ctx) << *this << ": TLSLD reloc refers external symbol " << sym;

      if (ctx.arg.relax && !ctx.arg.shared) {
        rel_types[i++] = R_TLSLD_RELAX_LE;
      } else {
        sym.flags |= NEEDS_TLSLD;
        rel_types[i] = R_TLSLD;
      }
      break;
    case R_X86_64_DTPOFF32:
    case R_X86_64_DTPOFF64:
      if (sym.is_imported)
        Fatal(ctx) << *this << ": DTPOFF reloc refers external symbol " << sym;

      if (ctx.arg.relax && !ctx.arg.shared)
        rel_types[i] = R_DTPOFF_RELAX;
      else
        rel_types[i] = R_DTPOFF;
      break;
    case R_X86_64_TPOFF32:
    case R_X86_64_TPOFF64:
      rel_types[i] = R_TPOFF;
      break;
    case R_X86_64_GOTTPOFF:
      ctx.has_gottpoff = true;

      if (ctx.arg.relax && !ctx.arg.shared && !sym.is_imported &&
          relax_gottpoff(loc - 3)) {
        rel_types[i] = R_GOTTPOFF_RELAX;
      } else {
        sym.flags |= NEEDS_GOTTPOFF;
        rel_types[i] = R_GOTTPOFF;
      }
      break;
    case R_X86_64_GOTPC32_TLSDESC:
      if (memcmp(loc - 3, "\x48\x8d\x05", 3))
        Fatal(ctx) << *this << ": GOTPC32_TLSDESC relocation is used"
                << " against an invalid code sequence";

      if (ctx.arg.relax && !ctx.arg.shared) {
        rel_types[i] = R_GOTPC_TLSDESC_RELAX_LE;
      } else {
        sym.flags |= NEEDS_TLSDESC;
        rel_types[i] = R_GOTPC_TLSDESC;
      }
      break;
    case R_X86_64_SIZE32:
    case R_X86_64_SIZE64:
      rel_types[i] = R_SIZE;
      break;
    case R_X86_64_TLSDESC_CALL:
      if (ctx.arg.relax && !ctx.arg.shared)
        rel_types[i] = R_TLSDESC_CALL_RELAX;
      else
        rel_types[i] = R_NONE;
      break;
    default:
      Fatal(ctx) << *this << ": unknown relocation: " << rel.r_type;
    }
  }
}
