#include "mold.h"
#include "../archive-file.h"
#include "../cmdline.h"
#include "../output-file.h"
#include "../sha.h"

#include <cstdlib>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/global_control.h>
#include <tbb/parallel_for_each.h>

namespace mold::macho {

static std::pair<std::string_view, std::string_view>
split_string(std::string_view str, char sep) {
  size_t pos = str.find(sep);
  if (pos == str.npos)
    return {str, ""};
  return {str.substr(0, pos), str.substr(pos + 1)};
}

template <typename E>
static bool has_lto_obj(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    if (file->lto_module)
      return true;
  return false;
}

template <typename E>
static void resolve_symbols(Context<E> &ctx) {
  Timer t(ctx, "resolve_symbols");

  auto for_each_file = [&](std::function<void(InputFile<E> *)> fn) {
    tbb::parallel_for_each(ctx.objs, fn);
    tbb::parallel_for_each(ctx.dylibs, fn);
  };

  for_each_file([&](InputFile<E> *file) { file->resolve_symbols(ctx); });

  std::vector<ObjectFile<E> *> live_objs;
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_alive)
      live_objs.push_back(file);

  for (i64 i = 0; i < live_objs.size(); i++) {
    live_objs[i]->mark_live_objects(ctx, [&](ObjectFile<E> *file) {
      live_objs.push_back(file);
    });
  }

  // Remove symbols of eliminated files.
  for_each_file([&](InputFile<E> *file) {
    if (!file->is_alive)
      file->clear_symbols();
  });

  std::erase_if(ctx.objs, [](InputFile<E> *file) { return !file->is_alive; });
  std::erase_if(ctx.dylibs, [](InputFile<E> *file) { return !file->is_alive; });

  for_each_file([&](InputFile<E> *file) { file->resolve_symbols(ctx); });

  if (has_lto_obj(ctx))
    do_lto(ctx);
}

template <typename E>
static void create_internal_file(Context<E> &ctx) {
  ObjectFile<E> *obj = new ObjectFile<E>;
  obj->is_alive = true;
  obj->mach_syms = obj->mach_syms2;
  ctx.obj_pool.emplace_back(obj);
  ctx.objs.push_back(obj);

  auto add = [&](std::string_view name) {
    Symbol<E> *sym = get_symbol(ctx, name);
    sym->file = obj;
    obj->syms.push_back(sym);
    return sym;
  };

  add("__dyld_private");

  switch (ctx.output_type) {
  case MH_EXECUTE: {
    Symbol<E> *sym = add("__mh_execute_header");
    sym->scope = SCOPE_EXTERN;
    sym->referenced_dynamically = true;
    sym->value = ctx.arg.pagezero_size;
    break;
  }
  case MH_DYLIB:
    add("__mh_dylib_header");
    break;
  case MH_BUNDLE:
    add("__mh_bundle_header");
    break;
  default:
    unreachable();
  }

  add("___dso_handle");
}

// Remove unreferenced subsections to eliminate code and data
// referenced by duplicated weakdef symbols.
template <typename E>
static void remove_unreferenced_subsections(Context<E> &ctx) {
  Timer t(ctx, "remove_unreferenced_subsections");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (i64 i = 0; i < file->mach_syms.size(); i++) {
      MachSym &msym = file->mach_syms[i];
      Symbol<E> &sym = *file->syms[i];
      if (sym.file != file && (msym.type == N_SECT) && (msym.desc & N_WEAK_DEF))
        file->sym_to_subsec[i]->is_alive = false;
    }
  });

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::erase_if(file->subsections, [](Subsection<E> *subsec) {
      return !subsec->is_alive;
    });
  });
}

template <typename E>
static void claim_unresolved_symbols(Context<E> &ctx) {
  Timer t(ctx, "claim_unresolved_symbols");

  for (std::string_view name : ctx.arg.U)
    if (Symbol<E> *sym = get_symbol(ctx, name); !sym->file)
      sym->is_imported = true;

  for (ObjectFile<E> *file : ctx.objs) {
    for (i64 i = 0; i < file->mach_syms.size(); i++) {
      MachSym &msym = file->mach_syms[i];
      if (!msym.is_extern || !msym.is_undef())
        continue;

      Symbol<E> &sym = *file->syms[i];
      std::scoped_lock lock(sym.mu);

      if (sym.is_imported) {
        if (!sym.file ||
            (!sym.file->is_dylib && file->priority < sym.file->priority)) {
          sym.file = file;
          sym.scope = SCOPE_PRIVATE_EXTERN;
          sym.is_imported = true;
          sym.subsec = nullptr;
          sym.value = 0;
          sym.is_common = false;
        }
      }
    }
  }
}

