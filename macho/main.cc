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

static void create_synthetic_chunks(Context &ctx) {
  for (ObjectFile *obj : ctx.objs) {
    for (std::unique_ptr<InputSection> &isec : obj->sections) {
      for (std::unique_ptr<Subsection> &subsec : isec->subsections)
        isec->osec.add_subsec(subsec.get());
      isec->osec.hdr.attr |= isec->hdr.attr;
    }
  }

  ctx.headerpad.hdr.size = ctx.arg.headerpad;

  ctx.text_seg->chunks.push_back(&ctx.mach_hdr);
  ctx.text_seg->chunks.push_back(&ctx.load_cmd);
  ctx.text_seg->chunks.push_back(&ctx.headerpad);
  ctx.text_seg->chunks.push_back(ctx.text);
  ctx.text_seg->chunks.push_back(&ctx.stubs);
  ctx.text_seg->chunks.push_back(&ctx.stub_helper);
  ctx.text_seg->chunks.push_back(ctx.cstring);
  ctx.text_seg->chunks.push_back(&ctx.unwind_info);

  ctx.data_const_seg->chunks.push_back(&ctx.got);

  ctx.data_seg->chunks.push_back(&ctx.lazy_symbol_ptr);
  ctx.data_seg->chunks.push_back(ctx.data);

  if (!ctx.common->members.empty())
    ctx.data_seg->chunks.push_back(ctx.common);
  if (!ctx.bss->members.empty())
    ctx.data_seg->chunks.push_back(ctx.bss);

  ctx.linkedit_seg->chunks.push_back(&ctx.rebase);
  ctx.linkedit_seg->chunks.push_back(&ctx.bind);
  ctx.linkedit_seg->chunks.push_back(&ctx.lazy_bind);
  ctx.linkedit_seg->chunks.push_back(&ctx.export_);
  ctx.linkedit_seg->chunks.push_back(&ctx.function_starts);
  ctx.linkedit_seg->chunks.push_back(&ctx.symtab);
  ctx.linkedit_seg->chunks.push_back(&ctx.indir_symtab);
  ctx.linkedit_seg->chunks.push_back(&ctx.strtab);
}

static void export_symbols(Context &ctx) {
  ctx.got.add(ctx, intern(ctx, "dyld_stub_binder"));

  for (ObjectFile *file : ctx.objs)
    for (Symbol *sym : file->syms)
      if (sym->file == file && sym->flags & NEEDS_GOT)
        ctx.got.add(ctx, sym);

  for (DylibFile *file : ctx.dylibs) {
    for (Symbol *sym : file->syms) {
      if (sym->file == file)
        if (sym->flags & NEEDS_STUB)
          ctx.stubs.add(ctx, sym);
      if (sym->flags & NEEDS_GOT)
        ctx.got.add(ctx, sym);
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
      std::string path =
        path_clean(ctx.arg.syslibroot + dir + "/lib" + name + ext);
      if (MappedFile<Context> *mf = MappedFile<Context>::open(ctx, path))
        return mf;
    }
  }
  return nullptr;
}

static void read_file(Context &ctx, MappedFile<Context> *mf) {
  switch (get_file_type(mf)) {
  case FileType::TAPI:
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

  create_internal_file(ctx);

  for (ObjectFile *file : ctx.objs)
    file->convert_common_symbols(ctx);

  create_synthetic_chunks(ctx);

  for (i64 i = 0; i < ctx.segments.size(); i++)
    ctx.segments[i]->seg_idx = i + 1;

  for (ObjectFile *file : ctx.objs)
    for (std::unique_ptr<InputSection> &sec : file->sections)
      sec->scan_relocations(ctx);

  export_symbols(ctx);
  i64 output_size = assign_offsets(ctx);
  fix_synthetic_symbol_values(ctx);

  ctx.output_file = OutputFile::open(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  for (std::unique_ptr<OutputSegment> &seg : ctx.segments)
    seg->copy_buf(ctx);

  ctx.output_file->close(ctx);
  ctx.checkpoint();
  return 0;
}

}
