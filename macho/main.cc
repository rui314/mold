#include "mold.h"
#include "../archive-file.h"
#include "../cmdline.h"
#include "../output-file.h"

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

  auto add = [&](std::string_view name, u64 value = 0) {
    Symbol *sym = intern(ctx, name);
    sym->file = obj;
    sym->value = value;
    obj->syms.push_back(sym);
  };

  add("__dyld_private");
  add("__mh_execute_header", PAGE_ZERO_SIZE);

  obj->syms.push_back(intern(ctx, "dyld_stub_binder"));
}

static void add_section(Context &ctx, OutputSection &osec,
                        std::string_view segname, std::string_view sectname) {
  for (ObjectFile *obj : ctx.objs) {
    for (std::unique_ptr<InputSection> &sec : obj->sections) {
      if (sec->hdr.segname == segname && sec->hdr.sectname == sectname) {
        for (Subsection &subsec : sec->subsections)
          osec.members.push_back(&subsec);
        sec->osec = &osec;
      }
    }
  }
}

static void create_synthetic_chunks(Context &ctx) {
  ctx.segments.push_back(&ctx.text_seg);
  ctx.segments.push_back(&ctx.data_const_seg);
  ctx.segments.push_back(&ctx.data_seg);
  ctx.segments.push_back(&ctx.linkedit_seg);

  ctx.text_seg.chunks.push_back(&ctx.mach_hdr);
  ctx.text_seg.chunks.push_back(&ctx.load_cmd);
  ctx.text_seg.chunks.push_back(&ctx.padding);

  ctx.padding.hdr.size = 14808;

  ctx.text.hdr.attr = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
  ctx.text.hdr.p2align = 4;
  add_section(ctx, ctx.text, "__TEXT", "__text");
  ctx.text_seg.chunks.push_back(&ctx.text);

  ctx.text_seg.chunks.push_back(&ctx.stubs);
  ctx.text_seg.chunks.push_back(&ctx.stub_helper);

  OutputSection *cstring = new OutputSection("__cstring");
  cstring->hdr.type = S_CSTRING_LITERALS;
  add_section(ctx, *cstring, "__TEXT", "__cstring");
  ctx.text_seg.chunks.push_back(cstring);

  ctx.text_seg.chunks.push_back(&ctx.unwind_info);

  ctx.data_const_seg.chunks.push_back(&ctx.got);

  ctx.data_seg.chunks.push_back(&ctx.lazy_symbol_ptr);
  ctx.data_seg.chunks.push_back(&ctx.data);

  ctx.linkedit_seg.chunks.push_back(&ctx.rebase);
  ctx.linkedit_seg.chunks.push_back(&ctx.bind);
  ctx.linkedit_seg.chunks.push_back(&ctx.lazy_bind);
  ctx.linkedit_seg.chunks.push_back(&ctx.export_);
  ctx.linkedit_seg.chunks.push_back(&ctx.function_starts);
  ctx.linkedit_seg.chunks.push_back(&ctx.symtab);
  ctx.linkedit_seg.chunks.push_back(&ctx.indir_symtab);
  ctx.linkedit_seg.chunks.push_back(&ctx.strtab);
}

static void fill_symtab(Context &ctx) {
  ctx.symtab.add(ctx, intern(ctx, "__dyld_private"), false, 8, 0x0);
  ctx.symtab.add(ctx, intern(ctx, "__mh_execute_header"), true, 1, 0x10);
  ctx.symtab.add(ctx, intern(ctx, "_hello"), true, 1, 0x0);
  ctx.symtab.add(ctx, intern(ctx, "_main"), true, 1, 0x0);
  ctx.symtab.add(ctx, intern(ctx, "_printf"), true, 0, 0x100);
  ctx.symtab.add(ctx, intern(ctx, "dyld_stub_binder"), true, 0, 0x100);
}

static void export_symbols(Context &ctx) {
  std::vector<Symbol *> syms;

  for (DylibFile *dylib : ctx.dylibs)
    for (Symbol *sym : dylib->syms)
      if (sym->file == dylib && sym->needs_stub)
        syms.push_back(sym);

  for (Symbol *sym : syms)
    ctx.stubs.add(ctx, sym);
}

static i64 assign_offsets(Context &ctx) {
  i64 fileoff = 0;
  i64 vmaddr = PAGE_ZERO_SIZE;

  for (OutputSegment *seg : ctx.segments) {
    seg->set_offset(ctx, fileoff, vmaddr);
    fileoff += seg->cmd.filesize;
    vmaddr += seg->cmd.vmsize;
  }
  return fileoff;
}

static void fix_synthetic_symbol_values(Context &ctx) {
  intern(ctx, "__dyld_private")->value = ctx.data.hdr.addr;
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

static void read_input_files(Context &ctx, std::span<std::string> args) {
  for (std::string &arg : args) {
    if (arg.starts_with("-l")) {
      MappedFile<Context> *mf = find_library(ctx, arg.substr(2));
      if (!mf)
        Fatal(ctx) << "library not found: " << arg;
      if (get_file_type(mf) == FileType::TAPI)
        ctx.dylibs.push_back(DylibFile::create(ctx, mf));
    } else {
      MappedFile<Context> *mf = MappedFile<Context>::must_open(ctx, arg);
      if (get_file_type(mf) == FileType::MACH_OBJ)
        ctx.objs.push_back(ObjectFile::create(ctx, mf));
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

  if (argc > 1 && std::string_view(argv[1]) == "-yamltest") {
    std::string path = "/Applications/Xcode.app/Contents/Developer/Platforms/"
      "MacOSX.platform/Developer/SDKs/MacOSX.sdk/usr/lib/libSystem.tbd";
    TextDylib tbd = parse_tbd(ctx, MappedFile<Context>::must_open(ctx, path));
    SyncOut(ctx) << "tbd: uuid=" << tbd.uuid
                 << " install_name=" << tbd.install_name
                 << " current_version=" << tbd.current_version
                 << " parent_umbrella=" << tbd.parent_umbrella;
    for (std::string_view sym : tbd.exports)
      SyncOut(ctx) << "  sym=" << sym;
    exit(0);
  }

  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string> file_args;
  parse_nonpositional_args(ctx, file_args);

  read_input_files(ctx, file_args);

  i64 priority = 1;
  for (ObjectFile *obj : ctx.objs)
    obj->priority = priority++;
  for (DylibFile *dylib : ctx.dylibs)
    dylib->priority = priority++;

  for (i64 i = 0; i < ctx.dylibs.size(); i++)
    ctx.dylibs[i]->dylib_idx = i + 1;

  for (ObjectFile *obj : ctx.objs)
    obj->parse(ctx);
  for (DylibFile *dylib : ctx.dylibs)
    dylib->parse(ctx);

  for (ObjectFile *obj : ctx.objs)
    obj->resolve_symbols(ctx);
  for (DylibFile *dylib : ctx.dylibs)
    dylib->resolve_symbols(ctx);

  create_internal_file(ctx);
  create_synthetic_chunks(ctx);

  for (ObjectFile *obj : ctx.objs)
    for (std::unique_ptr<InputSection> &sec : obj->sections)
      sec->scan_relocations(ctx);

  fill_symtab(ctx);
  export_symbols(ctx);
  i64 output_size = assign_offsets(ctx);
  fix_synthetic_symbol_values(ctx);

  ctx.output_file = open_output_file(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  for (OutputSegment *seg : ctx.segments)
    seg->copy_buf(ctx);

  ctx.output_file->close(ctx);
  return 0;
}

}
