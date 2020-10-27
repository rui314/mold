#pragma once

#include "llvm/ADT/ArrayRef.h"
#include "llvm/ADT/SmallVector.h"
#include "llvm/ADT/Twine.h"
#include "llvm/BinaryFormat/Magic.h"
#include "llvm/Object/Archive.h"
#include "llvm/Object/ELF.h"
#include "llvm/Object/ELFTypes.h"
#include "llvm/Option/ArgList.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/FileOutputBuffer.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/Timer.h"
#include "tbb/blocked_range.h"
#include "tbb/concurrent_hash_map.h"
#include "tbb/concurrent_unordered_set.h"
#include "tbb/concurrent_vector.h"
#include "tbb/global_control.h"
#include "tbb/parallel_for_each.h"
#include "tbb/parallel_reduce.h"
#include "tbb/parallel_sort.h"
#include "tbb/partitioner.h"
#include "tbb/task_arena.h"
#include "tbb/task_group.h"

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <shared_mutex>
#include <string>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <unistd.h>
#include <unordered_set>

#define SECTOR_SIZE 512
#define PAGE_SIZE 4096

using llvm::ArrayRef;
using llvm::ErrorOr;
using llvm::Error;
using llvm::Expected;
using llvm::MemoryBufferRef;
using llvm::SmallVector;
using llvm::StringRef;
using llvm::Twine;
using llvm::object::ELF64LE;
using llvm::object::ELFFile;

class Symbol;
class ObjectFile;
class InputSection;

struct Config {
  StringRef output;
};

extern Config config;

[[noreturn]] inline void error(const Twine &msg) {
  static std::mutex mu;
  std::lock_guard<std::mutex> lock(mu);

  llvm::errs() << msg << "\n";
  exit(1);
}

template <class T> T check(ErrorOr<T> e) {
  if (auto ec = e.getError())
    error(ec.message());
  return std::move(*e);
}

template <class T> T check(Expected<T> e) {
  if (!e)
    error(llvm::toString(e.takeError()));
  return std::move(*e);
}

template <class T>
T check2(ErrorOr<T> e, llvm::function_ref<std::string()> prefix) {
  if (auto ec = e.getError())
    error(prefix() + ": " + ec.message());
  return std::move(*e);
}

template <class T>
T check2(Expected<T> e, llvm::function_ref<std::string()> prefix) {
  if (!e)
    error(prefix() + ": " + toString(e.takeError()));
  return std::move(*e);
}

inline std::string toString(const Twine &s) { return s.str(); }

#define CHECK(E, S) check2((E), [&] { return toString(S); })

class Symbol;
class SymbolTable;
class InputSection;
class OutputSection;
class ObjectFile;

std::string toString(ObjectFile *);

template<typename T, typename Callable>
static void for_each(T &arr, Callable callback) {
  tbb::parallel_for_each(arr.begin(), arr.end(), callback);
}

//
// Interned string
//

namespace tbb {
template<>
struct tbb_hash_compare<StringRef> {
  static size_t hash(const StringRef& k) {
    return llvm::hash_value(k);
  }

  static bool equal(const StringRef& k1, const StringRef& k2) {
    return k1 == k2;
  }
};
}

template<typename ValueT>
class ConcurrentMap {
public:
  typedef tbb::concurrent_hash_map<StringRef, ValueT> MapT;

  ValueT *lookup(StringRef key) {
    typename MapT::accessor acc;
    if (map.find(acc, key))
      return &acc->second;
    return nullptr;
  }

  ValueT *insert(StringRef key, const ValueT &val) {
    typename MapT::accessor acc;
    map.insert(acc, std::make_pair(key, val));
    return &acc->second;
  }

private:
  MapT map;
};

//
// Symbol
//

class Symbol {
public:
  Symbol(StringRef name) : name(name) {}
  Symbol(const Symbol &other) : name(other.name), file(other.file) {}

  static Symbol *intern(StringRef name) {
    static ConcurrentMap<Symbol> map;
    return map.insert(name, Symbol(name));    
  }

  std::atomic_flag lock = ATOMIC_FLAG_INIT;
  StringRef name;
  ObjectFile *file = nullptr;
  InputSection *input_section = nullptr;

  uint64_t addr = 0;
  uint64_t value;
  uint8_t visibility = 0;
  bool is_weak = false;

  std::atomic_bool needs_got;
  std::atomic_bool needs_plt;
};

inline std::string toString(Symbol sym) {
  return (StringRef(sym.name) + "(" + toString(sym.file) + ")").str();
}

//
// input_chunks.cc
//

class InputSection {
public:
  InputSection(ObjectFile *file, const ELF64LE::Shdr &shdr, StringRef name);

