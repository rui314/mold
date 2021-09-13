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

static void print_bytes(u8 *buf, i64 size) {
  if (size == 0) {
    std::cout << "[]\n";
    return;
  }

  std::cout << "[" << (u32)buf[0];
  for (i64 i = 1; i < size; i++)
    std::cout << " " << (u32)buf[i];
  std::cout << "]\n";
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
            << "\nflags: 0x" << std::hex << hdr.flags
            << "\n\n";

  u8 *p = buf + sizeof(MachHeader);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;
    p += lc.cmdsize;

    switch (lc.cmd) {
    case LC_SYMTAB: {
      std::cout << "LC_SYMTAB\n";
      SymtabCommand &cmd = *(SymtabCommand *)&lc;
      std::cout << " cmdsize: " << cmd.cmdsize
                << "\n symoff: 0x" << std::hex << cmd.symoff
                << "\n nsyms: " << std::dec << cmd.nsyms
                << "\n stroff: 0x" << std::hex << cmd.stroff
                << "\n strsize: 0x" << cmd.strsize
                << "\n";
      break;
    }
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
      std::cout << " cmdsize: " << cmd.cmdsize
                << "\n segname: " << cmd.segname
                << "\n vmaddr: 0x" << std::hex << cmd.vmaddr
                << "\n vmsize: 0x" << cmd.vmsize
                << "\n fileoff: 0x" << cmd.fileoff
                << "\n filesize: 0x" << cmd.filesize
                << "\n maxprot: " << std::dec << cmd.maxprot
                << "\n initprot: " << cmd.initprot
                << "\n nsects: " << cmd.nsects
                << "\n flags: 0x" << std::hex << cmd.flags
                << "\n";

      Section *sec = (Section *)((u8 *)&lc + sizeof(cmd));
      for (i64 j = 0; j < cmd.nsects; j++) {
        std::cout << " section:\n  sectname: " << sec[j].sectname
                  << "\n  segname: " << sec[j].segname
                  << "\n  addr: 0x" << std::hex << sec[j].addr
                  << "\n  size: 0x" << sec[j].size
                  << "\n  offset: 0x" << sec[j].offset
                  << "\n  align: " << std::dec << (1 << sec[j].align)
                  << "\n  reloff: " << std::hex << sec[j].reloff
                  << "\n  nreloc: " << std::dec << sec[j].nreloc
                  << "\n  flags: 0x" << std::hex << sec[j].flags
                  << "\n";

        if (sec[j].size) {
          std::cout << "  contents: ";
          print_bytes(buf + sec[j].offset, sec[j].size);
        }
      }
      break;
    }
    case LC_UUID:
      std::cout << "LC_UUID\n cmdsize: " << lc.cmdsize << "\n";
      break;
    case LC_DYLD_INFO_ONLY: {
      std::cout << "LC_DYLD_INFO_ONLY\n";
      DyldInfoCommand &cmd = *(DyldInfoCommand *)&lc;

      if (cmd.rebase_off) {
        std::cout << "  rebase: ";
        print_bytes(buf + cmd.rebase_off, cmd.rebase_size);
        std::cout << "  rebase_off: 0x" << std::hex << cmd.rebase_off
                  << "\n  rebase_size: 0x" << cmd.rebase_size
                  << "\n";
      }

      if (cmd.bind_off) {
        std::cout << "  bind: ";
        print_bytes(buf + cmd.bind_off, cmd.bind_size);
        std::cout << "  bind_off: 0x" << std::hex << cmd.bind_off
                  << "\n  bind_size: 0x" << cmd.bind_size
                  << "\n";
      }

      if (cmd.weak_bind_off) {
        std::cout << "  weak_bind: ";
        print_bytes(buf + cmd.weak_bind_off, cmd.weak_bind_size);
        std::cout << "  weak_bind_off: 0x" << std::hex << cmd.weak_bind_off
                  << "\n  weak_bind_size: 0x" << cmd.weak_bind_size
                  << "\n";
      }

      if (cmd.lazy_bind_off) {
        std::cout << "  lazy_bind: ";
        print_bytes(buf + cmd.lazy_bind_off, cmd.lazy_bind_size);
        std::cout << "  lazy_bind_off: 0x" << std::hex << cmd.lazy_bind_off
                  << "\n  lazy_bind_size: 0x" << cmd.lazy_bind_size
                  << "\n";
      }

      if (cmd.export_off) {
        std::cout << "  export: ";
        print_bytes(buf + cmd.export_off, cmd.export_size);
        std::cout << "  export_off: 0x" << std::hex << cmd.export_off
                  << "\n  export_size: 0x" << cmd.export_size
                  << "\n";
      }
      break;
    }
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
  }

  return 0;
}

}
