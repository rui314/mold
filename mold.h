#pragma once

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include "llvm/ADT/ArrayRef.h"
#include "llvm/ADT/SmallVector.h"
#include "llvm/ADT/Twine.h"
#include "llvm/Object/Archive.h"
#include "llvm/Object/ELF.h"
#include "llvm/Object/ELFTypes.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/Timer.h"
#include "tbb/concurrent_hash_map.h"
#include "tbb/global_control.h"
#include "tbb/parallel_for_each.h"
#include "tbb/parallel_invoke.h"
#include "tbb/parallel_reduce.h"
#include "tbb/spin_mutex.h"
#include "tbb/task_group.h"

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <mutex>
#include <string>

#define SECTOR_SIZE 512
#define PAGE_SIZE 4096
#define GOT_SIZE 8
#define PLT_SIZE 16

#define LIKELY(x)   __builtin_expect((x), 1)
#define UNLIKELY(x) __builtin_expect((x), 0)

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

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
class InputChunk;
class InputSection;
class MergeableSection;
class ObjectFile;
class OutputChunk;
class OutputSection;
class MergedSection;

struct Config {
  StringRef dynamic_linker = "/lib64/ld-linux-x86-64.so.2";
  StringRef output;
  bool is_static = false;
  bool print_map = false;
  int filler = -1;
};

inline Config config;

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

std::string toString(ObjectFile *);

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

  size_t size() const { return map.size(); }

private:
  MapT map;
};

//
// Symbol
//

struct StringPiece {
  StringPiece(StringRef data) : data(data) {}

  StringPiece(const StringPiece &other)
    : data(other.data), isec(other.isec.load()),
      output_offset(other.output_offset) {}

  inline u64 get_addr() const;

  StringRef data;
  std::atomic<MergeableSection *> isec = ATOMIC_VAR_INIT(nullptr);
  u32 output_offset = -1;
};

struct StringPieceRef {
  StringPiece *piece = nullptr;
  u32 input_offset = 0;
  u32 addend = 0;
};

class Symbol {
public:
  Symbol(StringRef name, ObjectFile *file = nullptr)
    : name(name), file(file), is_placeholder(false), is_dso(false),
      is_weak(false), is_undef_weak(false), traced(false) {}

  Symbol(const Symbol &other) : Symbol(other.name, other.file) {}

  static Symbol *intern(StringRef name) {
    static ConcurrentMap<Symbol> map;
    return map.insert(name, Symbol(name));    
  }

  inline u64 get_addr() const;

  StringRef name;
  ObjectFile *file = nullptr;
  InputSection *input_section = nullptr;
  StringPieceRef piece_ref;

  u64 value = 0;
  u32 got_offset = 0;
  u32 gotplt_offset = 0;
  u32 gottp_offset = 0;
  u32 plt_offset = 0;
  u32 relplt_offset = 0;
  u32 dynsym_offset = 0;

  u32 shndx = 0;

  tbb::spin_mutex mu;

  u8 is_placeholder : 1;
  u8 is_dso : 1;
  u8 is_weak : 1;
  u8 is_undef_weak : 1;
  u8 traced : 1;

  enum { NEEDS_GOT = 1, NEEDS_GOTTP = 2, NEEDS_PLT = 4, NEEDS_DYNSYM = 8 };
  std::atomic_uint8_t flags = ATOMIC_VAR_INIT(0);

  u8 visibility = 0;
  u8 type = llvm::ELF::STT_NOTYPE;
};

inline std::string toString(Symbol sym) {
  return (StringRef(sym.name) + "(" + toString(sym.file) + ")").str();
}

//
// input_sections.cc
//

class InputChunk {
public:
  enum Kind : u8 { REGULAR, MERGEABLE };

  virtual void scan_relocations() {}
  virtual void copy_to(u8 *buf) {}

  ObjectFile *file;
  const ELF64LE::Shdr &shdr;
  OutputSection *output_section = nullptr;

  StringRef name;
  u32 offset;
  Kind kind;

protected:
  InputChunk(Kind kind, ObjectFile *file, const ELF64LE::Shdr &shdr, StringRef name);
};

