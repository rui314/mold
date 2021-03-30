#include "mold.h"

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
std::vector<MemoryMappedFile<E> *>
read_thin_archive_members(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data(ctx) + 8;
  std::vector<MemoryMappedFile<E> *> vec;
  std::string_view strtab;

  while (data < mb->data(ctx) + mb->size()) {
    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);

    if (!memcmp(hdr.ar_name, "// ", 3)) {
      strtab = {(char *)body, size};
      data = body + size;
      continue;
    }

    if (!memcmp(hdr.ar_name, "/ ", 2)) {
      data = body + size;
      continue;
    }

    if (hdr.ar_name[0] != '/')
      Fatal(ctx) << mb->name << ": filename is not stored as a long filename";

    const char *start = strtab.data() + atoi(hdr.ar_name + 1);
    std::string name(start, strstr(start, "/\n"));
    std::string path = path_dirname(mb->name) + "/" + name;
    vec.push_back(MemoryMappedFile<E>::must_open(ctx, path));
    data = body;
  }
  return vec;
}

template <typename E>
std::vector<MemoryMappedFile<E> *>
read_fat_archive_members(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data(ctx) + 8;
  std::vector<MemoryMappedFile<E> *> vec;
  std::string_view strtab;

  while (mb->data(ctx) + mb->size() - data >= 2) {
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
      name = {start, strstr(start, "/\n")};
    } else {
      name = {hdr.ar_name, strchr(hdr.ar_name, '/')};
    }

    vec.push_back(mb->slice(name, body - mb->data(ctx), size));
  }
  return vec;
}

template
std::vector<MemoryMappedFile<X86_64> *>
read_fat_archive_members(Context<X86_64> &ctx, MemoryMappedFile<X86_64> *mb);

template
std::vector<MemoryMappedFile<X86_64> *>
read_thin_archive_members(Context<X86_64> &ctx, MemoryMappedFile<X86_64> *mb);

template
std::vector<MemoryMappedFile<I386> *>
read_fat_archive_members(Context<I386> &ctx, MemoryMappedFile<I386> *mb);

template
std::vector<MemoryMappedFile<I386> *>
read_thin_archive_members(Context<I386> &ctx, MemoryMappedFile<I386> *mb);
