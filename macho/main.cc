#include "mold.h"

#include <cstdlib>
#include <fcntl.h>
#include <iostream>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

static u8 *open_file(std::string path) {
  i64 fd = ::open(path.c_str(), O_RDONLY);
  if (fd == -1)
    return nullptr;

  struct stat st;
  if (fstat(fd, &st) == -1) {
    std::cerr << path << ": fstat failed: " << errno_string();
    exit(1);
  }

  void *ptr = mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
  if (ptr == MAP_FAILED) {
    std::cerr << path << ": mmap failed: " << errno_string();
    exit(1);
  }

  close(fd);
  return (u8 *)ptr;
}

int main(int argc, char **argv) {
  if (argc == 1) {
    std::cout << "mold macho stub\n";
    exit(0);
  }

  if (argc != 2) {
    std::cerr << "usage: ld64.mold <executable-name>\n";
    exit(1);
  }

  u8 *buf = open_file(argv[1]);

  MachHeader &hdr = *(MachHeader *)buf;
  std::cout << "magic: 0x" << std::hex << hdr.magic << std::dec
            << "\ncputype: " << hdr.cputype
            << "\ncpusubtype: " << hdr.cpusubtype
            << "\nfiletype: " << hdr.filetype
            << "\nncmds: " << hdr.ncmds
            << "\nsizeofcmds: " << hdr.sizeofcmds
            << "\nflags: 0x" << std::hex << hdr.flags << std::dec
            << "\n";

  return 0;
}

}
