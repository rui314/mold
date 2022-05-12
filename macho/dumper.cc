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

struct ExportEntry {
  std::string name;
  ul32 flags;
  ul64 addr;
};

static void read_trie(std::vector<ExportEntry> &vec, u8 *start, i64 offset = 0,
                      const std::string &prefix = "") {
  u8 *buf = start + offset;

  if (read_uleb(buf)) {
    ExportEntry ent;
    ent.name = prefix;
    ent.flags = read_uleb(buf);
    ent.addr = read_uleb(buf);
    vec.push_back(std::move(ent));
  }

  for (i64 i = 0, n = read_uleb(buf); i < n; i++) {
    std::string suffix((char *)buf);
    buf += suffix.size() + 1;
    i64 off = read_uleb(buf);
    read_trie(vec, start, off, prefix + suffix);
  }
}

void dump_unwind_info(u8 *buf, MachSection &sec) {
  UnwindSectionHeader &hdr = *(UnwindSectionHeader *)(buf + sec.offset);

  std::cout << std::hex << "  Unwind info:"
            << "\n   version: 0x"   << hdr.version
            << "\n   encoding_offset: 0x"   << hdr.encoding_offset
            << "\n   encoding_count: 0x"   << hdr.encoding_count
            << "\n   personality_offset: 0x"   << hdr.personality_offset
            << "\n   personality_count: 0x"   << hdr.personality_count
            << "\n   page_offset: 0x"   << hdr.page_offset
            << "\n   page_count: 0x"   << hdr.page_count;

  std::cout << "\n   encoding:";
  u32 *enc = (u32 *)(buf + sec.offset + hdr.encoding_offset);
  for (i64 i = 0; i < hdr.encoding_count; i++)
    std::cout << std::hex << "\n    0x" << enc[i];

  std::cout << "\n   personality:";
  u32 *pers = (u32 *)(buf + sec.offset + hdr.personality_offset);
  for (i64 i = 0; i < hdr.personality_count; i++)
    std::cout << std::hex << "\n    0x" << pers[i];

  UnwindFirstLevelPage *ent =
    (UnwindFirstLevelPage *)(buf + sec.offset + hdr.page_offset);

  for (i64 i = 0; i < hdr.page_count; i++) {
    std::cout << std::hex << "\n   function:"  
              << "\n    func_addr: 0x"   << ent[i].func_addr
              << "\n    page_offset: 0x"   << ent[i].page_offset
              << "\n    lsda_offset: 0x"   << ent[i].lsda_offset;

    if (i != hdr.page_count - 1) {
      UnwindLsdaEntry *lsda =
        (UnwindLsdaEntry *)(buf + sec.offset + ent[i].lsda_offset);
      i64 lsda_size = ent[i + 1].lsda_offset - ent[i].lsda_offset;
      for (i64 j = 0; j < lsda_size / sizeof(UnwindLsdaEntry); j++)
        std::cout << std::hex
                  << "\n    lsda:"
                  << "\n     func_addr: 0x" << lsda[j].func_addr
                  << "\n     lsda_addr: 0x" << lsda[j].lsda_addr;
    }

    if (ent[i].page_offset == 0)
      break;

    u8 *addr = buf + sec.offset + ent[i].page_offset;

    switch (u32 kind = *addr; kind) {
    case UNWIND_SECOND_LEVEL_REGULAR: {
      std::cout << "\n    UNWIND_SECOND_LEVEL_REGULAR:"  ;
      break;
    }
    case UNWIND_SECOND_LEVEL_COMPRESSED: {
      std::cout << "\n    UNWIND_SECOND_LEVEL_COMPRESSED"  ;
      UnwindSecondLevelPage &hdr2 = *(UnwindSecondLevelPage *)addr;
      std::cout << std::hex
                << "\n     page_offset: 0x"   << hdr2.page_offset
                << "\n     page_count: 0x"   << hdr2.page_count
                << "\n     encoding_offset: 0x"   << hdr2.encoding_offset
                << "\n     encoding_count: 0x"   << hdr2.encoding_count;

      UnwindPageEntry *ent2 = (UnwindPageEntry *)(addr + hdr2.page_offset);
      for (i64 j = 0; j < hdr2.page_count; j++)
        std::cout << std::hex << "\n      ent 0x"  
                  << (ent[i].func_addr + ent2[j].func_addr)
                  << " 0x" << ent2[j].encoding;

      u32 *enc = (u32 *)(addr + hdr2.encoding_offset);
      for (i64 j = 0; j < hdr2.encoding_count; j++)
        std::cout << std::hex << "\n      0x"   << enc[j];
      break;
    }
    default:
      std::cout << "\n    bad 2nd-level unwind info header: " << kind;
    }
  }

  std::cout << "\n";
}