template <typename E>
static void merge_cstring_sections(Context<E> &ctx) {
  Timer t(ctx, "merge_cstring_sections");

  // Insert all strings into a hash table to merge them.
  tbb::concurrent_hash_map<std::string_view, Subsection<E> *, HashCmp> map;

  for (ObjectFile<E> *file : ctx.objs) {
    tbb::parallel_for_each(file->subsections, [&](Subsection<E> *subsec) {
      if (&subsec->isec.osec != ctx.cstring)
        return;

      typename decltype(map)::accessor acc;

      if (!map.insert(acc, {subsec->get_contents(), subsec})) {
        Subsection<E> *existing = acc->second;
        update_maximum(existing->p2align, subsec->p2align.load());
        subsec->is_coalesced = true;
        subsec->replacer = existing;

        static Counter counter("num_merged_strings");
        counter++;
      }
    });
  }

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections)
      if (isec)
        for (Relocation<E> &r : isec->rels)
          if (r.subsec && r.subsec->is_coalesced)
            r.subsec = r.subsec->replacer;
  });

  auto replace = [&](InputFile<E> *file) {
    for (Symbol<E> *sym : file->syms)
      if (sym->subsec && sym->subsec->is_coalesced)
        sym->subsec = sym->subsec->replacer;
  };

  tbb::parallel_for_each(ctx.objs, replace);
  tbb::parallel_for_each(ctx.dylibs, replace);

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::erase_if(file->subsections, [](Subsection<E> *subsec) {
      return subsec->is_coalesced;
    });
  });
}

template <typename E>
static void create_synthetic_chunks(Context<E> &ctx) {
  // Add subsections to output sections.
  for (ObjectFile<E> *file : ctx.objs)
    for (Subsection<E> *subsec : file->subsections)
      subsec->isec.osec.add_subsec(subsec);

  // Add output sections to segments.
  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk != ctx.data && chunk->is_output_section &&
        ((OutputSection<E> *)chunk)->members.empty())
      continue;

    OutputSegment<E> *seg =
      OutputSegment<E>::get_instance(ctx, chunk->hdr.get_segname());
    seg->chunks.push_back(chunk);
  }

  // Sort segments.
  sort(ctx.segments, [](const std::unique_ptr<OutputSegment<E>> &a,
                        const std::unique_ptr<OutputSegment<E>> &b) {
    // We want to sort output segments in the following order:
    // __TEXT, __DATA_CONST, __DATA, <other segments>, __LINKEDIT
    auto get_rank = [](std::string_view name) {
      if (name == "__TEXT")
        return 0;
      if (name == "__DATA_CONST")
        return 1;
      if (name == "__DATA")
        return 2;
      if (name == "__LINKEDIT")
        return 4;
      return 3;
    };

    std::string_view x = a->cmd.get_segname();
    std::string_view y = b->cmd.get_segname();
    return std::tuple{get_rank(x), x} < std::tuple{get_rank(y), y};
  });

  // Sort output sections.
  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments) {
    sort(seg->chunks, [&](const Chunk<E> *a, const Chunk<E> *b) {
      assert(a->hdr.get_segname() == b->hdr.get_segname());

      // __mach_header must be at the beginning of a file.
      if (a == &ctx.mach_hdr || b == &ctx.mach_hdr)
        return a == &ctx.mach_hdr;

      // BSS should be at the end of a segment.
      auto is_bss = [](const Chunk<E> *x) {
        return x->hdr.type == S_ZEROFILL ||
               x->hdr.type == S_THREAD_LOCAL_ZEROFILL;
      };

      if (is_bss(a) != is_bss(b))
        return !is_bss(a);

      // Place executable sections before non-executable sections
      // so that executable sections are contiguous.
      auto is_exec = [](const Chunk<E> *x) -> bool {
        return x->hdr.attr & S_ATTR_SOME_INSTRUCTIONS;
      };

      if (is_exec(a) != is_exec(b))
        return is_exec(a);

      // We want to place __ind_sym_tab, __string_table and
      // __code_signature at the end of a file. Ties are broken by
      // section name for build reproducibility.
      auto get_rank = [&](const Chunk<E> *x) {
        if (x == &ctx.indir_symtab)
          return 1;
        if (x == &ctx.strtab)
          return 2;
        if (x == ctx.code_sig.get())
          return 3;
        return 0;
      };

      return std::tuple{get_rank(a), a->hdr.get_sectname()} <
             std::tuple{get_rank(b), b->hdr.get_sectname()};
    });
  }
}

