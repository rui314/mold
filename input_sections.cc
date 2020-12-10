#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;

InputChunk::InputChunk(ObjectFile *file, const ElfShdr &shdr,
                       std::string_view name)
  : file(file), shdr(shdr), name(name),
    output_section(OutputSection::get_instance(name, shdr.sh_flags, shdr.sh_type)) {}

void InputSection::copy_buf() {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;

  // Copy data
  std::string_view data = file->obj.get_section_data(shdr);
  memcpy(out::buf + output_section->shdr.sh_offset + offset, &data[0], data.size());

  // Apply relocations
  u8 *base = out::buf + output_section->shdr.sh_offset + offset;
  u64 sh_addr = output_section->shdr.sh_addr + offset;

  for (int i = 0; i < rels.size(); i++) {
    const ELF64LE::Rela &rel = rels[i];
    StringPieceRef &ref = rel_pieces[i];

    Symbol &sym = *file->symbols[rel.getSymbol(false)];
    u8 *loc = base + rel.r_offset;

    if (!sym.file)
      continue;

#define S   (ref.piece ? ref.piece->get_addr() : sym.get_addr())
#define A   (ref.piece ? ref.addend : rel.r_addend)
#define P   (sh_addr + rel.r_offset)
#define L   sym.get_plt_addr()
#define G   sym.get_got_addr()
#define GOT out::got->shdr.sh_addr

    switch (rel.getType(false)) {
    case R_X86_64_NONE:
      break;
    case R_X86_64_64:
      *(u64 *)loc = S + A;
      break;
    case R_X86_64_PC32:
      *(u32 *)loc = S + A - P;
      break;
    case R_X86_64_GOT32:
      *(u64 *)loc = G - GOT + A;
      break;
    case R_X86_64_PLT32:
      if (sym.plt_idx == -1)
        *(u32 *)loc = S + A - P;
      else
        *(u32 *)loc = L + A - P;
      break;
    case R_X86_64_GOTPCREL:
      *(u32 *)loc = G + A - P;
      break;
    case R_X86_64_32:
    case R_X86_64_32S:
      *(u32 *)loc = S + A;
      break;
    case R_X86_64_16:
      *(u16 *)loc = S + A;
      break;
    case R_X86_64_PC16:
      *(u16 *)loc = S + A - P;
      break;
    case R_X86_64_8:
      *loc = S + A;
      break;
    case R_X86_64_PC8:
      *loc = S + A - P;
      break;
    case R_X86_64_TLSGD:
      if (sym.tlsgd_idx == -1) {
        // Relax GD to LE
        static const u8 insn[] = {
          0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
          0x48, 0x8d, 0x80, 0,    0,    0, 0,       // lea x@tpoff, %rax
        };
        memcpy(loc - 4, insn, sizeof(insn));
        *(u32 *)(loc + 8) = S - out::tls_end + A + 4;
        i++;
      } else {
        *(u32 *)loc = sym.get_tlsgd_addr() + A - P;
      }
      break;
    case R_X86_64_TLSLD:
      if (sym.tlsld_idx == -1) {
        // Relax LD to LE
        static const u8 insn[] = {
          0x66, 0x66, 0x66, 0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, // mov %fs:0, %rax
        };
        memcpy(loc - 3, insn, sizeof(insn));
        i++;
      } else {
        *(u32 *)loc = sym.get_tlsld_addr() + A - P;
      }
      break;
    case R_X86_64_DTPOFF32:
    case R_X86_64_TPOFF32:
      *(u32 *)loc = S - out::tls_end;
      break;
    case R_X86_64_DTPOFF64:
      *(u64 *)loc = S - out::tls_end;
      break;
    case R_X86_64_GOTTPOFF:
      *(u32 *)loc = sym.get_gottpoff_addr() + A - P;
      break;
    case R_X86_64_PC64:
      *(u64 *)loc = S + A - P;
      break;
    case R_X86_64_GOTPC32:
      *(u32 *)loc = GOT + A - P;
      break;
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      *(u32 *)loc = G + A - P;
      break;
    default:
      error(toString(this) + ": unknown relocation: " +
            std::to_string(rel.getType(false)));
    }

#undef S
#undef A
#undef P
#undef L
#undef G
#undef GOT
  }

  static Counter counter("relocs");
  counter.inc(rels.size());
}

void InputSection::scan_relocations() {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  for (int i = 0; i < rels.size(); i++) {
    const ELF64LE::Rela &rel = rels[i];
    Symbol &sym = *file->symbols[rel.getSymbol(false)];

    if (!sym.file || sym.is_placeholder) {
      file->has_error = true;
      continue;
    }

    switch (rel.getType(false)) {
    case R_X86_64_NONE:
      break;
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S:
    case R_X86_64_64:
    case R_X86_64_PC8:
    case R_X86_64_PC16:
    case R_X86_64_PC32:
    case R_X86_64_PC64:
      if (sym.is_imported) {
        if (sym.type == STT_OBJECT)
          sym.flags |= Symbol::NEEDS_COPYREL;
        else
          sym.flags |= Symbol::NEEDS_PLT;
      }
      break;
    case R_X86_64_GOT32:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      sym.flags |= Symbol::NEEDS_GOT;
      break;
    case R_X86_64_PLT32:
      if (sym.is_imported || sym.type == STT_GNU_IFUNC)
        sym.flags |= Symbol::NEEDS_PLT;
      break;
    case R_X86_64_TLSGD:
      assert(rels[i + 1].getType(false) == R_X86_64_PLT32);
      if (sym.is_imported)
        sym.flags |= Symbol::NEEDS_TLSGD;
      else
        i++;
      break;
    case R_X86_64_TLSLD:
      assert(rels[i + 1].getType(false) == R_X86_64_PLT32);
      if (sym.is_imported)
        sym.flags |= Symbol::NEEDS_TLSLD;
      else
        i++;
      break;
    case R_X86_64_TPOFF32:
    case R_X86_64_DTPOFF32:
    case R_X86_64_DTPOFF64:
      break;
    case R_X86_64_GOTTPOFF:
      sym.flags |= Symbol::NEEDS_GOTTPOFF;
      break;
    default:
      error(toString(this) + ": unknown relocation: " +
            std::to_string(rel.getType(false)));
    }
  }
}

void InputSection::report_undefined_symbols() {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  for (const ELF64LE::Rela &rel : rels) {
    Symbol &sym = *file->symbols[rel.getSymbol(false)];
    if (!sym.file || sym.is_placeholder)
      llvm::errs() << "undefined symbol: " << toString(file)
                   << ": " << sym.name << "\n";
  }
}

MergeableSection::MergeableSection(InputSection *isec, ArrayRef<u8> contents)
  : InputChunk(isec->file, isec->shdr, isec->name),
    parent(*MergedSection::get_instance(isec->name, isec->shdr.sh_flags,
                                        isec->shdr.sh_type)) {
  std::string_view data((const char *)&contents[0], contents.size());
  u32 offset = 0;

  while (!data.empty()) {
    size_t end = data.find('\0');
    if (end == std::string_view::npos)
      error(toString(this) + ": string is not null terminated");

    std::string_view substr = data.substr(0, end + 1);
    data = data.substr(end + 1);

    StringPiece *piece = parent.map.insert(substr, StringPiece(substr));
    pieces.push_back({piece, offset});
    offset += substr.size();
  }

  static Counter counter("string_pieces");
  counter.inc(pieces.size());
}

std::string toString(InputChunk *chunk) {
  return toString(chunk->file) + ":(" + std::string(chunk->name) + ")";
}
