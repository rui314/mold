#include "chibild.h"

OutputSection::OutputSection(StringRef name) : name(name) {}

uint64_t OutputSection::get_size() const {
  assert(size >= 0);
  return size;
}

void OutputSection::set_offset(uint64_t off) {
  offset = off;
  for (InputSection *sec : sections) {
    sec->offset = off;
    off += sec->get_size();
  }
  size = off - offset;
}

void OutputSection::writeTo(uint8_t *buf) {
  for (InputSection *sec : sections)
    sec->writeTo(buf);
}
