#include "mold.h"

#include <functional>
#include <map>
#include <signal.h>
#include <tbb/global_control.h>
#include <tbb/parallel_do.h>
#include <tbb/parallel_for_each.h>
#include <unordered_set>

i64 BuildId::size() const {
  switch (kind) {
  case HEX:
    return value.size();
  case HASH:
    return hash_size;
  case UUID:
    return 16;
  }
  unreachable();
}

static bool is_text_file(MemoryMappedFile *mb) {
  return mb->size() >= 4 &&
         isprint(mb->data()[0]) &&
         isprint(mb->data()[1]) &&
         isprint(mb->data()[2]) &&
         isprint(mb->data()[3]);
}

enum class FileType { UNKNOWN, OBJ, DSO, AR, THIN_AR, TEXT };

static FileType get_file_type(MemoryMappedFile *mb) {
  if (mb->size() >= 20 && memcmp(mb->data(), "\177ELF", 4) == 0) {
    ElfEhdr &ehdr = *(ElfEhdr *)mb->data();
    if (ehdr.e_type == ET_REL)
      return FileType::OBJ;
    if (ehdr.e_type == ET_DYN)
      return FileType::DSO;
    return FileType::UNKNOWN;
  }

  if (mb->size() >= 8 && memcmp(mb->data(), "!<arch>\n", 8) == 0)
    return FileType::AR;
  if (mb->size() >= 8 && memcmp(mb->data(), "!<thin>\n", 8) == 0)
    return FileType::THIN_AR;
  if (is_text_file(mb))
    return FileType::TEXT;
  return FileType::UNKNOWN;
}

static ObjectFile *new_object_file(MemoryMappedFile *mb,
                                   std::string archive_name,
                                   ReadContext &ctx) {
  static Counter count("parsed_objs");
  count++;

  bool in_lib = (!archive_name.empty() && !ctx.whole_archive);
  ObjectFile *file = new ObjectFile(mb, archive_name, in_lib);
  ctx.tg.run([=]() { file->parse(); });
  if (config.trace)
    SyncOut() << "trace: " << *file;
  return file;
}

static SharedFile *new_shared_file(MemoryMappedFile *mb, ReadContext &ctx) {
  SharedFile *file = new SharedFile(mb, ctx.as_needed);
  ctx.tg.run([=]() { file->parse(); });
  if (config.trace)
    SyncOut() << "trace: " << *file;
  return file;
}

template <typename T>
class FileCache {
public:
  void store(MemoryMappedFile *mb, T *obj) {
    Key k(mb->name, mb->size(), mb->mtime);
    cache[k].push_back(obj);
  }

  std::vector<T *> get(MemoryMappedFile *mb) {
    Key k(mb->name, mb->size(), mb->mtime);
    std::vector<T *> objs = cache[k];
    cache[k].clear();
    return objs;
  }

  T *get_one(MemoryMappedFile *mb) {
    std::vector<T *> objs = get(mb);
    return objs.empty() ? nullptr : objs[0];
  }

private:
  typedef std::tuple<std::string, i64, i64> Key;
  std::map<Key, std::vector<T *>> cache;
};

void read_file(MemoryMappedFile *mb, ReadContext &ctx) {
  if (ctx.visited.contains(mb->name))
    return;

  static FileCache<ObjectFile> obj_cache;
  static FileCache<SharedFile> dso_cache;

  if (ctx.is_preloading) {
    switch (get_file_type(mb)) {
    case FileType::OBJ:
      obj_cache.store(mb, new_object_file(mb, "", ctx));
      return;
    case FileType::DSO:
      dso_cache.store(mb, new_shared_file(mb, ctx));
      return;
    case FileType::AR:
      for (MemoryMappedFile *child : read_fat_archive_members(mb))
        obj_cache.store(mb, new_object_file(child, mb->name, ctx));
      return;
    case FileType::THIN_AR:
      for (MemoryMappedFile *child : read_thin_archive_members(mb))
        obj_cache.store(child, new_object_file(child, mb->name, ctx));
      return;
    case FileType::TEXT:
      parse_linker_script(mb, ctx);
      return;
    }
    Fatal() << mb->name << ": unknown file type";
  }

  switch (get_file_type(mb)) {
  case FileType::OBJ:
    if (ObjectFile *obj = obj_cache.get_one(mb))
      out::objs.push_back(obj);
    else
      out::objs.push_back(new_object_file(mb, "", ctx));
    return;
  case FileType::DSO:
    if (SharedFile *obj = dso_cache.get_one(mb))
      out::dsos.push_back(obj);
    else
      out::dsos.push_back(new_shared_file(mb, ctx));
    ctx.visited.insert(mb->name);
    return;
  case FileType::AR:
    if (std::vector<ObjectFile *> objs = obj_cache.get(mb); !objs.empty()) {
      append(out::objs, objs);
    } else {
      for (MemoryMappedFile *child : read_archive_members(mb))
        out::objs.push_back(new_object_file(child, mb->name, ctx));
    }
    ctx.visited.insert(mb->name);
    return;
  case FileType::THIN_AR:
    for (MemoryMappedFile *child : read_thin_archive_members(mb)) {
      if (ObjectFile *obj = obj_cache.get_one(child))
        out::objs.push_back(obj);
      else
        out::objs.push_back(new_object_file(child, mb->name, ctx));
    }
    ctx.visited.insert(mb->name);
    return;
  case FileType::TEXT:
    parse_linker_script(mb, ctx);
    return;
  }
  Fatal() << mb->name << ": unknown file type";
}

