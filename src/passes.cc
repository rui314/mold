#include "mold.h"
#include "blake3.h"

#include <fstream>
#include <functional>
#include <optional>
#include <regex>
#include <shared_mutex>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>
#include <unordered_set>

namespace mold {

// Since mold_main is a template, we can't run it without a type parameter.
// We speculatively run mold_main with X86_64, and if the speculation was
// wrong, re-run it with an actual machine type.
template <typename E>
int redo_main(Context<E> &ctx, int argc, char **argv) {
  std::string_view target = ctx.arg.emulation;

  if (target == I386::name)
    return mold_main<I386>(argc, argv);
  if (target == ARM64LE::name)
    return mold_main<ARM64LE>(argc, argv);
  if (target == ARM64BE::name)
    return mold_main<ARM64BE>(argc, argv);
  if (target == ARM32::name)
    return mold_main<ARM32>(argc, argv);
  if (target == RV64LE::name)
    return mold_main<RV64LE>(argc, argv);
  if (target == RV64BE::name)
    return mold_main<RV64BE>(argc, argv);
  if (target == RV32LE::name)
    return mold_main<RV32LE>(argc, argv);
  if (target == RV32BE::name)
    return mold_main<RV32BE>(argc, argv);
  if (target == PPC32::name)
    return mold_main<PPC32>(argc, argv);
  if (target == PPC64V1::name)
    return mold_main<PPC64V1>(argc, argv);
  if (target == PPC64V2::name)
    return mold_main<PPC64V2>(argc, argv);
  if (target == S390X::name)
    return mold_main<S390X>(argc, argv);
  if (target == SPARC64::name)
    return mold_main<SPARC64>(argc, argv);
  if (target == M68K::name)
    return mold_main<M68K>(argc, argv);
  if (target == SH4LE::name)
    return mold_main<SH4LE>(argc, argv);
  if (target == SH4BE::name)
    return mold_main<SH4BE>(argc, argv);
  if (target == LOONGARCH32::name)
    return mold_main<LOONGARCH32>(argc, argv);
  if (target == LOONGARCH64::name)
    return mold_main<LOONGARCH64>(argc, argv);
  abort();
}

template <typename E>
void apply_exclude_libs(Context<E> &ctx) {
  Timer t(ctx, "apply_exclude_libs");

  std::unordered_set<std::string_view> set(ctx.arg.exclude_libs.begin(),
                                           ctx.arg.exclude_libs.end());

  if (!set.empty())
    for (ObjectFile<E> *file : ctx.objs)
      if (!file->archive_name.empty())
        if (set.contains(path_filename(file->archive_name)) || set.contains("ALL"))
          file->exclude_libs = true;
}

template <typename E>
static bool has_debug_info_section(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    if (file->debug_info)
      return true;
  return false;
}

template <typename E>
void create_synthetic_sections(Context<E> &ctx) {
  auto push = [&](auto *x) {
    ctx.chunks.push_back(x);
    ctx.chunk_pool.emplace_back(x);
    return x;
  };

  if (!ctx.arg.oformat_binary) {
    auto find = [&](std::string_view name) {
      for (SectionOrder &ord : ctx.arg.section_order)
        if (ord.type == SectionOrder::SECTION && ord.name == name)
          return true;
      return false;
    };

    if (ctx.arg.section_order.empty() || find("EHDR"))
      ctx.ehdr = push(new OutputEhdr<E>(SHF_ALLOC));
    else
      ctx.ehdr = push(new OutputEhdr<E>(0));

    if (ctx.arg.section_order.empty() || find("PHDR"))
      ctx.phdr = push(new OutputPhdr<E>(SHF_ALLOC));
    else
      ctx.phdr = push(new OutputPhdr<E>(0));

    if (ctx.arg.z_sectionheader)
      ctx.shdr = push(new OutputShdr<E>);
  }

  ctx.got = push(new GotSection<E>);

  if constexpr (!is_sparc<E>)
    ctx.gotplt = push(new GotPltSection<E>(ctx));

  ctx.reldyn = push(new RelDynSection<E>);
  ctx.relplt = push(new RelPltSection<E>);

  if (ctx.arg.pack_dyn_relocs_relr)
    ctx.relrdyn = push(new RelrDynSection<E>);

  ctx.strtab = push(new StrtabSection<E>);
  ctx.plt = push(new PltSection<E>);
  ctx.pltgot = push(new PltGotSection<E>);
  ctx.symtab = push(new SymtabSection<E>);
  ctx.dynsym = push(new DynsymSection<E>);
  ctx.dynstr = push(new DynstrSection<E>);
  ctx.eh_frame = push(new EhFrameSection<E>);
  ctx.copyrel = push(new CopyrelSection<E>(false));
  ctx.copyrel_relro = push(new CopyrelSection<E>(true));

  if (ctx.shdr)
    ctx.shstrtab = push(new ShstrtabSection<E>);

  if (!ctx.arg.dynamic_linker.empty())
    ctx.interp = push(new InterpSection<E>);
  if (ctx.arg.build_id.kind != BuildId::NONE)
    ctx.buildid = push(new BuildIdSection<E>);
  if (ctx.arg.eh_frame_hdr)
    ctx.eh_frame_hdr = push(new EhFrameHdrSection<E>);
  if (ctx.arg.gdb_index && has_debug_info_section(ctx))
    ctx.gdb_index = push(new GdbIndexSection<E>);
  if (ctx.arg.z_relro && ctx.arg.section_order.empty() &&
      ctx.arg.z_separate_code != SEPARATE_LOADABLE_SEGMENTS)
    ctx.relro_padding = push(new RelroPaddingSection<E>);
  if (ctx.arg.hash_style_sysv)
    ctx.hash = push(new HashSection<E>);
  if (ctx.arg.hash_style_gnu)
    ctx.gnu_hash = push(new GnuHashSection<E>);
  if (!ctx.arg.version_definitions.empty())
    ctx.verdef = push(new VerdefSection<E>);
  if (ctx.arg.emit_relocs)
    ctx.eh_frame_reloc = push(new EhFrameRelocSection<E>);
  if (!ctx.arg.separate_debug_file.empty())
    ctx.gnu_debuglink = push(new GnuDebuglinkSection<E>);

  if (ctx.arg.shared || !ctx.dsos.empty() || ctx.arg.pie) {
    ctx.dynamic = push(new DynamicSection<E>(ctx));

    // If .dynamic exists, .dynsym and .dynstr must exist as well
    // since .dynamic refers to them.
    ctx.dynstr->add_string("");
    ctx.dynsym->symbols.resize(1);
  }

  ctx.versym = push(new VersymSection<E>);
  ctx.verneed = push(new VerneedSection<E>);
  ctx.note_package = push(new NotePackageSection<E>);

  if (!ctx.arg.oformat_binary) {
    ElfShdr<E> shdr = {};
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_MERGE | SHF_STRINGS;
    ctx.comment = MergedSection<E>::get_instance(ctx, ".comment", shdr);
  }

  if constexpr (is_x86<E>)
    ctx.extra.note_property = push(new NotePropertySection<E>);

  if constexpr (is_riscv<E>)
    ctx.extra.riscv_attributes = push(new RiscvAttributesSection<E>);

  if constexpr (is_ppc64v1<E>)
    ctx.extra.opd = push(new PPC64OpdSection);

  if constexpr (is_ppc64v2<E>)
    ctx.extra.save_restore = push(new PPC64SaveRestoreSection);
}

template <typename E>
static void mark_live_objects(Context<E> &ctx) {
  for (Symbol<E> *sym : ctx.arg.undefined)
    if (sym->file)
      sym->file->is_reachable = true;

  for (Symbol<E> *sym : ctx.arg.require_defined)
    if (sym->file)
      sym->file->is_reachable = true;

  if (!ctx.arg.undefined_glob.empty()) {
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      if (!file->is_reachable) {
        for (Symbol<E> *sym : file->get_global_syms()) {
          if (sym->file == file && ctx.arg.undefined_glob.find(sym->name())) {
            file->is_reachable = true;
            sym->gc_root = true;
            break;
          }
        }
      }
    });
  }

  std::vector<InputFile<E> *> roots;

  for (InputFile<E> *file : ctx.objs)
    if (file->is_reachable)
      roots.push_back(file);

  for (InputFile<E> *file : ctx.dsos)
    if (file->is_reachable)
      roots.push_back(file);

  tbb::parallel_for_each(roots, [&](InputFile<E> *file,
                                    tbb::feeder<InputFile<E> *> &feeder) {
    file->mark_live_objects(ctx, [&](InputFile<E> *obj) { feeder.add(obj); });
  });
}

template <typename E>
static void clear_symbols(Context<E> &ctx) {
  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dsos);

  tbb::parallel_for_each(files, [](InputFile<E> *file) {
    for (Symbol<E> *sym : file->get_global_syms()) {
      if (__atomic_load_n(&sym->file, __ATOMIC_ACQUIRE) == file) {
        sym->origin = 0;
        sym->value = -1;
        sym->sym_idx = -1;
        sym->ver_idx = VER_NDX_UNSPECIFIED;
        sym->is_weak = false;
        sym->is_imported = false;
        sym->is_exported = false;
        __atomic_store_n(&sym->file, nullptr, __ATOMIC_RELEASE);
      }
    }
  });
}

template <typename E>
void resolve_symbols(Context<E> &ctx) {
  Timer t(ctx, "resolve_symbols");

  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dsos);

  for (;;) {
    // Call resolve_symbols() to find the most appropriate file for each
    // symbol. And then mark reachable objects to decide which files to
    // include into an output.
    tbb::parallel_for_each(files, [&](InputFile<E> *file) {
      file->resolve_symbols(ctx);
    });

    mark_live_objects(ctx);

    // Now that we know the exact set of input files that are to be
    // included in the output file, we want to redo symbol resolution.
    // This is because symbols defined by object files in archive files
    // may have risen as a result of mark_live_objects().
    //
    // To redo symbol resolution, we want to clear the state first.
    clear_symbols(ctx);

    // COMDAT elimination needs to happen exactly here.
    //
    // It needs to be after archive extraction, otherwise we might
    // assign COMDAT leader to an archive member that is not supposed to
    // be extracted.
    //
    // It needs to happen before the final symbol resolution, otherwise
    // we could eliminate a symbol that is already resolved to and cause
    // dangling references.
    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      if (file->is_reachable)
        for (ComdatGroupRef<E> &ref : file->comdat_groups)
          update_minimum(ref.group->owner, file->priority);
    });

    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      if (file->is_reachable)
        for (ComdatGroupRef<E> &ref : file->comdat_groups)
          if (ref.group->owner != file->priority)
            for (u32 i : ref.members)
              if (InputSection<E> *isec = file->sections[i].get())
                isec->is_alive = false;
    });

    // Redo symbol resolution
    tbb::parallel_for_each(files, [&](InputFile<E> *file) {
      if (file->is_reachable)
        file->resolve_symbols(ctx);
    });

    // Symbols with hidden visibility need to be resolved within the
    // output file. If a hidden symbol was resolved to a DSO, we'll redo
    // symbol resolution from scratch with the flag to skip that symbol
    // next time. This should be rare.
    std::atomic_bool flag = false;

    tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
      if (file->is_reachable) {
        for (Symbol<E> *sym : file->symbols) {
          if (sym->file == file && sym->visibility == STV_HIDDEN) {
            sym->skip_dso = true;
            flag = true;
          }
        }
      }
    });

    if (!flag)
      return;
    clear_symbols(ctx);
  }
}

// Do link-time optimization. We pass all IR object files to the compiler
// backend to compile them into a few ELF object files.
template <typename E>
void do_lto(Context<E> &ctx) {
  Timer t(ctx, "do_lto");

  // The compiler backend needs to know how symbols are resolved, so
  // compute symbol visibility, import/export bits, etc early.
  mark_live_objects(ctx);
  apply_version_script(ctx);
  parse_symbol_version(ctx);
  compute_import_export(ctx);

  // If multiple IR object files define the same symbol, the LTO backend
  // would choose one of them randomly instead of reporting an error.
  // So we need to check for symbol duplication error before doing an LTO.
  if (!ctx.arg.allow_multiple_definition)
    check_duplicate_symbols(ctx);

  // Invoke the LTO plugin. This step compiles IR object files into a few
  // big ELF files.
  std::vector<ObjectFile<E> *> lto_objs = run_lto_plugin(ctx);
  append(ctx.objs, lto_objs);

  // Redo name resolution.
  clear_symbols(ctx);

  // Remove IR object files.
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_lto_obj)
      file->is_reachable = false;

  std::erase_if(ctx.objs, [](ObjectFile<E> *file) { return file->is_lto_obj; });

  resolve_symbols(ctx);
}

template <typename E>
void parse_eh_frame_sections(Context<E> &ctx) {
  Timer t(ctx, "parse_eh_frame_sections");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->parse_ehframe(ctx);

    for (InputSection<E> *isec : file->eh_frame_sections)
      isec->is_alive = false;
  });
}

template <typename E>
void create_merged_sections(Context<E> &ctx) {
  Timer t(ctx, "create_merged_sections");

  // Convert InputSections to MergeableSections.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->convert_mergeable_sections(ctx);
  });

  tbb::parallel_for_each(ctx.merged_sections,
                         [&](std::unique_ptr<MergedSection<E>> &sec) {
    if (sec->shdr.sh_flags & SHF_ALLOC)
      sec->resolve(ctx);
  });

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->reattach_section_pieces(ctx);
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
static bool has_ctors_and_init_array(Context<E> &ctx) {
  bool x = false;
  bool y = false;
  for (ObjectFile<E> *file : ctx.objs) {
    x |= file->has_ctors;
    y |= file->has_init_array;
  }
  return x && y;
}