  void copy_to(uint8_t *buf);
  void relocate(uint8_t *buf);
  void scan_relocations();

  ObjectFile *file;
  OutputSection *output_section;
  ArrayRef<ELF64LE::Rela> rels;
  const ELF64LE::Shdr &shdr;

  StringRef name;
  uint64_t offset;
};

std::string toString(InputSection *isec);

inline uint64_t align_to(uint64_t val, uint64_t align) {
  assert(__builtin_popcount(align) == 1);
  return (val + align - 1) & ~(align - 1);
}

//
// output_chunks.cc
//

class OutputChunk {
public:
  OutputChunk() { shdr.sh_addralign = 1; }

  virtual void copy_to(uint8_t *buf) = 0;
  virtual void relocate(uint8_t *buf) {}

  bool is_bss() const { return shdr.sh_type & llvm::ELF::SHT_NOBITS; }

  virtual uint64_t get_size() const = 0;

  StringRef name;
  int idx = 0;
  bool starts_new_ptload = false;
  ELF64LE::Shdr shdr = {};
};

// ELF header
class OutputEhdr : public OutputChunk {
public:
  OutputEhdr() { shdr.sh_flags = llvm::ELF::SHF_ALLOC; }

  void copy_to(uint8_t *buf) override {}
  void relocate(uint8_t *buf) override;

  uint64_t get_size() const override {
    return sizeof(ELF64LE::Ehdr);
  }
};

// Section header
class OutputShdr : public OutputChunk {
public:
  OutputShdr() { shdr.sh_flags = llvm::ELF::SHF_ALLOC; }

  void copy_to(uint8_t *buf) override {
    auto *p = (ELF64LE::Shdr *)(buf + shdr.sh_offset);
    for (ELF64LE::Shdr *ent : entries)
      *p++ = *ent;
  }

  uint64_t get_size() const override {
    return entries.size() * sizeof(ELF64LE::Shdr);
  }

  std::vector<ELF64LE::Shdr *> entries;
};

// Program header
class OutputPhdr : public OutputChunk {
public:
  OutputPhdr() { shdr.sh_flags = llvm::ELF::SHF_ALLOC; }

  void copy_to(uint8_t *buf) override;

  uint64_t get_size() const override {
    return entries.size() * sizeof(ELF64LE::Phdr);
  }

  void construct(std::vector<OutputChunk *> &sections);

private:
  struct Phdr {
    ELF64LE::Phdr phdr;
    std::vector<OutputChunk *> members;
  };

  std::vector<Phdr> entries;
};

// Sections
class OutputSection : public OutputChunk {
public:
  static OutputSection *get_instance(StringRef name, uint64_t flags, uint32_t type);

  OutputSection(StringRef name, uint64_t flags, uint32_t type) {
    this->name = name;
    shdr.sh_flags = flags;
    shdr.sh_type = type;
    idx = all_instances.size();
    all_instances.push_back(this);
  }

  void copy_to(uint8_t *buf) override {
    if (!is_bss())
      for_each(sections, [&](InputSection *isec) { isec->copy_to(buf); });
  }

  void relocate(uint8_t *buf) override {
    if (!is_bss())
      for_each(sections, [&](InputSection *isec) { isec->relocate(buf); });
  }

  uint64_t get_size() const override {
    return shdr.sh_size;
  }

  std::vector<InputSection *> sections;
  uint32_t idx;

  static std::vector<OutputSection *> all_instances;
};

class InterpSection : public OutputChunk {
public:
  InterpSection() {
    name = ".interp";
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_type = llvm::ELF::SHT_PROGBITS;
  }

  void copy_to(uint8_t *buf) override {
    memcpy(buf + shdr.sh_offset, path, sizeof(path));
  }

  uint64_t get_size() const override { return sizeof(path); }

private:
  static constexpr char path[] = "/lib64/ld-linux-x86-64.so.2";
};


class ShstrtabSection : public OutputChunk {
public:
  ShstrtabSection() {
    this->name = ".shstrtab";
    contents = '\0';
    shdr.sh_flags = 0;
    shdr.sh_type = llvm::ELF::SHT_STRTAB;
  }

  uint64_t add_string(StringRef s) {
    uint64_t ret = contents.size();
    contents += s.str();
    contents += '\0';
    return ret;
  }

  void copy_to(uint8_t *buf) override {
    memcpy(buf + shdr.sh_offset, &contents[0], contents.size());
  }

  uint64_t get_size() const override { return contents.size(); }

private:
  std::string contents;
};

class SymtabSection : public OutputChunk {
public:
  SymtabSection() {
    this->name = ".symtab";
    shdr.sh_flags = 0;
    shdr.sh_type = llvm::ELF::SHT_SYMTAB;
    shdr.sh_entsize = sizeof(ELF64LE::Sym);
    shdr.sh_addralign = 8;
  }