template <typename T>
static std::vector<std::span<T>> split(std::vector<T> &input, i64 unit) {
  assert(input.size() > 0);
  std::span<T> span(input);
  std::vector<std::span<T>> vec;

  while (span.size() >= unit) {
    vec.push_back(span.subspan(0, unit));
    span = span.subspan(unit);
  }
  if (!span.empty())
    vec.push_back(span);
  return vec;
}

static void apply_exclude_libs() {
  Timer t("apply_exclude_libs");

  if (config.exclude_libs.empty())
    return;

  std::unordered_set<std::string_view> set(config.exclude_libs.begin(),
                                           config.exclude_libs.end());

  for (ObjectFile *file : out::objs)
    if (!file->archive_name.empty())
      if (set.contains("ALL") || set.contains(file->archive_name))
        file->exclude_libs = true;
}

static void create_synthetic_sections() {
  auto add = [](OutputChunk *chunk) {
    out::chunks.push_back(chunk);
  };

  add(out::ehdr = new OutputEhdr);
  add(out::phdr = new OutputPhdr);
  add(out::shdr = new OutputShdr);
  add(out::got = new GotSection);
  add(out::gotplt = new GotPltSection);
  add(out::relplt = new RelPltSection);
  if (!config.strip_all)
    add(out::strtab = new StrtabSection);
  add(out::shstrtab = new ShstrtabSection);
  add(out::plt = new PltSection);
  add(out::pltgot = new PltGotSection);
  if (!config.strip_all)
    add(out::symtab = new SymtabSection);
  add(out::dynsym = new DynsymSection);
  add(out::dynstr = new DynstrSection);
  add(out::eh_frame = new EhFrameSection);
  add(out::copyrel = new CopyrelSection(".dynbss"));
  add(out::copyrel_relro = new CopyrelSection(".dynbss.rel.ro"));

  if (!config.dynamic_linker.empty())
    add(out::interp = new InterpSection);
  if (config.build_id.kind != BuildId::NONE)
    add(out::buildid = new BuildIdSection);
  if (config.eh_frame_hdr)
    add(out::eh_frame_hdr = new EhFrameHdrSection);
  if (config.hash_style_sysv)
    add(out::hash = new HashSection);
  if (config.hash_style_gnu)
    add(out::gnu_hash = new GnuHashSection);
  if (!config.version_definitions.empty())
    add(out::verdef = new VerdefSection);

  if (!config.is_static) {
    add(out::dynamic = new DynamicSection);
    add(out::reldyn = new RelDynSection);
    add(out::versym = new VersymSection);
    add(out::verneed = new VerneedSection);
  }
}

static void set_file_priority() {
  // File priority 1 is reserved for the internal file.
  i64 priority = 2;

  for (ObjectFile *file : out::objs)
    if (!file->is_in_lib)
      file->priority = priority++;
  for (ObjectFile *file : out::objs)
    if (file->is_in_lib)
      file->priority = priority++;
  for (SharedFile *file : out::dsos)
    file->priority = priority++;
}

static void resolve_obj_symbols() {
  Timer t("resolve_obj_symbols");

  // Register archive symbols
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    if (file->is_in_lib)
      file->resolve_lazy_symbols();
  });

  // Register defined symbols
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    if (!file->is_in_lib)
      file->resolve_regular_symbols();
  });

  // Mark reachable objects to decide which files to include
  // into an output.
  std::vector<ObjectFile *> roots;
  for (ObjectFile *file : out::objs)
    if (file->is_alive)
      roots.push_back(file);

  for (std::string_view name : config.undefined)
    if (InputFile *file = Symbol::intern(name)->file)
      if (!file->is_alive.exchange(true) && !file->is_dso)
        roots.push_back((ObjectFile *)file);

  tbb::parallel_do(roots,
                   [&](ObjectFile *file,
                       tbb::parallel_do_feeder<ObjectFile *> &feeder) {
                     file->mark_live_objects(
                       [&](ObjectFile *obj) { feeder.add(obj); });
                   });

  // Remove symbols of eliminated objects.
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    Symbol null_sym;
    if (!file->is_alive)
      for (Symbol *sym : file->get_global_syms())
        if (sym->file == file)
          sym->clear();
  });

  // Eliminate unused archive members.
  erase(out::objs, [](InputFile *file) { return !file->is_alive; });
}

static void resolve_dso_symbols() {
  Timer t("resolve_dso_symbols");

  // Register DSO symbols
  tbb::parallel_for_each(out::dsos, [](SharedFile *file) {
    file->resolve_symbols();
  });

  // Mark live DSOs
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym &esym = file->elf_syms[i];
      if (esym.is_defined())
        continue;

      Symbol &sym = *file->symbols[i];
      if (!sym.file || !sym.file->is_dso)
        continue;

      sym.file->is_alive = true;

      if (esym.st_bind != STB_WEAK) {
        std::lock_guard lock(sym.mu);
        sym.is_weak = false;
      }
    }
  });

  // Remove symbols of unreferenced DSOs.
  tbb::parallel_for_each(out::dsos, [](SharedFile *file) {
    Symbol null_sym;
    if (!file->is_alive)
      for (Symbol *sym : file->symbols)
        if (sym->file == file)
          sym->clear();
  });

  // Remove unreferenced DSOs
  erase(out::dsos, [](InputFile *file) { return !file->is_alive; });
}

