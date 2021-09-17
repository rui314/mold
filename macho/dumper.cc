#include "mold.h"

#include <cstdlib>
#include <fcntl.h>
#include <iomanip>
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

  std::cout << "[" << std::setw(2) << std::setfill('0') << (u32)buf[0];
  for (i64 i = 1; i < size; i++)
    std::cout << " " << std::setw(2) << std::setfill('0') << (u32)buf[i];
  std::cout << "]\n";
}

void dump_file(std::string path) {
  u8 *buf = open_file(path);

  MachHeader &hdr = *(MachHeader *)buf;
  std::cout << "magic: 0x" << std::hex << hdr.magic
            << "\ncputype: 0x" << hdr.cputype
            << "\ncpusubtype: 0x" << hdr.cpusubtype
            << "\nfiletype: 0x" << hdr.filetype
            << "\nncmds: 0x" << hdr.ncmds
            << "\nsizeofcmds: 0x" << hdr.sizeofcmds
            << "\nflags: 0x" << hdr.flags
            << "\n\n";

  u8 *p = buf + sizeof(MachHeader);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;
    p += lc.cmdsize;

    auto print_dylib = [&]() {
      DylibCommand &cmd = *(DylibCommand *)&lc;
      std::cout << " cmdsize: 0x" << cmd.cmdsize
                << "\n nameoff: 0x" << cmd.nameoff
                << "\n timestamp: 0x" << cmd.timestamp
                << "\n current_version: 0x" << cmd.current_version
                << "\n compatibility_version: 0x" << cmd.compatibility_version
                << "\n data: " << (char *)((char *)&cmd + sizeof(cmd))
                << "\n";
    };

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

      MachSym *syms = (MachSym *)(buf + cmd.symoff);
      for (i64 j = 0; j < cmd.nsyms; j++) {
        std::cout << " symbol:"
                  << "\n  name: " << (char *)(buf + cmd.stroff + syms[j].stroff)
                  << "\n  stub: " << (u32)syms[j].stub
                  << "\n  pext: " << (u32)syms[j].pext
                  << "\n  type: " << (u32)syms[j].type
                  << "\n  ext: " << (u32)syms[j].ext
                  << "\n  sect: 0x" << (u32)syms[j].sect
                  << "\n  desc: 0x" << (u32)syms[j].desc
                  << "\n  value: 0x" << syms[j].value
                  << "\n";
      }
      break;
    }
    case LC_DYSYMTAB: {
      std::cout << "LC_DYSYMTAB\n";
      DysymtabCommand &cmd = *(DysymtabCommand *)&lc;
      std::cout << " cmdsize: 0x" << cmd.cmdsize
                << "\n ilocalsym: 0x" << cmd.ilocalsym
                << "\n nlocalsym: 0x" << cmd.nlocalsym
                << "\n iextdefsym: 0x" << cmd.iextdefsym
                << "\n nextdefsym: 0x" << cmd.nextdefsym
                << "\n iundefsym: 0x" << cmd.iundefsym
                << "\n nundefsym: 0x" << cmd.nundefsym
                << "\n tocoff: 0x" << cmd.tocoff
                << "\n ntoc: 0x" << cmd.ntoc
                << "\n modtaboff: 0x" << cmd.modtaboff
                << "\n nmodtab: 0x" << cmd.nmodtab
                << "\n extrefsymoff: 0x" << cmd.extrefsymoff
                << "\n nextrefsyms: 0x" << cmd.nextrefsyms
                << "\n indirectsymoff: 0x" << cmd.indirectsymoff
                << "\n nindirectsyms: 0x" << cmd.nindirectsyms
                << "\n extreloff: 0x" << cmd.extreloff
                << "\n nextrel: 0x" << cmd.nextrel
                << "\n locreloff: 0x" << cmd.locreloff
                << "\n nlocrel: 0x" << cmd.nlocrel
                << "\n";

      if (cmd.indirectsymoff) {
        std::cout << " indirectsymdata: ";
        print_bytes(buf + cmd.indirectsymoff, 4 * cmd.nindirectsyms);
      }
      break;
    }
    case LC_LOAD_DYLIB:
      std::cout << "LC_LOAD_DYLIB\n";
      print_dylib();
      break;
    case LC_LOAD_WEAK_DYLIB:
      std::cout << "LC_LOAD_WEAK_DYLIB\n";
      print_dylib();
      break;
    case LC_ID_DYLIB:
      std::cout << "LC_ID_DYLIB\n";
      print_dylib();
      break;
    case LC_LOAD_DYLINKER: {
      std::cout << "LC_LOAD_DYLINKER\n";
      DylinkerCommand &cmd = *(DylinkerCommand *)&lc;
      std::cout << " cmdsize: 0x" << cmd.cmdsize
                << "\n nameoff: 0x" << cmd.nameoff
                << "\n data: " << (char *)((char *)&cmd + cmd.nameoff)
                << "\n";
      break;
    }
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

      MachSection *sec = (MachSection *)((u8 *)&lc + sizeof(cmd));
      for (i64 j = 0; j < cmd.nsects; j++) {
        std::cout << " section:\n  sectname: " << sec[j].sectname
                  << "\n  segname: " << sec[j].segname
                  << "\n  addr: 0x" << std::hex << sec[j].addr
                  << "\n  size: 0x" << sec[j].size
                  << "\n  offset: 0x" << sec[j].offset
                  << "\n  p2align: " << std::dec << sec[j].p2align
                  << "\n  reloff: " << std::hex << sec[j].reloff
                  << "\n  nreloc: " << std::dec << sec[j].nreloc
                  << "\n  type: 0x" << std::hex << sec[j].type
                  << "\n  attr: 0x" << std::hex << sec[j].attr
                  << "\n";

//        if (sec[j].size) {
//          std::cout << "  contents: ";
//          print_bytes(buf + sec[j].offset, sec[j].size);
//        }

        if (sec[j].reloff) {
          MachRel *rel = (MachRel *)(buf + sec[j].reloff);
          for (i64 k = 0; k < sec[j].nreloc; k++) {
            std::cout << "  reloc: "
                      << "\n   offset: 0x" << rel[k].offset
                      << "\n   idx: 0x" << rel[k].idx
                      << "\n   is_pcrel: " << rel[k].is_pcrel
                      << "\n   length: 0x" << rel[k].length
                      << "\n   is_extern: " << rel[k].is_extern
                      << "\n   type: " << rel[k].type
                      << "\n";
          }
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
    case LC_FUNCTION_STARTS: {
      std::cout << "LC_FUNCTION_STARTS\n";
      LinkEditDataCommand &cmd = *(LinkEditDataCommand *)&lc;
      std::cout << " dataoff: 0x" << cmd.dataoff
                << "\n datasize: 0x" << cmd.datasize
                << "\n data: ";
      print_bytes(buf + cmd.dataoff, cmd.datasize);
      break;
    }
    case LC_MAIN: {
      std::cout << "LC_MAIN\n";
      LinkEditDataCommand &cmd = *(LinkEditDataCommand *)&lc;
      std::cout << " dataoff: 0x" << cmd.dataoff
                << "\n datasize: 0x" << cmd.datasize
                << "\n";
      break;
    }
    case LC_DATA_IN_CODE: {
      std::cout << "LC_DATA_IN_CODE\n";
      LinkEditDataCommand &cmd = *(LinkEditDataCommand *)&lc;
      std::cout << " dataoff: 0x" << cmd.dataoff
                << "\n datasize: 0x" << cmd.datasize
                << "\n";
      break;
    }
    case LC_SOURCE_VERSION: {
      std::cout << "LC_SOURCE_VERSION\n";
      SourceVersionCommand &cmd = *(SourceVersionCommand *)&lc;
      std::cout << " version: 0x" << cmd.version
                << "\n";
      break;
    }
      break;
    case LC_BUILD_VERSION: {
      std::cout << "LC_BUILD_VERSION\n";
      BuildVersionCommand &cmd = *(BuildVersionCommand *)&lc;
      std::cout << " platform: 0x" << cmd.platform
                << "\n minos: 0x" << cmd.minos
                << "\n sdk: 0x" << cmd.sdk
                << "\n ntools: 0x" << cmd.ntools
                << "\n";
      break;
    }
    case LC_VERSION_MIN_MACOSX: {
      std::cout << "LC_VERSION_MIN_MACOSX\n";
      VersionMinCommand &cmd = *(VersionMinCommand *)&lc;
      std::cout << " version: " << (int)cmd.version
                << "\n sdk: " << (int)cmd.sdk
                << "\n";
      break;
    }
    case LC_CODE_SIGNATURE:
      std::cout << "LC_CODE_SIGNATURE\n";
      break;
    default:
      std::cout << "UNKNOWN (0x" << std::hex << lc.cmd << ")\n";
      break;
    }
  }
}

}
