#include "mold.h"

#include <functional>
#include <map>
#include <tbb/parallel_do.h>
#include <tbb/parallel_for_each.h>
#include <unordered_set>

template <typename E>
void apply_exclude_libs(Context<E> &ctx) {
  Timer t(ctx, "apply_exclude_libs");

  if (ctx.arg.exclude_libs.empty())
    return;

  std::unordered_set<std::string_view> set(ctx.arg.exclude_libs.begin(),
                                           ctx.arg.exclude_libs.end());

  for (ObjectFile<E> *file : ctx.objs)
    if (!file->archive_name.empty())
      if (set.contains("ALL") || set.contains(file->archive_name))
        file->exclude_libs = true;
}

template <typename E>
void create_synthetic_sections(Context<E> &ctx) {
  auto add = [&](auto &chunk) {
    ctx.chunks.push_back(chunk.get());
  };

  add(ctx.ehdr = std::make_unique<OutputEhdr<E>>());
  add(ctx.phdr = std::make_unique<OutputPhdr<E>>());
  add(ctx.shdr = std::make_unique<OutputShdr<E>>());
  add(ctx.got = std::make_unique<GotSection<E>>());
  add(ctx.gotplt = std::make_unique<GotPltSection<E>>());
  add(ctx.relplt = std::make_unique<RelPltSection<E>>());
  add(ctx.strtab = std::make_unique<StrtabSection<E>>());
  add(ctx.shstrtab = std::make_unique<ShstrtabSection<E>>());
  add(ctx.plt = std::make_unique<PltSection<E>>());
  add(ctx.pltgot = std::make_unique<PltGotSection<E>>());
  add(ctx.symtab = std::make_unique<SymtabSection<E>>());
  add(ctx.dynsym = std::make_unique<DynsymSection<E>>());
  add(ctx.dynstr = std::make_unique<DynstrSection<E>>());
  add(ctx.eh_frame = std::make_unique<EhFrameSection<E>>());
  add(ctx.dynbss = std::make_unique<DynbssSection<E>>(false));
  add(ctx.dynbss_relro = std::make_unique<DynbssSection<E>>(true));

  if (!ctx.arg.dynamic_linker.empty())
    add(ctx.interp = std::make_unique<InterpSection<E>>());
  if (ctx.arg.build_id.kind != BuildId::NONE)
    add(ctx.buildid = std::make_unique<BuildIdSection<E>>());
  if (ctx.arg.eh_frame_hdr)
    add(ctx.eh_frame_hdr = std::make_unique<EhFrameHdrSection<E>>());
  if (ctx.arg.hash_style_sysv)
    add(ctx.hash = std::make_unique<HashSection<E>>());
  if (ctx.arg.hash_style_gnu)
    add(ctx.gnu_hash = std::make_unique<GnuHashSection<E>>());
  if (!ctx.arg.version_definitions.empty())
    add(ctx.verdef = std::make_unique<VerdefSection<E>>());

  add(ctx.reldyn = std::make_unique<RelDynSection<E>>());
  add(ctx.dynamic = std::make_unique<DynamicSection<E>>());
  add(ctx.versym = std::make_unique<VersymSection<E>>());
  add(ctx.verneed = std::make_unique<VerneedSection<E>>());
}

template <typename E>
void set_file_priority(Context<E> &ctx) {
  // File priority 1 is reserved for the internal file.
  i64 priority = 2;

  for (ObjectFile<E> *file : ctx.objs)
    if (!file->is_in_lib)
      file->priority = priority++;
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_in_lib)
      file->priority = priority++;
  for (SharedFile<E> *file : ctx.dsos)
    file->priority = priority++;
}

template <typename E>
void resolve_obj_symbols(Context<E> &ctx) {
  Timer t(ctx, "resolve_obj_symbols");

  // Register archive symbols
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    if (file->is_in_lib)
      file->resolve_lazy_symbols(ctx);
  });

  // Register defined symbols
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    if (!file->is_in_lib)
      file->resolve_regular_symbols(ctx);
  });

  // Mark reachable objects to decide which files to include
  // into an output.
  std::vector<ObjectFile<E> *> roots;
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_alive)
      roots.push_back(file);

  for (std::string_view name : ctx.arg.undefined)
    if (InputFile<E> *file = Symbol<E>::intern(ctx, name)->file)
      if (!file->is_alive.exchange(true) && !file->is_dso)
        roots.push_back((ObjectFile<E> *)file);

  tbb::parallel_do(roots,
                   [&](ObjectFile<E> *file,
                       tbb::parallel_do_feeder<ObjectFile<E> *> &feeder) {
                     file->mark_live_objects(ctx, [&](ObjectFile<E> *obj) {
                       feeder.add(obj);
                     });
                   });

  // Remove symbols of eliminated objects.
  tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
    Symbol<E> null_sym;
    if (!file->is_alive)
      for (Symbol<E> *sym : file->get_global_syms())
        if (sym->file == file)
          sym->clear();
  });

  // Eliminate unused archive members.
  erase(ctx.objs, [](InputFile<E> *file) { return !file->is_alive; });
}

