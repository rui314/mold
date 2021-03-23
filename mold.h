#pragma once

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include "elf.h"
#include "xxHash/xxhash.h"

#include <atomic>
#include <cassert>
#include <cstdint>
#include <functional>
#include <iostream>
#include <mutex>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <tbb/concurrent_hash_map.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/spin_mutex.h>
#include <tbb/task_group.h>
#include <unordered_set>
#include <vector>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

static constexpr i64 SECTOR_SIZE = 512;
static constexpr i64 PAGE_SIZE = 4096;
static constexpr i64 GOT_SIZE = 8;
static constexpr i64 PLT_SIZE = 16;
static constexpr i64 PLT_GOT_SIZE = 8;
static constexpr i64 SHA256_SIZE = 32;

class InputFile;
class InputSection;
class MergedSection;
class ObjectFile;
class OutputChunk;
class OutputSection;
class ReadContext;
class SharedFile;
class Symbol;

struct BuildId {
  i64 size() const;

  enum { NONE, HEX, HASH, UUID } kind = NONE;
  std::vector<u8> value;
  i64 hash_size = 0;
};

struct Config {
  BuildId build_id;
  bool Bsymbolic = false;
  bool Bsymbolic_functions = false;
  bool allow_multiple_definition = false;
  bool demangle = true;
  bool discard_all = false;
  bool discard_locals = false;
  bool eh_frame_hdr = true;
  bool export_dynamic = false;
  bool fork = true;
  bool gc_sections = false;
  bool hash_style_gnu = false;
  bool hash_style_sysv = true;
  bool icf = false;
  bool is_static = false;
  bool perf = false;
  bool pic = false;
  bool pie = false;
  bool preload = false;
  bool print_gc_sections = false;
  bool print_icf_sections = false;
  bool print_map = false;
  bool quick_exit = true;
  bool relax = true;
  bool shared = false;
  bool stats = false;
  bool strip_all = false;
  bool strip_debug = false;
  bool trace = false;
  bool z_copyreloc = true;
  bool z_defs = false;
  bool z_delete = true;
  bool z_dlopen = true;
  bool z_execstack = false;
  bool z_now = false;
  bool z_relro = true;
  i16 default_version = VER_NDX_GLOBAL;
  std::vector<std::string_view> version_definitions;
  std::vector<std::pair<std::string_view, i16>> version_patterns;
  i64 filler = -1;
  i64 thread_count = -1;
  std::string Map;
  std::string dynamic_linker;
  std::string entry = "_start";
  std::string fini = "_fini";
  std::string init = "_init";
  std::string output;
  std::string rpaths;
  std::string soname;
  std::string sysroot;
  std::vector<std::string_view> auxiliary;
  std::vector<std::string_view> exclude_libs;
  std::vector<std::string_view> filter;
  std::vector<std::string_view> library_paths;
  std::vector<std::string_view> trace_symbol;
  std::vector<std::string_view> undefined;
  u64 image_base = 0x200000;
};

inline Config config;

void cleanup();

class SyncOut {
public:
  SyncOut(std::ostream &out = std::cout) : out(out) {}

  ~SyncOut() {
    static std::mutex mu;
    std::lock_guard lock(mu);
    out << ss.str() << "\n";
  }

  template <class T> SyncOut &operator<<(T &&val) {
    ss << std::forward<T>(val);
    return *this;
  }

private:
  std::ostream &out;
  std::stringstream ss;
};

class Error {
public:
  Error() {
    has_error = true;
  }

  template <class T> Error &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

  static void checkpoint() {
    if (!has_error)
      return;
    cleanup();
    _exit(1);
  }

private:
  static inline std::atomic_bool has_error = false;
  SyncOut out{std::cerr};
};

class Fatal {
public:
  [[noreturn]] ~Fatal() {
    out.~SyncOut();
    cleanup();
    _exit(1);
  }

  template <class T> Fatal &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

private:
  SyncOut out{std::cerr};
};

