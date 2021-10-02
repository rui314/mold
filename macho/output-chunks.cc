#include "mold.h"

namespace mold::macho {

void OutputMachHeader::copy_buf(Context &ctx) {
  MachHeader &mhdr = *(MachHeader *)(ctx.buf + hdr.offset);
  memset(&mhdr, 0, sizeof(mhdr));

  mhdr.magic = 0xfeedfacf;
  mhdr.cputype = CPU_TYPE_X86_64;
  mhdr.cpusubtype = CPU_SUBTYPE_X86_64_ALL;
  mhdr.filetype = MH_EXECUTE;
  mhdr.ncmds = ctx.load_cmd.ncmds;
  mhdr.sizeofcmds = ctx.load_cmd.hdr.size;
  mhdr.flags = MH_TWOLEVEL | MH_NOUNDEFS | MH_DYLDLINK | MH_PIE;
}

static std::vector<u8> create_page_zero_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(SegmentCommand));
  SegmentCommand &cmd = *(SegmentCommand *)buf.data();

  cmd.cmd = LC_SEGMENT_64;
  cmd.cmdsize = buf.size();
  strcpy(cmd.segname, "__PAGEZERO");
  cmd.vmsize = PAGE_ZERO_SIZE;
  return buf;
}

static std::vector<u8> create_dyld_info_only_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(DyldInfoCommand));
  DyldInfoCommand &cmd = *(DyldInfoCommand *)buf.data();

  cmd.cmd = LC_DYLD_INFO_ONLY;
  cmd.cmdsize = buf.size();

  cmd.rebase_off = ctx.rebase.hdr.offset;
  cmd.rebase_size = ctx.rebase.hdr.size;

  cmd.bind_off = ctx.bind.hdr.offset;
  cmd.bind_size = ctx.bind.hdr.size;

  cmd.lazy_bind_off = ctx.lazy_bind.hdr.offset;
  cmd.lazy_bind_size = ctx.lazy_bind.hdr.size;

  cmd.export_off = ctx.export_.hdr.offset;
  cmd.export_size = ctx.export_.hdr.size;
  return buf;
}

static std::vector<u8> create_symtab_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(SymtabCommand));
  SymtabCommand &cmd = *(SymtabCommand *)buf.data();

  cmd.cmd = LC_SYMTAB;
  cmd.cmdsize = buf.size();
  cmd.symoff = ctx.symtab.hdr.offset;
  cmd.nsyms = ctx.symtab.hdr.size / sizeof(MachSym);
  cmd.stroff = ctx.strtab.hdr.offset;
  cmd.strsize = ctx.strtab.hdr.size;
  return buf;
}

static std::vector<u8> create_dysymtab_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(DysymtabCommand));
  DysymtabCommand &cmd = *(DysymtabCommand *)buf.data();

  cmd.cmd = LC_DYSYMTAB;
  cmd.cmdsize = buf.size();
  cmd.nlocalsym = 1;
  cmd.iextdefsym = 1;
  cmd.nextdefsym = 3;
  cmd.iundefsym = 4;
  cmd.nundefsym = 2;
  cmd.indirectsymoff = ctx.indir_symtab.hdr.offset;
  cmd.nindirectsyms =
    ctx.indir_symtab.hdr.size / OutputIndirectSymtabSection::ENTRY_SIZE;
  return buf;
}

static std::vector<u8> create_dylinker_cmd(Context &ctx) {
  static constexpr char path[] = "/usr/lib/dyld";

  std::vector<u8> buf(align_to(sizeof(DylinkerCommand) + sizeof(path), 8));
  DylinkerCommand &cmd = *(DylinkerCommand *)buf.data();

  cmd.cmd = LC_LOAD_DYLINKER;
  cmd.cmdsize = buf.size();
  cmd.nameoff = sizeof(cmd);
  memcpy(buf.data() + sizeof(cmd), path, sizeof(path));
  return buf;
}

static std::vector<u8> create_uuid_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(UUIDCommand));
  UUIDCommand &cmd = *(UUIDCommand *)buf.data();

  cmd.cmd = LC_UUID;
  cmd.cmdsize = buf.size();
  return buf;
}

