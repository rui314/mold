#include "mold.h"

InputChunk::InputChunk(ObjectFile *file, const ElfShdr &shdr,
                       std::string_view name)
  : file(file), shdr(shdr), name(name),
    output_section(OutputSection::get_instance(name, shdr.sh_type, shdr.sh_flags)) {}

static int get_rel_size(u32 r_type) {
  switch (r_type) {
  case R_X86_64_NONE:
    return 0;
  case R_X86_64_8:
  case R_X86_64_PC8:
    return 1;
  case R_X86_64_16:
  case R_X86_64_PC16:
    return 2;
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
    return 4;
  case R_X86_64_64:
  case R_X86_64_PC64:
  case R_X86_64_TPOFF64:
  case R_X86_64_DTPOFF64:
    return 8;
  }
  unreachable();
}

void InputSection::copy_buf() {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;

  // Copy data
  std::string_view view = file->get_string(shdr);
  memcpy(out::buf + output_section->shdr.sh_offset + offset, view.data(), view.size());

  // Apply relocations
  u8 *base = out::buf + output_section->shdr.sh_offset + offset;

  ElfRela *dynrel = nullptr;
  if (out::reldyn)
    dynrel = (ElfRela *)(out::buf + out::reldyn->shdr.sh_offset +
                         file->reldyn_offset + reldyn_offset);

  for (int i = 0; i < rels.size(); i++) {
    const ElfRela &rel = rels[i];
    StringPieceRef &ref = rel_pieces[i];
    Symbol &sym = *file->symbols[rel.r_sym];
    u8 *loc = base + rel.r_offset;

    auto write = [&](u64 val) {
      switch (get_rel_size(rel.r_type)) {
      case 1: *loc = val; return;
      case 2: *(u16 *)loc = val; return;
      case 4: *(u32 *)loc = val; return;
      case 8: *(u64 *)loc = val; return;
      }
      unreachable();
    };

#define S   (ref.piece ? ref.piece->get_addr() : sym.get_addr())
#define A   (ref.piece ? ref.addend : rel.r_addend)
#define P   (output_section->shdr.sh_addr + offset + rel.r_offset)
#define L   sym.get_plt_addr()
#define G   (sym.get_got_addr() - out::got->shdr.sh_addr)
#define GOT out::got->shdr.sh_addr

    switch (rel_types[i]) {
    case R_NONE:
      break;
    case R_ABS:
      write(S + A);

      if (sym.needs_relative_rel()) {
        assert(get_rel_size(rel.r_type) == 8);
        memset(dynrel, 0, sizeof(*dynrel));
        dynrel->r_offset = P;
        dynrel->r_type = R_X86_64_RELATIVE;
        dynrel->r_addend = S + A;
        dynrel++;
      }
      break;
    case R_DYN:
      memset(dynrel, 0, sizeof(*dynrel));
      dynrel->r_offset = P;
      dynrel->r_type = R_X86_64_64;
      dynrel->r_sym = sym.dynsym_idx;
      dynrel->r_addend = A;
      dynrel++;
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
    case R_PLT:
      write(L + A - P);
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
        0x66, 0x66, 0x66, 0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
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
#undef L
#undef G
#undef GOT
  }
}

static std::string rel_to_string(u32 r_type) {
  switch (r_type) {
  case R_X86_64_8:
    return "R_X86_64_8";
  case R_X86_64_16:
    return "R_X86_64_16";
  case R_X86_64_32:
    return "R_X86_64_32";
  case R_X86_64_32S:
    return "R_X86_64_32S";
  }
  unreachable();
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

    if (!sym.file || sym.is_placeholder) {
      file->has_error = true;
      continue;
    }

    switch (rel.r_type) {
    case R_X86_64_NONE:
      rel_types[i] = R_NONE;
      break;
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S:
      if (config.pie)
        error(to_string(this) + ": " + rel_to_string(rel.r_type) +
              " relocation against symbol `" + std::string(sym.name) +
              "' can not be used when making a PIE object; recompile with -fPIE");

      if (!sym.is_imported) {
        rel_types[i] = R_ABS;
      } else if (sym.type == STT_OBJECT) {
        rel_types[i] = R_ABS;
        sym.flags |= NEEDS_COPYREL;
      } else {
        rel_types[i] = R_PLT;
        sym.flags |= NEEDS_PLT;
      }
      break;
    case R_X86_64_64:
      if (sym.is_imported) {
        rel_types[i] = R_DYN;
        sym.flags |= NEEDS_DYNSYM;
        file->num_dynrel++;
      } else {
        rel_types[i] = R_ABS;
        if (sym.needs_relative_rel())
          file->num_dynrel++;
      }
      break;
    case R_X86_64_PC8:
    case R_X86_64_PC16:
    case R_X86_64_PC32:
    case R_X86_64_PC64:
      if (!sym.is_imported) {
        rel_types[i] = R_PC;
      } else if (sym.type == STT_OBJECT) {
        rel_types[i] = R_PC;
        sym.flags |= NEEDS_COPYREL;
      } else {
        rel_types[i] = R_PLT;
        sym.flags |= NEEDS_PLT;
      }
      break;
    case R_X86_64_GOT32:
      rel_types[i] = R_GOT;
      sym.flags |= NEEDS_GOT;
      break;
    case R_X86_64_GOTPC32:
      rel_types[i] = R_GOTPC;
      sym.flags |= NEEDS_GOT;
      break;
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      rel_types[i] = R_GOTPCREL;
      sym.flags |= NEEDS_GOT;
      break;
    case R_X86_64_PLT32:
      if (sym.is_imported || sym.type == STT_GNU_IFUNC) {
        rel_types[i] = R_PLT;
        sym.flags |= NEEDS_PLT;
      } else {
        rel_types[i] = R_PC;
      }
      break;
    case R_X86_64_TLSGD:
      if (rels[i + 1].r_type != R_X86_64_PLT32)
        error(to_string(this) + ": TLSGD reloc not followed by PLT32");

      if (sym.is_imported) {
        rel_types[i] = R_TLSGD;
        sym.flags |= NEEDS_TLSGD;
      } else {
        rel_types[i] = R_TLSGD_RELAX_LE;
        i++;
      }
      break;
    case R_X86_64_TLSLD:
      if (rels[i + 1].r_type != R_X86_64_PLT32)
        error(to_string(this) + ": TLSLD reloc not followed by PLT32");

      if (sym.is_imported) {
        rel_types[i] = R_TLSLD;
        sym.flags |= NEEDS_TLSLD;
      } else {
        rel_types[i] = R_TLSLD_RELAX_LE;
        i++;
      }
      break;
    case R_X86_64_TPOFF32:
    case R_X86_64_DTPOFF32:
    case R_X86_64_TPOFF64:
    case R_X86_64_DTPOFF64:
      rel_types[i] = R_TPOFF;
      break;
    case R_X86_64_GOTTPOFF:
      rel_types[i] = R_GOTTPOFF;
      sym.flags |= NEEDS_GOTTPOFF;
      break;
    default:
      error(to_string(this) + ": unknown relocation: " + std::to_string(rel.r_type));
    }
  }
}

void InputSection::report_undefined_symbols() {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  for (const ElfRela &rel : rels) {
    Symbol &sym = *file->symbols[rel.r_sym];
    if (!sym.file || sym.is_placeholder)
      std::cerr << "undefined symbol: " << to_string(file)
                << ": " << sym.name << "\n";
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
      error(to_string(this) + ": string is not null terminated");

    std::string_view substr = data.substr(0, end + 1);
    data = data.substr(end + 1);

    StringPiece *piece = parent.map.insert(substr, StringPiece(substr));
    pieces.push_back({piece, offset});
    offset += substr.size();
  }

  static Counter counter("string_pieces");
  counter.inc(pieces.size());
}

std::string to_string(InputChunk *chunk) {
  return to_string(chunk->file) + ":(" + std::string(chunk->name) + ")";
}
