#include "mold.h"

namespace mold::elf {

struct ArHdr {
  char ar_name[16];
  char ar_date[12];
  char ar_uid[6];
  char ar_gid[6];
  char ar_mode[8];
  char ar_size[10];
  char ar_fmag[2];
};

template <typename E>
std::vector<MappedFile<Context<E>> *>
read_thin_archive_members(Context<E> &ctx, MappedFile<Context<E>> *mb) {
  u8 *begin = mb->data;
  u8 *data = begin + 8;
  std::vector<MappedFile<Context<E>> *> vec;
  std::string_view strtab;

  while (data < begin + mb->size) {
    // Each header is aligned to a 2 byte boundary.
    if ((begin - data) % 2)
      data++;

    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);

    // Read a string table.
    if (!memcmp(hdr.ar_name, "// ", 3)) {
      strtab = {(char *)body, size};
      data = body + size;
      continue;
    }

    // Skip a symbol table.
    if (!memcmp(hdr.ar_name, "/ ", 2)) {
      data = body + size;
      continue;
    }

    if (hdr.ar_name[0] != '/')
      Fatal(ctx) << mb->name << ": filename is not stored as a long filename";

    const char *start = strtab.data() + atoi(hdr.ar_name + 1);
    std::string name(start, (const char *)strstr(start, "/\n"));
    std::string path = name.starts_with('/') ?
      name : std::string(path_dirname(mb->name)) + "/" + name;
    vec.push_back(MappedFile<Context<E>>::must_open(ctx, path));
    data = body;
  }
  return vec;
}

template <typename E>
std::vector<MappedFile<Context<E>> *>
read_fat_archive_members(Context<E> &ctx, MappedFile<Context<E>> *mb) {
  u8 *begin = mb->data;
  u8 *data = begin + 8;
  std::vector<MappedFile<Context<E>> *> vec;
  std::string_view strtab;

  while (begin + mb->size - data >= 2) {
    if ((begin - data) % 2)
      data++;

    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);
    data = body + size;

    if (!memcmp(hdr.ar_name, "// ", 3)) {
      strtab = {(char *)body, size};
      continue;
    }

    if (!memcmp(hdr.ar_name, "/ ", 2) ||
        !memcmp(hdr.ar_name, "__.SYMDEF/", 10))
      continue;

    std::string name;

    if (hdr.ar_name[0] == '/') {
      const char *start = strtab.data() + atoi(hdr.ar_name + 1);
      name = {start, (const char *)strstr(start, "/\n")};
    } else {
      name = {hdr.ar_name, strchr(hdr.ar_name, '/')};
    }

    vec.push_back(mb->slice(ctx, name, body - begin, size));
  }
  return vec;
}

template <typename E>
std::vector<MappedFile<Context<E>> *>
read_archive_members(Context<E> &ctx, MappedFile<Context<E>> *mb) {
  switch (get_file_type(ctx, mb)) {
  case FileType::AR:
    return read_fat_archive_members(ctx, mb);
  case FileType::THIN_AR:
    return read_thin_archive_members(ctx, mb);
  default:
    unreachable(ctx);
  }
}

#define INSTANTIATE(E)                                                  \
  template std::vector<MappedFile<Context<E>> *>                        \
  read_fat_archive_members(Context<E> &, MappedFile<Context<E>> *);     \
  template std::vector<MappedFile<Context<E>> *>                        \
  read_thin_archive_members(Context<E> &, MappedFile<Context<E>> *);    \
  template std::vector<MappedFile<Context<E>> *>                        \
  read_archive_members(Context<E> &, MappedFile<Context<E>> *);

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(AARCH64);

} // namespace mold::elf
