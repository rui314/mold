#pragma once

#include "mold.h"

namespace mold {

enum class FileType {
  UNKNOWN,
  EMPTY,
  ELF_OBJ,
  ELF_DSO,
  MACH_OBJ,
  MACH_DYLIB,
  MACH_UNIVERSAL,
  AR,
  THIN_AR,
  TAPI,
  TEXT,
  LLVM_BITCODE,
};

template <typename C>
bool is_text_file(MappedFile<C> *mf) {
  u8 *data = mf->data;
  return mf->size >= 4 && isprint(data[0]) && isprint(data[1]) &&
         isprint(data[2]) && isprint(data[3]);
}

template <typename C>
FileType get_file_type(MappedFile<C> *mf) {
  std::string_view data = mf->get_contents();

  if (data.empty())
    return FileType::EMPTY;

  if (data.starts_with("\177ELF")) {
    switch (*(u16 *)(data.data() + 16)) {
    case 1: // ET_REL
      return FileType::ELF_OBJ;
    case 3: // ET_DYN
      return FileType::ELF_DSO;
    }
    return FileType::UNKNOWN;
  }

  if (data.starts_with("\xcf\xfa\xed\xfe")) {
    switch (*(u32 *)(data.data() + 12)) {
    case 1: // MH_OBJECT
      return FileType::MACH_OBJ;
    case 6: // MH_DYLIB
      return FileType::MACH_DYLIB;
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
  case FileType::MACH_OBJ: return "MACH_OBJ";
  case FileType::MACH_DYLIB: return "MACH_DYLIB";
  case FileType::MACH_UNIVERSAL: return "MACH_UNIVERSAL";
  case FileType::AR: return "AR";
  case FileType::THIN_AR: return "THIN_AR";
  case FileType::TAPI: return "TAPI";
  case FileType::TEXT: return "TEXT";
  case FileType::LLVM_BITCODE: return "LLVM_BITCODE";
  }
  return "UNKNOWN";
}

inline std::ostream &operator<<(std::ostream &out, FileType type) {
  out << filetype_to_string(type);
  return out;
}

} // namespace mold
