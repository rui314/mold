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

static constexpr i32 SECTOR_SIZE = 512;
static constexpr i32 PAGE_SIZE = 4096;
static constexpr i32 GOT_SIZE = 8;
static constexpr i32 PLT_SIZE = 16;
static constexpr i32 PLT_GOT_SIZE = 8;
static constexpr i32 SHA256_SIZE = 32;

template <typename E> class InputFile;
template <typename E> class InputSection;
template <typename E> class MergedSection;
template <typename E> class ObjectFile;
template <typename E> class OutputChunk;
template <typename E> class OutputSection;
template <typename E> class SharedFile;
template <typename E> class Symbol;

template <typename E> struct Context;

template <typename E> void cleanup();

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

template <typename E>
struct SectionFragment {
  SectionFragment(MergedSection<E> *sec, std::string_view data)
    : output_section(*sec), data(data) {}

  SectionFragment(const SectionFragment &other)
    : output_section(other.output_section), data(other.data),
      offset(other.offset), alignment(other.alignment.load()),
      is_alive(other.is_alive.load()) {}

  inline u64 get_addr(Context<E> &ctx) const;

  MergedSection<E> &output_section;
  std::string_view data;
  u32 offset = -1;
  std::atomic_uint16_t alignment = 1;
  std::atomic_bool is_alive = false;
};

template <typename E>
struct SectionFragmentRef {
  SectionFragment<E> *frag = nullptr;
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

template <typename E>
class Symbol {
public:
  Symbol() = default;
  Symbol(std::string_view name) : name(name) {}
  Symbol(const Symbol<E> &other) : name(other.name) {}

  static Symbol<E> *intern(Context<E> &ctx, std::string_view key,
                           std::string_view name);

  static Symbol<E> *intern(Context<E> &ctx, std::string_view name) {
    return intern(ctx, name, name);
  }

  inline u64 get_addr(Context<E> &ctx) const;
  inline u64 get_got_addr(Context<E> &ctx) const;
  inline u64 get_gotplt_addr(Context<E> &ctx) const;
  inline u64 get_gottpoff_addr(Context<E> &ctx) const;
  inline u64 get_tlsgd_addr(Context<E> &ctx) const;
  inline u64 get_tlsdesc_addr(Context<E> &ctx) const;
  inline u64 get_plt_addr(Context<E> &ctx) const;

  inline bool is_alive() const;
  inline bool is_absolute(Context<E> &ctx) const;
  inline bool is_relative(Context<E> &ctx) const;
  inline bool is_undef() const;
  inline bool is_undef_weak() const;
  inline u32 get_type() const;
  inline std::string_view get_version() const;
  std::string_view get_demangled_name() const;

  inline void clear();

  std::string_view name;
  InputFile<E> *file = nullptr;
  const ElfSym<E> *esym = nullptr;
  InputSection<E> *input_section = nullptr;
  SectionFragment<E> *frag = nullptr;

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

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym);

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

template <typename E>
struct EhReloc {
  Symbol<E> &sym;
  u32 type;
  u32 offset;
  i64 addend;
};

template <typename E>
inline bool operator==(const EhReloc<E> &a, const EhReloc<E> &b) {
  return std::tuple(&a.sym, a.type, a.offset, a.addend) ==
         std::tuple(&b.sym, b.type, b.offset, b.addend);
}

template <typename E>
struct FdeRecord {
  FdeRecord(std::string_view contents, std::vector<EhReloc<E>> &&rels,
            u32 cie_idx)
    : contents(contents), rels(std::move(rels)), cie_idx(cie_idx) {}

  FdeRecord(const FdeRecord &&other)
    : contents(other.contents), rels(std::move(other.rels)),
      cie_idx(other.cie_idx), offset(other.offset),
      is_alive(other.is_alive.load()) {}

  std::string_view contents;
  std::vector<EhReloc<E>> rels;
  u32 cie_idx = -1;
  u32 offset = -1;
  std::atomic_bool is_alive = true;
};

template <typename E>
struct CieRecord {
  bool should_merge(const CieRecord &other) const;

  std::string_view contents;
  std::vector<EhReloc<E>> rels;
  std::vector<FdeRecord<E>> fdes;

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

template <typename E>
class InputSection {
public:
  InputSection(Context<E> &ctx, ObjectFile<E> &file, const ElfShdr<E> &shdr,
               std::string_view name, i64 section_idx);

  void scan_relocations(Context<E> &ctx);
  void report_undefined_symbols();
  void copy_buf(Context<E> &ctx);
  void apply_reloc_alloc(Context<E> &ctx, u8 *base);
  void apply_reloc_nonalloc(Context<E> &ctx, u8 *base);
  void kill();

