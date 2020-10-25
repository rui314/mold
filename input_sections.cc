#include "mold.h"

using namespace llvm::ELF;

std::atomic_int num_relocs;

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr &shdr, StringRef name)
  : file(file), shdr(shdr) {
  this->name = name;
  this->output_section = OutputSection::get_instance(this);

  uint64_t align = (shdr.sh_addralign == 0) ? 1 : shdr.sh_addralign;
  if (align > UINT32_MAX)
    error(toString(file) + ": section sh_addralign is too large");
  if (__builtin_popcount(align) != 1)
    error(toString(file) + ": section sh_addralign is not a power of two");
}

void InputSection::copy_to(uint8_t *buf) {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;
  ArrayRef<uint8_t> data = check(file->obj.getSectionContents(shdr));
  memcpy(buf + output_section->shdr.sh_offset + offset, &data[0], data.size());
}

thread_local int count;

void InputSection::scan_relocations() {
  if (rels.empty())
    return;

  for (const ELF64LE::Rela &rel : rels) {
    Symbol *sym = file->symbols[rel.getSymbol(false)];
    if (!sym)
      continue;

    switch (rel.getType(false)) {
    case R_X86_64_GOTPCREL:
    case R_X86_64_TLSGD:
    case R_X86_64_GOTTPOFF:
    case R_X86_64_PLT32:
      if (!sym->needs_got)
        sym->needs_got = true;
      break;
    default:
      count++;
    }
  }
}

void InputSection::relocate(uint8_t *buf) {
  if (rels.empty())
    return;

  int i = 0;
  for (const ELF64LE::Rela &rel : rels) {
    uint8_t *loc = buf + output_section->shdr.sh_offset + offset + rel.r_offset;
    uint64_t val = 5;

    switch (rel.getType(false)) {
    case R_X86_64_8:
      *loc = val;
      break;
    case R_X86_64_PC8:
      *loc = val;
      break;
    case R_X86_64_16:
      *(uint16_t *)loc = val;
      break;
    case R_X86_64_PC16:
      *(uint16_t *)loc = val;
      break;
    case R_X86_64_32:
      *(uint32_t *)loc = val;
      break;
    case R_X86_64_32S:
    case R_X86_64_TPOFF32:
    case R_X86_64_GOT32:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPC32_TLSDESC:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
    case R_X86_64_PC32:
    case R_X86_64_GOTTPOFF:
    case R_X86_64_PLT32:
    case R_X86_64_TLSGD:
    case R_X86_64_TLSLD:
    case R_X86_64_DTPOFF32:
    case R_X86_64_SIZE32:
      *(uint32_t *)loc = val;
      break;
    case R_X86_64_64:
    case R_X86_64_DTPOFF64:
    case R_X86_64_PC64:
    case R_X86_64_SIZE64:
    case R_X86_64_GOT64:
    case R_X86_64_GOTOFF64:
    case R_X86_64_GOTPC64:
      *(uint64_t *)loc = val;
      break;
    default:
      error(toString(this) + ": unknown relocation");
    }
    // num_relocs++;
  }
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
