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
  StringRef dynamic_linker = "/lib64/ld-linux-x86-64.so.2";
  StringRef output;
  bool is_static = false;
  bool print_map = false;
  int filler = -1;
  std::string sysroot;
  std::vector<StringRef> library_paths;
  u64 image_base = 0x200000;
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

inline void message(const Twine &msg) {
  static std::mutex mu;
  std::lock_guard lock(mu);
  llvm::outs() << msg << "\n";
}

inline std::string toString(const Twine &s) { return s.str(); }

#define CHECK(E, S) check2((E), [&] { return toString(S); })

std::string toString(InputFile *);

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
  i32 addend = 0;
};

class Symbol {
public:
  Symbol(StringRef name, InputFile *file = nullptr)
    : name(name), file(file), is_placeholder(false), is_imported(false),
      is_weak(false), is_undef_weak(false), traced(false) {}

  Symbol(const Symbol &other) : Symbol(other.name, other.file) {}

  static Symbol *intern(StringRef name) {
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

  StringRef name;
  InputFile *file = nullptr;
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
  u32 shndx = 0;
  u16 ver_idx = 0;

  tbb::spin_mutex mu;

  u8 is_placeholder : 1;
  u8 is_imported : 1;
  u8 is_weak : 1;
  u8 is_undef_weak : 1;
  u8 traced : 1;

  enum {
    NEEDS_GOT      = 1 << 0,
    NEEDS_PLT      = 1 << 1,
    NEEDS_GOTTPOFF = 1 << 2,
    NEEDS_TLSGD    = 1 << 3,
    NEEDS_TLSLD    = 1 << 4,
    NEEDS_COPYREL  = 1 << 5,
  };

  std::atomic_uint8_t flags = ATOMIC_VAR_INIT(0);

  u8 visibility = 0;
  u8 type = llvm::ELF::STT_NOTYPE;
  u8 binding = llvm::ELF::STB_GLOBAL;
  const ELF64LE::Sym *esym;
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

  virtual void copy_buf() {}
  u64 get_addr() const;

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

  void copy_buf() override;
  void scan_relocations();
  void report_undefined_symbols();

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

  virtual void initialize_buf() {}
  virtual void copy_buf() {}
  virtual void update_shdr() {}

  StringRef name;
  Kind kind;
  int shndx = 0;
  bool starts_new_ptload = false;
  ELF64LE::Shdr shdr = {};
};

// ELF header
class OutputEhdr : public OutputChunk {
public:
  OutputEhdr() : OutputChunk(HEADER) {
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_size = sizeof(ELF64LE::Ehdr);
  }

  void copy_buf() override;
};

// Section header
class OutputShdr : public OutputChunk {
public:
  OutputShdr() : OutputChunk(HEADER) {
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
  }

  void update_shdr() override;
  void copy_buf() override;
};

// Program header
class OutputPhdr : public OutputChunk {
public:
  OutputPhdr() : OutputChunk(HEADER) {
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class InterpSection : public OutputChunk {
public:
  InterpSection() : OutputChunk(SYNTHETIC) {
    name = ".interp";
    shdr.sh_type = llvm::ELF::SHT_PROGBITS;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_size = config.dynamic_linker.size() + 1;
  }

  void copy_buf() override;
};

// Sections
class OutputSection : public OutputChunk {
public:
  static OutputSection *get_instance(StringRef name, u64 flags, u32 type);

  OutputSection(StringRef name, u32 type, u64 flags)
    : OutputChunk(REGULAR) {
    this->name = name;
    shdr.sh_type = type;
    shdr.sh_flags = flags;
    idx = instances.size();
    instances.push_back(this);
  }

  void copy_buf() override;
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
    shdr.sh_type = type;
    shdr.sh_flags = flags;
    shdr.sh_addralign = align;
    shdr.sh_entsize = entsize;
  }
};

class GotSection : public OutputChunk {
public:
  GotSection() : OutputChunk(SYNTHETIC) {
    name = ".got";
    shdr.sh_type = llvm::ELF::SHT_PROGBITS;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC | llvm::ELF::SHF_WRITE;
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
    shdr.sh_type = llvm::ELF::SHT_PROGBITS;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC | llvm::ELF::SHF_WRITE;
    shdr.sh_addralign = GOT_SIZE;
    shdr.sh_size = GOT_SIZE * INIT_SIZE;
  }

  enum { INIT_SIZE = 3 };

