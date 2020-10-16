#include "chibild.h"

using namespace llvm::ELF;

OutputEhdr::OutputEhdr() {
  memcpy(&hdr.e_ident, "\177ELF", 4);

  hdr.e_ident[EI_CLASS] = ELFCLASS64;
  hdr.e_ident[EI_DATA] = ELFDATA2LSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_ident[EI_OSABI] = 0;
  hdr.e_ident[EI_ABIVERSION] = 0;
  hdr.e_machine = EM_X86_64;
  hdr.e_version = EV_CURRENT;
  hdr.e_flags = 0;
  hdr.e_ehsize = sizeof(ELF64LE::Ehdr);
  hdr.e_phnum = 0;
  hdr.e_shentsize = sizeof(ELF64LE::Shdr);
  hdr.e_phoff = sizeof(ELF64LE::Ehdr);
  hdr.e_phentsize = sizeof(ELF64LE::Phdr);
}

void OutputSection::set_offset(uint64_t off) {
  offset = off;
  for (InputSection *sec : sections) {
    sec->offset = off;
    off += sec->get_size();
  }
  size = off - offset;
}