void dump_compact_unwind(u8 *buf, MachSection &sec) {
  CompactUnwindEntry *ent = (CompactUnwindEntry *)(buf + sec.offset);
  i64 nentry = sec.size / sizeof(CompactUnwindEntry);

  std::cout << "  Compact unwind:"
            << "\n   num_entry: " << nentry;

  for (i64 i = 0; i < nentry; i++) {
    std::cout << std::hex
              << "\n   entry: 0x" << (i * sizeof(CompactUnwindEntry))
              << "\n    code_start: 0x" << ent[i].code_start
              << "\n    code_len: 0x" << ent[i].code_len
              << "\n    encoding: 0x" << ent[i].encoding
              << "\n    personality: 0x" << ent[i].personality
              << "\n    lsda: 0x" << ent[i].lsda;
  }

  std::cout << "\n";
}

void dump_file(std::string path) {
  u8 *buf = open_file(path);
  if (!buf) {
    std::cerr << "cannot open " << path << "\n";
    exit(1);
  }

  MachHeader &hdr = *(MachHeader *)buf;
  std::cout << "magic: 0x" << std::hex << hdr.magic
            << "\ncputype: 0x" << (u32)hdr.cputype
            << "\ncpusubtype: 0x" << (u32)hdr.cpusubtype
            << "\nfiletype: 0x" << (u32)hdr.filetype
            << "\nncmds: 0x" << hdr.ncmds
            << "\nsizeofcmds: 0x" << hdr.sizeofcmds
            << "\nflags: 0x" << hdr.flags
            << "\n\n";

  u8 *p = buf + sizeof(MachHeader);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    std::cout << "fileoff: 0x" << std::hex << (p - buf) << "\n";

    LoadCommand &lc = *(LoadCommand *)p;
    p += lc.cmdsize;

    auto print_dylib = [&] {
      DylibCommand &cmd = *(DylibCommand *)&lc;
      std::cout << " cmdsize: 0x" << cmd.cmdsize
                << "\n nameoff: 0x" << cmd.nameoff
                << "\n timestamp: 0x" << cmd.timestamp
                << "\n current_version: 0x" << cmd.current_version
                << "\n compatibility_version: 0x" << cmd.compatibility_version
                << "\n data: " << (char *)((char *)&cmd + cmd.nameoff)
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
                << "\n symdata: ";
      print_bytes(buf + cmd.symoff, cmd.nsyms * sizeof(MachSym));

      std::cout << " strdata: ";
      print_bytes(buf + cmd.stroff, cmd.strsize);

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
                << "\n segname: " << cmd.get_segname()
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
        std::cout << " section:\n  sectname: " << sec[j].get_sectname()
                  << "\n  segname: " << sec[j].get_segname()
                  << "\n  addr: 0x" << std::hex << sec[j].addr
                  << "\n  size: 0x" << sec[j].size
                  << "\n  offset: 0x" << sec[j].offset
                  << "\n  p2align: " << std::dec << sec[j].p2align
                  << "\n  reloff: " << std::hex << sec[j].reloff
                  << "\n  nreloc: " << std::dec << sec[j].nreloc
                  << "\n  type: 0x" << std::hex << (u32)sec[j].type
                  << "\n  attr: 0x" << std::hex << sec[j].attr
                  << "\n";

        if (sec[j].type != S_ZEROFILL) {
          std::cout << "  contents: ";
          print_bytes(buf + sec[j].offset, sec[j].size);
        }

        if (sec[j].reloff) {
          MachRel *rel = (MachRel *)(buf + sec[j].reloff);
          for (i64 k = 0; k < sec[j].nreloc; k++) {
            std::cout << "  reloc: "
                      << "\n   offset: 0x" << rel[k].offset
                      << "\n   idx: 0x" << rel[k].idx
                      << "\n   is_pcrel: " << (u32)rel[k].is_pcrel
                      << "\n   p2size: 0x" << (u32)rel[k].p2size
                      << "\n   is_extern: " << (u32)rel[k].is_extern
                      << "\n   type: " << (u32)rel[k].type
                      << "\n";
          }
        }

        if (sec[j].match("__TEXT", "__unwind_info"))
          dump_unwind_info(buf, sec[j]);

        if (sec[j].match("__LD", "__compact_unwind"))
          dump_compact_unwind(buf, sec[j]);
      }
      break;
    }
    case LC_UUID: {
      std::cout << "LC_UUID\n";
      UUIDCommand &cmd = *(UUIDCommand *)&lc;
      std::cout << " data: ";
      print_bytes(cmd.uuid, sizeof(cmd.uuid));
      break;
    }
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

        std::vector<ExportEntry> vec;
        read_trie(vec, buf + cmd.export_off);
        for (ExportEntry &ent : vec)
          std::cout << "  export_sym: " << ent.name << " 0x" << ent.addr << "\n";
      }
      break;
    }
    case LC_FUNCTION_STARTS: {
      std::cout << "LC_FUNCTION_STARTS\n";
      LinkEditDataCommand &cmd = *(LinkEditDataCommand *)&lc;
      std::cout << " dataoff: 0x" << cmd.dataoff
                << "\n datasize: 0x" << cmd.datasize
                << "\n data:";

      u8 *p = buf + cmd.dataoff;
      u64 addr = 0;
      for (;;) {
        u64 delta = read_uleb(p);
        if (!delta)
          break;
        addr += delta;
        std::cout << std::hex << " 0x" << addr;
      }
      std::cout << "\n";
      break;
    }
    case LC_MAIN: {
      std::cout << "LC_MAIN\n";
      EntryPointCommand &cmd = *(EntryPointCommand *)&lc;
      std::cout << " cmdsize: 0x" << cmd.cmdsize
                << "\n entryoff: 0x" << cmd.entryoff
                << "\n stacksize: 0x" << cmd.stacksize
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
      std::cout << " cmdsize: 0x" << cmd.cmdsize
                << "\n platform: 0x" << cmd.platform
                << "\n minos: 0x" << cmd.minos
                << "\n sdk: 0x" << cmd.sdk
                << "\n ntools: 0x" << cmd.ntools
                << "\n";

      BuildToolVersion *tools =
        (BuildToolVersion *)((u8 *)&lc + sizeof(BuildVersionCommand));
      for (i64 i = 0; i < cmd.ntools; i++)
        std::cout << "  tool: 0x" << tools[i].tool
                  << "\n  version: 0x" << tools[i].version
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
    case LC_CODE_SIGNATURE: {
      std::cout << "LC_CODE_SIGNATURE\n";
      LinkEditDataCommand &cmd = *(LinkEditDataCommand *)&lc;

      CodeSignatureHeader &sig = *(CodeSignatureHeader *)(buf + cmd.dataoff);
      std::cout << " magic: " << sig.magic
                << "\n length: " << sig.length
                << "\n count: " << sig.count
                << "\n";

      for (i64 i = 0; i < sig.count; i++) {
        CodeSignatureBlobIndex &idx = ((CodeSignatureBlobIndex *)(&sig + 1))[i];
        std::cout << " idx type: " << idx.type
                  << "\n idx offset: " << idx.offset
                  << "\n";

        CodeSignatureDirectory &dir =
          *(CodeSignatureDirectory *)((u8 *)&sig + idx.offset);
        std::cout << std::hex
                  << " magic: 0x" << dir.magic
                  << "\n version: 0x" << dir.version
                  << "\n flags: 0x" << dir.flags
                  << "\n hash_offset: 0x" << dir.hash_offset
                  << "\n n_code_slots: 0x" << dir.n_code_slots
                  << "\n hash_size: 0x" << (u32)dir.hash_size
                  << "\n hash_type: 0x" << (u32)dir.hash_type
                  << "\n page_size: 0x" << (1 << dir.page_size)
                  << "\n";
      }

      break;
    }
    default:
      std::cout << "UNKNOWN (0x" << std::hex << lc.cmd << ")\n";
      break;
    }
  }
}

} // namespace mold::macho
