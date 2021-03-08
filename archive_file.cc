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

std::vector<MemoryMappedFile *> read_thin_archive_members(MemoryMappedFile *mb) {
  u8 *data = mb->data() + 8;
  std::vector<MemoryMappedFile *> vec;
  std::string_view strtab;

  while (data < mb->data() + mb->size()) {
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
      Fatal() << mb->name << ": filename is not stored as a long filename";

    const char *start = strtab.data() + atoi(hdr.ar_name + 1);
    std::string name(start, strstr(start, "/\n"));
    std::string path = path_dirname(mb->name) + "/" + name;
    vec.push_back(MemoryMappedFile::must_open(path));
    data = body;
  }
  return vec;
}

std::vector<MemoryMappedFile *> read_fat_archive_members(MemoryMappedFile *mb) {
  u8 *data = mb->data() + 8;
  std::vector<MemoryMappedFile *> vec;
  std::string_view strtab;

  while (data < mb->data() + mb->size()) {
    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);
    data = body + size;

    if (!memcmp(hdr.ar_name, "// ", 3)) {
      strtab = {(char *)body, size};
      continue;
    }

    if (!memcmp(hdr.ar_name, "/ ", 2) || !memcmp(hdr.ar_name, "__.SYMDEF/", 10))
      continue;

    std::string name;

    if (hdr.ar_name[0] == '/') {
      const char *start = strtab.data() + atoi(hdr.ar_name + 1);
      name = {start, strstr(start, "/\n")};
    } else {
      name = {hdr.ar_name, strchr(hdr.ar_name, '/')};
    }

    vec.push_back(mb->slice(name, body - mb->data(), size));
  }
  return vec;
}

std::vector<MemoryMappedFile *> read_archive_members(MemoryMappedFile *mb) {
  if (mb->size() < 8)
    Fatal() << mb->name << ": not an archive file";
  if (memcmp(mb->data(), "!<arch>\n", 8) && memcmp(mb->data(), "!<thin>\n", 8))
    Fatal() << mb->name << ": not an archive file";

  bool is_thin = !memcmp(mb->data(), "!<thin>\n", 8);
  if (is_thin)
    return read_thin_archive_members(mb);
  return read_fat_archive_members(mb);
}
