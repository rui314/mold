#pragma once

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include "elf.h"

#include <atomic>
#include <cassert>
#include <cstdint>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <tbb/concurrent_hash_map.h>
#include <tbb/concurrent_vector.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/spin_mutex.h>
#include <tbb/task_group.h>
#include <unordered_set>
#include <vector>
#include <xxh3.h>

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
static constexpr i32 SHA256_SIZE = 32;

template <typename E> class InputFile;
template <typename E> class InputSection;
template <typename E> class MemoryMappedFile;
template <typename E> class MergedSection;
template <typename E> class ObjectFile;
template <typename E> class OutputChunk;
template <typename E> class OutputSection;
template <typename E> class SharedFile;
template <typename E> class Symbol;
template <typename E> struct CieRecord;
template <typename E> struct Context;
template <typename E> struct FdeRecord;

template <typename E> class ROutputChunk;
template <typename E> class ROutputEhdr;
template <typename E> class ROutputShdr;
template <typename E> class RStrtabSection;
template <typename E> class RSymtabSection;

class Compressor;
class TarFile;

template <typename E> void cleanup();

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym);

//
// Mergeable section fragments
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
  i32 idx = 0;
  i32 addend = 0;
};

// Additinal class members for dynamic symbols. Because most symbols
// don't need them and we allocate tens of millions of symbol objects
// for large programs, and we separate them from `Symbol` class to
// save memory.
struct SymbolAux {
  i32 got_idx = -1;
  i32 gotplt_idx = -1;
  i32 gottp_idx = -1;
  i32 tlsgd_idx = -1;
  i32 tlsdesc_idx = -1;
  i32 plt_idx = -1;
  i32 pltgot_idx = -1;
  i32 dynsym_idx = -1;
};

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
// input_sections.cc
//

enum {
  R_NONE = 1,
  R_ABS,
  R_DYN,
  R_BASEREL,
  R_PC,
  R_GOT,
  R_GOTOFF,
  R_GOTPC,
  R_GOTPCREL,
  R_SIZE,
  R_END,
};

// .eh_frame section contains CIE and FDE records to teach the runtime
// how to handle exceptions. Usually, a .eh_frame contains one CIE
// followed by as many FDEs as the number of functions defined by the
// file. CIE contains common information for FDEs (it is actually
// short for Common Information Entry). FDE contains the start address
// of a function and its length as well as how to handle exceptions
// for that function.
//
// Unlike other sections, the linker has to parse .eh_frame for optimal
// output for the following reasons:
//
//  - Compilers tend to emit the same CIE as long as the programming
//    language is the same, so CIEs in input object files are almost
//    always identical. We want to merge them to make a resulting
//    .eh_frame smaller.
//
//  - If we eliminate a function (e.g. when we see two object files
//    containing the duplicate definition of an inlined function), we
//    want to also eliminate a corresponding FDE so that a resulting
//    .eh_frame doesn't contain a dead FDE entry.
//
//  - If we need to compare two function definitions for equality for
//    ICF, we need to compare not only the function body but also its
//    exception handlers.
//
// Note that we assume that the first relocation entry for an FDE
// always points to the function that the FDE is associated to.
template <typename E>
struct FdeRecord {
  FdeRecord(u32 input_offset, u32 rel_idx)
    : input_offset(input_offset), rel_idx(rel_idx) {}

  FdeRecord(const FdeRecord &other)
    : cie(other.cie), input_offset(other.input_offset),
      output_offset(other.output_offset), rel_idx(other.rel_idx),
      is_alive(other.is_alive.load()) {}

  FdeRecord &operator=(const FdeRecord<E> &other) {
    cie = other.cie;
    input_offset = other.input_offset;
    output_offset = other.output_offset;
    rel_idx = other.rel_idx;
    is_alive = other.is_alive.load();
    return *this;
  }

  i64 size() const;
  std::string_view get_contents() const;
  std::span<ElfRel<E>> get_rels() const;

  union {
    CieRecord<E> *cie = nullptr;
    u32 cie_idx;
  };

  u32 input_offset = -1;
  u32 output_offset = -1;
  u32 rel_idx = -1;
  std::atomic_bool is_alive = true;
};

template <typename E>
struct CieRecord {
  CieRecord(Context<E> &ctx, ObjectFile<E> &file,
            InputSection<E> &isec, u32 input_offset, u32 rel_idx)
    : file(file), input_section(isec), input_offset(input_offset),
      rel_idx(rel_idx), rels(isec.get_rels(ctx)),
      contents(file.get_string(ctx, isec.shdr)) {}

  i64 size() const;
  std::string_view get_contents() const;
  std::span<ElfRel<E>> get_rels() const;
  bool equals(const CieRecord &other) const;

  ObjectFile<E> &file;
  InputSection<E> &input_section;
  u32 input_offset = -1;
  u32 output_offset = -1;
  u32 rel_idx = -1;
  u32 icf_idx = -1;
  bool is_leader = false;
  std::span<ElfRel<E>> rels;
  std::string_view contents;
};

// InputSection represents a section in an input object file.
template <typename E>
class InputSection {
public:
  InputSection(Context<E> &ctx, ObjectFile<E> &file, const ElfShdr<E> &shdr,
               std::string_view name, std::string_view contents,
               i64 section_idx)
    : file(file), shdr(shdr), nameptr(name.data()), namelen(name.size()),
      contents(contents), section_idx(section_idx) {
    output_section =
      OutputSection<E>::get_instance(ctx, name, shdr.sh_type, shdr.sh_flags);
  }

  void scan_relocations(Context<E> &ctx);
  void write_to(Context<E> &ctx, u8 *buf);
  void apply_reloc_alloc(Context<E> &ctx, u8 *base);
  void apply_reloc_nonalloc(Context<E> &ctx, u8 *base);
  inline void kill();

  inline std::string_view name() const {
    return {nameptr, (size_t)namelen};
  }