static void eliminate_comdats() {
  Timer t("eliminate_comdats");

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->resolve_comdat_groups();
  });

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->eliminate_duplicate_comdat_groups();
  });
}

static void convert_common_symbols() {
  Timer t("convert_common_symbols");

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->convert_common_symbols();
  });
}

static void compute_merged_section_sizes() {
  Timer t("compute_merged_section_sizes");

  // Add an identification string to .comment.
  const char *verstr = "mold linker";
  MergedSection *sec =
    MergedSection::get_instance(".comment", SHT_PROGBITS, 0);
  sec->insert(std::string_view(verstr, strlen(verstr) + 1), 1);

  tbb::parallel_for_each(MergedSection::instances, [](MergedSection *sec) {
    sec->assign_offsets();
  });
}

// So far, each input section has a pointer to its corresponding
// output section, but there's no reverse edge to get a list of
// input sections from an output section. This function creates it.
//
// An output section may contain millions of input sections.
// So, we append input sections to output sections in parallel.
static void bin_sections() {
  Timer t("bin_sections");

  i64 unit = (out::objs.size() + 127) / 128;
  std::vector<std::span<ObjectFile *>> slices = split(out::objs, unit);

  i64 num_osec = OutputSection::instances.size();

  std::vector<std::vector<std::vector<InputSection *>>> groups(slices.size());
  for (i64 i = 0; i < groups.size(); i++)
    groups[i].resize(num_osec);

  tbb::parallel_for((i64)0, (i64)slices.size(), [&](i64 i) {
    for (ObjectFile *file : slices[i])
      for (InputSection *isec : file->sections)
        if (isec)
          groups[i][isec->output_section->idx].push_back(isec);
  });

  std::vector<i64> sizes(num_osec);

  for (std::span<std::vector<InputSection *>> group : groups)
    for (i64 i = 0; i < group.size(); i++)
      sizes[i] += group[i].size();

  tbb::parallel_for((i64)0, num_osec, [&](i64 j) {
    OutputSection::instances[j]->members.reserve(sizes[j]);
    for (i64 i = 0; i < groups.size(); i++)
      append(OutputSection::instances[j]->members, groups[i][j]);
  });
}

static void check_duplicate_symbols() {
  Timer t("check_dup_syms");

  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym &esym = file->elf_syms[i];
      Symbol &sym = *file->symbols[i];
      bool is_weak = (esym.st_bind == STB_WEAK);
      bool is_eliminated =
        !esym.is_abs() && !esym.is_common() && !file->get_section(esym);

      if (esym.is_defined() && !is_weak && !is_eliminated && sym.file != file)
        Error() << "duplicate symbol: " << *file << ": " << *sym.file
                << ": " << sym;
    }
  });

  Error::checkpoint();
}

std::vector<OutputChunk *> collect_output_sections() {
  std::vector<OutputChunk *> vec;

  for (OutputSection *osec : OutputSection::instances)
    if (!osec->members.empty())
      vec.push_back(osec);
  for (MergedSection *osec : MergedSection::instances)
    if (osec->shdr.sh_size)
      vec.push_back(osec);

  // Sections are added to the section lists in an arbitrary order because
  // they are created in parallel.
  // Sort them to to make the output deterministic.
  sort(vec, [](OutputChunk *x, OutputChunk *y) {
    return std::tuple(x->name, x->shdr.sh_type, x->shdr.sh_flags) <
           std::tuple(y->name, y->shdr.sh_type, y->shdr.sh_flags);
  });
  return vec;
}

static void compute_section_sizes() {
  Timer t("compute_section_sizes");

  tbb::parallel_for_each(OutputSection::instances, [&](OutputSection *osec) {
    if (osec->members.empty())
      return;

    std::vector<std::span<InputSection *>> slices =
      split(osec->members, 10000);

    std::vector<i64> size(slices.size());
    std::vector<i64> alignments(slices.size());

    tbb::parallel_for((i64)0, (i64)slices.size(), [&](i64 i) {
      i64 off = 0;
      i64 align = 1;

      for (InputSection *isec : slices[i]) {
        off = align_to(off, isec->shdr.sh_addralign);
        isec->offset = off;
        off += isec->shdr.sh_size;
        align = std::max<i64>(align, isec->shdr.sh_addralign);
      }

      size[i] = off;
      alignments[i] = align;
    });

    i64 align = *std::max_element(alignments.begin(), alignments.end());

    std::vector<i64> start(slices.size());
    for (i64 i = 1; i < slices.size(); i++)
      start[i] = align_to(start[i - 1] + size[i - 1], align);

    tbb::parallel_for((i64)1, (i64)slices.size(), [&](i64 i) {
      for (InputSection *isec : slices[i])
        isec->offset += start[i];
    });

    osec->shdr.sh_size = start.back() + size.back();
    osec->shdr.sh_addralign = align;
  });
}

static void convert_undefined_weak_symbols() {
  Timer t("undef_weak");

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->convert_undefined_weak_symbols();
  });
}

