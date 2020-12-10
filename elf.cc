#include "mold.h"

ElfFile::ElfFile(MemoryMappedFile mb) : mb(mb), ehdr((ElfEhdr &)*mb.data) {
  if (mb.size < sizeof(ElfEhdr))
    error(mb.name + ": file too small");
  if (memcmp(mb.data, "\177ELF", 4))
    error(mb.name + ": not an ELF file");
}

std::span<ElfShdr> ElfFile::get_sections() const {
  u8 *begin = mb.data + ehdr.e_shoff;
  u8 *end = begin + ehdr.e_shnum * sizeof(ElfShdr);
  if (mb.data + mb.size < end)
    error(mb.name + ": e_shoff or e_shnum corrupted");
  return {(ElfShdr *)begin, (ElfShdr *)end};
}

std::span<ElfSym> ElfFile::get_symbols(const ElfShdr &shdr) const {
  u8 *begin = mb.data + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb.data + mb.size < end)
    error(mb.name + ": symbol shdr corrupted");
  return {(ElfSym *)begin, (ElfSym *)end};
}

std::string_view ElfFile::get_section_name(const ElfShdr &shdr) {
}

std::string_view ElfFile::get_section_contents(const ElfShdr &shdr) {
}