  inline i64 get_priority() const;
  inline u64 get_addr() const;
  inline i64 get_addend(const ElfRel<E> &rel) const;
  inline std::span<ElfRel<E>> get_rels(Context<E> &ctx) const;
  inline std::span<FdeRecord<E>> get_fdes() const;

  ObjectFile<E> &file;
  const ElfShdr<E> &shdr;
  OutputSection<E> *output_section = nullptr;

  std::string_view contents;

  std::unique_ptr<SectionFragmentRef<E>[]> rel_fragments;
  std::unique_ptr<u8[]> rel_types;
  i32 fde_begin = -1;
  i32 fde_end = -1;

  const char *nameptr = nullptr;
  i32 namelen = 0;

  u32 offset = -1;
  u32 section_idx = -1;
  u32 relsec_idx = -1;
  u32 reldyn_offset = 0;

  // For COMDAT de-duplication and garbage collection
  std::atomic_bool is_alive = true;

  // For garbage collection
  std::atomic_bool is_visited = false;

  // For ICF
  InputSection *leader = nullptr;
  u32 icf_idx = -1;
  bool icf_eligible = false;
  bool icf_leaf = false;

  bool is_ehframe = false;

private:
  typedef enum { NONE, ERROR, COPYREL, PLT, DYNREL, BASEREL } Action;

  void uncompress_old_style(Context<E> &ctx);
  void uncompress_new_style(Context<E> &ctx);
  void do_uncompress(Context<E> &ctx, std::string_view data, u64 size);

  void dispatch(Context<E> &ctx, Action table[3][4], u16 rel_type, i64 i);
  void report_undef(Context<E> &ctx, Symbol<E> &sym);
};

//
// output_chunks.cc
//

template <typename E>
bool is_relro(Context<E> &ctx, OutputChunk<E> *chunk);

// OutputChunk represents a contiguous region in an output file.
template <typename E>
class OutputChunk {
public:
  // There are three types of OutputChunks:
  //  - HEADER: the ELF, section or segment headers
  //  - REGULAR: output sections containing input sections
  //  - SYNTHETIC: linker-synthesized sections such as .got or .plt
  enum Kind : u8 { HEADER, REGULAR, SYNTHETIC };

  virtual ~OutputChunk() = default;
  virtual void copy_buf(Context<E> &ctx) {}
  virtual void write_to(Context<E> &ctx, u8 *buf);
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
  get_instance(Context<E> &ctx, std::string_view name, u64 type, u64 flags);

  void copy_buf(Context<E> &ctx) override;
  void write_to(Context<E> &ctx, u8 *buf) override;

  std::vector<InputSection<E> *> members;
  u32 idx;

private:
  OutputSection(std::string_view name, u32 type, u64 flags, u32 idx);
};

template <typename E>
class GotSection : public OutputChunk<E> {
public:
  GotSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".got";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = E::wordsize;
  }

  void add_got_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_gottp_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsgd_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsld(Context<E> &ctx);

  u64 get_tlsld_addr(Context<E> &ctx) const;
  i64 get_reldyn_size(Context<E> &ctx) const;
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> got_syms;
  std::vector<Symbol<E> *> gottp_syms;
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
    this->shdr.sh_addralign = E::wordsize;
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
    this->shdr.sh_addralign = E::plt_size;
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
    this->shdr.sh_addralign = E::pltgot_size;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class RelPltSection : public OutputChunk<E> {
public:
  RelPltSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = E::is_rel ? ".rel.plt" : ".rela.plt";
    this->shdr.sh_type = E::is_rel ? SHT_REL : SHT_RELA;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = E::wordsize;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class RelDynSection : public OutputChunk<E> {
public:
  RelDynSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = E::is_rel ? ".rel.dyn" : ".rela.dyn";
    this->shdr.sh_type = E::is_rel ? SHT_REL : SHT_RELA;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = E::wordsize;
  }

  void update_shdr(Context<E> &ctx) override;
  void sort(Context<E> &ctx);

  i64 relcount = 0;
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
    this->shdr.sh_size = 1;
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
    this->shdr.sh_addralign = E::wordsize;
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
    this->shdr.sh_addralign = E::wordsize;
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
    this->shdr.sh_addralign = E::wordsize;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void finalize(Context<E> &ctx);
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
    this->shdr.sh_addralign = E::wordsize;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  static constexpr i64 LOAD_FACTOR = 8;
  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 BLOOM_SHIFT = 26;
  static constexpr i64 ELFCLASS_BITS = E::wordsize * 8;

  u32 num_buckets = -1;
  u32 symoffset = -1;
  u32 num_bloom = 1;
};

template <typename E>
class MergedSection : public OutputChunk<E> {
public:
  static MergedSection<E> *
  get_instance(Context<E> &ctx, std::string_view name, u64 type, u64 flags);

  SectionFragment<E> *insert(std::string_view data, i64 alignment);
  void assign_offsets();
  void copy_buf(Context<E> &ctx) override;
  void write_to(Context<E> &ctx, u8 *buf) override;

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
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = E::wordsize;
  }

  void construct(Context<E> &ctx);
  void apply_reloc(Context<E> &ctx, ElfRel<E> &rel, u64 loc, u64 val);
  void copy_buf(Context<E> &ctx) override;
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

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  u32 num_fdes = 0;
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
    this->shdr.sh_addralign = E::wordsize;
  }

  void construct(Context<E> &ctx);
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

  void construct(Context<E> &ctx);
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
  void write_buildid(Context<E> &ctx);

  static constexpr i64 HEADER_SIZE = 16;
};

template <typename E>
class NotePropertySection : public OutputChunk<E> {
public:
  NotePropertySection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".note.gnu.property";
    this->shdr.sh_type = SHT_NOTE;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = E::wordsize;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  u32 features = 0;
};

