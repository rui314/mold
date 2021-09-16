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
  add(ctx.zero_page = std::make_unique<OutputPageZero>());
}

void compute_chunk_sizes(Context &ctx) {
  for (Chunk *chunk : ctx.chunks)
    chunk->update_hdr(ctx);
}

void assign_file_offsets(Context &ctx) {
  i64 fileoff = 0;

  for (Chunk *chunk : ctx.chunks) {
    chunk->fileoff = fileoff;
    fileoff += chunk->size;
  }
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
    assign_file_offsets(ctx);

    ctx.output_file =
      std::make_unique<OutputFile>(ctx, ctx.arg.output, 1024, 0777);
    ctx.buf = ctx.output_file->buf;

    for (Chunk *chunk : ctx.chunks)
      chunk->copy_buf(ctx);

    ctx.output_file->close(ctx);
    exit(0);
  }

  Fatal(ctx) << "usage: ld64.mold\n";
}

}
