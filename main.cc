#include "mold.h"

#include "llvm/BinaryFormat/Magic.h"
#include "llvm/Option/ArgList.h"
#include "llvm/Support/FileOutputBuffer.h"

#include <fcntl.h>
#include <iostream>
#include <libgen.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

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

llvm::TimerGroup parse_timer("parse", "parse");
llvm::TimerGroup before_copy_timer("before_copy", "before_copy");
llvm::TimerGroup copy_timer("copy", "copy");

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

static void read_file(StringRef path) {
  int fd = open(path.str().c_str(), O_RDONLY);
  if (fd == -1)
    error("cannot open " + path);

  struct stat st;
  if (fstat(fd, &st) == -1)
    error(path + ": stat failed");

  void *addr = mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
  if (addr == MAP_FAILED)
    error(path + ": mmap failed: " + strerror(errno));
  close(fd);

  auto &mb = *new MemoryBufferRef(StringRef((char *)addr, st.st_size), path);

  switch (identify_magic(mb.getBuffer())) {
  case file_magic::archive:
    for (MemoryBufferRef member : get_archive_members(mb))
      out::files.push_back(new ObjectFile(member, path));
    break;
  case file_magic::elf_relocatable:
  case file_magic::elf_shared_object:
    out::files.push_back(new ObjectFile(mb, ""));
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

static void resolve_symbols() {
  MyTimer t("resolve_symbols", before_copy_timer);

  // Register defined symbols
  tbb::parallel_for_each(out::files,
                         [](ObjectFile *file) { file->resolve_symbols(); });


  // Mark archive members we include into the final output.
  std::vector<ObjectFile *> root;
  for (ObjectFile *file : out::files)
    if (file->is_alive && !file->is_dso)
      root.push_back(file);

  tbb::parallel_do(
    root,
    [&](ObjectFile *file, tbb::parallel_do_feeder<ObjectFile *> &feeder) {
      file->mark_live_archive_members(feeder);
    });

  // Eliminate unused archive members.
  out::files.erase(std::remove_if(out::files.begin(), out::files.end(),
                                  [](ObjectFile *file){ return !file->is_alive; }),
                   out::files.end());

  // Convert weak symbols to absolute symbols with value 0.
  tbb::parallel_for_each(out::files, [](ObjectFile *file) {
    file->hanlde_undefined_weak_symbols();
  });
}

static void eliminate_comdats() {
  MyTimer t("comdat", before_copy_timer);

  tbb::parallel_for_each(out::files, [](ObjectFile *file) {
    file->resolve_comdat_groups();
  });

  tbb::parallel_for_each(out::files, [](ObjectFile *file) {
    file->eliminate_duplicate_comdat_groups();
  });
}

static void handle_mergeable_strings() {
  MyTimer t("resolve_strings", before_copy_timer);

  // Resolve mergeable string pieces
  tbb::parallel_for_each(out::files, [](ObjectFile *file) {
    for (MergeableSection &isec : file->mergeable_sections) {
      for (StringPieceRef &ref : isec.pieces) {
        MergeableSection *cur = ref.piece->isec;
        while (!cur || cur->file->priority > isec.file->priority)
          if (ref.piece->isec.compare_exchange_strong(cur, &isec))
            break;
      }
    }
  });

  // Calculate the total bytes of mergeable strings for each input section.
  tbb::parallel_for_each(out::files, [](ObjectFile *file) {
    for (MergeableSection &isec : file->mergeable_sections) {
      u32 offset = 0;
      for (StringPieceRef &ref : isec.pieces) {
        StringPiece &piece = *ref.piece;
        if (piece.isec == &isec && piece.output_offset == -1) {
          ref.piece->output_offset = offset;
          offset += ref.piece->data.size();
        }
      }
      isec.size = offset;
    }
  });

  // Assign each mergeable input section a unique index.
  for (ObjectFile *file : out::files) {
    for (MergeableSection &isec : file->mergeable_sections) {
      MergedSection &osec = isec.parent;
      isec.offset = osec.shdr.sh_size;
      osec.shdr.sh_size += isec.size;
    }
  }

  static Counter counter("merged_strings");
  for (MergedSection *osec : MergedSection::instances)
    counter.inc(osec->map.size());
}

// So far, each input section has a pointer to its corresponding
// output section, but there's no reverse edge to get a list of
// input sections from an output section. This function creates it.
//
// An output section may contain millions of input sections.
// So, we append input sections to output sections in parallel.
static void bin_sections() {
  MyTimer t("bin_sections", before_copy_timer);

  int unit = (out::files.size() + 127) / 128;
  std::vector<ArrayRef<ObjectFile *>> slices = split(out::files, unit);

  int num_osec = OutputSection::instances.size();

  std::vector<std::vector<std::vector<InputChunk *>>> groups(slices.size());
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

  for (ArrayRef<std::vector<InputChunk *>> group : groups)
    for (int i = 0; i < group.size(); i++)
      sizes[i] += group[i].size();

  tbb::parallel_for(0, num_osec, [&](int j) {
    OutputSection::instances[j]->members.reserve(sizes[j]);

    for (int i = 0; i < groups.size(); i++) {
      std::vector<InputChunk *> &sections = OutputSection::instances[j]->members;
      sections.insert(sections.end(), groups[i][j].begin(), groups[i][j].end());
    }
  });
}

static void set_isec_offsets() {
  MyTimer t("isec_offsets", before_copy_timer);

  tbb::parallel_for_each(OutputSection::instances, [&](OutputSection *osec) {
    if (osec->members.empty())
      return;

    std::vector<ArrayRef<InputChunk *>> slices = split(osec->members, 100000);
    std::vector<u64> size(slices.size());
    std::vector<u32> alignments(slices.size());

    tbb::parallel_for(0, (int)slices.size(), [&](int i) {
      u64 off = 0;
      u32 align = 1;

      for (InputChunk *isec : slices[i]) {
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
      start[i] = align_to(start[i - 1] + size[i - 1], align);

    tbb::parallel_for(1, (int)slices.size(), [&](int i) {
      for (InputChunk *isec : slices[i])
        isec->offset += start[i];
    });

    osec->shdr.sh_size = start.back() + size.back();
    osec->shdr.sh_addralign = align;
  });
}

static void scan_rels() {
  MyTimer t("scan_rels", before_copy_timer);

  tbb::parallel_for_each(out::files, [&](ObjectFile *file) {
    for (InputSection *isec : file->sections)
      if (isec)
        isec->scan_relocations();
  });

  std::vector<std::vector<Symbol *>> vec(out::files.size());

  tbb::parallel_for(0, (int)out::files.size(), [&](int i) {
    ObjectFile *file = out::files[i];
    for (Symbol *sym : file->symbols)
      if (sym->file == file && sym->flags)
        vec[i].push_back(sym);
  });

  out::dynsyms = flatten(vec);

  u32 got_idx = 0;
  u32 plt_idx = 0;
  u32 gotplt_idx = 0;
  u32 relplt_idx = 0;
  u32 reldyn_idx = 0;

  for (Symbol *sym : out::dynsyms) {
    if (sym->flags & Symbol::NEEDS_GOT) {
      llvm::outs() << "got=" << sym->name << "\n";
      sym->got_idx = got_idx++;

      if (!config.is_static) {
        out::dynsym->add_symbol(sym);
        sym->reldyn_idx = reldyn_idx++;
      }
    }

    if (sym->flags & Symbol::NEEDS_PLT) {
      sym->plt_idx = plt_idx++;

      if (sym->got_idx == -1) {
        sym->gotplt_idx = gotplt_idx++;
        sym->relplt_idx = relplt_idx++;
        sym->reldyn_idx = reldyn_idx++;
      }

      if (!config.is_static)
        out::dynsym->add_symbol(sym);
    }

    if ((sym->flags & Symbol::NEEDS_TLSGD) || (sym->flags & Symbol::NEEDS_TLSLD))
      error("not implemented");

    if (sym->flags & Symbol::NEEDS_GOTTPOFF)
      sym->gottp_idx = got_idx++;
  }

  out::relplt->shdr.sh_size = relplt_idx * sizeof(ELF64LE::Rela);
  out::reldyn->shdr.sh_size = reldyn_idx * sizeof(ELF64LE::Rela);
}

static void write_dynamic_rel(u8 *buf, u8 type, u64 addr, int dynsym_idx, u64 addend) {
  ELF64LE::Rela *rel = (ELF64LE::Rela *)buf;
  memset(rel, 0, sizeof(*rel));
  rel->setSymbolAndType(dynsym_idx, type, false);
  rel->r_offset = addr;
  rel->r_addend = addend;
}

static void write_got_plt() {
  MyTimer t("write_synthetic", copy_timer);

  u8 *got_buf = out::buf + out::got->shdr.sh_offset;
  u8 *gotplt_buf = out::buf + out::gotplt->shdr.sh_offset;
  u8 *plt_buf = out::buf + out::plt->shdr.sh_offset;
  u8 *relplt_buf = out::buf + out::relplt->shdr.sh_offset;

  tbb::parallel_for_each(out::dynsyms, [&](Symbol *sym) {
    if (sym->got_idx != -1) {
      if (config.is_static) {
        *(u64 *)(got_buf + sym->got_idx * GOT_SIZE) = sym->get_addr();
      } else {
        u8 *reldyn_buf = out::buf + out::reldyn->shdr.sh_offset;
        write_dynamic_rel(reldyn_buf + sym->reldyn_idx * sizeof(ELF64LE::Rela),
                          R_X86_64_GLOB_DAT, sym->get_got_addr(),
                          sym->dynsym_idx, 0);
      }
    }

    if (sym->gottp_idx != -1)
      *(u64 *)(got_buf + sym->gottp_idx * GOT_SIZE) = sym->get_addr() - out::tls_end;

    if (sym->gotgd_idx != -1 || sym->gotld_idx != -1)
      error("not implemented");

    if (sym->plt_idx != -1)
      out::plt->write_entry(sym);

    if (sym->relplt_idx != -1) {
      if (sym->type == STT_GNU_IFUNC) {
        write_dynamic_rel(relplt_buf + sym->relplt_idx * sizeof(ELF64LE::Rela),
                          R_X86_64_IRELATIVE, sym->get_gotplt_addr(),
                          sym->dynsym_idx, sym->get_addr());
      } else {
        write_dynamic_rel(relplt_buf + sym->relplt_idx * sizeof(ELF64LE::Rela),
                          R_X86_64_JUMP_SLOT, sym->get_gotplt_addr(),
                          sym->dynsym_idx, 0);
        *(u64 *)(gotplt_buf + sym->gotplt_idx * sizeof(ELF64LE::Sym)) =
          sym->get_plt_addr() + 6;
      }
    }
  });
}

static void write_merged_strings() {
  MyTimer t("write_merged_strings", copy_timer);

  tbb::parallel_for_each(out::files, [&](ObjectFile *file) {
    for (MergeableSection &isec : file->mergeable_sections) {
      u8 *base = out::buf + isec.parent.shdr.sh_offset + isec.offset;

      for (StringPieceRef &ref : isec.pieces) {
        StringPiece &piece = *ref.piece;
        if (piece.isec == &isec)
          memcpy(base + piece.output_offset, piece.data.data(), piece.data.size());
      }
    }
  });
}

static void clear_padding(u64 filesize) {
  MyTimer t("clear_padding", copy_timer);

  auto zero = [&](OutputChunk *chunk, u64 next_start) {
    u64 pos = chunk->shdr.sh_offset;
    if (chunk->shdr.sh_type != SHT_NOBITS)
      pos += chunk->shdr.sh_size;
    memset(out::buf + pos, 0, next_start - pos);
  };

  for (int i = 1; i < out::chunks.size(); i++)
    zero(out::chunks[i - 1], out::chunks[i]->shdr.sh_offset);
  zero(out::chunks.back(), filesize);
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
static int get_section_rank(const ELF64LE::Shdr &shdr) {
  bool alloc = shdr.sh_flags & SHF_ALLOC;
  bool writable = shdr.sh_flags & SHF_WRITE;
  bool exec = shdr.sh_flags & SHF_EXECINSTR;
  bool tls = shdr.sh_flags & SHF_TLS;
  bool nobits = shdr.sh_type == SHT_NOBITS;
  return (!alloc << 5) | (writable << 4) | (exec << 3) | (!tls << 2) | nobits;
}

static u64 set_osec_offsets(ArrayRef<OutputChunk *> chunks) {
  MyTimer t("osec_offset", before_copy_timer);

  u64 fileoff = 0;
  u64 vaddr = config.image_base;

  for (OutputChunk *chunk : chunks) {
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

static void fix_synthetic_symbols(ArrayRef<OutputChunk *> chunks) {
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
  for (OutputChunk *chunk : chunks) {
    if (chunk->kind == OutputChunk::REGULAR && chunk->name == ".bss") {
      start(chunk, out::__bss_start);
      break;
    }
  }

  // __ehdr_start
  for (OutputChunk *chunk : chunks) {
    if (chunk->shndx == 1) {
      out::__ehdr_start->shndx = 1;
      out::__ehdr_start->value = out::ehdr->shdr.sh_addr;
      break;
    }
  }

  // __rela_iplt_start and __rela_iplt_end
  start(out::relplt, out::__rela_iplt_start);
  stop(out::relplt, out::__rela_iplt_end);

  // __{init,fini}_array_{start,end}
  for (OutputChunk *chunk : chunks) {
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
  for (OutputChunk *chunk : chunks) {
    if (chunk->kind == OutputChunk::HEADER)
      continue;

    if (chunk->shdr.sh_flags & SHF_ALLOC)
      stop(chunk, out::_end);

    if (chunk->shdr.sh_flags & SHF_EXECINSTR)
      stop(chunk, out::_etext);

    if (chunk->shdr.sh_type != SHT_NOBITS && chunk->shdr.sh_flags & SHF_ALLOC)
      stop(chunk, out::_edata);
  }

  // _DYNAMIC
  if (out::dynamic)
    start(out::dynamic, out::_DYNAMIC);

  // _GLOBAL_OFFSET_TABLE_
  if (out::gotplt)
    start(out::gotplt, out::_GLOBAL_OFFSET_TABLE_);

  // __start_ and __stop_ symbols
  for (OutputChunk *chunk : chunks) {
    if (is_c_identifier(chunk->name)) {
      start(chunk, Symbol::intern(("__start_" + chunk->name).str()));
      stop(chunk, Symbol::intern(("__stop_" + chunk->name).str()));
    }
  }
}

static u8 *open_output_file(u64 filesize) {
  MyTimer t("open_file", before_copy_timer);

  int fd = open(config.output.str().c_str(), O_RDWR | O_CREAT, 0777);
  if (fd == -1)
    error("cannot open " + config.output + ": " + strerror(errno));

  if (ftruncate(fd, filesize))
    error("ftruncate");

  void *buf = mmap(nullptr, filesize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  if (buf == MAP_FAILED)
    error(config.output + ": mmap failed: " + strerror(errno));
  close(fd);

  if (config.filler != -1)
    memset(buf, config.filler, filesize);
  return (u8 *)buf;
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

static int parse_filler(opt::InputArgList &args) {
  auto *arg = args.getLastArg(OPT_filler);
  if (!arg)
    return -1;

  StringRef val = arg->getValue();
  if (!val.startswith("0x"))
    error("invalid argument: " + arg->getAsString(args));
  int ret;
  if (!to_integer(val.substr(2), ret, 16))
    error("invalid argument: " + arg->getAsString(args));
  return (u8)ret;
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
  config.filler = parse_filler(args);

  for (auto *arg : args.filtered(OPT_trace_symbol))
    Symbol::intern(arg->getValue())->traced = true;

  // Open input files
  {
    MyTimer t("open", parse_timer);
    for (auto *arg : args)
      if (arg->getOption().getID() == OPT_INPUT)
        read_file(arg->getValue());
  }

  // Parse input files
  {
    MyTimer t("parse", parse_timer);
    tbb::parallel_for_each(out::files, [](ObjectFile *file) { file->parse(); });
  }

  {
    MyTimer t("merge", parse_timer);
    tbb::parallel_for_each(out::files, [](ObjectFile *file) {
      file->initialize_mergeable_sections();
    });
  }

  Timer total_timer("total", "total");
  total_timer.startTimer();

  out::ehdr = new OutputEhdr;
  out::shdr = new OutputShdr;
  out::phdr = new OutputPhdr;
  out::got = new SpecialSection(".got", SHT_PROGBITS, SHF_ALLOC | SHF_WRITE, 8);
  out::gotplt = new GotPltSection;
  out::relplt = new RelPltSection;
  out::strtab = new StrtabSection(".strtab", 0);
  out::shstrtab = new ShstrtabSection;
  out::plt = new PltSection;
  out::symtab = new SymtabSection;
  out::dynsym = new DynsymSection;
  out::dynstr = new DynstrSection;

  if (!config.is_static) {
    out::interp = new InterpSection;
    out::dynamic = new DynamicSection;
    out::reldyn = new RelDynSection;
    out::hash = new HashSection;
  }

  // Set priorities to files
  int priority = 1;
  for (ObjectFile *file : out::files)
    if (!file->is_in_archive)
      file->priority = priority++;
  for (ObjectFile *file : out::files)
    if (file->is_in_archive)
      file->priority = priority++;

  // Resolve symbols and fix the set of object files that are
  // included to the final output.
  resolve_symbols();

  if (args.hasArg(OPT_trace))
    for (ObjectFile *file : out::files)
      message(toString(file));

  // Remove redundant comdat sections (e.g. duplicate inline functions).
  eliminate_comdats();

  // Merge strings constants in SHF_MERGE sections.
  handle_mergeable_strings();

  // Create .bss sections for common symbols.
  {
    MyTimer t("common", before_copy_timer);
    tbb::parallel_for_each(out::files,
                           [](ObjectFile *file) { file->convert_common_symbols(); });
  }

  // Bin input sections into output sections
  bin_sections();

  // Assign offsets within an output section to input sections.
  set_isec_offsets();

  // Sections are added to the section lists in an arbitrary order because
  // they are created in parallel. Sor them to to make the output deterministic.
  auto section_compare = [](OutputChunk *x, OutputChunk *y) {
    return std::make_tuple(x->name, (u32)x->shdr.sh_type, (u64)x->shdr.sh_flags) <
           std::make_tuple(y->name, (u32)y->shdr.sh_type, (u64)y->shdr.sh_flags);
  };

  std::stable_sort(OutputSection::instances.begin(), OutputSection::instances.end(),
                   section_compare);
  std::stable_sort(MergedSection::instances.begin(), MergedSection::instances.end(),
                   section_compare);

  // Add sections to the section lists
  for (OutputSection *osec : OutputSection::instances)
    if (osec->shdr.sh_size)
      out::chunks.push_back(osec);
  for (MergedSection *osec : MergedSection::instances)
    if (osec->shdr.sh_size)
      out::chunks.push_back(osec);

  // Create a dummy file containing linker-synthesized symbols
  // (e.g. `__bss_start`).
  ObjectFile *internal_file = ObjectFile::create_internal_file();
  internal_file->priority = priority++;
  out::files.push_back(internal_file);

  // Beyond this point, no new symbols will be added to the result.

  // Copy shared object name strings to .dynsym
  for (ObjectFile *file : out::files)
    if (file->is_alive && file->is_dso)
      out::dynstr->add_string(file->soname);

  // Scan relocations to fix the sizes of .got, .plt, .got.plt, .dynstr,
  // .rela.dyn, .rela.plt.
  scan_rels();

  // Add synthetic sections.
  out::chunks.push_back(out::got);
  out::chunks.push_back(out::plt);
  out::chunks.push_back(out::gotplt);
  out::chunks.push_back(out::relplt);
  out::chunks.push_back(out::reldyn);
  out::chunks.push_back(out::dynamic);
  out::chunks.push_back(out::dynsym);
  out::chunks.push_back(out::dynstr);
  out::chunks.push_back(out::shstrtab);
  out::chunks.push_back(out::symtab);
  out::chunks.push_back(out::strtab);
  out::chunks.push_back(out::hash);

  out::chunks.erase(std::remove_if(out::chunks.begin(), out::chunks.end(),
                                   [](OutputChunk *c){ return !c; }),
                    out::chunks.end());

  // Sort the sections by section flags so that we'll have to create
  // as few segments as possible.
  std::stable_sort(out::chunks.begin(), out::chunks.end(),
                   [](OutputChunk *a, OutputChunk *b) {
                     return get_section_rank(a->shdr) < get_section_rank(b->shdr);
                   });

  // Add headers and sections that have to be at the beginning
  // or the ending of a file.
  out::chunks.insert(out::chunks.begin(), out::ehdr);
  out::chunks.insert(out::chunks.begin() + 1, out::phdr);
  if (out::interp)
    out::chunks.insert(out::chunks.begin() + 2, out::interp);
  out::chunks.push_back(out::shdr);

  // Set section indices.
  for (int i = 0, shndx = 1; i < out::chunks.size(); i++)
    if (out::chunks[i]->kind != OutputChunk::HEADER)
      out::chunks[i]->shndx = shndx++;

  for (OutputChunk *chunk : out::chunks)
    chunk->update_shdr();

  // Assign offsets to output sections
  u64 filesize = set_osec_offsets(out::chunks);

  // Fix linker-synthesized symbol addresses.
  fix_synthetic_symbols(out::chunks);

  // At this point, file layout is fixed. Beyond this, you can assume
  // that symbol addresses including their GOT/PLT/etc addresses have
  // a correct final value.

  // Some types of relocations for TLS symbols need the ending address
  // of the TLS section. Find it out now.
  for (OutputChunk *chunk : out::chunks) {
    ELF64LE::Shdr &shdr = chunk->shdr;
    if (shdr.sh_flags & SHF_TLS)
      out::tls_end = align_to(shdr.sh_addr + shdr.sh_size, shdr.sh_addralign);
  }

  // Create an output file
  out::buf = open_output_file(filesize);

  // Copy input sections to the output file
  {
    MyTimer t("copy", copy_timer);

    tbb::parallel_for_each(out::chunks, [&](OutputChunk *chunk) {
      chunk->initialize_buf();
    });

    tbb::parallel_for_each(out::chunks, [&](OutputChunk *chunk) {
      chunk->copy_buf();
    });
  }

  // Fill .plt, .got, got.plt, .rela.plt sections
  write_got_plt();

  // Fill mergeable string sections
  write_merged_strings();

  // Zero-clear paddings between sections
  clear_padding(filesize);

  // Commit
  {
    MyTimer t("munmap", copy_timer);
    munmap(out::buf, filesize);
  }

  total_timer.stopTimer();

  if (config.print_map) {
    MyTimer t("print_map");
    print_map(out::files, out::chunks);
  }

#if 0
  for (ObjectFile *file : out::files)
    for (InputSection *isec : file->sections)
      if (isec)
        message(toString(isec));
#endif

  // Show stat numbers
  Counter num_input_sections("input_sections");
  for (ObjectFile *file : out::files)
    num_input_sections.inc(file->sections.size());

  Counter num_output_chunks("output_out::chunks", out::chunks.size());
  Counter num_files("files", out::files.size());
  Counter filesize_counter("filesize", filesize);

  Counter::print();
  llvm::TimerGroup::printAll(llvm::outs());

  llvm::outs().flush();
  _exit(0);
}