template <typename E>
class GabiCompressedSection : public OutputChunk<E> {
public:
  GabiCompressedSection(Context<E> &ctx, OutputChunk<E> &chunk);
  void copy_buf(Context<E> &ctx) override;

private:
  ElfChdr<E> chdr = {};
  std::unique_ptr<Compressor> contents;
};

template <typename E>
class GnuCompressedSection : public OutputChunk<E> {
public:
  GnuCompressedSection(Context<E> &ctx, OutputChunk<E> &chunk);
  void copy_buf(Context<E> &ctx) override;

private:
  static constexpr i64 HEADER_SIZE = 12;
  i64 original_size = 0;
  std::unique_ptr<Compressor> contents;
};

template <typename E>
class ReproSection : public OutputChunk<E> {
public:
  ReproSection() : OutputChunk<E>(this->SYNTHETIC) {
    this->name = ".repro";
    this->shdr.sh_type = SHT_PROGBITS;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

private:
  std::unique_ptr<TarFile> tar;
};

bool is_c_identifier(std::string_view name);

template <typename E>
std::vector<ElfPhdr<E>> create_phdr(Context<E> &ctx);

//
// object_file.cc
//

// A comdat section typically represents an inline function,
// which are de-duplicated by the linker.
//
// For each inline function, there's one comdat section, which
// contains section indices of the function code and its data such as
// string literals, if any.
//
// Comdat sections are identified by its signature. If two comdat
// sections have the same signature, the linker picks up one and
// discards the other by eliminating all sections that the other
// comdat section refers to.
struct ComdatGroup {
  ComdatGroup() = default;
  ComdatGroup(const ComdatGroup &other)
    : owner(other.owner.load()) {}

  // The file priority of the owner file of this comdat section.
  std::atomic_uint32_t owner = -1;
};

// InputFile is the base class of ObjectFile and SharedFile.
template <typename E>
class InputFile {
public:
  InputFile(Context<E> &ctx, MemoryMappedFile<E> *mb);
  InputFile() : name("<internal>") {}

  template<typename T> std::span<T>
  inline get_data(Context<E> &ctx, const ElfShdr<E> &shdr);

  template<typename T> std::span<T>
  inline get_data(Context<E> &ctx, i64 idx);

  inline std::string_view get_string(Context<E> &ctx, const ElfShdr<E> &shdr);
  inline std::string_view get_string(Context<E> &ctx, i64 idx);

  ElfShdr<E> *find_section(i64 type);

  MemoryMappedFile<E> *mb;
  std::span<ElfShdr<E>> elf_sections;
  std::vector<Symbol<E> *> symbols;

  std::string name;
  bool is_dso = false;
  u32 priority;
  std::atomic_bool is_alive = false;
  std::string_view shstrtab;

protected:
  std::unique_ptr<Symbol<E>[]> local_syms;
};

// ObjectFile represents an input .o file.
template <typename E>
class ObjectFile : public InputFile<E> {
public:
  static ObjectFile<E> *create(Context<E> &ctx, MemoryMappedFile<E> *mb,
                               std::string archive_name, bool is_in_lib);

  static ObjectFile<E> *create_internal_file(Context<E> &ctx);

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
  std::vector<std::unique_ptr<InputSection<E>>> sections;
  std::span<ElfSym<E>> elf_syms;
  i64 first_global = 0;
  const bool is_in_lib = false;
  std::vector<CieRecord<E>> cies;
  std::vector<FdeRecord<E>> fdes;
  std::vector<const char *> symvers;
  std::vector<SectionFragment<E> *> fragments;
  std::vector<SectionFragmentRef<E>> sym_fragments;
  std::vector<std::pair<ComdatGroup *, std::span<u32>>> comdat_groups;
  bool exclude_libs = false;
  u32 features = 0;

  u64 num_dynrel = 0;
  u64 reldyn_offset = 0;

  u64 local_symtab_offset = 0;
  u64 global_symtab_offset = 0;
  u64 num_local_symtab = 0;
  u64 num_global_symtab = 0;
  u64 strtab_offset = 0;
  u64 strtab_size = 0;
  u64 fde_idx = 0;
  u64 fde_offset = 0;
  u64 fde_size = 0;

private:
  ObjectFile();

  ObjectFile(Context<E> &ctx, MemoryMappedFile<E> *mb,
             std::string archive_name, bool is_in_lib);

  void initialize_sections(Context<E> &ctx);
  void initialize_symbols(Context<E> &ctx);
  void initialize_mergeable_sections(Context<E> &ctx);
  void initialize_ehframe_sections(Context<E> &ctx);
  u32 read_note_gnu_property(Context<E> &ctx, const ElfShdr<E> &shdr);
  void read_ehframe(Context<E> &ctx, InputSection<E> &isec);
  void maybe_override_symbol(Context<E> &ctx, Symbol<E> &sym, i64 symidx);
  void merge_visibility(Context<E> &ctx, Symbol<E> &sym, u8 visibility);

  std::pair<std::string_view, const ElfShdr<E> *>
  uncompress_contents(Context<E> &ctx, const ElfShdr<E> &shdr,
                      std::string_view name);

  bool has_common_symbol;

  std::string_view symbol_strtab;
  const ElfShdr<E> *symtab_sec;
  std::span<u32> symtab_shndx_sec;
};

// SharedFile represents an input .so file.
template <typename E>
class SharedFile : public InputFile<E> {
public:
  static SharedFile<E> *create(Context<E> &ctx, MemoryMappedFile<E> *mb);

  void parse(Context<E> &ctx);
  void resolve_dso_symbols(Context<E> &ctx);
  std::vector<Symbol<E> *> find_aliases(Symbol<E> *sym);
  bool is_readonly(Context<E> &ctx, Symbol<E> *sym);

  std::string_view soname;
  std::vector<std::string_view> version_strings;
  std::vector<Symbol<E> *> globals;
  std::vector<const ElfSym<E> *> elf_syms;

private:
  SharedFile(Context<E> &ctx, MemoryMappedFile<E> *mb);