#define unreachable() \
  do { Fatal() << "internal error at " << __FILE__ << ":" << __LINE__; } while (0)

std::ostream &operator<<(std::ostream &out, const InputFile &file);

//
// Interned string
//

inline u64 hash_string(std::string_view str) {
  return XXH3_64bits(str.data(), str.size());
}

namespace tbb {
template<> struct tbb_hash_compare<std::string_view> {
  static size_t hash(const std::string_view &k) {
    return hash_string(k);
  }

  static bool equal(const std::string_view &k1, const std::string_view &k2) {
    return k1 == k2;
  }
};
}

template<typename ValueT> class ConcurrentMap {
public:
  ValueT *insert(std::string_view key, const ValueT &val) {
    typename decltype(map)::const_accessor acc;
    map.insert(acc, std::make_pair(key, val));
    return const_cast<ValueT *>(&acc->second);
  }

  tbb::concurrent_hash_map<std::string_view, ValueT> map;
};

//
// Symbol
//

struct SectionFragment {
  SectionFragment(MergedSection *sec, std::string_view data)
    : output_section(*sec), data(data) {}

  SectionFragment(const SectionFragment &other)
    : output_section(other.output_section), data(other.data),
      offset(other.offset), alignment(other.alignment.load()),
      is_alive(other.is_alive.load()) {}

  inline u64 get_addr() const;

  MergedSection &output_section;
  std::string_view data;
  u32 offset = -1;
  std::atomic_uint16_t alignment = 1;
  std::atomic_bool is_alive = false;
};

struct SectionFragmentRef {
  SectionFragment *frag = nullptr;
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
  NEEDS_TLSDESC  = 1 << 7,
};

class Symbol {
public:
  Symbol() = default;
  Symbol(std::string_view name) : name(name) {}
  Symbol(const Symbol &other) : name(other.name) {}

  static Symbol *intern(std::string_view key, std::string_view name) {
    static ConcurrentMap<Symbol> map;
    return map.insert(key, {name});
  }

  static Symbol *intern(std::string_view name) {
    return intern(name, name);
  }

  static Symbol *intern_alloc(std::string name) {
    return intern(*new std::string(name));
  }

  inline u64 get_addr() const;
  inline u64 get_got_addr() const;
  inline u64 get_gotplt_addr() const;
  inline u64 get_gottpoff_addr() const;
  inline u64 get_tlsgd_addr() const;
  inline u64 get_tlsdesc_addr() const;
  inline u64 get_plt_addr() const;

  inline bool is_alive() const;
  inline bool is_absolute() const;
  inline bool is_relative() const { return !is_absolute(); }
  inline bool is_undef() const;
  inline bool is_undef_weak() const;
  inline u32 get_type() const;
  inline std::string_view get_version() const;

  inline void clear();

  std::string_view name;
  InputFile *file = nullptr;
  const ElfSym *esym = nullptr;
  InputSection *input_section = nullptr;
  SectionFragment *frag = nullptr;

  u64 value = -1;
  u32 got_idx = -1;
  u32 gotplt_idx = -1;
  u32 gottpoff_idx = -1;
  u32 tlsgd_idx = -1;
  u32 tlsdesc_idx = -1;
  u32 plt_idx = -1;
  u32 dynsym_idx = -1;
  u16 shndx = 0;
  u16 ver_idx = 0;

  tbb::spin_mutex mu;
  std::atomic_uint8_t visibility = STV_DEFAULT;
  std::atomic_uint8_t flags = 0;

  u8 is_lazy : 1 = false;
  u8 is_weak : 1 = false;
  u8 write_to_symtab : 1 = false;
  u8 traced : 1 = false;
  u8 has_copyrel : 1 = false;
  u8 copyrel_readonly : 1 = false;
  u8 is_imported : 1 = false;
  u8 is_exported : 1 = false;
};

std::ostream &operator<<(std::ostream &out, const Symbol &sym);

//
// input_sections.cc
//