template <typename E>
static void scan_unwind_info(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    for (Subsection<E> *subsec : file->subsections)
      for (UnwindRecord<E> &rec : subsec->get_unwind_records())
        if (rec.personality)
          rec.personality->flags |= NEEDS_GOT;
}

template <typename E>
static void export_symbols(Context<E> &ctx) {
  Timer t(ctx, "export_symbols");

  ctx.got.add(ctx, get_symbol(ctx, "dyld_stub_binder"));

  for (ObjectFile<E> *file : ctx.objs) {
    for (Symbol<E> *sym : file->syms) {
      if (sym && sym->file == file) {
        if (sym->flags & NEEDS_GOT)
          ctx.got.add(ctx, sym);
        if (sym->flags & NEEDS_THREAD_PTR)
          ctx.thread_ptrs.add(ctx, sym);
      }
    }
  }

  for (DylibFile<E> *file : ctx.dylibs) {
    for (Symbol<E> *sym : file->syms) {
      if (sym && sym->file == file) {
        if (sym->flags & NEEDS_STUB)
          ctx.stubs.add(ctx, sym);
        if (sym->flags & NEEDS_GOT)
          ctx.got.add(ctx, sym);
        if (sym->flags & NEEDS_THREAD_PTR)
          ctx.thread_ptrs.add(ctx, sym);
      }
    }
  }
}

template <typename E>
static i64 assign_offsets(Context<E> &ctx) {
  Timer t(ctx, "assign_offsets");

  i64 sect_idx = 1;
  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (!chunk->is_hidden)
        chunk->sect_idx = sect_idx++;

  i64 fileoff = 0;
  i64 vmaddr = ctx.arg.pagezero_size;

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments) {
    seg->set_offset(ctx, fileoff, vmaddr);
    fileoff += seg->cmd.filesize;
    vmaddr += seg->cmd.vmsize;
  }
  return fileoff;
}

// An address of a symbol of type S_THREAD_LOCAL_VARIABLES is computed
// as a relative address to the beginning of the first thread-local
// section. This function finds the beginnning address.
template <typename E>
static u64 get_tls_begin(Context<E> &ctx) {
  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (chunk->hdr.type == S_THREAD_LOCAL_REGULAR ||
          chunk->hdr.type == S_THREAD_LOCAL_ZEROFILL)
        return chunk->hdr.addr;
  return 0;
}

template <typename E>
static void fix_synthetic_symbol_values(Context<E> &ctx) {
  get_symbol(ctx, "__dyld_private")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "__mh_dylib_header")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "__mh_bundle_header")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "___dso_handle")->value = ctx.data->hdr.addr;
}

template <typename E>
static void copy_sections_to_output_file(Context<E> &ctx) {
  Timer t(ctx, "copy_sections_to_output_file");

  tbb::parallel_for_each(ctx.segments,
                         [&](std::unique_ptr<OutputSegment<E>> &seg) {
    Timer t2(ctx, std::string(seg->cmd.get_segname()), &t);

    // Fill text segment paddings with NOPs
    if (seg->cmd.get_segname() == "__TEXT")
      memset(ctx.buf + seg->cmd.fileoff, 0x90, seg->cmd.filesize);

    tbb::parallel_for_each(seg->chunks, [&](Chunk<E> *sec) {
      if (sec->hdr.type != S_ZEROFILL) {
        Timer t3(ctx, std::string(sec->hdr.get_sectname()), &t2);
        sec->copy_buf(ctx);
      }
    });
  });
}