  std::string_view get_soname(Context<E> &ctx);
  void maybe_override_symbol(Symbol<E> &sym, const ElfSym<E> &esym);
  std::vector<std::string_view> read_verdef(Context<E> &ctx);

  std::vector<u16> versyms;
  std::string_view symbol_strtab;
  const ElfShdr<E> *symtab_sec;
};

//
// memory_mapped_file.cc
//

// MemoryMappedFile represents an mmap'ed input file.
// mold uses mmap-IO only.
template <typename E>
class MemoryMappedFile {
public:
  static MemoryMappedFile *open(Context<E> &ctx, std::string path);
  static MemoryMappedFile *must_open(Context<E> &ctx, std::string path);

  ~MemoryMappedFile();

  MemoryMappedFile *slice(Context<E> &ctx, std::string name, u64 start,
                          u64 size);

  u8 *data(Context<E> &ctx);
  i64 size() const { return size_; }

  std::string_view get_contents(Context<E> &ctx) {
    return std::string_view((char *)data(ctx), size());
  }

  std::string name;
  i64 mtime = 0;
  bool given_fullpath = true;

private:
  MemoryMappedFile(std::string name, u8 *data, u64 size, u64 mtime = 0)
    : name(name), data_(data), size_(size), mtime(mtime) {}

  std::mutex mu;
  MemoryMappedFile *parent;
  std::atomic<u8 *> data_;
  i64 size_ = 0;
};

enum class FileType { UNKNOWN, OBJ, DSO, AR, THIN_AR, TEXT };

template <typename E>
FileType get_file_type(Context<E> &ctx, MemoryMappedFile<E> *mb);

//
// archive_file.cc
//

// Unlike traditional linkers, mold doesn't read archive file symbol
// tables. Instead, it directly read archive members.
template <typename E>
std::vector<MemoryMappedFile<E> *>
read_fat_archive_members(Context<E> &ctx, MemoryMappedFile<E> *mb);

template <typename E>
std::vector<MemoryMappedFile<E> *>
read_thin_archive_members(Context<E> &ctx, MemoryMappedFile<E> *mb);

template <typename E>
std::vector<MemoryMappedFile<E> *>
read_archive_members(Context<E> &ctx, MemoryMappedFile<E> *mb);

//
// linker_script.cc
//

template <typename E>
void parse_linker_script(Context<E> &ctx, MemoryMappedFile<E> *mb);

template <typename E>
i64 get_script_output_type(Context<E> &ctx, MemoryMappedFile<E> *mb);

template <typename E>
void parse_version_script(Context<E> &ctx, std::string path);

template <typename E>
void parse_dynamic_list(Context<E> &ctx, std::string path);

//
// output_file.cc
//

// OutputFile represents a mmap'ed output file.
template <typename E>
class OutputFile {
public:
  static std::unique_ptr<OutputFile>
  open(Context<E> &ctx, std::string path, u64 filesize);

  virtual void close(Context<E> &ctx) = 0;
  virtual ~OutputFile() {}

  static inline char *tmpfile;

  u8 *buf = nullptr;
  std::string path;
  u64 filesize;
  bool is_mmapped;

protected:
  OutputFile(std::string path, u64 filesize, bool is_mmapped)
    : path(path), filesize(filesize), is_mmapped(is_mmapped) {}
};

//
// filepath.cc
//

// These are various utility functions to deal with file pathnames.
std::string get_current_dir();
std::string_view path_dirname(std::string_view path);
std::string_view path_filename(std::string_view path);
std::string_view path_basename(std::string_view path);
std::string path_to_absolute(std::string_view path);
std::string path_clean(std::string_view path);

//
// glob.cc
//

// GlobPattern handles the glob pattern. Currently, only '*' (zero or
// more occurrences of any character) is supported as a metacharacter.
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

// Counter is used to collect statistics numbers.
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

// Timer and TimeRecord records elapsed time (wall clock time)
// used by each pass of the linker.
struct TimerRecord {
  TimerRecord(std::string name, TimerRecord *parent = nullptr);
  void stop();

  std::string name;
  TimerRecord *parent;
  tbb::concurrent_vector<TimerRecord *> children;
  i64 start;
  i64 end;
  i64 user;
  i64 sys;
  bool stopped = false;
};

template <typename E>
class Timer {
public:
  Timer(Context<E> &ctx, std::string name, Timer *parent = nullptr);
  ~Timer();
  void stop();
  static void print(Context<E> &ctx);

private:
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
// relocatable.cc
//

template <typename E>
void combine_objects(Context<E> &ctx, std::span<std::string_view> file_args);

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
void try_resume_daemon(Context<E> &ctx);

template <typename E>
void daemonize(Context<E> &ctx, std::function<void()> *wait_for_client,
               std::function<void()> *on_complete);

template <typename E>
[[noreturn]]
void process_run_subcommand(Context<E> &ctx, int argc, char **argv);

//
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
std::string create_response_file(Context<E> &ctx);

template <typename E>
void parse_nonpositional_args(Context<E> &ctx,
                              std::vector<std::string_view> &remaining);

//
// compress.cc
//

class Compressor {
public:
  Compressor(std::string_view input);
  void write_to(u8 *buf);
  i64 size() const;

private:
  std::vector<std::vector<u8>> shards;
  u64 checksum = 0;
};

//
// tar.cc
//

// TarFile is a class to create a tar file.
//
// If you pass `--reproduce=repro.tar` to mold, mold collects all
// input files and put them into `repro.tar`, so that it is easy to
// run the same command with the same command line arguments.
class TarFile {
public:
  static constexpr i64 BLOCK_SIZE = 512;


  TarFile(std::string basedir) : basedir(basedir) {}