  inline i64 get_priority() const;
  inline u64 get_addr() const;
  inline i64 get_addend(const ElfRel<E> &rel) const;

  ObjectFile<E> &file;
  const ElfShdr<E> &shdr;
  OutputSection<E> *output_section = nullptr;

  std::string_view name;
  std::string_view contents;

  std::span<ElfRel<E>> rels;
  std::vector<bool> has_fragments;
  std::vector<SectionFragmentRef<E>> rel_fragments;
  std::vector<RelType> rel_types;
  std::span<FdeRecord<E>> fdes;

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

private:
  typedef enum { NONE, ERROR, COPYREL, PLT, DYNREL, BASEREL } Action;

  void dispatch(Context<E> &ctx, Action table[3][4], RelType rel_type, i64 i);
};

//
// output_chunks.cc
//

template <typename E>
bool is_relro(Context<E> &ctx, OutputChunk<E> *chunk);

template <typename E>
class OutputChunk {
public:
  enum Kind : u8 { HEADER, REGULAR, SYNTHETIC };

  virtual void copy_buf(Context<E> &ctx) {}
  virtual void update_shdr(Context<E> &ctx) {}

  std::string_view name;
  i64 shndx = 0;
  Kind kind;
  bool new_page = false;
  bool new_page_end = false;
  ElfShdr<E> shdr = {};

protected:
  OutputChunk(Kind kind) : kind(kind) {
    shdr.sh_addralign = 1;
  }
};

// ELF header
template <typename E>
class OutputEhdr : public OutputChunk<E> {
public:
  OutputEhdr() : OutputChunk<E>(this->HEADER) {
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_size = sizeof(ElfEhdr<E>);
  }

