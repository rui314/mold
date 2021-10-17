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
  ctx.symtab.add(ctx, "__dyld_private", N_SECT, false, 8, 0x0, 0x100008008);
  ctx.symtab.add(ctx, "__mh_execute_header", N_SECT, true, 1, 0x10, 0x100000000);
  ctx.symtab.add(ctx, "_hello", N_SECT, true, 1, 0x0, 0x100003f50);
  ctx.symtab.add(ctx, "_main", N_SECT, true, 1, 0x0, 0x100003f70);
  ctx.symtab.add(ctx, "_printf", N_UNDF, true, 0, 0x100, 0x0);
  ctx.symtab.add(ctx, "dyld_stub_binder", N_UNDF, true, 0, 0x100, 0x0);

  ctx.strtab.hdr.size = align_to(ctx.strtab.hdr.size, 8);
}

static void export_symbols(Context &ctx) {
  ctx.stubs.add(ctx, *intern(ctx, "_printf"), 1, 0, 3, 0);
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

void read_file(Context &ctx, MappedFile<Context> *mf) {
  switch (get_file_type(mf)) {
  case FileType::MACH_OBJ:
    ctx.objs.push_back(ObjectFile::create(ctx, mf));
    return;
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
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
    std::vector<YamlNode> nodes = parse_yaml(ctx, R"(
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, x86_64-maccatalyst, arm64-macos, arm64-maccatalyst,
                   arm64e-macos, arm64e-maccatalyst ]
uuids:
  - target:          x86_64-macos
    value:           89E331F9-9A00-3EA4-B49F-FA2B91AE9182
install-name:    '/usr/lib/libSystem.B.dylib'
)");

    for (YamlNode &node : nodes)
      dump_yaml(ctx, node);
    exit(0);
  }

  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string_view> file_args;
  parse_nonpositional_args(ctx, file_args);

  for (std::string_view arg : file_args)
    read_file(ctx, MappedFile<Context>::must_open(ctx, std::string(arg)));

  for (ObjectFile *obj : ctx.objs)
    obj->parse(ctx);

  for (ObjectFile *obj : ctx.objs)
    obj->resolve_symbols(ctx);

  create_internal_file(ctx);
  create_synthetic_chunks(ctx);
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
