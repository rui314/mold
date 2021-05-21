#include "mold.h"

#include <functional>
#include <map>
#include <signal.h>
#include <tbb/global_control.h>
#include <tbb/parallel_for_each.h>
#include <unistd.h>
#include <unordered_set>

template <typename E>
static bool is_text_file(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data(ctx);
  return mb->size() >= 4 &&
         isprint(data[0]) &&
         isprint(data[1]) &&
         isprint(data[2]) &&
         isprint(data[3]);
}

template <typename E>
std::string_view save_string(Context<E> &ctx, const std::string &str) {
  u8 *buf = new u8[str.size() + 1];
  memcpy(buf, str.data(), str.size());
  buf[str.size()] = '\0';
  ctx.owning_bufs.push_back(std::unique_ptr<u8[]>(buf));
  return {(char *)buf, str.size()};
}

std::string get_version_string() {
  if (strlen(GIT_HASH) == 0)
    return "mold " MOLD_VERSION " (compatible with GNU ld)";
  return "mold " MOLD_VERSION " (" GIT_HASH "; compatible with GNU ld)";
}

enum class FileType { UNKNOWN, OBJ, DSO, AR, THIN_AR, TEXT };

template <typename E>
static FileType get_file_type(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data(ctx);

  if (mb->size() >= 20 && memcmp(data, "\177ELF", 4) == 0) {
    ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)data;
    if (ehdr.e_type == ET_REL)
      return FileType::OBJ;
    if (ehdr.e_type == ET_DYN)
      return FileType::DSO;
    return FileType::UNKNOWN;
  }

  if (mb->size() >= 8 && memcmp(data, "!<arch>\n", 8) == 0)
    return FileType::AR;
  if (mb->size() >= 8 && memcmp(data, "!<thin>\n", 8) == 0)
    return FileType::THIN_AR;
  if (is_text_file(ctx, mb))
    return FileType::TEXT;
  return FileType::UNKNOWN;
}

