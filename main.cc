#include "mold.h"

#include "llvm/BinaryFormat/Magic.h"
#include "llvm/Option/ArgList.h"
#include "llvm/Support/FileOutputBuffer.h"

#include <iostream>

using namespace llvm;
using namespace llvm::ELF;

using llvm::object::Archive;
using llvm::opt::InputArgList;

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
  auto mb_or_err = MemoryBuffer::getFile(path, -1, false);
  if (auto ec = mb_or_err.getError())
    error("cannot open " + path + ": " + ec.message());

  std::unique_ptr<MemoryBuffer> &mb = *mb_or_err;
  MemoryBufferRef mbref = mb->getMemBufferRef();
  mb.release();

  switch (identify_magic(mbref.getBuffer())) {
  case file_magic::archive:
    for (MemoryBufferRef member : get_archive_members(mbref))
      files.push_back(new ObjectFile(member, path));
    break;
  case file_magic::elf_relocatable:
  case file_magic::elf_shared_object:
    files.push_back(new ObjectFile(mbref, ""));
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

static void handle_mergeable_strings(std::vector<ObjectFile *> &files) {
  static Counter counter("merged_strings");
  for (MergedSection *osec : MergedSection::instances)
    counter.inc(osec->map.size());

  // Resolve mergeable string pieces
  tbb::parallel_for_each(files, [](ObjectFile *file) {
    for (InputSection *isec : file->mergeable_sections) {
      for (StringPieceRef &ref : isec->pieces) {
        InputSection *cur = ref.piece->isec;
        while (!cur || cur->file->priority > isec->file->priority)
          if (ref.piece->isec.compare_exchange_strong(cur, isec))
            break;
      }
    }
  });

  // Calculate the total bytes of mergeable strings for each input section.
  tbb::parallel_for_each(files, [](ObjectFile *file) {
    for (InputSection *isec : file->mergeable_sections) {
      u32 offset = 0;
      for (StringPieceRef &ref : isec->pieces) {
        if (ref.piece->isec == isec) {
          ref.piece->output_offset = offset;
          offset += ref.piece->data.size() + 1;
        }
      }
      isec->merged_size = offset;
    }
  });

  // Assign each mergeable input section a unique index.
  for (ObjectFile *file : files) {
    for (InputSection *isec : file->mergeable_sections) {
      MergedSection *osec = isec->merged_section;
      isec->merged_offset = osec->shdr.sh_size;
      osec->shdr.sh_size += isec->merged_size;
    }
  }
}

static void bin_sections(std::vector<ObjectFile *> &files) {
  int unit = (files.size() + 127) / 128;
  std::vector<ArrayRef<ObjectFile *>> slices = split(files, unit);

  int num_osec = OutputSection::instances.size();

  std::vector<std::vector<std::vector<InputSection *>>> groups(slices.size());
  for (int i = 0; i < groups.size(); i++)
    groups[i].resize(num_osec);

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

  std::vector<int> sizes(num_osec);

  for (ArrayRef<std::vector<InputSection *>> group : groups)
    for (int i = 0; i < group.size(); i++)
      sizes[i] += group[i].size();

  tbb::parallel_for(0, num_osec, [&](int j) {
    OutputSection::instances[j]->sections.reserve(sizes[j]);

    for (int i = 0; i < groups.size(); i++) {
      std::vector<InputSection *> &sections = OutputSection::instances[j]->sections;
      sections.insert(sections.end(), groups[i][j].begin(), groups[i][j].end());
    }
  });
}

static void set_isec_offsets() {
#if 1
  tbb::parallel_for_each(OutputSection::instances, [&](OutputSection *osec) {
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
  tbb::parallel_for_each(OutputSection::instances, [&](OutputSection *osec) {
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

static void scan_rels(ArrayRef<ObjectFile *> files) {
  tbb::parallel_for_each(files, [&](ObjectFile *file) { file->scan_relocations(); });

  u32 got_offset = 0;
  u32 gotplt_offset = 0;
  u32 plt_offset = 0;
  u32 relplt_offset = 0;

  for (ObjectFile *file : files) {
    file->got_offset = got_offset;
    got_offset += file->num_got * 8;

    file->gotplt_offset = gotplt_offset;
    gotplt_offset += file->num_gotplt * 8;

    file->plt_offset = plt_offset;
    plt_offset += file->num_plt * 16;

    file->relplt_offset = relplt_offset;
    relplt_offset += file->num_relplt * sizeof(ELF64LE::Rela);
  }

  out::got->shdr.sh_size = got_offset;
  out::gotplt->shdr.sh_size = gotplt_offset;
  out::plt->shdr.sh_size = plt_offset;
  out::relplt->shdr.sh_size = relplt_offset;
}

static void assign_got_offsets(ArrayRef<ObjectFile *> files) {
  tbb::parallel_for_each(files, [&](ObjectFile *file) {
    u32 got_offset = file->got_offset;
    u32 gotplt_offset = file->gotplt_offset;
    u32 plt_offset = file->plt_offset;
    u32 relplt_offset = file->relplt_offset;

    for (Symbol *sym : file->symbols) {
      if (sym->file != file)
        continue;

      u8 flags = sym->flags.load(std::memory_order_relaxed);

      if (flags & Symbol::NEEDS_GOT) {
        sym->got_offset = got_offset;
        got_offset += 8;
      }

      if (flags & Symbol::NEEDS_GOTTP) {
        sym->gottp_offset = got_offset;
        got_offset += 8;
      }

      if (flags & Symbol::NEEDS_PLT) {
        // Write a .got.plt entry
        sym->gotplt_offset = gotplt_offset;
        gotplt_offset += 8;

        // Write a .plt entry
        sym->plt_offset = plt_offset;
        plt_offset += 16;

        // Write a .rela.dyn entry
        sym->relplt_offset = relplt_offset;
        relplt_offset += sizeof(ELF64LE::Rela);
      }
    }
  });
}

static void write_got(u8 *buf, ArrayRef<ObjectFile *> files) {
  u8 *got = buf + out::got->shdr.sh_offset;
  u8 *plt = buf + out::plt->shdr.sh_offset;
  u8 *relplt = buf + out::relplt->shdr.sh_offset;

  tbb::parallel_for_each(files, [&](ObjectFile *file) {
    for (Symbol *sym : file->symbols) {
      if (sym->file != file)
        continue;

      u8 flags = sym->flags.load(std::memory_order_relaxed);

      if (flags & Symbol::NEEDS_GOT)
        *(u64 *)(got + sym->got_offset) = sym->get_addr();

      if (flags & Symbol::NEEDS_GOTTP)
        *(u64 *)(got + sym->gottp_offset) = sym->get_addr() - out::tls_end;

      if (flags & Symbol::NEEDS_PLT) {
        // Write a .plt entry
        u64 S = out::gotplt->shdr.sh_addr + sym->gotplt_offset;
        u64 P = out::plt->shdr.sh_addr + sym->plt_offset;
        out::plt->write_entry(plt + sym->plt_offset, S - P - 6);

        // Write a .rela.dyn entry
        auto *rel = (ELF64LE::Rela *)(relplt + sym->relplt_offset);
        rel->r_offset = out::gotplt->shdr.sh_addr + sym->gotplt_offset;
        rel->setType(R_X86_64_IRELATIVE, false);
        rel->r_addend = sym->get_addr();
      }
    }
  });
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
create_shdr(ArrayRef<OutputChunk *> output_chunks) {
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

static u32 to_phdr_flags(u64 sh_flags) {
  u32 ret = PF_R;
  if (sh_flags & SHF_WRITE)
    ret |= PF_W;
  if (sh_flags & SHF_EXECINSTR)
    ret |= PF_X;
  return ret;
}

static std::vector<OutputPhdr::Entry>
create_phdr(ArrayRef<OutputChunk *> output_chunks) {
  std::vector<OutputPhdr::Entry> entries;

  auto add = [&](u32 type, u32 flags, u32 align, std::vector<OutputChunk *> members) {
    ELF64LE::Phdr phdr = {};
    phdr.p_type = type;
    phdr.p_flags = flags;
    phdr.p_align = align;
    entries.push_back({phdr, members});
  };

  // Create a PT_PHDR for the program header itself.
  add(PT_PHDR, PF_R, 8, {out::phdr});

  // Create an PT_INTERP.
  if (out::interp)
    add(PT_INTERP, PF_R, 1, {out::interp});

  // Create PT_LOAD segments.
  bool first = true;
  bool last_was_bss;

  for (OutputChunk *chunk : output_chunks) {
    if (!(chunk->shdr.sh_flags & SHF_ALLOC))
      break;

    u32 flags = to_phdr_flags(chunk->shdr.sh_flags);
    bool this_is_bss =
      (chunk->shdr.sh_type == SHT_NOBITS && !(chunk->shdr.sh_flags & SHF_TLS));

    if (first) {
      add(PT_LOAD, flags, PAGE_SIZE, {chunk});
      last_was_bss = this_is_bss;
      first = false;
      continue;
    }

    if (entries.back().phdr.p_flags != flags || (last_was_bss && !this_is_bss))
      add(PT_LOAD, flags, PAGE_SIZE, {chunk});
    else
      entries.back().members.push_back(chunk);

    last_was_bss = this_is_bss;
  }

  // Create a PT_TLS.
  for (int i = 0; i < output_chunks.size(); i++) {
    if (output_chunks[i]->shdr.sh_flags & SHF_TLS) {
      std::vector<OutputChunk *> vec = {output_chunks[i++]};
      while (i < output_chunks.size() && (output_chunks[i]->shdr.sh_flags & SHF_TLS))
        vec.push_back(output_chunks[i++]);
      add(PT_TLS, to_phdr_flags(output_chunks[i]->shdr.sh_flags), 1, vec);
    }
  }

  for (OutputPhdr::Entry &ent : entries)
    for (OutputChunk *chunk : ent.members)
      ent.phdr.p_align = std::max(ent.phdr.p_align, chunk->shdr.sh_addralign);

  for (OutputPhdr::Entry &ent : entries)
    if (ent.phdr.p_type == PT_LOAD)
      ent.members.front()->starts_new_ptload = true;

  return entries;
}

static u64 set_osec_offsets(ArrayRef<OutputChunk *> output_chunks) {
  u64 fileoff = 0;
  u64 vaddr = 0x200000;

  for (OutputChunk *chunk : output_chunks) {
    if (chunk->starts_new_ptload)
      vaddr = align_to(vaddr, PAGE_SIZE);

    bool is_bss = chunk->shdr.sh_type == SHT_NOBITS;

    if (!is_bss) {
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

    if (!is_bss)
      fileoff += chunk->shdr.sh_size;

    bool is_tbss = is_bss && (chunk->shdr.sh_flags & SHF_TLS);
    if (!is_tbss)
      vaddr += chunk->shdr.sh_size;
  }
  return fileoff;
}

static void fix_synthetic_symbols(ArrayRef<OutputChunk *> output_chunks) {
  auto start = [&](OutputChunk *chunk, Symbol *sym) {
                 if (sym) {
                   sym->shndx = chunk->shndx;
                   sym->value = chunk->shdr.sh_addr;
                 }
               };

  auto stop = [&](OutputChunk *chunk, Symbol *sym) {
                if (sym) {
                  sym->shndx = chunk->shndx;
                  sym->value = chunk->shdr.sh_addr + chunk->shdr.sh_size;
                }
              };

  // __bss_start
  for (OutputChunk *chunk : output_chunks) {
    if (chunk->name == ".bss" && !chunk->sections.empty()) {
      start(chunk, out::__bss_start);
      break;
    }
  }

  // __ehdr_start
  for (OutputChunk *chunk : output_chunks) {
    if (chunk->shndx == 1) {
      out::__ehdr_start->shndx = 1;
      out::__ehdr_start->value = out::ehdr->shdr.sh_addr - chunk->shdr.sh_addr;
      break;
    }
  }

  // __rela_iplt_start and __rela_iplt_end
  start(out::relplt, out::__rela_iplt_start);
  stop(out::relplt, out::__rela_iplt_end);

  // __{init,fini}_array_{start,end}
  for (OutputChunk *chunk : output_chunks) {
    switch (chunk->shdr.sh_type) {
    case SHT_INIT_ARRAY:
      start(chunk, out::__init_array_start);
      stop(chunk, out::__init_array_end);
      break;
    case SHT_FINI_ARRAY:
      start(chunk, out::__fini_array_start);
      stop(chunk, out::__fini_array_end);
      break;
    }
  }

  // _end, end, _etext, etext, _edata and edata
  for (OutputChunk *chunk : output_chunks) {
    if (chunk->sections.empty())
      continue;

    if (chunk->shdr.sh_flags & SHF_ALLOC) {
      stop(chunk, out::end);
      stop(chunk, out::_end);
    }

    if (chunk->shdr.sh_flags & SHF_EXECINSTR) {
      stop(chunk, out::etext);
      stop(chunk, out::_etext);
    }

    if (chunk->shdr.sh_type != SHT_NOBITS && chunk->shdr.sh_flags & SHF_ALLOC) {
      stop(chunk, out::edata);
      stop(chunk, out::_edata);
    }
  }

  // __start_ and __stop_ symbols
  for (OutputChunk *chunk : output_chunks) {
    if (!is_c_identifier(chunk->name))
      continue;

    start(chunk, Symbol::intern(("__start_" + chunk->name).str()));
    stop(chunk, Symbol::intern(("__stop_" + chunk->name).str()));
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

static FileOutputBuffer *open_output_file(u64 filesize) {
  Expected<std::unique_ptr<FileOutputBuffer>> buf_or_err =
    FileOutputBuffer::create(config.output, filesize, FileOutputBuffer::F_executable);

  if (!buf_or_err)
    error("failed to open " + config.output + ": " +
          llvm::toString(buf_or_err.takeError()));

  return std::move(*buf_or_err).release();
}

static void write_symtab(u8 *buf, std::vector<ObjectFile *> files) {
  std::vector<u64> symtab_off(files.size() + 1);
  std::vector<u64> strtab_off(files.size() + 1);
  symtab_off[0] = sizeof(ELF64LE::Sym);
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

  assert(symtab_off.back() == out::symtab->shdr.sh_size);
  assert(strtab_off.back() == out::strtab->shdr.sh_size);

  tbb::parallel_for((size_t)0, files.size(),
                    [&](size_t i) {
                      files[i]->write_global_symtab(buf, symtab_off[i], strtab_off[i]);
                    });
}

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

  Counter::enabled = args.hasArg(OPT_stat);

  if (auto *arg = args.getLastArg(OPT_o))
    config.output = arg->getValue();
  else
    error("-o option is missing");

  config.print_map = args.hasArg(OPT_print_map);
  config.is_static = args.hasArg(OPT_static);

  for (auto *arg : args.filtered(OPT_trace_symbol))
    Symbol::intern(arg->getValue())->traced = true;

  std::vector<ObjectFile *> files;

  llvm::TimerGroup parse("parse", "parse");
  llvm::TimerGroup before_copy("before_copy", "before_copy");
  llvm::TimerGroup copy("copy", "copy");

  // Open input files
  {
    MyTimer t("open", parse);
    for (auto *arg : args)
      if (arg->getOption().getID() == OPT_INPUT)
        read_file(files, arg->getValue());
  }

  // Parse input files
  {
    MyTimer t("parse", parse);
    tbb::parallel_for_each(files, [](ObjectFile *file) { file->parse(); });
  }

  {
    MyTimer t("merge", parse);
    tbb::parallel_for_each(files, [](ObjectFile *file) {
      file->initialize_mergeable_sections();
    });
  }

  Timer total_timer("total", "total");
  total_timer.startTimer();

  // Set priorities to files
  int priority = 1;
  for (ObjectFile *file : files)
    if (!file->is_in_archive)
      file->priority = priority++;
  for (ObjectFile *file : files)
    if (file->is_in_archive)
      file->priority = priority++;

  // Resolve symbols
  {
    MyTimer t("resolve_symbols", before_copy);

    tbb::parallel_for_each(files, [](ObjectFile *file) { file->resolve_symbols(); });

    // Resolve symbols
    std::vector<ObjectFile *> root;
    for (ObjectFile *file : files)
      if (file->is_alive)
        root.push_back(file);

    // Mark archive members we include into the final output.
    tbb::parallel_do(
      root,
      [&](ObjectFile *file, tbb::parallel_do_feeder<ObjectFile *> &feeder) {
        file->mark_live_archive_members(feeder);
      });

    // Eliminate unused archive members.
    files.erase(std::remove_if(files.begin(), files.end(),
                               [](ObjectFile *file){ return !file->is_alive; }),
                files.end());

    // Convert weak symbols to absolute symbols with value 0.
    tbb::parallel_for_each(files, [](ObjectFile *file) {
      file->hanlde_undefined_weak_symbols();
    });
  }

  if (args.hasArg(OPT_trace))
    for (ObjectFile *file : files)
      llvm::outs() << toString(file) << "\n";

  // Eliminate duplicate comdat groups.
  {
    MyTimer t("comdat", before_copy);
    tbb::parallel_for_each(files, [](ObjectFile *file) {
      file->eliminate_duplicate_comdat_groups();
    });
  }

  // Resolve mergeable strings
  {
    MyTimer t("resolve_strings", before_copy);
    handle_mergeable_strings(files);
  }

  // Create .bss sections for common symbols.
  {
    MyTimer t("common", before_copy);
    tbb::parallel_for_each(files,
                           [](ObjectFile *file) { file->convert_common_symbols(); });
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

  std::vector<OutputChunk *> output_chunks;

  for (OutputSection *osec : OutputSection::instances)
    if (!osec->empty())
      output_chunks.push_back(osec);

  for (MergedSection *osec : MergedSection::instances)
    if (osec->shdr.sh_size)
      output_chunks.push_back(osec);

  // Create a dummy file containing linker-synthesized symbols
  // (e.g. `__bss_start`).
  ObjectFile *internal_file = ObjectFile::create_internal_file(output_chunks);
  internal_file->priority = priority++;
  files.push_back(internal_file);

  // Create linker-synthesized sections.
  out::ehdr = new OutputEhdr;
  out::phdr = new OutputPhdr;
  out::shdr = new OutputShdr;
  if (!config.is_static)
    out::interp = new InterpSection;
  out::got = new GotSection(".got");
  out::gotplt = new GotSection(".got.plt");
  out::plt = new PltSection;
  out::relplt = new RelPltSection;
  out::shstrtab = new ShstrtabSection;
  out::symtab = new SymtabSection;
  out::strtab = new StrtabSection;

  // Scan relocations to fix the sizes of .got, .plt, .got.plt, .dynstr,
  // .rela.dyn, .rela.plt.
  {
    MyTimer t("scan_rels", before_copy);
    scan_rels(files);
  }

  // Compute .symtab and .strtab sizes
  {
    MyTimer t("symtab_size", before_copy);
    tbb::parallel_for_each(files, [](ObjectFile *file) { file->compute_symtab(); });

    for (ObjectFile *file : files) {
      out::symtab->shdr.sh_size += file->local_symtab_size + file->global_symtab_size;
      out::strtab->shdr.sh_size += file->local_strtab_size + file->global_strtab_size;
    }
  }

  // Add output sections.
  if (out::got->shdr.sh_size)
    output_chunks.push_back(out::got);
  if (out::plt->shdr.sh_size)
    output_chunks.push_back(out::plt);
  if (out::gotplt->shdr.sh_size)
    output_chunks.push_back(out::gotplt);
  if (out::relplt->shdr.sh_size)
    output_chunks.push_back(out::relplt);

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
  out::shdr->set_entries(create_shdr(output_chunks));
  out::phdr->set_entries(create_phdr(output_chunks));
  out::symtab->shdr.sh_link = out::strtab->shndx;

  // Assign offsets to output sections
  u64 filesize = 0;
  {
    MyTimer t("osec_offset", before_copy);
    filesize = set_osec_offsets(output_chunks);
  }

  // Assign symbols to GOT offsets
  {
    MyTimer t("assign_got_offsets", before_copy);
    assign_got_offsets(files);
  }

  // Fix linker-synthesized symbol addresses.
  fix_synthetic_symbols(output_chunks);

  for (OutputChunk *chunk : output_chunks) {
    ELF64LE::Shdr &shdr = chunk->shdr;
    if (shdr.sh_flags & SHF_TLS)
      out::tls_end = align_to(shdr.sh_addr + shdr.sh_size, shdr.sh_addralign);
  }

  tbb::task_group tg_unlink;
  {
    MyTimer t("unlink");
    unlink_async(tg_unlink, config.output);
  }

  // Create an output file
  FileOutputBuffer *output_buffer;

  {
    MyTimer t("open_output_file");
    output_buffer = open_output_file(filesize);
  }

  u8 *buf = output_buffer->getBufferStart();

  // Fill .symtab and .strtab
  {
    MyTimer t("write_symtab", copy);
    write_symtab(buf, files);
  }

  // Copy input sections to the output file
  {
    MyTimer t("copy", copy);
    tbb::parallel_for_each(output_chunks, [&](OutputChunk *chunk) {
      chunk->copy_to(buf);
    });
  }

  // Fill .plt, .got, got.plt and .rela.plt sections
  {
    MyTimer t("write_got", copy);
    write_got(buf, files);
  }

  // Fill mergeable string sections
  {
    MyTimer t("write_merged_strings", copy);

    tbb::parallel_for_each(files, [&](ObjectFile *file) {
      for (InputSection *isec : file->mergeable_sections) {
        MergedSection *osec = isec->merged_section;
        u8 *base = buf + osec->shdr.sh_offset + isec->merged_offset;

        for (StringPieceRef &ref : isec->pieces) {
          StringPiece &piece = *ref.piece;
          if (piece.isec == isec)
            memcpy(base + piece.output_offset, piece.data.data(), piece.data.size());
        }
      }
    });
  }

  {
    MyTimer t("commit", copy);
    if (auto e = output_buffer->commit())
      error("failed to write to the output file: " + toString(std::move(e)));
  }

  total_timer.stopTimer();

  {
    MyTimer t("unlink_wait");
    tg_unlink.wait();
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

  // Show stat numbers
  Counter num_input_sections("input_sections");
  for (ObjectFile *file : files)
    num_input_sections.inc(file->sections.size());

  Counter num_output_chunks("output_chunks", output_chunks.size());
  Counter num_files("files", files.size());
  Counter filesize_counter("filesize", filesize);

  Counter::print();
  llvm::TimerGroup::printAll(llvm::outs());

  llvm::outs().flush();
  _exit(0);
}
