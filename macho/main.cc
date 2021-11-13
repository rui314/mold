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

static void create_internal_file(Context &ctx) {
  ObjectFile *obj = new ObjectFile;
  ctx.obj_pool.push_back(std::unique_ptr<ObjectFile>(obj));
  ctx.objs.push_back(obj);

  auto add = [&](std::string_view name) {
    Symbol *sym = intern(ctx, name);
    sym->file = obj;
    obj->syms.push_back(sym);
    return sym;
  };

  add("__dyld_private");

  Symbol *sym = add("__mh_execute_header");
  sym->is_extern = true;
  sym->referenced_dynamically = true;
  sym->value = PAGE_ZERO_SIZE;
}

static bool compare_segments(const std::unique_ptr<OutputSegment> &a,
                             const std::unique_ptr<OutputSegment> &b) {
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

static bool compare_chunks(const Chunk *a, const Chunk *b) {
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

  auto get_rank = [](const Chunk *chunk) -> i64 {
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

static void create_synthetic_chunks(Context &ctx) {
  for (ObjectFile *file : ctx.objs)
    for (std::unique_ptr<Subsection> &subsec : file->subsections)
      subsec->isec.osec.add_subsec(subsec.get());

  for (Chunk *chunk : ctx.chunks) {
    if (chunk != ctx.data && chunk->is_regular &&
        ((OutputSection *)chunk)->members.empty())
      continue;

    OutputSegment *seg =
      OutputSegment::get_instance(ctx, chunk->hdr.get_segname());
    seg->chunks.push_back(chunk);
  }

  sort(ctx.segments, compare_segments);

  for (std::unique_ptr<OutputSegment> &seg : ctx.segments)
    sort(seg->chunks, compare_chunks);
}

static void export_symbols(Context &ctx) {
  ctx.got.add(ctx, intern(ctx, "dyld_stub_binder"));

  for (ObjectFile *file : ctx.objs) {
    for (Symbol *sym : file->syms) {
      if (sym && sym->file == file) {
        if (sym->flags & NEEDS_GOT)
          ctx.got.add(ctx, sym);
        if (sym->flags & NEEDS_THREAD_PTR)
          ctx.thread_ptrs.add(ctx, sym);
      }
    }
  }

  for (DylibFile *file : ctx.dylibs) {
    for (Symbol *sym : file->syms) {
      if (!sym)
        continue;

      if (sym->file == file)
        if (sym->flags & NEEDS_STUB)
          ctx.stubs.add(ctx, sym);
      if (sym->flags & NEEDS_GOT)
        ctx.got.add(ctx, sym);
      if (sym->flags & NEEDS_THREAD_PTR)
        ctx.thread_ptrs.add(ctx, sym);
    }
  }
}

static i64 assign_offsets(Context &ctx) {
  i64 sect_idx = 1;
  for (std::unique_ptr<OutputSegment> &seg : ctx.segments)
    for (Chunk *chunk : seg->chunks)
      if (!chunk->is_hidden)
        chunk->sect_idx = sect_idx++;

  i64 fileoff = 0;
  i64 vmaddr = PAGE_ZERO_SIZE;

  for (std::unique_ptr<OutputSegment> &seg : ctx.segments) {
    seg->set_offset(ctx, fileoff, vmaddr);
    fileoff += seg->cmd.filesize;
    vmaddr += seg->cmd.vmsize;
  }
  return fileoff;
}

static void fix_synthetic_symbol_values(Context &ctx) {
  intern(ctx, "__dyld_private")->value = ctx.data->hdr.addr;
}

MappedFile<Context> *find_library(Context &ctx, std::string name) {
  for (std::string dir : ctx.arg.library_paths) {
    for (std::string ext : {".tbd", ".dylib", ".a"}) {
      std::string path = dir + "/lib" + name + ext;
      if (MappedFile<Context> *mf = MappedFile<Context>::open(ctx, path))
        return mf;
    }
  }
  return nullptr;
}

static MappedFile<Context> *
strip_universal_header(Context &ctx, MappedFile<Context> *mf) {
  FatHeader &hdr = *(FatHeader *)mf->data;
  assert(hdr.magic == FAT_MAGIC);

  FatArch *arch = (FatArch *)(mf->data + sizeof(hdr));
  for (i64 i = 0; i < hdr.nfat_arch; i++)
    if (arch[i].cputype == CPU_TYPE_X86_64)
      return mf->slice(ctx, mf->name, arch[i].offset, arch[i].size);
  Fatal(ctx) << mf->name << ": fat file contains no matching file";
}

static void read_file(Context &ctx, MappedFile<Context> *mf) {
  if (get_file_type(mf) == FileType::MACH_UNIVERSAL)
    mf = strip_universal_header(ctx, mf);

  switch (get_file_type(mf)) {
  case FileType::TAPI:
  case FileType::MACH_DYLIB:
    ctx.dylibs.push_back(DylibFile::create(ctx, mf));
    break;
  case FileType::MACH_OBJ:
    ctx.objs.push_back(ObjectFile::create(ctx, mf, ""));
    break;
  case FileType::AR:
    for (MappedFile<Context> *child : read_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::MACH_OBJ)
        ctx.objs.push_back(ObjectFile::create(ctx, child, mf->name));
    break;
  default:
    break;
    // Fatal(ctx) << mf->name << ": unknown file type";
  }
}

static void read_input_files(Context &ctx, std::span<std::string> args) {
  for (std::string &arg : args) {
    if (arg.starts_with("-l")) {
      MappedFile<Context> *mf = find_library(ctx, arg.substr(2));
      if (!mf)
        Fatal(ctx) << "library not found: " << arg;
      read_file(ctx, mf);
    } else {
      read_file(ctx, MappedFile<Context>::must_open(ctx, arg));
    }
  }
}

int main(int argc, char **argv) {
  Context ctx;

  if (argc > 1 && std::string_view(argv[1]) == "-dump") {
    if (argc != 3)
      Fatal(ctx) << "usage: ld64.mold -dump <executable-name>\n";
    dump_file(argv[2]);
    exit(0);
  }

  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string> file_args;
  parse_nonpositional_args(ctx, file_args);

  read_input_files(ctx, file_args);

  i64 priority = 1;
  for (ObjectFile *file : ctx.objs)
    file->priority = priority++;
  for (DylibFile *dylib : ctx.dylibs)
    dylib->priority = priority++;

  for (i64 i = 0; i < ctx.dylibs.size(); i++)
    ctx.dylibs[i]->dylib_idx = i + 1;

  for (ObjectFile *file : ctx.objs)
    file->parse(ctx);
  for (DylibFile *dylib : ctx.dylibs)
    dylib->parse(ctx);

  for (ObjectFile *file : ctx.objs) {
    if (file->archive_name.empty())
      file->resolve_regular_symbols(ctx);
    else
      file->resolve_lazy_symbols(ctx);
  }

  std::vector<ObjectFile *> live_objs;
  for (ObjectFile *file : ctx.objs)
    if (file->is_alive)
      live_objs.push_back(file);

  for (i64 i = 0; i < live_objs.size(); i++)
    append(live_objs, live_objs[i]->mark_live_objects(ctx));

  for (DylibFile *dylib : ctx.dylibs)
    dylib->resolve_symbols(ctx);

  if (!intern(ctx, ctx.arg.entry)->file)
    Error(ctx) << "undefined entry point symbol: " << ctx.arg.entry;

  create_internal_file(ctx);

  erase(ctx.objs, [](ObjectFile *file) { return !file->is_alive; });
  erase(ctx.dylibs, [](DylibFile *file) { return !file->is_alive; });

  if (ctx.arg.trace) {
    for (ObjectFile *file : ctx.objs)
      SyncOut(ctx) << *file;
    for (DylibFile *file : ctx.dylibs)
      SyncOut(ctx) << *file;
  }

  for (ObjectFile *file : ctx.objs)
    file->convert_common_symbols(ctx);

  if (ctx.arg.dead_strip)
    dead_strip(ctx);

  create_synthetic_chunks(ctx);

  for (ObjectFile *file : ctx.objs)
    file->check_duplicate_symbols(ctx);

  for (i64 i = 0; i < ctx.segments.size(); i++)
    ctx.segments[i]->seg_idx = i + 1;

  for (ObjectFile *file : ctx.objs)
    for (std::unique_ptr<Subsection > &subsec : file->subsections)
      subsec->scan_relocations(ctx);

  export_symbols(ctx);
  i64 output_size = assign_offsets(ctx);
  fix_synthetic_symbol_values(ctx);

  ctx.output_file = OutputFile::open(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  for (std::unique_ptr<OutputSegment> &seg : ctx.segments)
    seg->copy_buf(ctx);
  ctx.code_sig.write_signature(ctx);

  ctx.output_file->close(ctx);
  ctx.checkpoint();

  if (!ctx.arg.map.empty())
    print_map(ctx);
  return 0;
}

}
