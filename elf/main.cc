#include "mold.h"
#include "../archive-file.h"
#include "../cmdline.h"
#include "../output-file.h"

#include <cstring>
#include <functional>
#include <iomanip>
#include <map>
#include <regex>
#include <signal.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/global_control.h>
#include <tbb/parallel_for_each.h>
#include <unordered_set>

#ifdef _WIN32
# include <direct.h>
# define _chdir chdir
#else
# include <unistd.h>
#endif

namespace mold::elf {

// Read the beginning of a given file and returns its machine type
// (e.g. EM_X86_64 or EM_386).
template <typename E>
static MachineType get_machine_type(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  auto get_elf_type = [&](u8 *buf) {
    bool is_le = (((EL32Ehdr *)buf)->e_ident[EI_DATA] == ELFDATA2LSB);
    bool is_64;
    u32 e_machine;

    if (is_le) {
      EL32Ehdr &ehdr = *(EL32Ehdr *)buf;
      is_64 = (ehdr.e_ident[EI_CLASS] == ELFCLASS64);
      e_machine = ehdr.e_machine;
    } else {
      EB32Ehdr &ehdr = *(EB32Ehdr *)buf;
      is_64 = (ehdr.e_ident[EI_CLASS] == ELFCLASS64);
      e_machine = ehdr.e_machine;
    }

    switch (e_machine) {
    case EM_386:
      return MachineType::I386;
    case EM_X86_64:
      return MachineType::X86_64;
    case EM_ARM:
      return MachineType::ARM32;
    case EM_AARCH64:
      return MachineType::ARM64;
    case EM_RISCV:
      if (is_le)
        return is_64 ? MachineType::RV64LE : MachineType::RV32LE;
      return is_64 ? MachineType::RV64BE : MachineType::RV32BE;
    case EM_PPC64:
      return is_le ? MachineType::PPC64V2 : MachineType::PPC64V1;
    case EM_S390X:
      return MachineType::S390X;
    case EM_SPARC64:
      return MachineType::SPARC64;
    default:
      return MachineType::NONE;
    }
  };

  switch (get_file_type(mf)) {
  case FileType::ELF_OBJ:
  case FileType::ELF_DSO:
  case FileType::GCC_LTO_OBJ:
    return get_elf_type(mf->data);
  case FileType::AR:
    for (MappedFile<Context<E>> *child : read_fat_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::ELF_OBJ)
        return get_elf_type(child->data);
    return MachineType::NONE;
  case FileType::THIN_AR:
    for (MappedFile<Context<E>> *child : read_thin_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::ELF_OBJ)
        return get_elf_type(child->data);
    return MachineType::NONE;
  case FileType::TEXT:
    return get_script_output_type(ctx, mf);
  default:
    return MachineType::NONE;
  }
}

template <typename E>
static void
check_file_compatibility(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  MachineType mt = get_machine_type(ctx, mf);
  if (mt != ctx.arg.emulation)
    Fatal(ctx) << mf->name << ": incompatible file type: "
               << ctx.arg.emulation << " is expected but got " << mt;
}

template <typename E>
static ObjectFile<E> *new_object_file(Context<E> &ctx, MappedFile<Context<E>> *mf,
                                      std::string archive_name) {
  static Counter count("parsed_objs");
  count++;

  check_file_compatibility(ctx, mf);

  bool in_lib = ctx.in_lib || (!archive_name.empty() && !ctx.whole_archive);
  ObjectFile<E> *file = ObjectFile<E>::create(ctx, mf, archive_name, in_lib);
  file->priority = ctx.file_priority++;
  ctx.tg.run([file, &ctx] { file->parse(ctx); });
  if (ctx.arg.trace)
    SyncOut(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
static ObjectFile<E> *new_lto_obj(Context<E> &ctx, MappedFile<Context<E>> *mf,
                                  std::string archive_name) {
  static Counter count("parsed_lto_objs");
  count++;

  if (ctx.arg.ignore_ir_file.count(mf->get_identifier()))
    return nullptr;

  ObjectFile<E> *file = read_lto_object(ctx, mf);
  file->priority = ctx.file_priority++;
  file->archive_name = archive_name;
  file->is_in_lib = ctx.in_lib || (!archive_name.empty() && !ctx.whole_archive);
  file->is_alive = !file->is_in_lib;
  ctx.has_lto_object = true;
  if (ctx.arg.trace)
    SyncOut(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
static SharedFile<E> *
new_shared_file(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  check_file_compatibility(ctx, mf);

  SharedFile<E> *file = SharedFile<E>::create(ctx, mf);
  file->priority = ctx.file_priority++;
  ctx.tg.run([file, &ctx] { file->parse(ctx); });
  if (ctx.arg.trace)
    SyncOut(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
void read_file(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  if (ctx.visited.contains(mf->name))
    return;

  FileType type = get_file_type(mf);
  switch (type) {
  case FileType::ELF_OBJ:
    ctx.objs.push_back(new_object_file(ctx, mf, ""));
    return;
  case FileType::ELF_DSO:
    ctx.dsos.push_back(new_shared_file(ctx, mf));
    ctx.visited.insert(mf->name);
    return;
  case FileType::AR:
  case FileType::THIN_AR:
    for (MappedFile<Context<E>> *child : read_archive_members(ctx, mf)) {
      switch (get_file_type(child)) {
      case FileType::ELF_OBJ:
        ctx.objs.push_back(new_object_file(ctx, child, mf->name));
        break;
      case FileType::GCC_LTO_OBJ:
      case FileType::LLVM_BITCODE:
        if (ObjectFile<E> *file = new_lto_obj(ctx, child, mf->name))
          ctx.objs.push_back(file);
        break;
      default:
        break;
      }
    }
    ctx.visited.insert(mf->name);
    return;
  case FileType::TEXT:
    parse_linker_script(ctx, mf);
    return;
  case FileType::GCC_LTO_OBJ:
  case FileType::LLVM_BITCODE:
    if (ObjectFile<E> *file = new_lto_obj(ctx, mf, ""))
      ctx.objs.push_back(file);
    return;
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
  }
}

template <typename E>
static MachineType
deduce_machine_type(Context<E> &ctx, std::span<std::string> args) {
  for (std::string_view arg : args)
    if (!arg.starts_with('-'))
      if (auto *mf = MappedFile<Context<E>>::open(ctx, std::string(arg)))
        if (MachineType ty = get_machine_type(ctx, mf); ty != MachineType::NONE)
          return ty;
  Fatal(ctx) << "-m option is missing";
}

template <typename E>
MappedFile<Context<E>> *open_library(Context<E> &ctx, std::string path) {
  MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path);
  if (!mf)
    return nullptr;

  MachineType ty = get_machine_type(ctx, mf);
  if (ty == MachineType::NONE || ty == E::machine_type)
    return mf;
  Warn(ctx) << path << ": skipping incompatible file " << (int)ty
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
static void read_input_files(Context<E> &ctx, std::span<std::string> args) {
  Timer t(ctx, "read_input_files");

  std::vector<std::tuple<bool, bool, bool, bool>> state;
  ctx.is_static = ctx.arg.is_static;

  while (!args.empty()) {
    std::string_view arg = args[0];
    args = args.subspan(1);

    if (arg == "--as-needed") {
      ctx.as_needed = true;
    } else if (arg == "--no-as-needed") {
      ctx.as_needed = false;
    } else if (arg == "--whole-archive") {
      ctx.whole_archive = true;
    } else if (arg == "--no-whole-archive") {
      ctx.whole_archive = false;
    } else if (arg == "--Bstatic") {
      ctx.is_static = true;
    } else if (arg == "--Bdynamic") {
      ctx.is_static = false;
    } else if (arg == "--start-lib") {
      ctx.in_lib = true;
    } else if (arg == "--end-lib") {
      ctx.in_lib = false;
    } else if (remove_prefix(arg, "--version-script=")) {
      parse_version_script(ctx, std::string(arg));
    } else if (remove_prefix(arg, "--dynamic-list=")) {
      parse_dynamic_list(ctx, std::string(arg));
    } else if (remove_prefix(arg, "--export-dynamic-symbol=")) {
      if (arg == "*")
        ctx.default_version = VER_NDX_GLOBAL;
      else
        ctx.version_patterns.push_back({arg, VER_NDX_GLOBAL, false});
    } else if (remove_prefix(arg, "--export-dynamic-symbol-list=")) {
      parse_dynamic_list(ctx, std::string(arg));
    } else if (arg == "--push-state") {
      state.push_back({ctx.as_needed, ctx.whole_archive, ctx.is_static,
                       ctx.in_lib});
    } else if (arg == "--pop-state") {
      if (state.empty())
        Fatal(ctx) << "no state pushed before popping";
      std::tie(ctx.as_needed, ctx.whole_archive, ctx.is_static, ctx.in_lib) =
        state.back();
      state.pop_back();
    } else if (remove_prefix(arg, "-l")) {
      MappedFile<Context<E>> *mf = find_library(ctx, std::string(arg));
      mf->given_fullpath = false;
      read_file(ctx, mf);
    } else {
      read_file(ctx, MappedFile<Context<E>>::must_open(ctx, std::string(arg)));
    }
  }

  if (ctx.objs.empty())
    Fatal(ctx) << "no input files";

  ctx.tg.wait();
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

      if (sec->shdr().sh_flags & SHF_ALLOC)
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

  if constexpr (needs_thunk<E>) {
    static Counter num_thunks("num_thunks");
    for (std::unique_ptr<OutputSection<E>> &osec : ctx.output_sections)
      for (std::unique_ptr<RangeExtensionThunk<E>> &thunk : osec->thunks)
        num_thunks += thunk->symbols.size();
  }

  Counter::print();

  for (std::unique_ptr<MergedSection<E>> &sec : ctx.merged_sections)
    sec->print_stats(ctx);
}

// Since elf_main is a template, we can't run it without a type parameter.
// We speculatively run elf_main with X86_64, and if the speculation was
// wrong, re-run it with an actual machine type.
template <typename E>
static int redo_main(int argc, char **argv, MachineType ty) {
  switch (ty) {
  case MachineType::I386:
    return elf_main<I386>(argc, argv);
  case MachineType::ARM64:
    return elf_main<ARM64>(argc, argv);
  case MachineType::ARM32:
    return elf_main<ARM32>(argc, argv);
  case MachineType::RV64LE:
    return elf_main<RV64LE>(argc, argv);
  case MachineType::RV64BE:
    return elf_main<RV64BE>(argc, argv);
  case MachineType::RV32LE:
    return elf_main<RV32LE>(argc, argv);
  case MachineType::RV32BE:
    return elf_main<RV32BE>(argc, argv);
  case MachineType::PPC64V1:
    return elf_main<PPC64V1>(argc, argv);
  case MachineType::PPC64V2:
    return elf_main<PPC64V2>(argc, argv);
  case MachineType::S390X:
    return elf_main<S390X>(argc, argv);
  case MachineType::SPARC64:
    return elf_main<SPARC64>(argc, argv);
  default:
    unreachable();
  }
}

template <typename E>
int elf_main(int argc, char **argv) {
  Context<E> ctx;

  // Process -run option first. process_run_subcommand() does not return.
  if (argc >= 2 && (argv[1] == "-run"sv || argv[1] == "--run"sv)) {
#if defined(_WIN32) || defined(__APPLE__)
    Fatal(ctx) << ": -run is supported only on Unix";
#endif
    process_run_subcommand(ctx, argc, argv);
  }

  // Parse non-positional command line options
  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string> file_args = parse_nonpositional_args(ctx);

  // If no -m option is given, deduce it from input files.
  if (ctx.arg.emulation == MachineType::NONE)
    ctx.arg.emulation = deduce_machine_type(ctx, file_args);

  // Redo if -m is not x86-64.
  if constexpr (std::is_same_v<E, X86_64>)
    if (ctx.arg.emulation != MachineType::X86_64)
      return redo_main<E>(argc, argv, ctx.arg.emulation);

  Timer t_all(ctx, "all");

  install_signal_handler();

  if (!ctx.arg.directory.empty())
    if (chdir(ctx.arg.directory.c_str()) == -1)
      Fatal(ctx) << "chdir failed: " << ctx.arg.directory
                 << ": " << errno_string();

  if (ctx.arg.relocatable) {
    combine_objects(ctx, file_args);
    return 0;
  }

  // Fork a subprocess unless --no-fork is given.
  std::function<void()> on_complete;

#if !defined(_WIN32) && !defined(__APPLE__)
  if (ctx.arg.fork)
    on_complete = fork_child();
#endif

  tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism,
                               ctx.arg.thread_count);

  // Handle --wrap options if any.
  for (std::string_view name : ctx.arg.wrap)
    get_symbol(ctx, name)->wrap = true;

  // Handle --retain-symbols-file options if any.
  if (ctx.arg.retain_symbols_file)
    for (std::string_view name : *ctx.arg.retain_symbols_file)
      get_symbol(ctx, name)->write_to_symtab = true;

  for (std::string_view arg : ctx.arg.trace_symbol)
    get_symbol(ctx, arg)->traced = true;

  // Parse input files
  read_input_files(ctx, file_args);

  // Uniquify shared object files by soname
  {
    std::unordered_set<std::string_view> seen;
    std::erase_if(ctx.dsos, [&](SharedFile<E> *file) {
      return !seen.insert(file->soname).second;
    });
  }

  Timer t_total(ctx, "total");
  Timer t_before_copy(ctx, "before_copy");

  // Apply -exclude-libs
  apply_exclude_libs(ctx);

  // Create a dummy file containing linker-synthesized symbols.
  create_internal_file(ctx);

  // Resolve symbols and fix the set of object files that are
  // included to the final output.
  resolve_symbols(ctx);

  // Resolve mergeable section pieces to merge them.
  register_section_pieces(ctx);

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

  // Read address-significant section information.
  if (ctx.arg.icf && !ctx.arg.icf_all)
    mark_addrsig(ctx);

  // Garbage-collect unreachable sections.
  if (ctx.arg.gc_sections)
    gc_sections(ctx);

  // Merge identical read-only sections.
  if (ctx.arg.icf)
    icf_sections(ctx);

  // Compute sizes of sections containing mergeable strings.
  compute_merged_section_sizes(ctx);

  // Create instances of linker-synthesized sections such as
  // .got or .plt.
  create_synthetic_sections(ctx);

  // Make sure that there's no duplicate symbol
  if (!ctx.arg.allow_multiple_definition)
    check_duplicate_symbols(ctx);

  if constexpr (std::is_same_v<E, PPC64V1>)
    ppc64v1_rewrite_opd(ctx);

  // Bin input sections into output sections.
  bin_sections(ctx);

  // Get a list of output sections.
  append(ctx.chunks, collect_output_sections(ctx));

  // Add synthetic symbols such as __ehdr_start or __end.
  add_synthetic_symbols(ctx);

  // Beyond this point, no new files will be added to ctx.objs
  // or ctx.dsos.

  // Handle `-z cet-report`.
  if (ctx.arg.z_cet_report != CET_REPORT_NONE)
    check_cet_errors(ctx);

  // Handle `-z execstack-if-needed`.
  if (ctx.arg.z_execstack_if_needed)
    for (ObjectFile<E> *file : ctx.objs)
      if (file->needs_executable_stack)
        ctx.arg.z_execstack = true;

  // If we are linking a .so file, remaining undefined symbols does
  // not cause a linker error. Instead, they are treated as if they
  // were imported symbols.
  //
  // If we are linking an executable, weak undefs are converted to
  // weakly imported symbol so that they'll have another chance to be
  // resolved.
  claim_unresolved_symbols(ctx);

  // Beyond this point, no new symbols will be added to the result.

  // Handle --print-dependencies
  if (ctx.arg.print_dependencies == 1)
    print_dependencies(ctx);
  else if (ctx.arg.print_dependencies == 2)
    print_dependencies_full(ctx);

  // Handle -repro
  if (ctx.arg.repro)
    write_repro_file(ctx);

  // Handle --require-defined
  for (std::string_view name : ctx.arg.require_defined)
    if (!get_symbol(ctx, name)->file)
      Error(ctx) << "--require-defined: undefined symbol: " << name;

  // .init_array and .fini_array contents have to be sorted by
  // a special rule. Sort them.
  sort_init_fini(ctx);

  // Likewise, .ctors and .dtors have to be sorted. They are rare
  // because they are superceded by .init_array/.fini_array, though.
  sort_ctor_dtor(ctx);

  // Handle --shuffle-sections
  if (ctx.arg.shuffle_sections != SHUFFLE_SECTIONS_NONE)
    shuffle_sections(ctx);

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

  if constexpr (std::is_same_v<E, PPC64V1>)
    ppc64v1_scan_symbols(ctx);

  // Scan relocations to find symbols that need entries in .got, .plt,
  // .got.plt, .dynsym, .dynstr, etc.
  scan_relocations(ctx);

  // Compute sizes of output sections while assigning offsets
  // within an output section to input sections.
  compute_section_sizes(ctx);

  // Sort sections by section attributes so that we'll have to
  // create as few segments as possible.
  sort_output_sections(ctx);

  // If --packed_dyn_relocs=relr was given, base relocations are stored
  // to a .relr.dyn section in a compressed form. Construct a compressed
  // relocations now so that we can fix section sizes and file layout.
  if (ctx.arg.pack_dyn_relocs_relr)
    construct_relr(ctx);

  // Reserve a space for dynamic symbol strings in .dynstr and sort
  // .dynsym contents if necessary. Beyond this point, no symbol will
  // be added to .dynsym.
  ctx.dynsym->finalize(ctx);

  // Print reports about undefined symbols, if needed.
  if (ctx.arg.unresolved_symbols == UNRESOLVED_ERROR)
    report_undef_errors(ctx);

  // Fill .gnu.version_d section contents.
  if (ctx.verdef)
    ctx.verdef->construct(ctx);

  // Fill .gnu.version_r section contents.
  ctx.verneed->construct(ctx);

  // Compute .symtab and .strtab sizes for each file.
  create_output_symtab(ctx);

  // .eh_frame is a special section from the linker's point of view,
  // as its contents are parsed and reconstructed by the linker,
  // unlike other sections that are regarded as opaque bytes.
  // Here, we construct output .eh_frame contents.
  ctx.eh_frame->construct(ctx);

  // Handle --gdb-index.
  if (ctx.arg.gdb_index)
    ctx.gdb_index->construct(ctx);

  // If --emit-relocs is given, we'll copy relocation sections from input
  // files to an output file.
  if (ctx.arg.emit_relocs)
    create_reloc_sections(ctx);

  // Update sh_size for each chunk and remove empty ones.
  for (Chunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  std::erase_if(ctx.chunks, [](Chunk<E> *chunk) {
    return chunk->kind() != OUTPUT_SECTION && chunk->shdr.sh_size == 0;
  });

  // Set section indices.
  for (i64 i = 0, shndx = 1; i < ctx.chunks.size(); i++)
    if (ctx.chunks[i]->kind() != HEADER)
      ctx.chunks[i]->shndx = shndx++;

  // Some types of section header refer other section by index.
  // Recompute the section header to fill such fields with correct values.
  for (Chunk<E> *chunk : ctx.chunks)
    chunk->update_shdr(ctx);

  // Assign offsets to output sections
  i64 filesize = set_osec_offsets(ctx);

  // On RISC-V, branches are encode using multiple instructions so
  // that they can jump to anywhere in ±2 GiB by default. They may
  // be replaced with shorter instruction sequences if destinations
  // are close enough. Do this optimization.
  if constexpr (is_riscv<E>)
    filesize = riscv_resize_sections(ctx);

  // Fix linker-synthesized symbol addresses.
  fix_synthetic_symbols(ctx);

  // Beyond this, you can assume that symbol addresses including their
  // GOT or PLT addresses have a correct final value.

  // If --compress-debug-sections is given, compress .debug_* sections
  // using zlib.
  if (ctx.arg.compress_debug_sections != COMPRESS_NONE)
    filesize = compress_debug_sections(ctx);

  // At this point, file layout is fixed.

  t_before_copy.stop();

  // Create an output file
  ctx.output_file =
    OutputFile<Context<E>>::open(ctx, ctx.arg.output, filesize, 0777);
  ctx.buf = ctx.output_file->buf;

  Timer t_copy(ctx, "copy");

  // Copy input sections to the output file and apply relocations.
  copy_chunks(ctx);

  // Some part of .gdb_index couldn't be computed until other debug
  // sections are complete. We have complete debug sections now, so
  // write the rest of .gdb_index.
  if (ctx.gdb_index)
    ctx.gdb_index->write_address_areas(ctx);

  // Dynamic linker works better with sorted .rela.dyn section,
  // so we sort them.
  ctx.reldyn->sort(ctx);

  // Zero-clear paddings between sections
  clear_padding(ctx);

  if (ctx.buildid)
    ctx.buildid->write_buildid(ctx);

  t_copy.stop();
  ctx.checkpoint();

  // Close the output file. This is the end of the linker's main job.
  ctx.output_file->close(ctx);

  // Handle --dependency-file
  if (!ctx.arg.dependency_file.empty())
    write_dependency_file(ctx);

  if (ctx.has_lto_object)
    lto_cleanup(ctx);

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
  ctx.checkpoint();
  return 0;
}

using E = MOLD_TARGET;

template void read_file(Context<E> &, MappedFile<Context<E>> *);

#ifdef MOLD_X86_64

extern template int elf_main<I386>(int, char **);
extern template int elf_main<ARM32>(int, char **);
extern template int elf_main<ARM64>(int, char **);
extern template int elf_main<RV32BE>(int, char **);
extern template int elf_main<RV32LE>(int, char **);
extern template int elf_main<RV64LE>(int, char **);
extern template int elf_main<RV64BE>(int, char **);
extern template int elf_main<PPC64V1>(int, char **);
extern template int elf_main<PPC64V2>(int, char **);
extern template int elf_main<S390X>(int, char **);
extern template int elf_main<SPARC64>(int, char **);

int main(int argc, char **argv) {
  return elf_main<X86_64>(argc, argv);
}

#else

template int elf_main<E>(int, char **);

#endif

} // namespace mold::elf
