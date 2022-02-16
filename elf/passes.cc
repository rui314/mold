#include "mold.h"

#include <functional>
#include <map>
#include <optional>
#include <random>
#include <regex>
#include <tbb/parallel_for_each.h>
#include <tbb/partitioner.h>
#include <unordered_set>

namespace mold::elf {

template <typename E>
void apply_exclude_libs(Context<E> &ctx) {
  Timer t(ctx, "apply_exclude_libs");

  if (ctx.arg.exclude_libs.empty())
    return;

  std::unordered_set<std::string_view> set(ctx.arg.exclude_libs.begin(),
                                           ctx.arg.exclude_libs.end());

  for (ObjectFile<E> *file : ctx.objs) {
    if (!file->archive_name.empty())
      if (set.contains("ALL") ||
          set.contains(filepath(file->archive_name).filename().string()))
        file->exclude_libs = true;
  }
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
  add(ctx.reldyn = std::make_unique<RelDynSection<E>>());
  add(ctx.relplt = std::make_unique<RelPltSection<E>>());

  if (ctx.arg.pack_dyn_relocs_relr)
    add(ctx.relrdyn = std::make_unique<RelrDynSection<E>>());

  add(ctx.strtab = std::make_unique<StrtabSection<E>>());
  add(ctx.shstrtab = std::make_unique<ShstrtabSection<E>>());
  add(ctx.plt = std::make_unique<PltSection<E>>());
  add(ctx.pltgot = std::make_unique<PltGotSection<E>>());
  add(ctx.symtab = std::make_unique<SymtabSection<E>>());
  add(ctx.dynsym = std::make_unique<DynsymSection<E>>());
  add(ctx.dynstr = std::make_unique<DynstrSection<E>>());
  add(ctx.eh_frame = std::make_unique<EhFrameSection<E>>());
  add(ctx.copyrel = std::make_unique<CopyrelSection<E>>(false));
  add(ctx.copyrel_relro = std::make_unique<CopyrelSection<E>>(true));

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

  add(ctx.dynamic = std::make_unique<DynamicSection<E>>());
  add(ctx.versym = std::make_unique<VersymSection<E>>());
  add(ctx.verneed = std::make_unique<VerneedSection<E>>());
  add(ctx.note_property = std::make_unique<NotePropertySection<E>>());
}

template <typename E>
void resolve_symbols(Context<E> &ctx) {
  Timer t(ctx, "resolve_symbols");

  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dsos);

  // Register symbols
  tbb::parallel_for_each(files, [&](InputFile<E> *file) {
    file->resolve_symbols(ctx);
  });

  // Mark reachable objects to decide which files to include
  // into an output.
  std::vector<InputFile<E> *> live_set = files;
  std::erase_if(live_set, [](InputFile<E> *file) { return !file->is_alive; });

  auto mark_symbol = [&](std::string_view name) {
    if (InputFile<E> *file = get_symbol(ctx, name)->file)
      if (!file->is_alive.exchange(true))
        live_set.push_back(file);
  };

  for (std::string_view name : ctx.arg.undefined)
    mark_symbol(name);
  for (std::string_view name : ctx.arg.require_defined)
    mark_symbol(name);

  tbb::parallel_for_each(live_set, [&](InputFile<E> *file,
                                       tbb::feeder<InputFile<E> *> &feeder) {
    file->mark_live_objects(ctx, [&](InputFile<E> *obj) { feeder.add(obj); });
  });

  // Remove symbols of eliminated files.
  tbb::parallel_for_each(files, [&](InputFile<E> *file) {
    if (!file->is_alive)
      file->clear_symbols(ctx);
  });

  // Since we have turned on object files live bits, their symbols
  // may now have higher priority than before. So run the symbol
  // resolution pass again to get the final resolution result.
  tbb::parallel_for_each(files, [&](InputFile<E> *file) {
    if (file->is_alive)
      file->resolve_symbols(ctx);
  });

  // Remove unused files
  std::erase_if(ctx.objs, [](InputFile<E> *file) { return !file->is_alive; });
  std::erase_if(ctx.dsos, [](InputFile<E> *file) { return !file->is_alive; });
}

template <typename E>
void register_section_pieces(Context<E> &ctx) {
  Timer t(ctx, "register_section_pieces");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->register_section_pieces(ctx);
  });
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
  ss << ctx.cmdline_args[1];
  for (i64 i = 2; i < ctx.cmdline_args.size(); i++)
    ss << " " << ctx.cmdline_args[i];
  return ss.str();
}

template <typename E>
void add_comment_string(Context<E> &ctx, std::string str) {
  std::string_view buf = save_string(ctx, str);
  MergedSection<E> *sec =
    MergedSection<E>::get_instance(ctx, ".comment", SHT_PROGBITS, 0);
  std::string_view data(buf.data(), buf.size() + 1);
  SectionFragment<E> *frag = sec->insert(data, hash_string(data), 0);
  frag->is_alive = true;
}