static void scan_rels() {
  Timer t("scan_rels");

  // Scan relocations to find dynamic symbols.
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    file->scan_relocations();
  });

  // Exit if there was a relocation that refers an undefined symbol.
  Error::checkpoint();

  // Add imported or exported symbols to .dynsym.
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (Symbol *sym : file->get_global_syms())
      if (sym->file == file)
        if (sym->is_imported || sym->is_exported)
          sym->flags |= NEEDS_DYNSYM;
  });

  // Aggregate dynamic symbols to a single vector.
  std::vector<InputFile *> files;
  append(files, out::objs);
  append(files, out::dsos);

  std::vector<std::vector<Symbol *>> vec(files.size());

  tbb::parallel_for((i64)0, (i64)files.size(), [&](i64 i) {
    for (Symbol *sym : files[i]->symbols)
      if (sym->flags && sym->file == files[i])
        vec[i].push_back(sym);
  });

  // Assign offsets in additional tables for each dynamic symbol.
  for (Symbol *sym : flatten(vec)) {
    if (sym->flags & NEEDS_DYNSYM)
      out::dynsym->add_symbol(sym);

    if (sym->flags & NEEDS_GOT)
      out::got->add_got_symbol(sym);

    if (sym->flags & NEEDS_PLT) {
      if (sym->flags & NEEDS_GOT)
        out::pltgot->add_symbol(sym);
      else
        out::plt->add_symbol(sym);
    }

    if (sym->flags & NEEDS_GOTTPOFF)
      out::got->add_gottpoff_symbol(sym);

    if (sym->flags & NEEDS_TLSGD)
      out::got->add_tlsgd_symbol(sym);

    if (sym->flags & NEEDS_TLSLD)
      out::got->add_tlsld();

    if (sym->flags & NEEDS_COPYREL) {
      assert(sym->file->is_dso);
      SharedFile *file = (SharedFile *)sym->file;
      sym->copyrel_readonly = file->is_readonly(sym);

      if (sym->copyrel_readonly)
        out::copyrel_relro->add_symbol(sym);
      else
        out::copyrel->add_symbol(sym);

      for (Symbol *alias : file->find_aliases(sym)) {
        alias->has_copyrel = true;
        alias->value = sym->value;
        alias->copyrel_readonly = sym->copyrel_readonly;
        out::dynsym->add_symbol(alias);
      }
    }
  }
}

static void apply_version_script() {
  Timer t("apply_version_script");

  for (std::pair<std::string_view, i16> pair : config.version_patterns) {
    std::string_view pattern = pair.first;
    i16 veridx = pair.second;
    assert(pattern != "*");

    if (pattern.find('*') == pattern.npos) {
      Symbol::intern(pattern)->ver_idx = veridx;
      continue;
    }

    GlobPattern glob(pattern);

    tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
      for (Symbol *sym : file->get_global_syms())
        if (sym->file == file && glob.match(sym->name))
          sym->ver_idx = veridx;
    });
  }
}

static void parse_symbol_version() {
  Timer t("parse_symbol_version");

  std::unordered_map<std::string_view, u16> verdefs;
  for (i64 i = 0; i < config.version_definitions.size(); i++)
    verdefs[config.version_definitions[i]] = i + VER_NDX_LAST_RESERVED + 1;

  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (i64 i = 0; i < file->symbols.size() - file->first_global; i++) {
      if (!file->symvers[i])
        continue;

      Symbol *sym = file->symbols[i + file->first_global];
      if (sym->file != file)
        continue;

      std::string_view ver = file->symvers[i];

      bool is_default = false;
      if (ver.starts_with('@')) {
        is_default = true;
        ver = ver.substr(1);
      }

      auto it = verdefs.find(ver);
      if (it == verdefs.end()) {
        Error() << *file << ": symbol " << *sym <<  " has undefined version "
                << ver;
        continue;
      }

      sym->ver_idx = it->second;
      if (!is_default)
        sym->ver_idx |= VERSYM_HIDDEN;
    }
  });
}

static void compute_import_export() {
  Timer t("compute_import_export");

  // Export symbols referenced by DSOs.
  if (!config.shared) {
    tbb::parallel_for_each(out::dsos, [&](SharedFile *file) {
      for (Symbol *sym : file->undefs)
        if (sym->file && !sym->file->is_dso && sym->visibility != STV_HIDDEN)
          sym->is_exported = true;
    });
  }

  // Global symbols are exported from DSO by default.
  if (config.shared || config.export_dynamic) {
    tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
      for (Symbol *sym : file->get_global_syms()) {
        if (sym->file != file)
          continue;

        if (sym->visibility == STV_HIDDEN || sym->ver_idx == VER_NDX_LOCAL)
          continue;

        sym->is_exported = true;

        if (config.shared && sym->visibility != STV_PROTECTED &&
            !config.Bsymbolic &&
            !(config.Bsymbolic_functions && sym->get_type() == STT_FUNC))
          sym->is_imported = true;
      }
    });
  }
}