template <typename E>
static void compute_uuid(Context<E> &ctx) {
  Timer t(ctx, "copy_sections_to_output_file");

  u8 buf[SHA256_SIZE];
  SHA256(ctx.buf, ctx.output_file->filesize, buf);
  memcpy(ctx.uuid, buf, 16);
  ctx.mach_hdr.copy_buf(ctx);
}

template <typename E>
MappedFile<Context<E>> *find_framework(Context<E> &ctx, std::string name) {
  std::string suffix;
  std::tie(name, suffix) = split_string(name, ',');

  for (std::string path : ctx.arg.framework_paths) {
    path = get_realpath(path + "/" + name + ".framework/" + name);

    if (!suffix.empty())
      if (auto *mf = MappedFile<Context<E>>::open(ctx, path + suffix))
        return mf;

    if (auto *mf = MappedFile<Context<E>>::open(ctx, path + ".tbd"))
      return mf;

    if (auto *mf = MappedFile<Context<E>>::open(ctx, path))
      return mf;
  }
  Fatal(ctx) << "-framework not found: " << name;
}

template <typename E>
MappedFile<Context<E>> *find_library(Context<E> &ctx, std::string name) {
  auto search = [&](std::vector<std::string> extn) -> MappedFile<Context<E>> * {
    for (std::string dir : ctx.arg.library_paths) {
      for (std::string e : extn) {
        std::string path = dir + "/lib" + name + e;
        if (MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path))
          return mf;
      }
    }
    return nullptr;
  };

  // -search_paths_first
  if (ctx.arg.search_paths_first)
    return search({".tbd", ".dylib", ".a"});

  // -search_dylibs_first
  if (MappedFile<Context<E>> *mf = search({".tbd", ".dylib"}))
    return mf;
  return search({".a"});
}

template <typename E>
static MappedFile<Context<E>> *
strip_universal_header(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  FatHeader &hdr = *(FatHeader *)mf->data;
  assert(hdr.magic == FAT_MAGIC);

  FatArch *arch = (FatArch *)(mf->data + sizeof(hdr));
  for (i64 i = 0; i < hdr.nfat_arch; i++)
    if (arch[i].cputype == E::cputype)
      return mf->slice(ctx, mf->name, arch[i].offset, arch[i].size);
  Fatal(ctx) << mf->name << ": fat file contains no matching file";
}

template <typename E>
static void read_file(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  if (get_file_type(mf) == FileType::MACH_UNIVERSAL)
    mf = strip_universal_header(ctx, mf);

  switch (get_file_type(mf)) {
  case FileType::TAPI:
  case FileType::MACH_DYLIB:
    ctx.dylibs.push_back(DylibFile<E>::create(ctx, mf));
    break;
  case FileType::MACH_OBJ:
  case FileType::LLVM_BITCODE:
    ctx.objs.push_back(ObjectFile<E>::create(ctx, mf, ""));
    break;
  case FileType::AR:
    for (MappedFile<Context<E>> *child : read_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::MACH_OBJ)
        ctx.objs.push_back(ObjectFile<E>::create(ctx, child, mf->name));
    break;
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
    break;
  }
}

template <typename E>
static std::vector<std::string>
read_filelist(Context<E> &ctx, std::string arg) {
  std::string path;
  std::string dir;

  if (size_t pos = arg.find(','); pos != arg.npos) {
    path = arg.substr(0, pos);
    dir = arg.substr(pos + 1) + "/";
  } else {
    path = arg;
  }

  MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path);
  if (!mf)
    Fatal(ctx) << "-filepath: cannot open " << path;

  std::vector<std::string> vec;
  for (std::string_view str = mf->get_contents(); !str.empty();) {
    std::string_view path;
    std::tie(path, str) = split_string(str, '\n');
    vec.push_back(path_clean(dir + std::string(path)));
  }
  return vec;
}

