#include "mold.h"

#include <iostream>

using namespace llvm;
using namespace llvm::ELF;

using llvm::object::Archive;
using llvm::opt::InputArgList;

Config config;

constexpr int PAGE_SIZE = 4096;

//
// Command-line option processing
//

enum {
  OPT_INVALID = 0,
#define OPTION(_1, _2, ID, _4, _5, _6, _7, _8, _9, _10, _11, _12) OPT_##ID,
#include "options.inc"
#undef OPTION
};

// Create prefix string literals used in Options.td
#define PREFIX(NAME, VALUE) const char *const NAME[] = VALUE;
#include "options.inc"
#undef PREFIX

// Create table mapping all options defined in Options.td
static const llvm::opt::OptTable::Info opt_info[] = {
#define OPTION(X1, X2, ID, KIND, GROUP, ALIAS, X7, X8, X9, X10, X11, X12)      \
  {X1, X2, X10,         X11,         OPT_##ID, llvm::opt::Option::KIND##Class, \
   X9, X8, OPT_##GROUP, OPT_##ALIAS, X7,       X12},
#include "options.inc"
#undef OPTION
};

class MyOptTable : llvm::opt::OptTable {
public:
  MyOptTable() : OptTable(opt_info) {}
  InputArgList parse(int argc, char **argv);
};

InputArgList MyOptTable::parse(int argc, char **argv) {
  unsigned missingIndex;
  unsigned missingCount;
  SmallVector<const char *, 256> vec(argv, argv + argc);

  InputArgList args = this->ParseArgs(vec, missingIndex, missingCount);
  if (missingCount)
    error(Twine(args.getArgString(missingIndex)) + ": missing argument");

  for (auto *arg : args.filtered(OPT_UNKNOWN))
    error("unknown argument '" + arg->getAsString(args) + "'");
  return args;
}

//
// Main
//

static std::vector<MemoryBufferRef> get_archive_members(MemoryBufferRef mb) {
  std::unique_ptr<Archive> file =
    CHECK(Archive::create(mb), mb.getBufferIdentifier() + ": failed to parse archive");

  std::vector<MemoryBufferRef> vec;

  Error err = Error::success();

  for (const Archive::Child &c : file->children(err)) {
    MemoryBufferRef mbref =
        CHECK(c.getMemoryBufferRef(),
              mb.getBufferIdentifier() +
                  ": could not get the buffer for a child of the archive");
    vec.push_back(mbref);
  }

  if (err)
    error(mb.getBufferIdentifier() + ": Archive::children failed: " +
          toString(std::move(err)));

  file.release(); // leak
  return vec;
}

static void read_file(std::vector<ObjectFile *> &files, StringRef path) {
  MemoryBufferRef mb = readFile(path);

  switch (identify_magic(mb.getBuffer())) {
  case file_magic::archive:
    for (MemoryBufferRef member : get_archive_members(mb))
      files.push_back(new ObjectFile(member, path));
    break;
  case file_magic::elf_relocatable:
    files.push_back(new ObjectFile(mb, ""));
    break;
  default:
    error(path + ": unknown file type");
  }
}

thread_local int foo;
thread_local int bar = 5;

// We want to sort output sections in the following order.
//
// alloc readonly data
// alloc readonly code
// alloc writable tdata
// alloc writable tbss
// alloc writable data
// alloc writable bss
// nonalloc
static int get_rank(OutputSection *x) {
  bool alloc = x->shdr.sh_flags & SHF_ALLOC;
  bool writable = x->shdr.sh_flags & SHF_WRITE;
  bool exec = x->shdr.sh_flags & SHF_EXECINSTR;
  bool tls = x->shdr.sh_flags & SHF_TLS;
  bool nobits = x->shdr.sh_type & SHT_NOBITS;
  return (alloc << 5) | (!writable << 4) | (!exec << 3) | (tls << 2) | !nobits;
}

static std::vector<OutputSection *> get_output_sections() {
  std::vector<OutputSection *> vec;
  for (OutputSection *osec : OutputSection::all_instances)
    if (!osec->chunks.empty())
      vec.push_back(osec);

  std::sort(vec.begin(), vec.end(), [](OutputSection *a, OutputSection *b) {
    int x = get_rank(a);
    int y = get_rank(b);
    if (x != y)
      return x > y;

    // Tie-break to make output deterministic.
    if (a->shdr.sh_flags != b->shdr.sh_flags)
      return a->shdr.sh_flags < b->shdr.sh_flags;
    if (a->shdr.sh_type != b->shdr.sh_type)
      return a->shdr.sh_type < b->shdr.sh_type;
    return a->name < b->name;
  });

  return vec;
}

static std::vector<ELF64LE::Shdr *>
create_shdrs(ArrayRef<OutputChunk *> output_chunks) {
  static ELF64LE::Shdr null_entry = {};

  std::vector<ELF64LE::Shdr *> vec;
  vec.push_back(&null_entry);

  for (OutputChunk *chunk : output_chunks)
    if (!chunk->name.empty())
      vec.push_back(&chunk->shdr);
  return vec;
}

static void fill_shdrs(ArrayRef<OutputChunk *> output_chunks) {
  int i = 1;

  for (OutputChunk *chunk : output_chunks) {
    if (chunk->name.empty())
      continue;
    chunk->shdr.sh_offset = chunk->fileoff;
    chunk->shdr.sh_size = chunk->get_size();
  }
}

static void unlink_async(tbb::task_group &tg, StringRef path) {
  if (!sys::fs::exists(path) || !sys::fs::is_regular_file(path))
    return;

  int fd;
  if (std::error_code ec = sys::fs::openFileForRead(path, fd))
    return;
  sys::fs::remove(path);
  tg.run([=]() { close(fd); });
}

class MyTimer {
public:
  MyTimer(StringRef name) {
    timer = new Timer(name, name);
    timer->startTimer();
  }

  MyTimer(StringRef name, llvm::TimerGroup &tg) {
    timer = new Timer(name, name, tg);
    timer->startTimer();
  }

  ~MyTimer() { timer->stopTimer(); }

private:
  llvm::Timer *timer;
};

int main(int argc, char **argv) {
  // tbb::task_scheduler_init init(1);
  tbb::task_group tg;

  // Parse command line options
  MyOptTable opt_table;
  InputArgList args = opt_table.parse(argc - 1, argv + 1);

  if (auto *arg = args.getLastArg(OPT_o))
    config.output = arg->getValue();
  else
    error("-o option is missing");

  std::vector<ObjectFile *> files;

  llvm::TimerGroup before_copy("before_copy", "before_copy");

  // Open input files
  {
    MyTimer t("parse", before_copy);
    for (auto *arg : args)
      if (arg->getOption().getID() == OPT_INPUT)
        read_file(files, arg->getValue());

    // Parse input files
    for_each(files, [](ObjectFile *file) { file->parse(); });
  }

  // Set priorities to files
  for (int i = 0; i < files.size(); i++)
    files[i]->priority = files[i]->is_in_archive() ? i + (1 << 31) : i;

  // Resolve symbols
  {
    MyTimer t("add_symbols", before_copy);
    for_each(files, [](ObjectFile *file) { file->register_defined_symbols(); });
    for_each(files, [](ObjectFile *file) { file->register_undefined_symbols(); });
  }

  // Eliminate unused archive members.
  files.erase(std::remove_if(files.begin(), files.end(),
                             [](ObjectFile *file){ return !file->is_alive; }),
              files.end());

  // Eliminate duplicate comdat groups.
  {
    MyTimer t("comdat", before_copy);
    for_each(files, [](ObjectFile *file) { file->eliminate_duplicate_comdat_groups(); });
  }

  auto bin_sections = [&]() {
    for (ObjectFile *file : files)
      for (InputSection *isec : file->sections)
        if (isec)
          isec->output_section->chunks.push_back(isec);
  };

  auto scan_rels = [&]() {
    for_each(files, [](ObjectFile *file) { file->scan_relocations(); });
  };

  // Bin input sections into output sections
  {
    MyTimer t("bin_sections", before_copy);
    bin_sections();
  }

  // Scan relocations to fix the sizes of .got, .plt, .got.plt, .dynstr,
  // .rela.dyn, .rela.plt.
  {
    MyTimer t("scan_rel", before_copy);
    scan_rels();
  }

  // Create linker-synthesized sections.
  out::ehdr = new OutputEhdr;
  out::phdr = new OutputPhdr;
  out::shdr = new OutputShdr;
  out::interp = new InterpSection;
  out::shstrtab = new StringTableSection(".shstrtab");

  // Add ELF and program header to the output.
  std::vector<OutputChunk *> output_chunks;
  output_chunks.push_back(out::ehdr);
  output_chunks.push_back(out::phdr);

  // Add .interp section.
  output_chunks.push_back(out::interp);

  // Add other output sections.
  for (OutputSection *osec : get_output_sections())
    output_chunks.push_back(osec);

  // Add a string table for section names.
  output_chunks.push_back(out::shstrtab);
  for (OutputChunk *chunk : output_chunks)
    if (!chunk->name.empty())
      chunk->shdr.sh_name = out::shstrtab->add_string(chunk->name);

  // Add a section header.
  out::shdr->entries = create_shdrs(output_chunks);
  output_chunks.push_back(out::shdr);

  // Create program header contents.
  out::phdr->construct(output_chunks);

  // Assign offsets to input sections
  uint64_t filesize = 0;
  {
    MyTimer t("file_offset", before_copy);
    for (OutputChunk *chunk : output_chunks) {
      chunk->set_fileoff(filesize);
      filesize += chunk->get_size();
    }
 }

  // Fill section header.
  fill_shdrs(output_chunks);

  {
    MyTimer t("unlink", before_copy);
    unlink_async(tg, config.output);
  }

  // Create an output file
  Expected<std::unique_ptr<FileOutputBuffer>> buf_or_err =
    FileOutputBuffer::create(config.output, filesize, 0);

  if (!buf_or_err)
    error("failed to open " + config.output + ": " +
          llvm::toString(buf_or_err.takeError()));

  std::unique_ptr<FileOutputBuffer> output_buffer = std::move(*buf_or_err);
  uint8_t *buf = output_buffer->getBufferStart();

  // Copy input sections to the output file
  {
    MyTimer t("copy");
    for_each(output_chunks, [&](OutputChunk *chunk) { chunk->copy_to(buf); });
  }

  {
    MyTimer t("reloc");
    for_each(output_chunks, [&](OutputChunk *chunk) { chunk->relocate(buf); });
  }

  {
    MyTimer t("commit");
    if (auto e = output_buffer->commit())
      error("failed to write to the output file: " + toString(std::move(e)));
  }

  int num_input_chunks = 0;
  for (ObjectFile *file : files)
    num_input_chunks += file->sections.size();

  {
    MyTimer t("wait");
    tg.wait();
  }

  llvm::outs() << " input_chunks=" << num_input_chunks << "\n"
               << "output_chunks=" << output_chunks.size() << "\n"
               << "        files=" << files.size() << "\n"
               << "     filesize=" << filesize << "\n"
               << " num_all_syms=" << num_all_syms << "\n"
               << "  num_defined=" << num_defined << "\n"
               << "num_undefined=" << num_undefined << "\n"
               << "  num_comdats=" << num_comdats << "\n"
               << "num_regular_sections=" << num_regular_sections << "\n"
               << "   num_relocs=" << num_relocs << "\n"
               << "num_relocs_alloc=" << num_relocs_alloc << "\n"
               << "      num_str=" << num_string_pieces << "\n";

  llvm::TimerGroup::printAll(llvm::outs());
  llvm::outs().flush();
  _exit(0);
}
