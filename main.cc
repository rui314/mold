#include "mold.h"

#include "llvm/BinaryFormat/Magic.h"
#include "llvm/Option/ArgList.h"
#include "llvm/Support/FileOutputBuffer.h"
#include "llvm/Support/FileSystem.h"

#include <fcntl.h>
#include <iostream>
#include <libgen.h>
#include <regex>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <unordered_set>

using namespace llvm;
using namespace llvm::ELF;
using namespace llvm::sys;

using llvm::object::Archive;
using llvm::opt::InputArgList;

class MyTimer {
public:
  MyTimer(std::string_view name) {
    timer = new Timer(name, name);
    timer->startTimer();
  }

  MyTimer(std::string_view name, llvm::TimerGroup &tg) {
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
    error(std::string(args.getArgString(missing_index)) + ": missing argument");

  for (auto *arg : args.filtered(OPT_UNKNOWN))
    error("unknown argument '" + arg->getAsString(args) + "'");
  return args;
}

//
// Main
//

static std::vector<MemoryMappedFile> get_archive_members(MemoryMappedFile mb) {
  std::unique_ptr<Archive> file =
    CHECK(Archive::create(mb), mb.name + ": failed to parse archive");

  std::vector<MemoryMappedFile> vec;

  Error err = Error::success();

  for (const Archive::Child &c : file->children(err)) {
    MemoryBufferRef mb =
      CHECK(c.getMemoryBufferRef(),
            mb.getBufferIdentifier().str() +
              ": could not get the buffer for a child of the archive");
    MemoryMappedFile file(mb.getBufferIdentifier().str(),
                          {mb.getBufferStart(), mb.getBufferSize()});
    vec.push_back(file);
  }

  if (err)
    error(mb.name + ": Archive::children failed: " + toString(std::move(err)));

  file.release(); // leak
  return vec;
}

MemoryMappedFile *open_input_file(std::string path) {
  int fd = open(path.c_str(), O_RDONLY);
  if (fd == -1)
    return nullptr;

  struct stat st;
  if (fstat(fd, &st) == -1)
    error(path + ": stat failed");

  void *addr = mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
  if (addr == MAP_FAILED)
    error(path + ": mmap failed: " + strerror(errno));
  close(fd);

  return new MemoryMappedFile(path, {(char *)addr, (size_t)st.st_size});
}

MemoryMappedFile must_open_input_file(std::string path) {
  MemoryMappedFile *mb = open_input_file(path);
  if (!mb)
    error("cannot open " + path);
  return *mb;
}

void read_file(MemoryMappedFile mb) {
  switch (identify_magic(mb.data)) {
  case file_magic::archive:
    for (MemoryMappedFile member : get_archive_members(mb))
      out::objs.push_back(new ObjectFile(member, mb.name));
    break;
  case file_magic::elf_relocatable:
    out::objs.push_back(new ObjectFile(mb, ""));
    break;
  case file_magic::elf_shared_object:
    out::dsos.push_back(new SharedFile(mb, config.as_needed));
    break;
  case file_magic::unknown:
    parse_linker_script(mb);
    break;
  default:
    error(mb.name + ": unknown file type");
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
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) { file->resolve_symbols(); });
  tbb::parallel_for_each(out::dsos, [](SharedFile *file) { file->resolve_symbols(); });

  // Mark reachable objects and DSOs to decide which files to include
  // into an output.
  std::vector<ObjectFile *> root;
  for (ObjectFile *file : out::objs)
    if (file->is_alive)
      root.push_back(file);

  tbb::parallel_do(
    root,
    [&](ObjectFile *file, tbb::parallel_do_feeder<ObjectFile *> &feeder) {
      file->mark_live_objects(feeder);
    });

  // Eliminate unused archive members and as-needed DSOs.
  auto callback = [](InputFile *file){ return !file->is_alive; };
  out::objs.erase(std::remove_if(out::objs.begin(), out::objs.end(), callback),
                  out::objs.end());
  out::dsos.erase(std::remove_if(out::dsos.begin(), out::dsos.end(), callback),
                  out::dsos.end());
}