template <typename E>
void resolve_dso_symbols(Context<E> &ctx) {
  Timer t(ctx, "resolve_dso_symbols");

  // Register DSO symbols
  tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
    file->resolve_symbols(ctx);
  });

  // Mark live DSOs
  tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      if (esym.is_defined())
        continue;

      Symbol<E> &sym = *file->symbols[i];
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
  tbb::parallel_for_each(ctx.dsos, [](SharedFile<E> *file) {
    Symbol<E> null_sym;
    if (!file->is_alive)
      for (Symbol<E> *sym : file->symbols)
        if (sym->file == file)
          sym->clear();
  });

  // Remove unreferenced DSOs
  erase(ctx.dsos, [](InputFile<E> *file) { return !file->is_alive; });
}

template <typename E>
void eliminate_comdats(Context<E> &ctx) {
  Timer t(ctx, "eliminate_comdats");

  tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
    file->resolve_comdat_groups();
  });

  tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
    file->eliminate_duplicate_comdat_groups();
  });
}

template <typename E>
void convert_common_symbols(Context<E> &ctx) {
  Timer t(ctx, "convert_common_symbols");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->convert_common_symbols(ctx);
  });
}

template <typename E>
static std::string get_cmdline_args(Context<E> &ctx) {
  std::stringstream ss;
  ss << ctx.cmdline_args[0];
  for (std::string_view arg :
         std::span<std::string_view>(ctx.cmdline_args).subspan(1))
    ss << " " << arg;
  return ss.str();
}

template <typename E>
void add_comment_string(Context<E> &ctx, std::string str) {
  std::string_view buf = save_string(ctx, str);
  MergedSection<E> *sec =
    MergedSection<E>::get_instance(ctx, ".comment", SHT_PROGBITS, 0);
  SectionFragment<E> *frag = sec->insert({buf.data(), buf.size() + 1}, 1);
  frag->is_alive = true;
}

template <typename E>
void compute_merged_section_sizes(Context<E> &ctx) {
  Timer t(ctx, "compute_merged_section_sizes");

  // Mark section fragments referenced by live objects.
  if (!ctx.arg.gc_sections) {
    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      for (SectionFragment<E> *frag : file->fragments)
        frag->is_alive = true;
    });
  }

  // Add an identification string to .comment.
  add_comment_string(ctx, get_version_string());

  // Also embed command line arguments for now for debugging.
  add_comment_string(ctx, "mold command line: " + get_cmdline_args(ctx));

  tbb::parallel_for_each(ctx.merged_sections,
                         [](std::unique_ptr<MergedSection<E>> &sec) {
    sec->assign_offsets();
  });
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

// So far, each input section has a pointer to its corresponding
// output section, but there's no reverse edge to get a list of
// input sections from an output section. This function creates it.
//
// An output section may contain millions of input sections.
// So, we append input sections to output sections in parallel.
template <typename E>
void bin_sections(Context<E> &ctx) {
  Timer t(ctx, "bin_sections");

  i64 unit = (ctx.objs.size() + 127) / 128;
  std::vector<std::span<ObjectFile<E> *>> slices = split(ctx.objs, unit);

  i64 num_osec = ctx.output_sections.size();

  std::vector<std::vector<std::vector<InputSection<E> *>>> groups(slices.size());
  for (i64 i = 0; i < groups.size(); i++)
    groups[i].resize(num_osec);

  tbb::parallel_for((i64)0, (i64)slices.size(), [&](i64 i) {
    for (ObjectFile<E> *file : slices[i])
      for (std::unique_ptr<InputSection<E>> &isec : file->sections)
        if (isec && isec->is_alive)
          groups[i][isec->output_section->idx].push_back(isec.get());
  });

  std::vector<i64> sizes(num_osec);

  for (std::span<std::vector<InputSection<E> *>> group : groups)
    for (i64 i = 0; i < group.size(); i++)
      sizes[i] += group[i].size();

  tbb::parallel_for((i64)0, num_osec, [&](i64 j) {
    ctx.output_sections[j]->members.reserve(sizes[j]);
    for (i64 i = 0; i < groups.size(); i++)
      append(ctx.output_sections[j]->members, groups[i][j]);
  });
}