static void fill_verdef() {
  Timer t("fill_verdef");

  if (config.version_definitions.empty())
    return;

  // Resize .gnu.version
  out::versym->contents.resize(out::dynsym->symbols.size(), 1);
  out::versym->contents[0] = 0;

  // Allocate a buffer for .gnu.version_d.
  out::verdef->contents.resize((sizeof(ElfVerdef) + sizeof(ElfVerdaux)) *
                               (config.version_definitions.size() + 1));

  u8 *buf = (u8 *)&out::verdef->contents[0];
  u8 *ptr = buf;
  ElfVerdef *verdef = nullptr;

  auto write = [&](std::string_view verstr, i64 idx, i64 flags) {
    out::verdef->shdr.sh_info++;
    if (verdef)
      verdef->vd_next = ptr - (u8 *)verdef;

    verdef = (ElfVerdef *)ptr;
    ptr += sizeof(ElfVerdef);

    verdef->vd_version = 1;
    verdef->vd_flags = flags;
    verdef->vd_ndx = idx;
    verdef->vd_cnt = 1;
    verdef->vd_hash = elf_hash(verstr);
    verdef->vd_aux = sizeof(ElfVerdef);

    ElfVerdaux *aux = (ElfVerdaux *)ptr;
    ptr += sizeof(ElfVerdaux);
    aux->vda_name = out::dynstr->add_string(verstr);
  };

  std::string_view basename = config.soname.empty() ?
    config.output : config.soname;
  write(basename, 1, VER_FLG_BASE);

  i64 idx = 2;
  for (std::string_view verstr : config.version_definitions)
    write(verstr, idx++, 0);

  for (Symbol *sym : std::span(out::dynsym->symbols).subspan(1))
    out::versym->contents[sym->dynsym_idx] = sym->ver_idx;
}

static void fill_verneed() {
  Timer t("fill_verneed");

  // Create a list of versioned symbols and sort by file and version.
  std::vector<Symbol *> syms(out::dynsym->symbols.begin() + 1,
                             out::dynsym->symbols.end());

  erase(syms, [](Symbol *sym) {
    return !sym->file->is_dso || sym->ver_idx <= VER_NDX_LAST_RESERVED;
  });

  if (syms.empty())
    return;

  sort(syms, [](Symbol *a, Symbol *b) {
    return std::tuple(((SharedFile *)a->file)->soname, a->ver_idx) <
           std::tuple(((SharedFile *)b->file)->soname, b->ver_idx);
  });

  // Resize of .gnu.version
  out::versym->contents.resize(out::dynsym->symbols.size(), 1);
  out::versym->contents[0] = 0;

  // Allocate a large enough buffer for .gnu.version_r.
  out::verneed->contents.resize((sizeof(ElfVerneed) + sizeof(ElfVernaux)) *
                                syms.size());

  // Fill .gnu.versoin_r.
  u8 *buf = (u8 *)&out::verneed->contents[0];
  u8 *ptr = buf;
  ElfVerneed *verneed = nullptr;
  ElfVernaux *aux = nullptr;

  u16 veridx = VER_NDX_LAST_RESERVED + config.version_definitions.size();

  auto start_group = [&](InputFile *file) {
    out::verneed->shdr.sh_info++;
    if (verneed)
      verneed->vn_next = ptr - (u8 *)verneed;

    verneed = (ElfVerneed *)ptr;
    ptr += sizeof(*verneed);
    verneed->vn_version = 1;
    verneed->vn_file = out::dynstr->find_string(((SharedFile *)file)->soname);
    verneed->vn_aux = sizeof(ElfVerneed);
    aux = nullptr;
  };

  auto add_entry = [&](Symbol *sym) {
    verneed->vn_cnt++;

    if (aux)
      aux->vna_next = sizeof(ElfVernaux);
    aux = (ElfVernaux *)ptr;
    ptr += sizeof(*aux);

    std::string_view verstr = sym->get_version();
    aux->vna_hash = elf_hash(verstr);
    aux->vna_other = ++veridx;
    aux->vna_name = out::dynstr->add_string(verstr);
  };

  for (i64 i = 0; i < syms.size(); i++) {
    if (i == 0 || syms[i - 1]->file != syms[i]->file) {
      start_group(syms[i]->file);
      add_entry(syms[i]);
    } else if (syms[i - 1]->ver_idx != syms[i]->ver_idx) {
      add_entry(syms[i]);
    }

    out::versym->contents[syms[i]->dynsym_idx] = veridx;
  }

  // Resize .gnu.version_r to fit to its contents.
  out::verneed->contents.resize(ptr - buf);
}

static void clear_padding(i64 filesize) {
  Timer t("clear_padding");

  auto zero = [](OutputChunk *chunk, i64 next_start) {
    i64 pos = chunk->shdr.sh_offset;
    if (chunk->shdr.sh_type != SHT_NOBITS)
      pos += chunk->shdr.sh_size;
    memset(out::buf + pos, 0, next_start - pos);
  };

  for (i64 i = 1; i < out::chunks.size(); i++)
    zero(out::chunks[i - 1], out::chunks[i]->shdr.sh_offset);
  zero(out::chunks.back(), filesize);
}

// We want to sort output chunks in the following order.
//
//   ELF header
//   program header
//   .interp
//   note
//   alloc readonly data
//   alloc readonly code
//   alloc writable tdata
//   alloc writable tbss
//   alloc writable RELRO data
//   alloc writable RELRO bss
//   alloc writable non-RELRO data
//   alloc writable non-RELRO bss
//   nonalloc
//   section header
static i64 get_section_rank(OutputChunk *chunk) {
  if (chunk == out::ehdr)
    return 0;
  if (chunk == out::phdr)
    return 1;
  if (chunk == out::interp)
    return 2;
  if (chunk == out::shdr)
    return 1 << 20;

  u64 type = chunk->shdr.sh_type;
  u64 flags = chunk->shdr.sh_flags;

  if (type == SHT_NOTE)
    return 3;
  if (!(flags & SHF_ALLOC))
    return (1 << 20) - 1;

  bool reaodnly = !(flags & SHF_WRITE);
  bool exec = (flags & SHF_EXECINSTR);
  bool tls = (flags & SHF_TLS);
  bool relro = is_relro(chunk);
  bool hasbits = !(type == SHT_NOBITS);

  return ((!reaodnly << 9) | (exec << 8) | (!tls << 7) |
          (!relro << 6) | (!hasbits << 5)) + 4;
}

