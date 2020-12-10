#include "mold.h"

ElfFile::ElfFile(MemoryMappedFile mb) : mb(mb), ehdr((ElfEhdr &)*mb.data) {
  if (mb.size < sizeof(ElfEhdr))
    error(mb.name + ": file too small");
  if (memcmp(mb.data, "\177ELF", 4))
    error(mb.name + ": not an ELF file");

  u8 *begin = mb.data + ehdr.e_shoff;
  u8 *end = begin + ehdr.e_shnum * sizeof(ElfShdr);
  if (mb.data + mb.size < end)
    error(mb.name + ": e_shoff or e_shnum corrupted");
  sections = {(ElfShdr *)begin, (ElfShdr *)end};
}

std::span<ElfShdr> ElfFile::get_sections() const {
  return sections;
}

std::span<ElfSym> ElfFile::get_symbols(const ElfShdr &shdr) const {
  std::string_view view = get_section_contents(shdr);
  return {(ElfSym *)view.data(), view.size() / sizeof(ElfSym)};
}

std::string_view ElfFile::get_section_name(const ElfShdr &shdr) const {
  std::string_view shstrtab = get_section_contents(ehdr.e_shstrndx);
  return shstrtab.data() + shdr.sh_name;
}

std::string_view ElfFile::get_section_contents(const ElfShdr &shdr) const {
  u8 *begin = mb.data + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb.data + mb.size < end)
    error(mb.name + ": shdr corrupted");
  return {(char *)begin, (char *)end};
}

std::string_view ElfFile::get_section_contents(u32 idx) const {
  if (sections.size() <= idx)
    error(mb.name + ": invalid section index");
  return get_section_contents(sections[idx]);
}