template <typename E>
void check_duplicate_symbols(Context<E> &ctx) {
  Timer t(ctx, "check_dup_syms");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];

      bool is_common = esym.is_common();
      bool is_weak = (esym.st_bind == STB_WEAK);
      bool is_eliminated = false;

      if (!esym.is_abs() && !esym.is_common())
        if (InputSection<E> *isec = file->get_section(esym))
          if (!isec->is_alive)
            is_eliminated = true;

      if (sym.file != file && esym.is_defined() && !is_common &&
          !is_weak && !is_eliminated)
        Error(ctx) << "duplicate symbol: " << *file << ": " << *sym.file
                   << ": " << sym;
    }
  });

  Error<E>::checkpoint(ctx);
}

template <typename E>
std::vector<OutputChunk<E> *> collect_output_sections(Context<E> &ctx) {
  std::vector<OutputChunk<E> *> vec;

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
    if (!osec->members.empty())
      vec.push_back(osec.get());
  for (std::unique_ptr<MergedSection<E>> &osec : ctx.merged_sections)
    if (osec->shdr.sh_size)
      vec.push_back(osec.get());

  // Sections are added to the section lists in an arbitrary order because
  // they are created in parallel.
  // Sort them to to make the output deterministic.
  sort(vec, [](OutputChunk<E> *x, OutputChunk<E> *y) {
    return std::tuple(x->name, x->shdr.sh_type, x->shdr.sh_flags) <
           std::tuple(y->name, y->shdr.sh_type, y->shdr.sh_flags);
  });
  return vec;
}

template <typename E>
void compute_section_sizes(Context<E> &ctx) {
  Timer t(ctx, "compute_section_sizes");

  tbb::parallel_for_each(ctx.output_sections,
                         [&](std::unique_ptr<OutputSection<E>> &osec) {
    if (osec->members.empty())
      return;

    std::vector<std::span<InputSection<E> *>> slices =
      split(osec->members, 10000);

    std::vector<i64> size(slices.size());
    std::vector<i64> alignments(slices.size());

    tbb::parallel_for((i64)0, (i64)slices.size(), [&](i64 i) {
      i64 off = 0;
      i64 align = 1;

      for (InputSection<E> *isec : slices[i]) {
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
      for (InputSection<E> *isec : slices[i])
        isec->offset += start[i];
    });

    osec->shdr.sh_size = start.back() + size.back();
    osec->shdr.sh_addralign = align;
  });
}

template <typename E>
void convert_undefined_weak_symbols(Context<E> &ctx) {
  Timer t(ctx, "undef_weak");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->convert_undefined_weak_symbols(ctx);
  });
}