static i64 set_osec_offsets(std::span<OutputChunk *> chunks) {
  Timer t("osec_offset");

  i64 fileoff = 0;
  i64 vaddr = config.image_base;

  for (OutputChunk *chunk : chunks) {
    if (chunk->new_page)
      vaddr = align_to(vaddr, PAGE_SIZE);

    if (vaddr % PAGE_SIZE > fileoff % PAGE_SIZE)
      fileoff += vaddr % PAGE_SIZE - fileoff % PAGE_SIZE;
    else if (vaddr % PAGE_SIZE < fileoff % PAGE_SIZE)
      fileoff = align_to(fileoff, PAGE_SIZE) + vaddr % PAGE_SIZE;

    fileoff = align_to(fileoff, chunk->shdr.sh_addralign);
    vaddr = align_to(vaddr, chunk->shdr.sh_addralign);

    chunk->shdr.sh_offset = fileoff;
    if (chunk->shdr.sh_flags & SHF_ALLOC)
      chunk->shdr.sh_addr = vaddr;

    bool is_bss = chunk->shdr.sh_type == SHT_NOBITS;
    if (!is_bss)
      fileoff += chunk->shdr.sh_size;

    bool is_tbss = is_bss && (chunk->shdr.sh_flags & SHF_TLS);
    if (!is_tbss)
      vaddr += chunk->shdr.sh_size;

    if (chunk->new_page_end)
      vaddr = align_to(vaddr, PAGE_SIZE);
  }
  return fileoff;
}

static void fix_synthetic_symbols(std::span<OutputChunk *> chunks) {
  auto start = [](Symbol *sym, OutputChunk *chunk) {
    if (sym && chunk) {
      sym->shndx = chunk->shndx;
      sym->value = chunk->shdr.sh_addr;
    }
  };

  auto stop = [](Symbol *sym, OutputChunk *chunk) {
    if (sym && chunk) {
      sym->shndx = chunk->shndx;
      sym->value = chunk->shdr.sh_addr + chunk->shdr.sh_size;
    }
  };

  // __bss_start
  for (OutputChunk *chunk : chunks) {
    if (chunk->kind == OutputChunk::REGULAR && chunk->name == ".bss") {
      start(out::__bss_start, chunk);
      break;
    }
  }

  // __ehdr_start and __executable_start
  for (OutputChunk *chunk : chunks) {
    if (chunk->shndx == 1) {
      out::__ehdr_start->shndx = 1;
      out::__ehdr_start->value = out::ehdr->shdr.sh_addr;

      out::__executable_start->shndx = 1;
      out::__executable_start->value = out::ehdr->shdr.sh_addr;
      break;
    }
  }

  // __rela_iplt_start and __rela_iplt_end
  start(out::__rela_iplt_start, out::relplt);
  stop(out::__rela_iplt_end, out::relplt);

  // __{init,fini}_array_{start,end}
  for (OutputChunk *chunk : chunks) {
    switch (chunk->shdr.sh_type) {
    case SHT_INIT_ARRAY:
      start(out::__init_array_start, chunk);
      stop(out::__init_array_end, chunk);
      break;
    case SHT_FINI_ARRAY:
      start(out::__fini_array_start, chunk);
      stop(out::__fini_array_end, chunk);
      break;
    }
  }

  // _end, _etext, _edata and the like
  for (OutputChunk *chunk : chunks) {
    if (chunk->kind == OutputChunk::HEADER)
      continue;

    if (chunk->shdr.sh_flags & SHF_ALLOC)
      stop(out::_end, chunk);

    if (chunk->shdr.sh_flags & SHF_EXECINSTR)
      stop(out::_etext, chunk);

    if (chunk->shdr.sh_type != SHT_NOBITS && chunk->shdr.sh_flags & SHF_ALLOC)
      stop(out::_edata, chunk);
  }

  // _DYNAMIC
  start(out::_DYNAMIC, out::dynamic);

  // _GLOBAL_OFFSET_TABLE_
  start(out::_GLOBAL_OFFSET_TABLE_, out::gotplt);

  // __GNU_EH_FRAME_HDR
  start(out::__GNU_EH_FRAME_HDR, out::eh_frame_hdr);

  // __start_ and __stop_ symbols
  for (OutputChunk *chunk : chunks) {
    if (is_c_identifier(chunk->name)) {
      start(Symbol::intern_alloc("__start_" + std::string(chunk->name)), chunk);
      stop(Symbol::intern_alloc("__stop_" + std::string(chunk->name)), chunk);
    }
  }
}

void cleanup() {
  if (OutputFile::tmpfile)
    unlink(OutputFile::tmpfile);
  if (socket_tmpfile)
    unlink(socket_tmpfile);
}

static void signal_handler(int) {
  cleanup();
  _exit(1);
}