template <typename E>
static ObjectFile<E> *new_object_file(Context<E> &ctx, MemoryMappedFile<E> *mb,
                                      std::string archive_name) {
  static Counter count("parsed_objs");
  count++;

  bool in_lib = (!archive_name.empty() && !ctx.whole_archive);
  ObjectFile<E> *file = ObjectFile<E>::create(ctx, mb, archive_name, in_lib);
  ctx.tg.run([file, &ctx]() { file->parse(ctx); });
  if (ctx.arg.trace)
    SyncOut(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
static SharedFile<E> *new_shared_file(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  SharedFile<E> *file = SharedFile<E>::create(ctx, mb);
  ctx.tg.run([file, &ctx]() { file->parse(ctx); });
  if (ctx.arg.trace)
    SyncOut(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
void read_file(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  if (ctx.visited.contains(mb->name))
    return;

  if (ctx.is_preloading) {
    switch (get_file_type(ctx, mb)) {
    case FileType::OBJ:
      ctx.obj_cache.store(mb, new_object_file(ctx, mb, ""));
      return;
    case FileType::DSO:
      ctx.dso_cache.store(mb, new_shared_file(ctx, mb));
      return;
    case FileType::AR:
      for (MemoryMappedFile<E> *child : read_fat_archive_members(ctx, mb))
        if (get_file_type(ctx, child) == FileType::OBJ)
          ctx.obj_cache.store(mb, new_object_file(ctx, child, mb->name));
      return;
    case FileType::THIN_AR:
      for (MemoryMappedFile<E> *child : read_thin_archive_members(ctx, mb))
        if (get_file_type(ctx, child) == FileType::OBJ)
          ctx.obj_cache.store(child, new_object_file(ctx, child, mb->name));
      return;
    case FileType::TEXT:
      parse_linker_script(ctx, mb);
      return;
    default:
      Fatal(ctx) << mb->name << ": unknown file type";
    }
  }

  switch (get_file_type(ctx, mb)) {
  case FileType::OBJ:
    if (ObjectFile<E> *obj = ctx.obj_cache.get_one(mb))
      ctx.objs.push_back(obj);
    else
      ctx.objs.push_back(new_object_file(ctx, mb, ""));
    return;
  case FileType::DSO:
    if (SharedFile<E> *obj = ctx.dso_cache.get_one(mb))
      ctx.dsos.push_back(obj);
    else
      ctx.dsos.push_back(new_shared_file(ctx, mb));
    ctx.visited.insert(mb->name);
    return;
  case FileType::AR: {
    std::vector<ObjectFile<E> *> objs = ctx.obj_cache.get(mb);
    if (!objs.empty()) {
      append(ctx.objs, objs);
    } else {
      for (MemoryMappedFile<E> *child : read_fat_archive_members(ctx, mb))
        if (get_file_type(ctx, child) == FileType::OBJ)
          ctx.objs.push_back(new_object_file(ctx, child, mb->name));
    }
    ctx.visited.insert(mb->name);
    return;
  }
  case FileType::THIN_AR:
    for (MemoryMappedFile<E> *child : read_thin_archive_members(ctx, mb)) {
      if (ObjectFile<E> *obj = ctx.obj_cache.get_one(child))
        ctx.objs.push_back(obj);
      else if (get_file_type(ctx, child) == FileType::OBJ)
        ctx.objs.push_back(new_object_file(ctx, child, mb->name));
    }
    ctx.visited.insert(mb->name);
    return;
  case FileType::TEXT:
    parse_linker_script(ctx, mb);
    return;
  default:
    Fatal(ctx) << mb->name << ": unknown file type";
  }
}

template <typename E>
void cleanup() {
  if (OutputFile<E>::tmpfile)
    unlink(OutputFile<E>::tmpfile);
  if (socket_tmpfile)
    unlink(socket_tmpfile);
}

template <typename E>
static void signal_handler(int) {
  cleanup<E>();
  _exit(1);
}

template <typename E>
MemoryMappedFile<E> *find_library(Context<E> &ctx, std::string name) {
  if (name.starts_with(':')) {
    for (std::string_view dir : ctx.arg.library_paths) {
      std::string root = dir.starts_with("/") ? ctx.arg.sysroot : "";
      std::string path = root + std::string(dir) + "/" + name.substr(1);
      if (MemoryMappedFile<E> *mb = MemoryMappedFile<E>::open(ctx, path))
        return mb;
    }
    Fatal(ctx) << "library not found: " << name;
  }

  for (std::string_view dir : ctx.arg.library_paths) {
    std::string root = dir.starts_with("/") ? ctx.arg.sysroot : "";
    std::string stem = root + std::string(dir) + "/lib" + name;
    if (!ctx.is_static)
      if (MemoryMappedFile<E> *mb = MemoryMappedFile<E>::open(ctx, stem + ".so"))
        return mb;
    if (MemoryMappedFile<E> *mb = MemoryMappedFile<E>::open(ctx, stem + ".a"))
      return mb;
  }
  Fatal(ctx) << "library not found: " << name;
}

template <typename E>
static void read_input_files(Context<E> &ctx, std::span<std::string_view> args) {
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
      MemoryMappedFile<E> *mb = find_library(ctx, std::string(arg));
      read_file(ctx, mb);
    } else {
      read_file(ctx, MemoryMappedFile<E>::must_open(ctx, std::string(args[0])));
      args = args.subspan(1);
    }
  }
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
  for (std::unique_ptr<MemoryMappedFile<E>> &mb : ctx.owning_mbs)
    num_bytes += mb->size();

  static Counter num_input_sections("input_sections");
  for (ObjectFile<E> *file : ctx.objs)
    num_input_sections += file->sections.size();

  static Counter num_output_chunks("output_chunks", ctx.chunks.size());
  static Counter num_objs("num_objs", ctx.objs.size());
  static Counter num_dsos("num_dsos", ctx.dsos.size());

  Counter::print();
}

template <typename E>
int do_main(int argc, char **argv) {
  Context<E> ctx;

  // Process -run option first. process_run_subcommand() does not return.
  if (argc >= 2)
    if (std::string_view arg = argv[1]; arg == "-run" || arg == "--run")
      process_run_subcommand(ctx, argc, argv);

  Timer t_all(ctx, "all");

  // Parse non-positional command line options
  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string_view> file_args;
  parse_nonpositional_args(ctx, file_args);

  if (!ctx.arg.preload)
    try_resume_daemon(ctx, argv);

  tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism,
                               ctx.arg.thread_count);

  signal(SIGINT, signal_handler<E>);
  signal(SIGTERM, signal_handler<E>);

  if (!ctx.arg.directory.empty())
    if (chdir(ctx.arg.directory.c_str()) == -1)
      Fatal(ctx) << "chdir failed: " << ctx.arg.directory
                 << ": " << strerror(errno);

  // Preload input files
  std::function<void()> on_complete;

  if (ctx.arg.preload) {
    Timer t(ctx, "preload");
    std::function<void()> wait_for_client;
    daemonize(ctx, argv, &wait_for_client, &on_complete);

    ctx.reset_reader_context(true);
    read_input_files(ctx, file_args);
    ctx.tg.wait();
    t.stop();

    Timer t2(ctx, "wait_for_client");
    wait_for_client();
  } else if (ctx.arg.fork) {
    on_complete = fork_child();
  }

  for (std::string_view arg : ctx.arg.trace_symbol)
    Symbol<E>::intern(ctx, arg)->traced = true;

  // Parse input files
  {
    Timer t(ctx, "parse");
    ctx.reset_reader_context(false);
    read_input_files(ctx, file_args);
    ctx.tg.wait();
  }

  if (ctx.objs.empty())
    Fatal(ctx) << "no input files";

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

  // Set unique indices to files.
  set_file_priority(ctx);

  // Resolve symbols and fix the set of object files that are
  // included to the final output.
  resolve_obj_symbols(ctx);

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

  // ctx input sections into output sections
  bin_sections(ctx);

  // Get a list of output sections.
  append(ctx.chunks, collect_output_sections(ctx));

  // Create a dummy file containing linker-synthesized symbols
  // (e.g. `__bss_start`).
  ctx.internal_obj = ObjectFile<E>::create_internal_file(ctx);
  ctx.internal_obj->resolve_regular_symbols(ctx);
  ctx.objs.push_back(ctx.internal_obj);

  // Add symbols from shared object files.
  resolve_dso_symbols(ctx);

  // Beyond this point, no new files will be added to ctx.objs
  // or ctx.dsos.

  // Convert weak symbols to absolute symbols with value 0.
  convert_undefined_weak_symbols(ctx);

  // If we are linking a .so file, remaining undefined symbols does
  // not cause a linker error. Instead, they are treated as if they
  // were imported symbols.
  if (ctx.arg.shared && !ctx.arg.z_defs) {
    Timer t(ctx, "claim_unresolved_symbols");
    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      file->claim_unresolved_symbols();
    });
  }

  // Beyond this point, no new symbols will be added to the result.

  // Make sure that all symbols have been resolved.
  if (!ctx.arg.allow_multiple_definition)
    check_duplicate_symbols(ctx);

  // Compute sizes of output sections while assigning offsets
  // within an output section to input sections.
  compute_section_sizes(ctx);

  // Sort sections by section attributes so that we'll have to
  // create as few segments as possible.
  sort(ctx.chunks, [&](OutputChunk<E> *a, OutputChunk<E> *b) {
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

  // Sort .dynsym contents. Beyond this point, no symbol will be
  // added to .dynsym.
  ctx.dynsym->sort_symbols(ctx);

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
    erase(ctx.chunks, [](OutputChunk<E> *chunk) {
      return chunk->kind == OutputChunk<E>::REGULAR &&
             chunk->name == ".eh_frame";
    });
    ctx.eh_frame->construct(ctx);
  }

  // Now that we have computed sizes for all sections and assigned
  // section indices to them, so we can fix section header contents
  // for all output sections.
  for (OutputChunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  erase(ctx.chunks, [](OutputChunk<E> *chunk) {
    return chunk->kind == OutputChunk<E>::SYNTHETIC &&
           chunk->shdr.sh_size == 0;
  });

  // Set section indices.
  for (i64 i = 0, shndx = 1; i < ctx.chunks.size(); i++)
    if (ctx.chunks[i]->kind != OutputChunk<E>::HEADER)
      ctx.chunks[i]->shndx = shndx++;

  for (OutputChunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  // Assign offsets to output sections
  i64 filesize = set_osec_offsets(ctx);

  // Fix linker-synthesized symbol addresses.
  fix_synthetic_symbols(ctx);

  // If --compress-debug-sections is given, compress .debug_* sections
  // using zlib.
  if (ctx.arg.compress_debug_sections) {
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
  ctx.output_file = OutputFile<E>::open(ctx, ctx.arg.output, filesize);
  ctx.buf = ctx.output_file->buf;

  Timer t_copy(ctx, "copy");

  // Copy input sections to the output file
  {
    Timer t(ctx, "copy_buf");

    tbb::parallel_for_each(ctx.chunks, [&](OutputChunk<E> *chunk) {
      std::string name(chunk->name);
      if (name.empty())
        name = "(header)";
      Timer t2(ctx, name, &t);

      chunk->copy_buf(ctx);
    });

    Error<E>::checkpoint(ctx);
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
    Timer<E>::print(ctx);

  std::cout << std::flush;
  std::cerr << std::flush;
  if (on_complete)
    on_complete();

  if (ctx.arg.quick_exit)
    std::quick_exit(0);

  for (std::function<void()> &fn : ctx.on_exit)
    fn();
  return 0;
}

enum class MachineType { X86_64, I386 };

static MachineType get_machine_type(int argc, char **argv) {
  for (i64 i = 1; i < argc; i++) {
    if (std::string_view(argv[i]) == "-m") {
      if (i + 1 == argc)
        break;
      i++;

      std::string_view val(argv[i]);
      if (val == "elf_x86_64")
        return MachineType::X86_64;
      if (val == "elf_i386")
        return MachineType::I386;
      std::cerr << "unknown -m argument: " << val << "\n";
      exit(1);
    }
  }
  return MachineType::X86_64;
  //  std::cerr << "-m is missing";
  //  exit(1);
}

int main(int argc, char **argv) {
  switch (get_machine_type(argc, argv)) {
  case MachineType::X86_64:
    return do_main<X86_64>(argc, argv);
  case MachineType::I386:
    return do_main<I386>(argc, argv);
  }
}

#define INSTANTIATE(E)                                                  \
  template void read_file(Context<E> &, MemoryMappedFile<E> *);         \
  template std::string_view save_string(Context<E> &, const std::string &)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