  void append(std::string path, std::string_view data);
  void write(u8 *buf);
  i64 size() const { return size_; }

private:
  std::string basedir;
  std::vector<std::pair<std::string, std::string_view>> contents;
  i64 size_ = BLOCK_SIZE * 2;
};

//
// passes.cc
//

template <typename E> void apply_exclude_libs(Context<E> &);
template <typename E> void create_synthetic_sections(Context<E> &);
template <typename E> void set_file_priority(Context<E> &);
template <typename E> void resolve_obj_symbols(Context<E> &);
template <typename E> void eliminate_comdats(Context<E> &);
template <typename E> void convert_common_symbols(Context<E> &);
template <typename E> void compute_merged_section_sizes(Context<E> &);
template <typename E> void bin_sections(Context<E> &);
template <typename E> void check_duplicate_symbols(Context<E> &);
template <typename E> void sort_init_fini(Context<E> &);
template <typename E> std::vector<OutputChunk<E> *>
collect_output_sections(Context<E> &);
template <typename E> void compute_section_sizes(Context<E> &);
template <typename E> void convert_undefined_weak_symbols(Context<E> &);
template <typename E> void scan_rels(Context<E> &);
template <typename E> void apply_version_script(Context<E> &);
template <typename E> void parse_symbol_version(Context<E> &);
template <typename E> void compute_import_export(Context<E> &);
template <typename E> void clear_padding(Context<E> &);
template <typename E> i64 get_section_rank(Context<E> &, OutputChunk<E> *chunk);
template <typename E> i64 set_osec_offsets(Context<E> &);
template <typename E> void fix_synthetic_symbols(Context<E> &);
template <typename E> void compress_debug_sections(Context<E> &);

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

typedef enum { COMPRESS_NONE, COMPRESS_GABI, COMPRESS_GNU } CompressKind;

struct VersionPattern {
  std::string_view pattern;
  i16 ver_idx;
  bool is_extern_cpp;
};

template <typename E, typename T>
class FileCache {
public:
  void store(MemoryMappedFile<E> *mb, T *obj) {
    Key k(mb->name, mb->size(), mb->mtime);
    cache[k].push_back(obj);
  }

  std::vector<T *> get(MemoryMappedFile<E> *mb) {
    Key k(mb->name, mb->size(), mb->mtime);
    std::vector<T *> objs = cache[k];
    cache[k].clear();
    return objs;
  }

  T *get_one(MemoryMappedFile<E> *mb) {
    std::vector<T *> objs = get(mb);
    return objs.empty() ? nullptr : objs[0];
  }

private:
  typedef std::tuple<std::string, i64, i64> Key;
  std::map<Key, std::vector<T *>> cache;
};

// Context represents a context object for each invocation of the linker.
// It contains command line flags, pointers to singleton objects
// (such as linker-synthesized output sections), unique_ptrs for
// resource management, and other miscellaneous objects.
template <typename E>
struct Context {
  Context() = default;
  Context(const Context<E> &) = delete;

  // Command-line arguments
  struct {
    BuildId build_id;
    CompressKind compress_debug_sections = COMPRESS_NONE;
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
    bool omagic = false;
    bool perf = false;
    bool pic = false;
    bool pie = false;
    bool preload = false;
    bool print_gc_sections = false;
    bool print_icf_sections = false;
    bool print_map = false;
    bool quick_exit = true;
    bool relax = true;
    bool relocatable = false;
    bool repro = false;
    bool shared = false;
    bool stats = false;
    bool strip_all = false;
    bool strip_debug = false;
    bool trace = false;
    bool warn_common = false;
    bool warn_unresolved_symbols = false;
    bool z_copyreloc = true;
    bool z_defs = false;
    bool z_delete = true;
    bool z_dlopen = true;
    bool z_execstack = false;
    bool z_initfirst = false;
    bool z_interpose = false;
    bool z_now = false;
    bool z_relro = true;
    i16 default_version = VER_NDX_GLOBAL;
    std::vector<std::string_view> version_definitions;
    std::vector<VersionPattern> version_patterns;
    i64 filler = -1;
    i64 spare_dynamic_tags = 5;
    i64 thread_count = -1;
    std::string Map;
    std::string chroot;
    std::string directory;
    std::string dynamic_linker;
    std::string entry = "_start";
    std::string fini = "_fini";
    std::string init = "_init";
    std::string output;
    std::string rpaths;
    std::string soname;
    std::string sysroot;
    std::unique_ptr<std::unordered_set<std::string_view>> retain_symbols_file;
    std::unordered_set<std::string_view> wrap;
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
  i64 file_priority = 2;
  std::unordered_set<std::string_view> visited;
  tbb::task_group tg;

  bool has_error = false;

  // Symbol table
  ConcurrentMap<Symbol<E>> symbol_map;

  ConcurrentMap<ComdatGroup> comdat_groups;
  tbb::concurrent_vector<std::unique_ptr<MergedSection<E>>> merged_sections;
  tbb::concurrent_vector<std::unique_ptr<OutputChunk<E>>> output_chunks;
  std::vector<std::unique_ptr<OutputSection<E>>> output_sections;
  FileCache<E, ObjectFile<E>> obj_cache;
  FileCache<E, SharedFile<E>> dso_cache;

  tbb::concurrent_vector<std::unique_ptr<TimerRecord>> timer_records;
  tbb::concurrent_vector<std::function<void()>> on_exit;

  tbb::concurrent_vector<std::unique_ptr<ObjectFile<E>>> owning_objs;
  tbb::concurrent_vector<std::unique_ptr<SharedFile<E>>> owning_dsos;
  tbb::concurrent_vector<std::unique_ptr<u8[]>> owning_bufs;
  tbb::concurrent_vector<std::unique_ptr<ElfShdr<E>>> owning_shdrs;
  tbb::concurrent_vector<std::unique_ptr<MemoryMappedFile<E>>> owning_mbs;

  // Symbol auxiliary data
  std::vector<SymbolAux> symbol_aux;

  // Fully-expanded command line args
  std::vector<std::string_view> cmdline_args;

  // Input files
  std::vector<ObjectFile<E> *> objs;
  std::vector<SharedFile<E> *> dsos;
  ObjectFile<E> *internal_obj = nullptr;

