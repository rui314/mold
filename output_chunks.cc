#include "mold.h"

#include <shared_mutex>

using namespace llvm::ELF;

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
  if (shdr.sh_type == llvm::ELF::SHT_NOBITS)
    return;

  int num_members = members.size();

  tbb::parallel_for(0, num_members, [&](int i) {
    if (members[i]->shdr.sh_type != SHT_NOBITS) {
      // Copy section contents to an output file
      members[i]->copy_to(buf);

      // Zero-clear trailing padding
      u64 this_end = members[i]->offset + members[i]->shdr.sh_size;
      u64 next_start = (i == num_members - 1) ? shdr.sh_size : members[i + 1]->offset;
      memset(buf + shdr.sh_offset + this_end, 0, next_start - this_end);
    }
  });
}

bool OutputSection::empty() const {
  if (!members.empty())
    for (InputChunk *mem : members)
      if (mem->shdr.sh_size)
        return false;
  return true;
}

void PltSection::initialize(u8 *buf) {
  const u8 data[] = {
    0xff, 0x35, 0, 0, 0, 0, // pushq GOTPLT+8(%rip)
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
    0x0f, 0x1f, 0x40, 0x00, // nop
  };

  u8 *base = buf + shdr.sh_offset;
  memcpy(base, data, sizeof(data));
  *(u32 *)(base + 2) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 2;
  *(u32 *)(base + 8) = out::gotplt->shdr.sh_addr - shdr.sh_addr + 4;
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