template <typename E>
void compute_merged_section_sizes(Context<E> &ctx) {
  Timer t(ctx, "compute_merged_section_sizes");

  // Mark section fragments referenced by live objects.
  if (!ctx.arg.gc_sections) {
    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      for (std::unique_ptr<MergeableSection<E>> &m : file->mergeable_sections)
        if (m)
          for (SectionFragment<E> *frag : m->fragments)
            frag->is_alive.store(true, std::memory_order_relaxed);
    });
  }

  // Add an identification string to .comment.
  add_comment_string(ctx, mold_version);

  // Embed command line arguments for debugging.
  if (char *env = getenv("MOLD_DEBUG"); env && env[0])
    add_comment_string(ctx, "mold command line: " + get_cmdline_args(ctx));

  Timer t2(ctx, "MergedSection assign_offsets");
  tbb::parallel_for_each(ctx.merged_sections,
                         [&](std::unique_ptr<MergedSection<E>> &sec) {
    sec->assign_offsets(ctx);
  });
}

template <typename T>
static std::vector<std::span<T>> split(std::vector<T> &input, i64 unit) {
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

  if (ctx.objs.empty())
    return;

  static constexpr i64 num_shards = 128;
  i64 unit = (ctx.objs.size() + num_shards - 1) / num_shards;
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

static std::optional<u64> parse_defsym_addr(std::string_view s) {
  if (s.starts_with("0x") || s.starts_with("0X")) {
    size_t nread;
    u64 addr = std::stoull(std::string(s), &nread, 16);
    if (s.size() != nread)
      return {};
    return addr;
  }

  if (s.find_first_not_of("0123456789") == s.npos)
    return std::stoull(std::string(s), nullptr, 10);
  return {};
}

// Create a dummy object file containing linker-synthesized
// symbols.
template <typename E>
ObjectFile<E> *create_internal_file(Context<E> &ctx) {
  ObjectFile<E> *obj = new ObjectFile<E>;
  ctx.obj_pool.push_back(std::unique_ptr<ObjectFile<E>>(obj));

  // Create linker-synthesized symbols.
  auto *esyms = new std::vector<ElfSym<E>>(1);
  obj->symbols.push_back(new Symbol<E>);
  obj->first_global = 1;
  obj->is_alive = true;
  obj->features = -1;
  obj->priority = 1;

  auto add = [&](std::string_view name) {
    ElfSym<E> esym = {};
    esym.st_type = STT_NOTYPE;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = STV_HIDDEN;
    esyms->push_back(esym);

    Symbol<E> *sym = get_symbol(ctx, name);
    sym->shndx = 1; // dummy value to make it a relative symbol
    obj->symbols.push_back(sym);
    return sym;
  };

  ctx.__ehdr_start = add("__ehdr_start");
  ctx.__init_array_start = add("__init_array_start");
  ctx.__init_array_end = add("__init_array_end");
  ctx.__fini_array_start = add("__fini_array_start");
  ctx.__fini_array_end = add("__fini_array_end");
  ctx.__preinit_array_start = add("__preinit_array_start");
  ctx.__preinit_array_end = add("__preinit_array_end");
  ctx._DYNAMIC = add("_DYNAMIC");
  ctx._GLOBAL_OFFSET_TABLE_ = add("_GLOBAL_OFFSET_TABLE_");
  ctx.__bss_start = add("__bss_start");
  ctx._end = add("_end");
  ctx._etext = add("_etext");
  ctx._edata = add("_edata");
  ctx.__executable_start = add("__executable_start");

  ctx.__rel_iplt_start =
    add(E::is_rel ? "__rel_iplt_start" : "__rela_iplt_start");
  ctx.__rel_iplt_end =
    add(E::is_rel ? "__rel_iplt_end" : "__rela_iplt_end");

  if (ctx.arg.eh_frame_hdr)
    ctx.__GNU_EH_FRAME_HDR = add("__GNU_EH_FRAME_HDR");

  if (!get_symbol(ctx, "end")->file)
    ctx.end = add("end");
  if (!get_symbol(ctx, "etext")->file)
    ctx.etext = add("etext");
  if (!get_symbol(ctx, "edata")->file)
    ctx.edata = add("edata");

  if (E::e_machine == EM_RISCV && !ctx.arg.shared)
    ctx.__global_pointer = add("__global_pointer$");

  for (Chunk<E> *chunk : ctx.chunks) {
    if (!is_c_identifier(chunk->name))
      continue;

    add(save_string(ctx, "__start_" + std::string(chunk->name)));
    add(save_string(ctx, "__stop_" + std::string(chunk->name)));
  }

  for (std::pair<std::string_view, std::string_view> pair : ctx.arg.defsyms) {
    ElfSym<E> esym = {};
    esym.st_type = STT_NOTYPE;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = STV_DEFAULT;
    esyms->push_back(esym);

    Symbol<E> *sym = get_symbol(ctx, pair.first);
    if (!parse_defsym_addr(pair.second))
      sym->shndx = 1; // dummy value to make it a relative symbol
    obj->symbols.push_back(sym);
  };

  obj->elf_syms = *esyms;
  obj->sym_fragments.resize(obj->elf_syms.size());

  i64 num_globals = obj->elf_syms.size() - obj->first_global;
  obj->symvers.resize(num_globals);

  ctx.on_exit.push_back([=] {
    delete esyms;
    delete obj->symbols[0];
  });

  return obj;
}

template <typename E>
void check_cet_errors(Context<E> &ctx) {
  bool warning = (ctx.arg.z_cet_report == CET_REPORT_WARNING);
  bool error = (ctx.arg.z_cet_report == CET_REPORT_ERROR);
  assert(warning || error);

  for (ObjectFile<E> *file : ctx.objs) {
    if (!(file->features & GNU_PROPERTY_X86_FEATURE_1_IBT)) {
      if (warning)
        Warn(ctx) << *file << ": -cet-report=warning: "
                  << "missing GNU_PROPERTY_X86_FEATURE_1_IBT";
      else
        Error(ctx) << *file << ": -cet-report=error: "
                   << "missing GNU_PROPERTY_X86_FEATURE_1_IBT";
    }

    if (!(file->features & GNU_PROPERTY_X86_FEATURE_1_SHSTK)) {
      if (warning)
        Warn(ctx) << *file << ": -cet-report=warning: "
                  << "missing GNU_PROPERTY_X86_FEATURE_1_SHSTK";
      else
        Error(ctx) << *file << ": -cet-report=error: "
                   << "missing GNU_PROPERTY_X86_FEATURE_1_SHSTK";
    }
  }
}

template <typename E>
void print_dependencies(Context<E> &ctx) {
  SyncOut(ctx) <<
R"(# This is an output of the mold linker's --print-dependencies option.
#
# Each line consists of three fields, <input-file>, <output-file> and
# <symbol> separated by tab characters. It indicates that <input-file>
# depends on <output-file> to use <symbol>.)";

  auto print = [&](InputFile<E> *file) {
    for (i64 i = file->first_global; i < file->symbols.size(); i++) {
      ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];
      if (esym.is_undef() && sym.file && sym.file != file)
        SyncOut(ctx) << *file << "\t" << *sym.file << "\t" << sym;
    }
  };

  for (InputFile<E> *file : ctx.objs)
    print(file);
  for (InputFile<E> *file : ctx.dsos)
    print(file);
}

