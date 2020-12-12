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

std::vector<MemoryMappedFile> read_archive_members(MemoryMappedFile mb) {
  if (mb.size < 8 || memcmp(mb.data, "!<arch>\n", 8))
    error(mb.name + ": not an archive file");
  u8 *data = mb.data + 8;

  std::vector<MemoryMappedFile> vec;
  std::string_view strtab;

  while (data < mb.data + mb.size) {
    ArHdr &hdr = *(ArHdr *)data;
    data += sizeof(ArHdr);
    
    std::string name(hdr.ar_name, strchr(hdr.ar_name, ' '));
    u32 size = atoi(hdr.ar_size);
    
    if (name == "//")
      strtab = {(char *)data, size};
    else if (name != "/" && name != "__.SYMDEF")
      vec.push_back({name, data, size});
    data += size;
  }

  for (MemoryMappedFile &mb : vec) {
    if (mb.name.size() > 0 && mb.name[0] == '/') {
      u32 pos = atoi(mb.name.data() + 1);
      mb.name = strtab.substr(pos, strtab.find('\n', pos));
    }
  }

  return vec;
}
