#include "mold.h"

InputChunk::InputChunk(ObjectFile *file, const ElfShdr &shdr,
                       std::string_view name)
  : file(file), shdr(shdr), name(name),
    output_section(OutputSection::get_instance(name, shdr.sh_type, shdr.sh_flags)) {}

static std::string rel_to_string(u32 r_type) {
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

static void overflow_check(InputSection *sec, Symbol &sym, u32 r_type, u8 *loc, u64 val) {
  auto out_of_range = [&](std::string range) {
    Error() << *sec << ": relocation " << rel_to_string(r_type)
            << " against " << sym.name << " out of range: "
            << val << " is not in [" << range << "]";
  };

  switch (r_type) {
  case R_X86_64_8:
    if (val != (u8)val)
      out_of_range("0, 255");
    return;
  case R_X86_64_PC8:
    if (val != (i8)val)
      out_of_range("-128, 127");
    return;
  case R_X86_64_16:
    if (val != (u16)val)
      out_of_range("0, 65535");
    return;
  case R_X86_64_PC16:
    if (val != (i16)val)
      out_of_range("-32768, 32767");
    return;
  case R_X86_64_32:
    if (val != (u32)val)
      out_of_range("0, 4294967296");
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
    if (val != (i32)val)
      out_of_range("-2147483648, 2147483647");
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

static void write_val(u32 r_type, u8 *loc, u64 val) {
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

  u8 *base = out::buf + output_section->shdr.sh_offset + offset;

  // Copy data
  std::string_view contents = file->get_string(shdr);
  memcpy(base, contents.data(), contents.size());

  // Apply relocations
  int ref_idx = 0;
  ElfRela *dynrel = nullptr;

  if (out::reldyn)
    dynrel = (ElfRela *)(out::buf + out::reldyn->shdr.sh_offset +
                         file->reldyn_offset + reldyn_offset);

  for (int i = 0; i < rels.size(); i++) {
    const ElfRela &rel = rels[i];
    Symbol &sym = *file->symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    const StringPieceRef *ref = nullptr;
    if (has_rel_piece[i])
      ref = &rel_pieces[ref_idx++];

    auto write = [&](u64 val) {
      overflow_check(this, sym, rel.r_type, loc, val);
      write_val(rel.r_type, loc, val);
    };

#define S   (ref ? ref->piece->get_addr() \
             : (sym.plt_idx == -1 ? sym.get_addr() : sym.get_plt_addr()))
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
    case R_ABS_DYN:
      write(S + A);
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
    case R_TLSGD:
      write(sym.get_tlsgd_addr() + A - P);
      break;
    case R_TLSGD_RELAX_LE: {
      // Relax GD to LE
      static const u8 insn[] = {
        0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
        0x48, 0x8d, 0x80, 0,    0,    0, 0,       // lea x@tpoff, %rax
      };
      memcpy(loc - 4, insn, sizeof(insn));
      *(u32 *)(loc + 8) = S - out::tls_end + A + 4;
      i++;
      break;
    }
    case R_TLSLD:
      write(sym.get_tlsld_addr() + A - P);
      break;
    case R_TLSLD_RELAX_LE: {
      // Relax LD to LE
      static const u8 insn[] = {
        // mov %fs:0, %rax
        0x66, 0x66, 0x66, 0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0,
      };
      memcpy(loc - 3, insn, sizeof(insn));
      i++;
      break;
    }
    case R_TPOFF:
      write(S + A - out::tls_end);
      break;
    case R_GOTTPOFF:
      write(sym.get_gottpoff_addr() + A - P);
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

void InputSection::scan_relocations() {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  reldyn_offset = file->num_dynrel * sizeof(ElfRela);

  static Counter counter("relocs");
  counter.inc(rels.size());

  for (int i = 0; i < rels.size(); i++) {
    const ElfRela &rel = rels[i];
    Symbol &sym = *file->symbols[rel.r_sym];
    bool is_readonly = !(shdr.sh_flags & SHF_WRITE);
    bool is_code = !(sym.st_type == STT_OBJECT);

    if (!sym.file || sym.is_placeholder) {
      Error() << "undefined symbol: " << *file << ": " << sym.name;
      continue;
    }

    auto report_error = [&]() {
      Error() << *this << ": " << rel_to_string(rel.r_type)
              << " relocation against symbol `" << sym.name
              << "' can not be used; recompile with -fPIE";
    };

    switch (rel.r_type) {
    case R_X86_64_NONE:
      rel_types[i] = R_NONE;
      break;
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S:
      if (config.pie && sym.is_relative())
        report_error();
      if (sym.is_imported)
        sym.flags |= is_code ? NEEDS_PLT : NEEDS_COPYREL;
      rel_types[i] = R_ABS;
      break;
    case R_X86_64_64:
      if (config.pie) {
        if (sym.is_imported) {
          if (is_readonly)
            report_error();
          sym.flags |= NEEDS_DYNSYM;
          rel_types[i] = R_DYN;
          file->num_dynrel++;
        } else if (sym.is_relative()) {
          if (is_readonly)
            report_error();
          rel_types[i] = R_ABS_DYN;
          file->num_dynrel++;
        } else {
          rel_types[i] = R_ABS;
        }
      } else {
        if (sym.is_imported)
          sym.flags |= is_code ? NEEDS_PLT : NEEDS_COPYREL;
        rel_types[i] = R_ABS;
      }
      break;
    case R_X86_64_PC8:
    case R_X86_64_PC16:
    case R_X86_64_PC32:
    case R_X86_64_PC64:
      if (sym.is_imported)
        sym.flags |= is_code ? NEEDS_PLT : NEEDS_COPYREL;
      rel_types[i] = R_PC;
      break;
    case R_X86_64_GOT32:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOT;
      break;
    case R_X86_64_GOTPC32:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOTPC;
      break;
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      sym.flags |= NEEDS_GOT;
      rel_types[i] = R_GOTPCREL;
      break;
    case R_X86_64_PLT32:
      if (sym.is_imported || sym.st_type == STT_GNU_IFUNC)
        sym.flags |= NEEDS_PLT;
      rel_types[i] = R_PC;
      break;
    case R_X86_64_TLSGD:
      if (i + 1 == rels.size() || rels[i + 1].r_type != R_X86_64_PLT32)
        Error() << *this << ": TLSGD reloc not followed by PLT32";

      if (sym.is_imported || !config.relax) {
        sym.flags |= NEEDS_TLSGD;
        sym.flags |= NEEDS_DYNSYM;
        rel_types[i] = R_TLSGD;
      } else {
        rel_types[i] = R_TLSGD_RELAX_LE;
        i++;
      }
      break;
    case R_X86_64_TLSLD:
      if (i + 1 == rels.size() || rels[i + 1].r_type != R_X86_64_PLT32)
        Error() << *this << ": TLSLD reloc not followed by PLT32";
      if (sym.is_imported)
        Error() << *this << ": TLSLD reloc refers external symbol " << sym.name;

      if (config.relax) {
        rel_types[i] = R_TLSLD_RELAX_LE;
        i++;
      } else {
        sym.flags |= NEEDS_TLSLD;
        sym.flags |= NEEDS_DYNSYM;
        rel_types[i] = R_TLSLD;
      }
      break;
    case R_X86_64_TPOFF32:
    case R_X86_64_DTPOFF32:
    case R_X86_64_TPOFF64:
    case R_X86_64_DTPOFF64:
      rel_types[i] = R_TPOFF;
      break;
    case R_X86_64_GOTTPOFF:
      sym.flags |= NEEDS_GOTTPOFF;
      rel_types[i] = R_GOTTPOFF;
      break;
    default:
      Error() << *this << ": unknown relocation: " << rel.r_type;
    }
  }
}

MergeableSection::MergeableSection(InputSection *isec, std::string_view data)
  : InputChunk(isec->file, isec->shdr, isec->name),
    parent(*MergedSection::get_instance(isec->name, isec->shdr.sh_type,
                                        isec->shdr.sh_flags)) {
  u32 offset = 0;

  while (!data.empty()) {
    size_t end = data.find('\0');
    if (end == std::string_view::npos)
      Error() << *this << ": string is not null terminated";

    std::string_view substr = data.substr(0, end + 1);
    data = data.substr(end + 1);

    StringPiece *piece = parent.map.insert(substr, StringPiece(substr));
    pieces.push_back(piece);
    piece_offsets.push_back(offset);
    offset += substr.size();
  }

  static Counter counter("string_pieces");
  counter.inc(pieces.size());
}