template <typename E>
static u64 canonicalize_type(std::string_view name, u64 type) {
  // Some old assemblers don't recognize these section names and
  // create them as SHT_PROGBITS.
  if (type == SHT_PROGBITS) {
    if (name == ".init_array" || name.starts_with(".init_array."))
      return SHT_INIT_ARRAY;
    if (name == ".fini_array" || name.starts_with(".fini_array."))
      return SHT_FINI_ARRAY;
  }

  // The x86-64 psABI defines SHT_X86_64_UNWIND for .eh_frame, allowing
  // the linker to recognize the section not by name but by section type.
  // However, that spec change was generally considered a mistake; it has
  // just complicated the situation. As a result, .eh_frame on x86-64 may
  // be either SHT_PROGBITS or SHT_X86_64_UNWIND. We use SHT_PROGBITS
  // consistently.
  if constexpr (is_x86_64<E>)
    if (type == SHT_X86_64_UNWIND)
      return SHT_PROGBITS;

  return type;
}

struct OutputSectionKey {
  bool operator==(const OutputSectionKey &) const = default;
  std::string_view name;
  u64 type;

  struct Hash {
    size_t operator()(const OutputSectionKey &k) const {
      return combine_hash(hash_string(k.name), std::hash<u64>{}(k.type));
    }
  };
};

template <typename E>
static std::string_view
get_output_name(Context<E> &ctx, std::string_view name, u64 flags) {
  if (ctx.arg.relocatable && !ctx.arg.relocatable_merge_sections)
    return name;
  if (ctx.arg.unique && ctx.arg.unique->match(name))
    return name;
  if (flags & SHF_MERGE)
    return name;

  if constexpr (is_arm32<E>) {
    if (name.starts_with(".ARM.exidx"))
      return ".ARM.exidx";
    if (name.starts_with(".ARM.extab"))
      return ".ARM.extab";
  }

  if (ctx.arg.z_keep_text_section_prefix) {
    static std::string_view prefixes[] = {
      ".text.hot.", ".text.unknown.", ".text.unlikely.", ".text.startup.",
      ".text.exit."
    };

    for (std::string_view prefix : prefixes) {
      std::string_view stem = prefix.substr(0, prefix.size() - 1);
      if (name == stem || name.starts_with(prefix))
        return stem;
    }
  }

  static std::string_view prefixes[] = {
    ".text.", ".data.rel.ro.", ".data.", ".rodata.", ".bss.rel.ro.", ".bss.",
    ".init_array.", ".fini_array.", ".tbss.", ".tdata.", ".gcc_except_table.",
    ".ctors.", ".dtors.", ".gnu.warning.", ".openbsd.randomdata.",
    ".sdata.", ".sbss.", ".srodata", ".gnu.build.attributes.",
  };

  for (std::string_view prefix : prefixes) {
    std::string_view stem = prefix.substr(0, prefix.size() - 1);
    if (name == stem || name.starts_with(prefix))
      return stem;
  }

  return name;
}

template <typename E>
static OutputSectionKey
get_output_section_key(Context<E> &ctx, InputSection<E> &isec,
                       bool ctors_in_init_array) {
  // If .init_array/.fini_array exist, .ctors/.dtors must be merged
  // with them.
  //
  // CRT object files contain .ctors/.dtors sections without any
  // relocations. They contain sentinel values, 0 and -1, to mark the
  // beginning and the end of the initializer/finalizer pointer arrays.
  // We do not place them into .init_array/.fini_array because such
  // invalid pointer values would simply make the program to crash.
  if (ctors_in_init_array && !isec.get_rels(ctx).empty()) {
    std::string_view name = isec.name();
    if (name == ".ctors" || name.starts_with(".ctors."))
      return {".init_array", SHT_INIT_ARRAY};
    if (name == ".dtors" || name.starts_with(".dtors."))
      return {".fini_array", SHT_FINI_ARRAY};
  }

  const ElfShdr<E> &shdr = isec.shdr();
  std::string_view name = get_output_name(ctx, isec.name(), shdr.sh_flags);
  u64 type = canonicalize_type<E>(name, shdr.sh_type);
  return {name, type};
}

template <typename E>
static bool is_relro(OutputSection<E> &osec) {
  // PT_GNU_RELRO segment is a security mechanism to make more pages
  // read-only than we could have done without it.
  //
  // Traditionally, sections are either read-only or read-write. If a
  // section contains dynamic relocations, it must have been put into a
  // read-write segment so that the program loader can mutate its
  // contents in memory, even if no one will write to it at runtime.
  //
  // RELRO segment allows us to make such pages writable only when a
  // program is being loaded. After that, the page becomes read-only.
  //
  // Some sections, such as .init, .fini, .got, .dynamic, contain
  // dynamic relocations but doesn't have to be writable at runtime,
  // so they are put into a RELRO segment.
  u32 type = osec.shdr.sh_type;
  u32 flags = osec.shdr.sh_flags;

  return osec.name == ".toc" || osec.name.ends_with(".rel.ro") ||
         type == SHT_INIT_ARRAY || type == SHT_FINI_ARRAY ||
         type == SHT_PREINIT_ARRAY || (flags & SHF_TLS);
}

// Create output sections for input sections.
//
// Since one output section could contain millions of input sections,
// we need to do it efficiently.
template <typename E>
void create_output_sections(Context<E> &ctx) {
  Timer t(ctx, "create_output_sections");

  using MapType = std::unordered_map<OutputSectionKey, OutputSection<E> *,
                                     OutputSectionKey::Hash>;
  MapType map;
  std::shared_mutex mu;
  bool ctors_in_init_array = has_ctors_and_init_array(ctx);
  tbb::enumerable_thread_specific<MapType> caches;

  // Instantiate output sections
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    // Make a per-thread cache of the main map to avoid lock contention.
    // It makes a noticeable difference if we have millions of input sections.
    MapType &cache = caches.local();

    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (!isec || !isec->is_alive)
        continue;

      const ElfShdr<E> &shdr = isec->shdr();
      u32 sh_flags = shdr.sh_flags & ~SHF_MERGE & ~SHF_STRINGS &
                     ~SHF_COMPRESSED & ~SHF_GNU_RETAIN;

      if (ctx.arg.relocatable && (sh_flags & SHF_GROUP)) {
        OutputSection<E> *osec = new OutputSection<E>(isec->name(), shdr.sh_type);
        osec->sh_flags = sh_flags;
        isec->output_section = osec;
        ctx.osec_pool.emplace_back(osec);
        continue;
      }

      auto get_or_insert = [&] {
        OutputSectionKey key =
          get_output_section_key(ctx, *isec, ctors_in_init_array);

        if (auto it = cache.find(key); it != cache.end())
          return it->second;

        {
          std::shared_lock lock(mu);
          if (auto it = map.find(key); it != map.end()) {
            cache.insert({key, it->second});
            return it->second;
          }
        }

        std::unique_ptr<OutputSection<E>> osec =
          std::make_unique<OutputSection<E>>(key.name, key.type);

        std::unique_lock lock(mu);
        auto [it, inserted] = map.insert({key, osec.get()});

        if (inserted)
          ctx.osec_pool.emplace_back(std::move(osec));
        cache.insert({key, it->second});
        return it->second;
      };

      OutputSection<E> *osec = get_or_insert();
      sh_flags &= ~SHF_GROUP;
      if ((osec->sh_flags & sh_flags) != sh_flags)
        osec->sh_flags |= sh_flags;
      isec->output_section = osec;
    }
  });

  // Add input sections to output sections
  for (std::unique_ptr<OutputSection<E>> &osec : ctx.osec_pool)
    osec->members_vec.resize(ctx.objs.size());

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections)
      if (isec && isec->output_section)
        isec->output_section->members_vec[i].push_back(isec.get());
  });

  // Compute section alignment
  for (std::unique_ptr<OutputSection<E>> &osec : ctx.osec_pool) {
    Atomic<u32> p2align;
    tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
      u32 x = 0;
      for (InputSection<E> *isec : osec->members_vec[i])
        x = std::max<u32>(x, isec->p2align);
      update_maximum(p2align, x);
    });
    osec->shdr.sh_addralign = 1 << p2align;
  }

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.osec_pool) {
    osec->shdr.sh_flags = osec->sh_flags;
    osec->is_relro = is_relro(*osec);
    osec->members = flatten(osec->members_vec);
    osec->members_vec.clear();
    osec->members_vec.shrink_to_fit();
  }

  // Add output sections and mergeable sections to ctx.chunks
  std::vector<Chunk<E> *> chunks;
  for (std::unique_ptr<OutputSection<E>> &osec : ctx.osec_pool)
    chunks.push_back(osec.get());
  for (std::unique_ptr<MergedSection<E>> &osec : ctx.merged_sections)
    chunks.push_back(osec.get());

  // Sections are added to the section lists in an arbitrary order
  // because they are created in parallel. Sort them to to make the
  // output deterministic.
  tbb::parallel_sort(chunks.begin(), chunks.end(), [](Chunk<E> *x, Chunk<E> *y) {
    return std::tuple(x->name, x->shdr.sh_type, x->shdr.sh_flags) <
           std::tuple(y->name, y->shdr.sh_type, y->shdr.sh_flags);
  });

  append(ctx.chunks, chunks);
}

// Create a dummy object file containing linker-synthesized
// symbols.
template <typename E>
void create_internal_file(Context<E> &ctx) {
  ObjectFile<E> *obj = new ObjectFile<E>;
  ctx.obj_pool.emplace_back(obj);
  ctx.internal_obj = obj;
  ctx.objs.push_back(obj);

  // Create linker-synthesized symbols.
  ctx.internal_esyms.resize(1);

  obj->symbols.push_back(new Symbol<E>);
  obj->first_global = 1;
  obj->is_reachable = true;
  obj->priority = 1;

  auto add = [&](Symbol<E> *sym) {
    obj->symbols.push_back(sym);

    // An actual value will be set to a linker-synthesized symbol by
    // fix_synthetic_symbols(). Until then, `value` doesn't have a valid
    // value. 0xdeadbeef is a unique dummy value to make debugging easier
    // if the field is accidentally used before it gets a valid one.
    sym->value = 0xdeadbeef;

    ElfSym<E> esym;
    memset(&esym, 0, sizeof(esym));
    esym.st_type = STT_NOTYPE;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = STV_DEFAULT;
    ctx.internal_esyms.push_back(esym);
  };

  // Add --defsym'd symbols
  for (i64 i = 0; i < ctx.arg.defsyms.size(); i++)
    add(ctx.arg.defsyms[i].first);

  // Add --section-order symbols
  for (SectionOrder &ord : ctx.arg.section_order)
    if (ord.type == SectionOrder::SYMBOL)
      add(get_symbol(ctx, ord.name));

  obj->elf_syms = ctx.internal_esyms;
}

template <typename E>
static std::optional<std::string>
get_start_stop_name(Context<E> &ctx, Chunk<E> &chunk) {
  if ((chunk.shdr.sh_flags & SHF_ALLOC) && !chunk.name.empty()) {
    if (is_c_identifier(chunk.name))
      return std::string(chunk.name);

    if (ctx.arg.start_stop) {
      auto isalnum = [](char c) {
        return ('a' <= c && c <= 'z') || ('A' <= c && c <= 'Z') ||
               ('0' <= c && c <= '9');
      };

      std::string s{chunk.name};
      if (s.starts_with('.'))
        s = s.substr(1);

      for (i64 i = 0; i < s.size(); i++)
        if (!isalnum(s[i]))
          s[i] = '_';
      return s;
    }
  }

  return {};
}

