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

OutputSegment::OutputSegment(std::string_view name, u32 prot, u32 flags)
  : Chunk(REGULAR) {
  p2align = __builtin_ctz(PAGE_SIZE);

  load_cmd.resize(sizeof(SegmentCommand));
  SegmentCommand &cmd = *(SegmentCommand *)load_cmd.data();

  assert(name.size() <= sizeof(cmd.segname));

  cmd.cmd = LC_SEGMENT_64;
  memcpy(cmd.segname, name.data(), name.size());
  cmd.maxprot = prot;
  cmd.initprot = prot;
  cmd.flags = flags;
}

void OutputSegment::update_hdr(Context &ctx) {
  SegmentCommand &cmd = *(SegmentCommand *)load_cmd.data();
  cmd.cmdsize = sizeof(SegmentCommand) + sizeof(MachSection) * sections.size();
  cmd.nsects = sections.size();

  load_cmd.resize(sizeof(cmd) + sizeof(MachSection) * sections.size());
  MachSection *hdrs = (MachSection *)(load_cmd.data() + sizeof(cmd));

  for (i64 i = 0; i < sections.size(); i++) {
    OutputSection &sec = *sections[i];
    sec.update_hdr(ctx);
    hdrs[i] = sec.hdr;
  }

  i64 fileoff = 0;
  for (OutputSection *sec : sections) {
    fileoff = align_to(fileoff, 1 << sec->hdr.p2align);
    sec->hdr.offset = fileoff;
    fileoff += sec->hdr.size;
  }
  size = fileoff;
}

void OutputSegment::copy_buf(Context &ctx) {
  for (OutputSection *sec : sections)
    sec->copy_buf(ctx);
}

OutputSection::OutputSection(OutputSegment &parent, std::string_view name)
  : parent(parent) {
  assert(name.size() <= sizeof(hdr.sectname));
  memcpy(hdr.sectname, name.data(), name.size());

  SegmentCommand &cmd = *(SegmentCommand *)parent.load_cmd.data();
  memcpy(hdr.segname, cmd.segname, sizeof(cmd.segname));
}

TextSection::TextSection(OutputSegment &parent)
  : OutputSection(parent, "__text") {
  hdr.p2align = __builtin_ctz(8);
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;

  contents = {
    0x55, 0x48, 0x89, 0xe5, 0x48, 0x8d, 0x3d, 0x43, 0x00, 0x00, 0x00,
    0xb0, 0x00, 0xe8, 0x1c, 0x00, 0x00, 0x00, 0x5d, 0xc3, 0x66, 0x2e,
    0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00, 0x66, 0x90, 0x55,
    0x48, 0x89, 0xe5, 0xe8, 0xd7, 0xff, 0xff, 0xff, 0x31, 0xc0, 0x5d,
    0xc3,
  };

  hdr.size = contents.size();
}


void TextSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.fileoff + hdr.offset, contents);
}

StubsSection::StubsSection(OutputSegment &parent)
  : OutputSection(parent, "__stubs") {
  hdr.p2align = __builtin_ctz(2);
  hdr.type = S_SYMBOL_STUBS;
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  contents = {0x40, 0x7c, 0x25, 0xff, 0x00, 0x00};
  hdr.size = contents.size();
}

void StubsSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.fileoff + hdr.offset, contents);
}

StubHelperSection::StubHelperSection(OutputSegment &parent)
  : OutputSection(parent, "__stub_helper") {
  hdr.p2align = __builtin_ctz(4);
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;

  contents = {
    0x7d, 0x1d, 0x8d, 0x4c, 0x41, 0x00, 0x00, 0x40,
    0x6d, 0x25, 0xff, 0x53, 0x90, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x68, 0xff, 0xe6, 0xe9, 0x00,
    0xff, 0xff,
  };

  hdr.size = contents.size();
}

void StubHelperSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.fileoff + hdr.offset, contents);
}

CstringSection::CstringSection(OutputSegment &parent)
  : OutputSection(parent, "__cstring") {
  hdr.p2align = __builtin_ctz(4);
  hdr.type = S_CSTRING_LITERALS;
  hdr.size = sizeof(contents);
}

void CstringSection::copy_buf(Context &ctx) {
  memcpy(ctx.buf + parent.fileoff + hdr.offset, contents, sizeof(contents));
}

GotSection::GotSection(OutputSegment &parent)
  : OutputSection(parent, "__cstring") {
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_NON_LAZY_SYMBOL_POINTERS;
  hdr.size = 8;
}

LaSymbolPtrSection::LaSymbolPtrSection(OutputSegment &parent)
  : OutputSection(parent, "__la_symbol_ptr") {
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_LAZY_SYMBOL_POINTERS;
  contents = {0x94, 0x3f, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00};
  hdr.size = contents.size();
}

void LaSymbolPtrSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.fileoff + hdr.offset, contents);
}

DataSection::DataSection(OutputSegment &parent)
  : OutputSection(parent, "__data") {
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_LAZY_SYMBOL_POINTERS;

  contents.resize(8);
  hdr.size = contents.size();
}

void DataSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.fileoff + hdr.offset, contents);
}

} // namespace mold::macho
