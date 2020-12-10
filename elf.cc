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

std::string_view ElfFile::get_section_name(const ElfShdr &shdr) const {
  std::string_view shstrtab = get_string(ehdr.e_shstrndx);
  return shstrtab.data() + shdr.sh_name;
}

std::string_view ElfFile::get_string(const ElfShdr &shdr) const {
  u8 *begin = mb.data + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb.data + mb.size < end)
    error(mb.name + ": shdr corrupted");
  return {(char *)begin, (char *)end};
}

std::string_view ElfFile::get_string(u32 idx) const {
  if (sections.size() <= idx)
    error(mb.name + ": invalid section index");
  return get_string(sections[idx]);
}

struct ArHdr {
  char ar_name[16];
  char ar_date[12];
  char ar_uid[6];
  char ar_gid[6];
  char ar_mode[8];
  char ar_size[10];
  char ar_fmag[2];
};

std::vector<MemoryMappedFile> read_archive_members(MemoryMappedFile mb) {
  if (mb.size < 8 || memcmp(mb.data, "!<arch>\n", 8))
    error(mb.name + ": not an archive file");
  u8 *data = mb.data + 8;

  std::vector<MemoryMappedFile> vec;
  std::string_view strtab;

  while (data < mb.data + mb.size) {
    ArHdr &hdr = *(ArHdr *)data;
    data += sizeof(ArHdr);
    
    std::string name(hdr.ar_name, strchr(hdr.ar_name, ' '));
    u32 size = atoi(hdr.ar_size);
    
    if (name == "//")
      strtab = {(char *)data, size};
    else if (name != "/" && name != "__.SYMDEF")
      vec.push_back({name, data, size});
    data += size;
  }

  for (MemoryMappedFile &mb : vec) {
    if (mb.name.size() > 0 && mb.name[0] == '/') {
      u32 pos = atoi(mb.name.data() + 1);
      mb.name = strtab.substr(pos, strtab.find('\n', pos));
    }
  }

  return vec;
}
