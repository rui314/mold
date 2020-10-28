#include "mold.h"

#include <iostream>

using namespace llvm;
using namespace llvm::ELF;

using llvm::object::Archive;
using llvm::opt::InputArgList;

Config config;

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

template <typename T>
static std::vector<ArrayRef<T>> split(const std::vector<T> &input, int unit) {
  ArrayRef<T> arr(input);
  std::vector<ArrayRef<T>> vec;

  while (arr.size() >= unit) {
    vec.push_back(arr.slice(0, unit));
    arr = arr.slice(unit);
  }
  if (!arr.empty())
    vec.push_back(arr);
  return vec;
}

static void bin_sections(std::vector<ObjectFile *> &files) {
#if 1
  int unit = (files.size() + 127) / 128;
  std::vector<ArrayRef<ObjectFile *>> slices = split(files, unit);

  std::vector<std::vector<std::vector<InputSection *>>> groups(slices.size());
  for (int i = 0; i < groups.size(); i++)
    groups[i].resize(OutputSection::instances.size());

  tbb::parallel_for(0, (int)slices.size(), [&](int i) {
    for (ObjectFile *file : slices[i]) {
      for (InputSection *isec : file->sections) {
        if (!isec)
          continue;
        OutputSection *osec = isec->output_section;
        groups[i][osec->idx].push_back(isec);
      }
    }
  });

  std::vector<int> sizes(OutputSection::instances.size());

  for (ArrayRef<std::vector<InputSection *>> group : groups)
    for (int i = 0; i < group.size(); i++)
      sizes[i] += group[i].size();

  for (int i = 0; i < sizes.size(); i++)
    OutputSection::instances[i]->sections.reserve(sizes[i]);

  for (ArrayRef<std::vector<InputSection *>> group : groups) {
    for (int i = 0; i < group.size(); i++) {
      std::vector<InputSection *> &sections = OutputSection::instances[i]->sections;
      sections.insert(sections.end(), group[i].begin(), group[i].end());
    }
  }
#else
  for (ObjectFile *file : files) {
    for (InputSection *isec : file->sections) {
      if (!isec)
        continue;
      OutputSection *osec = isec->output_section;
      osec->sections.push_back(isec);
    }
  }
#endif
}

static void set_isec_offsets() {
#if 1
  for_each(OutputSection::instances, [&](OutputSection *osec) {
    if (osec->sections.empty())
      return;

    std::vector<ArrayRef<InputSection *>> slices = split(osec->sections, 100000);
    std::vector<uint64_t> size(slices.size());
    std::vector<uint32_t> alignments(slices.size());

    tbb::parallel_for(0, (int)slices.size(), [&](int i) {
      uint64_t off = 0;
      uint32_t align = 1;

      for (InputSection *isec : slices[i]) {
        off = align_to(off, isec->shdr.sh_addralign);
        isec->offset = off;
        off += isec->shdr.sh_size;
        align = std::max<uint32_t>(align, isec->shdr.sh_addralign);
      }

      size[i] = off;
      alignments[i] = align;
    });

    uint32_t align = *std::max_element(alignments.begin(), alignments.end());

    std::vector<uint64_t> start(slices.size());
    for (int i = 1; i < slices.size(); i++)
      start[i] = align_to(start[i - 1] + size[i], align);

    tbb::parallel_for(1, (int)slices.size(), [&](int i) {
      for (InputSection *isec : slices[i])
        isec->offset += start[i];
    });

    osec->shdr.sh_size = start.back() + size.back();
    osec->shdr.sh_addralign = align;
  });
#else
  for_each(OutputSection::instances, [&](OutputSection *osec) {
    if (osec->sections.empty())
      return;

    uint64_t off = 0;
    uint32_t align = 0;

    for (InputSection *isec : osec->sections) {
      off = align_to(off, isec->shdr.sh_addralign);
      isec->offset = off;
      off += isec->shdr.sh_size;
      align = std::max<uint32_t>(align, isec->shdr.sh_addralign);
    }

    osec->shdr.sh_size = off;
    osec->shdr.sh_addralign = align;
  });
#endif
}

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

static bool is_osec_empty(OutputSection *osec) {
  if (osec->sections.empty())
    return true;
  for (InputSection *isec : osec->sections)
    if (isec->shdr.sh_size)
      return false;
  return true;
}

