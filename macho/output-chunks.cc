#include "mold.h"

namespace mold::macho {

void OutputMachHeader::copy_buf(Context &ctx) {
  MachHeader &mhdr = *(MachHeader *)(ctx.buf + parent.cmd.fileoff + hdr.offset);
  memset(&mhdr, 0, sizeof(mhdr));

  mhdr.magic = 0xfeedfacf;
  mhdr.cputype = CPU_TYPE_X86_64;
  mhdr.cpusubtype = CPU_SUBTYPE_X86_64_ALL;
  mhdr.filetype = MH_EXECUTE;
  mhdr.ncmds = ctx.load_cmd->ncmds;
  mhdr.sizeofcmds = ctx.load_cmd->hdr.size;
  mhdr.flags = MH_TWOLEVEL | MH_NOUNDEFS | MH_DYLDLINK | MH_PIE;
}

static DyldInfoCommand create_dyld_info_only_cmd(Context &ctx) {
  DyldInfoCommand cmd = {};
  cmd.cmd = LC_DYLD_INFO_ONLY;
  cmd.cmdsize = sizeof(cmd);

  i64 off = ctx.linkedit_seg->cmd.fileoff;

  cmd.rebase_off = off + ctx.rebase->hdr.offset;
  cmd.rebase_size = ctx.rebase->contents.size();

  cmd.bind_size = off + ctx.bind->hdr.offset;
  off += ctx.bind->contents.size();

  cmd.lazy_bind_size = off + ctx.lazy_bind->hdr.offset;
  off += ctx.lazy_bind->contents.size();

  cmd.export_size = ctx.export_->hdr.offset;
  off += ctx.export_->contents.size();
  return cmd;
}

static SymtabCommand create_symtab_cmd(Context &ctx) {
  SymtabCommand cmd = {};
  cmd.cmd = LC_SYMTAB;
  cmd.cmdsize = sizeof(cmd);
  cmd.symoff = ctx.linkedit_seg->cmd.fileoff + ctx.symtab->hdr.offset;
  cmd.nsyms = ctx.symtab->contents.size() / sizeof(MachSym);
  cmd.stroff = ctx.linkedit_seg->cmd.fileoff + ctx.strtab->hdr.offset;
  cmd.strsize = ctx.strtab->contents.size();
  return cmd;
}

static DysymtabCommand create_dysymtab_cmd(Context &ctx) {
  DysymtabCommand cmd = {};
  cmd.cmd = LC_DYSYMTAB;
  cmd.cmdsize = sizeof(cmd);
  cmd.nlocalsym = 1;
  cmd.iextdefsym = 1;
  cmd.nextdefsym = 3;
  cmd.iundefsym = 4;
  cmd.nundefsym = 2;
  cmd.indirectsymoff =
    ctx.linkedit_seg->cmd.fileoff + ctx.indir_symtab->hdr.offset;
  cmd.nindirectsyms = ctx.indir_symtab->hdr.size / 4;
  return cmd;
}

static DylinkerCommand create_dylinker_cmd(Context &ctx) {
  DylinkerCommand cmd = {};
  cmd.cmd = LC_LOAD_DYLINKER;
  cmd.cmdsize = sizeof(cmd);
  cmd.nameoff = offsetof(DylinkerCommand, name);
  strcpy(cmd.name, "/usr/lib/dyld");
  return cmd;
}

static UUIDCommand create_uuid_cmd(Context &ctx) {
  UUIDCommand cmd = {};
  cmd.cmd = LC_UUID;
  cmd.cmdsize = sizeof(cmd);
  memcpy(cmd.uuid,
         "\x65\x35\x2b\xae\x49\x1d\x34\xa5\xa9\x1d\x85\xfa\x37\x4b\xb9\xb2",
         16);
  return cmd;
}

static BuildVersionCommand create_build_version_cmd(Context &ctx) {
  BuildVersionCommand cmd = {};
  cmd.cmd = LC_BUILD_VERSION;
  cmd.cmdsize = sizeof(cmd);
  cmd.platform = PLATFORM_MACOS;
  cmd.minos = 0xb0000;
  cmd.sdk = 0xb0300;
  cmd.ntools = TOOL_CLANG;
  return cmd;
}

static SourceVersionCommand create_source_version_cmd(Context &ctx) {
  SourceVersionCommand cmd = {};
  cmd.cmd = LC_SOURCE_VERSION;
  cmd.cmdsize = sizeof(cmd);
  return cmd;
}

static LinkEditDataCommand create_main_cmd(Context &ctx) {
  LinkEditDataCommand cmd = {};
  cmd.cmd = LC_MAIN;
  cmd.cmdsize = sizeof(cmd);
  cmd.dataoff = 0x3f70;
  return cmd;
}

static DylibCommand create_load_dylib_cmd(Context &ctx) {
  DylibCommand cmd = {};
  cmd.cmd = LC_LOAD_DYLIB;
  cmd.cmdsize = sizeof(cmd);
  cmd.nameoff = offsetof(DylibCommand, name);
  cmd.timestamp = 2;
  cmd.current_version = 0x50c6405;
  cmd.compatibility_version = 0x10000;
  strcpy(cmd.name, "/usr/lib/libSystem.B.dylib");
  return cmd;
}

static LinkEditDataCommand create_function_starts_cmd(Context &ctx) {
  LinkEditDataCommand cmd = {};
  cmd.cmd = LC_FUNCTION_STARTS;
  cmd.cmdsize = sizeof(cmd);
  cmd.dataoff = ctx.linkedit_seg->cmd.fileoff + ctx.function_starts->hdr.offset;
  cmd.datasize = ctx.function_starts->hdr.size;
  return cmd;
}

static LinkEditDataCommand create_data_in_code_cmd(Context &ctx) {
  LinkEditDataCommand cmd = {};
  cmd.cmd = LC_DATA_IN_CODE;
  cmd.cmdsize = sizeof(cmd);
  cmd.dataoff = 0xc070;
  return cmd;
}

static std::pair<std::vector<u8>, i64>
create_load_commands(Context &ctx) {
  std::vector<u8> vec;
  i64 ncmds = 0;

  auto add = [&](auto x) {
    i64 off = vec.size();
    vec.resize(vec.size() + sizeof(x));
    memcpy(vec.data() + off, &x, sizeof(x));
  };

  // Add a PAGE_ZERO command
  SegmentCommand zero = {};
  zero.cmd = LC_SEGMENT_64;
  zero.cmdsize = sizeof(SegmentCommand);
  strcpy(zero.segname, "__PAGEZERO");
  zero.vmsize = PAGE_ZERO_SIZE;

  add(zero);
  ncmds++;

  // Add LC_SEGMENT_64 comamnds
  for (OutputSegment *seg : ctx.segments) {
    add(seg->cmd);
    ncmds++;

    for (OutputSection *sec : seg->sections)
      if (!sec->is_hidden)
        add(sec->hdr);
  }

  // LC_DYLD_INFO_ONLY
  add(create_dyld_info_only_cmd(ctx));
  ncmds++;

  // LC_SYMTAB
  add(create_symtab_cmd(ctx));
  ncmds++;

  // LC_DYSYMTAB
  add(create_dysymtab_cmd(ctx));
  ncmds++;

  // LC_LOAD_DYLINKER
  add(create_dylinker_cmd(ctx));
  ncmds++;

  // LC_UUID
  add(create_uuid_cmd(ctx));
  ncmds++;

  // LC_BUILD_VERSION
  add(create_build_version_cmd(ctx));
  ncmds++;

  // LC_SOURCE_VERSION
  add(create_source_version_cmd(ctx));
  ncmds++;

  // LC_MAIN
  add(create_main_cmd(ctx));
  ncmds++;

  // LC_LOAD_DYLIB
  add(create_load_dylib_cmd(ctx));
  ncmds++;

  // LC_FUNCTION_STARTS
  add(create_function_starts_cmd(ctx));
  ncmds++;

  // LC_DATA_IN_CODE
  add(create_data_in_code_cmd(ctx));
  ncmds++;

  return {vec, ncmds};
}

void OutputLoadCommand::update_hdr(Context &ctx) {
  std::vector<u8> contents;
  std::tie(contents, ncmds) = create_load_commands(ctx);
  hdr.size = contents.size();
}

void OutputLoadCommand::copy_buf(Context &ctx) {
  std::vector<u8> contents;
  std::tie(contents, std::ignore) = create_load_commands(ctx);
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

OutputSegment::OutputSegment(std::string_view name, u32 prot, u32 flags) {
  assert(name.size() <= sizeof(cmd.segname));

  cmd.cmd = LC_SEGMENT_64;
  memcpy(cmd.segname, name.data(), name.size());
  cmd.maxprot = prot;
  cmd.initprot = prot;
  cmd.flags = flags;
}

void OutputSegment::update_hdr(Context &ctx) {
  cmd.cmdsize = sizeof(SegmentCommand);
  cmd.nsects = 0;

  for (OutputSection *sec : sections) {
    if (!sec->is_hidden) {
      cmd.cmdsize += sizeof(MachSection);
      cmd.nsects++;
    }
  }

  i64 fileoff = 0;
  for (OutputSection *sec : sections) {
    sec->update_hdr(ctx);
    fileoff = align_to(fileoff, 1 << sec->hdr.p2align);
    sec->hdr.offset = fileoff;
    fileoff += sec->hdr.size;
  }

  cmd.vmsize = fileoff;
  cmd.filesize = fileoff;
}

void OutputSegment::copy_buf(Context &ctx) {
  for (OutputSection *sec : sections)
    sec->copy_buf(ctx);
}

void OutputRebaseSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

void OutputBindSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

void OutputLazyBindSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

void OutputExportSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

void OutputFunctionStartsSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

void OutputSymtabSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

void OutputStrtabSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

void OutputIndirectSymtabSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

OutputSection::OutputSection(OutputSegment &parent)
  : parent(parent) {
  memcpy(hdr.segname, parent.cmd.segname, sizeof(parent.cmd.segname));
}

TextSection::TextSection(OutputSegment &parent) : OutputSection(parent) {
  strcpy(hdr.sectname, "__text");
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
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

StubsSection::StubsSection(OutputSegment &parent) : OutputSection(parent) {
  strcpy(hdr.sectname, "__stubs");
  hdr.p2align = __builtin_ctz(2);
  hdr.type = S_SYMBOL_STUBS;
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  contents = {0x40, 0x7c, 0x25, 0xff, 0x00, 0x00};
  hdr.size = contents.size();
}

void StubsSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

StubHelperSection::StubHelperSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__stub_helper");
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
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

CstringSection::CstringSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__cstring");
  hdr.p2align = __builtin_ctz(4);
  hdr.type = S_CSTRING_LITERALS;
  hdr.size = sizeof(contents);
}

void CstringSection::copy_buf(Context &ctx) {
  memcpy(ctx.buf + parent.cmd.fileoff + hdr.offset, contents, sizeof(contents));
}

GotSection::GotSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__cstring");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_NON_LAZY_SYMBOL_POINTERS;
  hdr.size = 8;
}

LaSymbolPtrSection::LaSymbolPtrSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__la_symbol_ptr");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_LAZY_SYMBOL_POINTERS;
  contents = {0x94, 0x3f, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00};
  hdr.size = contents.size();
}

void LaSymbolPtrSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

DataSection::DataSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__data");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_LAZY_SYMBOL_POINTERS;

  contents.resize(8);
  hdr.size = contents.size();
}

void DataSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + parent.cmd.fileoff + hdr.offset, contents);
}

} // namespace mold::macho
