#include "mold.h"

namespace mold::macho {

void OutputMachHeader::copy_buf(Context &ctx) {
  MachHeader &mhdr = *(MachHeader *)(ctx.buf + hdr.offset);
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

  cmd.rebase_off = ctx.rebase->hdr.offset;
  cmd.rebase_size = ctx.rebase->hdr.size;

  cmd.bind_off = ctx.bind->hdr.offset;
  cmd.bind_size = ctx.bind->hdr.size;

  cmd.lazy_bind_off = ctx.lazy_bind->hdr.offset;
  cmd.lazy_bind_size = ctx.lazy_bind->hdr.size;

  cmd.export_off = ctx.export_->hdr.offset;
  cmd.export_size = ctx.export_->hdr.size;
  return cmd;
}

static SymtabCommand create_symtab_cmd(Context &ctx) {
  SymtabCommand cmd = {};
  cmd.cmd = LC_SYMTAB;
  cmd.cmdsize = sizeof(cmd);
  cmd.symoff = ctx.symtab->hdr.offset;
  cmd.nsyms = ctx.symtab->contents.size() / sizeof(MachSym);
  cmd.stroff = ctx.strtab->hdr.offset;
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
  cmd.indirectsymoff = ctx.indir_symtab->hdr.offset;
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

static BuildVersionCommand create_build_version_cmd(Context &ctx, i64 ntools) {
  BuildVersionCommand cmd = {};
  cmd.cmd = LC_BUILD_VERSION;
  cmd.cmdsize = sizeof(cmd) + sizeof(BuildToolVersion);
  cmd.platform = PLATFORM_MACOS;
  cmd.minos = 0xb0000;
  cmd.sdk = 0xb0300;
  cmd.ntools = ntools;
  return cmd;
}

static SourceVersionCommand create_source_version_cmd(Context &ctx) {
  SourceVersionCommand cmd = {};
  cmd.cmd = LC_SOURCE_VERSION;
  cmd.cmdsize = sizeof(cmd);
  return cmd;
}

static EntryPointCommand create_main_cmd(Context &ctx) {
  EntryPointCommand cmd = {};
  cmd.cmd = LC_MAIN;
  cmd.cmdsize = sizeof(cmd);
  cmd.entryoff = 0x3f70;
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
  cmd.dataoff = ctx.function_starts->hdr.offset;
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
  i64 last_off = 0;

  auto add = [&](auto x) {
    if (alignof(decltype(x)) == 8 && vec.size() % 8) {
      LoadCommand &cmd = *(LoadCommand *)(vec.data() + last_off);
      cmd.cmdsize += 4;
      vec.resize(vec.size() + 4);
    }

    i64 off = vec.size();
    vec.resize(vec.size() + sizeof(x));
    memcpy(vec.data() + off, &x, sizeof(x));
    last_off = off;
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
  add(create_build_version_cmd(ctx, 1));
  add(BuildToolVersion{3, 0x28a0900});
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
  write_vector(ctx.buf + hdr.offset, contents);
}

OutputSegment::OutputSegment(std::string_view name, u32 prot, u32 flags) {
  assert(name.size() <= sizeof(cmd.segname));

  cmd.cmd = LC_SEGMENT_64;
  memcpy(cmd.segname, name.data(), name.size());
  cmd.maxprot = prot;
  cmd.initprot = prot;
  cmd.flags = flags;
}

// Compute the size of the padding after the load commands.
static i64 compute_text_padding_size(std::span<OutputSection *> sections) {
  u64 addr = 0;

  // Skip the first two sections which are the mach-o header and the
  // load commands.
  for (i64 i = sections.size() - 1; i >= 2; i--) {
    OutputSection &sec = *sections[i];
    addr -= sec.hdr.size;
    addr = align_down(addr, 1 << sec.hdr.p2align);
  }

  addr -= sections[0]->hdr.size;
  addr -= sections[1]->hdr.size;
  return addr % PAGE_SIZE;
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

  i64 offset = 0;

  auto set_offset = [&](OutputSection &sec) {
    sec.update_hdr(ctx);
    offset = align_to(offset, 1 << sec.hdr.p2align);
    sec.hdr.addr = cmd.vmaddr + offset;
    sec.hdr.offset = cmd.fileoff + offset;
    offset += sec.hdr.size;
  };

  if (this == ctx.segments[0]) {
    // In the __TEXT segment, any extra space is put after the load commands
    // so that a post-processing tool can add more load commands there.
    set_offset(*sections[0]);
    set_offset(*sections[1]);
    offset += compute_text_padding_size(sections);
    for (OutputSection *sec : std::span(sections).subspan(2))
      set_offset(*sec);
  } else {
    // In other sections, any extra space is put at end of segment.
    for (OutputSection *sec : sections)
      set_offset(*sec);
  }

  cmd.vmsize = align_to(offset, PAGE_SIZE);
  cmd.filesize =
    (this == ctx.segments.back()) ? offset : align_to(offset, PAGE_SIZE);
}

void OutputSegment::copy_buf(Context &ctx) {
  for (OutputSection *sec : sections)
    sec->copy_buf(ctx);
}

void OutputRebaseSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputBindSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputLazyBindSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputExportSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputFunctionStartsSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputSymtabSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputStrtabSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputIndirectSymtabSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

OutputSection::OutputSection(OutputSegment &parent)
  : parent(parent) {
  memcpy(hdr.segname, parent.cmd.segname, sizeof(parent.cmd.segname));
}

TextSection::TextSection(OutputSegment &parent) : OutputSection(parent) {
  strcpy(hdr.sectname, "__text");
  hdr.p2align = __builtin_ctz(16);
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  hdr.size = contents.size();
}


void TextSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

StubsSection::StubsSection(OutputSegment &parent) : OutputSection(parent) {
  strcpy(hdr.sectname, "__stubs");
  hdr.p2align = __builtin_ctz(2);
  hdr.type = S_SYMBOL_STUBS;
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  hdr.size = contents.size();
  hdr.reserved2 = 6;
}

void StubsSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

StubHelperSection::StubHelperSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__stub_helper");
  hdr.p2align = __builtin_ctz(4);
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  hdr.size = contents.size();
}

void StubHelperSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

CstringSection::CstringSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__cstring");
  hdr.type = S_CSTRING_LITERALS;
  hdr.size = sizeof(contents);
}

void CstringSection::copy_buf(Context &ctx) {
  memcpy(ctx.buf + hdr.offset, contents, sizeof(contents));
}

UnwindInfoSection::UnwindInfoSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__unwind_info");
  hdr.p2align = __builtin_ctz(4);
  hdr.size = contents.size();
}

void UnwindInfoSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

GotSection::GotSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__got");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_NON_LAZY_SYMBOL_POINTERS;
  hdr.size = 8;
  hdr.reserved1 = 1;
}

LaSymbolPtrSection::LaSymbolPtrSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__la_symbol_ptr");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_LAZY_SYMBOL_POINTERS;
  hdr.size = contents.size();
  hdr.reserved1 = 2;
}

void LaSymbolPtrSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

DataSection::DataSection(OutputSegment &parent)
  : OutputSection(parent) {
  strcpy(hdr.sectname, "__data");
  hdr.p2align = __builtin_ctz(8);

  contents.resize(8);
  hdr.size = contents.size();
}

void DataSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

} // namespace mold::macho