template <typename E>
void add_synthetic_symbols(Context<E> &ctx) {
  ObjectFile<E> &obj = *ctx.internal_obj;

  auto add = [&](std::string_view name, u32 type = STT_NOTYPE) {
    ElfSym<E> esym;
    memset(&esym, 0, sizeof(esym));
    esym.st_type = type;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = STV_HIDDEN;
    ctx.internal_esyms.push_back(esym);

    Symbol<E> *sym = get_symbol(ctx, name);
    sym->value = 0xdeadbeef; // unique dummy value
    obj.symbols.push_back(sym);
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
  ctx._PROCEDURE_LINKAGE_TABLE_ = add("_PROCEDURE_LINKAGE_TABLE_");
  ctx.__bss_start = add("__bss_start");
  ctx._end = add("_end");
  ctx._etext = add("_etext");
  ctx._edata = add("_edata");
  ctx.__executable_start = add("__executable_start");

  ctx.__rel_iplt_start =
    add(E::is_rela ? "__rela_iplt_start" : "__rel_iplt_start");
  ctx.__rel_iplt_end =
    add(E::is_rela ? "__rela_iplt_end" : "__rel_iplt_end");

  if (ctx.arg.eh_frame_hdr)
    ctx.__GNU_EH_FRAME_HDR = add("__GNU_EH_FRAME_HDR");

  if (!get_symbol(ctx, "end")->file)
    ctx.end = add("end");
  if (!get_symbol(ctx, "etext")->file)
    ctx.etext = add("etext");
  if (!get_symbol(ctx, "edata")->file)
    ctx.edata = add("edata");
  if (!get_symbol(ctx, "__dso_handle")->file)
    ctx.__dso_handle = add("__dso_handle");

  if constexpr (supports_tlsdesc<E>)
    ctx._TLS_MODULE_BASE_ = add("_TLS_MODULE_BASE_", STT_TLS);

  if constexpr (is_riscv<E>) {
    ctx.__global_pointer = add("__global_pointer$");
    if (ctx.dynamic && !ctx.arg.shared)
      ctx.__global_pointer->is_exported = true;
  }

  if constexpr (is_arm32<E>) {
    ctx.__exidx_start = add("__exidx_start");
    ctx.__exidx_end = add("__exidx_end");
  }

  if constexpr (is_ppc64<E>)
    ctx.extra.TOC = add(".TOC.");

  if constexpr (is_ppc32<E>)
    ctx.extra._SDA_BASE_ = add("_SDA_BASE_");

  auto add_start_stop = [&](std::string s) {
    add(save_string(ctx, s));
    if (ctx.arg.z_start_stop_visibility_protected)
      get_symbol(ctx, save_string(ctx, s))->is_exported = true;
  };

  for (Chunk<E> *chunk : ctx.chunks) {
    if (std::optional<std::string> name = get_start_stop_name(ctx, *chunk)) {
      add_start_stop("__start_" + *name);
      add_start_stop("__stop_" + *name);

      if (ctx.arg.physical_image_base) {
        add_start_stop("__phys_start_" + *name);
        add_start_stop("__phys_stop_" + *name);
      }
    }
  }

  if constexpr (is_ppc64v2<E>)
    for (std::pair<std::string_view, u32> p : ppc64_save_restore_insns)
      if (std::string_view label = p.first; !label.empty())
        add(label);

  obj.elf_syms = ctx.internal_esyms;
  obj.resolve_symbols(ctx);

  // Make all synthetic symbols relative ones by associating them to
  // a dummy output section.
  for (Symbol<E> *sym : obj.symbols) {
    if (sym->file == &obj) {
      sym->set_output_section(ctx.symtab);
      sym->is_imported = false;
    }
  }

  // Handle --defsym symbols.
  for (i64 i = 0; i < ctx.arg.defsyms.size(); i++) {
    Symbol<E> *sym1 = ctx.arg.defsyms[i].first;
    std::variant<Symbol<E> *, u64> val = ctx.arg.defsyms[i].second;

    if (Symbol<E> **ref = std::get_if<Symbol<E> *>(&val)) {
      Symbol<E> *sym2 = *ref;
      if (!sym2->file) {
        Error(ctx) << "--defsym: undefined symbol: " << *sym2;
        continue;
      }

      ElfSym<E> &esym = obj.elf_syms[i + 1];
      esym.st_type = sym2->esym().st_type;
      if constexpr (is_ppc64v2<E>)
        esym.ppc64_local_entry = sym2->esym().ppc64_local_entry;

      if (sym2->is_absolute())
        sym1->origin = 0;
    } else {
      sym1->origin = 0;
    }
  }
}

template <typename E>
void apply_section_align(Context<E> &ctx) {
  std::unordered_map<std::string_view, u64> &map = ctx.arg.section_align;
  if (!map.empty())
    for (Chunk<E> *chunk : ctx.chunks)
      if (OutputSection<E> *osec = chunk->to_osec())
        if (auto it = map.find(osec->name); it != map.end())
          osec->shdr.sh_addralign = it->second;
}

template <typename E>
void check_cet_errors(Context<E> &ctx) {
  bool warning = (ctx.arg.z_cet_report == CET_REPORT_WARNING);
  assert(warning || ctx.arg.z_cet_report == CET_REPORT_ERROR);

  auto has_feature = [](ObjectFile<E> *file, u32 feature) {
    return std::any_of(file->gnu_properties.begin(), file->gnu_properties.end(),
                       [&](std::pair<u32, u32> kv) {
      return kv.first == GNU_PROPERTY_X86_FEATURE_1_AND &&
             (kv.second & feature);
    });
  };

  for (ObjectFile<E> *file : ctx.objs) {
    if (file == ctx.internal_obj)
      continue;

    if (!has_feature(file, GNU_PROPERTY_X86_FEATURE_1_IBT)) {
      if (warning)
        Warn(ctx) << *file << ": -cet-report=warning: "
                  << "missing GNU_PROPERTY_X86_FEATURE_1_IBT";
      else
        Error(ctx) << *file << ": -cet-report=error: "
                   << "missing GNU_PROPERTY_X86_FEATURE_1_IBT";
    }

    if (!has_feature(file, GNU_PROPERTY_X86_FEATURE_1_SHSTK)) {
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
  Out(ctx) <<
R"(# This is an output of the mold linker's --print-dependencies option.
#
# Each line consists of 4 fields, <section1>, <section2>, <symbol-type> and
# <symbol>, separated by tab characters. It indicates that <section1> depends
# on <section2> to use <symbol>. <symbol-type> is either "u" or "w" for
# regular undefined or weak undefined, respectively.
#
# If you want to obtain dependency information per function granularity,
# compile source files with the -ffunction-sections compiler flag.
)";

  auto println = [&](auto &src, Symbol<E> &sym, ElfSym<E> &esym) {
    if (InputSection<E> *isec = sym.get_input_section())
      Out(ctx) << src << "\t" << *isec
               << "\t" << (esym.is_weak() ? 'w' : 'u')
               << "\t" << sym;
    else
      Out(ctx) << src << "\t" << *sym.file
               << "\t" << (esym.is_weak() ? 'w' : 'u')
               << "\t" << sym;
  };

  for (ObjectFile<E> *file : ctx.objs) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (!isec)
        continue;

      std::unordered_set<void *> visited;

      for (const ElfRel<E> &r : isec->get_rels(ctx)) {
        if (r.r_type == R_NONE || file->elf_syms.size() <= r.r_sym)
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

  std::string cwd = std::filesystem::current_path().string();
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
    TarWriter::open(path, path_filename(ctx.arg.output) + ".repro");
  if (!tar)
    Fatal(ctx) << "cannot open " << path << ": " << errno_string();

  tar->append("response.txt", create_response_file(ctx));
  tar->append("version.txt", get_mold_version() + "\n");

  std::unordered_set<std::string_view> seen;

  for (std::unique_ptr<MappedFile> &mf : ctx.mf_pool) {
    if (!mf->parent && seen.insert(mf->name).second) {
      // We reopen a file because we may have modified the contents of mf
      // in memory, which is mapped with PROT_WRITE and MAP_PRIVATE.
      MappedFile *mf2 = must_open_file(ctx, mf->name);
      tar->append(std::filesystem::absolute(mf->name).string(),
                  mf2->get_contents());
      mf2->unmap();
    }
  }
}

template <typename E>
void check_duplicate_symbols(Context<E> &ctx) {
  Timer t(ctx, "check_duplicate_symbols");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    if (!file->is_reachable)
      return;

    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];

      // Skip if our symbol is undef or weak
      if (!sym.file || sym.file == file || sym.file == ctx.internal_obj ||
          esym.is_undef() || esym.is_common() || (esym.st_bind == STB_WEAK))
        continue;

      // Skip if our symbol is in a dead section. In most cases, the
      // section has been eliminated due to comdat deduplication.
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

// If --no-allow-shlib-undefined is specified, we report errors on
// unresolved symbols in shared libraries. This is useful when you are
// creating a final executable and want to make sure that all symbols
// including ones in shared libraries have been resolved.
//
// If you do not pass --no-allow-shlib-undefined, undefined symbols in
// shared libraries will be reported as run-time error by the dynamic
// linker.
template <typename E>
void check_shlib_undefined(Context<E> &ctx) {
  Timer t(ctx, "check_shlib_undefined");

  auto is_sparc_register = [](const ElfSym<E> &esym) {
    // Dynamic symbol table for SPARC contains bogus entries which
    // we need to ignore
    if constexpr (is_sparc<E>)
      return esym.st_type == STT_SPARC_REGISTER;
    return false;
  };

  // Obtain a list of known shared library names.
  std::unordered_set<std::string_view> sonames;
  for (std::unique_ptr<SharedFile<E>> &file : ctx.dso_pool)
    sonames.insert(file->soname);

  tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
    // Skip the file if it depends on a file that we know nothing about.
    // This is because missing symbols may be provided by that unknown file.
    for (std::string_view needed : file->get_dt_needed(ctx))
      if (!sonames.contains(needed))
        return;

    // Check if all undefined symbols have been resolved.
    for (i64 i = 0; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];
      if (esym.is_undef() && !esym.is_weak() && !sym.file &&
          !is_sparc_register(esym))
        Error(ctx) << *file << ": --no-allow-shlib-undefined: undefined symbol: "
                   << sym;
    }
  });

  // Beyond this point, DSOs that are not referenced directly by any
  // object file are not needed. They were kept by
  // SharedFile<E>::mark_live_objects just for this pass. Therefore,
  // remove unneeded DSOs from the list now.
  for (SharedFile<E> *file : ctx.dsos)
    file->is_reachable = false;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->get_global_syms())
      if (InputFile<E> *file = sym->file)
        if (file->is_dso)
          file->is_reachable.test_and_set();
  });

  std::erase_if(ctx.dsos, [](SharedFile<E> *file) { return !file->is_reachable; });
}

template <typename E>
void check_symbol_types(Context<E> &ctx) {
  Timer t(ctx, "check_symbol_types");

  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dsos);

  auto canonicalize = [](u32 ty) -> u32 {
    if (ty == STT_GNU_IFUNC)
      return STT_FUNC;
    if (ty == STT_COMMON)
      return STT_OBJECT;
    return ty;
  };

  tbb::parallel_for_each(files.begin(), files.end(), [&](InputFile<E> *file) {
    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      Symbol<E> &sym = *file->symbols[i];
      if (!sym.file || sym.file == file)
        continue;

      const ElfSym<E> &esym1 = sym.esym();
      const ElfSym<E> &esym2 = file->elf_syms[i];

      if (esym1.st_type != STT_NOTYPE && esym2.st_type != STT_NOTYPE &&
          canonicalize(esym1.st_type) != canonicalize(esym2.st_type)) {
        Warn(ctx) << "symbol type mismatch: " << sym << '\n'
                  << ">>> defined in " << *sym.file << " as "
                  << stt_to_string<E>(esym1.st_type) << '\n'
                  << ">>> defined in " << *file << " as "
                  << stt_to_string<E>(esym2.st_type);
      }
    }
  });
}

template <typename E>
static i64 get_init_fini_priority(InputSection<E> *isec) {
  static std::regex re(R"(\.(\d+)$)", std::regex_constants::optimize);
  std::string_view name = isec->name();
  std::cmatch m;
  if (std::regex_search(name.data(), name.data() + name.size(), m, re))
    return std::stoi(m[1]);
  return 65536;
}

template <typename E>
static i64 get_ctor_dtor_priority(InputSection<E> *isec) {
  auto opts = std::regex_constants::optimize | std::regex_constants::ECMAScript;
  static std::regex re1(R"((?:clang_rt\.)?crtbegin)", opts);
  static std::regex re2(R"((?:clang_rt\.)?crtend)", opts);
  static std::regex re3(R"(\.(\d+)$)", opts);

  // crtbegin.o and crtend.o contain marker symbols such as
  // __CTOR_LIST__ or __DTOR_LIST__. So they have to be at the
  // beginning or end of the section.
  std::smatch m;
  if (std::regex_search(isec->file.filename, m, re1))
    return -2;
  if (std::regex_search(isec->file.filename, m, re2))
    return 65536;

  std::string name(isec->name());
  if (std::regex_search(name, m, re3))
    return std::stoi(m[1]);
  return -1;
}

template <typename E>
void sort_init_fini(Context<E> &ctx) {
  Timer t(ctx, "sort_init_fini");

  struct Entry {
    InputSection<E> *sect;
    i64 prio;
  };

  for (Chunk<E> *chunk : ctx.chunks) {
    if (OutputSection<E> *osec = chunk->to_osec()) {
      if (osec->name == ".init_array" || osec->name == ".preinit_array" ||
          osec->name == ".fini_array") {
        if (ctx.arg.shuffle_sections == SHUFFLE_SECTIONS_REVERSE)
          std::reverse(osec->members.begin(), osec->members.end());

        std::vector<Entry> vec;

        for (InputSection<E> *isec : osec->members) {
          std::string_view name = isec->name();
          if (name.starts_with(".ctors") || name.starts_with(".dtors"))
            vec.push_back({isec, 65535 - get_ctor_dtor_priority(isec)});
          else
            vec.push_back({isec, get_init_fini_priority(isec)});
        }

        sort(vec, [](const Entry &a, const Entry &b) { return a.prio < b.prio; });

        for (i64 i = 0; i < vec.size(); i++)
          osec->members[i] = vec[i].sect;
      }
    }
  }
}

template <typename E>
void sort_ctor_dtor(Context<E> &ctx) {
  Timer t(ctx, "sort_ctor_dtor");

  struct Entry {
    InputSection<E> *sect;
    i64 prio;
  };

  for (Chunk<E> *chunk : ctx.chunks) {
    if (OutputSection<E> *osec = chunk->to_osec()) {
      if (osec->name == ".ctors" || osec->name == ".dtors") {
        if (ctx.arg.shuffle_sections != SHUFFLE_SECTIONS_REVERSE)
          std::reverse(osec->members.begin(), osec->members.end());

        std::vector<Entry> vec;
        for (InputSection<E> *isec : osec->members)
          vec.push_back({isec, get_ctor_dtor_priority(isec)});

        sort(vec, [](const Entry &a, const Entry &b) { return a.prio < b.prio; });

        for (i64 i = 0; i < vec.size(); i++)
          osec->members[i] = vec[i].sect;
      }
    }
  }
}

// .ctors/.dtors serves the same purpose as .init_array/.fini_array,
// albeit with very subtly differences. Both contain pointers to
// initializer/finalizer functions. The runtime executes them one by one
// but in the exact opposite order to one another. Therefore, if we are to
// place the contents of .ctors/.dtors into .init_array/.fini_array, we
// need to reverse them.
//
// It's unfortunate that we have both .ctors/.dtors and
// .init_array/.fini_array in ELF for historical reasons, but that's
// the reality we need to deal with.
template <typename E>
void fixup_ctors_in_init_array(Context<E> &ctx) {
  Timer t(ctx, "fixup_ctors_in_init_array");

  auto reverse_contents = [&](InputSection<E> &isec) {
    if (isec.sh_size % sizeof(Word<E>))
      Fatal(ctx) << isec << ": section corrupted";

    u8 *buf = (u8 *)isec.contents.data();
    std::reverse((Word<E> *)buf, (Word<E> *)(buf + isec.sh_size));

    std::span<ElfRel<E>> rels = isec.get_rels(ctx);
    for (ElfRel<E> &r : rels)
      r.r_offset = isec.sh_size - r.r_offset - sizeof(Word<E>);

    sort(rels, [](const ElfRel<E> &a, const ElfRel<E> &b) {
      return a.r_offset < b.r_offset;
    });
  };

  if (Chunk<E> *chunk = find_chunk(ctx, ".init_array"))
    if (OutputSection<E> *osec = chunk->to_osec())
      for (InputSection<E> *isec : osec->members)
        if (isec->name().starts_with(".ctors"))
          reverse_contents(*isec);

  if (Chunk<E> *chunk = find_chunk(ctx, ".fini_array"))
    if (OutputSection<E> *osec = chunk->to_osec())
      for (InputSection<E> *isec : osec->members)
        if (isec->name().starts_with(".dtors"))
          reverse_contents(*isec);
}

