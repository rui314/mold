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

static void read_file(std::vector<ObjectFile *> &files, StringRef path) {
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
      files.push_back(new ObjectFile(member, path));
    break;
  case file_magic::elf_relocatable:
  case file_magic::elf_shared_object:
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

static void resolve_symbols(std::vector<ObjectFile *> &files) {
  MyTimer t("resolve_symbols", before_copy_timer);

  // Register defined symbols
  tbb::parallel_for_each(files, [](ObjectFile *file) { file->resolve_symbols(); });


  // Mark archive members we include into the final output.
  std::vector<ObjectFile *> root;
  for (ObjectFile *file : files)
    if (file->is_alive)
      root.push_back(file);

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

static void eliminate_comdats(std::vector<ObjectFile *> &files) {
  MyTimer t("comdat", before_copy_timer);

  tbb::parallel_for_each(files, [](ObjectFile *file) {
    file->resolve_comdat_groups();
  });

  tbb::parallel_for_each(files, [](ObjectFile *file) {
    file->eliminate_duplicate_comdat_groups();
  });
}

static void handle_mergeable_strings(std::vector<ObjectFile *> &files) {
  MyTimer t("resolve_strings", before_copy_timer);

  // Resolve mergeable string pieces
  tbb::parallel_for_each(files, [](ObjectFile *file) {
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
  tbb::parallel_for_each(files, [](ObjectFile *file) {
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
  for (ObjectFile *file : files) {
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
static void bin_sections(std::vector<ObjectFile *> &files) {
  MyTimer t("bin_sections", before_copy_timer);

  int unit = (files.size() + 127) / 128;
  std::vector<ArrayRef<ObjectFile *>> slices = split(files, unit);

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

static void scan_rels(ArrayRef<ObjectFile *> files) {
  MyTimer t("scan_rels", before_copy_timer);

  tbb::parallel_for_each(files, [&](ObjectFile *file) {
    for (InputSection *isec : file->sections)
      if (isec)
        isec->scan_relocations();
  });

  tbb::parallel_for_each(files, [&](ObjectFile *file) {
    for (Symbol *sym : file->symbols) {
      if (sym->file != file)
        continue;

      u8 rels = sym->rels.load(std::memory_order_relaxed);
      bool needs_dynsym = false;

      if (rels & Symbol::HAS_GOT_REL) {
        sym->got_idx = file->num_got++;
        file->num_reldyn++;
        needs_dynsym = true;
      }

      if (rels & Symbol::HAS_PLT_REL) {
        sym->plt_idx = file->num_plt++;
        needs_dynsym = true;

        if (sym->got_idx == -1) {
          sym->gotplt_idx = file->num_gotplt++;
          sym->relplt_idx = file->num_relplt++;
        }
      }

      if (rels & Symbol::HAS_TLSGD_REL) {
        sym->gotgd_idx = file->num_got;
        file->num_got += 2;
        file->num_reldyn += 2;
        needs_dynsym = true;
      }

      if (rels & Symbol::HAS_TLSLD_REL) {
        sym->gotgd_idx = file->num_got++;
        file->num_reldyn++;
        needs_dynsym = true;
      }

      if (rels & Symbol::HAS_GOTTP_REL)
       sym->gottp_idx = file->num_got++;

      if (needs_dynsym)
        sym->dynsym_idx = file->num_dynsym++;
    }
  });

  for (ObjectFile *file : files) {

    file->got_offset = out::got->shdr.sh_size;
    out::got->shdr.sh_size += file->num_got * GOT_SIZE;

    file->gotplt_offset = out::gotplt->shdr.sh_size;
    out::gotplt->shdr.sh_size += file->num_gotplt * GOT_SIZE;

    file->plt_offset = out::plt->shdr.sh_size;
    out::plt->shdr.sh_size += file->num_plt * PLT_SIZE;

    file->relplt_offset = out::relplt->shdr.sh_size;
    out::relplt->shdr.sh_size += file->num_relplt * sizeof(ELF64LE::Rela);

    file->reldyn_offset = out::reldyn->shdr.sh_size;
    out::reldyn->shdr.sh_size += file->num_reldyn * sizeof(ELF64LE::Rela);

    file->dynsym_offset = out::dynsym->shdr.sh_size;
    out::dynsym->shdr.sh_size += file->num_dynsym * sizeof(ELF64LE::Sym);

    file->dynstr_offset = out::dynstr->shdr.sh_size;
    out::dynstr->shdr.sh_size += file->dynstr_size;
  }
}

static void write_dynamic_rel(u8 *buf, u8 type, u64 offset, u64 addend) {
  ELF64LE::Rela *rel = (ELF64LE::Rela *)buf;
  memset(rel, 0, sizeof(*rel));
  rel->setType(type, false);
  rel->r_offset = offset;
  rel->r_addend = addend;
}

static void write_got(u8 *buf, ArrayRef<ObjectFile *> files) {
  MyTimer t("write_synthetic", copy_timer);

  tbb::parallel_for_each(files, [&](ObjectFile *file) {
    u8 *got_buf = buf + out::got->shdr.sh_offset + file->got_offset;
    u8 *gotplt_buf = buf + out::gotplt->shdr.sh_offset + file->gotplt_offset;
    u8 *plt_buf = buf + out::plt->shdr.sh_offset + file->plt_offset;
    u8 *relplt_buf = buf + out::relplt->shdr.sh_offset + file->relplt_offset;
    u8 *reldyn_buf = buf + out::reldyn->shdr.sh_offset + file->reldyn_offset;
    u8 *dynsym_buf = buf + out::dynsym->shdr.sh_offset + file->dynsym_offset;
    u8 *dynstr_buf = buf + out::dynstr->shdr.sh_offset;

    int reldyn_idx = 0;
    int dynstr_offset = file->dynstr_offset;

    for (Symbol *sym : file->symbols) {
      if (sym->file != file)
        continue;

      if (sym->got_idx != -1) {
        *(u64 *)(got_buf + sym->got_idx * GOT_SIZE) = sym->get_addr();
        write_dynamic_rel(reldyn_buf + reldyn_idx++ * sizeof(ELF64LE::Rela),
                          R_X86_64_GLOB_DAT, sym->get_got_addr(), 0);
      }

      if (sym->gottp_idx != -1)
        *(u64 *)(got_buf + sym->gottp_idx * GOT_SIZE) = sym->get_addr() - out::tls_end;

      if (sym->gotgd_idx != -1)
        error("unimplemented");

      if (sym->gotld_idx != -1)
        error("unimplemented");

      if (sym->plt_idx != -1)
        out::plt->write_entry(buf, sym);

      if (sym->relplt_idx != -1) {
        if (sym->type == STT_GNU_IFUNC) {
          write_dynamic_rel(relplt_buf + sym->relplt_idx * sizeof(ELF64LE::Rela),
                            R_X86_64_IRELATIVE, sym->get_plt_addr(), 0);
        } else {
          write_dynamic_rel(relplt_buf + sym->relplt_idx * sizeof(ELF64LE::Rela),
                            R_X86_64_JUMP_SLOT, sym->get_plt_addr(), 0);
        }
      }

      if (sym->dynsym_idx != -1) {
        // Write to .dynsym
        auto &esym = *(ELF64LE::Sym *)(dynsym_buf +
                                       sym->dynsym_idx * sizeof(ELF64LE::Sym));
        memset(&esym, 0, sizeof(esym));
        esym.st_name = dynstr_offset;
        esym.setType(sym->type);
        esym.setBinding(STB_GLOBAL);

        // Write to .dynstr
        write_string(dynstr_buf + dynstr_offset, sym->name);
        dynstr_offset += sym->name.size() + 1;

        // Write to .hash
        if (out::hash)
          out::hash->write_symbol(buf, sym);
      }
    }
  });
}

static void write_shstrtab(u8 *buf, ArrayRef<OutputChunk *> chunks) {
  int offset = out::shstrtab->shdr.sh_offset + 1;
  for (OutputChunk *chunk : chunks) {
    if (!chunk->name.empty()) {
      write_string(buf + offset, chunk->name);
      offset += chunk->name.size() + 1;
    }
  }
}

static void write_dso_paths(u8 *buf, ArrayRef<ObjectFile *> files) {
  int offset = out::dynstr->shdr.sh_offset + 1;
  for (ObjectFile *file : files) {
    if (!file->soname.empty()) {
      write_string(buf + offset, file->soname);
      offset += file->soname.size() + 1;
    }
  }
}

static void write_merged_strings(u8 *buf, ArrayRef<ObjectFile *> files) {
  MyTimer t("write_merged_strings", copy_timer);

  tbb::parallel_for_each(files, [&](ObjectFile *file) {
    for (MergeableSection &isec : file->mergeable_sections) {
      u8 *base = buf + isec.parent.shdr.sh_offset + isec.offset;

      for (StringPieceRef &ref : isec.pieces) {
        StringPiece &piece = *ref.piece;
        if (piece.isec == &isec)
          memcpy(base + piece.output_offset, piece.data.data(), piece.data.size());
      }
    }
  });
}

static void clear_padding(u8 *buf, ArrayRef<OutputChunk *> chunks, u64 filesize) {
  MyTimer t("clear_padding", copy_timer);

  auto zero = [&](OutputChunk *chunk, u64 next_start) {
    u64 pos = chunk->shdr.sh_offset;
    if (chunk->shdr.sh_type != SHT_NOBITS)
      pos += chunk->shdr.sh_size;
    memset(buf + pos, 0, next_start - pos);
  };

  for (int i = 1; i < chunks.size(); i++)
    zero(chunks[i - 1], chunks[i]->shdr.sh_offset);
  zero(chunks.back(), filesize);
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
  return (alloc << 5) | (!writable << 4) | (!exec << 3) | (tls << 2) | !nobits;
}

static void sort_output_chunks(std::vector<OutputChunk *> &chunks) {
}

static std::vector<u8> create_ehdr() {
  ELF64LE::Ehdr hdr = {};

  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = ELFCLASS64;
  hdr.e_ident[EI_DATA] = ELFDATA2LSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_ident[EI_OSABI] = 0;
  hdr.e_ident[EI_ABIVERSION] = 0;
  hdr.e_type = ET_EXEC;
  hdr.e_machine = EM_X86_64;
  hdr.e_version = EV_CURRENT;
  hdr.e_entry = Symbol::intern("_start")->get_addr();
  hdr.e_phoff = out::phdr->shdr.sh_offset;
  hdr.e_shoff = out::shdr->shdr.sh_offset;
  hdr.e_flags = 0;
  hdr.e_ehsize = sizeof(ELF64LE::Ehdr);
  hdr.e_phentsize = sizeof(ELF64LE::Phdr);
  hdr.e_phnum = out::phdr->shdr.sh_size / sizeof(ELF64LE::Phdr);
  hdr.e_shentsize = sizeof(ELF64LE::Shdr);
  hdr.e_shnum = out::shdr->shdr.sh_size / sizeof(ELF64LE::Shdr);
  hdr.e_shstrndx = out::shstrtab->shndx;

  std::vector<u8> ret(sizeof(hdr));
  memcpy(ret.data(), &hdr, sizeof(hdr));
  return ret;
}

template<typename T>
static std::vector<u8> to_u8vector(const std::vector<T> &vec) {
  std::vector<u8> ret(vec.size() * sizeof(T));
  memcpy(ret.data(), vec.data(), ret.size());
  return ret;
}

static std::vector<u8> create_shdr(ArrayRef<OutputChunk *> chunks) {
  std::vector<ELF64LE::Shdr> vec(1);
  for (OutputChunk *chunk : chunks)
    if (chunk->kind != OutputChunk::HEADER)
      vec.push_back(chunk->shdr);
  return to_u8vector(vec);
}

static u32 to_phdr_flags(OutputChunk *chunk) {
  u32 ret = PF_R;
  if (chunk->shdr.sh_flags & SHF_WRITE)
    ret |= PF_W;
  if (chunk->shdr.sh_flags & SHF_EXECINSTR)
    ret |= PF_X;
  return ret;
}

static std::vector<u8> create_phdr(ArrayRef<OutputChunk *> chunks) {
  std::vector<ELF64LE::Phdr> vec;

  auto define = [&](u32 type, u32 flags, u32 align, OutputChunk *chunk) {
    vec.push_back({});
    ELF64LE::Phdr &phdr = vec.back();
    phdr.p_type = type;
    phdr.p_flags = flags;
    phdr.p_align = std::max<u64>(align, chunk->shdr.sh_addralign);
    phdr.p_offset = chunk->shdr.sh_offset;
    phdr.p_filesz = (chunk->shdr.sh_type == SHT_NOBITS) ? 0 : chunk->shdr.sh_size;
    phdr.p_vaddr = chunk->shdr.sh_addr;
    phdr.p_memsz = chunk->shdr.sh_size;

    if (type == PT_LOAD)
      chunk->starts_new_ptload = true;
  };

  auto append = [&](OutputChunk *chunk) {
    ELF64LE::Phdr &phdr = vec.back();
    phdr.p_align = std::max<u64>(phdr.p_align, chunk->shdr.sh_addralign);
    phdr.p_filesz = (chunk->shdr.sh_type == SHT_NOBITS)
      ? chunk->shdr.sh_offset - phdr.p_offset
      : chunk->shdr.sh_offset + chunk->shdr.sh_size - phdr.p_offset;
    phdr.p_memsz = chunk->shdr.sh_addr + chunk->shdr.sh_size - phdr.p_vaddr;
  };

  auto is_bss = [](OutputChunk *chunk) {
    return chunk->shdr.sh_type == SHT_NOBITS && !(chunk->shdr.sh_flags & SHF_TLS);
  };

  // Create a PT_PHDR for the program header itself.
  define(PT_PHDR, PF_R, 8, out::phdr);

  // Create an PT_INTERP.
  if (out::interp)
    define(PT_INTERP, PF_R, 1, out::interp);

  // Create PT_LOAD segments.
  for (int i = 0, end = chunks.size(); i < end;) {
    OutputChunk *first = chunks[i++];
    if (!(first->shdr.sh_flags & SHF_ALLOC))
      break;

    u32 flags = to_phdr_flags(first);
    define(PT_LOAD, flags, PAGE_SIZE, first);

    if (!is_bss(first))
      while (i < end && !is_bss(chunks[i]) && to_phdr_flags(chunks[i]) == flags)
        append(chunks[i++]);

    while (i < end && is_bss(chunks[i]) && to_phdr_flags(chunks[i]) == flags)
      append(chunks[i++]);
  }

  // Create a PT_TLS.
  for (int i = 0; i < chunks.size(); i++) {
    if (chunks[i]->shdr.sh_flags & SHF_TLS) {
      define(PT_TLS, to_phdr_flags(chunks[i]), 1, chunks[i]);
      i++;
      while (i < chunks.size() && (chunks[i]->shdr.sh_flags & SHF_TLS))
        append(chunks[i++]);
    }
  }

  // Add PT_DYNAMIC
  if (out::dynamic)
    define(PT_DYNAMIC, PF_R | PF_W, out::dynamic->shdr.sh_addralign, out::dynamic);

  return to_u8vector(vec);
}

static std::vector<u8>
create_dynamic_section(ArrayRef<ObjectFile *> files) {
  std::vector<u64> vec;

  auto define = [&](u64 tag, u64 val) {
    vec.push_back(tag);
    vec.push_back(val);
  };

  int i = 1;
  for (ObjectFile *file : files) {
    if (!file->soname.empty()) {
      define(DT_NEEDED, i);
      i += file->soname.size() + 1;
    }
  }

  define(DT_RELA, out::reldyn->shdr.sh_addr);
  define(DT_RELASZ, out::reldyn->shdr.sh_size);
  define(DT_RELAENT, sizeof(ELF64LE::Rela));
  define(DT_JMPREL, out::relplt->shdr.sh_addr);
  define(DT_PLTRELSZ, out::relplt->shdr.sh_size);
  define(DT_PLTGOT, out::gotplt->shdr.sh_addr);
  define(DT_PLTREL, DT_RELA);
  define(DT_SYMTAB, out::dynsym->shdr.sh_addr);
  define(DT_SYMENT, sizeof(ELF64LE::Sym));
  define(DT_STRTAB, out::dynstr->shdr.sh_addr);
  define(DT_STRSZ, out::dynstr->shdr.sh_size);
  define(DT_HASH, out::hash->shdr.sh_addr);
  define(DT_INIT_ARRAY, out::__init_array_start->value);
  define(DT_INIT_ARRAYSZ, out::__init_array_end->value - out::__init_array_start->value);
  define(DT_FINI_ARRAY, out::__fini_array_start->value);
  define(DT_FINI_ARRAYSZ, out::__fini_array_end->value - out::__fini_array_start->value);
  define(DT_NULL, 0);
  return to_u8vector(vec);
}

static u64 set_osec_offsets(ArrayRef<OutputChunk *> chunks) {
  MyTimer t("osec_offset", before_copy_timer);

  u64 fileoff = 0;
  u64 vaddr = 0x200000;

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

  // __start_ and __stop_ symbols
  for (OutputChunk *chunk : chunks) {
    if (is_c_identifier(chunk->name)) {
      start(chunk, Symbol::intern(("__start_" + chunk->name).str()));
      stop(chunk, Symbol::intern(("__stop_" + chunk->name).str()));
    }
  }
}

static u8 *open_output_file(u64 filesize) {
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

static void write_symtab(u8 *buf, std::vector<ObjectFile *> files) {
  MyTimer t("write_symtab", copy_timer);

  std::vector<u64> local_symtab_off(files.size() + 1);
  std::vector<u64> local_strtab_off(files.size() + 1);
  local_symtab_off[0] = sizeof(ELF64LE::Sym);
  local_strtab_off[0] = 1;

  for (int i = 1; i < files.size() + 1; i++) {
    local_symtab_off[i] = local_symtab_off[i - 1] + files[i - 1]->local_symtab_size;
    local_strtab_off[i] = local_strtab_off[i - 1] + files[i - 1]->local_strtab_size;
  }

  out::symtab->shdr.sh_info = local_symtab_off.back() / sizeof(ELF64LE::Sym);

  std::vector<u64> global_symtab_off(files.size() + 1);
  std::vector<u64> global_strtab_off(files.size() + 1);
  global_symtab_off[0] = local_symtab_off.back();
  global_strtab_off[0] = local_strtab_off.back();

  for (int i = 1; i < files.size() + 1; i++) {
    global_symtab_off[i] = global_symtab_off[i - 1] + files[i - 1]->global_symtab_size;
    global_strtab_off[i] = global_strtab_off[i - 1] + files[i - 1]->global_strtab_size;
  }

  assert(global_symtab_off.back() == out::symtab->shdr.sh_size);
  assert(global_strtab_off.back() == out::strtab->shdr.sh_size);

  tbb::parallel_for((size_t)0, files.size(), [&](size_t i) {
    files[i]->write_local_symtab(buf, local_symtab_off[i], local_strtab_off[i]);
    files[i]->write_global_symtab(buf, global_symtab_off[i], global_strtab_off[i]);
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

static void write_vector(u8 *buf, ArrayRef<u8> vec) {
  memcpy(buf, vec.data(), vec.size());
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

  std::vector<ObjectFile *> files;

  // Open input files
  {
    MyTimer t("open", parse_timer);
    for (auto *arg : args)
      if (arg->getOption().getID() == OPT_INPUT)
        read_file(files, arg->getValue());
  }

  // Parse input files
  {
    MyTimer t("parse", parse_timer);
    tbb::parallel_for_each(files, [](ObjectFile *file) { file->parse(); });
  }

  {
    MyTimer t("merge", parse_timer);
    tbb::parallel_for_each(files, [](ObjectFile *file) {
      file->initialize_mergeable_sections();
    });
  }

  Timer total_timer("total", "total");
  total_timer.startTimer();

  out::ehdr = new OutputHeader;
  out::shdr = new OutputHeader;
  out::phdr = new OutputHeader;
  out::got = new SpecialSection(".got", SHT_PROGBITS, SHF_ALLOC | SHF_WRITE, 8);
  out::gotplt = new GotPltSection;
  out::relplt = new SpecialSection(".rela.plt", SHT_RELA, SHF_ALLOC,
                                   8, sizeof(ELF64LE::Rela));
  out::strtab = new StrtabSection(".strtab", 0);
  out::shstrtab = new StrtabSection(".shstrtab", 0);
  out::plt = new PltSection;
  out::symtab = new SymtabSection(".symtab", SHT_SYMTAB, 0);
  out::dynsym = new SymtabSection(".dynsym", SHT_DYNSYM, SHF_ALLOC);
  out::dynstr = new StrtabSection(".dynstr", SHF_ALLOC);

  out::dynsym->shdr.sh_size = sizeof(ELF64LE::Sym);

  if (!config.is_static) {
    out::interp = new SpecialSection(".interp", SHT_PROGBITS, SHF_ALLOC);
    out::dynamic = new SpecialSection(".dynamic", SHT_DYNAMIC, SHF_ALLOC | SHF_WRITE,
                                      8, sizeof(ELF64LE::Dyn));
    out::reldyn = new SpecialSection(".rela.dyn", SHT_RELA, SHF_ALLOC, 8,
                                     sizeof(ELF64LE::Rela));
    out::hash = new HashSection;

    out::interp->shdr.sh_size = config.dynamic_linker.size() + 1;
  }

  // Set priorities to files
  int priority = 1;
  for (ObjectFile *file : files)
    if (!file->is_in_archive)
      file->priority = priority++;
  for (ObjectFile *file : files)
    if (file->is_in_archive)
      file->priority = priority++;

  // Resolve symbols and fix the set of object files that are
  // included to the final output.
  resolve_symbols(files);

  if (args.hasArg(OPT_trace))
    for (ObjectFile *file : files)
      llvm::outs() << toString(file) << "\n";

  // Remove redundant comdat sections (e.g. duplicate inline functions).
  eliminate_comdats(files);

  // Merge strings constants in SHF_MERGE sections.
  handle_mergeable_strings(files);

  // Create .bss sections for common symbols.
  {
    MyTimer t("common", before_copy_timer);
    tbb::parallel_for_each(files,
                           [](ObjectFile *file) { file->convert_common_symbols(); });
  }

  // Bin input sections into output sections
  bin_sections(files);

  // Assign offsets within an output section to input sections.
  set_isec_offsets();

  // Create a list of output sections.
  std::vector<OutputChunk *> chunks;

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
      chunks.push_back(osec);
  for (MergedSection *osec : MergedSection::instances)
    if (osec->shdr.sh_size)
      chunks.push_back(osec);

  // Create a dummy file containing linker-synthesized symbols
  // (e.g. `__bss_start`).
  ObjectFile *internal_file = ObjectFile::create_internal_file(chunks);
  internal_file->priority = priority++;
  files.push_back(internal_file);

  // Beyond this point, no new symbols will be added to the result.

  // Reserve space in .dynsym for DT_NEEDED strings.
  for (ObjectFile *file : files)
    if (file->is_alive && file->is_dso)
      out::dynstr->shdr.sh_size += file->soname.size() + 1;

  // Scan relocations to fix the sizes of .got, .plt, .got.plt, .dynstr,
  // .rela.dyn, .rela.plt.
  scan_rels(files);

  // Compute .symtab and .strtab sizes
  {
    MyTimer t("symtab_size", before_copy_timer);
    tbb::parallel_for_each(files, [](ObjectFile *file) { file->compute_symtab(); });

    for (ObjectFile *file : files) {
      out::symtab->shdr.sh_size += file->local_symtab_size + file->global_symtab_size;
      out::strtab->shdr.sh_size += file->local_strtab_size + file->global_strtab_size;
    }
  }

  // Add synthetic sections.
  chunks.push_back(out::got);
  chunks.push_back(out::plt);
  chunks.push_back(out::gotplt);
  chunks.push_back(out::relplt);
  chunks.push_back(out::reldyn);
  chunks.push_back(out::dynamic);
  chunks.push_back(out::dynsym);
  chunks.push_back(out::dynstr);
  chunks.push_back(out::shstrtab);
  chunks.push_back(out::symtab);
  chunks.push_back(out::strtab);
  chunks.push_back(out::hash);

  chunks.erase(std::remove_if(chunks.begin(), chunks.end(),
                              [](OutputChunk *c){ return !c; }),
               chunks.end());

  // Sort the sections by section flags so that we'll have to create
  // as few segments as possible.
  std::stable_sort(chunks.begin(), chunks.end(), [](OutputChunk *a, OutputChunk *b) {
    return get_section_rank(a->shdr) > get_section_rank(b->shdr);
  });

  // Add headers and sections that have to be at the beginning
  // or the ending of a file.
  chunks.insert(chunks.begin(), out::ehdr);
  chunks.insert(chunks.begin() + 1, out::phdr);
  if (out::interp)
    chunks.insert(chunks.begin() + 2, out::interp);
  chunks.push_back(out::shdr);

  // Fix .shstrtab contents.
  for (OutputChunk *chunk : chunks) {
    if (!chunk->name.empty()) {
      chunk->shdr.sh_name = out::shstrtab->shdr.sh_size;
      out::shstrtab->shdr.sh_size += chunk->name.size() + 1;
    }
  }

  // Set section indices.
  for (int i = 0, shndx = 1; i < chunks.size(); i++)
    if (chunks[i]->kind != OutputChunk::HEADER)
      chunks[i]->shndx = shndx++;

  // Initialize synthetic section contents
  out::ehdr->shdr.sh_size = sizeof(ELF64LE::Ehdr);
  out::shdr->shdr.sh_size = create_shdr(chunks).size();
  out::phdr->shdr.sh_size = create_phdr(chunks).size();
  if (out::dynamic)
    out::dynamic->shdr.sh_size = create_dynamic_section(files).size();

  if (out::hash)
    out::hash->set_num_dynsym(out::dynsym->shdr.sh_size / sizeof(ELF64LE::Sym));

  out::symtab->shdr.sh_link = out::strtab->shndx;

  if (out::dynsym) {
    out::dynsym->shdr.sh_info = 1;
    out::dynsym->shdr.sh_link = out::dynstr->shndx;
  }

  if (out::hash && out::dynsym)
    out::hash->shdr.sh_link = out::dynsym->shndx;

  if (out::dynamic && out::dynstr)
    out::dynamic->shdr.sh_link = out::dynstr->shndx;

  // Assign offsets to output sections
  u64 filesize = set_osec_offsets(chunks);

  // Fix linker-synthesized symbol addresses.
  fix_synthetic_symbols(chunks);

  // At this point, file layout is fixed. Beyond this, you can assume
  // that symbol addresses including their GOT/PLT/etc addresses have
  // a correct final value.

  // Some types of relocations for TLS symbols need the ending address
  // of the TLS section. Find it out now.
  for (OutputChunk *chunk : chunks) {
    ELF64LE::Shdr &shdr = chunk->shdr;
    if (shdr.sh_flags & SHF_TLS)
      out::tls_end = align_to(shdr.sh_addr + shdr.sh_size, shdr.sh_addralign);
  }

  // Create an output file
  u8 *buf;
  {
    MyTimer t("open_file", before_copy_timer);
    buf = open_output_file(filesize);
  }

  // Initialize the output buffer.
  {
    MyTimer t("copy", copy_timer);
    tbb::parallel_for_each(chunks, [&](OutputChunk *chunk) {
      chunk->initialize(buf);
    });
  }

  // Copy input sections to the output file
  {
    MyTimer t("copy", copy_timer);
    tbb::parallel_for_each(chunks, [&](OutputChunk *chunk) {
      chunk->copy_to(buf);
    });
  }

  // Fill .symtab and .strtab
  write_symtab(buf, files);

  // Fill .shstrtab
  write_shstrtab(buf, chunks);

  // Write DT_NEEDED paths to .dynstr.
  write_dso_paths(buf, files);

  // Fill .plt, .got, got.plt, .rela.plt sections
  write_got(buf, files);

  // Fill mergeable string sections
  write_merged_strings(buf, files);

  // Write headers and synthetic sections.
  write_vector(buf + out::ehdr->shdr.sh_offset, create_ehdr());
  write_vector(buf + out::shdr->shdr.sh_offset, create_shdr(chunks));
  write_vector(buf + out::phdr->shdr.sh_offset, create_phdr(chunks));

  if (out::interp)
    write_string(buf + out::interp->shdr.sh_offset, config.dynamic_linker);
  if (out::dynamic)
    write_vector(buf + out::dynamic->shdr.sh_offset, create_dynamic_section(files));

  // Zero-clear paddings between sections
  clear_padding(buf, chunks, filesize);

  // Commit
  {
    MyTimer t("munmap", copy_timer);
    munmap(buf, filesize);
  }

  total_timer.stopTimer();

  if (config.print_map) {
    MyTimer t("print_map");
    print_map(files, chunks);
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

  Counter num_output_chunks("output_chunks", chunks.size());
  Counter num_files("files", files.size());
  Counter filesize_counter("filesize", filesize);

  Counter::print();
  llvm::TimerGroup::printAll(llvm::outs());

  llvm::outs().flush();
  _exit(0);
}