template <typename E>
void print_dependencies_full(Context<E> &ctx) {
  SyncOut(ctx) <<
R"(# This is an output of the mold linker's --print-dependencies=full option.
#
# Each line consists of 4 fields, <input-section>, <output-section>,
# <symbol-type> and <symbol>, separated by tab characters. It indicates that
# <input-section> depends on <output-section> to use <symbol>. <symbol-type>
# is either "u" or "w" for regular or weak undefined, respectively.
#
# If you want to obtain dependency information per function granularity,
# compile source files with the -ffunction-sections compiler flag.)";

  auto println = [&](auto &src, Symbol<E> &sym, ElfSym<E> &esym) {
    if (sym.input_section)
      SyncOut(ctx) << src << "\t" << *sym.input_section
                   << "\t" << (esym.is_weak() ? 'w' : 'u')
                   << "\t" << sym;
    else
      SyncOut(ctx) << src << "\t" << *sym.file
                   << "\t" << (esym.is_weak() ? 'w' : 'u')
                   << "\t" << sym;
  };

  for (ObjectFile<E> *file : ctx.objs) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (!isec)
        continue;

      std::unordered_set<void *> visited;

      for (ElfRel<E> &r : isec->get_rels(ctx)) {
        if (r.r_type == E::R_NONE)
          continue;

        ElfSym<E> &esym = file->elf_syms[r.r_sym];
        Symbol<E> &sym = *file->symbols[r.r_sym];

        if (esym.is_undef() && sym.file && sym.file != file &&
            visited.insert((void *)&sym).second)
          println(*isec, sym, esym);
      }
    }
  }

  for (SharedFile<E> *file : ctx.dsos) {
    for (i64 i = file->first_global; i < file->symbols.size(); i++) {
      ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];
      if (esym.is_undef() && sym.file && sym.file != file)
        println(*file, sym, esym);
    }
  }
}

template <typename E>
static std::string create_response_file(Context<E> &ctx) {
  std::string buf;
  std::stringstream out;

  std::string cwd = std::filesystem::current_path();
  out << "-C " << cwd.substr(1) << "\n";

  if (cwd != "/") {
    out << "--chroot ..";
    i64 depth = std::count(cwd.begin(), cwd.end(), '/');
    for (i64 i = 1; i < depth; i++)
      out << "/..";
    out << "\n";
  }

  for (i64 i = 1; i < ctx.cmdline_args.size(); i++) {
    std::string_view arg = ctx.cmdline_args[i];
    if (arg != "-repro" && arg != "--repro")
      out << arg << "\n";
  }
  return out.str();
}

