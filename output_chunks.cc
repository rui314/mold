#include "mold.h"

using namespace llvm::ELF;

OutputEhdr *out::ehdr;
OutputShdr *out::shdr;
OutputPhdr *out::phdr;
InterpSection *out::interp;
StringTableSection *out::shstrtab;

std::vector<OutputSection *> OutputSection::all_instances;

static int get_section_idx(OutputChunk *chunk) {
  for (int i = 0; i < out::shdr->entries.size(); i++)
    if (&chunk->shdr == out::shdr->entries[i])
      return i;
  error("unreachable");
}

void OutputEhdr::relocate(uint8_t *buf) {
  auto *hdr = (ELF64LE::Ehdr *)buf;
  memset(hdr, 0, sizeof(*hdr));

  memcpy(&hdr->e_ident, "\177ELF", 4);
  hdr->e_ident[EI_CLASS] = ELFCLASS64;
  hdr->e_ident[EI_DATA] = ELFDATA2LSB;
  hdr->e_ident[EI_VERSION] = EV_CURRENT;
  hdr->e_ident[EI_OSABI] = 0;
  hdr->e_ident[EI_ABIVERSION] = 0;
  hdr->e_machine = EM_X86_64;
  hdr->e_version = EV_CURRENT;
  hdr->e_entry = Symbol::intern("_start")->addr;
  hdr->e_phoff = out::phdr->shdr.sh_offset;
  hdr->e_shoff = out::shdr->shdr.sh_offset;
  hdr->e_flags = 0;
  hdr->e_ehsize = sizeof(ELF64LE::Ehdr);
  hdr->e_phentsize = sizeof(ELF64LE::Phdr);
  hdr->e_phnum = out::phdr->get_size() / sizeof(ELF64LE::Phdr);
  hdr->e_shentsize = sizeof(ELF64LE::Shdr);
  hdr->e_shnum = out::shdr->entries.size();
  hdr->e_shstrndx = get_section_idx(out::shstrtab);
}

static uint32_t to_phdr_flags(uint64_t sh_flags) {
  uint32_t ret = PF_R;
  if (sh_flags & SHF_WRITE)
    ret |= PF_W;
  if (sh_flags & SHF_EXECINSTR)
    ret |= PF_X;
  return ret;
}

void OutputPhdr::construct(std::vector<OutputChunk *> &chunks) {
  auto add = [&](uint32_t type, uint32_t flags, std::vector<OutputChunk *> members) {
    ELF64LE::Phdr phdr = {};
    phdr.p_type = type;
    phdr.p_flags = flags;
    entries.push_back({phdr, members});
  };

  // Create a PT_PHDR for the program header itself.
  add(PT_PHDR, PF_R, {out::phdr});

  // Create an PT_INTERP.
  if (out::interp)
    add(PT_INTERP, PF_R, {out::interp});

  // Create PT_LOAD segments.
  add(PT_LOAD, PF_R, {});

  bool last_was_bss = false;

  for (OutputChunk *chunk : chunks) {
    if (!(chunk->shdr.sh_flags & SHF_ALLOC))
      break;

    uint32_t flags = to_phdr_flags(chunk->shdr.sh_flags);
    bool this_is_bss = chunk->shdr.sh_type & SHT_NOBITS;

    if (entries.back().phdr.p_flags != flags || (last_was_bss && !this_is_bss))
      add(PT_LOAD, flags, {chunk});
    else
      entries.back().members.push_back(chunk);

    last_was_bss = this_is_bss;
  }

  // Create a PT_TLS.
  for (int i = 0; i < chunks.size(); i++) {
    if (chunks[i]->shdr.sh_flags & SHF_TLS) {
      std::vector<OutputChunk *> vec = {chunks[i++]};
      while (i < chunks.size() && (chunks[i]->shdr.sh_flags & SHF_TLS))
        vec.push_back(chunks[i++]);
      add(PT_TLS, to_phdr_flags(chunks[i]->shdr.sh_flags), vec);
    }
  }

  for (Phdr &ent : entries)
    for (OutputChunk *chunk : ent.members)
      ent.phdr.p_align = std::max(ent.phdr.p_align, chunk->shdr.sh_addralign);

  for (Phdr &ent : entries)
    if (ent.phdr.p_type == PT_LOAD)
      ent.members.front()->starts_new_ptload = true;
}

void OutputPhdr::copy_to(uint8_t *buf) {
  for (Phdr &ent : entries) {
    OutputChunk *front = ent.members.front();
    OutputChunk *back = ent.members.back();

    ent.phdr.p_offset = front->shdr.sh_offset;
    ent.phdr.p_filesz =
      back->shdr.sh_offset + back->get_size() - front->shdr.sh_offset;
    ent.phdr.p_vaddr = front->shdr.sh_addr;
    ent.phdr.p_memsz =
      back->shdr.sh_addr + back->shdr.sh_size - front->shdr.sh_addr;
  }

  auto *p = (ELF64LE::Phdr *)(buf + shdr.sh_offset);
  for (Phdr &ent : entries)
    *p++ = ent.phdr;
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
  uint64_t iflags = isec->shdr.sh_flags & ~SHF_GROUP;

  auto find = [&]() -> OutputSection * {
    for (OutputSection *osec : OutputSection::all_instances)
      if (iname == osec->name && iflags == (osec->shdr.sh_flags & ~SHF_GROUP) &&
          isec->shdr.sh_type == osec->shdr.sh_type)
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
  return new OutputSection(iname, iflags, isec->shdr.sh_type);
}
