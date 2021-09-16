#include "mold.h"

namespace mold::macho {

void OutputMachHeader::copy_buf(Context &ctx) {
  MachHeader &hdr = *(MachHeader *)(ctx.buf + fileoff);
  memset(&hdr, 0, sizeof(hdr));

  hdr.magic = 0xfeedfacf;
  hdr.cputype = CPU_TYPE_X86_64;
  hdr.cpusubtype = CPU_SUBTYPE_X86_64_ALL;
  hdr.filetype = MH_EXECUTE;
  hdr.ncmds = ctx.load_cmd->ncmds;
  hdr.sizeofcmds = ctx.load_cmd->size;
  hdr.flags = MH_TWOLEVEL | MH_NOUNDEFS | MH_DYLDLINK | MH_PIE;
}

void OutputLoadCommand::update_hdr(Context &ctx) {
  size = 0;
  ncmds = 0;

  for (Chunk *chunk : ctx.chunks) {
    if (chunk->load_cmd.size() > 0) {
      size += chunk->load_cmd.size();
      ncmds++;
    }
  }
}

void OutputLoadCommand::copy_buf(Context &ctx) {
  u8 *buf = ctx.buf + fileoff;
  i64 off = 0;

  for (Chunk *chunk : ctx.chunks) {
    if (chunk->load_cmd.size() > 0) {
      std::vector<u8> &vec = chunk->load_cmd;
      memcpy(buf + off, vec.data(), vec.size());
      off += vec.size();
    }
  }
}

OutputPageZero::OutputPageZero() : Chunk(SYNTHETIC) {
  load_cmd.resize(sizeof(SegmentCommand));
  SegmentCommand &cmd = *(SegmentCommand *)load_cmd.data();
  cmd.cmd = LC_SEGMENT_64;
  cmd.cmdsize = sizeof(cmd);
  strcpy(cmd.segname, "__PAGEZERO");
  cmd.vmsize = 0x100000000;
}

} // namespace mold::macho
