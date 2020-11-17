#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;

InputChunk::InputChunk(Kind kind, ObjectFile *file, const ELF64LE::Shdr &shdr,
                       StringRef name)
  : kind(kind), file(file), shdr(shdr), name(name) {
  output_section = OutputSection::get_instance(name, shdr.sh_flags, shdr.sh_type);

  u64 align = (shdr.sh_addralign == 0) ? 1 : shdr.sh_addralign;
  if (align > UINT32_MAX)
    error(toString(file) + ": section sh_addralign is too large");
  if (__builtin_popcount(align) != 1)
    error(toString(file) + ": section sh_addralign is not a power of two");
}

void InputSection::copy_buf() {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;

  // Copy data
  ArrayRef<u8> data = check(file->obj.getSectionContents(shdr));
  memcpy(out::buf + output_section->shdr.sh_offset + offset, &data[0], data.size());

  // Apply relocations
  u8 *base = out::buf + output_section->shdr.sh_offset + offset;
  u64 sh_addr = output_section->shdr.sh_addr + offset;
  u64 GOT = out::got->shdr.sh_addr;

  for (int i = 0; i < rels.size(); i++) {
    const ELF64LE::Rela &rel = rels[i];
    StringPieceRef &ref = rel_pieces[i];

    Symbol &sym = *file->symbols[rel.getSymbol(false)];
    u8 *loc = base + rel.r_offset;

    u64 S = ref.piece ? ref.piece->get_addr() : sym.get_addr();
    u64 A = ref.piece ? ref.addend : rel.r_addend;
    u64 P = sh_addr + rel.r_offset;

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
      *(u64 *)loc = sym.get_got_addr() - GOT + A;
      break;
    case R_X86_64_PLT32:
      if (sym.type == STT_GNU_IFUNC || (!config.is_static && sym.file->is_dso))
        *(u32 *)loc = sym.get_plt_addr() + A - P;
      else
        *(u32 *)loc = S + A - P; // todo
      break;
    case R_X86_64_GOTPCREL:
      *(u32 *)loc = sym.get_got_addr() + A - P;
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
    case R_X86_64_TLSLD:
      // TODO
      break;
    case R_X86_64_GOTTPOFF:
      *(u32 *)loc = sym.get_gottp_addr() + A - P;
      break;
    case R_X86_64_TPOFF32:
      *(u32 *)loc = S - out::tls_end;
      break;
    case R_X86_64_PC64:
      *(u64 *)loc = S + A - P;
      break;
    case R_X86_64_GOTPC32:
      *(u32 *)loc = GOT + A - P;
      break;
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      *(u32 *)loc = sym.get_got_addr() + A - P;
      break;
    default:
      error(toString(this) + ": unknown relocation: " +
            std::to_string(rel.getType(false)));
    }
  }

  static Counter counter("relocs");
  counter.inc(rels.size());
}

void InputSection::scan_relocations() {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  for (const ELF64LE::Rela &rel : rels) {
    Symbol *sym = file->symbols[rel.getSymbol(false)];
    if (!sym->file || !sym->file->is_alive)
      continue;

    switch (rel.getType(false)) {
    case R_X86_64_8:
    case R_X86_64_16:
    case R_X86_64_32:
    case R_X86_64_32S:
    case R_X86_64_64:
    case R_X86_64_PC8:
    case R_X86_64_PC16:
    case R_X86_64_PC32:
    case R_X86_64_PC64:
    case R_X86_64_TPOFF32:
      sym->rels |= Symbol::HAS_ADDR_REL;
      break;
    case R_X86_64_GOT32:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      sym->rels |= Symbol::HAS_GOT_REL;
      break;
    case R_X86_64_PLT32:
      sym->rels |= Symbol::HAS_PLT_REL;
      break;
    case R_X86_64_TLSGD:
      sym->rels |= Symbol::HAS_TLSGD_REL;
      break;
    case R_X86_64_TLSLD:
      sym->rels |= Symbol::HAS_TLSLD_REL;
      break;
    case R_X86_64_GOTTPOFF:
      sym->rels |= Symbol::HAS_GOTTP_REL;
      break;
    case R_X86_64_NONE:
      break;
    default:
      error(toString(this) + ": unknown relocation: " +
            std::to_string(rel.getType(false)));
    }
  }
}

MergeableSection::MergeableSection(InputSection *isec, ArrayRef<u8> contents)
  : InputChunk(MERGEABLE, isec->file, isec->shdr, isec->name),
    parent(*MergedSection::get_instance(isec->name, isec->shdr.sh_flags,
                                        isec->shdr.sh_type)) {
  StringRef data((const char *)&contents[0], contents.size());
  u32 offset = 0;

  while (!data.empty()) {
    size_t end = data.find('\0');
    if (end == StringRef::npos)
      error(toString(this) + ": string is not null terminated");

    StringRef substr = data.substr(0, end + 1);
    data = data.substr(end + 1);

    StringPiece *piece = parent.map.insert(substr, StringPiece(substr));
    pieces.push_back({piece, offset});
    offset += substr.size();
  }

  static Counter counter("string_pieces");
  counter.inc(pieces.size());
}

std::string toString(InputChunk *chunk) {
  return (toString(chunk->file) + ":(" + chunk->name + ")").str();
}