  void copy_buf() override;
};

class PltSection : public OutputChunk {
public:
  PltSection() : OutputChunk(SYNTHETIC) {
    name = ".plt";
    shdr.sh_type = llvm::ELF::SHT_PROGBITS;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC | llvm::ELF::SHF_EXECINSTR;
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
    shdr.sh_type = llvm::ELF::SHT_RELA;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_entsize = sizeof(ELF64LE::Rela);
    shdr.sh_addralign = 8;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class RelDynSection : public OutputChunk {
public:
  RelDynSection() : OutputChunk(SYNTHETIC) {
    name = ".rela.dyn";
    shdr.sh_type = llvm::ELF::SHT_RELA;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_entsize = sizeof(ELF64LE::Rela);
    shdr.sh_addralign = 8;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class StrtabSection : public OutputChunk {
public:
  StrtabSection() : OutputChunk(SYNTHETIC) {
    name = ".strtab";
    shdr.sh_type = llvm::ELF::SHT_STRTAB;
    shdr.sh_addralign = 1;
    shdr.sh_size = 1;
  }

  void initialize_buf() override;
};

class ShstrtabSection : public OutputChunk {
public:
  ShstrtabSection() : OutputChunk(SYNTHETIC) {
    name = ".shstrtab";
    shdr.sh_type = llvm::ELF::SHT_STRTAB;
    shdr.sh_addralign = 1;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class DynstrSection : public OutputChunk {
public:
DynstrSection() : OutputChunk(SYNTHETIC) {
    name = ".dynstr";
    shdr.sh_type = llvm::ELF::SHT_STRTAB;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_size = 1;
    shdr.sh_addralign = 1;
  }

  u32 add_string(StringRef str);
  void copy_buf() override;

private:
  std::vector<StringRef> contents;
};

class DynamicSection : public OutputChunk {
public:
  DynamicSection() : OutputChunk(SYNTHETIC) {
    name = ".dynamic";
    shdr.sh_type = llvm::ELF::SHT_DYNAMIC;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC | llvm::ELF::SHF_WRITE;
    shdr.sh_addralign = 8;
    shdr.sh_entsize = sizeof(ELF64LE::Dyn);
  }

  void update_shdr() override;
  void copy_buf() override;
};

class SymtabSection : public OutputChunk {
public:
  SymtabSection() : OutputChunk(SYNTHETIC) {
    name = ".symtab";
    shdr.sh_type = llvm::ELF::SHT_SYMTAB;
    shdr.sh_entsize = sizeof(ELF64LE::Sym);
    shdr.sh_addralign = 8;
    shdr.sh_size = sizeof(ELF64LE::Sym);
  }

  void update_shdr() override;
  void copy_buf() override;

private:
  std::vector<u64> local_symtab_off;
  std::vector<u64> local_strtab_off;
  std::vector<u64> global_symtab_off;
  std::vector<u64> global_strtab_off;
};

class DynsymSection : public OutputChunk {
public:
  DynsymSection() : OutputChunk(SYNTHETIC) {
    name = ".dynsym";
    shdr.sh_type = llvm::ELF::SHT_DYNSYM;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_entsize = sizeof(ELF64LE::Sym);
    shdr.sh_addralign = 8;
    shdr.sh_size = sizeof(ELF64LE::Sym);
    shdr.sh_info = 1;
  }

  void add_symbol(Symbol *sym);
  void update_shdr() override;
  void initialize_buf() override;
  void copy_buf() override;

  std::vector<Symbol *> symbols;
};

class HashSection : public OutputChunk {
public:
  HashSection() : OutputChunk(SYNTHETIC) {
    name = ".hash";
    shdr.sh_type = llvm::ELF::SHT_HASH;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_entsize = 4;
    shdr.sh_addralign = 4;
  }

  void update_shdr() override;
  void copy_buf() override;

private:
  static u32 hash(StringRef name);
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

class CopyrelSection : public OutputChunk {
public:
  CopyrelSection() : OutputChunk(SYNTHETIC) {
    name = ".bss";
    shdr.sh_type = llvm::ELF::SHT_NOBITS;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC | llvm::ELF::SHF_WRITE;
    shdr.sh_addralign = 32;
  }

  void add_symbol(Symbol *sym);

  std::vector<Symbol *> symbols;
};

class VersymSection : public OutputChunk {
public:
  VersymSection() : OutputChunk(SYNTHETIC) {
    name = ".gnu.version";
    shdr.sh_type = llvm::ELF::SHT_GNU_versym;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_addralign = 2;
  }

  void update_shdr() override;
  void copy_buf() override;
};

class VerneedSection : public OutputChunk {
public:
  VerneedSection() : OutputChunk(SYNTHETIC) {
    name = ".gnu.version_r";
    shdr.sh_type = llvm::ELF::SHT_GNU_verneed;
    shdr.sh_flags = llvm::ELF::SHF_ALLOC;
    shdr.sh_addralign = 4;
  }

  void update_shdr() override;
  void copy_buf() override;
};

bool is_c_identifier(StringRef name);

namespace out {
using namespace llvm::ELF;

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

class InputFile {
public:
  InputFile(MemoryBufferRef mb, bool is_dso)
    : mb(mb), name(mb.getBufferIdentifier()), is_dso(is_dso),
      obj(check(ELFFile<ELF64LE>::create(mb.getBuffer()))) {}

  std::string name;
  bool is_dso;
  u32 priority;
  MemoryBufferRef mb;
  ELFFile<ELF64LE> obj;
  std::vector<Symbol *> symbols;
  std::atomic_bool is_alive = ATOMIC_VAR_INIT(false);
};

class ObjectFile : public InputFile {
public:
  ObjectFile(MemoryBufferRef mb, StringRef archive_name);

  void parse();
  void initialize_mergeable_sections();
  void resolve_symbols();
  void mark_live_archive_members(tbb::parallel_do_feeder<ObjectFile *> &feeder);
  void handle_undefined_weak_symbols();
  void resolve_comdat_groups();
  void eliminate_duplicate_comdat_groups();
  void assign_mergeable_string_offsets();
  void convert_common_symbols();
  void compute_symtab();

  void write_local_symtab(u64 symtab_off, u64 strtab_off);
  void write_global_symtab(u64 symtab_off, u64 strtab_off);

  static ObjectFile *create_internal_file();

  StringRef archive_name;
  std::vector<InputSection *> sections;
  ArrayRef<ELF64LE::Sym> elf_syms;
  int first_global = 0;
  const bool is_in_archive;
  std::atomic_bool has_error = ATOMIC_VAR_INIT(false);

  u64 local_symtab_size = 0;
  u64 local_strtab_size = 0;
  u64 global_symtab_size = 0;
  u64 global_strtab_size = 0;

  std::vector<MergeableSection> mergeable_sections;

private:
  void initialize_sections();
  void initialize_symbols();
  std::vector<StringPieceRef> read_string_pieces(InputSection *isec);

  void maybe_override_symbol(Symbol &sym, int symidx);
  void write_symtab(u64 symtab_off, u64 strtab_off, u32 start, u32 end);

  std::vector<std::pair<ComdatGroup *, ArrayRef<ELF64LE::Word>>> comdat_groups;

  std::vector<Symbol> local_symbols;
  std::vector<StringPieceRef> sym_pieces;
  bool has_common_symbol;

  ArrayRef<ELF64LE::Shdr> elf_sections;
  StringRef symbol_strtab;
  const ELF64LE::Shdr *symtab_sec;
};

class SharedFile : public InputFile {
public:
  SharedFile(MemoryBufferRef mb) : InputFile(mb, true) {}

  void parse();
  void resolve_symbols();
  ArrayRef<Symbol *> find_aliases(Symbol *sym);

  StringRef soname;
  u32 soname_dynstr_idx = -1;

  std::vector<StringRef> version_strings;

private:
  StringRef get_soname(ArrayRef<ELF64LE::Shdr> elf_sections);
  void maybe_override_symbol(Symbol &sym, const ELF64LE::Sym &esym);
  std::vector<StringRef> read_verdef();

  std::vector<const ELF64LE::Sym *> elf_syms;
  std::vector<u16> versyms;

  StringRef symbol_strtab;
  const ELF64LE::Shdr *symtab_sec;
};

inline u64 Symbol::get_addr() const {
  if (piece_ref.piece)
    return piece_ref.piece->get_addr() + piece_ref.addend;
  if (copyrel_offset != -1)
    return out::copyrel->shdr.sh_addr + copyrel_offset;
  if (input_section)
    return input_section->get_addr() + value;
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

inline void write_string(u8 *buf, StringRef str) {
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

//
// linker_script.cc
//

void parse_linker_script(StringRef path, StringRef input);

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

void print_map();

//
// main.cc
//

MemoryBufferRef find_library(const Twine &path);
MemoryBufferRef *open_input_file(const Twine &path);
MemoryBufferRef must_open_input_file(const Twine &path);
void read_file(MemoryBufferRef mb);
