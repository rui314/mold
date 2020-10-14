#include "chibild.h"

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name)
  : file(file), hdr(hdr), name(name) {}

uint64_t InputSection::getOnFileSize() const {
  return hdr->sh_size;
}
