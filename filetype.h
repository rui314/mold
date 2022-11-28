#pragma once

#include "mold.h"
#include "elf/elf.h"

namespace mold {

enum class FileType {
  UNKNOWN,
  EMPTY,
  ELF_OBJ,
  ELF_DSO,
  MACH_OBJ,
  MACH_EXE,
  MACH_DYLIB,
  MACH_BUNDLE,
  MACH_UNIVERSAL,
  AR,
  THIN_AR,
  TAPI,
  TEXT,
  GCC_LTO_OBJ,
  LLVM_BITCODE,
};

template <typename C>
bool is_text_file(MappedFile<C> *mf) {
  u8 *data = mf->data;
  return mf->size >= 4 && isprint(data[0]) && isprint(data[1]) &&
         isprint(data[2]) && isprint(data[3]);
}

template <typename E, typename C>
inline bool is_gcc_lto_obj(MappedFile<C> *mf) {
  using namespace mold::elf;

  const char *data = mf->get_contents().data();
  ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)data;
  ElfShdr<E> *sh_begin = (ElfShdr<E> *)(data + ehdr.e_shoff);
  std::span<ElfShdr<E>> shdrs{(ElfShdr<E> *)(data + ehdr.e_shoff), ehdr.e_shnum};

  // e_shstrndx is a 16-bit field. If .shstrtab's section index is
  // too large, the actual number is stored to sh_link field.
  i64 shstrtab_idx = (ehdr.e_shstrndx == SHN_XINDEX)
    ? sh_begin->sh_link : ehdr.e_shstrndx;

  for (ElfShdr<E> &sec : shdrs) {
    // GCC LTO object contains only sections symbols followed by a common
    // symbol whose name is `__gnu_lto_slim` (or `__gnu_lto_v1` for older
    // GCC releases).
    //
    // However, FAT LTO objects don't have any of the above mentioned symbols
    // and can identify LTO by `.gnu.lto_.symtab.` section, similarly
    // to what lto-plugin does.
    std::string_view name = data + shdrs[shstrtab_idx].sh_offset + sec.sh_name;
    if (name.starts_with (".gnu.lto_.symtab."))
      return true;

    if (sec.sh_type != SHT_SYMTAB)
      continue;

    std::span<ElfSym<E>> elf_syms{(ElfSym<E> *)(data + sec.sh_offset),
                                  (size_t)sec.sh_size / sizeof(ElfSym<E>)};

    auto skip = [](u8 type) {
      return type == STT_NOTYPE || type == STT_FILE || type == STT_SECTION;
    };

    i64 i = 1;
    while (i < elf_syms.size() && skip(elf_syms[i].st_type))
      i++;

    if (i < elf_syms.size() && elf_syms[i].st_shndx == SHN_COMMON) {
      std::string_view name =
        data + shdrs[sec.sh_link].sh_offset + elf_syms[i].st_name;
      if (name.starts_with("__gnu_lto_"))
        return true;
    }
    break;
  }

  return false;
}

template <typename C>
FileType get_file_type(MappedFile<C> *mf) {
  std::string_view data = mf->get_contents();

  if (data.empty())
    return FileType::EMPTY;

  if (data.starts_with("\177ELF")) {
    u8 byte_order = ((elf::EL32Ehdr *)data.data())->e_ident[elf::EI_DATA];

    if (byte_order == elf::ELFDATA2LSB) {
      elf::EL32Ehdr &ehdr = *(elf::EL32Ehdr *)data.data();

      if (ehdr.e_type == elf::ET_REL) {
        if (ehdr.e_ident[elf::EI_CLASS] == elf::ELFCLASS32) {
          if (is_gcc_lto_obj<elf::I386>(mf))
            return FileType::GCC_LTO_OBJ;
        } else {
          if (is_gcc_lto_obj<elf::X86_64>(mf))
            return FileType::GCC_LTO_OBJ;
        }
        return FileType::ELF_OBJ;
      }

      if (ehdr.e_type == elf::ET_DYN)
        return FileType::ELF_DSO;
    } else {
      elf::EB32Ehdr &ehdr = *(elf::EB32Ehdr *)data.data();

      if (ehdr.e_type == elf::ET_REL) {
        if (ehdr.e_ident[elf::EI_CLASS] == elf::ELFCLASS32) {
          if (is_gcc_lto_obj<elf::M68K>(mf))
            return FileType::GCC_LTO_OBJ;
        } else {
          if (is_gcc_lto_obj<elf::SPARC64>(mf))
            return FileType::GCC_LTO_OBJ;
        }
        return FileType::ELF_OBJ;
      }

      if (ehdr.e_type == elf::ET_DYN)
        return FileType::ELF_DSO;
    }
    return FileType::UNKNOWN;
  }

  if (data.starts_with("\xcf\xfa\xed\xfe")) {
    switch (*(ul32 *)(data.data() + 12)) {
    case 1: // MH_OBJECT
      return FileType::MACH_OBJ;
    case 2: // MH_EXECUTE
      return FileType::MACH_EXE;
    case 6: // MH_DYLIB
      return FileType::MACH_DYLIB;
    case 8: // MH_BUNDLE
      return FileType::MACH_BUNDLE;
    }
    return FileType::UNKNOWN;
  }

  if (data.starts_with("!<arch>\n"))
    return FileType::AR;
  if (data.starts_with("!<thin>\n"))
    return FileType::THIN_AR;
  if (data.starts_with("--- !tapi-tbd"))
    return FileType::TAPI;
  if (data.starts_with("\xca\xfe\xba\xbe"))
    return FileType::MACH_UNIVERSAL;
  if (is_text_file(mf))
    return FileType::TEXT;
  if (data.starts_with("\xde\xc0\x17\x0b"))
    return FileType::LLVM_BITCODE;
  if (data.starts_with("BC\xc0\xde"))
    return FileType::LLVM_BITCODE;
  return FileType::UNKNOWN;
}

inline std::string filetype_to_string(FileType type) {
  switch (type) {
  case FileType::UNKNOWN: return "UNKNOWN";
  case FileType::EMPTY: return "EMPTY";
  case FileType::ELF_OBJ: return "ELF_OBJ";
  case FileType::ELF_DSO: return "ELF_DSO";
  case FileType::MACH_EXE: return "MACH_EXE";
  case FileType::MACH_OBJ: return "MACH_OBJ";
  case FileType::MACH_DYLIB: return "MACH_DYLIB";
  case FileType::MACH_BUNDLE: return "MACH_BUNDLE";
  case FileType::MACH_UNIVERSAL: return "MACH_UNIVERSAL";
  case FileType::AR: return "AR";
  case FileType::THIN_AR: return "THIN_AR";
  case FileType::TAPI: return "TAPI";
  case FileType::TEXT: return "TEXT";
  case FileType::GCC_LTO_OBJ: return "GCC_LTO_OBJ";
  case FileType::LLVM_BITCODE: return "LLVM_BITCODE";
  }
  return "UNKNOWN";
}

inline std::ostream &operator<<(std::ostream &out, FileType type) {
  out << filetype_to_string(type);
  return out;
}

} // namespace mold
