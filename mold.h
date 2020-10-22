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
#include "tbb/parallel_for_each.h"
#include "tbb/parallel_sort.h"
#include "tbb/partitioner.h"

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <shared_mutex>
#include <string>
#include <unordered_set>

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
struct tbb_hash<StringRef> {
  size_t operator()(const StringRef& k) const {
    return llvm::hash_value(k);
  }
};

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

  StringRef name;
  ObjectFile *file = nullptr;
  std::atomic_flag lock = ATOMIC_FLAG_INIT;
};

inline std::string toString(Symbol sym) {
  return (StringRef(sym.name) + "(" + toString(sym.file) + ")").str();
}

//
// input_chunks.cc
//

class InputChunk {
public:
  virtual void copy_to(uint8_t *buf) = 0;
  virtual void relocate(uint8_t *buf) {}
  virtual uint64_t get_size() const = 0;

  StringRef name;
  int64_t offset = -1;
  uint64_t flags;
  uint32_t type;
  uint32_t alignment = 1;
};

class InputSection : public InputChunk {
public:
  InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name);
  void copy_to(uint8_t *buf) override;
  void relocate(uint8_t *buf) override;
  uint64_t get_size() const override;

  ObjectFile *file;
  OutputSection *output_section;
  ArrayRef<ELF64LE::Rela> rels;
  const ELF64LE::Shdr *hdr;
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
  virtual void copy_to(uint8_t *buf) = 0;
  virtual void relocate(uint8_t *buf) {}
  virtual void set_offset(uint64_t off) { offset = off; }
  uint64_t get_offset() const { return offset; }
  virtual uint64_t get_size() const = 0;
  virtual const ELF64LE::Shdr *get_shdr() const { return nullptr; }

  StringRef name;
  ELF64LE::Shdr hdr = {};

protected:
  int64_t offset = -1;
  int64_t size = -1;
};

// ELF header
class OutputEhdr : public OutputChunk {
public:
  void copy_to(uint8_t *buf) override {}
  void relocate(uint8_t *buf) override;

  uint64_t get_size() const override {
    return sizeof(ELF64LE::Ehdr);
  }
};

// Section header
class OutputShdr : public OutputChunk {
public:
  void copy_to(uint8_t *buf) override {
    memcpy(buf + offset, &hdr[0], get_size());
  }

  uint64_t get_size() const override {
    return hdr.size() * sizeof(hdr[0]);
  }

  std::vector<ELF64LE::Shdr> hdr;
};

// Program header
class OutputPhdr : public OutputChunk {
public:
  void copy_to(uint8_t *buf) override {
    memcpy(buf + offset, &hdr[0], get_size());
  }

  uint64_t get_size() const override {
    return hdr.size() * sizeof(hdr[0]);
  }

  std::vector<ELF64LE::Phdr> hdr;
};

// Sections
class OutputSection : public OutputChunk {
public:
  static OutputSection *get_instance(InputSection *isec);

  OutputSection(StringRef name, uint64_t flags, uint32_t type) {
    this->name = name;
    hdr.sh_flags = flags;
    hdr.sh_type = type;
    all_instances.push_back(this);
  }

  void copy_to(uint8_t *buf) override {
    for_each(chunks, [&](InputChunk *isec) { isec->copy_to(buf); });
  }

  void relocate(uint8_t *buf) override {
    for_each(chunks, [&](InputChunk *isec) { isec->relocate(buf); });
  }

  uint64_t get_size() const override {
    assert(size >= 0);
    return size;
  }

  void set_offset(uint64_t off) override;
  const ELF64LE::Shdr *get_shdr() const override { return &hdr; }

  std::vector<InputChunk *> chunks;
  static std::vector<OutputSection *> all_instances;

private:
  uint64_t file_offset = 0;
  uint64_t on_file_size = -1;
};

class InterpSection : public OutputChunk {
public:
  InterpSection() {
    name = ".interp";
    hdr.sh_flags = llvm::ELF::SHF_ALLOC;
    hdr.sh_type = llvm::ELF::SHT_PROGBITS;
  }

  void copy_to(uint8_t *buf) override {
    memcpy(buf + offset, path, sizeof(path));
  }

  uint64_t get_size() const override { return sizeof(path); }

private:
  static constexpr char path[] = "/lib64/ld-linux-x86-64.so.2";
};


class StringTableSection : public OutputChunk {
public:
  StringTableSection(StringRef name) {
    this->name = name;
    hdr.sh_flags = 0;
    hdr.sh_type = llvm::ELF::SHT_STRTAB;
  }

  uint64_t addString(StringRef s);
  void copy_to(uint8_t *buf) override;
  uint64_t get_size() const override { return contents.size(); }

private:
  std::string contents;
};

namespace out {
extern OutputEhdr *ehdr;
extern OutputShdr *shdr;
extern OutputPhdr *phdr;
extern StringTableSection *shstrtab;
}

//
// input_files.cc
//

class ObjectFile { 
public:
  ObjectFile(MemoryBufferRef mb, StringRef archive_name);

  void parse();
  void register_defined_symbols();
  void register_undefined_symbols();
  void eliminate_duplicate_comdat_groups();
  StringRef get_filename();
  bool is_in_archive();

  std::vector<InputSection *> sections;
  StringRef archive_name;
  ELFFile<ELF64LE> obj;
  uint32_t priority;
  std::atomic_bool is_alive;

private:
  void initialize_sections();
  void initialize_symbols();

  MemoryBufferRef mb;
  std::vector<Symbol *> symbols;
  std::vector<std::pair<bool *, ArrayRef<ELF64LE::Word>>> comdat_groups;

  ArrayRef<ELF64LE::Shdr> elf_sections;
  ArrayRef<ELF64LE::Sym> elf_syms;
  StringRef string_table;
  const ELF64LE::Shdr *symtab_sec;
  int first_global = 0;
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

extern SymbolTable symbol_table;
extern std::atomic_int num_defined;
extern std::atomic_int num_undefined;
extern std::atomic_int num_files;
extern std::atomic_int num_relocs;