template <typename E>
static void read_input_files(Context<E> &ctx, std::span<std::string> args) {
  Timer t(ctx, "read_input_files");

  auto must_find_library = [&](std::string arg) {
    MappedFile<Context<E>> *mf = find_library(ctx, arg);
    if (!mf)
      Fatal(ctx) << "library not found: -l" << arg;
    return mf;
  };

  while (!args.empty()) {
    const std::string &arg = args[0];
    args = args.subspan(1);

    if (arg == "-all_load") {
      ctx.all_load = true;
    } else if (arg == "-noall_load") {
      ctx.all_load = false;
    } else if (arg == "-filelist") {
      for (std::string &path : read_filelist(ctx, args[0])) {
        MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path);
        if (!mf)
          Fatal(ctx) << "-filepath " << args[0] << ": cannot open file: " << path;
        read_file(ctx, mf);
      }
      args = args.subspan(1);
    } else if (arg == "-force_load") {
      bool orig = ctx.all_load;
      ctx.all_load = true;
      read_file(ctx, MappedFile<Context<E>>::must_open(ctx, args[0]));
      ctx.all_load = orig;
      args = args.subspan(1);
    } else if (arg == "-framework") {
      read_file(ctx, find_framework(ctx, args[0]));
      args = args.subspan(1);
    } else if (arg == "-needed_framework") {
      ctx.needed_l = true;
      read_file(ctx, find_framework(ctx, args[0]));
      ctx.needed_l = false;
      args = args.subspan(1);
    } else if (arg == "-l") {
      read_file(ctx, must_find_library(args[0]));
      args = args.subspan(1);
    } else if (arg == "-needed-l") {
      ctx.needed_l = true;
      read_file(ctx, must_find_library(args[0]));
      ctx.needed_l = false;
      args = args.subspan(1);
    } else if (arg == "-hidden-l") {
      ctx.hidden_l = true;
      read_file(ctx, must_find_library(args[0]));
      ctx.hidden_l = false;
      args = args.subspan(1);
    } else {
      read_file(ctx, MappedFile<Context<E>>::must_open(ctx, arg));
    }
  }

  for (ObjectFile<E> *file : ctx.objs) {
    std::vector<std::string> opts = file->get_linker_options(ctx);

    for (i64 j = 0; j < opts.size();) {
      if (opts[j] == "-framework") {
        read_file(ctx, find_framework(ctx, opts[j + 1]));
        j += 2;
      } else if (opts[j].starts_with("-l")) {
        read_file(ctx, must_find_library(opts[j].substr(2)));
        j++;
      } else {
        Fatal(ctx) << *file << ": unknown LC_LINKER_OPTION command: " << opts[j];
      }
    }
  }

  if (ctx.objs.empty())
    Fatal(ctx) << "no input files";

  for (ObjectFile<E> *file : ctx.objs)
    file->priority = ctx.file_priority++;
  for (DylibFile<E> *dylib : ctx.dylibs)
    dylib->priority = ctx.file_priority++;

  for (i64 i = 0; i < ctx.dylibs.size(); i++)
    ctx.dylibs[i]->dylib_idx = i + 1;
}

template <typename E>
static void parse_input_files(Context<E> &ctx) {
  Timer t(ctx, "parse_input_files");

  auto parse = [&](InputFile<E> *file) { file->parse(ctx); };
  tbb::parallel_for_each(ctx.objs, parse);
  tbb::parallel_for_each(ctx.dylibs, parse);
}

template <typename E>
static void print_stats(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs) {
    static Counter subsections("num_subsections");
    subsections += file->subsections.size();

    static Counter syms("num_syms");
    syms += file->syms.size();

    static Counter rels("num_rels");
    for (std::unique_ptr<InputSection<E>> &isec : file->sections)
      if (isec)
        rels += isec->rels.size();
  }

  Counter::print();
}