template <typename T>
static void shuffle(std::vector<T> &vec, u64 seed) {
  if (vec.empty())
    return;

  // Xorshift random number generator. We use this RNG because it is
  // measurably faster than MT19937.
  auto rand = [&] {
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

  auto is_eligible = [](OutputSection<E> *osec) {
    if (osec) {
      std::string_view name = osec->name;
      return name != ".init" && name != ".fini" &&
             name != ".ctors" && name != ".dtors" &&
             name != ".init_array" && name != ".preinit_array" &&
             name != ".fini_array";
    }
    return false;
  };

  switch (ctx.arg.shuffle_sections) {
  case SHUFFLE_SECTIONS_SHUFFLE: {
    tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
      if (OutputSection<E> *osec = chunk->to_osec(); is_eligible(osec)) {
        u64 seed = ctx.arg.shuffle_sections_seed + hash_string(osec->name);
        shuffle(osec->members, seed);
      }
    });
    break;
  }
  case SHUFFLE_SECTIONS_REVERSE:
    tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
      if (OutputSection<E> *osec = chunk->to_osec(); is_eligible(osec))
        std::reverse(osec->members.begin(), osec->members.end());
    });
    break;
  default:
    unreachable();
  }
}

template <typename E>
void compute_section_sizes(Context<E> &ctx) {
  Timer t(ctx, "compute_section_sizes");

  if constexpr (needs_thunk<E>) {
    std::vector<Chunk<E> *> vec = ctx.chunks;

    auto mid = std::partition(vec.begin(), vec.end(), [&](Chunk<E> *chunk) {
      return chunk->to_osec() && (chunk->shdr.sh_flags & SHF_EXECINSTR) &&
             !ctx.arg.relocatable;
    });

    // create_range_extension_thunks is not thread-safe
    for (Chunk<E> *chunk : std::span(vec.begin(), mid))
      chunk->to_osec()->create_range_extension_thunks(ctx);

    tbb::parallel_for_each(mid, vec.end(), [&](Chunk<E> *chunk) {
      chunk->compute_section_size(ctx);
    });
  } else {
    tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
      chunk->compute_section_size(ctx);
    });
  }
}

// Find all unresolved symbols and attach them to the most appropriate files.
//
// Note that even a symbol that will be reported as an undefined symbol
// will get an owner file in this function. Such symbol will be reported
// by ObjectFile<E>::scan_relocations(). This is because we want to report
// errors only on symbols that are actually referenced.
template <typename E>
void claim_unresolved_symbols(Context<E> &ctx) {
  Timer t(ctx, "claim_unresolved_symbols");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    if (file == ctx.internal_obj)
      return;

    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];
      if (!esym.is_undef())
        continue;

      std::scoped_lock lock(sym.mu);

      if (sym.file)
        if (!sym.esym().is_undef() || sym.file->priority <= file->priority)
          continue;

      // If a symbol name is in the form of "foo@version", search for
      // symbol "foo" and check if the symbol has version "version".
      if (file->has_symver[i - file->first_global]) {
        std::string_view str = file->symbol_strtab.data() + esym.st_name;
        i64 pos = str.find('@');
        assert(pos != str.npos);

        std::string_view name = str.substr(0, pos);
        std::string_view ver = str.substr(pos + 1);

        Symbol<E> *sym2 = get_symbol(ctx, name);
        if (sym2->file && sym2->file->is_dso && sym2->get_version() == ver) {
          file->symbols[i] = sym2;
          sym2->is_imported = true;
          continue;
        }
      }

      auto claim = [&](bool is_imported) {
        if (sym.is_traced)
          Out(ctx) << "trace-symbol: " << *file << ": unresolved"
                   << (esym.is_weak() ? " weak" : "")
                   << " symbol " << sym;

        sym.file = file;
        sym.origin = 0;
        sym.value = 0;
        sym.sym_idx = i;
        sym.is_weak = false;
        sym.is_imported = is_imported;
        sym.is_exported = false;
        sym.ver_idx = is_imported ? 0 : ctx.default_version;
      };

      if (esym.is_undef_weak()) {
        if (ctx.arg.z_dynamic_undefined_weak && sym.visibility != STV_HIDDEN) {
          // Global weak undefined symbols are promoted to dynamic symbols
          // by default only when linking a DSO. We generally cannot do that
          // for executables because we may need to create a copy relocation
          // for a data symbol, but the symbol size is not available for an
          // unclaimed weak symbol.
          //
          // In contrast, GNU ld promotes weak symbols to dynamic ones even
          // for an executable as long as they don't need copy relocations
          // (i.e. they need only PLT entries.) That may result in an
          // inconsistent behavior of a linked program depending on whether
          // whether its object files were compiled with -fPIC or not. I think
          // that's bad semantics, so we don't do that.
          claim(true);
        } else {
          // Otherwise, weak undefs are converted to absolute symbols with value 0.
          claim(false);
        }
        continue;
      }

      // Traditionally, remaining undefined symbols cause a link failure
      // only when we are creating an executable. Undefined symbols in
      // shared objects are promoted to dynamic symbols, so that they'll
      // get another chance to be resolved at run-time. You can change the
      // behavior by passing `-z defs` to the linker.
      //
      // Even if `-z defs` is given, weak undefined symbols are still
      // promoted to dynamic symbols for compatibility with other linkers.
      // Some major programs, notably Firefox, depend on the behavior
      // (they use this loophole to export symbols from libxul.so).
      if (ctx.arg.shared && sym.visibility != STV_HIDDEN &&
          ctx.arg.unresolved_symbols != UNRESOLVED_ERROR) {
        claim(true);
        continue;
      }

      // Convert remaining undefined symbols to absolute symbols with value 0.
      claim(false);
    }
  });
}

template <typename E>
void scan_relocations(Context<E> &ctx) {
  Timer t(ctx, "scan_relocations");

  // Scan relocations to find dynamic symbols.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->scan_relocations(ctx);
  });

  // Word-size absolute relocations (e.g. R_X86_64_64) are handled
  // separately because they can be promoted to dynamic relocations.
  tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
    if (OutputSection<E> *osec = chunk->to_osec())
      if (osec->shdr.sh_flags & SHF_ALLOC)
        osec->scan_abs_relocations(ctx);
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

  if (ctx.needs_tlsld)
    ctx.got->add_tlsld(ctx);

  // Assign offsets in additional tables for each dynamic symbol.
  for (Symbol<E> *sym : syms) {
    sym->add_aux(ctx);

    if (sym->is_imported || sym->is_exported)
      ctx.dynsym->add_symbol(ctx, sym);

    if (sym->flags & NEEDS_GOT)
      ctx.got->add_got_symbol(ctx, sym);

    if (sym->flags & NEEDS_CPLT) {
      sym->is_canonical = true;

      // A canonical PLT needs to be visible from DSOs.
      sym->is_exported = true;

      // We can't use .plt.got for a canonical PLT because otherwise
      // .plt.got and .got would refer to each other, resulting in an
      // infinite loop at runtime.
      ctx.plt->add_symbol(ctx, sym);
    } else if (sym->flags & NEEDS_PLT) {
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

    if (sym->flags & NEEDS_COPYREL) {
      if (ctx.arg.z_relro && ((SharedFile<E> *)sym->file)->is_readonly(sym))
        ctx.copyrel_relro->add_symbol(ctx, sym);
      else
        ctx.copyrel->add_symbol(ctx, sym);
    }

    if constexpr (is_ppc64v1<E>)
      if (sym->flags & NEEDS_PPC_OPD)
        ctx.extra.opd->add_symbol(ctx, sym);

    sym->flags = 0;
  }

  if (ctx.has_textrel && ctx.arg.warn_textrel)
    Warn(ctx) << "creating a DT_TEXTREL in an output file";
}

// Compute the is_weak bit for each imported symbol.
//
// If all references to a shared symbol is weak, the symbol is marked
// as weak in .dynsym.
template <typename E>
void compute_imported_symbol_weakness(Context<E> &ctx) {
  Timer t(ctx, "compute_imported_symbol_weakness");

  tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];

      if (esym.is_undef() && !esym.is_weak() && sym.file && sym.file->is_dso) {
        std::scoped_lock lock(sym.mu);
        sym.is_weak = false;
      }
    }
  });
}

// Report all undefined symbols, grouped by symbol.
template <typename E>
void report_undef_errors(Context<E> &ctx) {
  constexpr i64 MAX_ERRORS = 3;

  if (ctx.arg.unresolved_symbols == UNRESOLVED_IGNORE)
    return;

  for (auto &pair : ctx.undef_errors) {
    Symbol<E> *sym = pair.first;
    std::span<std::string> errors = pair.second;

    std::stringstream ss;
    ss << "undefined symbol: "
       << (ctx.arg.demangle ? demangle(*sym) : sym->name())
       << "\n";

    for (i64 i = 0; i < errors.size() && i < MAX_ERRORS; i++)
      ss << errors[i];

    if (MAX_ERRORS < errors.size())
      ss << ">>> referenced " << (errors.size() - MAX_ERRORS) << " more times\n";

    // Remove the trailing '\n' because Error/Warn adds it automatically
    std::string msg = ss.str();
    msg.pop_back();

    if (ctx.arg.unresolved_symbols == UNRESOLVED_ERROR)
      Error(ctx) << msg;
    else
      Warn(ctx) << msg;
  }

  ctx.checkpoint();
}

template <typename E>
void create_reloc_sections(Context<E> &ctx) {
  Timer t(ctx, "create_reloc_sections");

  // Create .rela.* sections
  tbb::parallel_for((i64)0, (i64)ctx.chunks.size(), [&](i64 i) {
    if (OutputSection<E> *osec = ctx.chunks[i]->to_osec())
      osec->reloc_sec.reset(new RelocSection<E>(ctx, *osec));
  });

  for (i64 i = 0, end = ctx.chunks.size(); i < end; i++)
    if (OutputSection<E> *osec = ctx.chunks[i]->to_osec())
      if (RelocSection<E> *x = osec->reloc_sec.get())
        ctx.chunks.push_back(x);
}

// Copy chunks to an output file
template <typename E>
void copy_chunks(Context<E> &ctx) {
  Timer t(ctx, "copy_chunks");

  auto copy = [&](Chunk<E> &chunk) {
    std::string name = chunk.name.empty() ? "(header)" : std::string(chunk.name);
    Timer t2(ctx, name, &t);
    chunk.copy_buf(ctx);
  };

  // For --relocatable and --emit-relocs, we want to copy non-relocation
  // sections first. This is because REL-type relocation sections (as
  // opposed to RELA-type) stores relocation addends to target sections.
  //
  // We also does that for SH4 because despite being RELA, we always need
  // to write addends to relocated places for SH4.
  auto is_rel = [](Chunk<E> &chunk) {
    return chunk.shdr.sh_type == SHT_REL ||
           (is_sh4<E> && chunk.shdr.sh_type == SHT_RELA);
  };

  tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
    if (!is_rel(*chunk))
      copy(*chunk);
  });

  tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
    if (is_rel(*chunk))
      copy(*chunk);
  });

  // Undefined symbols in SHF_ALLOC sections are found by scan_relocations(),
  // but those in non-SHF_ALLOC sections cannot be found until we copy section
  // contents. So we need to call this function again to report possible
  // undefined errors.
  report_undef_errors(ctx);

  // Zero-clear paddings between chunks
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

template <typename E>
void construct_relr(Context<E> &ctx) {
  Timer t(ctx, "construct_relr");

  tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
    chunk->construct_relr(ctx);
  });
}

// The hash function for .gnu.hash.
static u32 djb_hash(std::string_view name) {
  u32 h = 5381;
  for (u8 c : name)
    h = (h << 5) + h + c;
  return h;
}

template <typename E>
void sort_dynsyms(Context<E> &ctx) {
  Timer t(ctx, "sort_dynsyms");

  std::span<Symbol<E> *> syms = ctx.dynsym->symbols;
  if (syms.empty())
    return;

  // In any symtab, local symbols must precede global symbols.
  auto first_global = std::stable_partition(syms.begin() + 1, syms.end(),
                                            [&](Symbol<E> *sym) {
    return sym->is_local(ctx);
  });

  // .gnu.hash imposes more restrictions on the order of the symbols in
  // .dynsym.
  if (ctx.gnu_hash) {
    auto first_exported = std::stable_partition(first_global, syms.end(),
                                                [](Symbol<E> *sym) {
      return !sym->is_exported;
    });

    // Count the number of exported symbols to compute the size of .gnu.hash.
    i64 num_exported = syms.end() - first_exported;
    u32 num_buckets = num_exported / ctx.gnu_hash->LOAD_FACTOR + 1;

    tbb::parallel_for_each(first_exported, syms.end(), [&](Symbol<E> *sym) {
      sym->set_djb_hash(ctx, djb_hash(sym->name()));
    });

    tbb::parallel_sort(first_exported, syms.end(),
                       [&](Symbol<E> *a, Symbol<E> *b) {
      return std::tuple(a->get_djb_hash(ctx) % num_buckets, a->name()) <
             std::tuple(b->get_djb_hash(ctx) % num_buckets, b->name());
    });

    ctx.gnu_hash->num_buckets = num_buckets;
    ctx.gnu_hash->num_exported = num_exported;
  }

  // Compute .dynstr size
  ctx.dynsym->dynstr_offset = ctx.dynstr->shdr.sh_size;

  tbb::enumerable_thread_specific<i64> size;
  tbb::parallel_for((i64)1, (i64)syms.size(), [&](i64 i) {
    syms[i]->set_dynsym_idx(ctx, i);
    size.local() += syms[i]->name().size() + 1;
  });

  ctx.dynstr->shdr.sh_size += size.combine(std::plus());

  // ELF's symbol table sh_info holds the offset of the first global symbol.
  ctx.dynsym->shdr.sh_info = first_global - syms.begin();
}

