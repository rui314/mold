#include "mold.h"
#include "config.h"

#include <cstring>
#include <filesystem>
#include <functional>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/parallel_for_each.h>
#include <unordered_set>

namespace mold {

template <typename E>
static void
check_file_compatibility(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf) {
  std::string_view target = get_machine_type(ctx, rctx, mf);
  if (target.empty())
    Fatal(ctx) << mf->name << ": unknown machine type";
  if (target != ctx.arg.emulation)
    Fatal(ctx) << mf->name << ": incompatible file type: "
               << ctx.arg.emulation << " is expected but got " << target;
}

template <typename E>
static void new_object_file(Context<E> &ctx, ReaderContext &rctx,
                            MappedFile *mf, std::string archive_name) {
  static Counter count("parsed_objs");
  count++;

  check_file_compatibility(ctx, rctx, mf);

  ObjectFile<E> *file =
    ctx.arena.template make<ObjectFile<E>>(ctx, mf, archive_name);
  ctx.obj_pool.emplace_back(file);
  file->as_needed =
    rctx.in_lib || (!archive_name.empty() && !rctx.whole_archive);

  file->parse_symbols(ctx);
  ctx.unsorted_input_files.push_back({rctx.pos, file});
}

template <typename E>
static void new_lto_obj(Context<E> &ctx, ReaderContext &rctx,
                        MappedFile *mf, std::string archive_name) {
  static Counter count("parsed_lto_objs");
  count++;

  if (ctx.arg.ignore_ir_file.count(mf->get_identifier()))
    return;

  ObjectFile<E> *file = read_lto_object(ctx, mf);
  if (!file)
    return;

  file->archive_name = archive_name;
  file->as_needed =
    rctx.in_lib || (!archive_name.empty() && !rctx.whole_archive);

  ctx.unsorted_input_files.push_back({rctx.pos, file});
}

template <typename E>
static void new_shared_file(Context<E> &ctx, ReaderContext &rctx,
                            MappedFile *mf) {
  if (rctx.static_)
    Fatal(ctx) << mf->name << ": attempted static link of a dynamic object";

  check_file_compatibility(ctx, rctx, mf);

  SharedFile<E> *file = ctx.arena.template make<SharedFile<E>>(ctx, mf);
  ctx.dso_pool.emplace_back(file);
  file->as_needed = rctx.as_needed;

  file->parse(ctx);
  ctx.unsorted_input_files.push_back({rctx.pos, file});
}

// Reads a file inside an archive.
template <typename E>
static void read_archive_member(Context<E> &ctx, ReaderContext &rctx,
                                MappedFile *mf, std::string archive_name) {
  switch (get_file_type(ctx, mf)) {
  case FileType::ELF_OBJ:
    new_object_file(ctx, rctx, mf, archive_name);
    break;
  case FileType::GCC_LTO_OBJ:
  case FileType::LLVM_BITCODE:
    new_lto_obj(ctx, rctx, mf, archive_name);
    break;
  case FileType::ELF_DSO:
    Warn(ctx) << archive_name << "(" << mf->name
              << "): shared object file in an archive is ignored";
    break;
  default:
    break;
  }
}

// Reads the given file, which is located at rctx.pos in the command
// line. If the file is a container, i.e. an archive file or a linker
// script, the files in it are read in place, recursively.
//
// read_input_files() reads top-level files with this function too but
// overrides the container cases to read archive members in parallel.
template <typename E>
void read_file(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf) {
  switch (get_file_type(ctx, mf)) {
  case FileType::ELF_OBJ:
    new_object_file(ctx, rctx, mf, "");
    return;
  case FileType::ELF_DSO:
    new_shared_file(ctx, rctx, mf);
    return;
  case FileType::AR:
  case FileType::THIN_AR:
    for (MappedFile *child : read_archive_members(ctx, mf)) {
      ReaderContext rctx2 = rctx.next_child();
      read_archive_member(ctx, rctx2, child, mf->name);
    }
    return;
  case FileType::TEXT:
    Script(ctx, rctx, mf).parse_linker_script();
    return;
  case FileType::GCC_LTO_OBJ:
  case FileType::LLVM_BITCODE:
    new_lto_obj(ctx, rctx, mf, "");
    return;
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
  }
}

template <typename E>
static std::string_view
detect_machine_type(Context<E> &ctx, std::span<ReaderJob> jobs) {
  for (ReaderJob &job : jobs)
    if (!job.is_lib)
      if (MappedFile *mf = open_file(ctx, job.name))
        if (get_file_type(ctx, mf) != FileType::TEXT)
          if (std::string_view target = get_machine_type(ctx, job.rctx, mf);
              !target.empty())
            return target;

  for (ReaderJob &job : jobs)
    if (!job.is_lib)
      if (MappedFile *mf = open_file(ctx, job.name))
        if (get_file_type(ctx, mf) == FileType::TEXT)
          if (std::string_view target =
              Script(ctx, job.rctx, mf).get_script_output_type();
              !target.empty())
            return target;

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

// Reads all input files.
//
// Reading input files is I/O- and CPU-intensive, and a large program
// can easily consist of tens of thousands of them, so we want to read
// files in parallel. The command line, on the other hand, is
// inherently sequential: options such as --as-needed or
// --whole-archive apply to the files after them, and a file's
// priority for symbol resolution is its position in the command line.
//
// We reconcile the two as follows: the command line parser has
// already recorded the reader state and the position for each file
// argument in its ReaderJob. We open and read files in parallel and
// then sort the files we've found back into the command line order to
// assign priorities.
template <typename E>
static void read_input_files(Context<E> &ctx, std::vector<ReaderJob> &jobs) {
  Timer t(ctx, "read_input_files");

  // Open and read files in parallel. Archive files are expanded into
  // one job per member so that members are read in parallel too.
  //
  // Linker scripts are the exception to the parallelism: they can
  // modify the context, e.g. by defining symbol versions, so we only
  // collect them here and parse them after this loop, one at a time
  // and in the command line order, to keep their effects
  // deterministic. Scripts given as input files are rare and small,
  // such as the GROUP file that glibc installs as libc.so, so the
  // lost parallelism doesn't matter.
  //
  // Parsing scripts late assumes that no script directive affects how
  // command line arguments after the script are read. That holds for
  // the directives we currently support: a script can only add input
  // files, whose positions order them correctly, and define symbol
  // versions or symbols, which are not used until after this
  // function. If we add a directive that doesn't satisfy this, such
  // as SEARCH_DIR, which affects how -l arguments after it are
  // resolved, this scheme needs to be revisited.
  tbb::concurrent_vector<ReaderJob> scripts;

  tbb::parallel_for_each(jobs, [&](ReaderJob &job,
                                   tbb::feeder<ReaderJob> &feeder) {
    ReaderContext &rctx = job.rctx;

    // An archive member is enqueued in an already-opened form by the
    // job that read its archive file.
    if (!job.archive_name.empty()) {
      read_archive_member(ctx, rctx, job.mf, job.archive_name);
      return;
    }

    // Everything else is a command line argument that we need to open.
    MappedFile *mf;
    if (job.is_lib) {
      mf = find_library(ctx, rctx, job.name);
      mf->given_fullpath = false;
    } else {
      mf = must_open_file(ctx, job.name);
    }

    switch (get_file_type(ctx, mf)) {
    case FileType::AR:
    case FileType::THIN_AR:
      for (MappedFile *child : read_archive_members(ctx, mf)) {
        ReaderJob job2;
        job2.rctx = rctx.next_child();
        job2.mf = child;
        job2.archive_name = mf->name;
        feeder.add(std::move(job2));
      }
      break;
    case FileType::TEXT:
      job.mf = mf;
      scripts.push_back(std::move(job));
      break;
    default:
      read_file(ctx, rctx, mf);
    }
  });

  // Parse linker scripts and read the files they name.
  ranges::sort(scripts, {}, [](const ReaderJob &job) {
    return job.rctx.pos;
  });

  for (ReaderJob &job : scripts)
    Script(ctx, job.rctx, job.mf).parse_linker_script();

  // Sort the files into the command line order and assign priorities.
  ranges::sort(ctx.unsorted_input_files, {},
               &std::pair<std::vector<u32>, InputFile<E> *>::first);

  for (auto &pair : ctx.unsorted_input_files) {
    InputFile<E> *file = pair.second;
    file->priority = ctx.file_priority++;
    if (ctx.arg.trace)
      Out(ctx) << "trace: " << *file;

    if (file->is_dso)
      ctx.dsos.push_back(file->to_dso());
    else
      ctx.objs.push_back(file->to_obj());
  }

  ctx.unsorted_input_files.clear();

  if (ctx.objs.empty() && ctx.dsos.empty())
    Fatal(ctx) << "no input files";
}

template <typename E>
static bool has_lto_obj(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_reachable && (file->is_lto_obj || file->is_gcc_offload_obj))
      return true;
  return false;
}

template <typename E>
static i64 get_thread_count(Context<E> &ctx) {
  if (ctx.arg.thread_count.has_value())
    return *ctx.arg.thread_count;

  // mold doesn't scale well with too many threads, so limit it to 32.
  int n = tbb::global_control::active_value(
    tbb::global_control::max_allowed_parallelism);
  return std::min(n, 32);
}

template <typename E>
int mold_main(int argc, char **argv) {
  Context<E> ctx;

  // Process -run option first. process_run_subcommand() does not return.
  if (argc >= 2 && (argv[1] == "-run"sv || argv[1] == "--run"sv))
    process_run_subcommand(ctx, argc, argv);

  // parse_nonpositional_args() may chdir(2) for -C. If we end up
  // restarting in redo_main(), we need to re-enter from the original
  // directory so relative paths (e.g. response files) still resolve.
  std::error_code ec;
  std::filesystem::path orig_cwd = std::filesystem::current_path(ec);

  // Parse non-positional command line options
  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<ReaderJob> jobs = parse_nonpositional_args(ctx);

  // If no -m option is given, deduce it from input files.
  if (ctx.arg.emulation.empty())
    ctx.arg.emulation = detect_machine_type(ctx, jobs);

  // Redo if -m does not match with our speculation.
  if (ctx.arg.emulation != E::name) {
    if (!ec)
      std::filesystem::current_path(orig_cwd, ec);
    return redo_main<E>(ctx.arg.emulation, argc, argv);
  }

  Timer t_all(ctx, "all");

  install_signal_handler();

  // Fork a subprocess unless --no-fork is given.
  if (ctx.arg.fork)
    fork_child<E>();

  acquire_global_lock();

  ctx.global_limit.emplace(tbb::global_control::max_allowed_parallelism,
                           get_thread_count(ctx));

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
  read_input_files(ctx, jobs);

  // Uniquify shared object files by soname
  {
    std::unordered_set<std::string_view> seen;
    std::erase_if(ctx.dsos, [&](SharedFile<E> *file) {
      return !seen.insert(file->soname).second;
    });
  }

  // Handle -repro
  if (ctx.arg.repro)
    write_repro_file(ctx);

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
  std::erase_if(ctx.objs, [](InputFile<E> *file) { return !file->is_reachable; });
  std::erase_if(ctx.dsos, [](InputFile<E> *file) { return !file->is_reachable; });

  // Building .gdb_index is split into three stages because the required data
  // becomes available at different points in the link. Compilation units and
  // public names depend only on input sections, so read them now in a
  // low-priority arena while foreground passes continue.
  bool create_gdb_index = ctx.arg.gdb_index && !ctx.arg.relocatable;
  tbb::task_arena gdb_input_arena(tbb::task_arena::automatic, 1,
                                  tbb::task_arena::priority::low);
  tbb::task_group gdb_task;
  if (create_gdb_index)
    gdb_input_arena.execute([&] {
      gdb_task.run([&] { read_gdb_index_inputs(ctx); });
    });

  // Parse .eh_frame section contents.
  parse_eh_frame_sections(ctx);

  // Parse .sframe section contents.
  parse_sframe_sections(ctx);

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

  // Make sure that there's no duplicate symbol
  if (!ctx.arg.allow_multiple_definition)
    check_duplicate_symbols(ctx);

  // Handle --zero-to-bss, which converts data sections containing only
  // zeros into BSS.
  if (ctx.arg.zero_to_bss)
    convert_zero_to_bss(ctx);

  // Set "address-taken" bits for input sections.
  if (ctx.arg.icf)
    compute_address_significance(ctx);

  // Handle PPC64-specific .opd sections.
  if constexpr (is_ppc64v1<E>)
    ppc64v1_rewrite_opd(ctx);

  // Garbage-collect unreachable sections.
  if (ctx.arg.gc_sections)
    gc_sections(ctx);

  // Merge identical read-only sections.
  if (ctx.arg.icf)
    icf_sections(ctx);

  // Create linker-synthesized sections such as .got or .plt.
  create_synthetic_sections(ctx);

  // Handle --no-allow-shlib-undefined
  if (!ctx.arg.allow_shlib_undefined)
    check_shlib_undefined(ctx);

  // Warn if symbols with different types are defined under the same name.
  check_symbol_types(ctx);

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
  add_dynamic_strings(ctx);

  if constexpr (is_ppc64v1<E>)
    ppc64v1_scan_symbols(ctx);

  // Scan relocations to find symbols that need entries in .got, .plt,
  // .got.plt, .dynsym, .dynstr, etc.
  scan_relocations(ctx);

  // Now that we know all exported symbols, make sure that no versioned
  // name is defined twice.
  check_symbol_version_conflicts(ctx);

  // Compute the is_weak bit for each imported symbol.
  compute_imported_symbol_weakness(ctx);

  // Sort sections by section attributes so that we'll have to
  // create as few segments as possible.
  sort_output_sections(ctx);

  // Handle --separate-debug-file.
  if (ctx.gnu_debuglink)
    separate_debug_sections(ctx);

  // Compute sizes of output sections while assigning offsets
  // within an output section to input sections.
  compute_section_sizes(ctx);

  // RELR is encoded independently for each output chunk using offsets
  // relative to that chunk.
  if (ctx.arg.pack_dyn_relocs_relr)
    ctx.reldyn->construct_relr(ctx);

  // Reserve a space for dynamic symbol strings in .dynstr and sort
  // .dynsym contents if necessary. Beyond this point, no symbol will
  // be added to .dynsym.
  sort_dynsyms(ctx);

  // sort_debug_info_sections may uncompress the same .debug_info sections.
  if (create_gdb_index)
    gdb_input_arena.execute([&] { gdb_task.wait(); });

  // Sort .debug_info contents so that DWARF32 debug info precedes that of
  // DWARF64. This is to mitigate the possibility of a relocation overflow.
  sort_debug_info_sections(ctx);

  // Type vectors identify compilation units by their order in the output
  // .debug_info section. That order is now fixed, so build the table while the
  // remaining layout passes continue. This stage stops scaling after about 12
  // workers, so limit its arena to avoid competing with foreground work.
  i64 gdb_table_workers =
    std::clamp<i64>(get_thread_count(ctx) * 3 / 8, 1, 12);
  tbb::task_arena gdb_table_arena(gdb_table_workers, 1,
                                  tbb::task_arena::priority::low);
  if (ctx.gdb_index && !ctx.gnu_debuglink)
    gdb_table_arena.execute([&] {
      gdb_task.run([&] { build_gdb_index_tables(ctx); });
    });

  // Print reports about undefined symbols, if needed.
  if (ctx.arg.unresolved_symbols == UNRESOLVED_ERROR)
    report_undef_errors(ctx);

  // Fill .gnu.version_d section contents.
  if (ctx.verdef)
    ctx.verdef->construct(ctx);

  // Fill .gnu.version_r section contents.
  ctx.verneed->construct(ctx);

  // .eh_frame is a special section from the linker's point of view,
  // as its contents are parsed and reconstructed by the linker,
  // unlike other sections that are regarded as opaque bytes.
  // Here, we construct output .eh_frame contents.
  ctx.eh_frame->construct(ctx);

  // .sframe is likewise parsed and reconstructed by the linker. Build
  // the merged, PC-sorted output .sframe.
  if constexpr (supports_sframe<E>)
    ctx.sframe->construct(ctx);

  // If --emit-relocs is given, we'll copy relocation sections from input
  // files to an output file.
  if (ctx.arg.emit_relocs)
    create_reloc_sections(ctx);

  // Compute .symtab and .strtab sizes for each file.
  if (!ctx.arg.strip_all)
    create_output_symtab(ctx);

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

  // We've created range extension thunks with a pessimistive assumption
  // that all out-of-section references are out of range. Now that we know
  // the addresses of all sections,, we can eliminate excessive thunks.
  if constexpr (needs_thunk<E>) {
    remove_redundant_thunks(ctx);
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
  // using zlib or zstd.
  if (ctx.arg.compress_debug_sections != ELFCOMPRESS_NONE) {
    compress_debug_sections(ctx);
    filesize = set_osec_offsets(ctx);
  }

  // Gather thunk symbols and attach them to themselves.
  if constexpr (needs_thunk<E>)
    gather_thunk_addresses(ctx);

  // Re-finalize layout. fix_synthetic_symbols above may have changed
  // addends for dynamic relocations referencing synthetic symbols, which
  // can shift the encoded size of .rela.dyn under --pack-dyn-relocs=android
  // because Android's packed format encodes addends in variable-length
  // SLEB128. Other modes, including ordinary RELR, encode nothing whose
  // size depends on addends, so they do not need this pass.
  if (ctx.arg.pack_dyn_relocs_android)
    filesize = set_osec_offsets(ctx);

  // At this point, both memory and file layouts are fixed.

  ctx.reldyn->update_shdr(ctx);

  t_before_copy.stop();

  // Create an output file
  ctx.output_file = OutputFile<E>::open(ctx, ctx.arg.output, filesize, 0777);
  ctx.buf = ctx.output_file->buf;

  Timer t_copy(ctx, "copy");

  // Copy input sections to the output file and apply relocations.
  copy_chunks(ctx);

  if constexpr (is_arm32be<E>)
    arm32be_swap_bytes(ctx);

  if constexpr (is_x86_64<E>)
    if (ctx.arg.z_rewrite_endbr)
      rewrite_endbr(ctx);

  // Dynamic linker works better with sorted .rela.dyn section,
  // so we sort them.
  sort_reldyn(ctx);

  // The final stage reads address ranges, which requires relocated debug
  // sections. We have applied the relocations now, so finish the index.
  if (ctx.gdb_index && !ctx.gnu_debuglink) {
    gdb_table_arena.execute([&] { gdb_task.wait(); });
    write_gdb_index(ctx);
  }

  // .note.gnu.build-id section contains a cryptographic hash of the
  // entire output file. Now that we wrote everything except build-id,
  // we can compute it.
  if (ctx.buildid)
    write_build_id(ctx);

  if (ctx.gnu_debuglink)
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

  if (ctx.gnu_debuglink)
    write_separate_debug_file(ctx);

  // Show stats numbers
  if (ctx.arg.stats)
    show_stats(ctx);

  if (ctx.arg.perf)
    print_timer_records(ctx.timer_records);

  std::cout << std::flush;
  std::cerr << std::flush;

  notify_parent<E>();
  release_global_lock();

#if HAVE_MADVISE
  // Dropping page table entries here in parallel makes process exit
  // faster, as the kernel otherwise reclaims them in a single thread
  // on exit. File contents stay in the page cache.
  tbb::parallel_for_each(ctx.mf_pool, [](std::unique_ptr<MappedFile> &mf) {
    if (!mf->parent && mf->data && mf->size)
      madvise(mf->data, mf->size, MADV_DONTNEED);
  });
#endif

  if (ctx.arg.quick_exit)
    _exit(0);

  ctx.checkpoint();
  return 0;
}

using E = MOLD_TARGET;

template int mold_main<E>(int, char **);
template void read_file(Context<E> &, ReaderContext &, MappedFile *);

} // namespace mold
