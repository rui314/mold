#include "mold.h"

#include <cstring>
#include <functional>
#include <iomanip>
#include <map>
#include <signal.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/parallel_for_each.h>
#include <unistd.h>
#include <unordered_set>

namespace mold::elf {

std::regex glob_to_regex(std::string_view pattern) {
  std::stringstream ss;
  for (u8 c : pattern) {
    if (c == '*')
      ss << ".*";
    else
      ss << "\\x" << std::hex << std::setw(2) << std::setfill('0') << (int)c;
  }
  return std::regex(ss.str(), std::regex::optimize);
}

template <typename E>
static ObjectFile<E> *new_object_file(Context<E> &ctx, MappedFile<Context<E>> *mf,
                                      std::string archive_name) {
  static Counter count("parsed_objs");
  count++;

  bool in_lib = (!archive_name.empty() && !ctx.whole_archive);
  ObjectFile<E> *file = ObjectFile<E>::create(ctx, mf, archive_name, in_lib);
  file->priority = ctx.file_priority++;
  ctx.tg.run([file, &ctx]() { file->parse(ctx); });
  if (ctx.arg.trace)
    SyncOut(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
static SharedFile<E> *new_shared_file(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  SharedFile<E> *file = SharedFile<E>::create(ctx, mf);
  file->priority = ctx.file_priority++;
  ctx.tg.run([file, &ctx]() { file->parse(ctx); });
  if (ctx.arg.trace)
    SyncOut(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
void read_file(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  if (ctx.visited.contains(mf->name))
    return;

  switch (get_file_type(mf)) {
  case FileType::ELF_OBJ:
    ctx.objs.push_back(new_object_file(ctx, mf, ""));
    return;
  case FileType::ELF_DSO:
    ctx.dsos.push_back(new_shared_file(ctx, mf));
    ctx.visited.insert(mf->name);
    return;
  case FileType::AR:
  case FileType::THIN_AR:
    for (MappedFile<Context<E>> *child : read_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::ELF_OBJ)
        ctx.objs.push_back(new_object_file(ctx, child, mf->name));
    ctx.visited.insert(mf->name);
    return;
  case FileType::TEXT:
    parse_linker_script(ctx, mf);
    return;
  case FileType::LLVM_BITCODE:
    Fatal(ctx) << mf->name << ": looks like this is an LLVM bitcode, "
               << "but mold does not support LTO";
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
  }
}

// Read the beginning of a given file and returns its machine type
// (e.g. EM_X86_64 or EM_386). Return -1 if unknown.
template <typename E>
static i64 get_machine_type(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  switch (get_file_type(mf)) {
  case FileType::ELF_DSO:
    return ((ElfEhdr<E> *)mf->data)->e_machine;
  case FileType::AR:
    for (MappedFile<Context<E>> *child : read_fat_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::ELF_OBJ)
        return ((ElfEhdr<E> *)child->data)->e_machine;
    return -1;
  case FileType::THIN_AR:
    for (MappedFile<Context<E>> *child : read_thin_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::ELF_OBJ)
        return ((ElfEhdr<E> *)child->data)->e_machine;
    return -1;
  case FileType::TEXT:
    return get_script_output_type(ctx, mf);
  default:
    return -1;
  }
}

template <typename E>
static MappedFile<Context<E>> *open_library(Context<E> &ctx, std::string path) {
  MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path);
  if (!mf)
    return nullptr;

  i64 type = get_machine_type(ctx, mf);
  if (type == -1 || type == E::e_machine)
    return mf;
  Warn(ctx) << path << ": skipping incompatible file " << (int)type
            << " " << (int)E::e_machine;
  return nullptr;
}

template <typename E>
MappedFile<Context<E>> *find_library(Context<E> &ctx, std::string name) {
  if (name.starts_with(':')) {
    for (std::string_view dir : ctx.arg.library_paths) {
      std::string path = std::string(dir) + "/" + name.substr(1);
      if (MappedFile<Context<E>> *mf = open_library(ctx, path))
        return mf;
    }
    Fatal(ctx) << "library not found: " << name;
  }

  for (std::string_view dir : ctx.arg.library_paths) {
    std::string stem = std::string(dir) + "/lib" + name;
    if (!ctx.is_static)
      if (MappedFile<Context<E>> *mf = open_library(ctx, stem + ".so"))
        return mf;
    if (MappedFile<Context<E>> *mf = open_library(ctx, stem + ".a"))
      return mf;
  }
  Fatal(ctx) << "library not found: " << name;
}

template <typename E>
static void read_input_files(Context<E> &ctx, std::span<std::string_view> args) {
  Timer t(ctx, "read_input_files");

  std::vector<std::tuple<bool, bool, bool>> state;
  ctx.is_static = ctx.arg.is_static;

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
    } else if (read_arg(ctx, args, arg, "version-script")) {
      parse_version_script(ctx, std::string(arg));
    } else if (read_arg(ctx, args, arg, "dynamic-list")) {
      parse_dynamic_list(ctx, std::string(arg));
    } else if (read_flag(args, "push-state")) {
      state.push_back({ctx.as_needed, ctx.whole_archive, ctx.is_static});
    } else if (read_flag(args, "pop-state")) {
      if (state.empty())
        Fatal(ctx) << "no state pushed before popping";
      std::tie(ctx.as_needed, ctx.whole_archive, ctx.is_static) = state.back();
      state.pop_back();
    } else if (read_arg(ctx, args, arg, "l")) {
      MappedFile<Context<E>> *mf = find_library(ctx, std::string(arg));
      mf->given_fullpath = false;
      read_file(ctx, mf);
    } else {
      read_file(ctx, MappedFile<Context<E>>::must_open(ctx, std::string(args[0])));
      args = args.subspan(1);
    }
  }

  if (ctx.objs.empty())
    Fatal(ctx) << "no input files";

  ctx.tg.wait();
}

template <typename E>
static i64 get_mtime(Context<E> &ctx, std::string path) {
  struct stat st;
  if (stat(path.c_str(), &st) < 0)
    Fatal(ctx) << path << ": stat failed: " << errno_string();
  return st.st_mtime;
}

template <typename E>
static bool reload_input_files(Context<E> &ctx) {
  Timer t(ctx, "reload_input_files");

  std::vector<ObjectFile<E> *> objs;
  std::vector<SharedFile<E> *> dsos;

  // Reload updated .o files
  for (ObjectFile<E> *file : ctx.objs) {
    if (file->mf->parent) {
      if (get_mtime(ctx, file->mf->parent->name) != file->mf->parent->mtime)
        return false;
      objs.push_back(file);
      continue;
    }

    if (get_mtime(ctx, file->mf->name) == file->mf->mtime) {
      objs.push_back(file);
      continue;
    }

    MappedFile<Context<E>> *mf =
      MappedFile<Context<E>>::must_open(ctx, file->mf->name);
    objs.push_back(new_object_file(ctx, mf, file->mf->name));
  }

  // Reload updated .so files
  for (SharedFile<E> *file : ctx.dsos) {
    MappedFile<Context<E>> *mf =
      MappedFile<Context<E>>::must_open(ctx, file->mf->name);

    if (get_mtime(ctx, file->mf->name) == file->mf->mtime) {
      dsos.push_back(file);
    } else {
      MappedFile<Context<E>> *mf =
        MappedFile<Context<E>>::must_open(ctx, file->mf->name);
      dsos.push_back(new_shared_file(ctx, mf));
    }
  }

  ctx.objs = objs;
  ctx.dsos = dsos;
  return true;
}

template <typename E>
static void show_stats(Context<E> &ctx) {
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

      if (sec->shdr.sh_flags & SHF_ALLOC)
        alloc += sec->get_rels(ctx).size();
      else
        nonalloc += sec->get_rels(ctx).size();
    }

    static Counter comdats("comdats");
    comdats += obj->comdat_groups.size();

    static Counter removed_comdats("removed_comdat_mem");
    for (auto &pair : obj->comdat_groups)
      if (ComdatGroup *group = pair.first; group->owner != obj->priority)
        removed_comdats += pair.second.size();

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
  for (std::unique_ptr<MappedFile<Context<E>>> &mf : ctx.mf_pool)
    num_bytes += mf->size;

  static Counter num_input_sections("input_sections");
  for (ObjectFile<E> *file : ctx.objs)
    num_input_sections += file->sections.size();

  static Counter num_output_chunks("output_chunks", ctx.chunks.size());
  static Counter num_objs("num_objs", ctx.objs.size());
  static Counter num_dsos("num_dsos", ctx.dsos.size());

  Counter::print();
}

template <typename E>
static int elf_main(int argc, char **argv) {
  Context<E> ctx;

  // Process -run option first. process_run_subcommand() does not return.
  if (argc >= 2)
    if (argv[1] == "-run"sv || argv[1] == "--run"sv)
      process_run_subcommand(ctx, argc, argv);

  // Parse non-positional command line options
  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string_view> file_args;
  parse_nonpositional_args(ctx, file_args);

  // Redo if -m is not x86-64.
  if (ctx.arg.emulation != E::e_machine) {
    switch (ctx.arg.emulation) {
    case EM_386:
      return elf_main<I386>(argc, argv);
    case EM_AARCH64:
      return elf_main<AARCH64>(argc, argv);
    }
    unreachable();
  }

  Timer t_all(ctx, "all");

  if (ctx.arg.relocatable) {
    combine_objects(ctx, file_args);
    return 0;
  }

  if (!ctx.arg.preload)
    try_resume_daemon(ctx);

  set_thread_count(ctx.arg.thread_count);
  install_signal_handler();

  if (!ctx.arg.directory.empty() && chdir(ctx.arg.directory.c_str()) == -1)
    Fatal(ctx) << "chdir failed: " << ctx.arg.directory
               << ": " << errno_string();

  // Handle --wrap options if any.
  for (std::string_view name : ctx.arg.wrap)
    Symbol<E>::intern(ctx, name)->wrap = true;

  // Handle --retain-symbols-file options if any.
  if (ctx.arg.retain_symbols_file)
    for (std::string_view name : *ctx.arg.retain_symbols_file)
      Symbol<E>::intern(ctx, name)->write_to_symtab = true;

  // Preload input files
  std::function<void()> on_complete;
  std::function<void()> wait_for_client;

  if (ctx.arg.preload)
    daemonize(ctx, &wait_for_client, &on_complete);
  else if (ctx.arg.fork)
    on_complete = fork_child();

  for (std::string_view arg : ctx.arg.trace_symbol)
    Symbol<E>::intern(ctx, arg)->traced = true;

  // Parse input files
  read_input_files(ctx, file_args);

  if (ctx.arg.preload) {
    wait_for_client();
    if (!reload_input_files(ctx)) {
      std::vector<char *> args(argv, argv + argc);
      args.push_back((char *)"--no-preload");
      return elf_main<E>(argc + 1, args.data());
    }
  }

  {
    Timer t(ctx, "register_subsections");
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      file->register_subsections(ctx);
    });
  }

  // Uniquify shared object files by soname
  {
    std::unordered_set<std::string_view> seen;
    erase(ctx.dsos, [&](SharedFile<E> *file) {
      return !seen.insert(file->soname).second;
    });
  }

  Timer t_total(ctx, "total");
  Timer t_before_copy(ctx, "before_copy");

  // Apply -exclude-libs
  apply_exclude_libs(ctx);

  // Create instances of linker-synthesized sections such as
  // .got or .plt.
  create_synthetic_sections(ctx);

  // Resolve symbols and fix the set of object files that are
  // included to the final output.
  resolve_symbols(ctx);

  // Remove redundant comdat sections (e.g. duplicate inline functions).
  eliminate_comdats(ctx);

  // Create .bss sections for common symbols.
  convert_common_symbols(ctx);

  // Apply version scripts.
  apply_version_script(ctx);

  // Parse symbol version suffixes (e.g. "foo@ver1").
  parse_symbol_version(ctx);

  // Set is_import and is_export bits for each symbol.
  compute_import_export(ctx);

  // Garbage-collect unreachable sections.
  if (ctx.arg.gc_sections)
    gc_sections(ctx);

  // Merge identical read-only sections.
  if (ctx.arg.icf)
    icf_sections(ctx);

  // Compute sizes of sections containing mergeable strings.
  compute_merged_section_sizes(ctx);

  // Put input sections into output sections
  bin_sections(ctx);

  // Get a list of output sections.
  append(ctx.chunks, collect_output_sections(ctx));

  // Create a dummy file containing linker-synthesized symbols
  // (e.g. `__bss_start`).
  ctx.internal_obj = create_internal_file(ctx);
  ctx.internal_obj->resolve_regular_symbols(ctx);
  ctx.objs.push_back(ctx.internal_obj);

  // Beyond this point, no new files will be added to ctx.objs
  // or ctx.dsos.

  // If we are linking a .so file, remaining undefined symbols does
  // not cause a linker error. Instead, they are treated as if they
  // were imported symbols.
  claim_unresolved_symbols(ctx);

  // Beyond this point, no new symbols will be added to the result.

  // Make sure that all symbols have been resolved.
  if (!ctx.arg.allow_multiple_definition)
    check_duplicate_symbols(ctx);

  for (std::string_view name : ctx.arg.require_defined)
    if (!Symbol<E>::intern(ctx, name)->file)
      Error(ctx) << "--require-defined: undefined symbol: " << name;

  // .init_array and .fini_array contents have to be sorted by
  // a special rule. Sort them.
  sort_init_fini(ctx);

  // Compute sizes of output sections while assigning offsets
  // within an output section to input sections.
  compute_section_sizes(ctx);

  // Sort sections by section attributes so that we'll have to
  // create as few segments as possible.
  sort(ctx.chunks, [&](Chunk<E> *a, Chunk<E> *b) {
    return get_section_rank(ctx, a) < get_section_rank(ctx, b);
  });

  // Copy string referred by .dynamic to .dynstr.
  for (SharedFile<E> *file : ctx.dsos)
    ctx.dynstr->add_string(file->soname);
  for (std::string_view str : ctx.arg.auxiliary)
    ctx.dynstr->add_string(str);
  for (std::string_view str : ctx.arg.filter)
    ctx.dynstr->add_string(str);
  if (!ctx.arg.rpaths.empty())
    ctx.dynstr->add_string(ctx.arg.rpaths);
  if (!ctx.arg.soname.empty())
    ctx.dynstr->add_string(ctx.arg.soname);

  // Scan relocations to find symbols that need entries in .got, .plt,
  // .got.plt, .dynsym, .dynstr, etc.
  scan_rels(ctx);

  // Reserve a space for dynamic symbol strings in .dynstr and sort
  // .dynsym contents if necessary. Beyond this point, no symbol will
  // be added to .dynsym.
  ctx.dynsym->finalize(ctx);

  // Fill .gnu.version_d section contents.
  ctx.verdef->construct(ctx);

  // Fill .gnu.version_r section contents.
  ctx.verneed->construct(ctx);

  // Compute .symtab and .strtab sizes for each file.
  {
    Timer t(ctx, "compute_symtab");
    tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
      file->compute_symtab(ctx);
    });
  }

  // .eh_frame is a special section from the linker's point of view,
  // as its contents are parsed and reconstructed by the linker,
  // unlike other sections that are regarded as opaque bytes.
  // Here, we transplant .eh_frame sections from a regular output
  // section to the special EHFrameSection.
  {
    Timer t(ctx, "eh_frame");
    erase(ctx.chunks, [](Chunk<E> *chunk) {
      return chunk->kind == Chunk<E>::REGULAR &&
             chunk->name == ".eh_frame";
    });
    ctx.eh_frame->construct(ctx);
  }

  // Now that we have computed sizes for all sections and assigned
  // section indices to them, so we can fix section header contents
  // for all output sections.
  for (Chunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  erase(ctx.chunks, [](Chunk<E> *chunk) {
    return chunk->kind == Chunk<E>::SYNTHETIC &&
           chunk->shdr.sh_size == 0;
  });

  // Set section indices.
  for (i64 i = 0, shndx = 1; i < ctx.chunks.size(); i++)
    if (ctx.chunks[i]->kind != Chunk<E>::HEADER)
      ctx.chunks[i]->shndx = shndx++;

  for (Chunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  // Assign offsets to output sections
  i64 filesize = set_osec_offsets(ctx);

  // Fix linker-synthesized symbol addresses.
  fix_synthetic_symbols(ctx);

  // If --compress-debug-sections is given, compress .debug_* sections
  // using zlib.
  if (ctx.arg.compress_debug_sections != COMPRESS_NONE) {
    compress_debug_sections(ctx);
    filesize = set_osec_offsets(ctx);
  }

  // At this point, file layout is fixed.

  // Beyond this, you can assume that symbol addresses including their
  // GOT or PLT addresses have a correct final value.

  // Some types of relocations for TLS symbols need the TLS segment
  // address. Find it out now.
  for (ElfPhdr<E> phdr : create_phdr(ctx)) {
    if (phdr.p_type == PT_TLS) {
      ctx.tls_begin = phdr.p_vaddr;
      ctx.tls_end = align_to(phdr.p_vaddr + phdr.p_memsz, phdr.p_align);
      break;
    }
  }

  t_before_copy.stop();

  // Create an output file
  ctx.output_file = OutputFile<E>::open(ctx, ctx.arg.output, filesize, 0777);
  ctx.buf = ctx.output_file->buf;

  Timer t_copy(ctx, "copy");

  // Copy input sections to the output file
  {
    Timer t(ctx, "copy_buf");

    tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
      std::string name(chunk->name);
      if (name.empty())
        name = "(header)";
      Timer t2(ctx, name, &t);

      chunk->copy_buf(ctx);
    });

    ctx.checkpoint();
  }

  // Dynamic linker works better with sorted .rela.dyn section,
  // so we sort them.
  ctx.reldyn->sort(ctx);

  // Zero-clear paddings between sections
  clear_padding(ctx);

  if (ctx.buildid) {
    Timer t(ctx, "build_id");
    ctx.buildid->write_buildid(ctx);
  }

  t_copy.stop();

  // Commit
  ctx.output_file->close(ctx);

  t_total.stop();
  t_all.stop();

  if (ctx.arg.print_map)
    print_map(ctx);

  // Show stats numbers
  if (ctx.arg.stats)
    show_stats(ctx);

  if (ctx.arg.perf)
    print_timer_records(ctx.timer_records);

  std::cout << std::flush;
  std::cerr << std::flush;
  if (on_complete)
    on_complete();

  if (ctx.arg.quick_exit)
    _exit(0);

  for (std::function<void()> &fn : ctx.on_exit)
    fn();
  return 0;
}

int main(int argc, char **argv) {
  return elf_main<X86_64>(argc, argv);
}

#define INSTANTIATE(E)                                                  \
  template void read_file(Context<E> &, MappedFile<Context<E>> *);

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(AARCH64);

} // namespace mold::elf
