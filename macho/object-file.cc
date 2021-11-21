#include "mold.h"

#include "../archive-file.h"

namespace mold::macho {

std::ostream &operator<<(std::ostream &out, const InputFile &file) {
  if (file.archive_name.empty())
    out << path_clean(file.mf->name);
  else
    out << path_clean(file.archive_name) << "(" << path_clean(file.mf->name) + ")";
  return out;
}

ObjectFile *ObjectFile::create(Context &ctx, MappedFile<Context> *mf,
                               std::string archive_name) {
  ObjectFile *obj = new ObjectFile;
  obj->mf = mf;
  obj->archive_name = archive_name;
  obj->is_alive = archive_name.empty();
  ctx.obj_pool.push_back(std::unique_ptr<ObjectFile>(obj));
  return obj;
};

void ObjectFile::parse(Context &ctx) {
  parse_sections(ctx);
  parse_symtab(ctx);
  split_subsections(ctx);
  parse_data_in_code(ctx);

  for (std::unique_ptr<InputSection> &isec : sections)
    if (isec)
      isec->parse_relocations(ctx);

  if (unwind_sec)
    parse_compact_unwind(ctx, *unwind_sec);
}

void ObjectFile::parse_sections(Context &ctx) {
  MachHeader &hdr = *(MachHeader *)mf->data;
  u8 *p = mf->data + sizeof(hdr);

  // Read all but a symtab section
  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;
    p += lc.cmdsize;
    if (lc.cmd != LC_SEGMENT_64)
      continue;

    SegmentCommand &cmd = *(SegmentCommand *)&lc;
    MachSection *mach_sec = (MachSection *)((u8 *)&lc + sizeof(cmd));

    for (MachSection &msec : std::span(mach_sec, mach_sec + cmd.nsects)) {
      sections.push_back(nullptr);

      if (msec.match("__LD", "__compact_unwind")) {
        unwind_sec = &msec;
        continue;
      }

      if (msec.attr & S_ATTR_DEBUG)
        continue;

      sections.back().reset(new InputSection(ctx, *this, msec));
    }
  }
}

void ObjectFile::parse_symtab(Context &ctx) {
  SymtabCommand *cmd = (SymtabCommand *)find_load_command(ctx, LC_SYMTAB);
  if (!cmd)
    return;

  mach_syms = {(MachSym *)(mf->data + cmd->symoff), cmd->nsyms};
  syms.reserve(mach_syms.size());

  i64 nlocal = 0;
  for (MachSym &msym : mach_syms)
    if (!msym.ext)
      nlocal++;
  local_syms.reserve(nlocal);

  for (MachSym &msym : mach_syms) {
    std::string_view name = (char *)(mf->data + cmd->stroff + msym.stroff);

    if (msym.ext) {
      syms.push_back(intern(ctx, name));
    } else {
      local_syms.emplace_back(name);
      syms.push_back(&local_syms.back());
    }
  }
}

struct SplitInfo {
  struct Region {
    u32 offset;
    u32 size;
    u32 symidx;
    bool is_alt_entry;
  };

  InputSection *isec = nullptr;
  std::vector<Region> regions;
};

static std::vector<SplitInfo> split(Context &ctx, ObjectFile &file) {
  std::vector<SplitInfo> vec;

  for (std::unique_ptr<InputSection> &isec : file.sections)
    vec.push_back({isec.get()});

  for (i64 i = 0; i < file.mach_syms.size(); i++) {
    MachSym &msym = file.mach_syms[i];
    if (msym.type == N_SECT) {
      SplitInfo::Region r;
      r.offset = msym.value - file.sections[msym.sect - 1]->hdr.addr;
      r.symidx = i;
      r.is_alt_entry = (msym.desc & N_ALT_ENTRY);
      vec[msym.sect - 1].regions.push_back(r);
    }
  }

  erase(vec, [](const SplitInfo &info) { return !info.isec; });

  sort(vec, [](const SplitInfo &a, const SplitInfo &b) {
    return a.isec->hdr.addr < b.isec->hdr.addr;
  });

  for (SplitInfo &info : vec) {
    std::vector<SplitInfo::Region> &r = info.regions;

    if (r.empty()) {
      r.push_back({0, (u32)info.isec->hdr.size, (u32)-1, false});
      continue;
    }

    sort(r, [](const SplitInfo::Region &a, const SplitInfo::Region &b) {
      return a.offset < b.offset;
    });

    if (r[0].offset > 0)
      r.insert(r.begin(), {0, r[0].offset, (u32)-1, false});

    for (i64 i = 0; i < r.size() - 1; i++)
      r[i].size = r[i + 1].offset - r[i].offset;
    r.back().size = info.isec->hdr.size - r.back().offset;
  }
  return vec;
}