template <typename E>
void write_repro_file(Context<E> &ctx) {
  std::string path = ctx.arg.output + ".repro.tar";

  std::unique_ptr<TarWriter> tar =
    TarWriter::open(path, filepath(ctx.arg.output).filename().string() + ".repro");
  if (!tar)
    Fatal(ctx) << "cannot open " << path << ": " << errno_string();

  tar->append("response.txt", save_string(ctx, create_response_file(ctx)));
  tar->append("version.txt", save_string(ctx, mold_version + "\n"));

  std::unordered_set<std::string> seen;
  for (std::unique_ptr<MappedFile<Context<E>>> &mf : ctx.mf_pool) {
    if (!mf->parent) {
      std::string path = to_abs_path(mf->name);
      if (seen.insert(path).second)
        tar->append(path, mf->get_contents());
    }
  }
}

template <typename E>
void check_duplicate_symbols(Context<E> &ctx) {
  Timer t(ctx, "check_duplicate_symbols");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];

      if (sym.file == file || sym.file == ctx.internal_obj ||
          esym.is_undef() || esym.is_common() || (esym.st_bind == STB_WEAK))
        continue;

      if (!esym.is_abs()) {
        InputSection<E> *isec = file->get_section(esym);
        if (!isec || !isec->is_alive)
          continue;
      }

      Error(ctx) << "duplicate symbol: " << *file << ": " << *sym.file
                 << ": " << sym;
    }
  });

  ctx.checkpoint();
}

template <typename E>
void sort_init_fini(Context<E> &ctx) {
  Timer t(ctx, "sort_init_fini");

  auto get_priority = [](InputSection<E> *isec) {
    static std::regex re(R"(_array\.(\d+)$)", std::regex_constants::optimize);
    std::string name = isec->name().begin();
    std::smatch m;
    if (std::regex_search(name, m, re))
      return std::stoi(m[1]);
    return 65536;
  };

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections) {
    if (osec->name == ".init_array" || osec->name == ".fini_array") {
      sort(osec->members, [&](InputSection<E> *a, InputSection<E> *b) {
        return get_priority(a) < get_priority(b);
      });
    }
  }
}

template <typename T>
static void shuffle(std::vector<T> &vec, u64 seed) {
  if (vec.empty())
    return;

  // Xorshift random number generator. We use this RNG because it is
  // measurably faster than MT19937.
  auto rand = [&]() {
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    return seed;
  };

  // The Fisher-Yates shuffling algorithm.
  //
  // We don't want to use std::shuffle for build reproducibility. That is,
  // std::shuffle's implementation is not guaranteed to be the same across
  // platform, so even though the result is guaranteed to be randomly
  // shuffled, the exact order may be different across implementations.
  //
  // We are not using std::uniform_int_distribution for the same reason.
  for (i64 i = 0; i < vec.size() - 1; i++)
    std::swap(vec[i], vec[i + rand() % (vec.size() - i)]);
}

template <typename E>
void shuffle_sections(Context<E> &ctx) {
  Timer t(ctx, "shuffle_sections");

  u64 seed = std::random_device()();

  tbb::parallel_for_each(ctx.output_sections,
                         [&](std::unique_ptr<OutputSection<E>> &osec) {
    if (osec->name != ".init" && osec->name != ".fini" &&
        osec->name != ".init_array" && osec->name != ".fini_array")
      shuffle(osec->members, seed + hash_string(osec->name));
  });
}

template <typename E>
std::vector<Chunk<E> *> collect_output_sections(Context<E> &ctx) {
  std::vector<Chunk<E> *> vec;

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
    if (!osec->members.empty())
      vec.push_back(osec.get());
  for (std::unique_ptr<MergedSection<E>> &osec : ctx.merged_sections)
    if (osec->shdr.sh_size)
      vec.push_back(osec.get());

  // Sections are added to the section lists in an arbitrary order because
  // they are created in parallel.
  // Sort them to to make the output deterministic.
  sort(vec, [](Chunk<E> *x, Chunk<E> *y) {
    return std::tuple(x->name, x->shdr.sh_type, x->shdr.sh_flags) <
           std::tuple(y->name, y->shdr.sh_type, y->shdr.sh_flags);
  });
  return vec;
}

template <typename E>
void compute_section_sizes(Context<E> &ctx) {
  Timer t(ctx, "compute_section_sizes");

  struct Group {
    i64 size = 0;
    i64 p2align = 0;
    i64 offset = 0;
    std::span<InputSection<E> *> members;
  };

  tbb::parallel_for_each(ctx.output_sections,
                         [&](std::unique_ptr<OutputSection<E>> &osec) {
    // Since one output section may contain millions of input sections,
    // we first split input sections into groups and assign offsets to
    // groups.
    std::vector<Group> groups;
    constexpr i64 group_size = 10000;

    for (std::span<InputSection<E> *> span : split(osec->members, group_size))
      groups.push_back(Group{.members = span});

    tbb::parallel_for_each(groups, [](Group &group) {
      for (InputSection<E> *isec : group.members) {
        group.size = align_to(group.size, 1 << isec->p2align) + isec->sh_size;
        group.p2align = std::max<i64>(group.p2align, isec->p2align);
      }
    });

    i64 offset = 0;
    i64 p2align = 0;

    for (i64 i = 0; i < groups.size(); i++) {
      offset = align_to(offset, 1 << groups[i].p2align);
      groups[i].offset = offset;
      offset += groups[i].size;
      p2align = std::max(p2align, groups[i].p2align);
    }

    osec->shdr.sh_size = offset;
    osec->shdr.sh_addralign = 1 << p2align;

    // Assign offsets to input sections.
    tbb::parallel_for_each(groups, [](Group &group) {
      i64 offset = group.offset;
      for (InputSection<E> *isec : group.members) {
        offset = align_to(offset, 1 << isec->p2align);
        isec->offset = offset;
        offset += isec->sh_size;
      }
    });
  });
}