template <typename E>
void create_output_symtab(Context<E> &ctx) {
  Timer t(ctx, "compute_symtab_size");

  if constexpr (needs_thunk<E>) {
    i64 n = 0;
    for (Chunk<E> *chunk : ctx.chunks)
      if (OutputSection<E> *osec = chunk->to_osec())
        for (std::unique_ptr<Thunk<E>> &thunk : osec->thunks)
          thunk->name = "thunk" + std::to_string(n++);
  }

  tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
    chunk->compute_symtab_size(ctx);
  });

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->compute_symtab_size(ctx);
  });

  tbb::parallel_for_each(ctx.dsos, [&](SharedFile<E> *file) {
    file->compute_symtab_size(ctx);
  });
}

template <typename E>
void apply_version_script(Context<E> &ctx) {
  Timer t(ctx, "apply_version_script");

  // Assign versions to symbols specified with `extern "C++"` or
  // wildcard patterns first.
  MultiGlob matcher;
  MultiGlob cpp_matcher;

  // The "local:" label has a special meaning in the version script.
  // It can appear in any VERSION clause, and it hides matched symbols
  // unless other non-local patterns match to them. In other words,
  // "local:" has lower precedence than other version definitions.
  //
  // If two or more non-local patterns match to the same symbol, the
  // last one takes precedence.
  std::vector<VersionPattern> patterns = ctx.version_patterns;

  std::stable_partition(patterns.begin(), patterns.end(),
                        [](const VersionPattern &pat) {
    return pat.ver_idx == VER_NDX_LOCAL;
  });

  auto has_wildcard = [](std::string_view str) {
    return str.find_first_of("*?[") != str.npos;
  };

  for (i64 i = 0; i < patterns.size(); i++) {
    VersionPattern &v = patterns[i];
    if (v.is_cpp) {
      if (!cpp_matcher.add(v.pattern, i))
        Fatal(ctx) << "invalid version pattern: " << v.pattern;
    } else if (has_wildcard(v.pattern)) {
      if (!matcher.add(v.pattern, i))
        Fatal(ctx) << "invalid version pattern: " << v.pattern;
    }
  }

  if (!matcher.empty() || !cpp_matcher.empty()) {
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      for (Symbol<E> *sym : file->get_global_syms()) {
        if (sym->file != file)
          continue;

        std::string_view name = sym->name();
        i64 match = -1;

        if (std::optional<i64> idx = matcher.find(name))
          match = std::max(match, *idx);

        // Match non-mangled symbols against the C++ pattern as well.
        // Weird, but required to match other linkers' behavior.
        if (!cpp_matcher.empty()) {
          if (std::optional<std::string_view> s = demangle_cpp(name))
            name = *s;
          if (std::optional<i64> idx = cpp_matcher.find(name))
            match = std::max(match, *idx);
        }

        if (match != -1)
          sym->ver_idx = patterns[match].ver_idx;
      }
    });
  }

  // Next, assign versions to symbols specified by exact name.
  // In other words, exact matches have higher precedence over
  // wildcard or `extern "C++"` patterns.
  for (VersionPattern &v : patterns) {
    if (!v.is_cpp && !has_wildcard(v.pattern)) {
      Symbol<E> *sym = get_symbol(ctx, v.pattern);

      if (!sym->file && !ctx.arg.undefined_version)
        Warn(ctx) << v.source << ": cannot assign version `" << v.ver_str
                  << "` to symbol `" << *sym << "`: symbol not found";

      if (sym->file && !sym->file->is_dso)
        sym->ver_idx = v.ver_idx;
    }
  }
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
    if (file == ctx.internal_obj)
      return;

    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      // Match VERSION part of symbol foo@VERSION with version definitions.
      if (!file->has_symver[i - file->first_global])
        continue;

      Symbol<E> *sym = file->symbols[i];
      if (sym->file != file)
        continue;

      const char *name = file->symbol_strtab.data() + file->elf_syms[i].st_name;
      std::string_view ver = strchr(name, '@') + 1;

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

      // If both symbol `foo` and `foo@VERSION` are defined, `foo@VERSION`
      // hides `foo` so that all references to `foo` are resolved to a
      // versioned symbol. Likewise, if `foo@VERSION` and `foo@@VERSION` are
      // defined, the default one takes precedence.
      Symbol<E> *sym2 = get_symbol(ctx, sym->name());
      if (sym2->file == file &&
          !file->has_symver[sym2->sym_idx - file->first_global])
        if (sym2->ver_idx == ctx.default_version ||
            (sym2->ver_idx & ~VERSYM_HIDDEN) == (sym->ver_idx & ~VERSYM_HIDDEN))
          sym2->ver_idx = VER_NDX_LOCAL;
    }
  });
}

template <typename E>
static bool should_export(Context<E> &ctx, Symbol<E> &sym) {
  if (sym.visibility == STV_HIDDEN)
    return false;

  switch (sym.ver_idx) {
  case VER_NDX_UNSPECIFIED:
    if (ctx.arg.dynamic_list_data)
      if (u32 ty = sym.get_type(); ty != STT_FUNC && ty != STT_GNU_IFUNC)
        return true;
    if (ctx.arg.shared)
      return !((ObjectFile<E> *)sym.file)->exclude_libs;
    return ctx.arg.export_dynamic;
  case VER_NDX_LOCAL:
    return false;
  default:
    return true;
  }
};

template <typename E>
static bool is_protected(Context<E> &ctx, Symbol<E> &sym) {
  if (sym.visibility == STV_PROTECTED)
    return true;

  switch (ctx.arg.Bsymbolic) {
  case BSYMBOLIC_ALL:
    return true;
  case BSYMBOLIC_NONE:
    return false;
  case BSYMBOLIC_FUNCTIONS:
    return sym.get_type() == STT_FUNC;
  case BSYMBOLIC_NON_WEAK:
    return !sym.is_weak;
  case BSYMBOLIC_NON_WEAK_FUNCTIONS:
    return !sym.is_weak && sym.get_type() == STT_FUNC;
  default:
    unreachable();
  }
}

template <typename E>
void compute_import_export(Context<E> &ctx) {
  Timer t(ctx, "compute_import_export");

  // If we are creating an executable, we want to export symbols referenced
  // by DSOs unless they are explicitly marked as local by a version script.
  if (!ctx.arg.shared) {
    tbb::parallel_for_each(ctx.dsos, [](SharedFile<E> *file) {
      for (Symbol<E> *sym : file->symbols) {
        if (sym->file && !sym->file->is_dso && sym->visibility != STV_HIDDEN &&
            sym->ver_idx != VER_NDX_LOCAL) {
          std::scoped_lock lock(sym->mu);
          sym->is_exported = true;
        }
      }
    });
  }

  // Export symbols that are not hidden or marked as local.
  // We also want to mark imported symbols as such.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->get_global_syms()) {
      // If we are using a symbol in a DSO, we need to import it.
      if (sym->file && sym->file->is_dso) {
        std::scoped_lock lock(sym->mu);
        sym->is_imported = true;
        continue;
      }

      // If we have a definition of a symbol, we may want to export it.
      if (sym->file == file && should_export(ctx, *sym)) {
        sym->is_exported = true;

        // Exported symbols are marked as imported as well by default
        // for DSOs.
        if (ctx.arg.shared && !is_protected(ctx, *sym))
          sym->is_imported = true;
      }
    }
  });

  // Apply --dynamic-list, --export-dynamic-symbol and
  // --export-dynamic-symbol-list options.
  //
  // The semantics of these options vary depending on whether we are
  // creating an executalbe or a shared object.
  //
  // For executable, matched symbols are exported.
  //
  // For shared objects, matched symbols are imported if it is already
  // exported so that they are interposable. In other words, symbols
  // that did not match will be bound locally within the output file,
  // effectively turning them into protected symbols.
  MultiGlob matcher;
  MultiGlob cpp_matcher;

  auto handle_match = [&](Symbol<E> *sym) {
    if (ctx.arg.shared) {
      if (sym->is_exported)
        sym->is_imported = true;
    } else {
      if (sym->file && !sym->file->is_dso && sym->visibility != STV_HIDDEN)
        sym->is_exported = true;
    }
  };

  for (DynamicPattern &p : ctx.dynamic_list_patterns) {
    if (p.is_cpp) {
      if (!cpp_matcher.add(p.pattern, 1))
        Fatal(ctx) << p.source << ": invalid dynamic list entry: "
                   << p.pattern;
      continue;
    }

    if (p.pattern.find_first_of("*?[") != p.pattern.npos) {
      if (!matcher.add(p.pattern, 1))
        Fatal(ctx) << p.source << ": invalid dynamic list entry: "
                   << p.pattern;
      continue;
    }

    handle_match(get_symbol(ctx, p.pattern));
  }

  if (!matcher.empty() || !cpp_matcher.empty()) {
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      for (Symbol<E> *sym : file->get_global_syms()) {
        if (sym->file != file)
          continue;
        if (ctx.arg.shared && !sym->is_exported)
          continue;

        std::string_view name = sym->name();

        if (matcher.find(name)) {
          handle_match(sym);
        } else if (!cpp_matcher.empty()) {
          if (std::optional<std::string_view> s = demangle_cpp(name))
            name = *s;
          if (cpp_matcher.find(name))
            handle_match(sym);
        }
      }
    });
  }
}

// Compute the "address-taken" bit for each input section.
//
// As a space-saving optimization, we want to merge two read-only objects
// into a single object if their contents are equivalent. That
// optimization is called the Identical Code Folding or ICF.
//
// A catch is that comparing object contents is not enough to determine if
// two objects can be merged safely; we need to take care of pointer
// equivalence.
//
// In C/C++, two pointers are equivalent if and only if they are taken for
// the same object. Merging two objects into a single object can break
// this assumption because two distinctive pointers would become
// equivalent as a result of merging. We can still merge one object with
// another if no pointer to the object was taken in code, because without
// a pointer, comparing its address becomes moot.
//
// In mold, each input section has an "address-taken" bit. If there is a
// pointer-taking reference to the object, it's set to true. At the ICF
// stage, we merge only objects whose addresses were not taken.
//
// For functions, address-taking relocations are separated from
// non-address-taking ones. For example, x86-64 uses R_X86_64_PLT32 for
// direct function calls (e.g., "call foo" to call the function foo) while
// R_X86_64_PC32 or R_X86_64_GOT32 are used for pointer-taking operations.
//
// Unfortunately, for data, we can't distinguish between address-taking
// relocations and non-address-taking ones. LLVM generates an "address
// significance" table in the ".llvm_addrsig" section to mark symbols
// whose addresses are taken in code. If that table is available, we use
// that information in this function. Otherwise, we conservatively assume
// that all data items are address-taken.
template <typename E>
void compute_address_significance(Context<E> &ctx) {
  Timer t(ctx, "compute_address_significance");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    // If .llvm_addrsig is available, use it.
    if (InputSection<E> *sec = file->llvm_addrsig.get()) {
      u8 *p = (u8 *)sec->contents.data();
      u8 *end = p + sec->contents.size();
      while (p != end) {
        Symbol<E> *sym = file->symbols[read_uleb(&p)];
        if (InputSection<E> *isec = sym->get_input_section())
          isec->address_taken = true;
      }
      return;
    }

    // Otherwise, infer address significance.
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (!isec || !isec->is_alive || !(isec->shdr().sh_flags & SHF_ALLOC))
        continue;

      if (!(isec->shdr().sh_flags & SHF_EXECINSTR))
        isec->address_taken = true;

      for (const ElfRel<E> &r : isec->get_rels(ctx))
        if (!is_func_call_rel(r))
          if (Symbol<E> *sym = file->symbols[r.r_sym];
              InputSection<E> *dst = sym->get_input_section())
            if (dst->shdr().sh_flags & SHF_EXECINSTR)
              dst->address_taken = true;
    }
  });

  auto mark = [](Symbol<E> *sym) {
    if (sym)
      if (InputSection<E> *isec = sym->get_input_section())
        isec->address_taken = true;
  };

  // Some symbols' pointer values are leaked to the dynamic section.
  mark(ctx.arg.entry);
  mark(ctx.arg.init);
  mark(ctx.arg.fini);

  // Exported symbols are conservatively considered address-taken.
  if (ctx.dynsym)
    for (Symbol<E> *sym : ctx.dynsym->symbols)
      if (sym && sym->is_exported)
        mark(sym);
}

