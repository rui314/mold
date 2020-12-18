#pragma once

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include "elf.h"

#include "tbb/concurrent_hash_map.h"
#include "tbb/parallel_for_each.h"
#include "tbb/parallel_invoke.h"
#include "tbb/parallel_reduce.h"
#include "tbb/spin_mutex.h"

#include <algorithm>
#include <atomic>
#include <cassert>
#include <cstdint>
#include <iostream>
#include <mutex>
#include <span>
#include <string>
#include <string_view>

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

class InputChunk;
class InputFile;
class InputSection;
class MergeableSection;
class MergedSection;
class ObjectFile;
class OutputChunk;
class OutputSection;
class SharedFile;
class Symbol;

struct Config {
  std::string dynamic_linker = "/lib64/ld-linux-x86-64.so.2";
  std::string entry = "_start";
  std::string output;
  bool as_needed = false;
  bool export_dynamic = false;
  bool is_static = false;
  bool perf = false;
  bool pie = false;
  bool print_map = false;
  bool trace = false;
  bool z_now = false;
  int filler = -1;
  int thread_count = -1;
  std::string sysroot;
  std::vector<std::string> library_paths;
  std::vector<std::string> rpaths;
  std::vector<std::string> globals;
  u64 image_base = 0x200000;
};

inline Config config;

[[noreturn]] inline void error(std::string msg) {
  static std::mutex mu;
  std::lock_guard lock(mu);
  std::cerr << msg << "\n";
  exit(1);
}

#define unreachable() \
  error("internal error at " + std::string(__FILE__) + ":" + std::to_string(__LINE__))

std::string to_string(const InputFile *);

//
// Interned string
//

namespace tbb {
template<>
struct tbb_hash_compare<std::string_view> {
  static size_t hash(const std::string_view& k) {
    return std::hash<std::string_view>()(k);
  }

  static bool equal(const std::string_view& k1, const std::string_view& k2) {
    return k1 == k2;
  }
};
}

template<typename ValueT>
class ConcurrentMap {
public:
  typedef tbb::concurrent_hash_map<std::string_view, ValueT> MapT;

  ValueT *insert(std::string_view key, const ValueT &val) {
    typename MapT::const_accessor acc;
    map.insert(acc, std::make_pair(key, val));
    return const_cast<ValueT *>(&acc->second);
  }

  void for_each_value(std::function<void(ValueT &)> fn) {
    for (typename MapT::iterator it = map.begin(); it != map.end(); ++it)
      fn(it->second);
  }

  size_t size() const { return map.size(); }

private:
  MapT map;
};

//
// Symbol
//

struct StringPiece {
  StringPiece(std::string_view view)
    : data((const char *)view.data()), size(view.size()) {}

  StringPiece(const StringPiece &other)
    : isec(other.isec.load()), data(other.data), size(other.size),
      output_offset(other.output_offset) {}

  inline u64 get_addr() const;

  std::atomic<MergeableSection *> isec = ATOMIC_VAR_INIT(nullptr);
  const char *data;
  u32 size;
  u32 output_offset = -1;
};

struct StringPieceRef {
  StringPiece *piece = nullptr;
  u32 input_offset = 0;
  i32 addend = 0;
};

enum {
  NEEDS_GOT      = 1 << 0,
  NEEDS_PLT      = 1 << 1,
  NEEDS_GOTTPOFF = 1 << 2,
  NEEDS_TLSGD    = 1 << 3,
  NEEDS_TLSLD    = 1 << 4,
  NEEDS_COPYREL  = 1 << 5,
  NEEDS_DYNSYM   = 1 << 6,
};

class Symbol {
public:
  Symbol(std::string_view name) : name(name) {}
  Symbol(const Symbol &other) : Symbol(other.name) {}

  static Symbol *intern(std::string_view name) {
    static ConcurrentMap<Symbol> map;
    return map.insert(name, Symbol(name));
  }

