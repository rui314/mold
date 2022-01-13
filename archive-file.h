#pragma once

#include "mold.h"
#include "filetype.h"

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
    if (memcmp(hdr.ar_name, "// ", 3) == 0) {
      strtab = {(char *)body, (size_t)size};
      data = body + size;
      continue;
    }

    // Skip a symbol table.
    if (memcmp(hdr.ar_name, "/ ", 2) == 0) {
      data = body + size;
      continue;
    }

    if (hdr.ar_name[0] != '/')
      Fatal(ctx) << mf->name << ": filename is not stored as a long filename";

    const char *start = strtab.data() + atoi(hdr.ar_name + 1);
    std::string name(start, (const char *)strstr(start, "/\n"));
    std::string path = name.starts_with('/') ?
      name : (filepath(mf->name).parent_path() / name).string();
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
    if (memcmp(hdr.ar_name, "// ", 3) == 0) {
      strtab = {(char *)body, (size_t)size};
      continue;
    }

    // Skip if symbol table
    if (memcmp(hdr.ar_name, "/ ", 2) == 0)
      continue;

    // Read the name field
    std::string name;

    if (memcmp(hdr.ar_name, "#1/", 3) == 0) {
      size_t namelen = (size_t)atoi(hdr.ar_name + 3);
      name = {(char *)(&hdr + 1), namelen};
      if (size_t pos = name.find('\0'))
        name = name.substr(0, pos);
      body += namelen;
    } else if (hdr.ar_name[0] == '/') {
      const char *start = strtab.data() + atoi(hdr.ar_name + 1);
      if (!start)
        continue;
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