  // Output buffer
  std::unique_ptr<OutputFile<E>> output_file;
  u8 *buf = nullptr;

  std::vector<OutputChunk<E> *> chunks;
  std::atomic_bool has_gottp_rel = false;
  std::atomic_bool has_textrel = false;

  // Output chunks
  std::unique_ptr<OutputEhdr<E>> ehdr;
  std::unique_ptr<OutputShdr<E>> shdr;
  std::unique_ptr<OutputPhdr<E>> phdr;
  std::unique_ptr<InterpSection<E>> interp;
  std::unique_ptr<GotSection<E>> got;
  std::unique_ptr<GotPltSection<E>> gotplt;
  std::unique_ptr<RelPltSection<E>> relplt;
  std::unique_ptr<RelDynSection<E>> reldyn;
  std::unique_ptr<DynamicSection<E>> dynamic;
  std::unique_ptr<StrtabSection<E>> strtab;
  std::unique_ptr<DynstrSection<E>> dynstr;
  std::unique_ptr<HashSection<E>> hash;
  std::unique_ptr<GnuHashSection<E>> gnu_hash;
  std::unique_ptr<ShstrtabSection<E>> shstrtab;
  std::unique_ptr<PltSection<E>> plt;
  std::unique_ptr<PltGotSection<E>> pltgot;
  std::unique_ptr<SymtabSection<E>> symtab;
  std::unique_ptr<DynsymSection<E>> dynsym;
  std::unique_ptr<EhFrameSection<E>> eh_frame;
  std::unique_ptr<EhFrameHdrSection<E>> eh_frame_hdr;
  std::unique_ptr<DynbssSection<E>> dynbss;
  std::unique_ptr<DynbssSection<E>> dynbss_relro;
  std::unique_ptr<VersymSection<E>> versym;
  std::unique_ptr<VerneedSection<E>> verneed;
  std::unique_ptr<VerdefSection<E>> verdef;
  std::unique_ptr<BuildIdSection<E>> buildid;
  std::unique_ptr<NotePropertySection<E>> note_property;
  std::unique_ptr<ReproSection<E>> repro;

  // For --relocatable
  std::vector<ROutputChunk<E> *> r_chunks;
  ROutputEhdr<E> *r_ehdr = nullptr;
  ROutputShdr<E> *r_shdr = nullptr;
  RStrtabSection<E> *r_shstrtab = nullptr;
  RStrtabSection<E> *r_strtab = nullptr;
  RSymtabSection<E> *r_symtab = nullptr;

  u64 tls_begin = -1;
  u64 tls_end = -1;

  // Linker-synthesized symbols
  Symbol<E> *_DYNAMIC = nullptr;
  Symbol<E> *_GLOBAL_OFFSET_TABLE_ = nullptr;
  Symbol<E> *__GNU_EH_FRAME_HDR = nullptr;
  Symbol<E> *__bss_start = nullptr;
  Symbol<E> *__ehdr_start = nullptr;
  Symbol<E> *__executable_start = nullptr;
  Symbol<E> *__fini_array_end = nullptr;
  Symbol<E> *__fini_array_start = nullptr;
  Symbol<E> *__init_array_end = nullptr;
  Symbol<E> *__init_array_start = nullptr;
  Symbol<E> *__preinit_array_end = nullptr;
  Symbol<E> *__preinit_array_start = nullptr;
  Symbol<E> *__rel_iplt_end = nullptr;
  Symbol<E> *__rel_iplt_start = nullptr;
  Symbol<E> *_edata = nullptr;
  Symbol<E> *_end = nullptr;
  Symbol<E> *_etext = nullptr;
  Symbol<E> *edata = nullptr;
  Symbol<E> *end = nullptr;
  Symbol<E> *etext = nullptr;
};

template <typename E>
MemoryMappedFile<E> *find_library(Context<E> &ctx, std::string path);

template <typename E>
void read_file(Context<E> &ctx, MemoryMappedFile<E> *mb);

template <typename E>
std::string_view save_string(Context<E> &ctx, const std::string &str);

std::string get_version_string();

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
  Fatal(Context<E> &ctx) : out(ctx, std::cerr) {
    out << "mold: ";
  }

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
    out << "mold: ";
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
    out << "mold: ";
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
// Symbol
//

enum {
  NEEDS_GOT      = 1 << 0,
  NEEDS_PLT      = 1 << 1,
  NEEDS_GOTTP    = 1 << 2,
  NEEDS_TLSGD    = 1 << 3,
  NEEDS_TLSLD    = 1 << 4,
  NEEDS_COPYREL  = 1 << 5,
  NEEDS_DYNSYM   = 1 << 6,
  NEEDS_TLSDESC  = 1 << 7,
};

// Symbol class represents a defined symbol.
//
// A symbol has not only one but several different addresses if it
// has PLT or GOT entries. This class provides various functions to
// compute different addresses.
template <typename E>
class Symbol {
public:
  Symbol() = default;
  Symbol(std::string_view name) : nameptr(name.data()), namelen(name.size()) {}
  Symbol(const Symbol<E> &other) : Symbol(other.name()) {}

  // If we haven't seen the same `key` before, create a new instance
  // of Symbol and returns it. Otherwise, returns the previously-
  // instantiated object. `key` is usually the same as `name`.
  static Symbol<E> *intern(Context<E> &ctx, std::string_view key,
                           std::string_view name) {
    return ctx.symbol_map.insert(key, {name});
  }

  static Symbol<E> *intern(Context<E> &ctx, std::string_view name) {
    return intern(ctx, name, name);
  }

