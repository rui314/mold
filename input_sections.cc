#include "mold.h"

#include <limits>
#include <zlib.h>

static u64 read64be(u8 *buf) {
  return ((u64)buf[0] << 56) | ((u64)buf[1] << 48) |
         ((u64)buf[2] << 40) | ((u64)buf[3] << 32) |
         ((u64)buf[4] << 24) | ((u64)buf[5] << 16) |
         ((u64)buf[6] << 8)  | (u64)buf[7];
}

InputSection::InputSection(ObjectFile &file, const ElfShdr &shdr,
                           std::string_view name, i64 section_idx,
                           std::string_view contents,
                           OutputSection *osec)
  : file(file), shdr(shdr), name(name), section_idx(section_idx),
    contents(contents), output_section(osec) {}

InputSection *InputSection::create(ObjectFile &file, const ElfShdr *shdr,
                                   std::string_view name, i64 section_idx) {
  std::string_view contents;

  auto do_uncompress = [&](std::string_view data, u64 size) {
    u8 *buf = new u8[size];
    unsigned long size2 = size;
    if (uncompress(buf, &size2, (u8 *)&data[0], data.size()) != Z_OK)
      Fatal() << file << ": " << name << ": uncompress failed";
    if (size != size2)
      Fatal() << file << ": " << name << ": uncompress: invalid size";
    contents = {(char *)buf, size};

    ElfShdr *shdr2 = new ElfShdr;
    *shdr2 = *shdr;
    shdr2->sh_size = size;
    shdr2->sh_flags &= ~(u64)SHF_COMPRESSED;
    shdr = shdr2;
  };

  if (name.starts_with(".zdebug")) {
    // Old-style compressed section
    std::string_view data = file.get_string(*shdr);
    if (!data.starts_with("ZLIB") || data.size() <= 12)
      Fatal() << file << ": " << name << ": corrupted compressed section";
    u64 size = read64be((u8 *)&data[4]);

    // Rename .zdebug -> .debug
    name = *new std::string("." + std::string(name.substr(2)));
  } else if (shdr->sh_flags & SHF_COMPRESSED) {
    // New-style compressed section
    std::string_view data = file.get_string(*shdr);
    if (data.size() < sizeof(ElfChdr))
      Fatal() << file << ": " << name << ": corrupted compressed section";

    ElfChdr &hdr = *(ElfChdr *)&data[0];
    if (hdr.ch_type != ELFCOMPRESS_ZLIB)
      Fatal() << file << ": " << name << ": unsupported compression type";
    do_uncompress(data.substr(sizeof(ElfChdr)), hdr.ch_size);
  } else if (shdr->sh_type != SHT_NOBITS) {
    contents = file.get_string(*shdr);
  }

  OutputSection *osec =
    OutputSection::get_instance(name, shdr->sh_type, shdr->sh_flags);
  return new InputSection(file, *shdr, name, section_idx, contents, osec);
}

static std::string rel_to_string(u64 r_type) {
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
  unreachable();
}

__attribute__((always_inline))
static void overflow_check(InputSection *sec, Symbol &sym, u64 r_type, u64 val) {
  switch (r_type) {
  case R_X86_64_8:
    if (val != (u8)val)
      Error() << *sec << ": relocation R_X86_64_8 against " << sym
              << " out of range: " << val << " is not in [0, 255]";
    return;
  case R_X86_64_PC8:
    if (val != (i8)val)
      Error() << *sec << ": relocation R_X86_64_PC8 against " << sym
              << " out of range: " << (i64)val << " is not in [-128, 127]";
    return;
  case R_X86_64_16:
    if (val != (u16)val)
      Error() << *sec << ": relocation R_X86_64_16 against " << sym
              << " out of range: " << val << " is not in [0, 65535]";
    return;
  case R_X86_64_PC16:
    if (val != (i16)val)
      Error() << *sec << ": relocation R_X86_64_PC16 against " << sym
              << " out of range: " << (i64)val << " is not in [-32768, 32767]";
    return;
  case R_X86_64_32:
    if (val != (u32)val)
      Error() << *sec << ": relocation R_X86_64_32 against " << sym
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
  case R_X86_64_TLSDESC_CALL:
    if (val != (i32)val)
      Error() << *sec << ": relocation " << rel_to_string(r_type)
              << " against " << sym << " out of range: " << (i64)val
              << " is not in [-2147483648, 2147483647]";
    return;
  case R_X86_64_NONE:
  case R_X86_64_64:
  case R_X86_64_PC64:
  case R_X86_64_TPOFF64:
  case R_X86_64_DTPOFF64:
    return;
  }
  unreachable();
}