template <typename E>
void scan_rels(Context<E> &ctx) {
  Timer t(ctx, "scan_rels");

  // Scan relocations to find dynamic symbols.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->scan_relocations(ctx);
  });

  // Exit if there was a relocation that refers an undefined symbol.
  Error<E>::checkpoint(ctx);

  // Add imported or exported symbols to .dynsym.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->get_global_syms())
      if (sym->file == file)
        if (sym->is_imported || sym->is_exported)
          sym->flags |= NEEDS_DYNSYM;
  });

  // Add symbol aliases for COPYREL.
  tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
    for (Symbol<E> *sym : file->symbols)
      if (sym->flags & NEEDS_COPYREL)
        for (Symbol<E> *alias : file->find_aliases(sym))
          alias->flags |= NEEDS_DYNSYM;
  });

  // Aggregate dynamic symbols to a single vector.
  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dsos);

  std::vector<std::vector<Symbol<E> *>> vec(files.size());

  tbb::parallel_for((i64)0, (i64)files.size(), [&](i64 i) {
    for (Symbol<E> *sym : files[i]->symbols)
      if (sym->flags && sym->file == files[i])
        vec[i].push_back(sym);
  });

  std::vector<Symbol<E> *> syms = flatten(vec);

  ctx.symbol_aux.resize(syms.size());
  for (i64 i = 0; i < syms.size(); i++)
    syms[i]->aux_idx = i;

  // Assign offsets in additional tables for each dynamic symbol.
  for (Symbol<E> *sym : syms) {
    if (sym->flags & NEEDS_DYNSYM)
      ctx.dynsym->add_symbol(ctx, sym);

    if (sym->flags & NEEDS_GOT)
      ctx.got->add_got_symbol(ctx, sym);

    if (sym->flags & NEEDS_PLT) {
      if (sym->flags & NEEDS_GOT)
        ctx.pltgot->add_symbol(ctx, sym);
      else
        ctx.plt->add_symbol(ctx, sym);
    }

    if (sym->flags & NEEDS_GOTTP)
      ctx.got->add_gottp_symbol(ctx, sym);

    if (sym->flags & NEEDS_TLSGD)
      ctx.got->add_tlsgd_symbol(ctx, sym);

    if (sym->flags & NEEDS_TLSDESC)
      ctx.got->add_tlsdesc_symbol(ctx, sym);

    if (sym->flags & NEEDS_TLSLD)
      ctx.got->add_tlsld(ctx);

    if (sym->flags & NEEDS_COPYREL) {
      assert(sym->file->is_dso);
      SharedFile<E> *file = (SharedFile<E> *)sym->file;
      sym->copyrel_readonly = file->is_readonly(ctx, sym);

      if (sym->copyrel_readonly)
        ctx.dynbss_relro->add_symbol(ctx, sym);
      else
        ctx.dynbss->add_symbol(ctx, sym);

      for (Symbol<E> *alias : file->find_aliases(sym)) {
        alias->has_copyrel = true;
        alias->value = sym->value;
        alias->copyrel_readonly = sym->copyrel_readonly;
        ctx.dynsym->add_symbol(ctx, alias);
      }
    }
  }
}

template <typename E>
void apply_version_script(Context<E> &ctx) {
  Timer t(ctx, "apply_version_script");

  for (VersionPattern &elem : ctx.arg.version_patterns) {
    assert(elem.pattern != "*");

    if (!elem.is_extern_cpp &&
        elem.pattern.find('*') == elem.pattern.npos) {
      Symbol<E>::intern(ctx, elem.pattern)->ver_idx = elem.ver_idx;
      continue;
    }

    GlobPattern glob(elem.pattern);

    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      for (Symbol<E> *sym : file->get_global_syms()) {
        if (sym->file == file) {
          std::string_view name = elem.is_extern_cpp
            ? sym->get_demangled_name() : sym->get_name();
          if (glob.match(name))
            sym->ver_idx = elem.ver_idx;
        }
      }
    });
  }
}

template <typename E>
void parse_symbol_version(Context<E> &ctx) {
  Timer t(ctx, "parse_symbol_version");

  std::unordered_map<std::string_view, u16> verdefs;
  for (i64 i = 0; i < ctx.arg.version_definitions.size(); i++)
    verdefs[ctx.arg.version_definitions[i]] = i + VER_NDX_LAST_RESERVED + 1;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (i64 i = 0; i < file->symbols.size() - file->first_global; i++) {
      if (!file->symvers[i])
        continue;

      Symbol<E> *sym = file->symbols[i + file->first_global];
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
        Error(ctx) << *file << ": symbol " << *sym <<  " has undefined version "
                   << ver;
        continue;
      }

      sym->ver_idx = it->second;
      if (!is_default)
        sym->ver_idx |= VERSYM_HIDDEN;
    }
  });
}

template <typename E>
void compute_import_export(Context<E> &ctx) {
  Timer t(ctx, "compute_import_export");

  // Export symbols referenced by DSOs.
  if (!ctx.arg.shared) {
    tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
      for (Symbol<E> *sym : file->undefs)
        if (sym->file && !sym->file->is_dso && sym->visibility != STV_HIDDEN)
          sym->is_exported = true;
    });
  }

  // Global symbols are exported from DSO by default.
  if (ctx.arg.shared || ctx.arg.export_dynamic) {
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      for (Symbol<E> *sym : file->get_global_syms()) {
        if (sym->file != file)
          continue;

        if (sym->visibility == STV_HIDDEN || sym->ver_idx == VER_NDX_LOCAL)
          continue;

        sym->is_exported = true;

        if (ctx.arg.shared && sym->visibility != STV_PROTECTED &&
            !ctx.arg.Bsymbolic &&
            !(ctx.arg.Bsymbolic_functions && sym->get_type() == STT_FUNC))
          sym->is_imported = true;
      }
    });
  }
}