class InputSection : public InputChunk {
public:
  InputSection(ObjectFile *file, const ELF64LE::Shdr &shdr, StringRef name)
    : InputChunk(REGULAR, file, shdr, name) {}

  void copy_to(u8 *buf) override;
  void scan_relocations() override;

  ArrayRef<ELF64LE::Rela> rels;
  std::vector<StringPieceRef> rel_pieces;
  MergeableSection *mergeable = nullptr;
};

class MergeableSection : public InputChunk {
public:
  MergeableSection(InputSection *isec, ArrayRef<u8> contents);

  MergedSection &parent;
  std::vector<StringPieceRef> pieces;
  u32 size = 0;
};

std::string toString(InputChunk *isec);

inline u64 align_to(u64 val, u64 align) {
  assert(__builtin_popcount(align) == 1);
  return (val + align - 1) & ~(align - 1);
}

//
// output_chunks.cc
//

class OutputChunk {
public:
  enum Kind : u8 { HEADER, REGULAR, SYNTHETIC };

  OutputChunk(Kind kind) : kind(kind) { shdr.sh_addralign = 1; }

  virtual void initialize(u8 *buf) {}
  virtual void copy_to(u8 *buf) {}

  StringRef name;
  Kind kind;
  int shndx = 0;
  bool starts_new_ptload = false;
  ELF64LE::Shdr shdr = {};
};

// ELF, Section or Program header
class OutputHeader : public OutputChunk {
public:
  OutputHeader() : OutputChunk(HEADER) {
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
  }
};

// Sections
class OutputSection : public OutputChunk {
public:
  static OutputSection *get_instance(StringRef name, u64 flags, u32 type);

  OutputSection(StringRef name, u64 flags, u32 type)
    : OutputChunk(REGULAR) {
    this->name = name;
    shdr.sh_flags = flags;
    shdr.sh_type = type;
    idx = instances.size();
    instances.push_back(this);
  }

  void copy_to(u8 *buf) override;
  bool empty() const;

  static inline std::vector<OutputSection *> instances;

  std::vector<InputChunk *> members;
  u32 idx;
};

class SpecialSection : public OutputChunk {
public:
  SpecialSection(StringRef name, u32 type, u64 flags, u32 align = 1, u32 entsize = 0)
    : OutputChunk(SYNTHETIC) {
    this->name = name;
    shdr.sh_flags = flags;
    shdr.sh_type = type;
    shdr.sh_addralign = align;
    shdr.sh_entsize = entsize;
  }
};

class PltSection : public OutputChunk {
public:
  PltSection() : OutputChunk(SYNTHETIC) {
    this->name = ".plt";
    shdr.sh_flags = llvm::ELF::SHF_ALLOC | llvm::ELF::SHF_EXECINSTR;
    shdr.sh_type = llvm::ELF::SHT_PROGBITS;
    shdr.sh_addralign = 8;
  }

  void write_entry(u8 *buf, u32 value) {
    memset(buf, 0, 16);
    buf[0] = 0xff;
    buf[1] = 0x25;
    *(u32 *)(buf + 2) = value;
  }
};

class RelPltSection : public OutputChunk {
public:
  RelPltSection() : OutputChunk(SYNTHETIC) {
    this->name = ".rela.plt";
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_type = llvm::ELF::SHT_RELA;
    shdr.sh_entsize = sizeof(ELF64LE::Rela);
    shdr.sh_addralign = 8;
  }
};

class ShstrtabSection : public OutputChunk {
public:
  ShstrtabSection() : OutputChunk(SYNTHETIC) {
    this->name = ".shstrtab";
    contents = '\0';
    shdr.sh_flags = 0;
    shdr.sh_type = llvm::ELF::SHT_STRTAB;
    shdr.sh_size = 1;
  }

  u64 add_string(StringRef s) {
    u64 ret = contents.size();
    contents += s.str();
    contents += '\0';
    shdr.sh_size = contents.size();
    return ret;
  }

  void copy_to(u8 *buf) override {
    memcpy(buf + shdr.sh_offset, &contents[0], contents.size());
  }

private:
  std::string contents;
};

