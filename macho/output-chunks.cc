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

  i64 locals = ctx.symtab.locals.size();
  i64 globals = ctx.symtab.globals.size();
  i64 undefs = ctx.symtab.undefs.size();

  cmd.ilocalsym = 0;
  cmd.nlocalsym = locals;
  cmd.iextdefsym = locals;
  cmd.nextdefsym = globals;
  cmd.iundefsym = locals + globals;
  cmd.nundefsym = undefs;

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
  cmd.entryoff = intern(ctx, "_main")->get_addr(ctx) - PAGE_ZERO_SIZE;
  return buf;
}

static std::vector<u8> create_load_dylib_cmd(Context &ctx, std::string_view name) {
  i64 size = sizeof(DylibCommand) + name.size() + 1; // +1 for NUL
  std::vector<u8> buf(align_to(size, 8));
  DylibCommand &cmd = *(DylibCommand *)buf.data();

  cmd.cmd = LC_LOAD_DYLIB;
  cmd.cmdsize = buf.size();
  cmd.nameoff = sizeof(cmd);
  cmd.timestamp = 2;
  cmd.current_version = 0x50c6405;
  cmd.compatibility_version = 0x10000;
  write_string(buf.data() + sizeof(cmd), name);
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
    for (Chunk *sec : seg->chunks)
      if (!sec->is_hidden)
        nsects++;

    SegmentCommand cmd = seg->cmd;
    cmd.cmdsize = sizeof(SegmentCommand) + sizeof(MachSection) * nsects;
    cmd.nsects = nsects;
    append(buf, cmd);

    for (Chunk *sec : seg->chunks) {
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
  for (DylibFile *dylib : ctx.dylibs)
    vec.push_back(create_load_dylib_cmd(ctx, dylib->install_name));
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

OutputSection::OutputSection(std::string_view name) {
  assert(name.size() < sizeof(hdr.sectname));
  memcpy(hdr.sectname, name.data(), name.size());
  is_regular = true;
}

void OutputSection::compute_size(Context &ctx) {
  i64 sz = 0;

  if (this == &ctx.data) {
    // As a special case, we need a word-size padding at the beginning
    // of __data for dyld. It is located by __dyld_private symbol.
    sz = 8;
  }

  for (Subsection *subsec : members) {
    subsec->output_offset = sz;
    sz += subsec->input_size;
  }
  hdr.size = sz;
}

void OutputSection::copy_buf(Context &ctx) {
  u8 *buf = ctx.buf + hdr.offset;
  i64 offset = 0;

  for (Subsection *subsec : members) {
    std::string_view data = subsec->get_contents();
    memcpy(buf + offset, data.data(), data.size());
    subsec->apply_reloc(ctx, buf + offset);
    offset += data.size();
  }
}

OutputSegment::OutputSegment(std::string_view name, u32 prot, u32 flags) {
  assert(name.size() <= sizeof(cmd.segname));

  cmd.cmd = LC_SEGMENT_64;
  memcpy(cmd.segname, name.data(), name.size());
  cmd.maxprot = prot;
  cmd.initprot = prot;
  cmd.flags = flags;
}

void OutputSegment::set_offset(Context &ctx, i64 fileoff, u64 vmaddr) {
  cmd.fileoff = fileoff;
  cmd.vmaddr = vmaddr;

  i64 offset = 0;

  for (Chunk *sec : chunks) {
    offset = align_to(offset, 1 << sec->hdr.p2align);
    sec->hdr.addr = vmaddr + offset;
    sec->hdr.offset = fileoff + offset;
    sec->compute_size(ctx);
    offset += sec->hdr.size;
  }

  cmd.vmsize = align_to(offset, PAGE_SIZE);
  cmd.filesize =
    (this == ctx.segments.back()) ? offset : align_to(offset, PAGE_SIZE);
}

void OutputSegment::copy_buf(Context &ctx) {
  for (Chunk *sec : chunks)
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

BindEncoder::BindEncoder() {
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

void OutputBindSection::compute_size(Context &ctx) {
  BindEncoder enc;

  for (Symbol *sym : ctx.got.syms) {
    assert(sym->file->is_dylib);

    enc.add(((DylibFile *)sym->file)->dylib_idx, sym->name, 0,
            ctx.data_const_seg.seg_idx,
            sym->get_got_addr(ctx) - ctx.data_const_seg.cmd.vmaddr);
  }

  enc.finish();

  contents = enc.buf;
  hdr.size = align_to(contents.size(), 8);
}

void OutputBindSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputLazyBindSection::add(Context &ctx, Symbol &sym, i64 flags) {
  auto emit = [&](u8 byte) {
    contents.push_back(byte);
  };

  i64 dylib_idx = ((DylibFile *)sym.file)->dylib_idx;
  if (dylib_idx < 16) {
    emit(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | dylib_idx);
  } else {
    emit(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
    encode_uleb(contents, dylib_idx);
  }

  assert(flags < 16);
  emit(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
  contents.insert(contents.end(), (u8 *)sym.name.data(),
                  (u8 *)(sym.name.data() + sym.name.size()));
  emit('\0');

  i64 seg_idx = ctx.data_seg.seg_idx;
  emit(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg_idx);

  i64 offset = ctx.lazy_symbol_ptr.hdr.addr +
               sym.stub_idx * LazySymbolPtrSection::ENTRY_SIZE -
               ctx.data_seg.cmd.vmaddr;
  encode_uleb(contents, offset);

  emit(BIND_OPCODE_DO_BIND);
  emit(BIND_OPCODE_DONE);
}

void OutputLazyBindSection::compute_size(Context &ctx) {
  ctx.stubs.bind_offsets.clear();

  for (Symbol *sym : ctx.stubs.syms) {
    ctx.stubs.bind_offsets.push_back(contents.size());
    add(ctx, *sym, 0);
  }

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

  root = construct_trie(entries, 0);

  i64 size = set_offset(root, 0);
  for (;;) {
    i64 sz = set_offset(root, 0);
    if (sz == size)
      return sz;
    size = sz;
  }
}

i64 ExportEncoder::common_prefix_len(std::span<Entry> entries, i64 len) {
  for (; len < entries[0].name.size(); len++)
    for (Entry &ent : entries.subspan(1))
      if (ent.name.size() == len || ent.name[len] != entries[0].name[len])
        return len;
  return len;
}

ExportEncoder::TrieNode
ExportEncoder::construct_trie(std::span<Entry> entries, i64 len) {
  TrieNode node;

  i64 new_len = common_prefix_len(entries, len);
  if (new_len > len) {
    node.prefix = entries[0].name.substr(len, new_len - len);
    if (entries[0].name.size() == new_len) {
      node.is_leaf = true;
      node.flags = entries[0].flags;
      node.addr = entries[0].addr;
      entries = entries.subspan(1);
    }
  }

  for (i64 i = 0; i < entries.size();) {
    i64 j = i + 1;
    u8 c = entries[i].name[new_len];
    while (j < entries.size() && c == entries[j].name[new_len])
      j++;
    node.children.push_back(construct_trie(entries.subspan(i, j - i), new_len));
    i = j;
  }
  return node;
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

void OutputFunctionStartsSection::compute_size(Context &ctx) {
  std::vector<u64> addrs;

  for (ObjectFile *obj : ctx.objs)
    for (Symbol *sym : obj->syms)
      if (sym->file == obj && sym->subsec && sym->subsec->isec.osec == &ctx.text)
        addrs.push_back(sym->get_addr(ctx));

  std::sort(addrs.begin(), addrs.end());

  contents.resize(addrs.size() * 5);

  u8 *p = contents.data();
  u64 last = PAGE_ZERO_SIZE;

  for (u64 val : addrs) {
    p += write_uleb(p, val - last);
    last = val;
  }

  hdr.size = p - contents.data();
  contents.resize(hdr.size);
}

void OutputFunctionStartsSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

void OutputSymtabSection::compute_size(Context &ctx) {
  for (ObjectFile *obj : ctx.objs)
    for (Symbol *sym : obj->syms)
      if (sym->file == obj)
        globals.push_back({sym, ctx.strtab.add_string(sym->name)});

  i64 idx = globals.size();

  for (DylibFile *dylib : ctx.dylibs) {
    for (Symbol *sym : dylib->syms) {
      if (sym->file == dylib) {
        if (sym->stub_idx != -1 || sym->got_idx != -1) {
          undefs.push_back({sym, ctx.strtab.add_string(sym->name)});

          if (sym->stub_idx != -1)
            ctx.indir_symtab.stubs.push_back({sym, idx});
          else
            ctx.indir_symtab.gots.push_back({sym, idx});
          idx++;
        }
      }
    }
  }

  hdr.size = idx * sizeof(MachSym);
}

void OutputSymtabSection::copy_buf(Context &ctx) {
  MachSym *buf = (MachSym *)(ctx.buf + hdr.offset);
  memset(buf, 0, hdr.size);

  auto write = [&](Entry &ent) {
    MachSym &msym = *buf++;
    Symbol &sym = *ent.sym;

    msym.stroff = ent.stroff;
    msym.type = (sym.file->is_dylib ? N_UNDF : N_SECT);
    msym.ext = sym.is_extern;

    if (!sym.file->is_dylib)
      msym.value = sym.get_addr(ctx);
    if (sym.subsec)
      msym.sect = sym.subsec->isec.osec->sect_idx;

    if (sym.file->is_dylib)
      msym.desc = ((DylibFile *)sym.file)->dylib_idx << 8;
    else if (sym.referenced_dynamically)
      msym.desc = REFERENCED_DYNAMICALLY;
  };

  for (Entry &ent : locals)
    write(ent);
  for (Entry &ent : globals)
    write(ent);
  for (Entry &ent : undefs)
    write(ent);
}

i64 OutputStrtabSection::add_string(std::string_view str) {
  hdr.size += str.size() + 1;

  i64 off = contents.size();
  contents += str;
  contents += '\0';
  return off;
}

void OutputStrtabSection::compute_size(Context &ctx) {
  hdr.size = align_to(hdr.size, 8);
}

void OutputStrtabSection::copy_buf(Context &ctx) {
  memcpy(ctx.buf + hdr.offset, &contents[0], contents.size());
}

void OutputIndirectSymtabSection::compute_size(Context &ctx) {
  ctx.stubs.hdr.reserved1 = 0;
  ctx.got.hdr.reserved1 = stubs.size();
  ctx.lazy_symbol_ptr.hdr.reserved1 = stubs.size() + gots.size();

  i64 nsyms = stubs.size() * 2 + gots.size();
  hdr.size = nsyms * ENTRY_SIZE;
}

void OutputIndirectSymtabSection::copy_buf(Context &ctx) {
  u32 *buf = (u32 *)(ctx.buf + hdr.offset);

  for (Entry &ent : stubs)
    buf[ent.sym->stub_idx] = ent.symtab_idx;
  buf += stubs.size();

  for (Entry &ent : gots)
    buf[ent.sym->got_idx] = ent.symtab_idx;
  buf += gots.size();

  for (Entry &ent : stubs)
    buf[ent.sym->stub_idx] = ent.symtab_idx;
}

StubsSection::StubsSection() {
  strcpy(hdr.sectname, "__stubs");
  hdr.p2align = __builtin_ctz(2);
  hdr.type = S_SYMBOL_STUBS;
  hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  hdr.reserved2 = ENTRY_SIZE;
}

void StubsSection::add(Context &ctx, Symbol *sym) {
  assert(sym->stub_idx == -1);
  sym->stub_idx = syms.size();

  syms.push_back(sym);

  i64 nsyms = syms.size();
  hdr.size = nsyms * ENTRY_SIZE;

  ctx.stub_helper.hdr.size =
    StubHelperSection::HEADER_SIZE + nsyms * StubHelperSection::ENTRY_SIZE;
  ctx.lazy_symbol_ptr.hdr.size = nsyms * LazySymbolPtrSection::ENTRY_SIZE;
}

void StubsSection::copy_buf(Context &ctx) {
  u8 *buf = ctx.buf + hdr.offset;

  for (i64 i = 0; i < syms.size(); i++) {
    // `ff 25 xx xx xx xx` is a RIP-relative indirect jump instruction,
    // i.e., `jmp *IMM(%rip)`. It loads an address from la_symbol_ptr
    // and jump there.
    assert(ENTRY_SIZE == 6);
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

  u8 insn0[16] = {
    0x4c, 0x8d, 0x1d, 0, 0, 0, 0, // lea $__dyld_private(%rip), %r11
    0x41, 0x53,                   // push %r11
    0xff, 0x25, 0, 0, 0, 0,       // jmp *$dyld_stub_binder@GOT(%rip)
    0x90,                         // nop
  };

  memcpy(buf, insn0, sizeof(insn0));
  *(u32 *)(buf + 3) = intern(ctx, "__dyld_private")->get_addr(ctx) - hdr.addr - 7;
  *(u32 *)(buf + 11) =
    intern(ctx, "dyld_stub_binder")->get_got_addr(ctx) - hdr.addr - 15;

  buf += 16;

  for (i64 i = 0; i < ctx.stubs.syms.size(); i++) {
    u8 insn[10] = {
      0x68, 0, 0, 0, 0, // push $bind_offset
      0xe9, 0, 0, 0, 0, // jmp $__stub_helper
    };

    memcpy(buf, insn, sizeof(insn));

    *(u32 *)(buf + 1) = ctx.stubs.bind_offsets[i];
    *(u32 *)(buf + 6) = start - buf - 10;
    buf += 10;
  }
}

UnwindInfoSection::UnwindInfoSection() {
  strcpy(hdr.sectname, "__unwind_info");
  hdr.p2align = __builtin_ctz(4);
  hdr.size = contents.size();
}

void UnwindEncoder::add(UnwindRecord &rec) {
  records.push_back(rec);
}

void UnwindEncoder::finish(Context &ctx) {
  i64 num_lsda = 0;

  for (UnwindRecord &rec : records) {
    if (rec.personality)
      rec.encoding |= encode_personality(ctx, rec.personality);
    if (rec.lsda)
      num_lsda++;
  }

  std::vector<std::span<UnwindRecord>> pages = split_records(ctx);

  // Allocate a buffer that is more than large enough to hold the
  // entire section.
  buf.resize(4096 * 1024);

  // Write the section header.
  UnwindSectionHeader &hdr = *(UnwindSectionHeader *)buf.data();
  hdr.version = UNWIND_SECTION_VERSION;
  hdr.encoding_offset = 0;
  hdr.encoding_count = 0;
  hdr.personality_offset = sizeof(hdr);
  hdr.personality_count = personalities.size();
  hdr.page_offset = sizeof(hdr) + personalities.size() * 4;
  hdr.page_count = pages.size() + 1;

  // Write the personalities
  u32 *per = (u32 *)(buf.data() + sizeof(hdr));
  for (Symbol *sym : personalities)
    *per++ = sym->get_addr(ctx);

  // Write first level pages, LSDA and second level pages
  UnwindFirstLevelPage *page1 = (UnwindFirstLevelPage *)per;
  UnwindLsdaEntry *lsda = (UnwindLsdaEntry *)(page1 + (pages.size() + 1));
  UnwindSecondLevelPage *page2 = (UnwindSecondLevelPage *)(lsda + num_lsda);

  for (std::span<UnwindRecord> span : pages) {
    page1->func_addr = span[0].get_func_addr(ctx);
    page1->page_offset = (u8 *)page2 - buf.data();
    page1->lsda_offset = (u8 *)lsda - buf.data();

    for (UnwindRecord &rec : span) {
      if (rec.lsda) {
        lsda->func_addr = rec.get_func_addr(ctx);
        lsda->lsda_addr = rec.lsda->get_addr(ctx) + rec.lsda_offset;
        lsda++;
      }
    }

    std::unordered_map<u32, u32> map;
    for (UnwindRecord &rec : span)
      map.insert({rec.encoding, map.size()});

    page2->kind = UNWIND_SECOND_LEVEL_COMPRESSED;
    page2->page_offset = sizeof(UnwindSecondLevelPage);
    page2->page_count = span.size();

    UnwindPageEntry *entry = (UnwindPageEntry *)(page2 + 1);
    for (UnwindRecord &rec : span) {
      entry->func_addr = rec.get_func_addr(ctx) - page1->func_addr;
      entry->encoding = map[rec.encoding];
      entry++;
    }

    page2->encoding_offset = (u8 *)entry - (u8 *)page2;
    page2->encoding_count = map.size();

    u32 *encoding = (u32 *)entry;
    for (std::pair<u32, u32> kv : map)
      encoding[kv.second] = kv.first;

    page1++;
    page2 = (UnwindSecondLevelPage *)(encoding + map.size());
    break;
  }

  // Write a terminator
  UnwindRecord &last = records[records.size() - 1];
  page1->func_addr = last.subsec->get_addr(ctx) + last.subsec->input_size + 1;
  page1->page_offset = 0;
  page1->lsda_offset = page1[-1].lsda_offset;

  buf.resize((u8 *)page2 - buf.data());
}

u32 UnwindEncoder::encode_personality(Context &ctx, Symbol *sym) {
  assert(sym);

  for (i64 i = 0; i < personalities.size(); i++)
    if (personalities[i] == sym)
      return (i + 1) << __builtin_ctz(UNWIND_PERSONALITY_MASK);

  if (personalities.size() == 3)
    Fatal(ctx) << ": too many personality functions";

  personalities.push_back(sym);
  return personalities.size() << __builtin_ctz(UNWIND_PERSONALITY_MASK);
}

std::vector<std::span<UnwindRecord>>
UnwindEncoder::split_records(Context &ctx) {
  constexpr i64 max_group_size = 4096;

  sort(records, [&](const UnwindRecord &a, const UnwindRecord &b) {
    return a.get_func_addr(ctx) < b.get_func_addr(ctx);
  });

  std::vector<std::span<UnwindRecord>> vec;

  for (i64 i = 0; i < records.size();) {
    i64 j = 1;
    u64 end_addr = records[i].get_func_addr(ctx) + (1 << 24);
    while (j < max_group_size && i + j < records.size() &&
           records[i + j].get_func_addr(ctx) < end_addr)
      j++;
    vec.push_back(std::span(records).subspan(i, j));
    i += j;
  }
  return vec;
}

static std::vector<u8> construct_unwind_info(Context &ctx) {
  UnwindEncoder enc;

  for (OutputSegment *seg : ctx.segments)
    for (Chunk *chunk : seg->chunks)
      if (chunk->is_regular)
        for (Subsection *subsec : ((OutputSection *)chunk)->members)
          for (UnwindRecord &rec : subsec->get_unwind_records())
            enc.add(rec);

  enc.finish(ctx);
  return std::move(enc.buf);
}

void UnwindInfoSection::compute_size(Context &ctx) {
  contents = construct_unwind_info(ctx);
  hdr.size = contents.size();
}

void UnwindInfoSection::copy_buf(Context &ctx) {
  write_vector(ctx.buf + hdr.offset, contents);
}

GotSection::GotSection() {
  strcpy(hdr.sectname, "__got");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_NON_LAZY_SYMBOL_POINTERS;
}

void GotSection::add(Context &ctx, Symbol *sym) {
  assert(sym->got_idx == -1);
  sym->got_idx = syms.size();
  syms.push_back(sym);
  hdr.size = syms.size() * ENTRY_SIZE;
}

LazySymbolPtrSection::LazySymbolPtrSection() {
  strcpy(hdr.sectname, "__la_symbol_ptr");
  hdr.p2align = __builtin_ctz(8);
  hdr.type = S_LAZY_SYMBOL_POINTERS;
}

void LazySymbolPtrSection::copy_buf(Context &ctx) {
  u64 *buf = (u64 *)(ctx.buf + hdr.offset);

  for (i64 i = 0; i < ctx.stubs.syms.size(); i++)
    buf[i] = ctx.stub_helper.hdr.addr + StubHelperSection::HEADER_SIZE +
             i * StubHelperSection::ENTRY_SIZE;
}

} // namespace mold::macho