static void eliminate_comdats() {
  MyTimer t("comdat", before_copy_timer);

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->resolve_comdat_groups();
  });

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->eliminate_duplicate_comdat_groups();
  });
}

static void handle_mergeable_strings() {
  MyTimer t("resolve_strings", before_copy_timer);

  // Resolve mergeable string pieces
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    for (MergeableSection &isec : file->mergeable_sections) {
      for (StringPieceRef &ref : isec.pieces) {
        MergeableSection *cur = ref.piece->isec;
        while (!cur || cur->file->priority > isec.file->priority)
          if (ref.piece->isec.compare_exchange_weak(cur, &isec))
            break;
      }
    }
  });

  // Calculate the total bytes of mergeable strings for each input section.
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
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
  for (ObjectFile *file : out::objs) {
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

  int unit = (out::objs.size() + 127) / 128;
  std::vector<ArrayRef<ObjectFile *>> slices = split(out::objs, unit);

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

static void check_duplicate_symbols() {
  MyTimer t("check_undef_syms", before_copy_timer);

  auto is_error = [](ObjectFile *file, int i) {
    const ElfSym &esym = file->elf_syms[i];
    Symbol &sym = *file->symbols[i];
    bool is_weak = (esym.st_bind == STB_WEAK);
    bool is_eliminated =
      !esym.is_abs() && !esym.is_common() && !file->sections[esym.st_shndx];
    return esym.is_defined() && !is_weak && !is_eliminated && sym.file != file;
  };

  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    if (!file->is_alive)
      return;

    for (int i = file->first_global; i < file->elf_syms.size(); i++) {
      if (is_error(file, i)) {
        file->has_error = true;
        return;
      }
    }
  });

  for (ObjectFile *file : out::objs)
    if (file->has_error)
      for (int i = file->first_global; i < file->elf_syms.size(); i++)
        if (is_error(file, i))
          llvm::errs() << "duplicate symbol: " << toString(file)
                       << ": " << toString(file->symbols[i]->file) << ": "
                       << file->symbols[i]->name << "\n";

  for (ObjectFile *file : out::objs)
    if (file->has_error)
      _exit(1);
}

static void set_isec_offsets() {
  MyTimer t("isec_offsets", before_copy_timer);

  tbb::parallel_for_each(OutputSection::instances, [&](OutputSection *osec) {
    if (osec->members.empty())
      return;

    std::vector<ArrayRef<InputChunk *>> slices = split(osec->members, 10000);
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

  // Scan relocations to find dynamic symbols.
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (InputSection *isec : file->sections)
      if (isec)
        isec->scan_relocations();
  });

  // If there was a relocation that refers an undefined symbol,
  // report an error.
  for (ObjectFile *file : out::objs)
    if (file->has_error)
      for (InputSection *isec : file->sections)
        if (isec)
          isec->report_undefined_symbols();

  for (ObjectFile *file : out::objs)
    if (file->has_error)
      _exit(1);

  // Aggregate dynamic symbols to a single vector.
  std::vector<InputFile *> files;
  files.insert(files.end(), out::objs.begin(), out::objs.end());
  files.insert(files.end(), out::dsos.begin(), out::dsos.end());

  std::vector<std::vector<Symbol *>> vec(files.size());

  tbb::parallel_for(0, (int)files.size(), [&](int i) {
    for (Symbol *sym : files[i]->symbols)
      if (sym->file == files[i] && sym->flags)
        vec[i].push_back(sym);
  });

  // Assign offsets in additional tables for each dynamic symbol.
  for (Symbol *sym : flatten(vec)) {
    if (sym->flags & Symbol::NEEDS_GOT)
      out::got->add_got_symbol(sym);

    if (sym->flags & Symbol::NEEDS_PLT)
      out::plt->add_symbol(sym);

    if (sym->flags & Symbol::NEEDS_GOTTPOFF)
      out::got->add_gottpoff_symbol(sym);

    if (sym->flags & Symbol::NEEDS_TLSGD)
      out::got->add_tlsgd_symbol(sym);

    if (sym->flags & Symbol::NEEDS_TLSLD)
      out::got->add_tlsld_symbol(sym);

    if (sym->flags & Symbol::NEEDS_COPYREL) {
      out::copyrel->add_symbol(sym);
      assert(sym->file->is_dso);

      for (Symbol *alias : ((SharedFile *)sym->file)->find_aliases(sym)) {
        if (sym == alias)
          continue;
        assert(alias->copyrel_offset == -1);
        alias->copyrel_offset = sym->copyrel_offset;
        out::dynsym->add_symbol(alias);
      }
    }
  }
}