class SymtabSection : public OutputChunk {
public:
  SymtabSection(StringRef name, u32 flags) : OutputChunk(SYNTHETIC) {
    this->name = name;
    shdr.sh_type = llvm::ELF::SHT_SYMTAB;
    shdr.sh_flags = flags;
    shdr.sh_entsize = sizeof(ELF64LE::Sym);
    shdr.sh_addralign = 8;
    shdr.sh_size = sizeof(ELF64LE::Sym);
    contents.push_back({});
  }

private:
  std::vector<ELF64LE::Sym> contents;
};

class HashSection : public OutputChunk {
public:
  HashSection() : OutputChunk(SYNTHETIC) {
    this->name = ".hash";
    shdr.sh_type = llvm::ELF::SHT_HASH;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_entsize = 4;
    shdr.sh_addralign = 4;
  }

  void initialize(u8 *buf) override {
    u32 *hdr = (u32 *)(buf + shdr.sh_offset);
    memset(buf + shdr.sh_offset, 0, shdr.sh_size);
    hdr[0] = hdr[1] = num_dynsym;
  }

  inline void write_symbol(u8 *buf, Symbol *sym);

  void set_num_dynsym(u32 num_dynsym) {
    this->num_dynsym = num_dynsym;
    shdr.sh_size = num_dynsym * 8 + 8;
  }

private:
  u32 num_dynsym;

  static u32 hash(StringRef name) {
    u32 h = 0;
    for (char c : name) {
      h = (h << 4) + c;
      u32 g = h & 0xf0000000;
      if (g != 0)
        h ^= g >> 24;
      h &= ~g;
    }
    return h;
  }
};

class MergedSection : public OutputChunk {
public:
  static MergedSection *get_instance(StringRef name, u64 flags, u32 type);

  static inline std::vector<MergedSection *> instances;

  ConcurrentMap<StringPiece> map;

private:
  MergedSection(StringRef name, u64 flags, u32 type)
    : OutputChunk(SYNTHETIC) {
    this->name = name;
    shdr.sh_flags = flags;
    shdr.sh_type = type;
    shdr.sh_addralign = 1;
  }
};

bool is_c_identifier(StringRef name);

namespace out {
using namespace llvm::ELF;

inline OutputHeader *ehdr;
inline OutputHeader *shdr;
inline OutputHeader *phdr;
inline SpecialSection *interp;
inline SpecialSection *got;
inline SpecialSection *gotplt;
inline SpecialSection *relplt;
inline SpecialSection *reldyn;
inline SpecialSection *dynamic;
inline SpecialSection *strtab;
inline SpecialSection *dynstr;
inline HashSection *hash;
inline ShstrtabSection *shstrtab;
inline PltSection *plt;
inline SymtabSection *symtab;
inline SymtabSection *dynsym;

inline u64 tls_end;

inline Symbol *__bss_start;
inline Symbol *__ehdr_start;
inline Symbol *__rela_iplt_start;
inline Symbol *__rela_iplt_end;
inline Symbol *__init_array_start;
inline Symbol *__init_array_end;
inline Symbol *__fini_array_start;
inline Symbol *__fini_array_end;
inline Symbol *__preinit_array_start;
inline Symbol *__preinit_array_end;
inline Symbol *_end;
inline Symbol *_etext;
inline Symbol *_edata;
}

inline u64 Symbol::get_addr() const {
  if (piece_ref.piece)
    return piece_ref.piece->get_addr() + piece_ref.addend;

  if (input_section)
    return input_section->output_section->shdr.sh_addr +
           input_section->offset + value;

  return value;
}

inline u64 StringPiece::get_addr() const {
  MergeableSection *is = isec.load();
  return is->parent.shdr.sh_addr + is->offset + output_offset;
}

inline void write_string(u8 *buf, StringRef str) {
  memcpy(buf, str.data(), str.size());
  buf[str.size()] = '\0';
}

//
// object_file.cc
//

struct ComdatGroup {
  ComdatGroup(ObjectFile *file, u32 i)
    : file(file), section_idx(i) {}
  ComdatGroup(const ComdatGroup &other)
    : file(other.file.load()), section_idx(other.section_idx) {}

  std::atomic<ObjectFile *> file;
  u32 section_idx;
};