// We want to sort output chunks in the following order.
//
//   <ELF header>
//   <program header>
//   .interp
//   .note
//   .hash
//   .gnu.hash
//   .dynsym
//   .dynstr
//   .gnu.version
//   .gnu.version_r
//   .rela.dyn
//   .rela.plt
//   <readonly data>
//   <readonly code>
//   <writable tdata>
//   <writable tbss>
//   <writable RELRO data>
//   .got
//   .toc
//   <writable RELRO bss>
//   .relro_padding
//   <writable non-RELRO data>
//   <writable non-RELRO bss>
//   <non-memory-allocated sections>
//   <section header>
//   .gdb_index
//
// .interp and some other linker-synthesized sections are placed at the
// beginning of a file because they are needed by loader. Especially on
// a hard drive with spinning disks, it is important to read these
// sections in a single seek.
//
// .note sections are also placed at the beginning so that they are
// included in a core crash dump even if it's truncated by ulimit. In
// particular, if .note.gnu.build-id is in a truncated core file, you
// can at least identify which executable has crashed.
//
// .gdb_index cannot be constructed before applying relocations to
// other debug sections, so we create it after completing other part
// of the output file and append it to the very end of the file.
//
// A PT_NOTE segment will contain multiple .note sections if exists,
// but there's no way to represent a gap between .note sections.
// Therefore, we sort .note sections by decreasing alignment
// requirement. I believe each .note section size is a multiple of its
// alignment, so by sorting them by alignment, we should be able to
// avoid a gap between .note sections.
//
// .toc is placed right after .got for PPC64. PPC-specific .toc section
// contains data that may be accessed with a 16-bit offset relative to
// %r2. %r2 is set to .got + 32 KiB. Therefore, .toc needs to be within
// [.got, .got + 64 KiB).
//
// Other file layouts are possible, but this layout is chosen to keep
// the number of segments as few as possible.
template <typename E>
void sort_output_sections_regular(Context<E> &ctx) {
  auto get_rank1 = [&](Chunk<E> *chunk) {
    u64 type = chunk->shdr.sh_type;
    u64 flags = chunk->shdr.sh_flags;

    if (chunk == ctx.ehdr)
      return 0;
    if (chunk == ctx.phdr)
      return 1;
    if (chunk == ctx.interp)
      return 2;
    if (type == SHT_NOTE && (flags & SHF_ALLOC))
      return 3;
    if (chunk == ctx.hash)
      return 4;
    if (chunk == ctx.gnu_hash)
      return 5;
    if (chunk == ctx.dynsym)
      return 6;
    if (chunk == ctx.dynstr)
      return 7;
    if (chunk == ctx.versym)
      return 8;
    if (chunk == ctx.verneed)
      return 9;
    if (chunk == ctx.reldyn)
      return 10;
    if (chunk == ctx.relplt)
      return 11;
    if (chunk == ctx.shdr)
      return INT32_MAX - 1;
    if (chunk == ctx.gdb_index)
      return INT32_MAX;

    bool alloc = (flags & SHF_ALLOC);
    bool writable = (flags & SHF_WRITE);
    bool exec = (flags & SHF_EXECINSTR);
    bool tls = (flags & SHF_TLS);
    bool relro = chunk->is_relro;
    bool is_bss = (type == SHT_NOBITS);

    return (1 << 10) | (!alloc << 9) | (writable << 8) | (exec << 7) |
           (!tls << 6) | (!relro << 5) | (is_bss << 4);
  };

  // Ties are broken by additional rules
  auto get_rank2 = [&](Chunk<E> *chunk) -> i64 {
    ElfShdr<E> &shdr = chunk->shdr;
    if (shdr.sh_type == SHT_NOTE)
      return -shdr.sh_addralign;

    if (chunk == ctx.got)
      return 2;
    if (chunk->name == ".toc")
      return 3;
    if (chunk == ctx.relro_padding)
      return INT64_MAX;
    return 0;
  };

  sort(ctx.chunks, [&](Chunk<E> *a, Chunk<E> *b) {
    return std::tuple{get_rank1(a), get_rank2(a), a->name} <
           std::tuple{get_rank1(b), get_rank2(b), b->name};
  });
}

template <typename E>
static std::string_view get_section_order_group(Chunk<E> &chunk) {
  if (chunk.shdr.sh_type == SHT_NOBITS)
    return "BSS";
  if (chunk.shdr.sh_flags & SHF_EXECINSTR)
    return "TEXT";
  if (chunk.shdr.sh_flags & SHF_WRITE)
    return "DATA";
  return "RODATA";
};

// Sort sections according to a --section-order argument.
template <typename E>
void sort_output_sections_by_order(Context<E> &ctx) {
  auto get_rank = [&](Chunk<E> *chunk) -> i64 {
    u64 flags = chunk->shdr.sh_flags;

    if (chunk == ctx.ehdr && !(chunk->shdr.sh_flags & SHF_ALLOC))
      return -2;
    if (chunk == ctx.phdr && !(chunk->shdr.sh_flags & SHF_ALLOC))
      return -1;

    if (chunk == ctx.shdr)
      return INT32_MAX;
    if (!(flags & SHF_ALLOC))
      return INT32_MAX - 1;

    for (i64 i = 0; const SectionOrder &arg : ctx.arg.section_order) {
      if (arg.type == SectionOrder::SECTION && arg.name == chunk->name)
        return i;
      i++;
    }

    std::string_view group = get_section_order_group(*chunk);

    for (i64 i = 0; i < ctx.arg.section_order.size(); i++) {
      SectionOrder arg = ctx.arg.section_order[i];
      if (arg.type == SectionOrder::GROUP && arg.name == group)
        return i;
    }

    Error(ctx) << "--section-order: missing section specification for "
               << chunk->name;
    return 0;
  };

  // It is an error if a section order cannot be determined by a given
  // section order list.
  for (Chunk<E> *chunk : ctx.chunks)
    chunk->sect_order = get_rank(chunk);

  // Sort output sections by --section-order
  sort(ctx.chunks, [](Chunk<E> *a, Chunk<E> *b) {
    return a->sect_order < b->sect_order;
  });
}

template <typename E>
void sort_output_sections(Context<E> &ctx) {
  if (ctx.arg.section_order.empty())
    sort_output_sections_regular(ctx);
  else
    sort_output_sections_by_order(ctx);
}

// This function assigns virtual addresses to output sections. Assigning
// addresses is a bit tricky because we want to pack sections as tightly
// as possible while not violating the constraints imposed by the hardware
// and the OS kernel. Specifically, we need to satisfy the following
// constraints:
//
// - Memory protection (readable, writable and executable) works at page
//   granularity. Therefore, if we want to set different memory attributes
//   to two sections, we need to place them into separate pages.
//
// - The ELF spec requires that a section's file offset is congruent to
//   its virtual address modulo the page size. For example, a section at
//   virtual address 0x401234 on x86-64 (4 KiB, or 0x1000 byte page
//   system) can be at file offset 0x3234 or 0x50234 but not at 0x1000.
//
// We need to insert paddings between sections if we can't satisfy the
// above constraints without them.
//
// We don't want to waste too much memory and disk space for paddings.
// There are a few tricks we can use to minimize paddings as below:
//
// - We want to place sections with the same memory attributes
//   contiguous as possible.
//
// - We can map the same file region to memory more than once. For
//   example, we can write code (with R and X bits) and read-only data
//   (with R bit) adjacent on file and map it twice as the last page of
//   the executable segment and the first page of the read-only data
//   segment. This doesn't save memory but saves disk space.
template <typename E>
static void set_virtual_addresses_regular(Context<E> &ctx) {
  constexpr i64 RELRO = 1LL << 32;

  auto get_flags = [&](Chunk<E> *chunk) {
    i64 flags = to_phdr_flags(ctx, chunk);
    if (chunk->is_relro)
      return flags | RELRO;
    return flags;
  };

  // Assign virtual addresses
  std::vector<Chunk<E> *> &chunks = ctx.chunks;
  u64 addr = ctx.arg.image_base;

  // TLS chunks alignments are special: in addition to having their virtual
  // addresses aligned, they also have to be aligned when the region of
  // tls_begin is copied to a new thread's storage area. In other words, their
  // offset against tls_begin also has to be aligned.
  //
  // A good way to achieve this is to take the largest alignment requirement
  // of all TLS sections and make tls_begin also aligned to that.
  Chunk<E> *first_tls_chunk = nullptr;
  u64 tls_alignment = 1;
  for (Chunk<E> *chunk : chunks) {
    if (chunk->shdr.sh_flags & SHF_TLS) {
      if (!first_tls_chunk)
        first_tls_chunk = chunk;
      tls_alignment = std::max(tls_alignment, (u64)chunk->shdr.sh_addralign);
    }
  }

  auto alignment = [&](Chunk<E> *chunk) {
    return chunk == first_tls_chunk ? tls_alignment : (u64)chunk->shdr.sh_addralign;
  };

  auto is_tbss = [](Chunk<E> *chunk) {
    return (chunk->shdr.sh_type == SHT_NOBITS) && (chunk->shdr.sh_flags & SHF_TLS);
  };

  for (i64 i = 0; i < chunks.size(); i++) {
    if (!(chunks[i]->shdr.sh_flags & SHF_ALLOC))
      continue;

    // .relro_padding is a padding section to extend a PT_GNU_RELRO
    // segment to cover an entire page. Technically, we don't need a
    // .relro_padding section because we can leave a trailing part of a
    // segment an unused space. However, the `strip` command would delete
    // such an unused trailing part and make an executable invalid.
    // So we add a dummy section.
    if (chunks[i] == ctx.relro_padding) {
      chunks[i]->shdr.sh_addr = addr;
      chunks[i]->shdr.sh_size = align_to(addr, ctx.page_size) - addr;
      addr += ctx.page_size;
      continue;
    }

    // Handle --section-start first
    if (auto it = ctx.arg.section_start.find(chunks[i]->name);
        it != ctx.arg.section_start.end()) {
      addr = it->second;
      chunks[i]->shdr.sh_addr = addr;
      addr += chunks[i]->shdr.sh_size;
      continue;
    }

    // Memory protection works at page size granularity. We need to
    // put sections with different memory attributes into different
    // pages. We do it by inserting paddings here.
    if (i > 0 && chunks[i - 1] != ctx.relro_padding) {
      i64 flags1 = get_flags(chunks[i - 1]);
      i64 flags2 = get_flags(chunks[i]);

      if (!ctx.arg.nmagic && flags1 != flags2) {
        switch (ctx.arg.z_separate_code) {
        case SEPARATE_LOADABLE_SEGMENTS:
          addr = align_to(addr, ctx.page_size);
          break;
        case SEPARATE_CODE:
          if ((flags1 & PF_X) != (flags2 & PF_X)) {
            addr = align_to(addr, ctx.page_size);
            break;
          }
          [[fallthrough]];
        case NOSEPARATE_CODE:
          if (addr % ctx.page_size != 0)
            addr += ctx.page_size;
          break;
        default:
          unreachable();
        }
      }
    }

    // TLS BSS sections are laid out so that they overlap with the
    // subsequent non-tbss sections. Overlapping is fine because a STT_TLS
    // segment contains an initialization image for newly-created threads,
    // and no one except the runtime reads its contents. Even the runtime
    // doesn't need a BSS part of a TLS initialization image; it just
    // leaves zero-initialized bytes as-is instead of copying zeros.
    // So no one really read tbss at runtime.
    //
    // We can instead allocate a dedicated virtual address space to tbss,
    // but that would be just a waste of the address and disk space.
    if (is_tbss(chunks[i])) {
      u64 addr2 = addr;
      for (;;) {
        addr2 = align_to(addr2, alignment(chunks[i]));
        chunks[i]->shdr.sh_addr = addr2;
        addr2 += chunks[i]->shdr.sh_size;
        if (i + 2 == chunks.size() || !is_tbss(chunks[i + 1]))
          break;
        i++;
      }
      continue;
    }

    addr = align_to(addr, alignment(chunks[i]));
    chunks[i]->shdr.sh_addr = addr;
    addr += chunks[i]->shdr.sh_size;
  }
}

template <typename E>
static void set_virtual_addresses_by_order(Context<E> &ctx) {
  std::vector<Chunk<E> *> &c = ctx.chunks;
  u64 addr = ctx.arg.image_base;
  i64 i = 0;

  while (i < c.size() && !(c[i]->shdr.sh_flags & SHF_ALLOC))
    i++;

  auto assign_addr = [&] {
    if (i != 0) {
      i64 flags1 = to_phdr_flags(ctx, c[i - 1]);
      i64 flags2 = to_phdr_flags(ctx, c[i]);

      // Memory protection works at page size granularity. We need to
      // put sections with different memory attributes into different
      // pages. We do it by inserting paddings here.
      if (flags1 != flags2) {
        switch (ctx.arg.z_separate_code) {
        case SEPARATE_LOADABLE_SEGMENTS:
          addr = align_to(addr, ctx.page_size);
          break;
        case SEPARATE_CODE:
          if ((flags1 & PF_X) != (flags2 & PF_X))
            addr = align_to(addr, ctx.page_size);
          break;
        default:
          break;
        }
      }
    }

    addr = align_to(addr, c[i]->shdr.sh_addralign);
    c[i]->shdr.sh_addr = addr;
    addr += c[i]->shdr.sh_size;

    do {
      i++;
    } while (i < c.size() && !(c[i]->shdr.sh_flags & SHF_ALLOC));
  };

  for (i64 j = 0; j < ctx.arg.section_order.size(); j++) {
    SectionOrder &ord = ctx.arg.section_order[j];
    switch (ord.type) {
    case SectionOrder::SECTION:
      if (i < c.size() && j == c[i]->sect_order)
        assign_addr();
      break;
    case SectionOrder::GROUP:
      while (i < c.size() && j == c[i]->sect_order)
        assign_addr();
      break;
    case SectionOrder::ADDR:
      addr = ord.value;
      break;
    case SectionOrder::ALIGN:
      addr = align_to(addr, ord.value);
      break;
    case SectionOrder::SYMBOL:
      get_symbol(ctx, ord.name)->value = addr;
      break;
    default:
      unreachable();
    }
  }
}

// Returns the smallest integer N that satisfies N >= val and
// N % align == skew % align.
//
// Section's file offset must be congruent to its virtual address modulo
// the page size. We use this function to satisfy that requirement.
static u64 align_with_skew(u64 val, u64 align, u64 skew) {
  return val + ((skew - val) & (align - 1));
}

// Assign file offsets to output sections.
template <typename E>
static i64 set_file_offsets(Context<E> &ctx) {
  std::vector<Chunk<E> *> &chunks = ctx.chunks;
  u64 fileoff = 0;
  i64 i = 0;

  while (i < chunks.size()) {
    Chunk<E> &first = *chunks[i];

    if (!(first.shdr.sh_flags & SHF_ALLOC)) {
      fileoff = align_to(fileoff, first.shdr.sh_addralign);
      first.shdr.sh_offset = fileoff;
      fileoff += first.shdr.sh_size;
      i++;
      continue;
    }

    if (first.shdr.sh_type == SHT_NOBITS) {
      first.shdr.sh_offset = fileoff;
      i++;
      continue;
    }

    if (first.shdr.sh_addralign > ctx.page_size)
      fileoff = align_to(fileoff, first.shdr.sh_addralign);
    else
      fileoff = align_with_skew(fileoff, ctx.page_size, first.shdr.sh_addr);

    // Assign ALLOC sections contiguous file offsets as long as they
    // are contiguous in memory.
    for (;;) {
      chunks[i]->shdr.sh_offset =
        fileoff + chunks[i]->shdr.sh_addr - first.shdr.sh_addr;
      i++;

      if (i >= chunks.size() ||
          !(chunks[i]->shdr.sh_flags & SHF_ALLOC) ||
          chunks[i]->shdr.sh_type == SHT_NOBITS)
        break;

      // If --start-section is given, addresses may not increase
      // monotonically.
      if (chunks[i]->shdr.sh_addr < first.shdr.sh_addr)
        break;

      i64 gap_size = chunks[i]->shdr.sh_addr - chunks[i - 1]->shdr.sh_addr -
                     chunks[i - 1]->shdr.sh_size;

      // If --start-section is given, there may be a large gap between
      // sections. We don't want to allocate a disk space for a gap if
      // exists.
      if (gap_size >= ctx.page_size)
        break;
    }

    fileoff = chunks[i - 1]->shdr.sh_offset + chunks[i - 1]->shdr.sh_size;

    while (i < chunks.size() &&
           (chunks[i]->shdr.sh_flags & SHF_ALLOC) &&
           chunks[i]->shdr.sh_type == SHT_NOBITS) {
      chunks[i]->shdr.sh_offset = fileoff;
      i++;
    }
  }

  return fileoff;
}