enum RelType : u16 {
  R_NONE = 1,
  R_ABS,
  R_DYN,
  R_BASEREL,
  R_PC,
  R_GOT,
  R_GOTPC,
  R_GOTPCREL,
  R_GOTPCRELX_RELAX,
  R_REX_GOTPCRELX_RELAX,
  R_TLSGD,
  R_TLSGD_RELAX_LE,
  R_TLSLD,
  R_TLSLD_RELAX_LE,
  R_DTPOFF,
  R_DTPOFF_RELAX,
  R_TPOFF,
  R_GOTTPOFF,
  R_GOTTPOFF_RELAX,
  R_GOTPC_TLSDESC,
  R_GOTPC_TLSDESC_RELAX_LE,
  R_SIZE,
  R_TLSDESC_CALL_RELAX,
};

struct EhReloc {
  Symbol &sym;
  u32 type;
  u32 offset;
  i64 addend;
};

inline bool operator==(const EhReloc &a, const EhReloc &b) {
  return std::tuple(&a.sym, a.type, a.offset, a.addend) ==
         std::tuple(&b.sym, b.type, b.offset, b.addend);
}

struct FdeRecord {
  FdeRecord(std::string_view contents, std::vector<EhReloc> &&rels,
            u32 cie_idx)
    : contents(contents), rels(std::move(rels)), cie_idx(cie_idx) {}

  FdeRecord(const FdeRecord &&other)
    : contents(other.contents), rels(std::move(other.rels)),
      cie_idx(other.cie_idx), offset(other.offset),
      is_alive(other.is_alive.load()) {}

  std::string_view contents;
  std::vector<EhReloc> rels;
  u32 cie_idx = -1;
  u32 offset = -1;
  std::atomic_bool is_alive = true;
};

struct CieRecord {
  bool should_merge(const CieRecord &other) const;

  std::string_view contents;
  std::vector<EhReloc> rels;
  std::vector<FdeRecord> fdes;

  // For .eh_frame
  u32 offset = -1;
  u32 leader_offset = -1;
  u32 fde_size = -1;

  // For .eh_frame_hdr
  u32 num_fdes = 0;
  u32 fde_idx = -1;

  // For ICF
  u32 icf_idx = -1;
};

class InputSection {
public:
  InputSection(ObjectFile &file, const ElfShdr &shdr,
               std::string_view name, i64 section_idx);

  void scan_relocations();
  void report_undefined_symbols();
  void copy_buf();
  void apply_reloc_alloc(u8 *base);
  void apply_reloc_nonalloc(u8 *base);
  void kill();

  inline i64 get_priority() const;
  inline u64 get_addr() const;

  ObjectFile &file;
  const ElfShdr &shdr;
  OutputSection *output_section = nullptr;

  std::string_view name;
  std::string_view contents;

  std::span<ElfRela> rels;
  std::vector<bool> has_fragments;
  std::vector<SectionFragmentRef> rel_fragments;
  std::vector<RelType> rel_types;
  std::span<FdeRecord> fdes;

  u32 offset = -1;
  u32 section_idx = -1;
  u32 reldyn_offset = 0;

  bool is_ehframe = false;

  // For COMDAT de-duplication and garbage collection
  std::atomic_bool is_alive = true;

  // For garbage collection
  std::atomic_bool is_visited = false;

  // For ICF
  InputSection *leader = nullptr;
  u32 icf_idx = -1;
  bool icf_eligible = false;
  bool icf_leaf = false;
};

//
// output_chunks.cc
//

bool is_relro(OutputChunk *chunk);

class OutputChunk {
public:
  enum Kind : u8 { HEADER, REGULAR, SYNTHETIC };

  virtual void copy_buf() {}
  virtual void update_shdr() {}

  std::string_view name;
  i64 shndx = 0;
  Kind kind;
  bool new_page = false;
  bool new_page_end = false;
  ElfShdr shdr = { .sh_addralign = 1 };

protected:
  OutputChunk(Kind kind) : kind(kind) {}
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
  static OutputSection *
  get_instance(std::string_view name, u64 type, u64 flags);

