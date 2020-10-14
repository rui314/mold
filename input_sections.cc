#include "chibild.h"

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name)
  : file(file), hdr(hdr), name(name) {}

uint64_t InputSection::get_on_file_size() const {
  return hdr->sh_size;
}
