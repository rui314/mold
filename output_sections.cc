#include "chibild.h"

OutputSection::OutputSection(StringRef name) : name(name) {}

uint64_t OutputSection::get_on_file_size() const {
  assert(on_file_size >= 0);
  return on_file_size;
}

void OutputSection::set_file_offset(uint64_t off) {
  file_offset = off;
  for (InputSection *sec : sections) {
    sec->output_file_offset = off;
    off += sec->get_on_file_size();
  }
  on_file_size = off - file_offset;
}

uint64_t OutputSection::get_file_offset() const {
  return file_offset;
}

void OutputSection::writeTo(uint8_t *buf) {}
