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
            << "\n\n";

  u8 *p = buf + sizeof(MachHeader);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;

    switch (lc.cmd) {
    case LC_SYMTAB:
      std::cout << "LC_SYMTAB\n";
      break;
    case LC_DYSYMTAB:
      std::cout << "LC_DYSYMTAB\n";
      break;
    case LC_LOAD_DYLIB:
      std::cout << "LC_LOAD_DYLIB\n";
      break;
    case LC_LOAD_DYLINKER:
      std::cout << "LC_LOAD_DYLINKER\n";
      break;
    case LC_SEGMENT_64: {
      std::cout << "LC_SEGMENT_64\n";
      SegmentCommand &cmd = *(SegmentCommand *)&lc;
      std::cout << " segname: " << cmd.segname
                << "\n vmaddr: 0x" << std::hex << cmd.vmaddr << std::dec
                << "\n vmsize: 0x" << std::hex << cmd.vmsize << std::dec
                << "\n fileoff: " << cmd.fileoff
                << "\n filesize: " << cmd.filesize
                << "\n maxprot: " << cmd.maxprot
                << "\n initprot: " << cmd.initprot
                << "\n nsects: " << cmd.nsects
                << "\n flags: 0x" << std::hex << cmd.flags << std::dec
                << "\n";
      break;
    }
    case LC_UUID:
      std::cout << "LC_UUID\n";
      break;
    case LC_DYLD_INFO_ONLY:
      std::cout << "LC_DYLD_INFO_ONLY\n";
      break;
    case LC_FUNCTION_STARTS:
      std::cout << "LC_FUNCTION_STARTS\n";
      break;
    case LC_MAIN:
      std::cout << "LC_MAIN\n";
      break;
    case LC_DATA_IN_CODE:
      std::cout << "LC_DATA_IN_CODE\n";
      break;
    case LC_SOURCE_VERSION:
      std::cout << "LC_SOURCE_VERSION\n";
      break;
    case LC_BUILD_VERSION:
      std::cout << "LC_BUILD_VERSION\n";
      break;
    default:
      std::cout << "UNKNOWN (0x" << std::hex << lc.cmd << ")\n";
      break;
    }

    p += lc.cmdsize;
  }

  return 0;
}

}
