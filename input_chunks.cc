#include "mold.h"

using namespace llvm::ELF;

std::atomic_int num_relocs;

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name)
  : file(file), hdr(hdr) {
  this->name = name;
  this->output_section = OutputSection::get_instance(this);

  uint64_t align = (hdr->sh_addralign == 0) ? 1 : hdr->sh_addralign;
  if (align > UINT32_MAX)
    error(toString(file) + ": section sh_addralign is too large");
  if (__builtin_popcount(align) != 1)
    error(toString(file) + ": section sh_addralign is not a power of two");
  this->alignment = align;
}

uint64_t InputSection::get_size() const {
  return hdr->sh_size;
}

void InputSection::copy_to(uint8_t *buf) {
  if (hdr->sh_type == SHT_NOBITS || hdr->sh_size == 0)
    return;
  ArrayRef<uint8_t> data = check(file->obj.getSectionContents(*hdr));
  memcpy(buf + offset, &data[0], data.size());
}

void InputSection::relocate(uint8_t *buf) {
  if (rels.empty())
    return;

  int i = 0;
  for (const ELF64LE::Rela &rel : rels) {
    uint8_t *loc = buf + offset + rel.r_offset;
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

uint64_t StringTableSection::addString(StringRef s) {
  uint64_t ret = contents.size();
  contents += s.str();
  contents += "\0";
  return ret;
}

void StringTableSection::copy_to(uint8_t *buf) {
  memcpy(buf + offset, &contents[0], contents.size());
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