template <typename E>
void fill_verdef(Context<E> &ctx) {
  Timer t(ctx, "fill_verdef");

  if (ctx.arg.version_definitions.empty())
    return;

  // Resize .gnu.version
  ctx.versym->contents.resize(ctx.dynsym->symbols.size(), 1);
  ctx.versym->contents[0] = 0;

  // Allocate a buffer for .gnu.version_d.
  ctx.verdef->contents.resize((sizeof(ElfVerdef<E>) + sizeof(ElfVerdaux<E>)) *
                               (ctx.arg.version_definitions.size() + 1));

  u8 *buf = (u8 *)&ctx.verdef->contents[0];
  u8 *ptr = buf;
  ElfVerdef<E> *verdef = nullptr;

  auto write = [&](std::string_view verstr, i64 idx, i64 flags) {
    ctx.verdef->shdr.sh_info++;
    if (verdef)
      verdef->vd_next = ptr - (u8 *)verdef;

    verdef = (ElfVerdef<E> *)ptr;
    ptr += sizeof(ElfVerdef<E>);

    verdef->vd_version = 1;
    verdef->vd_flags = flags;
    verdef->vd_ndx = idx;
    verdef->vd_cnt = 1;
    verdef->vd_hash = elf_hash(verstr);
    verdef->vd_aux = sizeof(ElfVerdef<E>);

    ElfVerdaux<E> *aux = (ElfVerdaux<E> *)ptr;
    ptr += sizeof(ElfVerdaux<E>);
    aux->vda_name = ctx.dynstr->add_string(verstr);
  };

  std::string_view basename = ctx.arg.soname.empty() ?
    ctx.arg.output : ctx.arg.soname;
  write(basename, 1, VER_FLG_BASE);

  i64 idx = 2;
  for (std::string_view verstr : ctx.arg.version_definitions)
    write(verstr, idx++, 0);

  for (Symbol<E> *sym : std::span<Symbol<E> *>(ctx.dynsym->symbols).subspan(1))
    ctx.versym->contents[sym->get_dynsym_idx(ctx)] = sym->ver_idx;
}

template <typename E>
void fill_verneed(Context<E> &ctx) {
  Timer t(ctx, "fill_verneed");

  if (ctx.dynsym->symbols.empty())
    return;

  // Create a list of versioned symbols and sort by file and version.
  std::vector<Symbol<E> *> syms(ctx.dynsym->symbols.begin() + 1,
                             ctx.dynsym->symbols.end());

  erase(syms, [](Symbol<E> *sym) {
    return !sym->file->is_dso || sym->ver_idx <= VER_NDX_LAST_RESERVED;
  });

  if (syms.empty())
    return;

  sort(syms, [](Symbol<E> *a, Symbol<E> *b) {
    return std::tuple(((SharedFile<E> *)a->file)->soname, a->ver_idx) <
           std::tuple(((SharedFile<E> *)b->file)->soname, b->ver_idx);
  });

  // Resize of .gnu.version
  ctx.versym->contents.resize(ctx.dynsym->symbols.size(), 1);
  ctx.versym->contents[0] = 0;

  // Allocate a large enough buffer for .gnu.version_r.
  ctx.verneed->contents.resize((sizeof(ElfVerneed<E>) + sizeof(ElfVernaux<E>)) *
                                syms.size());

  // Fill .gnu.version_r.
  u8 *buf = (u8 *)&ctx.verneed->contents[0];
  u8 *ptr = buf;
  ElfVerneed<E> *verneed = nullptr;
  ElfVernaux<E> *aux = nullptr;

  u16 veridx = VER_NDX_LAST_RESERVED + ctx.arg.version_definitions.size();

  auto start_group = [&](InputFile<E> *file) {
    ctx.verneed->shdr.sh_info++;
    if (verneed)
      verneed->vn_next = ptr - (u8 *)verneed;

    verneed = (ElfVerneed<E> *)ptr;
    ptr += sizeof(*verneed);
    verneed->vn_version = 1;
    verneed->vn_file = ctx.dynstr->find_string(((SharedFile<E> *)file)->soname);
    verneed->vn_aux = sizeof(ElfVerneed<E>);
    aux = nullptr;
  };

  auto add_entry = [&](Symbol<E> *sym) {
    verneed->vn_cnt++;

    if (aux)
      aux->vna_next = sizeof(ElfVernaux<E>);
    aux = (ElfVernaux<E> *)ptr;
    ptr += sizeof(*aux);

    std::string_view verstr = sym->get_version();
    aux->vna_hash = elf_hash(verstr);
    aux->vna_other = ++veridx;
    aux->vna_name = ctx.dynstr->add_string(verstr);
  };

  for (i64 i = 0; i < syms.size(); i++) {
    if (i == 0 || syms[i - 1]->file != syms[i]->file) {
      start_group(syms[i]->file);
      add_entry(syms[i]);
    } else if (syms[i - 1]->ver_idx != syms[i]->ver_idx) {
      add_entry(syms[i]);
    }

    ctx.versym->contents[syms[i]->get_dynsym_idx(ctx)] = veridx;
  }

  // Resize .gnu.version_r to fit to its contents.
  ctx.verneed->contents.resize(ptr - buf);
}