template <typename E>
static int do_main(int argc, char **argv) {
  Context<E> ctx;

  if (argc > 1 && std::string_view(argv[1]) == "-dump") {
    if (argc != 3)
      Fatal(ctx) << "usage: ld64.mold -dump <executable-name>\n";
    dump_file(argv[2]);
    exit(0);
  }

  for (i64 i = 0; i < argc; i++)
    ctx.cmdline_args.push_back(argv[i]);

  std::vector<std::string> file_args = parse_nonpositional_args(ctx);

  if (ctx.arg.arch != E::cputype) {
#if !defined(MOLD_DEBUG_X86_64_ONLY) && !defined(MOLD_DEBUG_ARM64_ONLY)
    switch (ctx.arg.arch) {
    case CPU_TYPE_X86_64:
      return do_main<X86_64>(argc, argv);
    case CPU_TYPE_ARM64:
      return do_main<X86_64>(argc, argv);
    }
#endif
    Fatal(ctx) << "unknown cputype: " << ctx.arg.arch;
  }

  Timer t(ctx, "all");

  // Handle -sectcreate
  for (SectCreateOption arg : ctx.arg.sectcreate) {
    MappedFile<Context<E>> *mf =
      MappedFile<Context<E>>::must_open(ctx, std::string(arg.filename));
    SectCreateSection<E> *sec =
      new SectCreateSection<E>(ctx, arg.segname, arg.sectname, mf->get_contents());
    ctx.chunk_pool.emplace_back(sec);
  }

  if (ctx.arg.adhoc_codesign)
    ctx.code_sig.reset(new CodeSignatureSection<E>(ctx));

  read_input_files(ctx, file_args);
  parse_input_files(ctx);

  if (ctx.arg.ObjC)
    for (ObjectFile<E> *file : ctx.objs)
      if (!file->archive_name.empty() && file->is_objc_object(ctx))
        file->is_alive = true;

  resolve_symbols(ctx);
  remove_unreferenced_subsections(ctx);

  if (ctx.output_type == MH_EXECUTE && !ctx.arg.entry->file)
    Error(ctx) << "undefined entry point symbol: " << *ctx.arg.entry;

  create_internal_file(ctx);

  if (ctx.arg.trace) {
    for (ObjectFile<E> *file : ctx.objs)
      SyncOut(ctx) << *file;
    for (DylibFile<E> *file : ctx.dylibs)
      SyncOut(ctx) << *file;
  }

  for (ObjectFile<E> *file : ctx.objs)
    file->convert_common_symbols(ctx);

  claim_unresolved_symbols(ctx);

  merge_cstring_sections(ctx);

  if (ctx.arg.dead_strip)
    dead_strip(ctx);

  create_synthetic_chunks(ctx);

  for (ObjectFile<E> *file : ctx.objs)
    file->check_duplicate_symbols(ctx);

  bool has_pagezero_seg = ctx.arg.pagezero_size;
  for (i64 i = 0; i < ctx.segments.size(); i++)
    ctx.segments[i]->seg_idx = (has_pagezero_seg ? i + 1 : i);

  for (ObjectFile<E> *file : ctx.objs)
    for (Subsection<E> *subsec : file->subsections)
      subsec->scan_relocations(ctx);

  scan_unwind_info(ctx);
  export_symbols(ctx);

  i64 output_size = assign_offsets(ctx);
  ctx.tls_begin = get_tls_begin(ctx);
  fix_synthetic_symbol_values(ctx);

  ctx.output_file =
    OutputFile<Context<E>>::open(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  copy_sections_to_output_file(ctx);

  if (ctx.code_sig)
    ctx.code_sig->write_signature(ctx);
  else if (ctx.arg.uuid == UUID_HASH)
    compute_uuid(ctx);

  ctx.output_file->close(ctx);
  ctx.checkpoint();
  t.stop();

  if (ctx.arg.perf)
    print_timer_records(ctx.timer_records);

  if (ctx.arg.stats)
    print_stats(ctx);

  if (!ctx.arg.map.empty())
    print_map(ctx);

  if (ctx.arg.quick_exit) {
    std::cout << std::flush;
    std::cerr << std::flush;
    _exit(0);
  }

  return 0;
}

int main(int argc, char **argv) {
  if (!getenv("MOLD_SUPPRESS_MACHO_WARNING")) {
    std::cerr <<
R"(********************************************************************************
mold for macOS is pre-alpha. Do not use unless you know what you are doing.
Do not report bugs because it's too early to manage missing features as bugs.
********************************************************************************
)";
  }

#ifdef MOLD_DEBUG_X86_64_ONLY
  return do_main<X86_64>(argc, argv);
#else
  return do_main<ARM64>(argc, argv);
#endif
}

}
