#include "chibild.h"

using namespace llvm::ELF;

OutputEhdr OutputEhdr::instance;
OutputShdr OutputShdr::instance;
OutputPhdr OutputPhdr::instance;

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
  hdr->e_phoff = OutputPhdr::instance.get_offset();
  hdr->e_shoff = OutputShdr::instance.get_offset();
  hdr->e_flags = 0;
  hdr->e_ehsize = sizeof(ELF64LE::Ehdr);
  hdr->e_phentsize = sizeof(ELF64LE::Phdr);
  hdr->e_phnum = OutputPhdr::instance.hdr.size();
  hdr->e_shentsize = sizeof(ELF64LE::Shdr);
  hdr->e_shnum = OutputShdr::instance.hdr.size();
  hdr->e_shstrndx = 0;
}

void OutputSection::set_offset(uint64_t off) {
  offset = off;
  for (InputSection *sec : sections) {
    sec->offset = off;
    off += sec->get_size();
  }
  size = off - offset;
}