  void copy_buf() override;

  static inline std::vector<OutputSection *> instances;

  std::vector<InputSection *> members;
  u32 idx;

private:
  OutputSection(std::string_view name, u32 type, u64 flags);
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
  void add_tlsdesc_symbol(Symbol *sym);
  void add_tlsld();

  u64 get_tlsld_addr() const {
    assert(tlsld_idx != -1);
    return shdr.sh_addr + tlsld_idx * GOT_SIZE;
  }

  i64 get_reldyn_size() const;
  void copy_buf() override;

  std::vector<Symbol *> got_syms;
  std::vector<Symbol *> gottpoff_syms;
  std::vector<Symbol *> tlsgd_syms;
  std::vector<Symbol *> tlsdesc_syms;
  u32 tlsld_idx = -1;
};

class GotPltSection : public OutputChunk {
public:
  GotPltSection() : OutputChunk(SYNTHETIC) {
    name = ".got.plt";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    shdr.sh_addralign = GOT_SIZE;
  }

  void copy_buf() override;
};

class PltSection : public OutputChunk {
public:
  PltSection() : OutputChunk(SYNTHETIC) {
    name = ".plt";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
    shdr.sh_addralign = 16;
  }

  void add_symbol(Symbol *sym);
  void copy_buf() override;

  std::vector<Symbol *> symbols;
};

class PltGotSection : public OutputChunk {
public:
  PltGotSection() : OutputChunk(SYNTHETIC) {
    name = ".plt.got";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
    shdr.sh_addralign = 8;
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
  void sort();
};

class StrtabSection : public OutputChunk {
public:
  StrtabSection() : OutputChunk(SYNTHETIC) {
    name = ".strtab";
    shdr.sh_type = SHT_STRTAB;
    shdr.sh_size = 1;
  }

  void update_shdr() override;
};

class ShstrtabSection : public OutputChunk {
public:
  ShstrtabSection() : OutputChunk(SYNTHETIC) {
    name = ".shstrtab";
    shdr.sh_type = SHT_STRTAB;
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
  }

  i64 add_string(std::string_view str);
  i64 find_string(std::string_view str);
  void update_shdr() override;
  void copy_buf() override;

  i64 dynsym_offset = -1;

private:
  std::unordered_map<std::string_view, i64> strings;
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
  }

  void add_symbol(Symbol *sym);
  void sort_symbols();
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
};

class GnuHashSection : public OutputChunk {
public:
  GnuHashSection() : OutputChunk(SYNTHETIC) {
    name = ".gnu.hash";
    shdr.sh_type = SHT_GNU_HASH;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_addralign = 8;
  }

  void update_shdr() override;
  void copy_buf() override;

  static constexpr i64 LOAD_FACTOR = 8;
  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 BLOOM_SHIFT = 26;
  static constexpr i64 ELFCLASS_BITS = 64;

  u32 num_buckets = -1;
  u32 symoffset = -1;
  u32 num_bloom = 1;
};

class MergedSection : public OutputChunk {
public:
  static MergedSection *
  get_instance(std::string_view name, u64 type, u64 flags);

  SectionFragment *insert(std::string_view data, i64 alignment);
  void assign_offsets();
  void copy_buf() override;

  static inline std::vector<MergedSection *> instances;

private:
  typedef tbb::concurrent_hash_map<std::string_view, SectionFragment> MapTy;
  static constexpr i64 NUM_SHARDS = 64;

  MergedSection(std::string_view name, u64 flags, u32 type)
    : OutputChunk(SYNTHETIC) {
    this->name = name;
    shdr.sh_flags = flags;
    shdr.sh_type = type;
  }

  MapTy maps[NUM_SHARDS];
  i64 shard_offsets[NUM_SHARDS + 1] = {};
  std::atomic_uint16_t max_alignment;
};

