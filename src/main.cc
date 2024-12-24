#include "mold.h"
#include "../lib/archive-file.h"

#include <cstring>
#include <functional>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/global_control.h>
#include <unordered_set>

#ifdef _WIN32
# include <direct.h>
# define chdir _chdir
#else
# include <unistd.h>
#endif

#ifdef MOLD_X86_64
int main(int argc, char **argv) {
  mold::set_mimalloc_options();
  return mold::mold_main<mold::X86_64>(argc, argv);
}
#endif

namespace mold {

template <typename E>
static void
check_file_compatibility(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf) {
  std::string_view target = get_machine_type(ctx, rctx, mf);
  if (target != ctx.arg.emulation)
    Fatal(ctx) << mf->name << ": incompatible file type: "
               << ctx.arg.emulation << " is expected but got " << target;
}

template <typename E>
static ObjectFile<E> *new_object_file(Context<E> &ctx, ReaderContext &rctx,
                                      MappedFile *mf, std::string archive_name) {
  static Counter count("parsed_objs");
  count++;

  check_file_compatibility(ctx, rctx, mf);

  bool in_lib = rctx.in_lib || (!archive_name.empty() && !rctx.whole_archive);

  ObjectFile<E> *file = new ObjectFile<E>(ctx, mf, archive_name, in_lib);
  ctx.obj_pool.emplace_back(file);
  file->priority = ctx.file_priority++;

  rctx.tg->run([file, &ctx] { file->parse(ctx); });
  if (ctx.arg.trace)
    Out(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
static ObjectFile<E> *new_lto_obj(Context<E> &ctx, ReaderContext &rctx,
                                  MappedFile *mf, std::string archive_name) {
  static Counter count("parsed_lto_objs");
  count++;

  if (ctx.arg.ignore_ir_file.count(mf->get_identifier()))
    return nullptr;

  ObjectFile<E> *file = read_lto_object(ctx, mf);
  file->priority = ctx.file_priority++;
  file->archive_name = archive_name;
  file->is_in_lib = rctx.in_lib || (!archive_name.empty() && !rctx.whole_archive);
  file->is_alive = !file->is_in_lib;
  if (ctx.arg.trace)
    Out(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
static SharedFile<E> *
new_shared_file(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf) {
  check_file_compatibility(ctx, rctx, mf);

  SharedFile<E> *file = new SharedFile<E>(ctx, mf);
  ctx.dso_pool.emplace_back(file);
  file->priority = ctx.file_priority++;
  file->is_alive = !rctx.as_needed;

  rctx.tg->run([file, &ctx] { file->parse(ctx); });
  if (ctx.arg.trace)
    Out(ctx) << "trace: " << *file;
  return file;
}

template <typename E>
void read_file(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf) {
  switch (get_file_type(ctx, mf)) {
  case FileType::ELF_OBJ:
    ctx.objs.push_back(new_object_file(ctx, rctx, mf, ""));
    return;
  case FileType::ELF_DSO:
    ctx.dsos.push_back(new_shared_file(ctx, rctx, mf));
    return;
  case FileType::AR:
  case FileType::THIN_AR:
    for (MappedFile *child : read_archive_members(ctx, mf)) {
      switch (get_file_type(ctx, child)) {
      case FileType::ELF_OBJ:
        ctx.objs.push_back(new_object_file(ctx, rctx, child, mf->name));
        break;
      case FileType::GCC_LTO_OBJ:
      case FileType::LLVM_BITCODE:
        if (ObjectFile<E> *file = new_lto_obj(ctx, rctx, child, mf->name))
          ctx.objs.push_back(file);
        break;
      case FileType::ELF_DSO:
        Warn(ctx) << mf->name << "(" << child->name
                  << "): shared object file in an archive is ignored";
        break;
      default:
        break;
      }
    }
    return;
  case FileType::TEXT:
    Script(ctx, rctx, mf).parse_linker_script();
    return;
  case FileType::GCC_LTO_OBJ:
  case FileType::LLVM_BITCODE:
    if (ObjectFile<E> *file = new_lto_obj(ctx, rctx, mf, ""))
      ctx.objs.push_back(file);
    return;
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
  }
}

template <typename E>
static std::string_view
detect_machine_type(Context<E> &ctx, std::vector<std::string> args) {
  for (ReaderContext rctx; const std::string &arg : args) {
    if (arg == "--Bstatic") {
      rctx.static_ = true;
    } else if (arg == "--Bdynamic") {
      rctx.static_ = false;
    } else if (!arg.starts_with('-')) {
      if (MappedFile *mf = open_file(ctx, arg))
        if (get_file_type(ctx, mf) != FileType::TEXT)
          if (std::string_view target = get_machine_type(ctx, rctx, mf);
              !target.empty())
            return target;
    }
  }

  for (ReaderContext rctx; const std::string &arg : args) {
    if (arg == "--Bstatic") {
      rctx.static_ = true;
    } else if (arg == "--Bdynamic") {
      rctx.static_ = false;
    } else if (!arg.starts_with('-')) {
      if (MappedFile *mf = open_file(ctx, arg))
        if (get_file_type(ctx, mf) == FileType::TEXT)
          if (std::string_view target =
              Script(ctx, rctx, mf).get_script_output_type();
              !target.empty())
            return target;
    }
  }

  Fatal(ctx) << "-m option is missing";
}

template <typename E>
MappedFile *open_library(Context<E> &ctx, ReaderContext &rctx, std::string path) {
  MappedFile *mf = open_file(ctx, path);
  if (!mf)
    return nullptr;

  std::string_view target = get_machine_type(ctx, rctx, mf);
  if (!target.empty() && target != E::name) {
    Warn(ctx) << path << ": skipping incompatible file: " << target
              << " (e_machine " << (int)E::e_machine << ")";
    return nullptr;
  }
  return mf;
}

template <typename E>
MappedFile *find_library(Context<E> &ctx, ReaderContext &rctx, std::string name) {
  if (name.starts_with(':')) {
    for (std::string_view dir : ctx.arg.library_paths) {
      std::string path = std::string(dir) + "/" + name.substr(1);
      if (MappedFile *mf = open_library(ctx, rctx, path))
        return mf;
    }
    Fatal(ctx) << "library not found: " << name;
  }

  for (std::string_view dir : ctx.arg.library_paths) {
    std::string stem = std::string(dir) + "/lib" + name;
    if (!rctx.static_)
      if (MappedFile *mf = open_library(ctx, rctx, stem + ".so"))
        return mf;
    if (MappedFile *mf = open_library(ctx, rctx, stem + ".a"))
      return mf;
  }
  Fatal(ctx) << "library not found: " << name;
}

template <typename E>
static void read_input_files(Context<E> &ctx, std::span<std::string> args) {
  Timer t(ctx, "read_input_files");

  ReaderContext rctx;
  std::vector<ReaderContext> stack;
  std::unordered_set<std::string_view> visited;

  tbb::task_group tg;
  rctx.tg = &tg;

  while (!args.empty()) {
    std::string_view arg = args[0];
    args = args.subspan(1);

    if (arg == "--as-needed") {
      rctx.as_needed = true;
    } else if (arg == "--no-as-needed") {
      rctx.as_needed = false;
    } else if (arg == "--whole-archive") {
      rctx.whole_archive = true;
    } else if (arg == "--no-whole-archive") {
      rctx.whole_archive = false;
    } else if (arg == "--Bstatic") {
      rctx.static_ = true;
    } else if (arg == "--Bdynamic") {
      rctx.static_ = false;
    } else if (arg == "--start-lib") {
      rctx.in_lib = true;
    } else if (arg == "--end-lib") {
      rctx.in_lib = false;
    } else if (arg == "--push-state") {
      stack.push_back(rctx);
    } else if (arg == "--pop-state") {
      if (stack.empty())
        Fatal(ctx) << "no state pushed before popping";
      rctx = stack.back();
      stack.pop_back();
    } else if (arg.starts_with("-l")) {
      arg = arg.substr(2);
      if (visited.contains(arg))
        continue;
      visited.insert(arg);

      MappedFile *mf = find_library(ctx, rctx, std::string(arg));
      mf->given_fullpath = false;
      read_file(ctx, rctx, mf);
    } else {
      read_file(ctx, rctx, must_open_file(ctx, std::string(arg)));
    }
  }

  if (ctx.objs.empty())
    Fatal(ctx) << "no input files";

  tg.wait();
}

template <typename E>
static bool has_lto_obj(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_alive && (file->is_lto_obj || file->is_gcc_offload_obj))
      return true;
  return false;
}

template <typename E>
int mold_main(int argc, char **argv) {
  Context<E> ctx;

  // Process -run option first. process_run_subcommand() does not return.
  if (argc >= 2 && (argv[1] == "-run"sv || argv[1] == "--run"sv))
    process_run_subcommand(ctx, argc, argv);

  // Parse non-positional command line options
  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string> file_args = parse_nonpositional_args(ctx);

  // If no -m option is given, deduce it from input files.
  if (ctx.arg.emulation.empty())
    ctx.arg.emulation = detect_machine_type(ctx, file_args);

  // Redo if -m is not x86-64.
  if constexpr (is_x86_64<E>)
    if (ctx.arg.emulation != X86_64::name)
      return redo_main(ctx, argc, argv);

  Timer t_all(ctx, "all");

  install_signal_handler();

  if (!ctx.arg.directory.empty())
    if (chdir(ctx.arg.directory.c_str()) == -1)
      Fatal(ctx) << "chdir failed: " << ctx.arg.directory
                 << ": " << errno_string();

  // Fork a subprocess unless --no-fork is given.
  if (ctx.arg.fork)
    fork_child();

  acquire_global_lock();

  tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism,
                               ctx.arg.thread_count);

  // Handle --wrap options if any.
  for (std::string_view name : ctx.arg.wrap)
    get_symbol(ctx, name)->is_wrapped = true;

  // Handle --retain-symbols-file options if any.
  if (ctx.arg.retain_symbols_file)
    for (Symbol<E> *sym : *ctx.arg.retain_symbols_file)
      sym->write_to_symtab = true;

  for (std::string_view arg : ctx.arg.trace_symbol)
    get_symbol(ctx, arg)->is_traced = true;

  // Parse input files
  read_input_files(ctx, file_args);

  // Uniquify shared object files by soname
  {
    std::unordered_set<std::string_view> seen;
    std::erase_if(ctx.dsos, [&](SharedFile<E> *file) {
      return !seen.insert(file->soname).second;
    });
  }

  Timer t_before_copy(ctx, "before_copy");

  // Apply -exclude-libs
  apply_exclude_libs(ctx);

  // Create a dummy file containing linker-synthesized symbols.
  if (!ctx.arg.relocatable)
    create_internal_file(ctx);

  // Resolve symbols by choosing the most appropriate file for each
  // symbol. This pass also removes redundant comdat sections (e.g.
  // duplicate inline functions).
  resolve_symbols(ctx);

  // If there's an object file compiled with -flto, do link-time
  // optimization.
  if (has_lto_obj(ctx))
    do_lto(ctx);

  // Now that we know which object files are to be included to the
  // final output, we can remove unnecessary files.
  std::erase_if(ctx.objs, [](InputFile<E> *file) { return !file->is_alive; });
  std::erase_if(ctx.dsos, [](InputFile<E> *file) { return !file->is_alive; });

  // Parse .eh_frame section contents.
  parse_eh_frame_sections(ctx);

  // Split mergeable section contents into section pieces.
  create_merged_sections(ctx);

  // Handle --relocatable. Since the linker's behavior is quite different
  // from the normal one when the option is given, the logic is implemented
  // to a separate file.
  if (ctx.arg.relocatable) {
    combine_objects(ctx);
    return 0;
  }

  // Create .bss sections for common symbols.
  convert_common_symbols(ctx);

  // Apply version scripts.
  apply_version_script(ctx);

  // Parse symbol version suffixes (e.g. "foo@ver1").
  parse_symbol_version(ctx);

  // Set is_imported and is_exported bits for each symbol.
  compute_import_export(ctx);

  // Set "address-taken" bits for input sections.
  if (ctx.arg.icf)
    compute_address_significance(ctx);

  // Garbage-collect unreachable sections.
  if (ctx.arg.gc_sections)
    gc_sections(ctx);

  // Merge identical read-only sections.
  if (ctx.arg.icf)
    icf_sections(ctx);

  // Create linker-synthesized sections such as .got or .plt.
  create_synthetic_sections(ctx);

  // Make sure that there's no duplicate symbol
  if (!ctx.arg.allow_multiple_definition)
    check_duplicate_symbols(ctx);

  if (!ctx.arg.allow_shlib_undefined)
    check_shlib_undefined(ctx);

  // Warn if symbols with different types are defined under the same name.
  check_symbol_types(ctx);

  if constexpr (is_ppc64v1<E>)
    ppc64v1_rewrite_opd(ctx);

  // Bin input sections into output sections.
  create_output_sections(ctx);

  // Convert an .ARM.exidx to a synthetic section.
  if constexpr (is_arm32<E>)
    create_arm_exidx_section(ctx);

  // Handle --section-align options.
  if (!ctx.arg.section_align.empty())
    apply_section_align(ctx);

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
  // weakly imported symbols so that they'll have another chance to be
  // resolved.
  claim_unresolved_symbols(ctx);

  // Beyond this point, no new symbols will be added to the result.

  // Handle --print-dependencies
  if (ctx.arg.print_dependencies)
    print_dependencies(ctx);

  // Handle -repro
  if (ctx.arg.repro)
    write_repro_file(ctx);

  // Handle --require-defined
  for (Symbol<E> *sym : ctx.arg.require_defined)
    if (!sym->file)
      Error(ctx) << "--require-defined: undefined symbol: " << *sym;

  // .init_array and .fini_array contents have to be sorted by
  // a special rule. Sort them.
  sort_init_fini(ctx);

  // Likewise, .ctors and .dtors have to be sorted. They are rare
  // because they are superceded by .init_array/.fini_array, though.
  sort_ctor_dtor(ctx);

  // If .ctors/.dtors are to be placed to .init_array/.fini_array,
  // we need to reverse their contents.
  fixup_ctors_in_init_array(ctx);

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

  if constexpr (is_ppc64v1<E>)
    ppc64v1_scan_symbols(ctx);

  // Scan relocations to find symbols that need entries in .got, .plt,
  // .got.plt, .dynsym, .dynstr, etc.
  scan_relocations(ctx);

  // Compute the is_weak bit for each imported symbol.
  compute_imported_symbol_weakness(ctx);

  // Sort sections by section attributes so that we'll have to
  // create as few segments as possible.
  sort_output_sections(ctx);

  if (!ctx.arg.separate_debug_file.empty())
    separate_debug_sections(ctx);

  // Compute sizes of output sections while assigning offsets
  // within an output section to input sections.
  compute_section_sizes(ctx);

  // If --packed_dyn_relocs=relr was given, base relocations are stored
  // to a .relr.dyn section in a compressed form. Construct a compressed
  // relocations now so that we can fix section sizes and file layout.
  if (ctx.arg.pack_dyn_relocs_relr)
    construct_relr(ctx);

  // Reserve a space for dynamic symbol strings in .dynstr and sort
  // .dynsym contents if necessary. Beyond this point, no symbol will
  // be added to .dynsym.
  sort_dynsyms(ctx);

  // Print reports about undefined symbols, if needed.
  if (ctx.arg.unresolved_symbols == UNRESOLVED_ERROR)
    report_undef_errors(ctx);

  // Fill .gnu.version_d section contents.
  if (ctx.verdef)
    ctx.verdef->construct(ctx);

  // Fill .gnu.version_r section contents.
  ctx.verneed->construct(ctx);

  // Compute .symtab and .strtab sizes for each file.
  if (!ctx.arg.strip_all)
    create_output_symtab(ctx);

  // .eh_frame is a special section from the linker's point of view,
  // as its contents are parsed and reconstructed by the linker,
  // unlike other sections that are regarded as opaque bytes.
  // Here, we construct output .eh_frame contents.
  ctx.eh_frame->construct(ctx);

  // If --emit-relocs is given, we'll copy relocation sections from input
  // files to an output file.
  if (ctx.arg.emit_relocs)
    create_reloc_sections(ctx);

  // Compute the section header values for all sections.
  compute_section_headers(ctx);

  // Assign offsets to output sections
  i64 filesize = set_osec_offsets(ctx);

  // On RISC-V, branches are encode using multiple instructions so
  // that they can jump to anywhere in ±2 GiB by default. They may
  // be replaced with shorter instruction sequences if destinations
  // are close enough. Do this optimization.
  if constexpr (is_riscv<E> || is_loongarch<E>) {
    shrink_sections(ctx);
    filesize = set_osec_offsets(ctx);
  }

  if constexpr (is_arm32<E>) {
    if (ctx.extra.exidx) {
      ctx.extra.exidx->remove_duplicate_entries(ctx);
      filesize = set_osec_offsets(ctx);
    }
  }

  // At this point, memory layout is fixed.

  // Set actual addresses to linker-synthesized symbols.
  fix_synthetic_symbols(ctx);

  // Beyond this, you can assume that symbol addresses including their
  // GOT or PLT addresses have a correct final value.

  // If --compress-debug-sections is given, compress .debug_* sections
  // using zlib.
  if (ctx.arg.compress_debug_sections != COMPRESS_NONE) {
    compress_debug_sections(ctx);
    filesize = set_osec_offsets(ctx);
  }

  // At this point, both memory and file layouts are fixed.

  t_before_copy.stop();

  // Create an output file
  ctx.output_file = OutputFile<E>::open(ctx, ctx.arg.output, filesize, 0777);
  ctx.buf = ctx.output_file->buf;

  Timer t_copy(ctx, "copy");

  // Copy input sections to the output file and apply relocations.
  copy_chunks(ctx);

  if constexpr (is_x86_64<E>)
    if (ctx.arg.z_rewrite_endbr)
      rewrite_endbr(ctx);

  // Dynamic linker works better with sorted .rela.dyn section,
  // so we sort them.
  ctx.reldyn->sort(ctx);

  // .gdb_index's contents cannot be constructed before applying
  // relocations to other debug sections. We have relocated debug
  // sections now, so write the .gdb_index section.
  if (ctx.gdb_index && ctx.arg.separate_debug_file.empty())
    write_gdb_index(ctx);

  // .note.gnu.build-id section contains a cryptographic hash of the
  // entire output file. Now that we wrote everything except build-id,
  // we can compute it.
  if (ctx.buildid)
    write_build_id(ctx);

  if (!ctx.arg.separate_debug_file.empty())
    write_gnu_debuglink(ctx);

  t_copy.stop();
  ctx.checkpoint();

  // Close the output file. This is the end of the linker's main job.
  ctx.output_file->close(ctx);

  // Handle --dependency-file
  if (!ctx.arg.dependency_file.empty())
    write_dependency_file(ctx);

  if (!ctx.arg.plugin.empty())
    lto_cleanup(ctx);

  t_all.stop();

  if (ctx.arg.print_map)
    print_map(ctx);

  if (!ctx.arg.separate_debug_file.empty())
    write_separate_debug_file(ctx);

  // Show stats numbers
  if (ctx.arg.stats)
    show_stats(ctx);

  if (ctx.arg.perf)
    print_timer_records(ctx.timer_records);

  std::cout << std::flush;
  std::cerr << std::flush;

  notify_parent();
  release_global_lock();

  if (ctx.arg.quick_exit)
    _exit(0);

  for (std::function<void()> &fn : ctx.on_exit)
    fn();
  ctx.checkpoint();
  return 0;
}

using E = MOLD_TARGET;

template int mold_main<E>(int, char **);

} // namespace mold