void ObjectFile::split_subsections(Context &ctx) {
  sym_to_subsec.resize(mach_syms.size());

  for (SplitInfo &info : split(ctx, *this)) {
    InputSection &isec = *info.isec;

    for (SplitInfo::Region &r : info.regions) {
      if (!r.is_alt_entry) {
        Subsection *subsec = new Subsection{
          .isec = isec,
          .input_offset = r.offset,
          .input_size = r.size,
          .input_addr = (u32)(isec.hdr.addr + r.offset),
          .p2align = (u8)isec.hdr.p2align,
        };
        subsections.push_back(std::unique_ptr<Subsection>(subsec));
      }

      if (r.symidx != -1)
        sym_to_subsec[r.symidx] = subsections.size() - 1;
    }
  }

  for (i64 i = 0; i < mach_syms.size(); i++)
    if (!mach_syms[i].ext)
      override_symbol(ctx, i);
}

void ObjectFile::parse_data_in_code(Context &ctx) {
  if (auto *cmd = (LinkEditDataCommand *)find_load_command(ctx, LC_DATA_IN_CODE))
    data_in_code_entries = {(DataInCodeEntry *)(mf->data + cmd->dataoff),
                            cmd->datasize / sizeof(DataInCodeEntry)};
}

LoadCommand *ObjectFile::find_load_command(Context &ctx, u32 type) {
  MachHeader &hdr = *(MachHeader *)mf->data;
  u8 *p = mf->data + sizeof(hdr);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;
    if (lc.cmd == type)
      return &lc;
    p += lc.cmdsize;
  }
  return nullptr;
}

i64 ObjectFile::find_subsection_idx(Context &ctx, u32 addr) {
  auto it = std::upper_bound(
      subsections.begin(), subsections.end(), addr,
      [&](u32 addr, const std::unique_ptr<Subsection> &subsec) {
    return addr < subsec->input_addr;
  });

  if (it == subsections.begin())
    return -1;
  return it - subsections.begin() - 1;
}

Subsection *ObjectFile::find_subsection(Context &ctx, u32 addr) {
  i64 i = find_subsection_idx(ctx, addr);
  return (i == -1) ? nullptr : subsections[i].get();
}

void ObjectFile::parse_compact_unwind(Context &ctx, MachSection &hdr) {
  if (hdr.size % sizeof(CompactUnwindEntry))
    Fatal(ctx) << *this << ": invalid __compact_unwind section size";

  i64 num_entries = hdr.size / sizeof(CompactUnwindEntry);
  unwind_records.reserve(num_entries);

  CompactUnwindEntry *src = (CompactUnwindEntry *)(mf->data + hdr.offset);

  // Read compact unwind entries
  for (i64 i = 0; i < num_entries; i++)
    unwind_records.emplace_back(src[i].code_len, src[i].encoding);

  // Read relocations
  MachRel *mach_rels = (MachRel *)(mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++) {
    MachRel &r = mach_rels[i];
    if (r.offset >= hdr.size)
      Fatal(ctx) << *this << ": relocation offset too large: " << i;

    i64 idx = r.offset / sizeof(CompactUnwindEntry);
    UnwindRecord &dst = unwind_records[idx];

    auto error = [&]() {
      Fatal(ctx) << *this << ": __compact_unwind: unsupported relocation: " << i;
    };

    switch (r.offset % sizeof(CompactUnwindEntry)) {
    case offsetof(CompactUnwindEntry, code_start): {
      if (r.is_pcrel || r.p2size != 3 || r.is_extern || r.type)
        error();

      Subsection *target = find_subsection(ctx, src[idx].code_start);
      if (!target)
        error();
      dst.subsec = target;
      dst.offset = src[idx].code_start - target->input_addr;
      break;
    }
    case offsetof(CompactUnwindEntry, personality):
      if (r.is_pcrel || r.p2size != 3 || !r.is_extern || r.type)
        error();
      dst.personality = syms[r.idx];
      break;
    case offsetof(CompactUnwindEntry, lsda): {
      if (r.is_pcrel || r.p2size != 3 || r.is_extern || r.type)
        error();

      i32 addr = *(i32 *)((u8 *)mf->data + hdr.offset + r.offset);
      Subsection *target = find_subsection(ctx, addr);
      if (!target)
        error();
      dst.lsda = target;
      dst.lsda_offset = addr - target->input_addr;
      break;
    }
    default:
      Fatal(ctx) << *this << ": __compact_unwind: unsupported relocation: " << i;
    }
  }

  for (i64 i = 0; i < num_entries; i++)
    if (!unwind_records[i].subsec)
      Fatal(ctx) << ": __compact_unwind: missing relocation at " << i;

  // Sort unwind entries by offset
  sort(unwind_records, [](const UnwindRecord &a, const UnwindRecord &b) {
    return std::tuple(a.subsec->input_addr, a.offset) <
           std::tuple(b.subsec->input_addr, b.offset);
  });

  // Associate unwind entries to subsections
  for (i64 i = 0; i < num_entries;) {
    Subsection &subsec = *unwind_records[i].subsec;
    subsec.unwind_offset = i;

    i64 j = i + 1;
    while (j < num_entries && unwind_records[j].subsec == &subsec)
      j++;
    subsec.nunwind = j - i;
    i = j;
  }
}