  u64 get_addr(Context<E> &ctx) const {
    if (SectionFragment<E> *frag = get_frag()) {
      if (!frag->is_alive) {
        // This condition is met if a non-alloc section refers an
        // alloc section and if the referenced piece of data is
        // garbage-collected. Typically, this condition is met if a
        // debug info section referring a string constant in .rodata.
        return 0;
      }

      return frag->get_addr(ctx) + value;
    }

    if (has_copyrel) {
      return copyrel_readonly
        ? ctx.dynbss_relro->shdr.sh_addr + value
        : ctx.dynbss->shdr.sh_addr + value;
    }

    if (has_plt(ctx) && esym().st_type == STT_GNU_IFUNC)
      return get_plt_addr(ctx);

    if (input_section) {
      if (input_section->is_ehframe) {
        // This is a special case: Only crtbegin.o and crtend.o
        // contain these symbols.
        if (name() == "__EH_FRAME_BEGIN__" || esym().st_type == STT_SECTION)
          return ctx.eh_frame->shdr.sh_addr;
        if (name() == "__FRAME_END__")
          return ctx.eh_frame->shdr.sh_addr + ctx.eh_frame->shdr.sh_size;
        Fatal(ctx) << "symbol referring .eh_frame is not supported: "
                   << *this << " " << *file;
      }

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

    if (has_plt(ctx))
      return get_plt_addr(ctx);
    return value;
  }

  u64 get_got_addr(Context<E> &ctx) const {
    return ctx.got->shdr.sh_addr + get_got_idx(ctx) * E::wordsize;
  }

  u64 get_gotplt_addr(Context<E> &ctx) const {
    assert(get_gotplt_idx(ctx) != -1);
    return ctx.gotplt->shdr.sh_addr + get_gotplt_idx(ctx) * E::wordsize;
  }

  u64 get_gottp_addr(Context<E> &ctx) const {
    assert(get_gottp_idx(ctx) != -1);
    return ctx.got->shdr.sh_addr + get_gottp_idx(ctx) * E::wordsize;
  }

  u64 get_tlsgd_addr(Context<E> &ctx) const {
    assert(get_tlsgd_idx(ctx) != -1);
    return ctx.got->shdr.sh_addr + get_tlsgd_idx(ctx) * E::wordsize;
  }

  u64 get_tlsdesc_addr(Context<E> &ctx) const {
    assert(get_tlsdesc_idx(ctx) != -1);
    return ctx.got->shdr.sh_addr + get_tlsdesc_idx(ctx) * E::wordsize;
  }

  u64 get_plt_addr(Context<E> &ctx) const {
    if (i32 idx = get_plt_idx(ctx); idx != -1)
      return ctx.plt->shdr.sh_addr + idx * E::plt_size;
    return ctx.pltgot->shdr.sh_addr + get_pltgot_idx(ctx) * E::pltgot_size;
  }

  void set_got_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].got_idx < 0);
    ctx.symbol_aux[aux_idx].got_idx = idx;
  }

  void set_gotplt_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].gotplt_idx < 0);
    ctx.symbol_aux[aux_idx].gotplt_idx = idx;
  }

  void set_gottp_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].gottp_idx < 0);
    ctx.symbol_aux[aux_idx].gottp_idx = idx;
  }

  void set_tlsgd_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].tlsgd_idx < 0);
    ctx.symbol_aux[aux_idx].tlsgd_idx = idx;
  }

  void set_tlsdesc_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].tlsdesc_idx < 0);
    ctx.symbol_aux[aux_idx].tlsdesc_idx = idx;
  }

  void set_plt_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].plt_idx < 0);
    ctx.symbol_aux[aux_idx].plt_idx = idx;
  }

  void set_pltgot_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].pltgot_idx < 0);
    ctx.symbol_aux[aux_idx].pltgot_idx = idx;
  }

  void set_dynsym_idx(Context<E> &ctx, i32 idx) const {
    assert(aux_idx != -1);
    assert(ctx.symbol_aux[aux_idx].dynsym_idx < 0);
    ctx.symbol_aux[aux_idx].dynsym_idx = idx;
  }

  i32 get_got_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].got_idx;
  }

  i32 get_gotplt_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].gotplt_idx;
  }

  i32 get_gottp_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].gottp_idx;
  }

  i32 get_tlsgd_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].tlsgd_idx;
  }

  i32 get_tlsdesc_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].tlsdesc_idx;
  }

  i32 get_plt_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].plt_idx;
  }

  i32 get_pltgot_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].pltgot_idx;
  }

  i32 get_dynsym_idx(Context<E> &ctx) const {
    return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].dynsym_idx;
  }

  bool has_plt(Context<E> &ctx) const {
    return get_plt_idx(ctx) != -1 || get_pltgot_idx(ctx) != -1;
  }

  bool has_got(Context<E> &ctx) const {
    return get_got_idx(ctx) != -1;
  }

  bool is_alive() const {
    if (SectionFragment<E> *frag = get_frag())
      return frag->is_alive;
    if (input_section)
      return input_section->is_alive;
    return true;
  }

  bool is_absolute(Context<E> &ctx) const {
    if (file == ctx.internal_obj)
      return false;
    if (file->is_dso)
      return esym().is_abs();
    if (is_imported)
      return false;
    if (get_frag())
      return false;
    return input_section == nullptr;
  }

  bool is_relative(Context<E> &ctx) const {
    return !is_absolute(ctx);
  }

  bool is_undef() const {
    return esym().is_undef() && esym().st_bind != STB_WEAK;
  }

  bool is_undef_weak() const {
    return esym().is_undef() && esym().st_bind == STB_WEAK;
  }

  u32 get_type() const {
    if (esym().st_type == STT_GNU_IFUNC && file->is_dso)
      return STT_FUNC;
    return esym().st_type;
  }

  std::string_view get_version() const {
    if (file->is_dso)
      return ((SharedFile<E> *)file)->version_strings[ver_idx];
    return "";
  }

  std::string_view get_demangled_name() const;

  const ElfSym<E> &esym() const {
    if (file->is_dso)
      return *((SharedFile<E> *)file)->elf_syms[sym_idx];
    return ((ObjectFile<E> *)file)->elf_syms[sym_idx];
  }

  SectionFragment<E> *get_frag() const {
    if (!file || file->is_dso)
      return nullptr;
    return ((ObjectFile<E> *)file)->sym_fragments[sym_idx].frag;
  }

  std::string_view name() const {
    return {nameptr, (size_t)namelen};
  }

  // A symbol is owned by a file. If two or more files define the
  // same symbol, the one with the strongest definition owns the symbol.
  // If `file` is null, the symbol is equivalent to nonexistent.
  InputFile<E> *file = nullptr;

  InputSection<E> *input_section = nullptr;
  const char *nameptr = nullptr;

  u64 value = -1;

  // Index into the symbol table of the owner file.
  i32 sym_idx = -1;

  i32 namelen = 0;
  i32 aux_idx = -1;
  u16 shndx = 0;
  u16 ver_idx = 0;

  // `flags` has NEEDS_ flags.
  std::atomic_uint8_t flags = 0;

  tbb::spin_mutex mu;
  std::atomic_uint8_t visibility = STV_DEFAULT;

  u8 is_lazy : 1 = false;
  u8 is_weak : 1 = false;
  u8 write_to_symtab : 1 = false;
  u8 traced : 1 = false;
  u8 wrap : 1 = false;
  u8 has_copyrel : 1 = false;
  u8 copyrel_readonly : 1 = false;

  // If a symbol can be interposed at runtime, `is_imported` is true.
  // If a symbol is a dynamic symbol and can be used by other ELF
  // module at runtime, `is_exported` is true.
  //
  // Note that both can be true at the same time. Such symbol
  // represents a function or data exported from this ELF module
  // which can be interposed by other definition at runtime.
  // That is the usual exported symbols when creating a DSO.
  // In other words, a dynamic symbol is exported by a DSO and
  // imported by itself.
  //
  // If is_imported is true and is_exported is false, it is a dynamic
  // symbol imported from other DSO.
  //
  // If is_imported is false and is_exported is true, there are two
  // possible cases. If we are creating an executable, we know that
  // exported symbols cannot be interposed by any DSO (because the
  // dynamic loader searches a dynamic symbol from an exectuable
  // before examining any DSOs), so any exported symbol is export-only.
  // If we are creating a DSO, export-only symbols represent a
  // protected symbol (i.e. a symbol whose visibility is STV_PROTECTED).
  u8 is_imported : 1 = false;
  u8 is_exported : 1 = false;
};

