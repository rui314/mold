#include "mold.h"

using namespace llvm::ELF;

OutputEhdr *out::ehdr;
OutputShdr *out::shdr;
OutputPhdr *out::phdr;
std::vector<OutputSection *> OutputSection::all_instances;

void OutputEhdr::relocate(uint8_t *buf) {
  auto *hdr = (ELF64LE::Ehdr *)buf;

  memcpy(&hdr->e_ident, "\177ELF", 4);
  hdr->e_ident[EI_CLASS] = ELFCLASS64;
  hdr->e_ident[EI_DATA] = ELFDATA2LSB;
  hdr->e_ident[EI_VERSION] = EV_CURRENT;
  hdr->e_ident[EI_OSABI] = 0;
  hdr->e_ident[EI_ABIVERSION] = 0;
  hdr->e_machine = EM_X86_64;
  hdr->e_version = EV_CURRENT;
  hdr->e_entry = 0;
  hdr->e_phoff = out::phdr->get_offset();
  hdr->e_shoff = out::shdr->get_offset();
  hdr->e_flags = 0;
  hdr->e_ehsize = sizeof(ELF64LE::Ehdr);
  hdr->e_phentsize = sizeof(ELF64LE::Phdr);
  hdr->e_phnum = out::phdr->hdr.size();
  hdr->e_shentsize = sizeof(ELF64LE::Shdr);
  hdr->e_shnum = out::shdr->hdr.size();
  hdr->e_shstrndx = 0;
}

void OutputSection::set_offset(uint64_t off) {
  offset = off;
  for (InputChunk *chunk : chunks) {
    chunk->offset = off;
    off += chunk->get_size();
  }
  size = off - offset;
}

static StringRef get_output_name(StringRef name) {
  static StringRef common_names[] = {
    ".text.", ".data.rel.ro.", ".data.", ".rodata.", ".bss.rel.ro.",
    ".bss.", ".init_array.", ".fini_array.", ".tbss.", ".tdata.",
  };

  for (StringRef s : common_names)
    if (name.startswith(s) || name == s.drop_back())
      return s.drop_back();
  return name;
}

OutputSection *OutputSection::get_instance(InputSection *isec) {
  StringRef iname = get_output_name(isec->name);
  uint64_t iflags = isec->hdr->sh_flags & ~SHF_GROUP;

  auto find = [&]() -> OutputSection * {
    for (OutputSection *osec : OutputSection::all_instances)
      if (iname == osec->name && iflags == (osec->hdr.sh_flags & ~SHF_GROUP) &&
          isec->hdr->sh_type == osec->hdr.sh_type)
        return osec;
    return nullptr;
  };

  // Search for an exiting output section.
  static std::shared_mutex mu;
  std::shared_lock shared_lock(mu);
  if (OutputSection *osec = find())
    return osec;
  shared_lock.unlock();

  // Create a new output section.
  std::unique_lock unique_lock(mu);
  if (OutputSection *osec = find())
    return osec;
  return new OutputSection(iname, iflags, isec->hdr->sh_type);
}

uint64_t StringTableSection::addString(StringRef s) {
  uint64_t ret = contents.size();
  contents += s.str();
  contents += "\0";
  return ret;
}

void StringTableSection::copy_to(uint8_t *buf) {
  memcpy(buf + offset, &contents[0], contents.size());
}