class ObjectFile {
public:
  ObjectFile(MemoryBufferRef mb, StringRef archive_name);

  void parse();
  void initialize_mergeable_sections();
  void resolve_symbols();
  void mark_live_archive_members(tbb::parallel_do_feeder<ObjectFile *> &feeder);
  void hanlde_undefined_weak_symbols();
  void resolve_comdat_groups();
  void eliminate_duplicate_comdat_groups();
  void assign_mergeable_string_offsets();
  void convert_common_symbols();
  void scan_relocations();
  void compute_symtab();

  void write_local_symtab(u8 *buf, u64 symtab_off, u64 strtab_off);
  void write_global_symtab(u8 *buf, u64 symtab_off, u64 strtab_off);

  static ObjectFile *create_internal_file(std::vector<OutputChunk *> chunks);

  std::string name;
  StringRef archive_name;
  ELFFile<ELF64LE> obj;
  std::vector<InputSection *> sections;
  std::vector<Symbol *> symbols;
  ArrayRef<ELF64LE::Sym> elf_syms;
  int first_global = 0;
  u32 priority;
  std::atomic_bool is_alive = ATOMIC_VAR_INIT(false);
  bool is_dso = false;
  const bool is_in_archive;

  u64 local_symtab_size = 0;
  u64 local_strtab_size = 0;
  u64 global_symtab_size = 0;
  u64 global_strtab_size = 0;

  std::atomic_uint32_t plt_size = ATOMIC_VAR_INIT(0);
  std::atomic_uint32_t got_size = ATOMIC_VAR_INIT(0);
  std::atomic_uint32_t gotplt_size = ATOMIC_VAR_INIT(0);
  std::atomic_uint32_t relplt_size = ATOMIC_VAR_INIT(0);
  std::atomic_uint32_t dynsym_size = ATOMIC_VAR_INIT(0);
  std::atomic_uint32_t dynstr_size = ATOMIC_VAR_INIT(0);

  u32 got_offset = 0;
  u32 gotplt_offset = 0;
  u32 plt_offset = 0;
  u32 relplt_offset = 0;
  u32 dynsym_offset = 0;
  u32 dynstr_offset = 0;

  std::vector<MergeableSection> mergeable_sections;

private:
  void initialize_sections();
  void initialize_symbols();
  std::vector<StringPieceRef> read_string_pieces(InputSection *isec);

  void maybe_override_symbol(const ELF64LE::Sym &esym, Symbol &sym, int idx);
  void write_symtab(u8 *buf, u64 symtab_off, u64 strtab_off, u32 start, u32 end);

  MemoryBufferRef mb;
  std::vector<std::pair<ComdatGroup *, ArrayRef<ELF64LE::Word>>> comdat_groups;

  std::vector<Symbol> local_symbols;
  std::vector<StringPieceRef> sym_pieces;
  bool has_common_symbol;

  ArrayRef<ELF64LE::Shdr> elf_sections;
  StringRef symbol_strtab;
  const ELF64LE::Shdr *symtab_sec;
};

inline void HashSection::write_symbol(u8 *buf, Symbol *sym) {
  u32 dynsym_idx = sym->dynsym_offset / sizeof(ELF64LE::Sym);
  u32 *buckets = (u32 *)(buf + shdr.sh_offset + 8);
  u32 *chains = buckets + num_dynsym;
  u32 idx = hash(sym->name) % num_dynsym;
  chains[dynsym_idx] = buckets[idx];
  buckets[idx] = dynsym_idx;
}

//
// perf.cc
//

class Counter {
public:
  Counter(StringRef name, u32 value = 0) : name(name), value(value) {
    static std::mutex mu;
    std::lock_guard lock(mu);
    instances.push_back(this);
  }

  void inc(u32 delta = 1) {
    if (enabled)
      value += delta;
  }

  void set(u32 value) {
    this->value = value;
  }

  static void print();

  static bool enabled;

private:
  StringRef name;
  std::atomic_uint32_t value;
  
  static std::vector<Counter *> instances;
};

//
// mapfile.cc
//

void print_map(ArrayRef<ObjectFile *> files, ArrayRef<OutputChunk *> output_sections);