  void copy_buf(Context<E> &ctx) override;
};

// Section header
template <typename E>
class OutputShdr : public OutputChunk<E> {
public:
  OutputShdr() : OutputChunk<E>(this->HEADER) {
    this->shdr.sh_flags = SHF_ALLOC;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

// Program header
template <typename E>
class OutputPhdr : public OutputChunk<E> {
public:
  OutputPhdr() : OutputChunk<E>(this->HEADER) {
    this->shdr.sh_flags = SHF_ALLOC;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class InterpSection : public OutputChunk<E> {
public:
  InterpSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".interp";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

// Sections
template <typename E>
class OutputSection : public OutputChunk<E> {
public:
  static OutputSection *
  get_instance(std::string_view name, u64 type, u64 flags);

  void copy_buf(Context<E> &ctx) override;

  static inline std::vector<OutputSection *> instances;

  std::vector<InputSection<E> *> members;
  u32 idx;

private:
  OutputSection(std::string_view name, u32 type, u64 flags);
};

template <typename E>
class GotSection : public OutputChunk<E> {
public:
  GotSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".got";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = GOT_SIZE;
  }

  void add_got_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_gottpoff_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsgd_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsld(Context<E> &ctx);

  u64 get_tlsld_addr(Context<E> &ctx) const {
    assert(tlsld_idx != -1);
    return this->shdr.sh_addr + tlsld_idx * GOT_SIZE;
  }

  i64 get_reldyn_size(Context<E> &ctx) const;
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> got_syms;
  std::vector<Symbol<E> *> gottpoff_syms;
  std::vector<Symbol<E> *> tlsgd_syms;
  std::vector<Symbol<E> *> tlsdesc_syms;
  u32 tlsld_idx = -1;
};

template <typename E>
class GotPltSection : public OutputChunk<E> {
public:
  GotPltSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".got.plt";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = GOT_SIZE;
  }

  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class PltSection : public OutputChunk<E> {
public:
  PltSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".plt";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
    this->shdr.sh_addralign = 16;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class PltGotSection : public OutputChunk<E> {
public:
  PltGotSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".plt.got";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
    this->shdr.sh_addralign = 8;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class RelPltSection : public OutputChunk<E> {
public:
  RelPltSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = E::is_rela ? ".rela.plt" : ".rel.plt";
    this->shdr.sh_type = E::is_rela ? SHT_RELA : SHT_REL;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = 8;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class RelDynSection : public OutputChunk<E> {
public:
  RelDynSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = E::is_rela ? ".rela.dyn" : ".rel.dyn";
    this->shdr.sh_type = E::is_rela ? SHT_RELA : SHT_REL;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = 8;
  }

  void update_shdr(Context<E> &ctx) override;
  void sort(Context<E> &ctx);
};

template <typename E>
class StrtabSection : public OutputChunk<E> {
public:
  StrtabSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".strtab";
    this->shdr.sh_type = SHT_STRTAB;
    this->shdr.sh_size = 1;
  }

  void update_shdr(Context<E> &ctx) override;
};

template <typename E>
class ShstrtabSection : public OutputChunk<E> {
public:
  ShstrtabSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".shstrtab";
    this->shdr.sh_type = SHT_STRTAB;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class DynstrSection : public OutputChunk<E> {
public:
DynstrSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".dynstr";
    this->shdr.sh_type = SHT_STRTAB;
    this->shdr.sh_flags = SHF_ALLOC;
  }

  i64 add_string(std::string_view str);
  i64 find_string(std::string_view str);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  i64 dynsym_offset = -1;

private:
  std::unordered_map<std::string_view, i64> strings;
};

template <typename E>
class DynamicSection : public OutputChunk<E> {
public:
  DynamicSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".dynamic";
    this->shdr.sh_type = SHT_DYNAMIC;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = 8;
    this->shdr.sh_entsize = sizeof(ElfDyn<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class SymtabSection : public OutputChunk<E> {
public:
  SymtabSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".symtab";
    this->shdr.sh_type = SHT_SYMTAB;
    this->shdr.sh_entsize = sizeof(ElfSym<E>);
    this->shdr.sh_addralign = 8;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class DynsymSection : public OutputChunk<E> {
public:
  DynsymSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".dynsym";
    this->shdr.sh_type = SHT_DYNSYM;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfSym<E>);
    this->shdr.sh_addralign = 8;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void sort_symbols(Context<E> &ctx);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class HashSection : public OutputChunk<E> {
public:
  HashSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".hash";
    this->shdr.sh_type = SHT_HASH;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = 4;
    this->shdr.sh_addralign = 4;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class GnuHashSection : public OutputChunk<E> {
public:
  GnuHashSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".gnu.hash";
    this->shdr.sh_type = SHT_GNU_HASH;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 8;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  static constexpr i64 LOAD_FACTOR = 8;
  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 BLOOM_SHIFT = 26;
  static constexpr i64 ELFCLASS_BITS = 64;

  u32 num_buckets = -1;
  u32 symoffset = -1;
  u32 num_bloom = 1;
};

template <typename E>
class MergedSection : public OutputChunk<E> {
public:
  static MergedSection<E> *
  get_instance(std::string_view name, u64 type, u64 flags);

  SectionFragment<E> *insert(std::string_view data, i64 alignment);
  void assign_offsets();
  void copy_buf(Context<E> &ctx) override;

  static inline std::vector<MergedSection<E> *> instances;

private:
  using MapTy =
    tbb::concurrent_hash_map<std::string_view, SectionFragment<E>>;

  static constexpr i64 NUM_SHARDS = 64;

  MergedSection(std::string_view name, u64 flags, u32 type)
    : OutputChunk<E>(this->SYNTHETIC) {
    this->name = name;
    this->shdr.sh_flags = flags;
    this->shdr.sh_type = type;
  }

  MapTy maps[NUM_SHARDS];
  i64 shard_offsets[NUM_SHARDS + 1] = {};
  std::atomic_uint16_t max_alignment;
};

template <typename E>
class EhFrameSection : public OutputChunk<E> {
public:
  EhFrameSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".eh_frame";
    this->shdr.sh_type = SHT_X86_64_UNWIND;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 8;
  }

  void construct(Context<E> &ctx);
  void copy_buf(Context<E> &ctx) override;
  u64 get_addr(Context<E> &ctx, const Symbol<E> &sym);

  std::vector<CieRecord<E> *> cies;
  u32 num_fdes = 0;
};

template <typename E>
class EhFrameHdrSection : public OutputChunk<E> {
public:
  EhFrameHdrSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".eh_frame_hdr";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 4;
    this->shdr.sh_size = HEADER_SIZE;
  }

  static constexpr i64 HEADER_SIZE = 12;
};

template <typename E>
class DynbssSection : public OutputChunk<E> {
public:
  DynbssSection(bool is_relro) : OutputChunk<E>(this->SYNTHETIC) {
    this->name = is_relro ? ".dynbss.rel.ro" : ".dynbss";
    this->shdr.sh_type = SHT_NOBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = 64;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);

  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class VersymSection : public OutputChunk<E> {
public:
  VersymSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".gnu.version";
    this->shdr.sh_type = SHT_GNU_VERSYM;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = 2;
    this->shdr.sh_addralign = 2;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u16> contents;
};

template <typename E>
class VerneedSection : public OutputChunk<E> {
public:
  VerneedSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".gnu.version_r";
    this->shdr.sh_type = SHT_GNU_VERNEED;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 8;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

template <typename E>
class VerdefSection : public OutputChunk<E> {
public:
  VerdefSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".gnu.version_d";
    this->shdr.sh_type = SHT_GNU_VERDEF;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 8;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

template <typename E>
class BuildIdSection : public OutputChunk<E> {
public:
  BuildIdSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".note.gnu.build-id";
    this->shdr.sh_type = SHT_NOTE;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 4;
    this->shdr.sh_size = 1;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
  void write_buildid(Context<E> &ctx, i64 filesize);

  static constexpr i64 HEADER_SIZE = 16;
};

bool is_c_identifier(std::string_view name);

template <typename E>
std::vector<ElfPhdr<E>> create_phdr(Context<E> &ctx);

//
// object_file.cc
//

struct ComdatGroup {
  ComdatGroup() = default;
  ComdatGroup(const ComdatGroup &other)
    : owner(other.owner.load()) {}

  std::atomic_uint32_t owner = -1;
};

template <typename E>
class MemoryMappedFile {
public:
  static MemoryMappedFile *open(std::string path);
  static MemoryMappedFile *must_open(Context<E> &ctx, std::string path);

  MemoryMappedFile(std::string name, u8 *data, u64 size, u64 mtime = 0)
    : name(name), data_(data), size_(size), mtime(mtime) {}
  MemoryMappedFile() = delete;

  MemoryMappedFile *slice(std::string name, u64 start, u64 size);

  u8 *data(Context<E> &ctx);
  i64 size() const { return size_; }

  std::string_view get_contents(Context<E> &ctx) {
    return std::string_view((char *)data(ctx), size());
  }

  std::string name;
  i64 mtime = 0;

private:
  std::mutex mu;
  MemoryMappedFile *parent;
  std::atomic<u8 *> data_;
  i64 size_ = 0;
};

template <typename E>
class InputFile {
public:
  InputFile(Context<E> &ctx, MemoryMappedFile<E> *mb);
  InputFile() : name("<internal>") {}

  std::string_view get_string(Context<E> &ctx, const ElfShdr<E> &shdr);
  std::string_view get_string(Context<E> &ctx, i64 idx);

  MemoryMappedFile<E> *mb;
  std::span<ElfShdr<E>> elf_sections;
  std::vector<Symbol<E> *> symbols;

  std::string name;
  bool is_dso = false;
  u32 priority;
  std::atomic_bool is_alive = false;

protected:
  template<typename T> std::span<T> get_data(Context<E> &ctx, const ElfShdr<E> &shdr);
  template<typename T> std::span<T> get_data(Context<E> &ctx, i64 idx);
  ElfShdr<E> *find_section(i64 type);

  std::string_view shstrtab;
};

template <typename E>
class ObjectFile : public InputFile<E> {
public:
  ObjectFile(Context<E> &ctx, MemoryMappedFile<E> *mb,
             std::string archive_name, bool is_in_lib);

  ObjectFile(Context<E> &ctx);

  void parse(Context<E> &ctx);
  void resolve_lazy_symbols(Context<E> &ctx);
  void resolve_regular_symbols(Context<E> &ctx);
  void mark_live_objects(Context<E> &ctx,
                         std::function<void(ObjectFile<E> *)> feeder);
  void convert_undefined_weak_symbols(Context<E> &ctx);
  void resolve_comdat_groups();
  void eliminate_duplicate_comdat_groups();
  void claim_unresolved_symbols();
  void scan_relocations(Context<E> &ctx);
  void convert_common_symbols(Context<E> &ctx);
  void compute_symtab(Context<E> &ctx);
  void write_symtab(Context<E> &ctx);

  inline i64 get_shndx(const ElfSym<E> &esym);
  inline InputSection<E> *get_section(const ElfSym<E> &esym);
  inline std::span<Symbol<E> *> get_global_syms();

  std::string archive_name;
  std::vector<InputSection<E> *> sections;
  std::span<ElfSym<E>> elf_syms;
  i64 first_global = 0;
  const bool is_in_lib = false;
  std::vector<CieRecord<E>> cies;
  std::vector<const char *> symvers;
  std::vector<SectionFragment<E> *> fragments;
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
  void initialize_sections(Context<E> &ctx);
  void initialize_symbols(Context<E> &ctx);
  void initialize_mergeable_sections(Context<E> &ctx);
  void initialize_ehframe_sections(Context<E> &ctx);
  void read_ehframe(Context<E> &ctx, InputSection<E> &isec);
  void maybe_override_symbol(Context<E> &ctx, Symbol<E> &sym, i64 symidx);
  void merge_visibility(Context<E> &ctx, Symbol<E> &sym, u8 visibility);

  std::vector<std::pair<ComdatGroup *, std::span<u32>>> comdat_groups;
  std::vector<SectionFragmentRef<E>> sym_fragments;
  bool has_common_symbol;

  std::string_view symbol_strtab;
  const ElfShdr<E> *symtab_sec;
  std::span<u32> symtab_shndx_sec;
};

template <typename E>
class SharedFile : public InputFile<E> {
public:
  SharedFile(Context<E> &ctx, MemoryMappedFile<E> *mb);
  void parse(Context<E> &ctx);
  void resolve_symbols(Context<E> &ctx);
  std::vector<Symbol<E> *> find_aliases(Symbol<E> *sym);
  bool is_readonly(Context<E> &ctx, Symbol<E> *sym);

  std::string_view soname;
  std::vector<std::string_view> version_strings;
  std::vector<Symbol<E> *> undefs;

private:
  std::string_view get_soname(Context<E> &ctx);
  void maybe_override_symbol(Symbol<E> &sym, const ElfSym<E> &esym);
  std::vector<std::string_view> read_verdef(Context<E> &ctx);

  std::vector<const ElfSym<E> *> elf_syms;
  std::vector<u16> versyms;
  std::string_view symbol_strtab;
  const ElfShdr<E> *symtab_sec;
};

//
// archive_file.cc
//

template <typename E>
std::vector<MemoryMappedFile<E> *>
read_fat_archive_members(Context<E> &ctx, MemoryMappedFile<E> *mb);

template <typename E>
std::vector<MemoryMappedFile<E> *>
read_thin_archive_members(Context<E> &ctx, MemoryMappedFile<E> *mb);

//
// linker_script.cc
//

template <typename E>
void parse_linker_script(Context<E> &ctx, MemoryMappedFile<E> *mb);

template <typename E>
void parse_version_script(Context<E> &ctx, std::string path);

template <typename E>
void parse_dynamic_list(Context<E> &ctx, std::string path);

//
// output_file.cc
//

template <typename E>
class OutputFile {
public:
  static OutputFile *open(Context<E> &ctx, std::string path, u64 filesize);
  virtual void close(Context<E> &ctx) = 0;

  u8 *buf;
  static inline char *tmpfile;

protected:
  OutputFile(std::string path, u64 filesize)
    : path(path), filesize(filesize) {}

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

template <typename E>
void gc_sections(Context<E> &ctx);

//
// icf.cc
//

template <typename E>
void icf_sections(Context<E> &ctx);

//
// mapfile.cc
//

template <typename E>
void print_map(Context<E> &ctx);

//
// subprocess.cc
//

inline char *socket_tmpfile;

std::function<void()> fork_child();

template <typename E>
bool resume_daemon(Context<E> &ctx, char **argv, i64 *code);

template <typename E>
void daemonize(Context<E> &ctx, char **argv,
               std::function<void()> *wait_for_client,
               std::function<void()> *on_complete);

template <typename E>
[[noreturn]]
void process_run_subcommand(Context<E> &ctx, int argc, char **argv);

// commandline.cc
//

template <typename E>
std::vector<std::string_view>
expand_response_files(Context<E> &ctx, char **argv);

bool read_flag(std::span<std::string_view> &args, std::string name);

template <typename E>
bool read_arg(Context<E> &ctx, std::span<std::string_view> &args,
              std::string_view &arg,
              std::string name);

template <typename E>
void parse_nonpositional_args(Context<E> &ctx,
                              std::vector<std::string_view> &remaining);

//
// passes.cc
//

template <typename E> void apply_exclude_libs(Context<E> &ctx);
template <typename E> void create_synthetic_sections(Context<E> &ctx);
template <typename E> void set_file_priority(Context<E> &ctx);
template <typename E> void resolve_obj_symbols(Context<E> &ctx);
template <typename E> void resolve_dso_symbols(Context<E> &ctx);
template <typename E> void eliminate_comdats(Context<E> &ctx);
template <typename E> void convert_common_symbols(Context<E> &ctx);
template <typename E> void add_comment_string(Context<E> &ctx, std::string str);
template <typename E> void compute_merged_section_sizes(Context<E> &ctx);
template <typename E> void bin_sections(Context<E> &ctx);
template <typename E> void check_duplicate_symbols(Context<E> &ctx);
template <typename E> std::vector<OutputChunk<E> *>
collect_output_sections(Context<E> &ctx);
template <typename E> void compute_section_sizes(Context<E> &ctx);
template <typename E> void convert_undefined_weak_symbols(Context<E> &ctx);
template <typename E> void scan_rels(Context<E> &ctx);
template <typename E> void apply_version_script(Context<E> &ctx);
template <typename E> void parse_symbol_version(Context<E> &ctx);
template <typename E> void compute_import_export(Context<E> &ctx);
template <typename E> void fill_verdef(Context<E> &ctx);
template <typename E> void fill_verneed(Context<E> &ctx);
template <typename E> void clear_padding(Context<E> &ctx, i64 filesize);
template <typename E> i64 get_section_rank(Context<E> &ctx, OutputChunk<E> *chunk);
template <typename E> i64 set_osec_offsets(Context<E> &ctx);
template <typename E> void fix_synthetic_symbols(Context<E> &ctx);

//
// main.cc
//

struct BuildId {
  template <typename E>
  i64 size(Context<E> &ctx) const;

  enum { NONE, HEX, HASH, UUID } kind = NONE;
  std::vector<u8> value;
  i64 hash_size = 0;
};

struct VersionPattern {
  std::string_view pattern;
  i16 ver_idx;
  bool is_extern_cpp;
};

template <typename E>
struct Context {
  Context() = default;
  Context(const Context<E> &) = delete;

  // Command-line arguments
  struct {
    BuildId build_id;
    bool Bsymbolic = false;
    bool Bsymbolic_functions = false;
    bool allow_multiple_definition = false;
    bool demangle = true;
    bool discard_all = false;
    bool discard_locals = false;
    bool eh_frame_hdr = true;
    bool export_dynamic = false;
    bool fatal_warnings = false;
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
    bool warn_common = false;
    bool z_copyreloc = true;
    bool z_defs = false;
    bool z_delete = true;
    bool z_dlopen = true;
    bool z_execstack = false;
    bool z_now = false;
    bool z_relro = true;
    i16 default_version = VER_NDX_GLOBAL;
    std::vector<std::string_view> version_definitions;
    std::vector<VersionPattern> version_patterns;
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
  } arg;

  void reset_reader_context(bool is_preloading) {
    as_needed = false;
    whole_archive = false;
    this->is_preloading = is_preloading;
    is_static = arg.is_static;
    visited.clear();
  }

  // Reader context
  bool as_needed;
  bool whole_archive;
  bool is_preloading;
  bool is_static;
  std::unordered_set<std::string_view> visited;
  tbb::task_group tg;

  bool has_error = false;

  // Symbol table
  ConcurrentMap<Symbol<E>> symbol_map;

  // Fully-expanded command line args
  std::vector<std::string_view> cmdline_args;

  // Input files
  std::vector<ObjectFile<E> *> objs;
  std::vector<SharedFile<E> *> dsos;
  ObjectFile<E> *internal_obj = nullptr;

  // Output buffer
  u8 *buf = nullptr;

  std::vector<OutputChunk<E> *> chunks;
  std::atomic_bool has_gottpoff = false;
  std::atomic_bool has_textrel = false;

  // Output chunks
  OutputEhdr<E> *ehdr = nullptr;
  OutputShdr<E> *shdr = nullptr;
  OutputPhdr<E> *phdr = nullptr;
  InterpSection<E> *interp = nullptr;
  GotSection<E> *got = nullptr;
  GotPltSection<E> *gotplt = nullptr;
  RelPltSection<E> *relplt = nullptr;
  RelDynSection<E> *reldyn = nullptr;
  DynamicSection<E> *dynamic = nullptr;
  StrtabSection<E> *strtab = nullptr;
  DynstrSection<E> *dynstr = nullptr;
  HashSection<E> *hash = nullptr;
  GnuHashSection<E> *gnu_hash = nullptr;
  ShstrtabSection<E> *shstrtab = nullptr;
  PltSection<E> *plt = nullptr;
  PltGotSection<E> *pltgot = nullptr;
  SymtabSection<E> *symtab = nullptr;
  DynsymSection<E> *dynsym = nullptr;
  EhFrameSection<E> *eh_frame = nullptr;
  EhFrameHdrSection<E> *eh_frame_hdr = nullptr;
  DynbssSection<E> *dynbss = nullptr;
  DynbssSection<E> *dynbss_relro = nullptr;
  VersymSection<E> *versym = nullptr;
  VerneedSection<E> *verneed = nullptr;
  VerdefSection<E> *verdef = nullptr;
  BuildIdSection<E> *buildid = nullptr;

  u64 tls_begin = -1;
  u64 tls_end = -1;

  // Linker-synthesized symbols
  Symbol<E> *__bss_start = nullptr;
  Symbol<E> *__ehdr_start = nullptr;
  Symbol<E> *__rela_iplt_start = nullptr;
  Symbol<E> *__rela_iplt_end = nullptr;
  Symbol<E> *__init_array_start = nullptr;
  Symbol<E> *__init_array_end = nullptr;
  Symbol<E> *__fini_array_start = nullptr;
  Symbol<E> *__fini_array_end = nullptr;
  Symbol<E> *__preinit_array_start = nullptr;
  Symbol<E> *__preinit_array_end = nullptr;
  Symbol<E> *_DYNAMIC = nullptr;
  Symbol<E> *_GLOBAL_OFFSET_TABLE_ = nullptr;
  Symbol<E> *__GNU_EH_FRAME_HDR = nullptr;
  Symbol<E> *_end = nullptr;
  Symbol<E> *_etext = nullptr;
  Symbol<E> *_edata = nullptr;
  Symbol<E> *__executable_start = nullptr;
};

template <typename E>
MemoryMappedFile<E> *find_library(Context<E> &ctx, std::string path);

template <typename E>
void read_file(Context<E> &ctx, MemoryMappedFile<E> *mb);

//
// Error output
//

inline thread_local bool opt_demangle = false;

template <typename E>
class SyncOut {
public:
  SyncOut(Context<E> &ctx, std::ostream &out = std::cout) : out(out) {
    opt_demangle = ctx.arg.demangle;
  }

  ~SyncOut() {
    std::lock_guard lock(mu);
    out << ss.str() << "\n";
  }

  template <class T> SyncOut &operator<<(T &&val) {
    ss << std::forward<T>(val);
    return *this;
  }

  static inline std::mutex mu;

private:
  std::ostream &out;
  std::stringstream ss;
};

template <typename E>
class Fatal {
public:
  Fatal(Context<E> &ctx) : out(ctx, std::cerr) {}

  [[noreturn]] ~Fatal() {
    out.~SyncOut();
    cleanup<E>();
    _exit(1);
  }

  template <class T> Fatal &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

private:
  SyncOut<E> out;
};

template <typename E>
class Error {
public:
  Error(Context<E> &ctx) : out(ctx, std::cerr) {
    ctx.has_error = true;
  }

  template <class T> Error &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

  static void checkpoint(Context<E> &ctx) {
    if (!ctx.has_error)
      return;
    cleanup<E>();
    _exit(1);
  }

private:
  SyncOut<E> out;
};

template <typename E>
class Warn {
public:
  Warn(Context<E> &ctx) : out(ctx, std::cerr) {
    if (ctx.arg.fatal_warnings)
      ctx.has_error = true;
  }

  template <class T> Warn &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

private:
  SyncOut<E> out;
};

#define unreachable(ctx)                                               \
  do {                                                                 \
    Fatal(ctx) << "internal error at " << __FILE__ << ":" << __LINE__; \
  } while (0)

template <typename E>
std::ostream &operator<<(std::ostream &out, const InputFile<E> &file);

//
// Inline objects and functions
//

template <typename E>
inline std::ostream &
operator<<(std::ostream &out, const InputSection<E> &isec) {
  out << isec.file << ":(" << isec.name << ")";
  return out;
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


template <typename E>
Symbol<E> *Symbol<E>::intern(Context<E> &ctx, std::string_view key,
                             std::string_view name) {
  return ctx.symbol_map.insert(key, {name});
}

template <typename E>
inline bool Symbol<E>::is_alive() const {
  if (frag)
    return frag->is_alive;
  if (input_section)
    return input_section->is_alive;
  return true;
}

template <typename E>
inline bool Symbol<E>::is_absolute(Context<E> &ctx) const {
  if (file == ctx.internal_obj)
    return false;
  if (file->is_dso)
    return esym->is_abs();
  if (is_imported)
    return false;
  if (frag)
    return false;
  return input_section == nullptr;
}

template <typename E>
inline bool Symbol<E>::is_relative(Context<E> &ctx) const {
  return !is_absolute(ctx);
}

template <typename E>
inline bool Symbol<E>::is_undef() const {
  return esym->is_undef() && esym->st_bind != STB_WEAK;
}

template <typename E>
inline bool Symbol<E>::is_undef_weak() const {
  return esym->is_undef() && esym->st_bind == STB_WEAK;
}

template <typename E>
inline u32 Symbol<E>::get_type() const {
  if (esym->st_type == STT_GNU_IFUNC && file->is_dso)
    return STT_FUNC;
  return esym->st_type;
}

template <typename E>
inline std::string_view Symbol<E>::get_version() const {
  if (file->is_dso)
    return ((SharedFile<E> *)file)->version_strings[ver_idx];
  return "";
}

template <typename E>
inline void Symbol<E>::clear() {
  Symbol<E> null;
  memcpy((char *)this, &null, sizeof(null));
}

template <typename E>
inline u64 Symbol<E>::get_addr(Context<E> &ctx) const {
  if (frag) {
    assert(frag->is_alive);
    return frag->get_addr(ctx) + value;
  }

  if (has_copyrel) {
    return copyrel_readonly
      ? ctx.dynbss_relro->shdr.sh_addr + value
      : ctx.dynbss->shdr.sh_addr + value;
  }

  if (plt_idx != -1 && esym->st_type == STT_GNU_IFUNC)
    return get_plt_addr(ctx);

  if (input_section) {
    if (input_section->is_ehframe)
      return ctx.eh_frame->get_addr(ctx, *this);

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
    return get_plt_addr(ctx);
  return value;
}

template <typename E>
inline u64 Symbol<E>::get_got_addr(Context<E> &ctx) const {
  assert(got_idx != -1);
  return ctx.got->shdr.sh_addr + got_idx * GOT_SIZE;
}

template <typename E>
inline u64 Symbol<E>::get_gotplt_addr(Context<E> &ctx) const {
  assert(gotplt_idx != -1);
  return ctx.gotplt->shdr.sh_addr + gotplt_idx * GOT_SIZE;
}

template <typename E>
inline u64 Symbol<E>::get_gottpoff_addr(Context<E> &ctx) const {
  assert(gottpoff_idx != -1);
  return ctx.got->shdr.sh_addr + gottpoff_idx * GOT_SIZE;
}

template <typename E>
inline u64 Symbol<E>::get_tlsgd_addr(Context<E> &ctx) const {
  assert(tlsgd_idx != -1);
  return ctx.got->shdr.sh_addr + tlsgd_idx * GOT_SIZE;
}

template <typename E>
inline u64 Symbol<E>::get_tlsdesc_addr(Context<E> &ctx) const {
  assert(tlsdesc_idx != -1);
  return ctx.got->shdr.sh_addr + tlsdesc_idx * GOT_SIZE;
}

template <typename E>
inline u64 Symbol<E>::get_plt_addr(Context<E> &ctx) const {
  assert(plt_idx != -1);

  if (got_idx == -1)
    return ctx.plt->shdr.sh_addr + plt_idx * PLT_SIZE;
  return ctx.pltgot->shdr.sh_addr + plt_idx * PLT_GOT_SIZE;
}

template <typename E>
inline u64 SectionFragment<E>::get_addr(Context<E> &ctx) const {
  return output_section.shdr.sh_addr + offset;
}

template <typename E>
inline u64 InputSection<E>::get_addr() const {
  return output_section->shdr.sh_addr + offset;
}

template <typename E>
inline i64 InputSection<E>::get_priority() const {
  return ((i64)file.priority << 32) | section_idx;
}

template <>
inline i64 InputSection<X86_64>::get_addend(const ElfRel<X86_64> &rel) const {
  return rel.r_addend;
}

template <>
inline i64 InputSection<I386>::get_addend(const ElfRel<I386> &rel) const {
  u8 *buf = (u8 *)contents.data();
  return *(i32 *)(buf + rel.r_offset);
}

template <typename E>
inline i64 ObjectFile<E>::get_shndx(const ElfSym<E> &esym) {
  assert(&elf_syms[0] <= &esym);
  assert(&esym < &elf_syms[elf_syms.size()]);

  if (esym.st_shndx == SHN_XINDEX)
    return symtab_shndx_sec[&esym - &elf_syms[0]];
  return esym.st_shndx;
}

template <typename E>
inline InputSection<E> *ObjectFile<E>::get_section(const ElfSym<E> &esym) {
  return sections[get_shndx(esym)];
}

template <typename E>
std::span<Symbol<E> *> ObjectFile<E>::get_global_syms() {
  return std::span<Symbol<E> *>(this->symbols).subspan(first_global);
}

template <typename E>
std::string rel_to_string(Context<E> &ctx, u32 r_type);

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
