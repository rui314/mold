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
  ctx.text_seg.reset(text);

  OutputSegment *data_const =
    new OutputSegment("__DATA_CONST", VM_PROT_READ | VM_PROT_WRITE, SG_READ_ONLY);
  ctx.segments.push_back(data_const);
  ctx.data_const_seg.reset(data_const);

  OutputSegment *data =
    new OutputSegment("__DATA", VM_PROT_READ | VM_PROT_WRITE, 0);
  ctx.segments.push_back(data);
  ctx.data_seg.reset(data);

  OutputSegment *linkedit = new OutputSegment("__LINKEDIT", VM_PROT_READ, 0);
  ctx.segments.push_back(linkedit);
  ctx.linkedit_seg.reset(linkedit);

  ctx.mach_hdr.reset(new OutputMachHeader(*ctx.text_seg));
  ctx.text_seg->sections.push_back(ctx.mach_hdr.get());

  ctx.load_cmd.reset(new OutputLoadCommand(*ctx.text_seg));
  ctx.text_seg->sections.push_back(ctx.load_cmd.get());

  ctx.text_seg->sections.push_back(new TextSection(*ctx.text_seg));
  ctx.text_seg->sections.push_back(new StubsSection(*ctx.text_seg));
  ctx.text_seg->sections.push_back(new StubHelperSection(*ctx.text_seg));
  ctx.text_seg->sections.push_back(new CstringSection(*ctx.text_seg));
  ctx.text_seg->sections.push_back(new UnwindInfoSection(*ctx.text_seg));

  ctx.data_const_seg->sections.push_back(new GotSection(*ctx.data_const_seg));

  ctx.data_seg->sections.push_back(new LaSymbolPtrSection(*ctx.data_seg));
  ctx.data_seg->sections.push_back(new DataSection(*ctx.data_seg));

  ctx.rebase.reset(new OutputRebaseSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.rebase.get());

  ctx.bind.reset(new OutputBindSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.bind.get());

  ctx.lazy_bind.reset(new OutputLazyBindSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.lazy_bind.get());

  ctx.export_.reset(new OutputExportSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.export_.get());

  ctx.function_starts.reset(new OutputFunctionStartsSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.function_starts.get());

  ctx.symtab.reset(new OutputSymtabSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.symtab.get());

  ctx.indir_symtab.reset(new OutputIndirectSymtabSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.indir_symtab.get());

  ctx.strtab.reset(new OutputStrtabSection(*ctx.linkedit_seg));
  ctx.linkedit_seg->sections.push_back(ctx.strtab.get());
}

void compute_seg_sizes(Context &ctx) {
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
    compute_seg_sizes(ctx);
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