static std::vector<u8> create_build_version_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(BuildVersionCommand) + sizeof(BuildToolVersion));
  BuildVersionCommand &cmd = *(BuildVersionCommand *)buf.data();

  cmd.cmd = LC_BUILD_VERSION;
  cmd.cmdsize = buf.size();
  cmd.platform = PLATFORM_MACOS;
  cmd.minos = 0xb0000;
  cmd.sdk = 0xb0300;
  cmd.ntools = 1;

  BuildToolVersion &tool = *(BuildToolVersion *)(buf.data() + sizeof(cmd));
  tool.tool = 3;
  tool.version = 0x28a0900;
  return buf;
}

static std::vector<u8> create_source_version_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(SourceVersionCommand));
  SourceVersionCommand &cmd = *(SourceVersionCommand *)buf.data();

  cmd.cmd = LC_SOURCE_VERSION;
  cmd.cmdsize = buf.size();
  return buf;
}

static std::vector<u8> create_main_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(EntryPointCommand));
  EntryPointCommand &cmd = *(EntryPointCommand *)buf.data();

  cmd.cmd = LC_MAIN;
  cmd.cmdsize = buf.size();
  cmd.entryoff = 0x3f70;
  return buf;
}

static std::vector<u8> create_load_dylib_cmd(Context &ctx) {
  static constexpr char path[] = "/usr/lib/libSystem.B.dylib";

  std::vector<u8> buf(align_to(sizeof(DylibCommand) + sizeof(path), 8));
  DylibCommand &cmd = *(DylibCommand *)buf.data();

  cmd.cmd = LC_LOAD_DYLIB;
  cmd.cmdsize = buf.size();
  cmd.nameoff = sizeof(cmd);
  cmd.timestamp = 2;
  cmd.current_version = 0x50c6405;
  cmd.compatibility_version = 0x10000;
  memcpy(buf.data() + sizeof(cmd), path, sizeof(path));
  return buf;
}

static std::vector<u8> create_function_starts_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(LinkEditDataCommand));
  LinkEditDataCommand &cmd = *(LinkEditDataCommand *)buf.data();

  cmd.cmd = LC_FUNCTION_STARTS;
  cmd.cmdsize = buf.size();
  cmd.dataoff = ctx.function_starts.hdr.offset;
  cmd.datasize = ctx.function_starts.hdr.size;
  return buf;
}

static std::vector<u8> create_data_in_code_cmd(Context &ctx) {
  std::vector<u8> buf(sizeof(LinkEditDataCommand));
  LinkEditDataCommand &cmd = *(LinkEditDataCommand *)buf.data();

  cmd.cmd = LC_DATA_IN_CODE;
  cmd.cmdsize = buf.size();
  cmd.dataoff = 0xc070;
  return buf;
}

static std::vector<std::vector<u8>> create_load_commands(Context &ctx) {
  std::vector<std::vector<u8>> vec;
  vec.push_back(create_page_zero_cmd(ctx));

  auto append = [&](std::vector<u8> &buf, auto x) {
    i64 off = buf.size();
    buf.resize(buf.size() + sizeof(x));
    memcpy(buf.data() + off, &x, sizeof(x));
  };

  // Add LC_SEGMENT_64 comamnds
  for (OutputSegment *seg : ctx.segments) {
    std::vector<u8> &buf = vec.emplace_back();

    i64 nsects = 0;
    for (OutputSection *sec : seg->sections)
      if (!sec->is_hidden)
        nsects++;

    SegmentCommand cmd = seg->cmd;
    cmd.cmdsize = sizeof(SegmentCommand) + sizeof(MachSection) * nsects;
    cmd.nsects = nsects;
    append(buf, cmd);

    for (OutputSection *sec : seg->sections) {
      if (!sec->is_hidden) {
        MachSection hdr = sec->hdr;
        memcpy(hdr.segname, cmd.segname, sizeof(cmd.segname));
        append(buf, hdr);
      }
    }
  }

  vec.push_back(create_dyld_info_only_cmd(ctx));
  vec.push_back(create_symtab_cmd(ctx));
  vec.push_back(create_dysymtab_cmd(ctx));
  vec.push_back(create_dylinker_cmd(ctx));
  vec.push_back(create_uuid_cmd(ctx));
  vec.push_back(create_build_version_cmd(ctx));
  vec.push_back(create_source_version_cmd(ctx));
  vec.push_back(create_main_cmd(ctx));
  vec.push_back(create_load_dylib_cmd(ctx));
  vec.push_back(create_function_starts_cmd(ctx));
  vec.push_back(create_data_in_code_cmd(ctx));
  return vec;
}