template <typename E>
void claim_unresolved_symbols(Context<E> &ctx) {
  Timer t(ctx, "claim_unresolved_symbols");
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->claim_unresolved_symbols(ctx);
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
  ctx.checkpoint();

  // Aggregate dynamic symbols to a single vector.
  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dsos);

  std::vector<std::vector<Symbol<E> *>> vec(files.size());

  tbb::parallel_for((i64)0, (i64)files.size(), [&](i64 i) {
    for (Symbol<E> *sym : files[i]->symbols)
      if (sym->file == files[i])
        if (sym->flags || sym->is_imported || sym->is_exported)
          vec[i].push_back(sym);
  });

  std::vector<Symbol<E> *> syms = flatten(vec);
  ctx.symbol_aux.reserve(syms.size());

  auto add_aux = [&](Symbol<E> *sym) {
    if (sym->aux_idx == -1) {
      i64 sz = ctx.symbol_aux.size();
      sym->aux_idx = sz;
      ctx.symbol_aux.resize(sz + 1);
    }
  };

  // Assign offsets in additional tables for each dynamic symbol.
  for (Symbol<E> *sym : syms) {
    add_aux(sym);

    if (sym->is_imported || sym->is_exported)
      ctx.dynsym->add_symbol(ctx, sym);

    if (sym->flags & NEEDS_GOT)
      ctx.got->add_got_symbol(ctx, sym);

    if (sym->flags & NEEDS_PLT) {
      bool is_canonical = (!ctx.arg.pic && sym->is_imported);

      // If a symbol needs a canonical PLT, it is considered both
      // imported and exported.
      if (is_canonical)
        sym->is_exported = true;

      if ((sym->flags & NEEDS_GOT) && !is_canonical) {
        ctx.pltgot->add_symbol(ctx, sym);
      } else{
        // If we need to create a canonical PLT, we can't use .plt.got
        // because otherwise .plt.got and .got would refer each other,
        // resulting in an infinite loop at runtime.
        ctx.plt->add_symbol(ctx, sym);
      }
    }

    if (sym->flags & NEEDS_GOTTP)
      ctx.got->add_gottp_symbol(ctx, sym);

    if (sym->flags & NEEDS_TLSGD)
      ctx.got->add_tlsgd_symbol(ctx, sym);

    if (sym->flags & NEEDS_TLSDESC)
      ctx.got->add_tlsdesc_symbol(ctx, sym);

    if (sym->flags & NEEDS_COPYREL) {
      assert(sym->file->is_dso);
      SharedFile<E> *file = (SharedFile<E> *)sym->file;
      sym->copyrel_readonly = file->is_readonly(ctx, sym);

      if (sym->copyrel_readonly)
        ctx.copyrel_relro->add_symbol(ctx, sym);
      else
        ctx.copyrel->add_symbol(ctx, sym);

      // If a symbol needs copyrel, it is considered both imported
      // and exported.
      assert(sym->is_imported);
      sym->is_exported = true;

      // Aliases of this symbol are also copied so that they will be
      // resolved to the same address at runtime.
      for (Symbol<E> *alias : file->find_aliases(sym)) {
        add_aux(alias);
        alias->is_imported = true;
        alias->is_exported = true;
        alias->has_copyrel = true;
        alias->value = sym->value;
        alias->copyrel_readonly = sym->copyrel_readonly;
        ctx.dynsym->add_symbol(ctx, alias);
      }
    }

    sym->flags = 0;
  }

  if (ctx.needs_tlsld)
    ctx.got->add_tlsld(ctx);

  if (ctx.has_textrel && ctx.arg.warn_textrel)
    Warn(ctx) << "creating a DT_TEXTREL in an output file";
}

template <typename E>
void create_reloc_sections(Context<E> &ctx) {
  Timer t(ctx, "create_reloc_sections");

  // Create .rela.* sections
  for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections) {
    RelocSection<E> *r = new RelocSection<E>(ctx, *osec);
    ctx.chunks.push_back(r);
    ctx.output_chunks.push_back(std::unique_ptr<Chunk<E>>(r));
  }

  // Create a table to map input symbol indices to output symbol indices
  auto set_indices = [&](InputFile<E> *file) {
    file->output_sym_indices.resize(file->symbols.size(), -1);

    for (i64 i = 1, j = 0; i < file->first_global; i++)
      if (Symbol<E> &sym = *file->symbols[i];
          sym.file == file && sym.write_to_symtab)
        file->output_sym_indices[i] = j++;

    for (i64 i = file->first_global, j = 0; i < file->symbols.size(); i++)
      if (Symbol<E> &sym = *file->symbols[i];
          sym.file == file && sym.write_to_symtab)
        file->output_sym_indices[i] = j++;
  };

  tbb::parallel_for_each(ctx.objs, set_indices);
  tbb::parallel_for_each(ctx.dsos, set_indices);
}

