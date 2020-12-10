#include "mold.h"

ElfFile::ElfFile(MemoryMappedFile mb) : mb(mb), ehdr((ElfEhdr &)*mb.data) {
  if (mb.size < sizeof(ElfEhdr))
    error(mb.name + ": file too small");
  if (memcmp(mb.data, "\177ELF", 4))
    error(mb.name + ": not an ELF file");
}

std::span<ElfShdr> ElfFile::get_sections() const {
}

std::span<ElfSym> ElfFile::get_symbols(const ElfShdr &shdr) const {
}

std::string_view ElfFile::get_section_name(const ElfShdr &shdr) {
}

std::string_view ElfFile::get_section_contents(const ElfShdr &shdr) {
}
