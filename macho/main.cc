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
  auto add = [&](auto &chunk) {
    ctx.chunks.push_back(chunk.get());
  };

  add(ctx.mach_hdr = std::make_unique<OutputMachHeader>());
  add(ctx.load_cmd = std::make_unique<OutputLoadCommand>());
  add(ctx.text_segment =
      std::make_unique<OutputSegment>("__TEXT", VM_PROT_READ | VM_PROT_EXECUTE, 0));
  add(ctx.data_const_segment =
      std::make_unique<OutputSegment>("__DATA_CONST", VM_PROT_READ | VM_PROT_WRITE,
                                      SG_READ_ONLY));
  add(ctx.data_segment =
      std::make_unique<OutputSegment>("__DATA", VM_PROT_READ | VM_PROT_WRITE, 0));
  add(ctx.linkedit_chunk = std::make_unique<OutputLinkEditChunk>());

  ctx.text_segment->sections.push_back(new TextSection(*ctx.text_segment));
  ctx.text_segment->sections.push_back(new StubsSection(*ctx.text_segment));
  ctx.text_segment->sections.push_back(new StubHelperSection(*ctx.text_segment));
  ctx.text_segment->sections.push_back(new CstringSection(*ctx.text_segment));
  ctx.data_const_segment->sections.push_back(new GotSection(*ctx.data_const_segment));
  ctx.data_segment->sections.push_back(new LaSymbolPtrSection(*ctx.data_segment));
  ctx.data_segment->sections.push_back(new DataSection(*ctx.data_segment));
}

void compute_chunk_sizes(Context &ctx) {
  for (Chunk *chunk : ctx.chunks)
    chunk->update_hdr(ctx);
}

i64 assign_offsets(Context &ctx) {
  i64 fileoff = 0;

  for (Chunk *chunk : ctx.chunks) {
    fileoff = align_to(fileoff, 1 << chunk->p2align);
    chunk->fileoff = fileoff;
    fileoff += chunk->filesize;
  }

  i64 vmaddr = PAGE_ZERO_SIZE;
  for (Chunk *chunk : ctx.chunks) {
    if (chunk->is_segment) {
      OutputSegment &seg = *(OutputSegment *)chunk;
      vmaddr = align_to(vmaddr, PAGE_SIZE);
      seg.cmd.vmaddr = vmaddr;
      vmaddr += seg.cmd.vmsize;
    }
  }

  ctx.linkedit_chunk->vmaddr = align_to(vmaddr, PAGE_SIZE);
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
    compute_chunk_sizes(ctx);
    i64 output_size = assign_offsets(ctx);

    ctx.output_file =
      std::make_unique<OutputFile>(ctx, ctx.arg.output, output_size, 0777);
    ctx.buf = ctx.output_file->buf;

    for (Chunk *chunk : ctx.chunks)
      chunk->copy_buf(ctx);

    ctx.output_file->close(ctx);
    exit(0);
  }

  Fatal(ctx) << "usage: ld64.mold\n";
}

}