  inline u64 get_addr() const;
  inline u64 get_got_addr() const;
  inline u64 get_gotplt_addr() const;
  inline u64 get_gottpoff_addr() const;
  inline u64 get_tlsgd_addr() const;
  inline u64 get_tlsld_addr() const;
  inline u64 get_plt_addr() const;

  bool needs_relative_rel() const {
    return config.pie && !is_undef_weak;
  }

  std::string_view name;
  InputFile *file = nullptr;
  const ElfSym *esym = nullptr;
  InputSection *input_section = nullptr;
  StringPieceRef piece_ref;

  u64 value = -1;
  u32 got_idx = -1;
  u32 gotplt_idx = -1;
  u32 gottpoff_idx = -1;
  u32 tlsgd_idx = -1;
  u32 tlsld_idx = -1;
  u32 plt_idx = -1;
  u32 relplt_idx = -1;
  u32 dynsym_idx = -1;
  u32 dynstr_offset = -1;
  u32 copyrel_offset = -1;
  u16 shndx = 0;
  u16 ver_idx = 0;

  tbb::spin_mutex mu;

  u8 is_placeholder : 1 = false;
  u8 is_imported : 1 = false;
  u8 is_weak : 1 = false;
  u8 is_undef_weak : 1 = false;
  u8 traced : 1 = false;

  std::atomic_uint8_t flags = ATOMIC_VAR_INIT(0);
  u8 type = STT_NOTYPE;
};

//
// input_sections.cc
//

class InputChunk {
public:
  virtual void copy_buf() {}
  inline u64 get_addr() const;

  ObjectFile *file;
  const ElfShdr &shdr;
  OutputSection *output_section = nullptr;

  std::string_view name;
  u32 offset;

protected:
  InputChunk(ObjectFile *file, const ElfShdr &shdr, std::string_view name);
};

enum RelType : u8 {
  R_NONE,
  R_ABS,
  R_DYN,
  R_PC,
  R_GOT,
  R_GOTPC,
  R_GOTPCREL,
  R_PLT,
  R_TLSGD,
  R_TLSGD_RELAX_LE,
  R_TLSLD,
  R_TLSLD_RELAX_LE,
  R_TPOFF,
  R_GOTTPOFF,
};

class InputSection : public InputChunk {
public:
  InputSection(ObjectFile *file, const ElfShdr &shdr, std::string_view name)
    : InputChunk(file, shdr, name) {}

  void copy_buf() override;
  void scan_relocations();
  void report_undefined_symbols();

  std::span<ElfRela> rels;
  std::vector<bool> has_rel_piece;
  std::vector<StringPieceRef> rel_pieces;
  std::vector<RelType> rel_types;
  u64 reldyn_offset = 0;
  bool is_comdat_member = false;
  bool is_alive = true;
};

class MergeableSection : public InputChunk {
public:
  MergeableSection(InputSection *isec, std::string_view contents);

  MergedSection &parent;
  std::vector<StringPieceRef> pieces;
  u32 size = 0;
};

std::string to_string(InputChunk *isec);

//
// output_chunks.cc
//

class OutputChunk {
public:
  enum Kind : u8 { HEADER, REGULAR, SYNTHETIC };

  OutputChunk(Kind kind) : kind(kind) { shdr.sh_addralign = 1; }

  virtual void copy_buf() {}
  virtual void update_shdr() {}

  std::string_view name;
  int shndx = 0;
  Kind kind;
  bool starts_new_ptload = false;
  ElfShdr shdr = {};
};

// ELF header
class OutputEhdr : public OutputChunk {
public:
  OutputEhdr() : OutputChunk(HEADER) {
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_size = sizeof(ElfEhdr);
  }

  void copy_buf() override;
};

// Section header
class OutputShdr : public OutputChunk {
public:
  OutputShdr() : OutputChunk(HEADER) {
    shdr.sh_flags = SHF_ALLOC;
  }

  void update_shdr() override;
  void copy_buf() override;
};

