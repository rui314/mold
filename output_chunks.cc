#include "mold.h"

#include <shared_mutex>

using namespace llvm::ELF;

void OutputEhdr::copy_to(u8 *buf) {
  auto &hdr = *(ELF64LE::Ehdr *)buf;
  memset(&hdr, 0, sizeof(hdr));

  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = ELFCLASS64;
  hdr.e_ident[EI_DATA] = ELFDATA2LSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_ident[EI_OSABI] = 0;
  hdr.e_ident[EI_ABIVERSION] = 0;
  hdr.e_type = ET_EXEC;
  hdr.e_machine = EM_X86_64;
  hdr.e_version = EV_CURRENT;
  hdr.e_entry = Symbol::intern("_start")->get_addr();
  hdr.e_phoff = out::phdr.shdr.sh_offset;
  hdr.e_shoff = out::shdr.shdr.sh_offset;
  hdr.e_flags = 0;
  hdr.e_ehsize = sizeof(ELF64LE::Ehdr);
  hdr.e_phentsize = sizeof(ELF64LE::Phdr);
  hdr.e_phnum = out::phdr.shdr.sh_size / sizeof(ELF64LE::Phdr);
  hdr.e_shentsize = sizeof(ELF64LE::Shdr);
  hdr.e_shnum = out::shdr.entries.size();
  hdr.e_shstrndx = out::shstrtab.shndx;
}

void OutputPhdr::copy_to(u8 *buf) {
  for (Entry &ent : entries) {
    OutputChunk *front = ent.members.front();
    OutputChunk *back = ent.members.back();

    ent.phdr.p_offset = front->shdr.sh_offset;
    ent.phdr.p_filesz = (back->shdr.sh_type == SHT_NOBITS)
      ? back->shdr.sh_offset - front->shdr.sh_offset
      : back->shdr.sh_offset - front->shdr.sh_offset + back->shdr.sh_size;
    ent.phdr.p_vaddr = front->shdr.sh_addr;
    ent.phdr.p_memsz =
      back->shdr.sh_addr + back->shdr.sh_size - front->shdr.sh_addr;
  }

  auto *p = (ELF64LE::Phdr *)(buf + shdr.sh_offset);
  for (Entry &ent : entries)
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
OutputSection::get_instance(StringRef name, u64 flags, u32 type) {
  name = get_output_name(name);
  flags = flags & ~(u64)SHF_GROUP;

  auto find = [&]() -> OutputSection * {
    for (OutputSection *osec : OutputSection::instances)
      if (name == osec->name && flags == (osec->shdr.sh_flags & ~SHF_GROUP) &&
          type == osec->shdr.sh_type)
        return osec;
    return nullptr;
  };

  // Search for an exiting output section.
  static std::shared_mutex mu;
  {
    std::shared_lock lock(mu);
    if (OutputSection *osec = find())
      return osec;
  }

  // Create a new output section.
  std::unique_lock lock(mu);
  if (OutputSection *osec = find())
    return osec;
  return new OutputSection(name, flags, type);
}

void OutputSection::copy_to(u8 *buf) {
  if (shdr.sh_type != llvm::ELF::SHT_NOBITS)
    tbb::parallel_for_each(members, [&](InputChunk *mem) { mem->copy_to(buf); });
}

bool OutputSection::empty() const {
  if (!members.empty())
    for (InputChunk *mem : members)
      if (mem->shdr.sh_size)
        return false;
  return true;
}

MergedSection *
MergedSection::get_instance(StringRef name, u64 flags, u32 type) {
  name = get_output_name(name);
  flags = flags & ~(u64)SHF_MERGE & ~(u64)SHF_STRINGS;

  auto find = [&]() -> MergedSection * {
    for (MergedSection *osec : MergedSection::instances)
      if (name == osec->name && flags == osec->shdr.sh_flags &&
          type == osec->shdr.sh_type)
        return osec;
    return nullptr;
  };

  // Search for an exiting output section.
  static std::shared_mutex mu;
  {
    std::shared_lock lock(mu);
    if (MergedSection *osec = find())
      return osec;
  }

  // Create a new output section.
  std::unique_lock lock(mu);
  if (MergedSection *osec = find())
    return osec;

  auto *osec = new MergedSection(name, flags, type);
  MergedSection::instances.push_back(osec);
  return osec;
}
