#include "mold.h"
#include "../archive-file.h"
#include "../cmdline.h"

#include <cstdlib>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

static std::pair<std::string_view, std::string_view>
split_string(std::string_view str, char sep) {
  size_t pos = str.find(sep);
  if (pos == str.npos)
    return {str, ""};
  return {str.substr(0, pos), str.substr(pos + 1)};
}

template <typename E>
static void create_internal_file(Context<E> &ctx) {
  ObjectFile<E> *obj = new ObjectFile<E>;
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
    sym->is_extern = true;
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
}

template <typename E>
static bool compare_segments(const std::unique_ptr<OutputSegment<E>> &a,
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
    return INT_MAX;
  };

  std::string_view na = a->cmd.get_segname();
  std::string_view nb = b->cmd.get_segname();
  i64 ra = get_rank(na);
  i64 rb = get_rank(nb);
  if (ra != INT_MAX || rb != INT_MAX)
    return ra < rb;

  if (na == "__LINKEDIT" || nb == "__LINKEDIT")
    return na != "__LINKEDIT";
  return na < nb;
}

template <typename E>
static bool compare_chunks(const Chunk<E> *a, const Chunk<E> *b) {
  assert(a->hdr.get_segname() == b->hdr.get_segname());

  if ((a->hdr.type == S_ZEROFILL) != (b->hdr.type == S_ZEROFILL))
    return a->hdr.type != S_ZEROFILL;

  static const std::string_view rank[] = {
    // __TEXT
    "__mach_header",
    "__text",
    "__stubs",
    "__stub_helper",
    "__gcc_except_tab",
    "__cstring",
    "__unwind_info",
    // __DATA_CONST
    "__got",
    // __DATA
    "__la_symbol_ptr",
    "__data",
    "__thread_ptr",
    "__thread_data",
    "__thread_vars",
    "__thread_bss",
    "__common",
    "__bss",
    // __LINKEDIT
    "__rebase",
    "__binding",
    "__weak_binding",
    "__lazy_binding",
    "__export",
    "__func_starts",
    "__data_in_code",
    "__symbol_table",
    "__ind_sym_tab",
    "__string_table",
    "__code_signature",
  };

  auto get_rank = [](const Chunk<E> *chunk) -> i64 {
    std::string_view name = chunk->hdr.get_sectname();
    i64 i = 0;
    for (; i < sizeof(rank) / sizeof(rank[0]); i++)
      if (name == rank[i])
        return i;
    return INT_MAX;
  };

  i64 ra = get_rank(a);
  i64 rb = get_rank(b);
  if (ra == INT_MAX && rb == INT_MAX)
    return a->hdr.get_sectname() < b->hdr.get_sectname();
  return ra < rb;
}

template <typename E>
static void create_synthetic_chunks(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    for (std::unique_ptr<Subsection<E>> &subsec : file->subsections)
      subsec->isec.osec.add_subsec(subsec.get());

  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk != ctx.data && chunk->is_regular &&
        ((OutputSection<E> *)chunk)->members.empty())
      continue;

    OutputSegment<E> *seg =
      OutputSegment<E>::get_instance(ctx, chunk->hdr.get_segname());
    seg->chunks.push_back(chunk);
  }

  sort(ctx.segments, compare_segments<E>);

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    sort(seg->chunks, compare_chunks<E>);
}

template <typename E>
static void scan_unwind_info(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    for (UnwindRecord<E> &rec : file->unwind_records)
      if (!ctx.arg.dead_strip || rec.is_alive)
        if (rec.personality)
          rec.personality->flags |= NEEDS_GOT;
}