//
// Inline objects and functions
//

template <typename E>
inline std::ostream &
operator<<(std::ostream &out, const InputSection<E> &isec) {
  out << isec.file << ":(" << isec.name() << ")";
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
inline u64 SectionFragment<E>::get_addr(Context<E> &ctx) const {
  return output_section.shdr.sh_addr + offset;
}


template <typename E>
inline void InputSection<E>::kill() {
  if (is_alive.exchange(false)) {
    is_alive = false;
    for (FdeRecord<E> &fde : get_fdes())
      fde.is_alive = false;
  }
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
inline std::span<ElfRel<E>> InputSection<E>::get_rels(Context<E> &ctx) const {
  if (relsec_idx == -1)
    return {};
  return file.template get_data<ElfRel<E>>(ctx, file.elf_sections[relsec_idx]);
}

template <typename E>
inline std::span<FdeRecord<E>> InputSection<E>::get_fdes() const {
  std::span<FdeRecord<E>> span(file.fdes);
  return span.subspan(fde_begin, fde_end - fde_begin);
}

template <typename E>
template <typename T>
inline std::span<T> InputFile<E>::get_data(Context<E> &ctx, const ElfShdr<E> &shdr) {
  std::string_view view = this->get_string(ctx, shdr);
  if (view.size() % sizeof(T))
    Fatal(ctx) << *this << ": corrupted section";
  return {(T *)view.data(), view.size() / sizeof(T)};
}

template <typename E>
template <typename T>
inline std::span<T> InputFile<E>::get_data(Context<E> &ctx, i64 idx) {
  if (elf_sections.size() <= idx)
    Fatal(ctx) << *this << ": invalid section index";
  return this->template get_data<T>(elf_sections[idx]);
}

template <typename E>
inline std::string_view
InputFile<E>::get_string(Context<E> &ctx, const ElfShdr<E> &shdr) {
  u8 *begin = mb->data(ctx) + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb->data(ctx) + mb->size() < end)
    Fatal(ctx) << *this << ": shdr corrupted";
  return {(char *)begin, (char *)end};
}

template <typename E>
inline std::string_view InputFile<E>::get_string(Context<E> &ctx, i64 idx) {
  assert(idx < elf_sections.size());

  if (elf_sections.size() <= idx)
    Fatal(ctx) << *this << ": invalid section index: " << idx;
  return this->get_string(ctx, elf_sections[idx]);
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
  return sections[get_shndx(esym)].get();
}

template <typename E>
std::span<Symbol<E> *> ObjectFile<E>::get_global_syms() {
  return std::span<Symbol<E> *>(this->symbols).subspan(first_global);
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

inline u32 djb_hash(std::string_view name) {
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

inline u64 read64be(u8 *buf) {
  return ((u64)buf[0] << 56) | ((u64)buf[1] << 48) |
         ((u64)buf[2] << 40) | ((u64)buf[3] << 32) |
         ((u64)buf[4] << 24) | ((u64)buf[5] << 16) |
         ((u64)buf[6] << 8)  | (u64)buf[7];
}

inline void write64be(u8 *buf, u64 val) {
  buf[0] = val >> 56;
  buf[1] = val >> 48;
  buf[2] = val >> 40;
  buf[3] = val >> 32;
  buf[4] = val >> 24;
  buf[5] = val >> 16;
  buf[6] = val >> 8;
  buf[7] = val;
}

inline void write32be(u8 *buf, u32 val) {
  buf[0] = val >> 24;
  buf[1] = val >> 16;
  buf[2] = val >> 8;
  buf[3] = val;
}
