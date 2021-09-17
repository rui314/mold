#include "mold.h"

#include <cstdlib>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

void create_synthetic_sections(Context &ctx) {
  OutputSegment *text =
    new OutputSegment("__TEXT", VM_PROT_READ | VM_PROT_EXECUTE, 0);
  ctx.segments.push_back(text);
  ctx.text_segment.reset(text);

  OutputSegment *data_const =
    new OutputSegment("__DATA_CONST", VM_PROT_READ | VM_PROT_WRITE, SG_READ_ONLY);
  ctx.segments.push_back(data_const);
  ctx.data_const_segment.reset(data_const);

  OutputSegment *data =
    new OutputSegment("__DATA", VM_PROT_READ | VM_PROT_WRITE, 0);
  ctx.segments.push_back(data);
  ctx.data_segment.reset(data);

  OutputSegment *linkedit = new OutputSegment("__LINKEDIT", VM_PROT_READ, 0);
  ctx.segments.push_back(linkedit);
  ctx.linkedit_segment.reset(linkedit);

  ctx.mach_hdr.reset(new OutputMachHeader(*ctx.text_segment));
  ctx.text_segment->sections.push_back(ctx.mach_hdr.get());

  ctx.load_cmd.reset(new OutputLoadCommand(*ctx.text_segment));
  ctx.text_segment->sections.push_back(ctx.load_cmd.get());

  ctx.text_segment->sections.push_back(new TextSection(*ctx.text_segment));
  ctx.text_segment->sections.push_back(new StubsSection(*ctx.text_segment));
  ctx.text_segment->sections.push_back(new StubHelperSection(*ctx.text_segment));
  ctx.text_segment->sections.push_back(new CstringSection(*ctx.text_segment));

  ctx.data_const_segment->sections.push_back(new GotSection(*ctx.data_const_segment));

  ctx.data_segment->sections.push_back(new LaSymbolPtrSection(*ctx.data_segment));
  ctx.data_segment->sections.push_back(new DataSection(*ctx.data_segment));

  ctx.linkedit.reset(new OutputLinkEditChunk(*ctx.linkedit_segment));
  ctx.linkedit_segment->sections.push_back(ctx.linkedit.get());
}

void compute_segment_sizes(Context &ctx) {
  for (OutputSegment *seg : ctx.segments)
    seg->update_hdr(ctx);
}

i64 assign_offsets(Context &ctx) {
  i64 fileoff = 0;
  i64 vmaddr = PAGE_ZERO_SIZE;

  for (OutputSegment *seg : ctx.segments) {
    fileoff = align_to(fileoff, PAGE_SIZE);
    seg->cmd.fileoff = fileoff;
    fileoff += seg->cmd.filesize;

    vmaddr = align_to(vmaddr, PAGE_SIZE);
    seg->cmd.vmaddr = vmaddr;
    vmaddr += seg->cmd.vmsize;
  }

  return fileoff;
}

int main(int argc, char **argv) {
  Context ctx;

  // Parse command line arguments
  if (argc == 1) {
    SyncOut(ctx) << "mold macho stub\n";
    exit(0);
  }

  if (std::string_view(argv[1]) == "-dump") {
    if (argc != 3)
      Fatal(ctx) << "usage: ld64.mold -dump <executable-name>\n";
    dump_file(argv[2]);
    exit(0);
  }

  if (std::string_view(argv[1]) == "-out") {
    if (argc != 3)
      Fatal(ctx) << "usage: ld64.mold -out <output-file>\n";
    ctx.arg.output = argv[2];

    create_synthetic_sections(ctx);
    compute_segment_sizes(ctx);
    i64 output_size = assign_offsets(ctx);

    ctx.output_file =
      std::make_unique<OutputFile>(ctx, ctx.arg.output, output_size, 0777);
    ctx.buf = ctx.output_file->buf;

    for (OutputSegment *seg : ctx.segments)
      seg->copy_buf(ctx);

    ctx.output_file->close(ctx);
    exit(0);
  }

  Fatal(ctx) << "usage: ld64.mold\n";
}

}