template <typename E>
static void export_symbols(Context<E> &ctx) {
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

template <typename E>
static void fix_synthetic_symbol_values(Context<E> &ctx) {
  get_symbol(ctx, "__dyld_private")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "__mh_dylib_header")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "__mh_bundle_header")->value = ctx.data->hdr.addr;
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
  for (std::string dir : ctx.arg.library_paths) {
    for (std::string ext : {".tbd", ".dylib", ".a"}) {
      std::string path = dir + "/lib" + name + ext;
      if (MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path))
        return mf;
    }
  }
  return nullptr;
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
static void read_file(Context<E> &ctx, MappedFile<Context<E>> *mf,
                      bool is_needed = false) {
  if (get_file_type(mf) == FileType::MACH_UNIVERSAL)
    mf = strip_universal_header(ctx, mf);

  switch (get_file_type(mf)) {
  case FileType::TAPI:
  case FileType::MACH_DYLIB:
    ctx.dylibs.push_back(DylibFile<E>::create(ctx, mf));
    if (is_needed || !ctx.arg.dead_strip_dylibs)
      ctx.dylibs.back()->is_needed = true;
    break;
  case FileType::MACH_OBJ:
    ctx.objs.push_back(ObjectFile<E>::create(ctx, mf, ""));
    break;
  case FileType::AR:
    for (MappedFile<Context<E>> *child : read_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::MACH_OBJ)
        ctx.objs.push_back(ObjectFile<E>::create(ctx, child, mf->name));
    break;
  default:
    break;
    // Fatal(ctx) << mf->name << ": unknown file type";
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
  while (!args.empty()) {
    if (args[0].starts_with("-filelist")) {
      for (std::string &path : read_filelist(ctx, args[1])) {
        MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path);
        if (!mf)
          Fatal(ctx) << "-filepath " << args[1] << ": cannot open file: " << path;
        read_file(ctx, mf);
      }
      args = args.subspan(2);
    } else if (args[0] == "-framework" || args[0] == "-needed_framework") {
      bool needed = (args[0] == "-needed_framework");
      read_file(ctx, find_framework(ctx, args[1]), needed);
      args = args.subspan(2);
    } else if (args[0] == "-l" || args[0] == "-needed-l") {
      MappedFile<Context<E>> *mf = find_library(ctx, args[1]);
      if (!mf)
        Fatal(ctx) << "library not found: -l" << args[1];
      bool needed = (args[0] == "-needed-l");
      read_file(ctx, mf, needed);
      args = args.subspan(2);
    } else {
      read_file(ctx, MappedFile<Context<E>>::must_open(ctx, args[0]));
      args = args.subspan(1);
    }
  }

  if (ctx.objs.empty())
    Fatal(ctx) << "no input files";
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

  std::vector<std::string> file_args;
  parse_nonpositional_args(ctx, file_args);

  if (ctx.arg.arch != E::cputype) {
    if (ctx.arg.arch == CPU_TYPE_X86_64)
      return do_main<X86_64>(argc, argv);
  }

  read_input_files(ctx, file_args);

  i64 priority = 1;
  for (ObjectFile<E> *file : ctx.objs)
    file->priority = priority++;
  for (DylibFile<E> *dylib : ctx.dylibs)
    dylib->priority = priority++;

  for (i64 i = 0; i < ctx.dylibs.size(); i++)
    ctx.dylibs[i]->dylib_idx = i + 1;

  for (ObjectFile<E> *file : ctx.objs)
    file->parse(ctx);
  for (DylibFile<E> *dylib : ctx.dylibs)
    dylib->parse(ctx);

  if (ctx.arg.ObjC)
    for (ObjectFile<E> *file : ctx.objs)
      if (!file->archive_name.empty() && file->is_objc_object(ctx))
        file->is_alive = true;

  for (ObjectFile<E> *file : ctx.objs) {
    if (file->is_alive)
      file->resolve_regular_symbols(ctx);
    else
      file->resolve_lazy_symbols(ctx);
  }

  std::vector<ObjectFile<E> *> live_objs;
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_alive)
      live_objs.push_back(file);

  for (i64 i = 0; i < live_objs.size(); i++)
    append(live_objs, live_objs[i]->mark_live_objects(ctx));

  for (DylibFile<E> *dylib : ctx.dylibs)
    dylib->resolve_symbols(ctx);

  if (ctx.output_type == MH_EXECUTE && !get_symbol(ctx, ctx.arg.entry)->file)
    Error(ctx) << "undefined entry point symbol: " << ctx.arg.entry;

  create_internal_file(ctx);

  std::erase_if(ctx.objs, [](ObjectFile<E> *file) { return !file->is_alive; });
  std::erase_if(ctx.dylibs, [](DylibFile<E> *file) { return !file->is_alive; });

  if (ctx.arg.trace) {
    for (ObjectFile<E> *file : ctx.objs)
      SyncOut(ctx) << *file;
    for (DylibFile<E> *file : ctx.dylibs)
      SyncOut(ctx) << *file;
  }

  for (ObjectFile<E> *file : ctx.objs)
    file->convert_common_symbols(ctx);

  if (ctx.arg.dead_strip)
    dead_strip(ctx);

  create_synthetic_chunks(ctx);

  for (ObjectFile<E> *file : ctx.objs)
    file->check_duplicate_symbols(ctx);

  for (i64 i = 0; i < ctx.segments.size(); i++)
    ctx.segments[i]->seg_idx = i + 1;

  for (ObjectFile<E> *file : ctx.objs)
    for (std::unique_ptr<Subsection<E> > &subsec : file->subsections)
      subsec->scan_relocations(ctx);

  scan_unwind_info(ctx);

  if (ctx.arg.dead_strip_dylibs)
    std::erase_if(ctx.dylibs, [](DylibFile<E> *file) { return !file->is_needed; });

  export_symbols(ctx);
  i64 output_size = assign_offsets(ctx);
  fix_synthetic_symbol_values(ctx);

  ctx.output_file = OutputFile<E>::open(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    seg->copy_buf(ctx);
  ctx.code_sig.write_signature(ctx);

  ctx.output_file->close(ctx);
  ctx.checkpoint();

  if (!ctx.arg.map.empty())
    print_map(ctx);
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

  return do_main<ARM64>(argc, argv);
}

}