MemoryMappedFile *find_library(std::string name,
                               std::span<std::string_view> lib_paths,
                               ReadContext &ctx) {
  for (std::string_view dir : lib_paths) {
    std::string root = dir.starts_with("/") ? config.sysroot : "";
    std::string stem = root + std::string(dir) + "/lib" + name;
    if (!ctx.is_static)
      if (MemoryMappedFile *mb = MemoryMappedFile::open(stem + ".so"))
        return mb;
    if (MemoryMappedFile *mb = MemoryMappedFile::open(stem + ".a"))
      return mb;
  }
  Fatal() << "library not found: " << name;
}

static void read_input_files(std::span<std::string_view> args,
                             ReadContext &ctx) {
  std::vector<std::tuple<bool, bool, bool>> state;

  while (!args.empty()) {
    std::string_view arg;

    if (read_flag(args, "as-needed")) {
      ctx.as_needed = true;
    } else if (read_flag(args, "no-as-needed")) {
      ctx.as_needed = false;
    } else if (read_flag(args, "whole-archive")) {
      ctx.whole_archive = true;
    } else if (read_flag(args, "no-whole-archive")) {
      ctx.whole_archive = false;
    } else if (read_flag(args, "Bstatic")) {
      ctx.is_static = true;
    } else if (read_flag(args, "Bdynamic")) {
      ctx.is_static = false;
    } else if (read_flag(args, "push-state")) {
      state.push_back({ctx.as_needed, ctx.whole_archive, ctx.is_static});
    } else if (read_flag(args, "pop-state")) {
      if (state.empty())
        Fatal() << "no state pushed before popping";
      std::tie(ctx.as_needed, ctx.whole_archive, ctx.is_static) = state.back();
      state.pop_back();
    } else if (read_arg(args, arg, "l")) {
      MemoryMappedFile *mb =
        find_library(std::string(arg), config.library_paths, ctx);
      read_file(mb, ctx);
    } else {
      read_file(MemoryMappedFile::must_open(std::string(args[0])), ctx);
      args = args.subspan(1);
    }
  }
}

static void show_stats() {
  for (ObjectFile *obj : out::objs) {
    static Counter defined("defined_syms");
    defined += obj->first_global - 1;

    static Counter undefined("undefined_syms");
    undefined += obj->symbols.size() - obj->first_global;
  }

  Counter num_input_sections("input_sections");
  for (ObjectFile *file : out::objs)
    num_input_sections += file->sections.size();

  Counter num_output_chunks("output_chunks", out::chunks.size());
  Counter num_objs("num_objs", out::objs.size());
  Counter num_dsos("num_dsos", out::dsos.size());

  Counter::print();
}

