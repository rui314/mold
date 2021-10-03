#include "mold.h"

#include <cstdlib>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

static void create_synthetic_sections(Context &ctx) {
  ctx.segments.push_back(&ctx.text_seg);
  ctx.segments.push_back(&ctx.data_const_seg);
  ctx.segments.push_back(&ctx.data_seg);
  ctx.segments.push_back(&ctx.linkedit_seg);

  ctx.text_seg.sections.push_back(&ctx.mach_hdr);
  ctx.text_seg.sections.push_back(&ctx.load_cmd);
  ctx.text_seg.sections.push_back(&ctx.text);
  ctx.text_seg.sections.push_back(&ctx.stubs);
  ctx.text_seg.sections.push_back(&ctx.stub_helper);
  ctx.text_seg.sections.push_back(&ctx.cstring);
  ctx.text_seg.sections.push_back(&ctx.unwind_info);

  ctx.data_const_seg.sections.push_back(&ctx.got);

  ctx.data_seg.sections.push_back(&ctx.lazy_symbol_ptr);
  ctx.data_seg.sections.push_back(&ctx.data);

  ctx.linkedit_seg.sections.push_back(&ctx.rebase);
  ctx.linkedit_seg.sections.push_back(&ctx.bind);
  ctx.linkedit_seg.sections.push_back(&ctx.lazy_bind);
  ctx.linkedit_seg.sections.push_back(&ctx.export_);
  ctx.linkedit_seg.sections.push_back(&ctx.function_starts);
  ctx.linkedit_seg.sections.push_back(&ctx.symtab);
  ctx.linkedit_seg.sections.push_back(&ctx.indir_symtab);
  ctx.linkedit_seg.sections.push_back(&ctx.strtab);
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
  ctx.stubs.add(ctx, 1, "_printf", 0, 3, 0);
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

int main(int argc, char **argv) {
  Context ctx;

  if (std::string_view(argv[1]) == "-dump") {
    if (argc != 3)
      Fatal(ctx) << "usage: ld64.mold -dump <executable-name>\n";
    dump_file(argv[2]);
    exit(0);
  }

  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string_view> file_args;
  parse_nonpositional_args(ctx, file_args);

  create_synthetic_sections(ctx);
  fill_symtab(ctx);
  export_symbols(ctx);
  ctx.load_cmd.compute_size(ctx);
  i64 output_size = assign_offsets(ctx);

  ctx.output_file =
    std::make_unique<OutputFile>(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  for (OutputSegment *seg : ctx.segments)
    seg->copy_buf(ctx);

  ctx.output_file->close(ctx);
  return 0;
}

}
