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
  unsigned missing_index = 0;
  unsigned missing_count = 0;
  SmallVector<const char *, 256> vec(argv, argv + argc);

  InputArgList args = this->ParseArgs(vec, missing_index, missing_count);
  if (missing_count)
    error(Twine(args.getArgString(missing_index)) + ": missing argument");

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

static ObjectFile *create_internal_file() {
  // Create a dummy object file.
  static char buf[256];
  std::unique_ptr<MemoryBuffer> mb =
    MemoryBuffer::getMemBuffer(StringRef(buf, sizeof(buf)));
  auto *obj = new ObjectFile(mb->getMemBufferRef(), "");
  obj->name = "<internal>";
  mb.release();

  // Create linker-synthesized symbols.
  auto *elf_syms = new std::vector<ELF64LE::Sym>;

  auto create = [&](StringRef name) {
    Symbol *sym = Symbol::intern(name);
    sym->file = obj;
    obj->symbols.push_back(sym);

    ELF64LE::Sym esym = {};
    esym.setType(STT_NOTYPE);
    esym.setBinding(STB_GLOBAL);
    elf_syms->push_back(esym);
    return sym;
  };

  out::__bss_start = create("__bss_start");
  out::__ehdr_start = create("__ehdr_start");
  obj->elf_syms = *elf_syms;
  return obj;
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

      if (toString(file) == "/usr/lib/x86_64-linux-gnu/libc.a:cxa_atexit.o")
        llvm::outs() << "isec=" << toString(isec)
                     << " " << toString(isec)
                     << " " << (void *)isec->output_section << "\n";
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
    std::vector<u64> size(slices.size());
    std::vector<u32> alignments(slices.size());

    tbb::parallel_for(0, (int)slices.size(), [&](int i) {
      u64 off = 0;
      u32 align = 1;

      for (InputSection *isec : slices[i]) {
        off = align_to(off, isec->shdr.sh_addralign);
        isec->offset = off;
        off += isec->shdr.sh_size;
        align = std::max<u32>(align, isec->shdr.sh_addralign);
      }

      size[i] = off;
      alignments[i] = align;
    });

    u32 align = *std::max_element(alignments.begin(), alignments.end());

    std::vector<u64> start(slices.size());
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

    u64 off = 0;
    u32 align = 0;

    for (InputSection *isec : osec->sections) {
      off = align_to(off, isec->shdr.sh_addralign);
      isec->offset = off;
      off += isec->shdr.sh_size;
      align = std::max<u32>(align, isec->shdr.sh_addralign);
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
static int get_rank(const ELF64LE::Shdr shdr) {
  bool alloc = shdr.sh_flags & SHF_ALLOC;
  bool writable = shdr.sh_flags & SHF_WRITE;
  bool exec = shdr.sh_flags & SHF_EXECINSTR;
  bool tls = shdr.sh_flags & SHF_TLS;
  bool nobits = shdr.sh_type == SHT_NOBITS;
  return (alloc << 5) | (!writable << 4) | (!exec << 3) | (tls << 2) | !nobits;
}

static void sort_output_chunks(std::vector<OutputChunk *> &chunks) {
  std::sort(chunks.begin(), chunks.end(), [](OutputChunk *a, OutputChunk *b) {
    int x = get_rank(a->shdr);
    int y = get_rank(b->shdr);
    if (x != y)
      return x > y;

    // Tie-break to make output deterministic.
    if (a->shdr.sh_flags != b->shdr.sh_flags)
      return a->shdr.sh_flags < b->shdr.sh_flags;
    if (a->shdr.sh_type != b->shdr.sh_type)
      return a->shdr.sh_type < b->shdr.sh_type;
    return a->name < b->name;
  });
}

static std::vector<ELF64LE::Shdr *>
create_shdrs(ArrayRef<OutputChunk *> output_chunks) {
  static ELF64LE::Shdr null_entry = {};

  std::vector<ELF64LE::Shdr *> vec;
  vec.push_back(&null_entry);

  int shndx = 1;
  for (OutputChunk *chunk : output_chunks) {
    if (!chunk->name.empty()) {
      vec.push_back(&chunk->shdr);
      chunk->shndx = shndx++;
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

static u64 set_osec_offsets(ArrayRef<OutputChunk *> output_chunks) {
  u64 fileoff = 0;
  u64 vaddr = 0x200000;

  for (OutputChunk *chunk : output_chunks) {
    if (chunk->starts_new_ptload)
      vaddr = align_to(vaddr, PAGE_SIZE);

    if (!chunk->is_bss()) {
      if (vaddr % PAGE_SIZE > fileoff % PAGE_SIZE)
        fileoff += vaddr % PAGE_SIZE - fileoff % PAGE_SIZE;
      else if (vaddr % PAGE_SIZE < fileoff % PAGE_SIZE)
        fileoff = align_to(fileoff, PAGE_SIZE) + vaddr % PAGE_SIZE;
    }

    fileoff = align_to(fileoff, chunk->shdr.sh_addralign);
    vaddr = align_to(vaddr, chunk->shdr.sh_addralign);

    chunk->shdr.sh_offset = fileoff;
    if (chunk->shdr.sh_flags & SHF_ALLOC)
      chunk->shdr.sh_addr = vaddr;

    if (!chunk->is_bss())
      fileoff += chunk->get_size();

    bool is_tbss = chunk->is_bss() && (chunk->shdr.sh_flags & SHF_TLS);
    if (!is_tbss)
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

static void write_symtab(u8 *buf, std::vector<ObjectFile *> files) {
  std::vector<u64> symtab_off(files.size() + 1);
  std::vector<u64> strtab_off(files.size() + 1);
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

static int get_thread_count(InputArgList &args) {
  if (auto *arg = args.getLastArg(OPT_thread_count)) {
    int n;
    if (!llvm::to_integer(arg->getValue(), n) || n <= 0)
      error(arg->getSpelling() + ": expected a positive integer, but got '" +
            arg->getValue() + "'");
    return n;
  }
  return tbb::global_control::active_value(tbb::global_control::max_allowed_parallelism);
}

int main(int argc, char **argv) {
  // Parse command line options
  MyOptTable opt_table;
  InputArgList args = opt_table.parse(argc - 1, argv + 1);

  tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism,
                               get_thread_count(args));

  if (auto *arg = args.getLastArg(OPT_o))
    config.output = arg->getValue();
  else
    error("-o option is missing");

  config.print_map = args.hasArg(OPT_print_map);

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

    std::vector<ObjectFile *> objs;
    for (ObjectFile *file : files)
      if (!file->is_in_archive())
        objs.push_back(file);

    tbb::parallel_do(
      objs.begin(), objs.end(),
      [&](ObjectFile *file, tbb::parallel_do_feeder<ObjectFile *>& feeder) {
        file->register_undefined_symbols(feeder);
      });

    for_each(files, [](ObjectFile *file) { file->hanlde_undefined_weak_symbols(); });
  }

  // Eliminate unused archive members.
  files.erase(std::remove_if(files.begin(), files.end(),
                             [](ObjectFile *file){ return !file->is_alive; }),
              files.end());

  files.push_back(create_internal_file());

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

  // Assign offsets within an output section to input sections.
  {
    MyTimer t("isec_offsets", before_copy);
    set_isec_offsets();
  }

  // Create linker-synthesized sections.
  out::ehdr = new OutputEhdr;
  out::phdr = new OutputPhdr;
  out::shdr = new OutputShdr;
  //  out::interp = new InterpSection;
  out::got = new GotSection(".got");
  out::gotplt = new GotSection(".got.plt");
  out::relplt = new RelPltSection;
  out::shstrtab = new ShstrtabSection;
  out::symtab = new SymtabSection;
  out::strtab = new StrtabSection;

  // Scan relocations to fix the sizes of .got, .plt, .got.plt, .dynstr,
  // .rela.dyn, .rela.plt.
  {
    MyTimer t("scan_rel", before_copy);

    std::atomic_int32_t num_got = 0;
    std::atomic_int32_t num_gotplt = 0;
    std::atomic_int32_t num_plt = 0;
    std::atomic_int32_t num_relplt = 0;

    for_each(files, [&](ObjectFile *file) {
                      i32 got = 0, gotplt = 0, plt = 0, relplt = 0;

                      for (InputSection *isec : file->sections)
                        if (isec)
                          isec->scan_relocations(got, gotplt, plt, relplt);

                      num_got += got;
                      num_gotplt += gotplt;
                      num_plt += plt;
                      num_relplt += relplt;
                    });

    out::got->size = num_got * 8;
    out::gotplt->size = num_gotplt * 8;
  }

  // Compute .symtab and .strtab sizes
  {
    MyTimer t("symtab_size", before_copy);
    for_each(files, [](ObjectFile *file) { file->compute_symtab(); });

    for (ObjectFile *file : files) {
      out::symtab->size += file->local_symtab_size + file->global_symtab_size;
      out::strtab->size += file->local_strtab_size + file->global_strtab_size;
    }
  }

  // Assign symbols to GOT offsets
  {
    MyTimer t("got");
    u64 offset = 0;

    out::got->symbols.reserve(out::got->size / 8);

    for (ObjectFile *file : files) {
      for (Symbol *sym : file->symbols) {
        if (sym->file != file)
          continue;

        if (sym->got_offset == -1) {
          out::got->symbols.push_back({GotSection::REGULAR, sym});
          sym->got_offset = offset;
          offset += 8;
        } else if (sym->gottp_offset == -1) {
          out::got->symbols.push_back({GotSection::TP, sym});
          sym->gottp_offset = offset;
          offset += 8;
        }
      }
    }

    llvm::outs() << "offset=" << offset
                 << " got->size=" << out::got->size
                 << "\n";
    llvm::outs().flush();
    assert(offset == out::got->size);
  }

  // Add output sections.
  std::vector<OutputChunk *> output_chunks;
  for (OutputSection *osec : OutputSection::instances)
    if (!osec->empty())
      output_chunks.push_back(osec);

  if (out::got->size)
    output_chunks.push_back(out::got);
  if (out::gotplt->size)
    output_chunks.push_back(out::gotplt);

  sort_output_chunks(output_chunks);

  // Add ELF header, program header and .interp to the output.
  output_chunks.insert(output_chunks.begin(), out::ehdr);
  output_chunks.insert(output_chunks.begin() + 1, out::phdr);
  // output_chunks.insert(output_chunks.begin() + 2, out::interp);

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
  out::symtab->shdr.sh_link = out::strtab->shndx;

  // Fill section header.
  fill_shdrs(output_chunks);

  // Assign offsets to output sections
  u64 filesize = 0;
  {
    MyTimer t("osec_offset", before_copy);
    filesize = set_osec_offsets(output_chunks);
  }

  // Attach linker-synthesized symbols to sections.
  {
    // __bss_start
    for (OutputChunk *chunk : output_chunks) {
      if (chunk->name == ".bss" && !chunk->sections.empty()) {
        out::__bss_start->input_section = chunk->sections[0];
        break;
      }
    }

    // __ehdr_start
    for (OutputChunk *chunk : output_chunks) {
      if (chunk->shndx == 1) {
        out::__ehdr_start->input_section = chunk->sections[0];
        out::__ehdr_start->addr = out::ehdr->shdr.sh_addr - chunk->shdr.sh_addr;
        break;
      }
    }
  }

  // Fix symbol addresses.
  {
    MyTimer t("sym_addr");
    for_each(files, [](ObjectFile *file) { file->fix_sym_addrs(); });

    for (OutputChunk *chunk : output_chunks)
      if (chunk->shdr.sh_flags & SHF_TLS)
        out::tls_end = chunk->shdr.sh_addr + chunk->shdr.sh_size;
  }

  tbb::task_group unlink_tg;
  {
    MyTimer t("unlink");
    unlink_async(unlink_tg, config.output);
  }

  // Create an output file
  std::unique_ptr<FileOutputBuffer> output_buffer;

  {
    MyTimer t("open");
    Expected<std::unique_ptr<FileOutputBuffer>> buf_or_err =
      FileOutputBuffer::create(config.output, filesize, FileOutputBuffer::F_executable);

    if (!buf_or_err)
      error("failed to open " + config.output + ": " +
            llvm::toString(buf_or_err.takeError()));

    output_buffer = std::move(*buf_or_err);
  }

  u8 *buf = output_buffer->getBufferStart();

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

  out::shdr->copy_to(buf);

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

  if (config.print_map) {
    MyTimer t("print_map");
    print_map(files, output_chunks);
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