template <typename E>
void clear_padding(Context<E> &ctx, i64 filesize) {
  Timer t(ctx, "clear_padding");

  auto zero = [&](OutputChunk<E> *chunk, i64 next_start) {
    i64 pos = chunk->shdr.sh_offset;
    if (chunk->shdr.sh_type != SHT_NOBITS)
      pos += chunk->shdr.sh_size;
    memset(ctx.buf + pos, 0, next_start - pos);
  };

  for (i64 i = 1; i < ctx.chunks.size(); i++)
    zero(ctx.chunks[i - 1], ctx.chunks[i]->shdr.sh_offset);
  zero(ctx.chunks.back(), filesize);
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
template <typename E>
i64 get_section_rank(Context<E> &ctx, OutputChunk<E> *chunk) {
  if (chunk == ctx.ehdr.get())
    return 0;
  if (chunk == ctx.phdr.get())
    return 1;
  if (chunk == ctx.interp.get())
    return 2;
  if (chunk == ctx.shdr.get())
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
  bool relro = is_relro(ctx, chunk);
  bool hasbits = !(type == SHT_NOBITS);

  return ((!reaodnly << 9) | (exec << 8) | (!tls << 7) |
          (!relro << 6) | (!hasbits << 5)) + 4;
}

// Returns the smallest number n such that
// n >= val and n % align == skew.
inline u64 align_with_skew(u64 val, u64 align, u64 skew) {
  return align_to(val + align - skew, align) - align + skew;
}

template <typename E>
i64 set_osec_offsets(Context<E> &ctx) {
  Timer t(ctx, "osec_offset");

  i64 fileoff = 0;
  i64 vaddr = ctx.arg.image_base;

  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (chunk->new_page)
      vaddr = align_to(vaddr, PAGE_SIZE);

    vaddr = align_to(vaddr, chunk->shdr.sh_addralign);
    fileoff = align_with_skew(fileoff, PAGE_SIZE, vaddr % PAGE_SIZE);

    chunk->shdr.sh_offset = fileoff;
    if (chunk->shdr.sh_flags & SHF_ALLOC)
      chunk->shdr.sh_addr = vaddr;

    bool is_bss = (chunk->shdr.sh_type == SHT_NOBITS);
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

template <typename E>
static i64 get_num_irelative_relocs(Context<E> &ctx) {
  i64 n = 0;
  for (Symbol<E> *sym : ctx.got->got_syms)
    if (sym->get_type() == STT_GNU_IFUNC)
      n++;
  return n;
}

template <typename E>
void fix_synthetic_symbols(Context<E> &ctx) {
  auto start = [](Symbol<E> *sym, auto &chunk) {
    if (sym && chunk) {
      sym->shndx = chunk->shndx;
      sym->value = chunk->shdr.sh_addr;
    }
  };

  auto stop = [](Symbol<E> *sym, auto &chunk) {
    if (sym && chunk) {
      sym->shndx = chunk->shndx;
      sym->value = chunk->shdr.sh_addr + chunk->shdr.sh_size;
    }
  };

  // __bss_start
  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (chunk->kind == OutputChunk<E>::REGULAR && chunk->name == ".bss") {
      start(ctx.__bss_start, chunk);
      break;
    }
  }

  // __ehdr_start and __executable_start
  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (chunk->shndx == 1) {
      ctx.__ehdr_start->shndx = 1;
      ctx.__ehdr_start->value = ctx.ehdr->shdr.sh_addr;

      ctx.__executable_start->shndx = 1;
      ctx.__executable_start->value = ctx.ehdr->shdr.sh_addr;
      break;
    }
  }

  // __rel_iplt_start
  start(ctx.__rel_iplt_start, ctx.reldyn);

  // __rel_iplt_end
  ctx.__rel_iplt_end->shndx = ctx.reldyn->shndx;
  ctx.__rel_iplt_end->value = ctx.reldyn->shdr.sh_addr +
    get_num_irelative_relocs(ctx) * sizeof(ElfRel<E>);

  // __{init,fini}_array_{start,end}
  for (OutputChunk<E> *chunk : ctx.chunks) {
    switch (chunk->shdr.sh_type) {
    case SHT_INIT_ARRAY:
      start(ctx.__init_array_start, chunk);
      stop(ctx.__init_array_end, chunk);
      break;
    case SHT_FINI_ARRAY:
      start(ctx.__fini_array_start, chunk);
      stop(ctx.__fini_array_end, chunk);
      break;
    }
  }

  // _end, _etext, _edata and the like
  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (chunk->kind == OutputChunk<E>::HEADER)
      continue;

    if (chunk->shdr.sh_flags & SHF_ALLOC)
      stop(ctx._end, chunk);

    if (chunk->shdr.sh_flags & SHF_EXECINSTR)
      stop(ctx._etext, chunk);

    if (chunk->shdr.sh_type != SHT_NOBITS && chunk->shdr.sh_flags & SHF_ALLOC)
      stop(ctx._edata, chunk);
  }

  // _DYNAMIC
  start(ctx._DYNAMIC, ctx.dynamic);

  // _GLOBAL_OFFSET_TABLE_
  start(ctx._GLOBAL_OFFSET_TABLE_, ctx.gotplt);

  // __GNU_EH_FRAME_HDR
  start(ctx.__GNU_EH_FRAME_HDR, ctx.eh_frame_hdr);

  // __start_ and __stop_ symbols
  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (is_c_identifier(chunk->name)) {
      std::string_view sym1 =
        save_string(ctx, "__start_" + std::string(chunk->name));
      std::string_view sym2 =
        save_string(ctx, "__stop_" + std::string(chunk->name));

      start(Symbol<E>::intern(ctx, sym1), chunk);
      stop(Symbol<E>::intern(ctx, sym2), chunk);
    }
  }
}

