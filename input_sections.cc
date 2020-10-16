#include "chibild.h"

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name)
  : file(file), hdr(hdr), name(name) {}

uint64_t InputSection::get_size() const {
  return hdr->sh_size;
}

void InputSection::copy_to(uint8_t *buf) {
  ArrayRef<uint8_t> data = check(file->obj.getSectionContents(*hdr));
  memcpy(buf + offset, &data[0], data.size());
}
