#include "chibild.h"

OutputSection::OutputSection(StringRef name) : name(name) {}

uint64_t OutputSection::getOnFileSize() const {
  assert(on_file_size >= 0);
  return on_file_size;
}

void OutputSection::setFileOffset(uint64_t off) {
  uint64_t orig = off;
  for (InputSection *sec : sections) {
    sec->output_file_offset = off;
    off += sec->getOnFileSize();
  }
  on_file_size = off - orig;
}