__attribute__((always_inline))
static void write_val(u64 r_type, u8 *loc, u64 val) {
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
  case R_X86_64_TLSDESC_CALL:
    *(u32 *)loc = val;
    return;
  case R_X86_64_64:
  case R_X86_64_PC64:
  case R_X86_64_TPOFF64:
  case R_X86_64_DTPOFF64:
    *(u64 *)loc = val;
    return;
  }
  unreachable();
}

void InputSection::copy_buf() {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;

  // Copy data
  u8 *base = out::buf + output_section->shdr.sh_offset + offset;
  memcpy(base, contents.data(), contents.size());

  // Apply relocations
  if (shdr.sh_flags & SHF_ALLOC)
    apply_reloc_alloc(base);
  else
    apply_reloc_nonalloc(base);
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
void InputSection::apply_reloc_alloc(u8 *base) {
  i64 ref_idx = 0;
  ElfRela *dynrel = nullptr;

  if (out::reldyn)
    dynrel = (ElfRela *)(out::buf + out::reldyn->shdr.sh_offset +
                         file.reldyn_offset + this->reldyn_offset);

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRela &rel = rels[i];
    Symbol &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const SectionFragmentRef *ref = nullptr;
    if (has_fragments[i])
      ref = &rel_fragments[ref_idx++];

    auto write = [&](u64 val) {
      overflow_check(this, sym, rel.r_type, val);
      write_val(rel.r_type, loc, val);
    };

#define S   (ref ? ref->frag->get_addr() : sym.get_addr())
#define A   (ref ? ref->addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define G   (sym.get_got_addr() - out::got->shdr.sh_addr)
#define GOT out::got->shdr.sh_addr

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
      write(sym.get_tlsgd_addr() + A - P);
      break;
    case R_TLSGD_RELAX_LE: {
      // Relax GD to LE
      static const u8 insn[] = {
        0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
        0x48, 0x8d, 0x80, 0,    0,    0, 0,       // lea 0(%rax), %rax
      };
      memcpy(loc - 4, insn, sizeof(insn));
      *(u32 *)(loc + 8) = S - out::tls_end + A + 4;
      i++;
      break;
    }
    case R_TLSLD:
      write(out::got->get_tlsld_addr() + A - P);
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
      write(S + A - out::tls_begin);
      break;
    case R_DTPOFF_RELAX:
      write(S + A - out::tls_end);
      break;
    case R_TPOFF:
      write(S + A - out::tls_end);
      break;
    case R_GOTTPOFF:
      write(sym.get_gottpoff_addr() + A - P);
      break;
    case R_GOTTPOFF_RELAX: {
      u32 insn = relax_gottpoff(loc - 3);
      loc[-3] = insn >> 16;
      loc[-2] = insn >> 8;
      loc[-1] = insn;
      write(S + A - out::tls_end + 4);
      break;
    }
    case R_GOTPC_TLSDESC:
      write(sym.get_tlsdesc_addr() + A - P);
      break;
    case R_GOTPC_TLSDESC_RELAX_LE: {
      static const u8 insn[] = {
        0x48, 0xc7, 0xc0, 0, 0, 0, 0, // mov $0, %rax
      };
      memcpy(loc - 3, insn, sizeof(insn));
      write(S + A - out::tls_end + 4);
      break;
    }
    case R_TLSDESC_CALL_RELAX:
      // call *(%rax) -> nop
      loc[0] = 0x66;
      loc[1] = 0x90;
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
void InputSection::apply_reloc_nonalloc(u8 *base) {
  static Counter counter("reloc_nonalloc");
  counter += rels.size();

  i64 ref_idx = 0;

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRela &rel = rels[i];
    Symbol &sym = *file.symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    if (!sym.file) {
      Error() << "undefined symbol: " << file << ": " << sym;
      continue;
    }

    const SectionFragmentRef *ref = nullptr;
    if (has_fragments[i])
      ref = &rel_fragments[ref_idx++];

    switch (rel.r_type) {
    case R_X86_64_NONE:
      break;
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S:
    case R_X86_64_64: {
      u64 val = ref ? ref->frag->get_addr() : sym.get_addr();
      overflow_check(this, sym, rel.r_type, val);
      write_val(rel.r_type, loc, val);
      break;
    }
    case R_X86_64_DTPOFF64:
      write_val(rel.r_type, loc, sym.get_addr() + rel.r_addend - out::tls_begin);
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
    case R_X86_64_DTPOFF32:
    case R_X86_64_TPOFF32:
    case R_X86_64_TPOFF64:
    case R_X86_64_GOTTPOFF:
      Fatal() << *this << ": invalid relocation for non-allocated sections: "
              << rel.r_type;
      break;
    default:
      Fatal() << *this << ": unknown relocation: " << rel.r_type;
    }
  }
}

static i64 get_output_type() {
  if (config.shared)
    return 0;
  if (config.pie)
    return 1;
  return 2;
}

static i64 get_sym_type(Symbol &sym) {
  if (sym.is_absolute())
    return 0;
  if (!sym.is_imported)
    return 1;
  if (sym.get_type() != STT_FUNC)
    return 2;
  return 3;
}

// Linker has to create data structures in an output file to apply
// some type of relocations. For example, if a relocation refers a GOT
// or a PLT entry of a symbol, linker has to create an entry in .got
// or in .plt for that symbol. In order to fix the file layout, we
// need to scan relocations.
void InputSection::scan_relocations() {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  static Counter counter("reloc_alloc");
  counter += rels.size();

  this->reldyn_offset = file.num_dynrel * sizeof(ElfRela);
  bool is_readonly = !(shdr.sh_flags & SHF_WRITE);

  // Scan relocations
  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRela &rel = rels[i];
    Symbol &sym = *file.symbols[rel.r_sym];
    u8 *loc = (u8 *)(contents.data() + rel.r_offset);

    if (!sym.file) {
      Error() << "undefined symbol: " << file << ": " << sym;
      continue;
    }

    typedef enum { NONE, ERROR, COPYREL, PLT, DYNREL, BASEREL } Action;

    auto dispatch = [&](Action action, RelType rel_type) {
      switch (action) {
      case NONE:
        rel_types[i] = rel_type;
        return;
      case ERROR:
        break;
      case COPYREL:
        if (!config.z_copyreloc)
          break;
        sym.flags |= NEEDS_COPYREL;
        rel_types[i] = rel_type;
        return;
      case PLT:
        sym.flags |= NEEDS_PLT;
        rel_types[i] = rel_type;
        return;
      case DYNREL:
        if (is_readonly)
          break;
        sym.flags |= NEEDS_DYNSYM;
        rel_types[i] = R_DYN;
        file.num_dynrel++;
        return;
      case BASEREL:
        if (is_readonly)
          break;
        rel_types[i] = R_BASEREL;
        file.num_dynrel++;
        return;
      default:
        unreachable();
      }

      Error() << *this << ": " << rel_to_string(rel.r_type)
              << " relocation against symbol `" << sym
              << "' can not be used; recompile with -fPIE";
    };

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

      dispatch(table[get_output_type()][get_sym_type(sym)], R_ABS);
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

      dispatch(table[get_output_type()][get_sym_type(sym)], R_ABS);
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

      dispatch(table[get_output_type()][get_sym_type(sym)], R_PC);
      break;
    }
    case R_X86_64_PC64: {
      Action table[][4] = {
        // Absolute  Local  Imported data  Imported code
        {  BASEREL,  NONE,  ERROR,         ERROR },      // DSO
        {  BASEREL,  NONE,  COPYREL,       PLT   },      // PIE
        {  NONE,     NONE,  COPYREL,       PLT   },      // PDE
      };

      dispatch(table[get_output_type()][get_sym_type(sym)], R_PC);
      break;
    }
    case R_X86_64_GOT32:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOT;
      break;
    case R_X86_64_GOTPC32:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOTPC;
      break;
    case R_X86_64_GOTPCREL:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOTPCREL;
      break;
    case R_X86_64_GOTPCRELX: {
      if (rel.r_addend != -4)
        Fatal() << *this << ": bad r_addend for R_X86_64_GOTPCRELX";

      if (config.relax && !sym.is_imported && sym.is_relative() &&
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
        Fatal() << *this << ": bad r_addend for R_X86_64_REX_GOTPCRELX";

      if (config.relax && !sym.is_imported && sym.is_relative() &&
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
        Fatal() << *this << ": TLSGD reloc must be followed by PLT32 or GOTPCREL";

      if (config.relax && !config.shared && !sym.is_imported) {
        rel_types[i++] = R_TLSGD_RELAX_LE;
      } else {
        sym.flags |= NEEDS_TLSGD;
        rel_types[i] = R_TLSGD;
      }
      break;
    }
    case R_X86_64_TLSLD:
      if (i + 1 == rels.size())
        Fatal() << *this << ": TLSGD reloc must be followed by PLT32 or GOTPCREL";
      if (sym.is_imported)
        Fatal() << *this << ": TLSLD reloc refers external symbol " << sym;

      if (config.relax && !config.shared) {
        rel_types[i++] = R_TLSLD_RELAX_LE;
      } else {
        sym.flags |= NEEDS_TLSLD;
        rel_types[i] = R_TLSLD;
      }
      break;
    case R_X86_64_DTPOFF32:
    case R_X86_64_DTPOFF64:
      if (sym.is_imported)
        Fatal() << *this << ": DTPOFF reloc refers external symbol " << sym;

      if (config.relax && !config.shared)
        rel_types[i] = R_DTPOFF_RELAX;
      else
        rel_types[i] = R_DTPOFF;
      break;
    case R_X86_64_TPOFF32:
    case R_X86_64_TPOFF64:
      rel_types[i] = R_TPOFF;
      break;
    case R_X86_64_GOTTPOFF:
      out::has_gottpoff = true;

      if (config.relax && !config.shared && relax_gottpoff(loc - 3)) {
        rel_types[i] = R_GOTTPOFF_RELAX;
      } else {
        sym.flags |= NEEDS_GOTTPOFF;
        rel_types[i] = R_GOTTPOFF;
      }
      break;
    case R_X86_64_GOTPC32_TLSDESC:
      if (memcmp(loc - 3, "\x48\x8d\x05", 3))
        Fatal() << *this << ": GOTPC32_TLSDESC relocation is used"
                << " against an invalid code sequence";

      if (config.relax && !config.shared) {
        rel_types[i] = R_GOTPC_TLSDESC_RELAX_LE;
      } else {
        sym.flags |= NEEDS_TLSDESC;
        rel_types[i] = R_GOTPC_TLSDESC;
      }
      break;
    case R_X86_64_TLSDESC_CALL:
      if (config.relax && !config.shared)
        rel_types[i] = R_TLSDESC_CALL_RELAX;
      else
        rel_types[i] = R_NONE;
      break;
    default:
      Fatal() << *this << ": unknown relocation: " << rel.r_type;
    }
  }
}

void InputSection::kill() {
  if (is_alive.exchange(false)) {
    is_alive = false;
    for (FdeRecord &fde : fdes)
      fde.is_alive = false;
    file.sections[section_idx] = nullptr;
  }
}
