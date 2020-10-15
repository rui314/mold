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
#include "tbb/parallel_for_each.h"
#include "tbb/parallel_sort.h"
#include "tbb/partitioner.h"

#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <cstring>
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
class ObjectFile;

//
// symtab.cc
//

class Symbol {
public:
  StringRef name;
  ObjectFile *file;
};

namespace tbb {
template<>
struct tbb_hash_compare<StringRef> {
  static size_t hash(const StringRef &k) {
    return llvm::hash_value(k);
  }

  static bool equal(const StringRef &k1, const StringRef &k2) {
    return k1 == k2;
  }
};
}

class SymbolTable {
public:
  Symbol *add(StringRef key, Symbol sym);
  Symbol *get(StringRef key);
  std::vector<StringRef> get_keys();

private:
  typedef tbb::concurrent_hash_map<StringRef, Symbol> MapType;

  MapType map;
};

//
// input_sections.cc
//

class InputSection {
public:
  InputSection(ObjectFile *file, const ELF64LE::Shdr *hdr, StringRef name);
  void writeTo(uint8_t *buf);
  uint64_t get_size() const;

  StringRef name;
  uint64_t output_file_offset;
  int64_t offset = -1;

private:
  const ELF64LE::Shdr *hdr;
  ObjectFile *file;
};

//
// output_sections.cc
//

class OutputChunk {
public:
  virtual ~OutputChunk() {}

  virtual void writeTo(uint8_t *buf) = 0;
  virtual void set_offset(uint64_t off) { offset = off; }
  uint64_t get_offset() const { return offset; }
  virtual uint64_t get_size() const = 0;

protected:
  int64_t offset = -1;
  int64_t size = -1;
};

class OutputEhdr : public OutputChunk {
public:
  void writeTo(uint8_t *buf) override;
  uint64_t get_size() const override;

  ELF64LE::Ehdr hdr;
};

class OutputShdr : public OutputChunk {
public:
  void writeTo(uint8_t *buf) override;
  uint64_t get_size() const override;

  std::vector<ELF64LE::Shdr> hdr;
};

class OutputPhdr : public OutputChunk {
public:
  void writeTo(uint8_t *buf) override;
  uint64_t get_size() const override;

  std::vector<ELF64LE::Phdr> hdr;
};

class OutputSection : public OutputChunk {
public:
  OutputSection(StringRef name);

  void writeTo(uint8_t *buf) override;
  uint64_t get_size() const override;
  void set_offset(uint64_t off) override;

  std::vector<InputSection *> sections;
  StringRef name;

private:
  uint64_t file_offset = 0;
  uint64_t on_file_size = -1;
};

//
// input_files.cc
//

class ObjectFile { 
public:
  ObjectFile(MemoryBufferRef mb, StringRef archive_name);

  void parse();
  void register_defined_symbols();
  void register_undefined_symbols();
  StringRef get_filename();

  std::vector<InputSection *> sections;
  StringRef archive_name;
  int priority;
  bool is_alive = false;
  std::unordered_set<ObjectFile *> liveness_edges;

private:
  MemoryBufferRef mb;
  std::vector<Symbol *> symbols;
  std::vector<Symbol> symbol_instances;
  ArrayRef<ELF64LE::Sym> elf_syms;
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

std::string toString(ObjectFile *);

extern SymbolTable symbol_table;
extern std::atomic_int num_defined;
extern std::atomic_int num_undefined;
extern std::atomic_int num_files;