  void copy_to(uint8_t *buf) override {}
  uint64_t get_size() const override { return size; }

  uint64_t size = 0;

private:
  std::vector<ELF64LE::Sym> contents;
};

class StrtabSection : public OutputChunk {
public:
  StrtabSection() {
    this->name = ".strtab";
    shdr.sh_flags = 0;
    shdr.sh_type = llvm::ELF::SHT_STRTAB;
  }

  void copy_to(uint8_t *buf) override {}
  uint64_t get_size() const override { return size; }

  uint64_t size = 1;
};

namespace out {
extern OutputEhdr *ehdr;
extern OutputShdr *shdr;
extern OutputPhdr *phdr;
extern InterpSection *interp;
extern ShstrtabSection *shstrtab;
extern SymtabSection *symtab;
extern StrtabSection *strtab;
}

//
// input_files.cc
//

struct ComdatGroup {
  ComdatGroup(ObjectFile *file, uint32_t i)
    : file(file), section_idx(i) {}
  ComdatGroup(const ComdatGroup &other)
    : file(other.file.load()), section_idx(other.section_idx) {}

  std::atomic_flag lock = ATOMIC_FLAG_INIT;
  std::atomic<ObjectFile *> file;
  uint32_t section_idx;
};

struct StringPiece {
  StringPiece(StringRef data) : data(data) {}
  StringPiece(const StringPiece &other) : data(other.data) {}

  StringRef data;
  std::atomic_flag flag = ATOMIC_FLAG_INIT;
};

class ObjectFile {
public:
  ObjectFile(MemoryBufferRef mb, StringRef archive_name);

  void parse();
  void register_defined_symbols();
  void register_undefined_symbols();
  void eliminate_duplicate_comdat_groups();
  void convert_common_symbols();
  void scan_relocations();
  void fix_sym_addrs();
  void compute_symtab();

  std::pair<uint64_t, uint64_t>
  write_symtab_local(uint8_t *buf, uint64_t symtab_off, uint64_t strtab_off);

  std::pair<uint64_t, uint64_t>
  write_symtab_global(uint8_t *buf, uint64_t symtab_off, uint64_t strtab_off);

  StringRef get_filename();
  bool is_in_archive();

  Symbol *get_symbol(uint32_t idx) const {
    if (idx < first_global)
      return nullptr;
    return symbols[idx - first_global];
  }

  uint64_t get_symbol_value(uint32_t idx) const {
    if (idx < first_global) {
      const ELF64LE::Sym &sym = elf_syms[idx];

      if (sym.st_shndx == llvm::ELF::SHN_ABS)
        return sym.st_value;

      InputSection *isec = sections[sym.st_shndx];
      if (isec)
        return isec->output_section->shdr.sh_addr + isec->offset + sym.st_value;
      return 0;
    }
    return symbols[idx - first_global]->addr;
  }

  std::vector<InputSection *> sections;
  StringRef archive_name;
  ELFFile<ELF64LE> obj;
  uint32_t priority;
  std::atomic_bool is_alive;

  uint64_t symtab_size = 0;
  uint64_t strtab_size = 0;

private:
  void initialize_sections();
  void initialize_symbols();
  void remove_comdat_members(uint32_t section_idx);
  void read_string_pieces(const ELF64LE::Shdr &shdr);

  MemoryBufferRef mb;
  std::vector<std::pair<ComdatGroup *, uint32_t>> comdat_groups;
  std::vector<StringPiece *> merged_strings_alloc;
  std::vector<StringPiece *> merged_strings_noalloc;

  std::vector<Symbol *> symbols;
  int first_global = 0;
  bool has_common_symbol;

  ArrayRef<ELF64LE::Shdr> elf_sections;
  ArrayRef<ELF64LE::Sym> elf_syms;
  StringRef symbol_strtab;
  const ELF64LE::Shdr *symtab_sec;

  // For .strtab construction
  std::vector<StringRef> local_symbols;
};

//
// writer.cc
//

void write();

//
// output_file.cc
//

class OutputFile {
public:
  OutputFile(uint64_t size);
  void commit();

private:
  std::unique_ptr<llvm::FileOutputBuffer> output_buffer;
  uint8_t *buf;
};

//
// main.cc
//

MemoryBufferRef readFile(StringRef path);

extern std::atomic_int num_defined;
extern std::atomic_int num_undefined;
extern std::atomic_int num_all_syms;
extern std::atomic_int num_comdats;
extern std::atomic_int num_regular_sections;
extern std::atomic_int num_files;
extern std::atomic_int num_relocs;
extern std::atomic_int num_relocs_alloc;
extern std::atomic_int num_string_pieces;
