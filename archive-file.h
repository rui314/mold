#pragma once

#include "mold.h"

namespace mold {

struct ArHdr {
  char ar_name[16];
  char ar_date[12];
  char ar_uid[6];
  char ar_gid[6];
  char ar_mode[8];
  char ar_size[10];
  char ar_fmag[2];
};

enum class FileType {
  UNKNOWN,
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

static bool equal(const char *p, const char *q) {
  return memcmp(p, q, strlen(q)) == 0;
}

template <typename C>
std::vector<MappedFile<C> *>
read_thin_archive_members(C &ctx, MappedFile<C> *mf) {
  u8 *begin = mf->data;
  u8 *data = begin + 8;
  std::vector<MappedFile<C> *> vec;
  std::string_view strtab;

  while (data < begin + mf->size) {
    // Each header is aligned to a 2 byte boundary.
    if ((begin - data) % 2)
      data++;

    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);

    // Read a string table.
    if (equal(hdr.ar_name, "// ")) {
      strtab = {(char *)body, size};
      data = body + size;
      continue;
    }

    // Skip a symbol table.
    if (equal(hdr.ar_name, "/ ")) {
      data = body + size;
      continue;
    }

    if (hdr.ar_name[0] != '/')
      Fatal(ctx) << mf->name << ": filename is not stored as a long filename";

    const char *start = strtab.data() + atoi(hdr.ar_name + 1);
    std::string name(start, (const char *)strstr(start, "/\n"));
    std::string path = name.starts_with('/') ?
      name : std::string(path_dirname(mf->name)) + "/" + name;
    vec.push_back(MappedFile<C>::must_open(ctx, path));
    data = body;
  }
  return vec;
}

template <typename C>
std::vector<MappedFile<C> *>
read_fat_archive_members(C &ctx, MappedFile<C> *mf) {
  u8 *begin = mf->data;
  u8 *data = begin + 8;
  std::vector<MappedFile<C> *> vec;
  std::string_view strtab;

  while (begin + mf->size - data >= 2) {
    if ((begin - data) % 2)
      data++;

    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);
    data = body + size;

    // Read if string table
    if (equal(hdr.ar_name, "// ")) {
      strtab = {(char *)body, size};
      continue;
    }

    // Skip if symbol table
    if (equal(hdr.ar_name, "/ "))
      continue;

    // Read the name field
    std::string name;

    if (equal(hdr.ar_name, "#1/")) {
      size_t namelen = (size_t)atoi(hdr.ar_name + 3);
      name = {(char *)(&hdr + 1), namelen};
      if (size_t pos = name.find('\0'))
        name = name.substr(0, pos);
      body += namelen;
    } else if (hdr.ar_name[0] == '/') {
      const char *start = strtab.data() + atoi(hdr.ar_name + 1);
      name = {start, (const char *)strstr(start, "/\n")};
    } else {
      char *end = (char *)memchr(hdr.ar_name, '/', sizeof(hdr.ar_name));
      if (!end)
        end = hdr.ar_name + sizeof(hdr.ar_name);
      name = {hdr.ar_name, end};
    }

    // Skip if symbol table
    if (name == "__.SYMDEF" || name == "__.SYMDEF SORTED")
      continue;

    vec.push_back(mf->slice(ctx, name, body - begin, size));
  }
  return vec;
}

template <typename C>
std::vector<MappedFile<C> *>
read_archive_members(C &ctx, MappedFile<C> *mf) {
  switch (get_file_type(mf)) {
  case FileType::AR:
    return read_fat_archive_members(ctx, mf);
  case FileType::THIN_AR:
    return read_thin_archive_members(ctx, mf);
  default:
    unreachable();
  }
}

} // namespace mold