template <typename E>
void construct_relr(Context<E> &ctx) {
  Timer t(ctx, "construct_relr");

  tbb::parallel_for_each(ctx.output_sections,
                         [&](std::unique_ptr<OutputSection<E>> &osec) {
    osec->construct_relr(ctx);
  });

  ctx.got->construct_relr(ctx);
}

template <typename E>
void create_output_symtab(Context<E> &ctx) {
  Timer t(ctx, "compute_symtab");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->compute_symtab(ctx);
  });

  tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
    file->compute_symtab(ctx);
  });
}

template <typename E>
void apply_version_script(Context<E> &ctx) {
  Timer t(ctx, "apply_version_script");

  // If all patterns are simple (i.e. not containing any meta-
  // characters and is not a C++ name), we can simply look up
  // symbols.
  auto is_simple = [&] {
    for (VersionPattern &v : ctx.version_patterns)
      if (v.is_cpp || v.pattern.find_first_of("*?[") != v.pattern.npos)
        return false;
    return true;
  };

  if (is_simple()) {
    for (VersionPattern &v : ctx.version_patterns)
      if (Symbol<E> *sym = get_symbol(ctx, v.pattern);
          sym->file && !sym->file->is_dso)
        sym->ver_idx = v.ver_idx;
    return;
  }

  // Otherwise, use glob pattern matchers.
  VersionMatcher matcher;
  VersionMatcher cpp_matcher;

  for (VersionPattern &v : ctx.version_patterns) {
    if (v.is_cpp) {
      if (!cpp_matcher.add(v.pattern, v.ver_idx))
        Fatal(ctx) << "invalid version pattern: " << v.pattern;
    } else {
      if (!matcher.add(v.pattern, v.ver_idx))
        Fatal(ctx) << "invalid version pattern: " << v.pattern;
    }
  }

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->get_global_syms()) {
      if (sym->file != file)
        continue;

      std::string_view name = sym->name();

      if (std::optional<u16> ver = matcher.find(name)) {
        sym->ver_idx = *ver;
        continue;
      }

      if (!cpp_matcher.empty())
        if (std::optional<u16> ver = cpp_matcher.find(demangle(name)))
          sym->ver_idx = *ver;
    }
  });
}

template <typename E>
void parse_symbol_version(Context<E> &ctx) {
  if (!ctx.arg.shared)
    return;

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
      for (Symbol<E> *sym : file->symbols) {
        if (sym->file && !sym->file->is_dso && sym->visibility != STV_HIDDEN) {
          std::scoped_lock lock(sym->mu);
          sym->is_exported = true;
        }
      }
    });
  }

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->get_global_syms()) {
      if (!sym->file || sym->visibility == STV_HIDDEN ||
          sym->ver_idx == VER_NDX_LOCAL)
        continue;

      if (sym->file != file && sym->file->is_dso) {
        sym->is_imported = true;
        continue;
      }

      if (sym->file == file) {
        sym->is_exported = true;

        if (ctx.arg.shared && sym->visibility != STV_PROTECTED &&
            !ctx.arg.Bsymbolic &&
            !(ctx.arg.Bsymbolic_functions && sym->get_type() == STT_FUNC))
          sym->is_imported = true;
      }
    }
  });
}

template <typename E>
void clear_padding(Context<E> &ctx) {
  Timer t(ctx, "clear_padding");

  auto zero = [&](Chunk<E> *chunk, i64 next_start) {
    i64 pos = chunk->shdr.sh_offset + chunk->shdr.sh_size;
    memset(ctx.buf + pos, 0, next_start - pos);
  };

  std::vector<Chunk<E> *> chunks = ctx.chunks;

  std::erase_if(chunks, [](Chunk<E> *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS;
  });

  for (i64 i = 1; i < chunks.size(); i++)
    zero(chunks[i - 1], chunks[i]->shdr.sh_offset);
  zero(chunks.back(), ctx.output_file->filesize);
}