// Symbols with higher priorities overwrites symbols with lower priorities.
// Here is the list of priorities, from the highest to the lowest.
//
//  1. Strong defined symbol
//  2. Weak defined symbol
//  3. Strong defined symbol in a DSO
//  4. Weak defined symbol in a DSO
//  5. Strong or weak defined symbol in an archive
//  6. Common symbol
//  7. Unclaimed (nonexistent) symbol
//
// Ties are broken by file priority.
static u64 get_rank(InputFile *file, MachSym &msym, bool is_lazy) {
  if (msym.is_common())
    return (6 << 24) + file->priority;
  if (is_lazy)
    return (5 << 24) + file->priority;
  if (file->is_dylib)
    return (3 << 24) + file->priority;
  return (1 << 24) + file->priority;
}

static u64 get_rank(Symbol &sym) {
  InputFile *file = sym.file;
  if (!file)
    return 7 << 24;
  if (sym.is_common)
    return (6 << 24) + file->priority;
  if (!file->archive_name.empty())
    return (5 << 24) + file->priority;
  if (file->is_dylib)
    return (3 << 24) + file->priority;
  return (1 << 24) + file->priority;
}

void ObjectFile::override_symbol(Context &ctx, i64 idx) {
  Symbol &sym = *syms[idx];
  MachSym &msym = mach_syms[idx];

  sym.file = this;
  sym.is_extern = msym.ext;
  sym.is_lazy = false;

  switch (msym.type) {
  case N_UNDF:
    assert(msym.is_common());
    sym.subsec = nullptr;
    sym.value = msym.value;
    sym.is_common = true;
    break;
  case N_ABS:
    sym.subsec = nullptr;
    sym.value = msym.value;
    sym.is_common = false;
    break;
  case N_SECT:
    sym.subsec = subsections[sym_to_subsec[idx]].get();
    sym.value = msym.value - sym.subsec->input_addr;
    sym.is_common = false;
    break;
  default:
    Fatal(ctx) << sym << ": unknown symbol type: " << (u64)msym.type;
  }
}

void ObjectFile::resolve_regular_symbols(Context &ctx) {
  for (i64 i = 0; i < syms.size(); i++) {
    MachSym &msym = mach_syms[i];
    if (!msym.ext || msym.is_undef())
      continue;

    Symbol &sym = *syms[i];
    std::lock_guard lock(sym.mu);
    if (get_rank(this, msym, false) < get_rank(sym))
      override_symbol(ctx, i);
  }
}

void ObjectFile::resolve_lazy_symbols(Context &ctx) {
  for (i64 i = 0; i < syms.size(); i++) {
    MachSym &msym = mach_syms[i];
    if (!msym.ext || msym.is_undef() || msym.is_common())
      continue;

    Symbol &sym = *syms[i];
    std::lock_guard lock(sym.mu);

    if (get_rank(this, msym, true) < get_rank(sym)) {
      sym.file = this;
      sym.subsec = nullptr;
      sym.value = 0;
      sym.is_extern = false;
      sym.is_lazy = true;
      sym.is_common = false;
    }
  }
}

bool ObjectFile::is_objc_object(Context &ctx) {
  for (std::unique_ptr<InputSection> &isec : sections)
    if (isec->hdr.match("__DATA", "__objc_catlist") ||
        isec->hdr.match("__TEXT", "__swift"))
      return true;

  for (i64 i = 0; i < syms.size(); i++)
    if (!mach_syms[i].is_undef() && mach_syms[i].ext &&
        syms[i]->name.starts_with("_OBJC_CLASS_$_"))
      return true;
  return false;
}