void OutputLoadCommand::compute_size(Context &ctx) {
  std::vector<std::vector<u8>> cmds = create_load_commands(ctx);
  ncmds = cmds.size();
  hdr.size = flatten(cmds).size();
}

void OutputLoadCommand::copy_buf(Context &ctx) {
  std::vector<std::vector<u8>> cmds = create_load_commands(ctx);
  write_vector(ctx.buf + hdr.offset, flatten(cmds));
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

void OutputSegment::set_offset(Context &ctx, i64 fileoff, u64 vmaddr) {
  cmd.fileoff = fileoff;
  cmd.vmaddr = vmaddr;

  i64 offset = 0;

  auto set_offset = [&](OutputSection &sec) {
    offset = align_to(offset, 1 << sec.hdr.p2align);
    sec.hdr.addr = vmaddr + offset;
    sec.hdr.offset = fileoff + offset;
    offset += sec.hdr.size;
  };

  if (fileoff == 0) {
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

OutputRebaseSection::OutputRebaseSection() {
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

OutputBindSection::OutputBindSection() {
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

OutputLazyBindSection::OutputLazyBindSection() {
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
ExportEncoder::construct_trie(TrieNode &parent, std::span<Entry> entries,
			      i64 len) {
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

    TrieNode child = {entries[i].name.substr(len, new_len - len)};
    construct_trie(child, subspan, new_len);
    parent.children.push_back(std::move(child));

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

  for (TrieNode &child : node.children) {
    // +1 for NUL byte
    size += child.prefix.size() + 1 + uleb_size(child.offset);
  }

  for (TrieNode &child : node.children)
    size += set_offset(child, offset + size);
  return size;
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

  *buf++ = node.children.size();

  for (TrieNode &child : node.children) {
    buf += write_string(buf, child.prefix);
    buf += write_uleb(buf, child.offset);
  }

  for (TrieNode &child : node.children)
    write_trie(start, child);
}

OutputExportSection::OutputExportSection() {
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

void OutputSymtabSection::add(Context &ctx, std::string_view name,
                              i64 type, bool is_external,
                              i64 sect_idx, i64 desc, u64 value) {
  MachSym &sym = symbols.emplace_back();
  hdr.size += sizeof(sym);

  memset(&sym, 0, sizeof(sym));

  sym.stroff = ctx.strtab.add_string(name);
  sym.type = type;
  sym.ext = is_external;
  sym.sect = sect_idx;
  sym.desc = desc;
  sym.value = value;
}

void OutputSymtabSection::copy_buf(Context &ctx) {
  memcpy(ctx.buf + hdr.offset, symbols.data(),
         symbols.size() * sizeof(symbols[0]));
}

i64 OutputStrtabSection::add_string(std::string_view str) {
  hdr.size += str.size() + 1;

  i64 off = contents.size();
  contents += str;
  contents += '\0';
  return off;
}

void OutputStrtabSection::copy_buf(Context &ctx) {
  memcpy(ctx.buf + hdr.offset, &contents[0], contents.size());
}

void OutputIndirectSymtabSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

TextSection::TextSection() {
  strcpy(hdr.sectname, "__text");
  hdr.p2align = __builtin_ctz(16);
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  hdr.size = contents.size();
}


void TextSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

StubsSection::StubsSection() {
  strcpy(hdr.sectname, "__stubs");
  hdr.p2align = __builtin_ctz(2);
  hdr.type = S_SYMBOL_STUBS;
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  hdr.reserved2 = 6;
}

void StubsSection::add(Context &ctx, i64 dylib_idx, std::string_view name,
                       i64 flags, i64 seg_idx, i64 offset) {
  entries.push_back({dylib_idx, name, flags, seg_idx, offset});

  i64 nsyms = entries.size();
  ctx.stubs.hdr.size = nsyms * StubsSection::ENTRY_SIZE;
  ctx.stub_helper.hdr.size =
    StubHelperSection::HEADER_SIZE + nsyms * StubHelperSection::ENTRY_SIZE;
  ctx.lazy_symbol_ptr.hdr.size = nsyms * LazySymbolPtrSection::ENTRY_SIZE;
}

void StubsSection::copy_buf(Context &ctx) {
  u8 *buf = ctx.buf + hdr.offset;

  for (i64 i = 0; i < entries.size(); i++) {
    // `ff 25 xx xx xx xx` is a RIP-relative indirect jump instruction,
    // i.e., `jmp *IMM(%rip)`. It loads an address from la_symbol_ptr
    // and jump there.
    buf[i * 6] = 0xff;
    buf[i * 6 + 1] = 0x25;
    *(u32 *)(buf + i * 6 + 2) =
      (ctx.lazy_symbol_ptr.hdr.addr + i * 10) - (hdr.addr + i * 6 + 6);
  }
}

StubHelperSection::StubHelperSection() {
  strcpy(hdr.sectname, "__stub_helper");
  hdr.p2align = __builtin_ctz(4);
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
}

void StubHelperSection::copy_buf(Context &ctx) {
  u8 *start = ctx.buf + hdr.offset;
  u8 *buf = start;

  static constexpr u64 dyld_private_addr = 0x100008008;

  u8 insn0[16] = {
    0x4c, 0x8d, 0x1d, 0, 0, 0, 0, // lea $__dyld_private(%rip), %r11
    0x41, 0x53,                   // push %r11
    0xff, 0x25, 0, 0, 0, 0,       // jmp *$dyld_stub_binder@GOT(%rip)
    0x90,                         // nop
  };

  memcpy(buf, insn0, sizeof(insn0));
  *(u32 *)(buf + 3) = dyld_private_addr - hdr.addr - 7;
  *(u32 *)(buf + 11) = ctx.got.hdr.addr - hdr.addr - 15;

  buf += 16;

  for (i64 i = 0; i < ctx.stubs.entries.size(); i++) {
    u8 insn[10] = {
      0x68, 0, 0, 0, 0, // push $idx
      0xe9, 0, 0, 0, 0, // jmp $__stub_helper
    };

    memcpy(buf, insn, sizeof(insn));

    *(u32 *)(buf + 1) = i;
    *(u32 *)(buf + 6) = start - buf - 10;
    buf += 10;
  }
}

CstringSection::CstringSection() {
  strcpy(hdr.sectname, "__cstring");
  hdr.type = S_CSTRING_LITERALS;
  hdr.size = sizeof(contents);
}

void CstringSection::copy_buf(Context &ctx) {
  memcpy(ctx.buf + hdr.offset, contents, sizeof(contents));
}

UnwindInfoSection::UnwindInfoSection() {
  strcpy(hdr.sectname, "__unwind_info");
  hdr.p2align = __builtin_ctz(4);
  hdr.size = contents.size();
}

void UnwindInfoSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

GotSection::GotSection() {
  strcpy(hdr.sectname, "__got");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_NON_LAZY_SYMBOL_POINTERS;
  hdr.size = 8;
  hdr.reserved1 = 1;
}

LazySymbolPtrSection::LazySymbolPtrSection() {
  strcpy(hdr.sectname, "__la_symbol_ptr");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_LAZY_SYMBOL_POINTERS;
  hdr.reserved1 = 2;
}

void LazySymbolPtrSection::copy_buf(Context &ctx) {
  u64 *buf = (u64 *)(ctx.buf + hdr.offset);

  for (i64 i = 0; i < ctx.stubs.entries.size(); i++)
    buf[i] = ctx.stub_helper.hdr.addr + StubHelperSection::HEADER_SIZE +
             i * StubHelperSection::ENTRY_SIZE;
}

DataSection::DataSection() {
  strcpy(hdr.sectname, "__data");
  hdr.p2align = __builtin_ctz(8);

  contents.resize(8);
  hdr.size = contents.size();
}

void DataSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

} // namespace mold::macho
