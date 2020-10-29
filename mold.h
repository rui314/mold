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
#include "llvm/Support/Format.h"
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
#include "tbb/spin_mutex.h"
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
#include <valarray>
#include <xmmintrin.h>

#define SECTOR_SIZE 512
#define PAGE_SIZE 4096

typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

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
  bool print_map = false;
};

extern Config config;

[[noreturn]] inline void error(const Twine &msg) {
  static std::mutex mu;
  std::lock_guard lock(mu);

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
#if 1
  tbb::parallel_for_each(arr.begin(), arr.end(), callback);
#else
  std::for_each(arr.begin(), arr.end(), callback);
#endif
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

  ValueT *insert(StringRef key, const ValueT &val) {
    typename MapT::const_accessor acc;
    map.insert(acc, std::make_pair(key, val));
    return const_cast<ValueT *>(&acc->second);
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

  tbb::spin_mutex mu;
  StringRef name;
  ObjectFile *file = nullptr;
  InputSection *input_section = nullptr;

  u64 addr = 0;
  u64 got_addr = 0;
  u64 plt_addr = 0;

  u64 value;
  uint8_t visibility = 0;
  bool is_weak = false;

  std::atomic_bool needs_got = ATOMIC_VAR_INIT(false);
  std::atomic_bool needs_plt =  ATOMIC_VAR_INIT(false);
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
  std::tuple<u64, u64> scan_relocations();

  ObjectFile *file;
  OutputSection *output_section;
  ArrayRef<ELF64LE::Rela> rels;
  const ELF64LE::Shdr &shdr;

  StringRef name;
  u64 offset;
};

std::string toString(InputSection *isec);

inline u64 align_to(u64 val, u64 align) {
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

  bool is_bss() const { return shdr.sh_type == llvm::ELF::SHT_NOBITS; }

  virtual u64 get_size() const = 0;

  StringRef name;
  int shndx = 0;
  bool starts_new_ptload = false;
  ELF64LE::Shdr shdr = {};
  std::vector<InputSection *> sections;
};

// ELF header
class OutputEhdr : public OutputChunk {
public:
  OutputEhdr() { shdr.sh_flags = llvm::ELF::SHF_ALLOC; }

  void copy_to(uint8_t *buf) override {}
  void relocate(uint8_t *buf) override;

  u64 get_size() const override {
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

  u64 get_size() const override {
    return entries.size() * sizeof(ELF64LE::Shdr);
  }

  std::vector<ELF64LE::Shdr *> entries;
};

// Program header
class OutputPhdr : public OutputChunk {
public:
  OutputPhdr() { shdr.sh_flags = llvm::ELF::SHF_ALLOC; }

  void copy_to(uint8_t *buf) override;

  u64 get_size() const override {
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
  static OutputSection *get_instance(StringRef name, u64 flags, u32 type);

  OutputSection(StringRef name, u64 flags, u32 type) {
    this->name = name;
    shdr.sh_flags = flags;
    shdr.sh_type = type;
    idx = instances.size();
    instances.push_back(this);
  }

  void copy_to(uint8_t *buf) override {
    if (!is_bss())
      for_each(sections, [&](InputSection *isec) { isec->copy_to(buf); });
  }

  void relocate(uint8_t *buf) override {
    if (!is_bss())
      for_each(sections, [&](InputSection *isec) { isec->relocate(buf); });
  }

  u64 get_size() const override {
    return shdr.sh_size;
  }

  bool empty() const {
    if (!sections.empty())
      for (InputSection *isec : sections)
        if (isec->shdr.sh_size)
          return false;
    return true;
  }

  static std::vector<OutputSection *> instances;

  u32 idx;
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

  u64 get_size() const override { return sizeof(path); }

private:
  static constexpr char path[] = "/lib64/ld-linux-x86-64.so.2";
};


class GotSection : public OutputChunk {
public:
  GotSection() {
    name = ".got";
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_type = llvm::ELF::SHT_PROGBITS;
    shdr.sh_addralign = 8;
  }

  void copy_to(uint8_t *buf) override {}

  void relocate(uint8_t *buf) override {
    buf += shdr.sh_offset;
    for (Symbol *sym : symbols) {
      *(u64 *)buf = sym->addr;
      buf += 8;
    }
  }

  u64 get_size() const override { return size; }

  u64 size = 0;
  std::vector<Symbol *> symbols;
};

class ShstrtabSection : public OutputChunk {
public:
  ShstrtabSection() {
    this->name = ".shstrtab";
    contents = '\0';
    shdr.sh_flags = 0;
    shdr.sh_type = llvm::ELF::SHT_STRTAB;
  }

  u64 add_string(StringRef s) {
    u64 ret = contents.size();
    contents += s.str();
    contents += '\0';
    return ret;
  }

  void copy_to(uint8_t *buf) override {
    memcpy(buf + shdr.sh_offset, &contents[0], contents.size());
  }

  u64 get_size() const override { return contents.size(); }

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
  u64 get_size() const override { return size; }

  u64 size = 0;

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
  u64 get_size() const override { return size; }

  u64 size = 1;
};

namespace out {
extern OutputEhdr *ehdr;
extern OutputShdr *shdr;
extern OutputPhdr *phdr;
extern InterpSection *interp;
extern GotSection *got;
extern ShstrtabSection *shstrtab;
extern SymtabSection *symtab;
extern StrtabSection *strtab;
}

//
// input_files.cc
//

struct ComdatGroup {
  ComdatGroup(ObjectFile *file, u32 i)
    : file(file), section_idx(i) {}
  ComdatGroup(const ComdatGroup &other)
    : file(other.file.load()), section_idx(other.section_idx) {}

  tbb::spin_mutex mu;
  std::atomic<ObjectFile *> file;
  u32 section_idx;
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
  void register_undefined_symbols(tbb::parallel_do_feeder<ObjectFile *> &feeder);
  void eliminate_duplicate_comdat_groups();
  void convert_common_symbols();
  std::tuple<u64, u64> scan_relocations();
  void fix_sym_addrs();
  void compute_symtab();

  void write_local_symtab(uint8_t *buf, u64 symtab_off, u64 strtab_off);
  void write_global_symtab(uint8_t *buf, u64 symtab_off, u64 strtab_off);

  StringRef get_filename();
  bool is_in_archive();

  Symbol *get_symbol(u32 idx) const {
    if (idx < first_global)
      return nullptr;
    return symbols[idx - first_global];
  }

  u64 get_symbol_addr(u32 idx) const {
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
  std::vector<Symbol *> symbols;
  u32 priority;
  std::atomic_bool is_alive = ATOMIC_VAR_INIT(false);

  u64 local_symtab_size = 0;
  u64 local_strtab_size = 0;
  u64 global_symtab_size = 0;
  u64 global_strtab_size = 0;

private:
  void initialize_sections();
  void initialize_symbols();
  void remove_comdat_members(u32 section_idx);
  void read_string_pieces(const ELF64LE::Shdr &shdr);

  MemoryBufferRef mb;
  std::vector<std::pair<ComdatGroup *, u32>> comdat_groups;
  std::vector<StringPiece *> merged_strings_alloc;
  std::vector<StringPiece *> merged_strings_noalloc;

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
  OutputFile(u64 size);
  void commit();

private:
  std::unique_ptr<llvm::FileOutputBuffer> output_buffer;
  uint8_t *buf;
};

//
// mapfile.cc
//

void print_map(ArrayRef<ObjectFile *> files, ArrayRef<OutputChunk *> output_sections);

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

//
// Other
//

inline void memcpy_nontemporal(void *dst_, const void *src_, size_t n) {
#if 1
  char *src = (char *)src_;
  char *dst = (char *)dst_;

  if ((uintptr_t)src % 16 || (uintptr_t)dst % 16) {
    memcpy(dst, src, n);
    return;
  }

  size_t i = 0;
  for (; i + 16 < n; i += 16) {
    __m128 val = __builtin_nontemporal_load((__m128 *)(src + i));
    __builtin_nontemporal_store(val, (__m128 *)(dst + i));
  }
  memcpy(dst + i, src + i, n - i);
#else
  memcpy(dst_, src_, n);
#endif
}