std::vector<ObjectFile *> ObjectFile::mark_live_objects(Context &ctx) {
  std::vector<ObjectFile *> vec;
  assert(is_alive);

  for (i64 i = 0; i < syms.size(); i++) {
    MachSym &msym = mach_syms[i];
    if (!msym.ext)
      continue;

    Symbol &sym = *syms[i];
    std::lock_guard lock(sym.mu);

    if (msym.is_undef()) {
      if (sym.file && !sym.file->is_alive.exchange(true))
        vec.push_back((ObjectFile *)sym.file);
      continue;
    }

    if (get_rank(this, msym, false) < get_rank(sym))
      override_symbol(ctx, i);
  }
  return vec;
}

void ObjectFile::convert_common_symbols(Context &ctx) {
  for (i64 i = 0; i < syms.size(); i++) {
    Symbol &sym = *syms[i];
    MachSym &msym = mach_syms[i];

    if (sym.file == this && sym.is_common) {
      InputSection *isec = get_common_sec(ctx);
      Subsection *subsec = new Subsection{
        .isec = *isec,
        .input_size = (u32)msym.value,
        .p2align = (u8)msym.p2align,
      };

      subsections.push_back(std::unique_ptr<Subsection>(subsec));

      sym.subsec = subsec;
      sym.value = 0;
      sym.is_common = false;
    }
  }
}

void ObjectFile::check_duplicate_symbols(Context &ctx) {
  for (i64 i = 0; i < syms.size(); i++) {
    Symbol *sym = syms[i];
    MachSym &msym = mach_syms[i];
    if (sym && !msym.is_undef() && !msym.is_common() && sym->file != this)
      Error(ctx) << "duplicate symbol: " << *this << ": " << *sym->file
                 << ": " << *sym;
  }
}

InputSection *ObjectFile::get_common_sec(Context &ctx) {
  if (!common_sec) {
    MachSection *hdr = new MachSection;
    common_hdr.reset(hdr);

    memset(hdr, 0, sizeof(*hdr));
    hdr->set_segname("__DATA");
    hdr->set_sectname("__common");
    hdr->type = S_ZEROFILL;

    common_sec = new InputSection(ctx, *this, *hdr);
    sections.push_back(std::unique_ptr<InputSection>(common_sec));
  }
  return common_sec;
}

DylibFile *DylibFile::create(Context &ctx, MappedFile<Context> *mf) {
  DylibFile *dylib = new DylibFile;
  dylib->mf = mf;
  ctx.dylib_pool.push_back(std::unique_ptr<DylibFile>(dylib));
  return dylib;
};

void DylibFile::read_trie(Context &ctx, u8 *start, i64 offset = 0,
                          const std::string &prefix = "") {
  u8 *buf = start + offset;

  if (*buf) {
    read_uleb(buf); // size
    read_uleb(buf); // flags
    read_uleb(buf); // addr
    syms.push_back(intern(ctx, prefix));
  } else {
    buf++;
  }

  i64 nchild = *buf++;

  for (i64 i = 0; i < nchild; i++) {
    std::string suffix((char *)buf);
    buf += suffix.size() + 1;
    i64 off = read_uleb(buf);
    read_trie(ctx, start, off, prefix + suffix);
  }
}

void DylibFile::parse_dylib(Context &ctx) {
  MachHeader &hdr = *(MachHeader *)mf->data;
  u8 *p = mf->data + sizeof(hdr);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;

    switch (lc.cmd) {
    case LC_ID_DYLIB: {
      DylibCommand &cmd = *(DylibCommand *)p;
      install_name = (char *)p + cmd.nameoff;
      break;
    }
    case LC_DYLD_INFO_ONLY: {
      DyldInfoCommand &cmd = *(DyldInfoCommand *)p;
      if (cmd.export_off)
        read_trie(ctx, mf->data + cmd.export_off);
      break;
    }
    case LC_DYLD_EXPORTS_TRIE: {
      LinkEditDataCommand &cmd = *(LinkEditDataCommand *)p;
      read_trie(ctx, mf->data + cmd.dataoff);
      break;
    }
    }

    p += lc.cmdsize;
  }
}

void DylibFile::parse(Context &ctx) {
  switch (get_file_type(mf)) {
  case FileType::TAPI: {
    TextDylib tbd = parse_tbd(ctx, mf);
    for (std::string_view sym : tbd.exports)
      syms.push_back(intern(ctx, sym));
    install_name = tbd.install_name;
    break;
  }
  case FileType::MACH_DYLIB:
    parse_dylib(ctx);
    break;
  default:
    Fatal(ctx) << mf->name << ": is not a dylib";
  }
}

void DylibFile::resolve_symbols(Context &ctx) {
  for (Symbol *sym : syms) {
    std::lock_guard lock(sym->mu);
    if (sym->file && sym->file->priority < priority)
      continue;
    sym->file = this;
    sym->is_extern = true;
  }
}

} // namespace mold::macho