// Remove debug sections from ctx.chunks and save them to ctx.debug_chunks.
// This is for --separate-debug-file.
template <typename E>
void separate_debug_sections(Context<E> &ctx) {
  auto is_debug_section = [&](Chunk<E> *chunk) {
    if (chunk->shdr.sh_flags & SHF_ALLOC)
      return false;
    return chunk == ctx.gdb_index || chunk == ctx.symtab || chunk == ctx.strtab ||
           chunk->name.starts_with(".debug_");
  };

  auto mid = std::stable_partition(ctx.chunks.begin(), ctx.chunks.end(),
                                   is_debug_section);

  ctx.debug_chunks = {ctx.chunks.begin(), mid};
  ctx.chunks.erase(ctx.chunks.begin(), mid);
}

template <typename E>
void compute_section_headers(Context<E> &ctx) {
  // Update sh_size for each chunk.
  for (Chunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  // Remove empty chunks.
  std::erase_if(ctx.chunks, [&](Chunk<E> *chunk) {
    return !chunk->to_osec() && chunk != ctx.gdb_index &&
           chunk->shdr.sh_size == 0;
  });

  // Set section indices.
  i64 shndx = 1;
  for (Chunk<E> *chunk : ctx.chunks)
    if (!chunk->is_header())
      chunk->shndx = shndx++;

  if (ctx.symtab && SHN_LORESERVE <= shndx) {
    SymtabShndxSection<E> *sec = new SymtabShndxSection<E>;
    sec->shndx = shndx++;
    sec->shdr.sh_link = ctx.symtab->shndx;
    ctx.symtab_shndx = sec;
    ctx.chunks.push_back(sec);
    ctx.chunk_pool.emplace_back(sec);
  }

  if (ctx.shdr)
    ctx.shdr->shdr.sh_size = shndx * sizeof(ElfShdr<E>);

  // Some types of section header refer to other section by index.
  // Recompute all section headers to fill such fields with correct values.
  for (Chunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  if (ctx.symtab_shndx) {
    i64 symtab_size = ctx.symtab->shdr.sh_size / sizeof(ElfSym<E>);
    ctx.symtab_shndx->shdr.sh_size = symtab_size * 4;
  }
}

// Assign virtual addresses and file offsets to output sections.
template <typename E>
i64 set_osec_offsets(Context<E> &ctx) {
  Timer t(ctx, "set_osec_offsets");

  for (;;) {
    if (ctx.arg.section_order.empty())
      set_virtual_addresses_regular(ctx);
    else
      set_virtual_addresses_by_order(ctx);

    // Assigning new offsets may change the contents and the length
    // of the program header, so repeat it until converge.
    i64 fileoff = set_file_offsets(ctx);

    if (ctx.phdr) {
      i64 sz = ctx.phdr->shdr.sh_size;
      ctx.phdr->update_shdr(ctx);
      if (sz != ctx.phdr->shdr.sh_size)
        continue;
    }

    return fileoff;
  }
}

template <typename E>
static i64 get_num_irelative_relocs(Context<E> &ctx) {
  i64 n = ctx.num_ifunc_dynrels;
  for (Symbol<E> *sym : ctx.got->got_syms)
    if (sym->is_ifunc())
      n++;
  return n;
}

template <typename E>
static u64 to_paddr(Context<E> &ctx, u64 vaddr) {
  for (ElfPhdr<E> &phdr : ctx.phdr->phdrs)
    if (phdr.p_type == PT_LOAD)
      if (phdr.p_vaddr <= vaddr && vaddr < phdr.p_vaddr + phdr.p_memsz)
        return phdr.p_paddr + (vaddr - phdr.p_vaddr);
  return 0;
}

template <typename E>
void fix_synthetic_symbols(Context<E> &ctx) {
  auto start = [](Symbol<E> *sym, auto &chunk, i64 bias = 0) {
    if (sym && chunk) {
      sym->set_output_section(chunk);
      sym->value = chunk->shdr.sh_addr + bias;
    }
  };

  auto stop = [](Symbol<E> *sym, auto &chunk) {
    if (sym && chunk) {
      sym->set_output_section(chunk);
      sym->value = chunk->shdr.sh_addr + chunk->shdr.sh_size;
    }
  };

  std::vector<Chunk<E> *> sections;
  for (Chunk<E> *chunk : ctx.chunks)
    if (!chunk->is_header() && (chunk->shdr.sh_flags & SHF_ALLOC))
      sections.push_back(chunk);

  auto find = [&](std::string name) -> Chunk<E> * {
    for (Chunk<E> *chunk : sections)
      if (chunk->name == name)
        return chunk;
    return nullptr;
  };

  // __bss_start
  if (Chunk<E> *chunk = find(".bss"))
    start(ctx.__bss_start, chunk);

  if (ctx.ehdr && (ctx.ehdr->shdr.sh_flags & SHF_ALLOC)) {
    ctx.__ehdr_start->set_output_section(sections[0]);
    ctx.__ehdr_start->value = ctx.ehdr->shdr.sh_addr;
    ctx.__executable_start->set_output_section(sections[0]);
    ctx.__executable_start->value = ctx.ehdr->shdr.sh_addr;
  }

  if (ctx.__dso_handle) {
    ctx.__dso_handle->set_output_section(sections[0]);
    ctx.__dso_handle->value = sections[0]->shdr.sh_addr;
  }

  // __rel_iplt_start and __rel_iplt_end. These symbols need to be
  // defined in a statically-linked non-relocatable executable because
  // such executable lacks the .dynamic section and thus there's no way
  // to find ifunc relocations other than these symbols.
  if (ctx.reldyn && ctx.arg.static_ && !ctx.arg.pie) {
    stop(ctx.__rel_iplt_start, ctx.reldyn);
    stop(ctx.__rel_iplt_end, ctx.reldyn);
    ctx.__rel_iplt_start->value -=
      get_num_irelative_relocs(ctx) * sizeof(ElfRel<E>);
  } else {
    // If the symbols are not ncessary, we turn them to absolute
    // symbols at address 0.
    ctx.__rel_iplt_start->origin = 0;
    ctx.__rel_iplt_end->origin = 0;
  }

  // __{init,fini}_array_{start,end}
  for (Chunk<E> *chunk : sections) {
    switch (chunk->shdr.sh_type) {
    case SHT_INIT_ARRAY:
      start(ctx.__init_array_start, chunk);
      stop(ctx.__init_array_end, chunk);
      break;
    case SHT_PREINIT_ARRAY:
      start(ctx.__preinit_array_start, chunk);
      stop(ctx.__preinit_array_end, chunk);
      break;
    case SHT_FINI_ARRAY:
      start(ctx.__fini_array_start, chunk);
      stop(ctx.__fini_array_end, chunk);
      break;
    }
  }

  // _end, _etext, _edata and the like
  for (Chunk<E> *chunk : sections) {
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

  // _GLOBAL_OFFSET_TABLE_. I don't know why, but for the sake of
  // compatibility with existing code, it must be set to the beginning of
  // .got.plt instead of .got only on i386 and x86-64.
  if constexpr (is_x86<E>)
    start(ctx._GLOBAL_OFFSET_TABLE_, ctx.gotplt);
  else
    start(ctx._GLOBAL_OFFSET_TABLE_, ctx.got);

  // _PROCEDURE_LINKAGE_TABLE_. We need this on SPARC.
  start(ctx._PROCEDURE_LINKAGE_TABLE_, ctx.plt);

  // _TLS_MODULE_BASE_. This symbol is used to obtain the address of
  // the TLS block in the TLSDESC model. I believe GCC and Clang don't
  // create a reference to it, but Intel compiler seems to be using
  // this symbol.
  if (ctx._TLS_MODULE_BASE_) {
    ctx._TLS_MODULE_BASE_->set_output_section(sections[0]);
    ctx._TLS_MODULE_BASE_->value = ctx.dtp_addr;
  }

  // __GNU_EH_FRAME_HDR
  start(ctx.__GNU_EH_FRAME_HDR, ctx.eh_frame_hdr);

  // RISC-V's __global_pointer$
  if (ctx.__global_pointer) {
    if (Chunk<E> *chunk = find(".sdata")) {
      start(ctx.__global_pointer, chunk, 0x800);
    } else {
      ctx.__global_pointer->set_output_section(sections[0]);
      ctx.__global_pointer->value = 0;
    }
  }

  // ARM32's __exidx_{start,end}
  if (ctx.__exidx_start) {
    if (Chunk<E> *chunk = find(".ARM.exidx")) {
      start(ctx.__exidx_start, chunk);
      stop(ctx.__exidx_end, chunk);
    }
  }

  // PPC64's ".TOC." symbol.
  if constexpr (is_ppc64<E>) {
    if (Chunk<E> *chunk = find(".got")) {
      start(ctx.extra.TOC, chunk, 0x8000);
    } else if (Chunk<E> *chunk = find(".toc")) {
      start(ctx.extra.TOC, chunk, 0x8000);
    } else {
      ctx.extra.TOC->set_output_section(sections[0]);
      ctx.extra.TOC->value = 0;
    }
  }

  // PPC64's _{save,rest}gpr{0,1}_{14,15,16,...,31} symbols
  if constexpr (is_ppc64v2<E>) {
    i64 offset = 0;
    for (std::pair<std::string_view, u32> p : ppc64_save_restore_insns) {
      std::string_view label = p.first;
      if (!label.empty())
        if (Symbol<E> *sym = get_symbol(ctx, label);
            sym->file == ctx.internal_obj)
          start(sym, ctx.extra.save_restore, offset);
      offset += 4;
    }
  }

  // __start_ and __stop_ symbols
  for (Chunk<E> *chunk : sections) {
    if (std::optional<std::string> name = get_start_stop_name(ctx, *chunk)) {
      start(get_symbol(ctx, save_string(ctx, "__start_" + *name)), chunk);
      stop(get_symbol(ctx, save_string(ctx, "__stop_" + *name)), chunk);

      if (ctx.arg.physical_image_base) {
        u64 paddr = to_paddr(ctx, chunk->shdr.sh_addr);

        Symbol<E> *x = get_symbol(ctx, save_string(ctx, "__phys_start_" + *name));
        x->set_output_section(chunk);
        x->value = paddr;

        Symbol<E> *y = get_symbol(ctx, save_string(ctx, "__phys_stop_" + *name));
        y->set_output_section(chunk);
        y->value = paddr + chunk->shdr.sh_size;
      }
    }
  }

  // --defsym=sym=value symbols
  for (i64 i = 0; i < ctx.arg.defsyms.size(); i++) {
    Symbol<E> *sym = ctx.arg.defsyms[i].first;
    std::variant<Symbol<E> *, u64> val = ctx.arg.defsyms[i].second;

    if (u64 *addr = std::get_if<u64>(&val)) {
      sym->origin = 0;
      sym->value = *addr;
    } else {
      Symbol<E> *sym2 = std::get<Symbol<E> *>(val);
      sym->value = sym2->value;
      sym->origin = sym2->origin;
      sym->visibility = sym2->visibility.load();
    }
  }

  // --section-order symbols
  for (SectionOrder &ord : ctx.arg.section_order)
    if (ord.type == SectionOrder::SYMBOL)
      get_symbol(ctx, ord.name)->set_output_section(sections[0]);
}

template <typename E>
void compress_debug_sections(Context<E> &ctx) {
  Timer t(ctx, "compress_debug_sections");

  tbb::parallel_for((i64)0, (i64)ctx.chunks.size(), [&](i64 i) {
    Chunk<E> &chunk = *ctx.chunks[i];

    if ((chunk.shdr.sh_flags & SHF_ALLOC) || chunk.shdr.sh_size == 0 ||
        !chunk.name.starts_with(".debug"))
      return;

    Chunk<E> *comp = new CompressedSection<E>(ctx, chunk);
    ctx.chunk_pool.emplace_back(comp);
    ctx.chunks[i] = comp;
  });

  if (ctx.shstrtab)
    ctx.shstrtab->update_shdr(ctx);

  if (ctx.ehdr)
    ctx.ehdr->update_shdr(ctx);
  if (ctx.shdr)
    ctx.shdr->update_shdr(ctx);
}

// BLAKE3 is a cryptographic hash function just like SHA256.
// We use it instead of SHA256 because it's faster.
static void blake3_hash(u8 *buf, i64 size, u8 *out) {
  blake3_hasher hasher;
  blake3_hasher_init(&hasher);
  blake3_hasher_update(&hasher, buf, size);
  blake3_hasher_finalize(&hasher, out, BLAKE3_OUT_LEN);
}

template <typename E>
std::vector<std::span<u8>> get_shards(Context<E> &ctx) {
  constexpr i64 shard_size = 4 * 1024 * 1024; // 4 MiB
  std::span<u8> buf = {ctx.buf, (size_t)ctx.output_file->filesize};
  std::vector<std::span<u8>> vec;

  while (!buf.empty()) {
    i64 sz = std::min<i64>(shard_size, buf.size());
    vec.push_back(buf.subspan(0, sz));
    buf = buf.subspan(sz);
  }
  return vec;
}

// Sort dynamic relocations. This is the reason why we do it.
// Quote from https://www.airs.com/blog/archives/186
//
//   The dynamic linker in glibc uses a one element cache when processing
//   relocs: if a relocation refers to the same symbol as the previous
//   relocation, then the dynamic linker reuses the value rather than
//   looking up the symbol again. Thus the dynamic linker gets the best
//   results if the dynamic relocations are sorted so that all dynamic
//   relocations for a given dynamic symbol are adjacent.
//
//   Other than that, the linker sorts together all relative relocations,
//   which don't have symbols. Two relative relocations, or two relocations
//   against the same symbol, are sorted by the address in the output
//   file. This tends to optimize paging and caching when there are two
//   references from the same page.
template <typename E>
void sort_reldyn(Context<E> &ctx) {
  Timer t(ctx, "sort_reldyn");

  ElfRel<E> *begin = (ElfRel<E> *)(ctx.buf + ctx.reldyn->shdr.sh_offset);
  ElfRel<E> *end = begin + ctx.reldyn->shdr.sh_size / sizeof(ElfRel<E>);

  // We group IFUNC relocations at the end of .rel.dyn because we want to
  // apply all the other relocations before running user-supplied IFUNC
  // resolvers.
  auto get_rank = [](u32 r_type) {
    if (r_type == E::R_RELATIVE)
      return 0;
    if constexpr (supports_ifunc<E>)
      if (r_type == E::R_IRELATIVE)
        return 2;
    return 1;
  };

  tbb::parallel_sort(begin, end, [&](const ElfRel<E> &a, const ElfRel<E> &b) {
    return std::tuple(get_rank(a.r_type), a.r_sym, a.r_offset) <
           std::tuple(get_rank(b.r_type), b.r_sym, b.r_offset);
  });
}

template <typename E>
void write_build_id(Context<E> &ctx) {
  Timer t(ctx, "write_build_id");

  switch (ctx.arg.build_id.kind) {
  case BuildId::HEX:
    ctx.buildid->contents = ctx.arg.build_id.value;
    break;
  case BuildId::HASH: {
    std::vector<std::span<u8>> shards = get_shards(ctx);
    std::vector<u8> hashes(shards.size() * BLAKE3_OUT_LEN);

    tbb::parallel_for((i64)0, (i64)shards.size(), [&](i64 i) {
      blake3_hash(shards[i].data(), shards[i].size(),
                  hashes.data() + i * BLAKE3_OUT_LEN);

#ifdef HAVE_MADVISE
      // Make the kernel page out the file contents we've just written
      // so that subsequent close(2) call will become quicker.
      if (i > 0 && ctx.output_file->is_mmapped)
        madvise(shards[i].data(), shards[i].size(), MADV_DONTNEED);
#endif
    });

    u8 buf[BLAKE3_OUT_LEN];
    blake3_hash(hashes.data(), hashes.size(), buf);

    assert(ctx.arg.build_id.size() <= BLAKE3_OUT_LEN);
    ctx.buildid->contents = {buf, buf + ctx.arg.build_id.size()};
    break;
  }
  case BuildId::UUID: {
    u8 buf[16];
    get_random_bytes(buf, 16);

    // Indicate that this is UUIDv4 as defined by RFC4122
    buf[6] = (buf[6] & 0b0000'1111) | 0b0100'0000;
    buf[8] = (buf[8] & 0b0011'1111) | 0b1000'0000;
    ctx.buildid->contents = {buf, buf + 16};
    break;
  }
  default:
    unreachable();
  }

  ctx.buildid->copy_buf(ctx);
}

// A .gnu_debuglink section contains a filename and a CRC32 checksum of a
// debug info file. When we are writing a .gnu_debuglink, we don't know
// its CRC32 checksum because we haven't created a debug info file. So we
// write a dummy value instead.
//
// We can't choose a random value as a dummy value for build
// reproducibility. We also don't want to write a fixed value for all
// files because the CRC checksum is in this section to prevent using
// wrong file on debugging. gdb rejects a debug info file if its CRC
// doesn't match with the one in .gdb_debuglink.
//
// Therefore, we'll try to make our CRC checksum as unique as possible.
// We'll remember that checksum, and after creating a debug info file, add
// a few bytes of garbage at the end of it so that the debug info file's
// CRC checksum becomes the one that we have precomputed.
template <typename E>
void write_gnu_debuglink(Context<E> &ctx) {
  Timer t(ctx, "write_gnu_debuglink");
  u32 crc32;

  if (ctx.buildid) {
    crc32 = compute_crc32(0, ctx.buildid->contents.data(),
                          ctx.buildid->contents.size());
  } else {
    std::vector<std::span<u8>> shards = get_shards(ctx);
    std::vector<U64<E>> hashes(shards.size());

    tbb::parallel_for((i64)0, (i64)shards.size(), [&](i64 i) {
      hashes[i] = hash_string({(char *)shards[i].data(), shards[i].size()});
    });
    crc32 = compute_crc32(0, (u8 *)hashes.data(), hashes.size() * 8);
  }

  ctx.gnu_debuglink->crc32 = crc32;
  ctx.gnu_debuglink->copy_buf(ctx);
}

// Write a separate debug file. This function is called after we finish
// writing to the usual output file.
template <typename E>
void write_separate_debug_file(Context<E> &ctx) {
  Timer t(ctx, "write_separate_debug_file");

  // Open an output file early
  LockingOutputFile<E> *file =
    new LockingOutputFile<E>(ctx, ctx.arg.separate_debug_file, 0666);

  // We want to write to the debug info file in background so that the
  // user doesn't have to wait for it to complete.
  if (ctx.arg.detach)
    notify_parent();

  // A debug info file contains all sections as the original file, though
  // most of them can be empty as if they were bss sections. We convert
  // real sections into dummy sections here.
  for (i64 i = 0; i < ctx.chunks.size(); i++) {
    Chunk<E> *chunk = ctx.chunks[i];
    if (chunk != ctx.ehdr && chunk != ctx.shdr && chunk != ctx.shstrtab &&
        chunk->shdr.sh_type != SHT_NOTE) {
      Chunk<E> *sec = new OutputSection<E>(chunk->name, SHT_NULL);
      sec->shdr = chunk->shdr;
      sec->shdr.sh_type = SHT_NOBITS;

      ctx.chunks[i] = sec;
      ctx.chunk_pool.emplace_back(sec);
    }
  }

  // Restore debug info sections that had been set aside while we were
  // creating the main file.
  tbb::parallel_for_each(ctx.debug_chunks, [&](Chunk<E> *chunk) {
    chunk->compute_section_size(ctx);
  });

  append(ctx.chunks, ctx.debug_chunks);

  // Handle --compress-debug-info
  if (ctx.arg.compress_debug_sections != COMPRESS_NONE)
    compress_debug_sections(ctx);

  // Write to the debug info file as if it were a regular output file.
  compute_section_headers(ctx);
  file->resize(ctx, set_osec_offsets(ctx));

  ctx.output_file.reset(file);
  ctx.buf = ctx.output_file->buf;

  copy_chunks(ctx);

  if (ctx.gdb_index)
    write_gdb_index(ctx);

  // Reverse-compute a CRC32 value so that the CRC32 checksum embedded to
  // the .gnu_debuglink section in the main executable matches with the
  // debug info file's CRC32 checksum.
  u32 crc = compute_crc32(0, ctx.buf, ctx.output_file->filesize);

  std::vector<u8> &buf2 = ctx.output_file->buf2;
  if (!buf2.empty())
    crc = compute_crc32(crc, buf2.data(), buf2.size());

  std::vector<u8> trailer = crc32_solve(crc, ctx.gnu_debuglink->crc32);
  append(ctx.output_file->buf2, trailer);
  ctx.output_file->close(ctx);
}

// Write Makefile-style dependency rules to a file specified by
// --dependency-file. This is analogous to the compiler's -M flag.
template <typename E>
void write_dependency_file(Context<E> &ctx) {
  std::vector<std::string> deps;
  std::unordered_set<std::string> seen;

  for (std::unique_ptr<MappedFile> &mf : ctx.mf_pool)
    if (mf->is_dependency && !mf->parent)
      if (std::string path = path_clean(mf->name); seen.insert(path).second)
        deps.push_back(path);

  std::ofstream out;
  out.open(ctx.arg.dependency_file);
  if (out.fail())
    Fatal(ctx) << "--dependency-file: cannot open " << ctx.arg.dependency_file
               << ": " << errno_string();

  out << ctx.arg.output << ":";
  for (std::string &s : deps)
    out << " " << s;
  out << "\n";

  for (std::string &s : deps)
    out << "\n" << s << ":\n";
  out.close();
}

template <typename E>
void show_stats(Context<E> &ctx) {
  for (ObjectFile<E> *obj : ctx.objs) {
    static Counter defined("defined_syms");
    defined += obj->first_global - 1;

    static Counter undefined("undefined_syms");
    undefined += obj->symbols.size() - obj->first_global;

    for (std::unique_ptr<InputSection<E>> &sec : obj->sections) {
      if (!sec || !sec->is_alive)
        continue;

      static Counter alloc("reloc_alloc");
      static Counter nonalloc("reloc_nonalloc");

      if (sec->shdr().sh_flags & SHF_ALLOC)
        alloc += sec->get_rels(ctx).size();
      else
        nonalloc += sec->get_rels(ctx).size();
    }

    static Counter comdats("comdats");
    comdats += obj->comdat_groups.size();

    static Counter removed_comdats("removed_comdat_mem");
    for (ComdatGroupRef<E> &ref : obj->comdat_groups)
      if (ref.group->owner != obj->priority)
        removed_comdats += ref.members.size();

    static Counter num_cies("num_cies");
    num_cies += obj->cies.size();

    static Counter num_unique_cies("num_unique_cies");
    for (CieRecord<E> &cie : obj->cies)
      if (cie.is_leader)
        num_unique_cies++;

    static Counter num_fdes("num_fdes");
    num_fdes +=  obj->fdes.size();
  }

  static Counter num_bytes("total_input_bytes");
  for (std::unique_ptr<MappedFile> &mf : ctx.mf_pool)
    num_bytes += mf->size;

  static Counter num_input_sections("input_sections");
  for (ObjectFile<E> *file : ctx.objs)
    num_input_sections += file->sections.size();

  static Counter num_output_chunks("output_chunks", ctx.chunks.size());
  static Counter num_objs("num_objs", ctx.objs.size());
  static Counter num_dsos("num_dsos", ctx.dsos.size());

  using Entry = typename ConcurrentMap<SectionFragment<E>>::Entry;

  static Counter merged_strings("merged_strings");
  for (std::unique_ptr<MergedSection<E>> &sec : ctx.merged_sections)
    for (Entry &ent : std::span(sec->map.entries, sec->map.nbuckets))
      if (ent.key)
        merged_strings++;

  if constexpr (needs_thunk<E>) {
    static Counter thunk_bytes("thunk_bytes");
    for (Chunk<E> *chunk : ctx.chunks)
      if (OutputSection<E> *osec = chunk->to_osec())
        for (std::unique_ptr<Thunk<E>> &thunk : osec->thunks)
          thunk_bytes += thunk->size();
  }

  if constexpr (is_riscv<E> || is_loongarch<E>) {
    static Counter num_rels("shrunk_relocs");
    for (Chunk<E> *chunk : ctx.chunks)
      if (OutputSection<E> *osec = chunk->to_osec())
        if (osec->shdr.sh_flags & SHF_EXECINSTR)
          for (InputSection<E> *isec : osec->members)
            num_rels += isec->extra.r_deltas.size();
  }

  Counter::print();

  for (std::unique_ptr<MergedSection<E>> &sec : ctx.merged_sections)
    sec->print_stats(ctx);
}

using E = MOLD_TARGET;

template int redo_main(Context<E> &, int, char **);
template void create_internal_file(Context<E> &);
template void apply_exclude_libs(Context<E> &);
template void create_synthetic_sections(Context<E> &);
template void resolve_symbols(Context<E> &);
template void do_lto(Context<E> &);
template void parse_eh_frame_sections(Context<E> &);
template void create_merged_sections(Context<E> &);
template void convert_common_symbols(Context<E> &);
template void create_output_sections(Context<E> &);
template void add_synthetic_symbols(Context<E> &);
template void check_cet_errors(Context<E> &);
template void apply_section_align(Context<E> &);
template void print_dependencies(Context<E> &);
template void write_repro_file(Context<E> &);
template void check_duplicate_symbols(Context<E> &);
template void check_shlib_undefined(Context<E> &);
template void check_symbol_types(Context<E> &);
template void sort_init_fini(Context<E> &);
template void sort_ctor_dtor(Context<E> &);
template void fixup_ctors_in_init_array(Context<E> &);
template void shuffle_sections(Context<E> &);
template void compute_section_sizes(Context<E> &);
template void sort_output_sections(Context<E> &);
template void claim_unresolved_symbols(Context<E> &);
template void compute_imported_symbol_weakness(Context<E> &);
template void scan_relocations(Context<E> &);
template void report_undef_errors(Context<E> &);
template void create_reloc_sections(Context<E> &);
template void copy_chunks(Context<E> &);
template void construct_relr(Context<E> &);
template void sort_dynsyms(Context<E> &);
template void create_output_symtab(Context<E> &);
template void apply_version_script(Context<E> &);
template void parse_symbol_version(Context<E> &);
template void compute_import_export(Context<E> &);
template void compute_address_significance(Context<E> &);
template void separate_debug_sections(Context<E> &);
template void compute_section_headers(Context<E> &);
template i64 set_osec_offsets(Context<E> &);
template void fix_synthetic_symbols(Context<E> &);
template void compress_debug_sections(Context<E> &);
template void sort_reldyn(Context<E> &);
template void write_build_id(Context<E> &);
template void write_gnu_debuglink(Context<E> &);
template void write_separate_debug_file(Context<E> &);
template void write_dependency_file(Context<E> &);
template void show_stats(Context<E> &);

} // namespace mold