static void export_dynamic() {
  MyTimer t("export_dynamic", before_copy_timer);

  tbb::parallel_for(0, (int)out::objs.size(), [&](int i) {
    ObjectFile *file = out::objs[i];
    for (Symbol *sym : makeArrayRef(file->symbols).slice(file->first_global))
      if (sym->file == file && config.export_dynamic)
        sym->ver_idx = VER_NDX_GLOBAL;
  });

  for (std::string_view name : config.globals)
    Symbol::intern(name)->ver_idx = VER_NDX_GLOBAL;

  std::vector<std::vector<Symbol *>> vec(out::objs.size());

  tbb::parallel_for(0, (int)out::objs.size(), [&](int i) {
    ObjectFile *file = out::objs[i];
    for (Symbol *sym : makeArrayRef(file->symbols).slice(file->first_global))
      if (sym->file == file && sym->ver_idx != VER_NDX_LOCAL)
        vec[i].push_back(sym);
  });

  for (Symbol *sym : flatten(vec))
    out::dynsym->add_symbol(sym);
}

static void fill_symbol_versions() {
  MyTimer t("fill_symbol_versions", before_copy_timer);

  // Create a list of versioned symbols and sort by file and version.
  std::vector<Symbol *> syms = out::dynsym->symbols;

  syms.erase(std::remove_if(syms.begin(), syms.end(),
                            [](Symbol *sym){ return sym->ver_idx < 2; }),
             syms.end());

  if (syms.empty())
    return;

  std::stable_sort(syms.begin(), syms.end(), [](Symbol *a, Symbol *b) {
    SharedFile *x = (SharedFile *)a->file;
    SharedFile *y = (SharedFile *)b->file;
    return std::make_tuple(x->soname, a->ver_idx) <
           std::make_tuple(y->soname, b->ver_idx);
  });

  // Compute sizes of .gnu.version and .gnu.version_r sections.
  out::versym->contents.resize(out::dynsym->symbols.size() + 1, 1);
  out::versym->contents[0] = 0;

  int sz = sizeof(ELF64LE::Verneed) + sizeof(ELF64LE::Vernaux);
  for (int i = 1; i < syms.size(); i++) {
    if (syms[i - 1]->file != syms[i]->file)
      sz += sizeof(ELF64LE::Verneed) + sizeof(ELF64LE::Vernaux);
    else if (syms[i - 1]->ver_idx != syms[i]->ver_idx)
      sz += sizeof(ELF64LE::Vernaux);
  }
  out::verneed->contents.resize(sz);

  // Fill .gnu.versoin_r.
  u8 *buf = (u8 *)&out::verneed->contents[0];
  u16 version = 1;
  ELF64LE::Verneed *verneed = nullptr;
  ELF64LE::Vernaux *aux = nullptr;

  auto add_aux = [&](Symbol *sym) {
    SharedFile *file = (SharedFile *)sym->file;
    std::string_view verstr = file->version_strings[sym->ver_idx];

    verneed->vn_cnt += 1;
    if (aux)
      aux->vna_next = sizeof(ELF64LE::Vernaux);

    aux = (ELF64LE::Vernaux *)buf;
    buf += sizeof(*aux);
    aux->vna_hash = elf_hash(verstr);
    aux->vna_other = ++version;
    aux->vna_name = out::dynstr->add_string(verstr);
  };

  auto add_verneed = [&](Symbol *sym) {
    SharedFile *file = (SharedFile *)sym->file;

    out::verneed->shdr.sh_info += 1;
    if (verneed)
      verneed->vn_next = buf - (u8 *)verneed;

    verneed = (ELF64LE::Verneed *)buf;
    buf += sizeof(*verneed);
    verneed->vn_version = 1;
    verneed->vn_file = out::dynstr->find_string(file->soname);
    verneed->vn_aux = sizeof(ELF64LE::Verneed);

    aux = nullptr;
    add_aux(sym);
  };

  add_verneed(syms[0]);
  out::versym->contents[syms[0]->dynsym_idx] = version;

  for (int i = 1; i < syms.size(); i++) {
    if (syms[i - 1]->file != syms[i]->file)
      add_verneed(syms[i]);
    else if (syms[i - 1]->ver_idx != syms[i]->ver_idx)
      add_aux(syms[i]);
    out::versym->contents[syms[i]->dynsym_idx] = version;
  }
}

