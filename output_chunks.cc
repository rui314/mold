#include "mold.h"

using namespace llvm::ELF;

OutputEhdr *out::ehdr;
OutputShdr *out::shdr;
OutputPhdr *out::phdr;
InterpSection *out::interp;
SymtabSection *out::symtab;
StringTableSection *out::strtab;
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
  hdr->e_type = ET_EXEC;
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
    phdr.p_align = PAGE_SIZE;
    entries.push_back({phdr, members});
  };

  // Create a PT_PHDR for the program header itself.
  add(PT_PHDR, PF_R, {out::phdr});

  // Create an PT_INTERP.
  if (out::interp)
    add(PT_INTERP, PF_R, {out::interp});

  // Create PT_LOAD segments.
  bool first = true;
  bool last_was_bss;

  for (OutputChunk *chunk : chunks) {
    if (!(chunk->shdr.sh_flags & SHF_ALLOC))
      break;

    uint32_t flags = to_phdr_flags(chunk->shdr.sh_flags);
    bool this_is_bss = chunk->shdr.sh_type & SHT_NOBITS;

    if (first) {
      add(PT_LOAD, flags, {chunk});
      last_was_bss = this_is_bss;
      first = false;
      continue;
    }

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
      back->shdr.sh_addr + back->get_size() - front->shdr.sh_addr;
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

OutputSection *
OutputSection::get_instance(StringRef name, uint64_t flags, uint32_t type) {
  name = get_output_name(name);
  flags = flags & ~SHF_GROUP;

  auto find = [&]() -> OutputSection * {
    for (OutputSection *osec : OutputSection::all_instances)
      if (name == osec->name && flags == (osec->shdr.sh_flags & ~SHF_GROUP) &&
          type == osec->shdr.sh_type)
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
  return new OutputSection(name, flags, type);
}

void SymtabSection::add(const ELF64LE::Sym &sym, uint64_t name, uint64_t value) {
  contents.push_back(sym);
  contents.back().st_shndx = get_section_idx(out::shstrtab);
  contents.back().st_name = name;
  contents.back().st_value = value;
}