// We want to sort output chunks in the following order.
//
//   ELF header
//   program header
//   .interp
//   alloc note
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
//
// .note sections are sorted by their alignments.
template <typename E>
i64 get_section_rank(Context<E> &ctx, Chunk<E> *chunk) {
  u64 type = chunk->shdr.sh_type;
  u64 flags = chunk->shdr.sh_flags;

  if (chunk == ctx.ehdr.get())
    return 0;
  if (chunk == ctx.phdr.get())
    return 1;
  if (chunk == ctx.interp.get())
    return 2;
  if (type == SHT_NOTE && (flags & SHF_ALLOC))
    return (1 << 10) + chunk->shdr.sh_addralign;
  if (chunk == ctx.shdr.get())
    return 1 << 30;
  if (!(flags & SHF_ALLOC))
    return (1 << 30) - 1;

  bool writable = (flags & SHF_WRITE);
  bool exec = (flags & SHF_EXECINSTR);
  bool tls = (flags & SHF_TLS);
  bool relro = is_relro(ctx, chunk);
  bool is_bss = (type == SHT_NOBITS);

  return (1 << 20) | (writable << 19) | (exec << 18) | (!tls << 17) |
         (!relro << 16) | (is_bss << 15);
}

// Returns the smallest number n such that
// val <= n and n % align == skew % align.
inline u64 align_with_skew(u64 val, u64 align, u64 skew) {
  skew = skew % align;
  u64 n = align_to(val + align - skew, align) - align + skew;
  assert(val <= n && n < val + align && n % align == skew % align);
  return n;
}

template <typename E>
static bool is_tbss(Chunk<E> *chunk) {
  return (chunk->shdr.sh_type == SHT_NOBITS) && (chunk->shdr.sh_flags & SHF_TLS);
}

// Assign virtual addresses and file offsets to output sections.
template <typename E>
i64 set_osec_offsets(Context<E> &ctx) {
  Timer t(ctx, "set_osec_offsets");

  std::vector<Chunk<E> *> &chunks = ctx.chunks;

  // Assign virtual addresses
  u64 addr = ctx.arg.image_base;
  for (i64 i = 0; i < chunks.size(); i++) {
    if (!(chunks[i]->shdr.sh_flags & SHF_ALLOC))
      continue;

    if (i > 0 && separate_page(ctx, chunks[i - 1], chunks[i]))
      addr = align_to(addr, ctx.page_size);

    if (is_tbss(chunks[i])) {
      chunks[i]->shdr.sh_addr = addr;
      continue;
    }

    addr = align_to(addr, chunks[i]->shdr.sh_addralign);
    chunks[i]->shdr.sh_addr = addr;
    addr += chunks[i]->shdr.sh_size;
  }

  // Fix tbss virtual addresses. tbss sections are laid out as if they
  // were overlapping to suceeding non-tbss sections. This is fine
  // because no one will actually access the TBSS part of a TLS
  // template image at runtime.
  //
  // We can lay out tbss sections in the same way as regular bss
  // sections, but that would need one more extra PT_LOAD segment.
  // Having fewer PT_LOAD segments is generally desirable, so we do this.
  for (i64 i = 0; i < chunks.size();) {
    if (is_tbss(chunks[i])) {
      u64 addr = chunks[i]->shdr.sh_addr;
      for (; i < chunks.size() && is_tbss(chunks[i]); i++) {
        addr = align_to(addr, chunks[i]->shdr.sh_addralign);
        chunks[i]->shdr.sh_addr = addr;
        addr += chunks[i]->shdr.sh_size;
      }
    } else {
      i++;
    }
  }

  // Assign file offsets
  u64 fileoff = 0;
  for (Chunk<E> *chunk : chunks) {
    if (chunk->shdr.sh_type == SHT_NOBITS) {
      chunk->shdr.sh_offset = fileoff;
    } else {
      fileoff = align_with_skew(fileoff, ctx.page_size, chunk->shdr.sh_addr);
      chunk->shdr.sh_offset = fileoff;
      fileoff += chunk->shdr.sh_size;
    }
  }
  return fileoff;
}