#define INSTANTIATE(E)                                                  \
  template void apply_exclude_libs(Context<E> &ctx);                    \
  template void create_synthetic_sections(Context<E> &ctx);             \
  template void set_file_priority(Context<E> &ctx);                     \
  template void resolve_obj_symbols(Context<E> &ctx);                   \
  template void resolve_dso_symbols(Context<E> &ctx);                   \
  template void eliminate_comdats(Context<E> &ctx);                     \
  template void convert_common_symbols(Context<E> &ctx);                \
  template void add_comment_string(Context<E> &ctx, std::string str);   \
  template void compute_merged_section_sizes(Context<E> &ctx);          \
  template void bin_sections(Context<E> &ctx);                          \
  template void check_duplicate_symbols(Context<E> &ctx);               \
  template std::vector<OutputChunk<E> *>                                \
    collect_output_sections(Context<E> &ctx);                           \
  template void compute_section_sizes(Context<E> &ctx);                 \
  template void convert_undefined_weak_symbols(Context<E> &ctx);        \
  template void scan_rels(Context<E> &ctx);                             \
  template void apply_version_script(Context<E> &ctx);                  \
  template void parse_symbol_version(Context<E> &ctx);                  \
  template void compute_import_export(Context<E> &ctx);                 \
  template void fill_verdef(Context<E> &ctx);                           \
  template void fill_verneed(Context<E> &ctx);                          \
  template void clear_padding(Context<E> &ctx, i64 filesize);           \
  template i64 get_section_rank(Context<E> &ctx, OutputChunk<E> *chunk); \
  template i64 set_osec_offsets(Context<E> &ctx);                       \
  template void fix_synthetic_symbols(Context<E> &ctx);

INSTANTIATE(X86_64);
INSTANTIATE(I386);
