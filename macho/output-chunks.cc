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

static SegmentCommand create_page_zero_cmd(Context &ctx) {
  SegmentCommand cmd = {};
  cmd.cmd = LC_SEGMENT_64;
  cmd.cmdsize = sizeof(SegmentCommand);
  strcpy(cmd.segname, "__PAGEZERO");
  cmd.vmsize = PAGE_ZERO_SIZE;
  return cmd;
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

  auto add = [&](auto x) {
    i64 off = vec.size();
    vec.resize(vec.size() + sizeof(x));
    memcpy(vec.data() + off, &x, sizeof(x));
  };

  auto add_cmd = [&](auto x) {
    add(x);
    ncmds++;
  };

  add_cmd(create_page_zero_cmd(ctx));

  // Add LC_SEGMENT_64 comamnds
  for (OutputSegment *seg : ctx.segments) {
    add_cmd(seg->cmd);
    for (OutputSection *sec : seg->sections)
      if (!sec->is_hidden)
        add(sec->hdr);
  }

  add_cmd(create_dyld_info_only_cmd(ctx));
  add_cmd(create_symtab_cmd(ctx));
  add_cmd(create_dysymtab_cmd(ctx));
  add_cmd(create_dylinker_cmd(ctx));
  add_cmd(create_uuid_cmd(ctx));

  add_cmd(create_build_version_cmd(ctx, 1));
  add(BuildToolVersion{3, 0x28a0900});

  add_cmd(create_source_version_cmd(ctx));
  add_cmd(create_main_cmd(ctx));
  add_cmd(create_load_dylib_cmd(ctx));
  add_cmd(create_function_starts_cmd(ctx));
  add_cmd(create_data_in_code_cmd(ctx));

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

RebaseEncoder::RebaseEncoder() {
  buf.push_back(REBASE_OPCODE_SET_TYPE_IMM | REBASE_TYPE_POINTER);
}

void RebaseEncoder::add(i64 seg_idx, i64 offset) {
  assert(seg_idx < 16);

  // Accumulate consecutive base relocations
  if (seg_idx == last_seg && offset == last_off + 8) {
    last_off = offset;
    times++;
    return;
  }

  // Flush the accumulated base relocations
  flush();

  // Advance the cursor
  if (seg_idx != last_seg) {
    buf.push_back(REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg_idx);
    encode_uleb(buf, offset);
  } else {
    i64 dist = offset - last_off;
    assert(dist >= 0);

    if (dist % 8 == 0 && dist < 128) {
      buf.push_back(REBASE_OPCODE_ADD_ADDR_IMM_SCALED | (dist >> 3));
    } else {
      buf.push_back(REBASE_OPCODE_ADD_ADDR_ULEB);
      encode_uleb(buf, dist);
    }
  }

  last_seg = seg_idx;
  last_off = offset;
  times = 1;
}

void RebaseEncoder::flush() {
  if (times == 0)
    return;

  if (times < 16) {
    buf.push_back(REBASE_OPCODE_DO_REBASE_IMM_TIMES | times);
  } else {
    buf.push_back(REBASE_OPCODE_DO_REBASE_ULEB_TIMES);
    encode_uleb(buf, times);
  }
  times = 0;
}

void RebaseEncoder::finish() {
  flush();
  buf.push_back(REBASE_OPCODE_DONE);
}

OutputRebaseSection::OutputRebaseSection(OutputSegment &parent)
  : OutputSection(parent) {
  is_hidden = true;

  RebaseEncoder enc;
  enc.add(3, 0);
  enc.finish();
  contents = enc.buf;
  hdr.size = align_to(contents.size(), 8);
}

void OutputRebaseSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

BindEncoder::BindEncoder(bool is_lazy) {
  if (!is_lazy)
    buf.push_back(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER);
}

void BindEncoder::add(i64 dylib_idx, std::string_view sym, i64 flags,
                      i64 seg_idx, i64 offset) {
  if (last_dylib != dylib_idx) {
    if (dylib_idx < 16) {
      buf.push_back(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | dylib_idx);
    } else {
      buf.push_back(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
      encode_uleb(buf, dylib_idx);
    }
  }

  if (last_sym != sym || last_flags != flags) {
    assert(flags < 16);
    buf.push_back(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
    buf.insert(buf.end(), (u8 *)sym.data(), (u8 *)(sym.data() + sym.size()));
    buf.push_back('\0');
  }

  if (last_seg != seg_idx || last_off != offset) {
    assert(seg_idx < 16);
    buf.push_back(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg_idx);
    encode_uleb(buf, offset);
  }

  buf.push_back(BIND_OPCODE_DO_BIND);

  last_dylib = dylib_idx;
  last_sym = sym;
  last_flags = flags;
  last_seg = seg_idx;
  last_off = offset;
}

void BindEncoder::finish() {
  buf.push_back(BIND_OPCODE_DONE);
}

OutputBindSection::OutputBindSection(OutputSegment &parent)
  : OutputSection(parent) {
  is_hidden = true;

  BindEncoder enc(false);
  enc.add(1, "dyld_stub_binder", 0, 2, 0);
  enc.finish();

  contents = enc.buf;
  hdr.size = align_to(contents.size(), 8);
}

void OutputBindSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

OutputLazyBindSection::OutputLazyBindSection(OutputSegment &parent)
  : OutputSection(parent) {
  is_hidden = true;

  BindEncoder enc(true);
  enc.add(1, "_printf", 0, 3, 0);
  enc.finish();

  contents = enc.buf;
  hdr.size = align_to(contents.size(), 8);
}

void OutputLazyBindSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void ExportEncoder::add(std::string_view name, u32 flags, u64 addr) {
  entries.push_back({name, flags, addr});
}

i64 ExportEncoder::finish() {
  sort(entries, [](const Entry &a, const Entry &b) {
    return a.name < b.name;
  });

  construct_trie(root, entries, 0);

  i64 size = set_offset(root, 0);
  for (;;) {
    i64 sz = set_offset(root, 0);
    if (sz == size)
      return sz;
    size = sz;
  }
}

void
ExportEncoder::construct_trie(TrieNode &parent, std::span<Entry> entries, i64 len) {
  if (entries.empty())
    return;

  if (entries[0].name.size() == len) {
    parent.is_leaf = true;
    parent.flags = entries[0].flags;
    parent.addr = entries[0].addr;
    entries = entries.subspan(1);
  }

  for (i64 i = 0; i < entries.size();) {
    i64 j = i + 1;
    u8 c = entries[i].name[len];

    while (j < entries.size() && c == entries[j].name[len])
      j++;

    std::span<Entry> subspan = entries.subspan(i, j - i);
    i64 new_len = common_prefix_len(subspan, len + 1);

    TrieNode *child = new TrieNode;
    child->prefix = entries[i].name.substr(len, new_len - len);
    construct_trie(*child, subspan, new_len);
    parent.children[c].reset(child);

    i = j;
  }
}

i64 ExportEncoder::common_prefix_len(std::span<Entry> entries, i64 len) {
  for (; len < entries[0].name.size(); len++)
    for (Entry &ent : entries.subspan(1))
      if (ent.name.size() == len || ent.name[len] != entries[0].name[len])
        return len;
  return len;
}

i64 ExportEncoder::set_offset(TrieNode &node, i64 offset) {
  node.offset = offset;

  i64 size = 0;
  if (node.is_leaf) {
    size = uleb_size(node.flags) + uleb_size(node.addr);
    size += uleb_size(size);
  } else {
    size = 1;
  }

  size++; // # of children

  for (std::unique_ptr<TrieNode> &child : node.children) {
    if (child) {
      // +1 for NUL byte
      size += child->prefix.size() + 1 + uleb_size(child->offset);
    }
  }

  offset += size;

  for (std::unique_ptr<TrieNode> &child : node.children)
    if (child)
      offset = set_offset(*child, offset);
  return offset;
}

void ExportEncoder::write_trie(u8 *start, TrieNode &node) {
  u8 *buf = start + node.offset;

  if (node.is_leaf) {
    buf += write_uleb(buf, uleb_size(node.flags) + uleb_size(node.addr));
    buf += write_uleb(buf, node.flags);
    buf += write_uleb(buf, node.addr);
  } else {
    *buf++ = 0;
  }

  u8 *num_children = buf++;
  *num_children = 0;

  for (std::unique_ptr<TrieNode> &child : node.children) {
    if (child) {
      *num_children += 1;
      buf += write_string(buf, child->prefix);
      buf += write_uleb(buf, child->offset);
    }
  }

  for (std::unique_ptr<TrieNode> &child : node.children)
    if (child)
      write_trie(start, *child);
}

OutputExportSection::OutputExportSection(OutputSegment &parent)
  : OutputSection(parent) {
  is_hidden = true;
  enc.add("__mh_execute_header", 0, 0);
  enc.add("_hello", 0, 0x3f50);
  enc.add("_main", 0, 0x3f70);
  hdr.size = align_to(enc.finish(), 8);
}

void OutputExportSection::copy_buf(Context &ctx) {
  enc.write_trie(ctx.buf + hdr.offset);
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