// Program header
class OutputPhdr : public OutputChunk {
public:
  OutputPhdr() : OutputChunk(HEADER) {
    shdr.sh_flags = SHF_ALLOC;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class InterpSection : public OutputChunk {
public:
  InterpSection() : OutputChunk(SYNTHETIC) {
    name = ".interp";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_size = config.dynamic_linker.size() + 1;
  }

  void copy_buf() override;
};

// Sections
class OutputSection : public OutputChunk {
public:
  static OutputSection *get_instance(std::string_view name, u32 type, u64 flags);

  OutputSection(std::string_view name, u32 type, u64 flags)
    : OutputChunk(REGULAR) {
    this->name = name;
    shdr.sh_type = type;
    shdr.sh_flags = flags;
    idx = instances.size();
    instances.push_back(this);
  }

  void copy_buf() override;

  static inline std::vector<OutputSection *> instances;

  std::vector<InputChunk *> members;
  u32 idx;
};

class GotSection : public OutputChunk {
public:
  GotSection() : OutputChunk(SYNTHETIC) {
    name = ".got";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    shdr.sh_addralign = GOT_SIZE;
  }

  void add_got_symbol(Symbol *sym);
  void add_gottpoff_symbol(Symbol *sym);
  void add_tlsgd_symbol(Symbol *sym);
  void add_tlsld_symbol(Symbol *sym);
  void copy_buf() override;

  std::vector<Symbol *> got_syms;
  std::vector<Symbol *> gottpoff_syms;
  std::vector<Symbol *> tlsgd_syms;
  std::vector<Symbol *> tlsld_syms;
};

class GotPltSection : public OutputChunk {
public:
  GotPltSection() : OutputChunk(SYNTHETIC) {
    name = ".got.plt";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    shdr.sh_addralign = GOT_SIZE;
    shdr.sh_size = GOT_SIZE * 3;
  }

  void copy_buf() override;
};

class PltSection : public OutputChunk {
public:
  PltSection() : OutputChunk(SYNTHETIC) {
    name = ".plt";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
    shdr.sh_addralign = 8;
    shdr.sh_size = PLT_SIZE;
  }

  void add_symbol(Symbol *sym);
  void copy_buf() override;

  std::vector<Symbol *> symbols;
};

class RelPltSection : public OutputChunk {
public:
  RelPltSection() : OutputChunk(SYNTHETIC) {
    name = ".rela.plt";
    shdr.sh_type = SHT_RELA;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_entsize = sizeof(ElfRela);
    shdr.sh_addralign = 8;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class RelDynSection : public OutputChunk {
public:
  RelDynSection() : OutputChunk(SYNTHETIC) {
    name = ".rela.dyn";
    shdr.sh_type = SHT_RELA;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_entsize = sizeof(ElfRela);
    shdr.sh_addralign = 8;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class StrtabSection : public OutputChunk {
public:
  StrtabSection() : OutputChunk(SYNTHETIC) {
    name = ".strtab";
    shdr.sh_type = SHT_STRTAB;
    shdr.sh_addralign = 1;
    shdr.sh_size = 1;
  }

  void update_shdr() override;
};

class ShstrtabSection : public OutputChunk {
public:
  ShstrtabSection() : OutputChunk(SYNTHETIC) {
    name = ".shstrtab";
    shdr.sh_type = SHT_STRTAB;
    shdr.sh_addralign = 1;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class DynstrSection : public OutputChunk {
public:
DynstrSection() : OutputChunk(SYNTHETIC) {
    name = ".dynstr";
    shdr.sh_type = SHT_STRTAB;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_size = 1;
    shdr.sh_addralign = 1;
  }

  u32 add_string(std::string_view str);
  u32 find_string(std::string_view str);
  void copy_buf() override;

private:
  std::vector<std::string_view> contents;
};

class DynamicSection : public OutputChunk {
public:
  DynamicSection() : OutputChunk(SYNTHETIC) {
    name = ".dynamic";
    shdr.sh_type = SHT_DYNAMIC;
    shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    shdr.sh_addralign = 8;
    shdr.sh_entsize = sizeof(ElfDyn);
  }

  void update_shdr() override;
  void copy_buf() override;
};

class SymtabSection : public OutputChunk {
public:
  SymtabSection() : OutputChunk(SYNTHETIC) {
    name = ".symtab";
    shdr.sh_type = SHT_SYMTAB;
    shdr.sh_entsize = sizeof(ElfSym);
    shdr.sh_addralign = 8;
    shdr.sh_size = sizeof(ElfSym);
  }

  void update_shdr() override;
  void copy_buf() override;
};

class DynsymSection : public OutputChunk {
public:
  DynsymSection() : OutputChunk(SYNTHETIC) {
    name = ".dynsym";
    shdr.sh_type = SHT_DYNSYM;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_entsize = sizeof(ElfSym);
    shdr.sh_addralign = 8;
    shdr.sh_size = sizeof(ElfSym);
    shdr.sh_info = 1;
  }

  void add_symbol(Symbol *sym);
  void update_shdr() override;
  void copy_buf() override;

  std::vector<Symbol *> symbols;
};

class HashSection : public OutputChunk {
public:
  HashSection() : OutputChunk(SYNTHETIC) {
    name = ".hash";
    shdr.sh_type = SHT_HASH;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_entsize = 4;
    shdr.sh_addralign = 4;
  }

  void update_shdr() override;
  void copy_buf() override;

private:
  static u32 hash(std::string_view name);
};

class MergedSection : public OutputChunk {
public:
  static MergedSection *get_instance(std::string_view name, u32 type, u64 flags);

  static inline std::vector<MergedSection *> instances;
  ConcurrentMap<StringPiece> map;

  void copy_buf() override;

private:
  MergedSection(std::string_view name, u64 flags, u32 type)
    : OutputChunk(SYNTHETIC) {
    this->name = name;
    shdr.sh_flags = flags;
    shdr.sh_type = type;
    shdr.sh_addralign = 1;
  }
};

class CopyrelSection : public OutputChunk {
public:
  CopyrelSection() : OutputChunk(SYNTHETIC) {
    name = ".bss";
    shdr.sh_type = SHT_NOBITS;
    shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    shdr.sh_addralign = 32;
  }

  void add_symbol(Symbol *sym);

  std::vector<Symbol *> symbols;
};

class VersymSection : public OutputChunk {
public:
  VersymSection() : OutputChunk(SYNTHETIC) {
    name = ".gnu.version";
    shdr.sh_type = SHT_GNU_VERSYM;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_entsize = 2;
    shdr.sh_addralign = 2;
  }

  void update_shdr() override;
  void copy_buf() override;

  std::vector<u16> contents;
};

class VerneedSection : public OutputChunk {
public:
  VerneedSection() : OutputChunk(SYNTHETIC) {
    name = ".gnu.version_r";
    shdr.sh_type = SHT_GNU_VERNEED;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_addralign = 8;
  }

  void update_shdr() override;
  void copy_buf() override;

  std::vector<u8> contents;
};

bool is_c_identifier(std::string_view name);
std::vector<ElfPhdr> create_phdr();

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

struct MemoryMappedFile {
  MemoryMappedFile(std::string name, u8 *data, u64 size)
    : name(name), data(data), size(size) {}

  std::string name;
  u8 *data;
  u64 size;
};

class InputFile {
public:
  InputFile(MemoryMappedFile mb);

  MemoryMappedFile mb;
  ElfEhdr &ehdr;
  std::span<ElfShdr> elf_sections;
  std::vector<Symbol *> symbols;

  std::string name;
  bool is_dso;
  u32 priority;
  std::atomic_bool is_alive = ATOMIC_VAR_INIT(false);

  std::string_view get_string(const ElfShdr &shdr) const;
  std::string_view get_string(u32 idx) const;

protected:
  template<typename T> std::span<T> get_data(const ElfShdr &shdr) const;
  template<typename T> std::span<T> get_data(u32 idx) const;
  ElfShdr *find_section(u32 type);
};

class ObjectFile : public InputFile {
public:
  ObjectFile(MemoryMappedFile mb, std::string archive_name);

  void parse();
  void initialize_mergeable_sections();
  void resolve_symbols();
  void mark_live_objects(tbb::parallel_do_feeder<ObjectFile *> &feeder);
  void handle_undefined_weak_symbols();
  void resolve_comdat_groups();
  void eliminate_duplicate_comdat_groups();
  void assign_mergeable_string_offsets();
  void convert_common_symbols();
  void compute_symtab();
  void write_symtab();

  static ObjectFile *create_internal_file();

  std::string archive_name;
  std::vector<InputSection *> sections;
  std::span<ElfSym> elf_syms;
  int first_global = 0;
  const bool is_in_archive;
  std::atomic_bool has_error = ATOMIC_VAR_INIT(false);

  u64 num_dynrel = 0;
  u64 reldyn_offset = 0;

  u64 local_symtab_offset = 0;
  u64 local_symtab_size = 0;
  u64 global_symtab_offset = 0;
  u64 global_symtab_size = 0;
  u64 strtab_offset = 0;
  u64 strtab_size = 0;

  std::vector<MergeableSection *> mergeable_sections;

private:
  void initialize_sections();
  void initialize_symbols();
  std::vector<StringPieceRef> read_string_pieces(InputSection *isec);
  void maybe_override_symbol(Symbol &sym, int symidx);

  std::vector<std::pair<ComdatGroup *, std::span<u32>>> comdat_groups;
  std::vector<Symbol> local_symbols;
  std::vector<StringPieceRef> sym_pieces;
  bool has_common_symbol;

  std::string_view symbol_strtab;
  const ElfShdr *symtab_sec;
};

class SharedFile : public InputFile {
public:
  SharedFile(MemoryMappedFile mb, bool as_needed) : InputFile(mb) {
    is_alive = !as_needed;
  }

  void parse();
  void resolve_symbols();
  std::span<Symbol *> find_aliases(Symbol *sym);

  std::string_view soname;

  std::vector<std::string_view> version_strings;

private:
  std::string_view get_soname();
  void maybe_override_symbol(Symbol &sym, const ElfSym &esym);
  std::vector<std::string_view> read_verdef();

  std::vector<const ElfSym *> elf_syms;
  std::vector<u16> versyms;

  std::string_view symbol_strtab;
  const ElfShdr *symtab_sec;
};

//
// archive_file.cc
//

std::vector<MemoryMappedFile> read_archive_members(MemoryMappedFile mb);

//
// linker_script.cc
//

void parse_linker_script(MemoryMappedFile mb);
void parse_version_script(std::string path);

//
// perf.cc
//

class Counter {
public:
  Counter(std::string_view name, u32 value = 0) : name(name), value(value) {
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
  std::string_view name;
  std::atomic_uint32_t value;

  static std::vector<Counter *> instances;
};

class Timer {
public:
  Timer(std::string name);
  void stop();
  static void print();

private:
  static std::vector<Timer *> instances;

  std::string name;
  u64 start;
  u64 end;
  u64 user;
  u64 sys;
  bool stopped = false;
};

class ScopedTimer {
public:
  ScopedTimer(std::string name) {
    timer = new Timer(name);
  }

  ~ScopedTimer() {
    timer->stop();
  }

private:
  Timer *timer;
};

//
// mapfile.cc
//

void print_map();

//
// main.cc
//

MemoryMappedFile find_library(std::string path);
MemoryMappedFile *open_input_file(std::string path);
MemoryMappedFile must_open_input_file(std::string path);
void read_file(MemoryMappedFile mb);

//
// Inline objects and functions
//

namespace out {
inline std::vector<ObjectFile *> objs;
inline std::vector<SharedFile *> dsos;
inline std::vector<OutputChunk *> chunks;
inline u8 *buf;

inline OutputEhdr *ehdr;
inline OutputShdr *shdr;
inline OutputPhdr *phdr;
inline InterpSection *interp;
inline GotSection *got;
inline GotPltSection *gotplt;
inline RelPltSection *relplt;
inline RelDynSection *reldyn;
inline DynamicSection *dynamic;
inline StrtabSection *strtab;
inline DynstrSection *dynstr;
inline HashSection *hash;
inline ShstrtabSection *shstrtab;
inline PltSection *plt;
inline SymtabSection *symtab;
inline DynsymSection *dynsym;
inline CopyrelSection *copyrel;
inline VersymSection *versym;
inline VerneedSection *verneed;

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
inline Symbol *_DYNAMIC;
inline Symbol *_GLOBAL_OFFSET_TABLE_;
inline Symbol *_end;
inline Symbol *_etext;
inline Symbol *_edata;
}

inline void message(std::string msg) {
  static std::mutex mu;
  std::lock_guard lock(mu);
  std::cout << msg << "\n";
}

inline std::string to_string(Symbol sym) {
  return std::string(sym.name) + "(" + to_string(sym.file) + ")";
}

inline u64 align_to(u64 val, u64 align) {
  assert(__builtin_popcount(align) == 1);
  return (val + align - 1) & ~(align - 1);
}

inline u64 Symbol::get_addr() const {
  if (piece_ref.piece)
    return piece_ref.piece->get_addr() + piece_ref.addend;

  if (copyrel_offset != -1)
    return out::copyrel->shdr.sh_addr + copyrel_offset;

  if (input_section) {
    if (!input_section->is_alive) {
      // The control can reach here if there's a relocation that refers
      // a local symbol belonging to a comdat group section. This is a
      // violation of the spec, as all relocations should use only global
      // symbols of comdat members. However, .eh_frame tends to have such
      // relocations.
      return 0;
    }

    return input_section->get_addr() + value;
  }

  if (file && file->is_dso && copyrel_offset == -1)
    return get_plt_addr();

  return value;
}

inline u64 Symbol::get_got_addr() const {
  assert(got_idx != -1);
  return out::got->shdr.sh_addr + got_idx * GOT_SIZE;
}

inline u64 Symbol::get_gotplt_addr() const {
  assert(gotplt_idx != -1);
  return out::gotplt->shdr.sh_addr + gotplt_idx * GOT_SIZE;
}

inline u64 Symbol::get_gottpoff_addr() const {
  assert(gottpoff_idx != -1);
  return out::got->shdr.sh_addr + gottpoff_idx * GOT_SIZE;
}

inline u64 Symbol::get_tlsgd_addr() const {
  assert(tlsgd_idx != -1);
  return out::got->shdr.sh_addr + tlsgd_idx * GOT_SIZE;
}

inline u64 Symbol::get_tlsld_addr() const {
  assert(tlsld_idx != -1);
  return out::got->shdr.sh_addr + tlsld_idx * GOT_SIZE;
}

inline u64 Symbol::get_plt_addr() const {
  assert(plt_idx != -1);
  return out::plt->shdr.sh_addr + plt_idx * PLT_SIZE;
}

inline u64 StringPiece::get_addr() const {
  MergeableSection *is = isec.load();
  return is->parent.shdr.sh_addr + is->offset + output_offset;
}

inline u64 InputChunk::get_addr() const {
  return output_section->shdr.sh_addr + offset;
}

inline u32 elf_hash(std::string_view name) {
  u32 h = 0;
  for (u8 c : name) {
    h = (h << 4) + c;
    u32 g = h & 0xf0000000;
    if (g != 0)
      h ^= g >> 24;
    h &= ~g;
  }
  return h;
}

inline void write_string(u8 *buf, std::string_view str) {
  memcpy(buf, str.data(), str.size());
  buf[str.size()] = '\0';
}

template <typename T>
inline void write_vector(u8 *buf, const std::vector<T> &vec) {
  memcpy(buf, vec.data(), vec.size() * sizeof(T));
}

template <typename T>
inline std::vector<T> flatten(std::vector<std::vector<T>> &vec) {
  std::vector<T> ret;
  for (std::vector<T> &v : vec)
    ret.insert(ret.end(), v.begin(), v.end());
  return ret;
}

template <typename T, typename U>
inline void erase(std::vector<T> &vec, U pred) {
  vec.erase(std::remove_if(vec.begin(), vec.end(), pred), vec.end());
}