static std::vector<OutputSection *> get_output_sections() {
  std::vector<OutputSection *> vec;
  for (OutputSection *osec : OutputSection::instances)
    if (!is_osec_empty(osec))
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

  int idx = 1;
  for (OutputChunk *chunk : output_chunks) {
    if (!chunk->name.empty()) {
      vec.push_back(&chunk->shdr);
      chunk->idx = idx++;
    }
  }
  return vec;
}

static void fill_shdrs(ArrayRef<OutputChunk *> output_chunks) {
  int i = 1;

  for (OutputChunk *chunk : output_chunks) {
    if (chunk->name.empty())
      continue;
    chunk->shdr.sh_size = chunk->get_size();
  }
}

static uint64_t set_osec_offsets(ArrayRef<OutputChunk *> output_chunks) {
  uint64_t fileoff = 0;
  uint64_t vaddr = 0x400000;

  for (OutputChunk *chunk : output_chunks) {
    if (chunk->starts_new_ptload) {
      fileoff = align_to(fileoff, PAGE_SIZE);
      vaddr = align_to(vaddr, PAGE_SIZE);
    }

    if (!chunk->is_bss())
      fileoff = align_to(fileoff, chunk->shdr.sh_addralign);
    vaddr = align_to(vaddr, chunk->shdr.sh_addralign);

    chunk->shdr.sh_offset = fileoff;
    if (chunk->shdr.sh_flags & SHF_ALLOC)
      chunk->shdr.sh_addr = vaddr;

    if (!chunk->is_bss())
      fileoff += chunk->get_size();
    vaddr += chunk->get_size();
  }
  return fileoff;
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

static void write_symtab(uint8_t *buf, std::vector<ObjectFile *> files) {
  std::vector<uint64_t> symtab_off(files.size() + 1);
  std::vector<uint64_t> strtab_off(files.size() + 1);
  strtab_off[0] = 1;

  for (int i = 1; i < files.size() + 1; i++) {
    symtab_off[i] = symtab_off[i - 1] + files[i - 1]->local_symtab_size;
    strtab_off[i] = strtab_off[i - 1] + files[i - 1]->local_strtab_size;
  }

  out::symtab->shdr.sh_info = symtab_off.back() / sizeof(ELF64LE::Sym);

  tbb::parallel_for((size_t)0, files.size(),
                    [&](size_t i) {
                      files[i]->write_local_symtab(buf, symtab_off[i], strtab_off[i]);
                    });

  symtab_off[0] = symtab_off.back();
  strtab_off[0] = strtab_off.back();

  for (int i = 1; i < files.size() + 1; i++) {
    symtab_off[i] = symtab_off[i - 1] + files[i - 1]->global_symtab_size;
    strtab_off[i] = strtab_off[i - 1] + files[i - 1]->global_strtab_size;
  }

  assert(symtab_off.back() == out::symtab->size);
  assert(strtab_off.back() == out::strtab->size);

  tbb::parallel_for((size_t)0, files.size(),
                    [&](size_t i) {
                      files[i]->write_global_symtab(buf, symtab_off[i], strtab_off[i]);
                    });
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
  // tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism, 64);

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
    MyTimer t("parse");
    for (auto *arg : args)
      if (arg->getOption().getID() == OPT_INPUT)
        read_file(files, arg->getValue());

    // Parse input files
    for_each(files, [](ObjectFile *file) { file->parse(); });
  }

  // Set priorities to files
  int priority = 1;
  for (ObjectFile *file : files)
    if (!file->is_in_archive())
      file->priority = priority++;
  for (ObjectFile *file : files)
    if (file->is_in_archive())
      file->priority = priority++;

  // Resolve symbols
  {
    MyTimer t("resolve_symbols", before_copy);

    for_each(files, [](ObjectFile *file) { file->register_defined_symbols(); });

    tbb::parallel_do(
      files.begin(), files.end(),
      [&](ObjectFile *file, tbb::parallel_do_feeder<ObjectFile *>& feeder) {
        if (!file->is_in_archive())
          file->register_undefined_symbols(feeder);
      });
  }

  {
    extern std::atomic_int skip;
    llvm::outs() << "skip=" << skip << "\n";
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

  // Create .bss sections for common symbols.
  {
    MyTimer t("common", before_copy);
    for_each(files, [](ObjectFile *file) { file->convert_common_symbols(); });
  }

  // Bin input sections into output sections
  {
    MyTimer t("bin_sections", before_copy);
    bin_sections(files);
  }

  {
    MyTimer t("isec_offsets", before_copy);
    set_isec_offsets();
  }

  // Scan relocations to fix the sizes of .got, .plt, .got.plt, .dynstr,
  // .rela.dyn, .rela.plt.
  {
    MyTimer t("scan_rel", before_copy);
    for_each(files, [](ObjectFile *file) { file->scan_relocations(); });
  }

  // Create linker-synthesized sections.
  out::ehdr = new OutputEhdr;
  out::phdr = new OutputPhdr;
  out::shdr = new OutputShdr;
  //  out::interp = new InterpSection;
  out::shstrtab = new ShstrtabSection;
  out::symtab = new SymtabSection;
  out::strtab = new StrtabSection;

  // Compute .symtab and .strtab sizes
  {
    MyTimer t("symtab_size", before_copy);
    for_each(files, [](ObjectFile *file) { file->compute_symtab(); });

    for (ObjectFile *file : files) {
      out::symtab->size += file->local_symtab_size + file->global_symtab_size;
      out::strtab->size += file->local_strtab_size + file->global_strtab_size;
    }
  }

  // Add ELF and program header to the output.
  std::vector<OutputChunk *> output_chunks;
  output_chunks.push_back(out::ehdr);
  output_chunks.push_back(out::phdr);

  // Add .interp section.
  //  output_chunks.push_back(out::interp);

  // Add other output sections.
  std::vector<OutputSection *> output_sections = get_output_sections();
  for (OutputSection *osec : output_sections)
    output_chunks.push_back(osec);

  // Add a string table for section names.
  output_chunks.push_back(out::shstrtab);

  // Add a section header.
  output_chunks.push_back(out::shdr);

  // Add .symtab and .strtab.
  output_chunks.push_back(out::symtab);
  output_chunks.push_back(out::strtab);

  // Fix .shstrtab contents.
  for (OutputChunk *chunk : output_chunks)
    if (!chunk->name.empty())
      chunk->shdr.sh_name = out::shstrtab->add_string(chunk->name);

  // Create section header and program header contents.
  out::shdr->entries = create_shdrs(output_chunks);
  out::phdr->construct(output_chunks);
  out::symtab->shdr.sh_link = out::strtab->idx;

  // Fill section header.
  fill_shdrs(output_chunks);

  // Assign offsets to input sections
  uint64_t filesize = 0;
  {
    MyTimer t("osec_offset", before_copy);
    filesize = set_osec_offsets(output_chunks);
  }

  {
    MyTimer t("sym_addr");
    for_each(files, [](ObjectFile *file) { file->fix_sym_addrs(); });
  }

  tbb::task_group unlink_tg;
  {
    MyTimer t("unlink");
    unlink_async(unlink_tg, config.output);
  }

  // Create an output file
  Expected<std::unique_ptr<FileOutputBuffer>> buf_or_err =
    FileOutputBuffer::create(config.output, filesize, FileOutputBuffer::F_executable);

  if (!buf_or_err)
    error("failed to open " + config.output + ": " +
          llvm::toString(buf_or_err.takeError()));

  std::unique_ptr<FileOutputBuffer> output_buffer = std::move(*buf_or_err);
  uint8_t *buf = output_buffer->getBufferStart();

  // Fill .symtab and .strtab
  tbb::task_group tg_symtab;
  tg_symtab.run([&]() {
    MyTimer t("write_symtab");
    write_symtab(buf, files);
  });

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
    MyTimer t("symtab_wait");
    tg_symtab.wait();
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
    MyTimer t("unlink_wait");
    unlink_tg.wait();
  }

#if 0
  for (ObjectFile *file : files)
    for (InputSection *isec : file->sections)
      if (isec)
        llvm::outs() << toString(isec) << "\n";
#endif

#if 1
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
#endif

  llvm::outs().flush();
  _exit(0);
}
