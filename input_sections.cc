#include "chibild.h"

using llvm::ELF::SHT_NOBITS;

static OutputSection *get_output_section(StringRef name) {
  static OutputSection common_sections[] = {
    {".text"}, {".data"}, {".data.rel.ro"}, {".rodata"}, {".bss"}, {".bss.rel.ro"},
    {".ctors"}, {".dtors"}, {".init_array"}, {".fini_array"}, {".tbss"}, {".tdata"},
  };

  for (OutputSection &osec : common_sections) {
    if (!name.startswith(osec.name))
      continue;
    if (name.size() == osec.name.size())
      return &osec;
    if (name.size() > osec.name.size() && name[osec.name.size()] == '.')
      return &osec;
  };

  static ConcurrentMap<OutputSection> map;
  return map.insert(name, OutputSection(name));
}

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name)
  : file(file), hdr(hdr) {
  this->name = name;
  this->output_section = get_output_section(name);

  uint64_t align = (hdr->sh_addralign == 0) ? 1 : hdr->sh_addralign;
  if (align > UINT32_MAX)
    error(toString(file) + ": section sh_addralign is too large");
  if (__builtin_popcount(align) != 1)
    error(toString(file) + ": section sh_addralign is not a power of two");
  this->alignment = align;
}

uint64_t InputSection::get_size() const {
  return hdr->sh_size;
}

void InputSection::copy_to(uint8_t *buf) {
  if (hdr->sh_type == SHT_NOBITS || hdr->sh_size == 0)
    return;
  ArrayRef<uint8_t> data = check(file->obj.getSectionContents(*hdr));
  memcpy(buf + offset, &data[0], data.size());
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
