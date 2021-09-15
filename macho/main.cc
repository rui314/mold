#include "mold.h"

#include <cstdlib>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

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

int main(int argc, char **argv) {
  Context ctx;

  if (argc == 1) {
    SyncOut(ctx) << "mold macho stub\n";
    exit(0);
  }

  if (std::string_view(argv[1]) == "-dump") {
    if (argc != 3)
      Fatal(ctx) << "usage: ld64.mold -dump <executable-name>\n";
    dump_file(argv[2]);
    exit(0);
  }

  if (argc != 2)
    Fatal(ctx) << "usage: ld64.mold <output-file>\n";
  ctx.arg.output = argv[1];

  ctx.output_file = std::make_unique<OutputFile>(ctx, ctx.arg.output, 1024, 0777);
  ctx.buf = ctx.output_file->buf;

  MachHeader &hdr = *(MachHeader *)ctx.buf;
  hdr.magic = 0xfeedfacf;
  hdr.cputype = CPU_TYPE_X86_64;
  hdr.cpusubtype = CPU_SUBTYPE_X86_64_ALL;
  hdr.filetype = MH_EXECUTE;
  hdr.ncmds = 0x10;
  hdr.sizeofcmds = 0x558;
  hdr.flags = MH_TWOLEVEL | MH_NOUNDEFS | MH_DYLDLINK | MH_PIE;

  ctx.output_file->close(ctx);

  return 0;
}

}
