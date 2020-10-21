#include "mold.h"

using namespace llvm::ELF;

std::atomic_int num_relocs;

typedef struct {
  StringRef name;
  uint64_t flags;
  uint64_t type;
} LookupKey;

namespace tbb {
template<>
struct tbb_hash_compare<LookupKey> {
  static size_t hash(const LookupKey& k) {
  return llvm::hash_combine(llvm::hash_value(k.name),
                            llvm::hash_value(k.flags),
                            llvm::hash_value(k.type));
  }

  static bool equal(const LookupKey& k1, const LookupKey& k2) {
    return k1.name == k2.name && k1.flags == k2.flags && k1.type == k2.type;
  }
};
}

static OutputSection *get_output_section(InputSection *isec) {
  static OutputSection common_sections[] = {
    {".text", 0, 0},
    {".data", 0, 0},
    {".data.rel.ro", 0, 0},
    {".rodata", 0, 0},
    {".bss", 0, 0},
    {".bss.rel.ro", 0, 0},
    {".ctors", 0, 0},
    {".dtors", 0, 0},
    {".init_array", 0, 0},
    {".fini_array", 0, 0},
    {".tbss", 0, 0},
    {".tdata", 0, 0},
  };

  for (OutputSection &osec : common_sections) {
    if (isec->name.startswith(osec.name) &&
        isec->hdr->sh_flags == osec.hdr.sh_flags &&
        isec->hdr->sh_type == osec.hdr.sh_type) {
      bool dot_follows =
        osec.name.size() < isec->name.size() && isec->name[osec.name.size()] == '.';
      if (isec->name.size() == osec.name.size() || dot_follows)
        return &osec;
    }
  }

  typedef tbb::concurrent_hash_map<LookupKey, OutputSection> T;
  static T map;
  T::accessor acc;

  LookupKey key = {isec->name, isec->hdr->sh_flags, isec->hdr->sh_type};
  OutputSection val(isec->name, isec->hdr->sh_flags, isec->hdr->sh_type);
  map.insert(acc, std::make_pair(key, val));
  return &acc->second;
}

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name)
  : file(file), hdr(hdr) {
  this->name = name;
  this->output_section = get_output_section(this);

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

void InputSection::relocate(uint8_t *buf) {
  if (rels.empty())
    return;

  int i = 0;
  for (const ELF64LE::Rela &rel : rels) {
    uint8_t *loc = buf + offset + rel.r_offset;
    uint64_t val = 5;

    switch (rel.getType(false)) {
    case R_X86_64_8:
      *loc = val;
      break;
    case R_X86_64_PC8:
      *loc = val;
      break;
    case R_X86_64_16:
      *(uint16_t *)loc = val;
      break;
    case R_X86_64_PC16:
      *(uint16_t *)loc = val;
      break;
    case R_X86_64_32:
      *(uint32_t *)loc = val;
      break;
    case R_X86_64_32S:
    case R_X86_64_TPOFF32:
    case R_X86_64_GOT32:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPC32_TLSDESC:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
    case R_X86_64_PC32:
    case R_X86_64_GOTTPOFF:
    case R_X86_64_PLT32:
    case R_X86_64_TLSGD:
    case R_X86_64_TLSLD:
    case R_X86_64_DTPOFF32:
    case R_X86_64_SIZE32:
      *(uint32_t *)loc = val;
      break;
    case R_X86_64_64:
    case R_X86_64_DTPOFF64:
    case R_X86_64_PC64:
    case R_X86_64_SIZE64:
    case R_X86_64_GOT64:
    case R_X86_64_GOTOFF64:
    case R_X86_64_GOTPC64:
      *(uint64_t *)loc = val;
      break;
    default:
      error(toString(this) + ": unknown relocation");
    }
    // num_relocs++;
  }
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