static void write_merged_strings() {
  MyTimer t("write_merged_strings", copy_timer);

  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
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

  auto zero = [](OutputChunk *chunk, u64 next_start) {
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
static int get_section_rank(const ElfShdr &shdr) {
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

    if (vaddr % PAGE_SIZE > fileoff % PAGE_SIZE)
      fileoff += vaddr % PAGE_SIZE - fileoff % PAGE_SIZE;
    else if (vaddr % PAGE_SIZE < fileoff % PAGE_SIZE)
      fileoff = align_to(fileoff, PAGE_SIZE) + vaddr % PAGE_SIZE;

    fileoff = align_to(fileoff, chunk->shdr.sh_addralign);
    vaddr = align_to(vaddr, chunk->shdr.sh_addralign);

    chunk->shdr.sh_offset = fileoff;
    if (chunk->shdr.sh_flags & SHF_ALLOC)
      chunk->shdr.sh_addr = vaddr;

    bool is_bss = chunk->shdr.sh_type == SHT_NOBITS;
    if (!is_bss)
      fileoff += chunk->shdr.sh_size;

    bool is_tbss = is_bss && (chunk->shdr.sh_flags & SHF_TLS);
    if (!is_tbss)
      vaddr += chunk->shdr.sh_size;
  }
  return fileoff;
}

static void fix_synthetic_symbols(ArrayRef<OutputChunk *> chunks) {
  auto start = [](OutputChunk *chunk, Symbol *sym) {
    if (sym) {
      sym->shndx = chunk->shndx;
      sym->value = chunk->shdr.sh_addr;
    }
  };

  auto stop = [](OutputChunk *chunk, Symbol *sym) {
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

static u32 get_umask() {
  u32 mask = umask(0);
  umask(mask);
  return mask;
}

static u8 *open_output_file(u64 filesize) {
  MyTimer t("open_file", before_copy_timer);

  int fd = open(std::string(config.output).c_str(), O_RDWR | O_CREAT, 0777);
  if (fd == -1)
    error("cannot open " + config.output + ": " + strerror(errno));

  if (ftruncate(fd, filesize))
    error("ftruncate failed");

  if (fchmod(fd, (0777 & ~get_umask())) == -1)
    error("fchmod failed");

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
      error(arg->getSpelling().str() + ": expected a positive integer, but got '" +
            arg->getValue() + "'");
    return n;
  }
  return tbb::global_control::active_value(tbb::global_control::max_allowed_parallelism);
}

std::vector<std::string> get_args(opt::InputArgList &args, int id) {
  std::vector<std::string> vec;
  for (auto *arg : args.filtered(id))
    vec.push_back(arg->getValue());
  return vec;
}

static int parse_filler(opt::InputArgList &args) {
  auto *arg = args.getLastArg(OPT_filler);
  if (!arg)
    return -1;

  std::string_view val = arg->getValue();
  if (!val.starts_with("0x"))
    error("invalid argument: " + arg->getAsString(args));
  int ret;
  if (!to_integer(val.substr(2), ret, 16))
    error("invalid argument: " + arg->getAsString(args));
  return (u8)ret;
}

MemoryMappedFile find_library(std::string name) {
  for (std::string_view dir : config.library_paths) {
    std::string root = dir.starts_with("/") ? config.sysroot : "";
    std::string stem = root + std::string(dir) + "/lib" + name;
    if (!config.is_static)
      if (MemoryMappedFile *mb = open_input_file(stem + ".so"))
        return *mb;
    if (MemoryMappedFile *mb = open_input_file(stem + ".a"))
      return *mb;
  }
  error("library not found: " + name);
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

  config.filler = parse_filler(args);
  config.is_static = args.hasArg(OPT_static);
  config.library_paths = get_args(args, OPT_library_path);
  config.print_map = args.hasArg(OPT_print_map);
  config.sysroot = args.getLastArgValue(OPT_sysroot, "");
  config.export_dynamic = args.hasArg(OPT_export_dynamic);

  for (auto *arg : args.filtered(OPT_rpath))
    config.rpaths.push_back(arg->getValue());

  for (auto *arg : args.filtered(OPT_version_script))
    parse_version_script(arg->getValue());

  for (auto *arg : args.filtered(OPT_trace_symbol))
    Symbol::intern(arg->getValue())->traced = true;

  // Open input files
  {
    MyTimer t("open", parse_timer);
    for (auto *arg : args) {
      switch (arg->getOption().getID()) {
      case OPT_INPUT:
        read_file(must_open_input_file(arg->getValue()));
        break;
      case OPT_library:
        read_file(find_library(arg->getValue()));
        break;
      case OPT_as_needed:
        config.as_needed = true;
        break;
      case OPT_no_as_needed:
        config.as_needed = false;
        break;
      }
    }
  }

  // Parse input files
  {
    MyTimer t("parse", parse_timer);
    tbb::parallel_for_each(out::objs, [](ObjectFile *file) { file->parse(); });
    tbb::parallel_for_each(out::dsos, [](SharedFile *file) { file->parse(); });
  }

  // Uniquify shared object files with soname
  {
    std::vector<SharedFile *> vec;
    llvm::StringSet<> seen;
    for (SharedFile *file : out::dsos)
      if (seen.insert(file->soname).second)
        vec.push_back(file);
    out::dsos = vec;
  }

  // Parse mergeable string sections
  {
    MyTimer t("merge", parse_timer);
    tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
      file->initialize_mergeable_sections();
    });
  }

  Timer total_timer("total", "total");
  total_timer.startTimer();

  out::ehdr = new OutputEhdr;
  out::shdr = new OutputShdr;
  out::phdr = new OutputPhdr;
  out::got = new GotSection;
  out::gotplt = new GotPltSection;
  out::relplt = new RelPltSection;
  out::strtab = new StrtabSection;
  out::shstrtab = new ShstrtabSection;
  out::plt = new PltSection;
  out::symtab = new SymtabSection;
  out::dynsym = new DynsymSection;
  out::dynstr = new DynstrSection;
  out::copyrel = new CopyrelSection;

  if (!config.is_static) {
    out::interp = new InterpSection;
    out::dynamic = new DynamicSection;
    out::reldyn = new RelDynSection;
    out::hash = new HashSection;
    out::versym = new VersymSection;
    out::verneed = new VerneedSection;
  }

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
  out::chunks.push_back(out::copyrel);
  out::chunks.push_back(out::versym);
  out::chunks.push_back(out::verneed);

  // Set priorities to files. File priority 1 is reserved for the internal file.
  int priority = 2;
  for (ObjectFile *file : out::objs)
    if (!file->is_in_archive)
      file->priority = priority++;
  for (ObjectFile *file : out::objs)
    if (file->is_in_archive)
      file->priority = priority++;
  for (SharedFile *file : out::dsos)
    file->priority = priority++;

  // Resolve symbols and fix the set of object files that are
  // included to the final output.
  resolve_symbols();

  if (args.hasArg(OPT_trace)) {
    for (ObjectFile *file : out::objs)
      message(toString(file));
    for (SharedFile *file : out::dsos)
      message(toString(file));
  }

  // Remove redundant comdat sections (e.g. duplicate inline functions).
  eliminate_comdats();

  // Merge strings constants in SHF_MERGE sections.
  handle_mergeable_strings();

  // Create .bss sections for common symbols.
  {
    MyTimer t("common", before_copy_timer);
    tbb::parallel_for_each(out::objs,
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

  out::chunks.erase(std::remove_if(out::chunks.begin(), out::chunks.end(),
                                   [](OutputChunk *c) { return !c; }),
                    out::chunks.end());

  // Sort the sections by section flags so that we'll have to create
  // as few segments as possible.
  std::stable_sort(out::chunks.begin(), out::chunks.end(),
                   [](OutputChunk *a, OutputChunk *b) {
                     return get_section_rank(a->shdr) < get_section_rank(b->shdr);
                   });

  // Create a dummy file containing linker-synthesized symbols
  // (e.g. `__bss_start`).
  ObjectFile *internal_file = ObjectFile::create_internal_file();
  internal_file->priority = 1;
  internal_file->resolve_symbols();
  out::objs.push_back(internal_file);

  // Convert weak symbols to absolute symbols with value 0.
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->handle_undefined_weak_symbols();
  });

  // Beyond this point, no new symbols will be added to the result.

  // Copy shared object name strings to .dynstr
  for (SharedFile *file : out::dsos)
    out::dynstr->add_string(file->soname);

  // Copy DT_RUNPATH strings to .dynstr.
  for (std::string_view path : config.rpaths)
    out::dynstr->add_string(path);

  // Add headers and sections that have to be at the beginning
  // or the ending of a file.
  out::chunks.insert(out::chunks.begin(), out::ehdr);
  out::chunks.insert(out::chunks.begin() + 1, out::phdr);
  if (out::interp)
    out::chunks.insert(out::chunks.begin() + 2, out::interp);
  out::chunks.push_back(out::shdr);

  // Make sure that all symbols have been resolved.
  check_duplicate_symbols();

  // Scan relocations to find symbols that need entries in .got, .plt,
  // .got.plt, .dynsym, .dynstr, etc.
  scan_rels();

  // Put symbols to .dynsym.
  export_dynamic();

  // Fill .gnu.version and .gnu.version_r section contents.
  fill_symbol_versions();

  // Compute .symtab and .strtab sizes for each file.
  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    file->compute_symtab();
  });

  // Now that we have computed sizes for all sections and assigned
  // section indices to them, so we can fix section header contents
  // for all output sections.
  for (OutputChunk *chunk : out::chunks)
    chunk->update_shdr();

  out::chunks.erase(std::remove_if(out::chunks.begin(), out::chunks.end(),
                                   [](OutputChunk *c) { return c->shdr.sh_size == 0; }),
                    out::chunks.end());

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
  for (ELF64LE::Phdr phdr : create_phdr())
    if (phdr.p_type == PT_TLS)
      out::tls_end = align_to(phdr.p_vaddr + phdr.p_memsz, phdr.p_align);

  // Create an output file
  out::buf = open_output_file(filesize);

  // Copy input sections to the output file
  {
    MyTimer t("copy", copy_timer);
    tbb::parallel_for_each(out::chunks, [&](OutputChunk *chunk) {
      chunk->copy_buf();
    });
  }

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
    print_map();
  }

  // Show stat numbers
  Counter num_input_sections("input_sections");
  for (ObjectFile *file : out::objs)
    num_input_sections.inc(file->sections.size());

  Counter num_output_chunks("output_out::chunks", out::chunks.size());
  Counter num_objs("num_objs", out::objs.size());
  Counter num_dsos("num_dsos", out::dsos.size());
  Counter filesize_counter("filesize", filesize);

  Counter::print();
  llvm::TimerGroup::printAll(llvm::outs());

  llvm::outs().flush();
  _exit(0);
}