template <typename E>
static i64 get_num_irelative_relocs(Context<E> &ctx) {
  return std::count_if(
    ctx.got->got_syms.begin(), ctx.got->got_syms.end(),
    [](Symbol<E> *sym) { return sym->get_type() == STT_GNU_IFUNC; });
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
  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk->is_output_section() && chunk->name == ".bss") {
      start(ctx.__bss_start, chunk);
      break;
    }
  }

  // __ehdr_start and __executable_start
  for (Chunk<E> *chunk : ctx.chunks) {
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
  for (Chunk<E> *chunk : ctx.chunks) {
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
  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk->is_header())
      continue;

    if (chunk->shdr.sh_flags & SHF_ALLOC) {
      stop(ctx._end, chunk);
      stop(ctx.end, chunk);
    }

    if (chunk->shdr.sh_flags & SHF_EXECINSTR) {
      stop(ctx._etext, chunk);
      stop(ctx.etext, chunk);
    }

    if (chunk->shdr.sh_type != SHT_NOBITS &&
        (chunk->shdr.sh_flags & SHF_ALLOC)) {
      stop(ctx._edata, chunk);
      stop(ctx.edata, chunk);
    }
  }

  // _DYNAMIC
  start(ctx._DYNAMIC, ctx.dynamic);

  // _GLOBAL_OFFSET_TABLE_
  if (E::e_machine == EM_X86_64 || E::e_machine == EM_386)
    start(ctx._GLOBAL_OFFSET_TABLE_, ctx.gotplt);
  else if (E::e_machine == EM_AARCH64 || E::e_machine == EM_RISCV)
    start(ctx._GLOBAL_OFFSET_TABLE_, ctx.got);
  else
    unreachable();

  // __GNU_EH_FRAME_HDR
  start(ctx.__GNU_EH_FRAME_HDR, ctx.eh_frame_hdr);

  // RISC-V's __global_pointer$
  if (E::e_machine == EM_RISCV && !ctx.arg.shared) {
    ctx.__global_pointer->shndx = 1;
    ctx.__global_pointer->value = 0x800;

    for (Chunk<E> *chunk : ctx.chunks) {
      if (chunk->name == ".sdata") {
        ctx.__global_pointer->shndx = chunk->shndx;
        break;
      }
    }
  }

  // __start_ and __stop_ symbols
  for (Chunk<E> *chunk : ctx.chunks) {
    if (is_c_identifier(chunk->name)) {
      std::string_view sym1 =
        save_string(ctx, "__start_" + std::string(chunk->name));
      std::string_view sym2 =
        save_string(ctx, "__stop_" + std::string(chunk->name));

      start(get_symbol(ctx, sym1), chunk);
      stop(get_symbol(ctx, sym2), chunk);
    }
  }

  // --defsym=sym=value symbols
  for (std::pair<std::string_view, std::string_view> defsym : ctx.arg.defsyms) {
    Symbol<E> *sym = get_symbol(ctx, defsym.first);
    sym->input_section = nullptr;

    if (std::optional<u64> addr = parse_defsym_addr(defsym.second)) {
      sym->value = *addr;
      continue;
    }

    Symbol<E> *sym2 = get_symbol(ctx, defsym.second);
    if (!sym2->file) {
      Error(ctx) << "--defsym: undefined symbol: " << *sym2;
      continue;
    }

    sym->value = sym2->get_addr(ctx);
    sym->visibility = sym2->visibility.load();

    if (InputSection<E> *isec = sym2->input_section)
      sym->shndx = isec->output_section->shndx;
  }
}

template <typename E>
void compress_debug_sections(Context<E> &ctx) {
  Timer t(ctx, "compress_debug_sections");

  tbb::parallel_for((i64)0, (i64)ctx.chunks.size(), [&](i64 i) {
    Chunk<E> &chunk = *ctx.chunks[i];

    if ((chunk.shdr.sh_flags & SHF_ALLOC) || chunk.shdr.sh_size == 0 ||
        !chunk.name.starts_with(".debug"))
      return;

    Chunk<E> *comp = nullptr;
    if (ctx.arg.compress_debug_sections == COMPRESS_GABI)
      comp = new GabiCompressedSection<E>(ctx, chunk);
    else if (ctx.arg.compress_debug_sections == COMPRESS_GNU)
      comp = new GnuCompressedSection<E>(ctx, chunk);
    assert(comp);

    ctx.output_chunks.push_back(std::unique_ptr<Chunk<E>>(comp));
    ctx.chunks[i] = comp;
  });

  ctx.shstrtab->update_shdr(ctx);
  ctx.ehdr->update_shdr(ctx);
  ctx.shdr->update_shdr(ctx);
}

#define INSTANTIATE(E)                                                  \
  template void apply_exclude_libs(Context<E> &);                       \
  template void create_synthetic_sections(Context<E> &);                \
  template void resolve_symbols(Context<E> &);                          \
  template void register_section_pieces(Context<E> &);                  \
  template void eliminate_comdats(Context<E> &);                        \
  template void convert_common_symbols(Context<E> &);                   \
  template void compute_merged_section_sizes(Context<E> &);             \
  template void bin_sections(Context<E> &);                             \
  template ObjectFile<E> *create_internal_file(Context<E> &);           \
  template void check_cet_errors(Context<E> &);                         \
  template void print_dependencies(Context<E> &);                       \
  template void print_dependencies_full(Context<E> &);                  \
  template void write_repro_file(Context<E> &);                         \
  template void check_duplicate_symbols(Context<E> &);                  \
  template void sort_init_fini(Context<E> &);                           \
  template void shuffle_sections(Context<E> &);                         \
  template std::vector<Chunk<E> *> collect_output_sections(Context<E> &); \
  template void compute_section_sizes(Context<E> &);                    \
  template void claim_unresolved_symbols(Context<E> &);                 \
  template void scan_rels(Context<E> &);                                \
  template void create_reloc_sections(Context<E> &);                    \
  template void construct_relr(Context<E> &);                           \
  template void create_output_symtab(Context<E> &);                     \
  template void apply_version_script(Context<E> &);                     \
  template void parse_symbol_version(Context<E> &);                     \
  template void compute_import_export(Context<E> &);                    \
  template void clear_padding(Context<E> &);                            \
  template i64 get_section_rank(Context<E> &, Chunk<E> *);              \
  template i64 set_osec_offsets(Context<E> &);                          \
  template void fix_synthetic_symbols(Context<E> &);                    \
  template void compress_debug_sections(Context<E> &);

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(ARM64);
INSTANTIATE(RISCV64);

} // namespace mold::elf