int main(int argc, char **argv) {
  Timer t_all("all");

  // Parse non-positional command line options
  std::vector<std::string_view> arg_vector = expand_response_files(argv + 1);
  std::vector<std::string_view> file_args;
  parse_nonpositional_args(arg_vector, file_args);

  if (!config.preload)
    if (i64 code; resume_daemon(argv, &code))
      exit(code);

  tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism,
                               config.thread_count);

  signal(SIGINT, signal_handler);
  signal(SIGTERM, signal_handler);

  // Preload input files
  std::function<void()> on_complete;

  if (config.preload) {
    Timer t("preload");
    std::function<void()> wait_for_client;
    daemonize(argv, &wait_for_client, &on_complete);

    ReadContext ctx(true);
    read_input_files(file_args, ctx);
    ctx.tg.wait();
    t.stop();

    Timer t2("wait_for_client");
    wait_for_client();
  } else if (config.fork) {
    on_complete = fork_child();
  }

  for (std::string_view arg : config.trace_symbol)
    Symbol::intern(arg)->traced = true;

  // Parse input files
  {
    Timer t("parse");
    ReadContext ctx(false);
    read_input_files(file_args, ctx);
    ctx.tg.wait();
  }

  // Uniquify shared object files with soname
  {
    std::vector<SharedFile *> vec;
    std::unordered_set<std::string_view> seen;
    for (SharedFile *file : out::dsos)
      if (seen.insert(file->soname).second)
        vec.push_back(file);
    out::dsos = vec;
  }

  // Apply -exclude-libs
  apply_exclude_libs();

  Timer t_total("total");
  Timer t_before_copy("before_copy");

  // Create instances of linker-synthesized sections such as
  // .got or .plt.
  create_synthetic_sections();

  // Set priorities to files.
  set_file_priority();

  // Resolve symbols and fix the set of object files that are
  // included to the final output.
  resolve_obj_symbols();

  // Remove redundant comdat sections (e.g. duplicate inline functions).
  eliminate_comdats();

  // Create .bss sections for common symbols.
  convert_common_symbols();

  // Apply version scripts.
  apply_version_script();

  // Parse symbol version suffixes (e.g. "foo@ver1").
  parse_symbol_version();

  // Set is_import and is_export bits for each symbol.
  compute_import_export();

  // Garbage-collect unreachable sections.
  if (config.gc_sections)
    gc_sections();

  // Merge identical read-only sections.
  if (config.icf)
    icf_sections();

  // Compute sizes of sections containing mergeable strings.
  compute_merged_section_sizes();

  // Bin input sections into output sections
  bin_sections();

  // Get a list of output sections.
  append(out::chunks, collect_output_sections());

  // Create a dummy file containing linker-synthesized symbols
  // (e.g. `__bss_start`).
  out::internal_obj = new ObjectFile;
  out::internal_obj->resolve_regular_symbols();
  out::objs.push_back(out::internal_obj);

  // Add symbols from shared object files.
  resolve_dso_symbols();

  // Beyond this point, no new files will be added to out::objs
  // or out::dsos.

  // Compute sizes of output sections while assigning offsets
  //within an output section to input sections.
  compute_section_sizes();

  // Sort sections by section attributes so that we'll have to
  // create as few segments as possible.
  sort(out::chunks, [](OutputChunk *a, OutputChunk *b) {
    return get_section_rank(a) < get_section_rank(b);
  });

  // Convert weak symbols to absolute symbols with value 0.
  convert_undefined_weak_symbols();

  // If we are linking a .so file, remaining undefined symbols does
  // not cause a linker error. Instead, they are treated as if they
  // were imported symbols.
  if (config.shared && !config.z_defs) {
    Timer t("claim_unresolved_symbols");
    tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
      file->claim_unresolved_symbols();
    });
  }

  // Beyond this point, no new symbols will be added to the result.

  // Make sure that all symbols have been resolved.
  if (!config.allow_multiple_definition)
    check_duplicate_symbols();

  // Copy string referred by .dynamic to .dynstr.
  for (SharedFile *file : out::dsos)
    out::dynstr->add_string(file->soname);
  for (std::string_view str : config.auxiliary)
    out::dynstr->add_string(str);
  for (std::string_view str : config.filter)
    out::dynstr->add_string(str);
  if (!config.rpaths.empty())
    out::dynstr->add_string(config.rpaths);
  if (!config.soname.empty())
    out::dynstr->add_string(config.soname);

  // Scan relocations to find symbols that need entries in .got, .plt,
  // .got.plt, .dynsym, .dynstr, etc.
  scan_rels();

  // Sort .dynsym contents. Beyond this point, no symbol should be
  // added to .dynsym.
  out::dynsym->sort_symbols();

  // Fill .gnu.version_d section contents.
  fill_verdef();

  // Fill .gnu.version_r section contents.
  fill_verneed();

  // Compute .symtab and .strtab sizes for each file.
  {
    Timer t("compute_symtab");
    tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
      file->compute_symtab();
    });
  }

  // .eh_frame is a special section from the linker's point of view,
  // as its contents are parsed and reconstructed by the linker,
  // unlike other sections that are regarded as opaque bytes.
  // Here, we transplant .eh_frame sections from a regular output
  // section to the special EHFrameSection.
  {
    Timer t("eh_frame");
    erase(out::chunks, [](OutputChunk *chunk) {
      return chunk->kind == OutputChunk::REGULAR &&
             chunk->name == ".eh_frame";
    });
    out::eh_frame->construct();
  }

  // Now that we have computed sizes for all sections and assigned
  // section indices to them, so we can fix section header contents
  // for all output sections.
  for (OutputChunk *chunk : out::chunks)
    chunk->update_shdr();

  erase(out::chunks, [](OutputChunk *chunk) {
    return chunk->kind == OutputChunk::SYNTHETIC &&
           chunk->shdr.sh_size == 0;
  });

  // Set section indices.
  for (i64 i = 0, shndx = 1; i < out::chunks.size(); i++)
    if (out::chunks[i]->kind != OutputChunk::HEADER)
      out::chunks[i]->shndx = shndx++;

  for (OutputChunk *chunk : out::chunks)
    chunk->update_shdr();

  // Assign offsets to output sections
  i64 filesize = set_osec_offsets(out::chunks);

  // At this point, file layout is fixed. Beyond this, you can assume
  // that symbol addresses including their GOT/PLT/etc addresses have
  // a correct final value.

  // Fix linker-synthesized symbol addresses.
  fix_synthetic_symbols(out::chunks);

  // Some types of relocations for TLS symbols need the TLS segment
  // address. Find it out now.
  for (ElfPhdr phdr : create_phdr()) {
    if (phdr.p_type == PT_TLS) {
      out::tls_begin = phdr.p_vaddr;
      out::tls_end = align_to(phdr.p_vaddr + phdr.p_memsz, phdr.p_align);
      break;
    }
  }

  t_before_copy.stop();

  // Create an output file
  OutputFile *file = OutputFile::open(config.output, filesize);
  out::buf = file->buf;

  Timer t_copy("copy");

  // Copy input sections to the output file
  {
    Timer t("copy_buf");
    tbb::parallel_for_each(out::chunks, [&](OutputChunk *chunk) {
      chunk->copy_buf();
    });
    Error::checkpoint();
  }

  // Dynamic linker works better with sorted .rela.dyn section,
  // so we sort them.
  if (out::reldyn)
    out::reldyn->sort();

  // Zero-clear paddings between sections
  clear_padding(filesize);

  if (out::buildid) {
    Timer t("build_id");
    out::buildid->write_buildid(filesize);
  }

  t_copy.stop();

  // Commit
  file->close();

  t_total.stop();
  t_all.stop();

  if (config.print_map)
    print_map();

  // Show stats numbers
  if (config.stats)
    show_stats();

  if (config.perf)
    Timer::print();

  std::cout << std::flush;
  std::cerr << std::flush;
  if (on_complete)
    on_complete();

  if (config.quick_exit)
    std::quick_exit(0);
  return 0;
}
