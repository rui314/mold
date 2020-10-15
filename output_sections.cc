#include "chibild.h"

void OutputEhdr::writeTo(uint8_t *buf) {
  memcpy(buf + offset, &hdr, sizeof(hdr));
}

uint64_t OutputEhdr::get_size() const {
  return sizeof(hdr);
}

void OutputShdr::writeTo(uint8_t *buf) {
  memcpy(buf + offset, &hdr[0], get_size());
}

uint64_t OutputShdr::get_size() const {
  return hdr.size() * sizeof(hdr[0]);
}

void OutputPhdr::writeTo(uint8_t *buf) {
  memcpy(buf + offset, &hdr[0], get_size());
}

uint64_t OutputPhdr::get_size() const {
  return hdr.size() * sizeof(hdr[0]);
}

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