class EhFrameSection : public OutputChunk {
public:
  EhFrameSection() : OutputChunk(SYNTHETIC) {
    name = ".eh_frame";
    shdr.sh_type = SHT_X86_64_UNWIND;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_addralign = 8;
  }

  void construct();
  void copy_buf() override;
  u64 get_addr(const Symbol &sym);

  std::vector<CieRecord *> cies;
  u32 num_fdes = 0;
};

class EhFrameHdrSection : public OutputChunk {
public:
  EhFrameHdrSection() : OutputChunk(SYNTHETIC) {
    name = ".eh_frame_hdr";
    shdr.sh_type = SHT_PROGBITS;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_addralign = 4;
    shdr.sh_size = HEADER_SIZE;
  }

  static constexpr i64 HEADER_SIZE = 12;
};

class CopyrelSection : public OutputChunk {
public:
  CopyrelSection(std::string_view name) : OutputChunk(SYNTHETIC) {
    this->name = name;
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

class VerdefSection : public OutputChunk {
public:
  VerdefSection() : OutputChunk(SYNTHETIC) {
    name = ".gnu.version_d";
    shdr.sh_type = SHT_GNU_VERDEF;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_addralign = 8;
  }

  void update_shdr() override;
  void copy_buf() override;

  std::vector<u8> contents;
};

class BuildIdSection : public OutputChunk {
public:
  BuildIdSection() : OutputChunk(SYNTHETIC) {
    name = ".note.gnu.build-id";
    shdr.sh_type = SHT_NOTE;
    shdr.sh_flags = SHF_ALLOC;
    shdr.sh_addralign = 4;
    shdr.sh_size = 1;
  }

  void update_shdr() override;
  void copy_buf() override;
  void write_buildid(i64 filesize);

  static constexpr i64 HEADER_SIZE = 16;
};

bool is_c_identifier(std::string_view name);
std::vector<ElfPhdr> create_phdr();

//
// object_file.cc
//

struct ComdatGroup {
  ComdatGroup() = default;
  ComdatGroup(const ComdatGroup &other)
    : owner(other.owner.load()) {}

  std::atomic_uint32_t owner = -1;
};

class MemoryMappedFile {
public:
  static MemoryMappedFile *open(std::string path);
  static MemoryMappedFile *must_open(std::string path);

  MemoryMappedFile(std::string name, u8 *data, u64 size, u64 mtime = 0)
    : name(name), data_(data), size_(size), mtime(mtime) {}
  MemoryMappedFile() = delete;

  MemoryMappedFile *slice(std::string name, u64 start, u64 size);

  u8 *data();
  i64 size() const { return size_; }

  std::string_view get_contents() {
    return std::string_view((char *)data(), size());
  }

  std::string name;
  i64 mtime = 0;

private:
  std::mutex mu;
  MemoryMappedFile *parent;
  std::atomic<u8 *> data_;
  i64 size_ = 0;
};

class InputFile {
public:
  InputFile(MemoryMappedFile *mb);
  InputFile() : name("<internal>") {}

  std::string_view get_string(const ElfShdr &shdr);
  std::string_view get_string(i64 idx);

  MemoryMappedFile *mb;
  std::span<ElfShdr> elf_sections;
  std::vector<Symbol *> symbols;

  std::string name;
  bool is_dso = false;
  u32 priority;
  std::atomic_bool is_alive = false;

protected:
  template<typename T> std::span<T> get_data(const ElfShdr &shdr);
  template<typename T> std::span<T> get_data(i64 idx);
  ElfShdr *find_section(i64 type);

  std::string_view shstrtab;
};

class ObjectFile : public InputFile {
public:
  ObjectFile(MemoryMappedFile *mb, std::string archive_name, bool is_in_lib);
  ObjectFile();

  void parse();
  void resolve_lazy_symbols();
  void resolve_regular_symbols();
  void mark_live_objects(std::function<void(ObjectFile *)> feeder);
  void convert_undefined_weak_symbols();
  void resolve_comdat_groups();
  void eliminate_duplicate_comdat_groups();
  void claim_unresolved_symbols();
  void scan_relocations();
  void convert_common_symbols();
  void compute_symtab();
  void write_symtab();

  inline i64 get_shndx(const ElfSym &esym);
  inline InputSection *get_section(const ElfSym &esym);
  inline std::span<Symbol *> get_global_syms();

  std::string archive_name;
  std::vector<InputSection *> sections;
  std::span<ElfSym> elf_syms;
  i64 first_global = 0;
  const bool is_in_lib = false;
  std::vector<CieRecord> cies;
  std::vector<const char *> symvers;
  std::vector<SectionFragment *> fragments;
  bool exclude_libs = false;

  u64 num_dynrel = 0;
  u64 reldyn_offset = 0;

  u64 local_symtab_offset = 0;
  u64 global_symtab_offset = 0;
  u64 num_local_symtab = 0;
  u64 num_global_symtab = 0;
  u64 strtab_offset = 0;
  u64 strtab_size = 0;

private:
  void initialize_sections();
  void initialize_symbols();
  void initialize_mergeable_sections();
  void initialize_ehframe_sections();
  void read_ehframe(InputSection &isec);
  void maybe_override_symbol(Symbol &sym, i64 symidx);
  void merge_visibility(Symbol &sym, u8 visibility);

  std::vector<std::pair<ComdatGroup *, std::span<u32>>> comdat_groups;
  std::vector<SectionFragmentRef> sym_fragments;
  bool has_common_symbol;

  std::string_view symbol_strtab;
  const ElfShdr *symtab_sec;
  std::span<u32> symtab_shndx_sec;
};

class SharedFile : public InputFile {
public:
  SharedFile(MemoryMappedFile *mb, bool as_needed) : InputFile(mb) {
    is_alive = !as_needed;
  }

  void parse();
  void resolve_symbols();
  std::vector<Symbol *> find_aliases(Symbol *sym);
  bool is_readonly(Symbol *sym);

  std::string_view soname;
  std::vector<std::string_view> version_strings;
  std::vector<Symbol *> undefs;

private:
  std::string_view get_soname();
  void maybe_override_symbol(Symbol &sym, const ElfSym &esym);
  std::vector<std::string_view> read_verdef();

  std::vector<const ElfSym *> elf_syms;
  std::vector<u16> versyms;
  std::string_view symbol_strtab;
  const ElfShdr *symtab_sec;
};

inline std::ostream &operator<<(std::ostream &out, const InputSection &isec) {
  out << isec.file << ":(" << isec.name << ")";
  return out;
}

//
// archive_file.cc
//

std::vector<MemoryMappedFile *> read_archive_members(MemoryMappedFile *mb);
std::vector<MemoryMappedFile *> read_fat_archive_members(MemoryMappedFile *mb);
std::vector<MemoryMappedFile *> read_thin_archive_members(MemoryMappedFile *mb);

//
// linker_script.cc
//

void parse_linker_script(MemoryMappedFile *mb, ReadContext &ctx);
void parse_version_script(std::string path);
void parse_dynamic_list(std::string path);

//
// output_file.cc
//

class OutputFile {
public:
  static OutputFile *open(std::string path, u64 filesize);
  virtual void close() = 0;

  u8 *buf;
  static inline char *tmpfile;

protected:
  OutputFile(std::string path, u64 filesize) : path(path), filesize(filesize) {}

  std::string path;
  u64 filesize;
};

//
// filepath.cc
//

std::string path_dirname(std::string_view path);
std::string path_basename(std::string_view path);
std::string path_clean(std::string_view path);

//
// glob.cc
//

class GlobPattern {
public:
  GlobPattern(std::string_view pat);
  bool match(std::string_view str) const;

private:
  enum { EXACT, PREFIX, SUFFIX, GENERIC } kind;
  std::string_view pat;
};

//
// perf.cc
//

class Counter {
public:
  Counter(std::string_view name, i64 value = 0) : name(name), values(value) {
    static std::mutex mu;
    std::lock_guard lock(mu);
    instances.push_back(this);
  }

  Counter &operator++(int) {
    if (enabled)
      values.local()++;
    return *this;
  }

  Counter &operator+=(int delta) {
    if (enabled)
      values.local() += delta;
    return *this;
  }

  static void print();

  static inline bool enabled = false;

private:
  i64 get_value();

  std::string_view name;
  tbb::enumerable_thread_specific<i64> values;

  static inline std::vector<Counter *> instances;
};

struct TimerRecord {
  TimerRecord(std::string name);
  void stop();

  std::string name;
  i64 start;
  i64 end;
  i64 user;
  i64 sys;
  bool stopped = false;
};

class Timer {
public:
  Timer(std::string name);
  ~Timer();
  void stop();
  static void print();

private:
  static inline std::vector<TimerRecord *> records;
  TimerRecord *record;
};

//
// gc_sections.cc
//

void gc_sections();

//
// icf.cc
//

void icf_sections();

//
// mapfile.cc
//

void print_map();

//
// subprocess.cc
//

inline char *socket_tmpfile;

std::function<void()> fork_child();
bool resume_daemon(char **argv, i64 *code);
void daemonize(char **argv, std::function<void()> *wait_for_client,
               std::function<void()> *on_complete);

//
// commandline.cc
//

std::vector<std::string_view> expand_response_files(char **argv);
bool read_flag(std::span<std::string_view> &args, std::string name);

bool read_arg(std::span<std::string_view> &args, std::string_view &arg,
              std::string name);

void parse_nonpositional_args(std::span<std::string_view> args,
                              std::vector<std::string_view> &remaining);

//
// main.cc
//

class ReadContext {
public:
  ReadContext(bool x) : is_preloading(x) {}

  std::function<void(InputFile *)> parse;

  bool as_needed = false;
  bool whole_archive = false;
  bool is_preloading = false;
  bool is_static = config.is_static;
  std::unordered_set<std::string_view> visited;
  tbb::task_group tg;
};

MemoryMappedFile *find_library(std::string path,
                               std::span<std::string_view> lib_paths,
                               ReadContext &ctx);

void read_file(MemoryMappedFile *mb, ReadContext &ctx);

//
// Inline objects and functions
//

namespace out {
inline u8 *buf;

inline std::vector<ObjectFile *> objs;
inline std::vector<SharedFile *> dsos;
inline std::vector<OutputChunk *> chunks;
inline std::atomic_bool has_gottpoff = false;
inline std::atomic_bool has_textrel = false;
inline ObjectFile *internal_obj;

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
inline GnuHashSection *gnu_hash;
inline ShstrtabSection *shstrtab;
inline PltSection *plt;
inline PltGotSection *pltgot;
inline SymtabSection *symtab;
inline DynsymSection *dynsym;
inline EhFrameSection *eh_frame;
inline EhFrameHdrSection *eh_frame_hdr;
inline CopyrelSection *copyrel;
inline CopyrelSection *copyrel_relro;
inline VersymSection *versym;
inline VerneedSection *verneed;
inline VerdefSection *verdef;
inline BuildIdSection *buildid;

inline u64 tls_begin;
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
inline Symbol *__GNU_EH_FRAME_HDR;
inline Symbol *_end;
inline Symbol *_etext;
inline Symbol *_edata;
inline Symbol *__executable_start;
}

inline u64 align_to(u64 val, u64 align) {
  if (align == 0)
    return val;
  assert(__builtin_popcount(align) == 1);
  return (val + align - 1) & ~(align - 1);
}

inline u64 next_power_of_two(u64 val) {
  assert(val >> 63 == 0);
  if (val == 0 || val == 1)
    return 1;
  return (u64)1 << (64 - __builtin_clzl(val - 1));
}

inline bool Symbol::is_alive() const {
  if (frag)
    return frag->is_alive;
  if (input_section)
    return input_section->is_alive;
  return true;
}

inline bool Symbol::is_absolute() const {
  if (file == out::internal_obj)
    return false;
  if (file->is_dso)
    return esym->is_abs();
  if (is_imported)
    return false;
  return input_section == nullptr;
}

inline bool Symbol::is_undef() const {
  return esym->is_undef() && esym->st_bind != STB_WEAK;
}

inline bool Symbol::is_undef_weak() const {
  return esym->is_undef() && esym->st_bind == STB_WEAK;
}

inline u32 Symbol::get_type() const {
  if (esym->st_type == STT_GNU_IFUNC && file->is_dso)
    return STT_FUNC;
  return esym->st_type;
}

inline std::string_view Symbol::get_version() const {
  if (file->is_dso)
    return ((SharedFile *)file)->version_strings[ver_idx];
  return "";
}

inline void Symbol::clear() {
  Symbol null;
  memcpy((char *)this, &null, sizeof(null));
}

inline u64 Symbol::get_addr() const {
  if (frag) {
    assert(frag->is_alive);
    return frag->get_addr() + value;
  }

  if (has_copyrel) {
    return copyrel_readonly
      ? out::copyrel_relro->shdr.sh_addr + value
      : out::copyrel->shdr.sh_addr + value;
  }

  if (plt_idx != -1 && esym->st_type == STT_GNU_IFUNC)
    return get_plt_addr();

  if (input_section) {
    if (input_section->is_ehframe)
      return out::eh_frame->get_addr(*this);

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

  if (plt_idx != -1)
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

inline u64 Symbol::get_tlsdesc_addr() const {
  assert(tlsdesc_idx != -1);
  return out::got->shdr.sh_addr + tlsdesc_idx * GOT_SIZE;
}

inline u64 Symbol::get_plt_addr() const {
  assert(plt_idx != -1);

  if (got_idx == -1)
    return out::plt->shdr.sh_addr + plt_idx * PLT_SIZE;
  return out::pltgot->shdr.sh_addr + plt_idx * PLT_GOT_SIZE;
}

inline u64 SectionFragment::get_addr() const {
  return output_section.shdr.sh_addr + offset;
}

inline u64 InputSection::get_addr() const {
  return output_section->shdr.sh_addr + offset;
}

inline i64 InputSection::get_priority() const {
  return ((i64)file.priority << 32) | section_idx;
}

inline i64 ObjectFile::get_shndx(const ElfSym &esym) {
  assert(&elf_syms[0] <= &esym);
  assert(&esym < &elf_syms[elf_syms.size()]);

  if (esym.st_shndx == SHN_XINDEX)
    return symtab_shndx_sec[&esym - &elf_syms[0]];
  return esym.st_shndx;
}

inline InputSection *ObjectFile::get_section(const ElfSym &esym) {
  return sections[get_shndx(esym)];
}

std::span<Symbol *> ObjectFile::get_global_syms() {
  return std::span(symbols).subspan(first_global);
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

inline u32 gnu_hash(std::string_view name) {
  u32 h = 5381;
  for (u8 c : name)
    h = (h << 5) + h + c;
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

template <typename T, typename U>
inline void append(std::vector<T> &vec1, std::vector<U> vec2) {
  vec1.insert(vec1.end(), vec2.begin(), vec2.end());
}

template <typename T>
inline std::vector<T> flatten(std::vector<std::vector<T>> &vec) {
  std::vector<T> ret;
  for (std::vector<T> &v : vec)
    append(ret, v);
  return ret;
}

template <typename T, typename U>
inline void erase(std::vector<T> &vec, U pred) {
  vec.erase(std::remove_if(vec.begin(), vec.end(), pred), vec.end());
}

template <typename T, typename U>
inline void sort(T &vec, U less) {
  std::stable_sort(vec.begin(), vec.end(), less);
}
