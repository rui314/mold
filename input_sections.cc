#include "chibild.h"

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

  if (hdr->sh_addralign > UINT32_MAX)
    error(toString(file) + ": section sh_addralign is too large");
}

uint64_t InputSection::get_size() const {
  return hdr->sh_size;
}

void InputSection::copy_to(uint8_t *buf) {
  ArrayRef<uint8_t> data = check(file->obj.getSectionContents(*hdr));
  memcpy(buf + offset, &data[0], data.size());
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
