#pragma once

#include "elf.h"
#include "../mold.h"

#include <atomic>
#include <cassert>
#include <cstdint>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <tbb/concurrent_hash_map.h>
#include <tbb/concurrent_unordered_map.h>
#include <tbb/concurrent_vector.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/spin_mutex.h>
#include <tbb/task_group.h>
#include <unistd.h>
#include <unordered_map>
#include <unordered_set>
#include <variant>
#include <vector>

#define XXH_INLINE_ALL 1
#include <xxhash.h>

namespace mold::elf {

static constexpr i32 SHA256_SIZE = 32;

template <typename E> class InputFile;
template <typename E> class InputSection;
template <typename E> class MergedSection;
template <typename E> class ObjectFile;
template <typename E> class Chunk;
template <typename E> class OutputSection;
template <typename E> class SharedFile;
template <typename E> class Symbol;
template <typename E> struct CieRecord;
template <typename E> struct Context;
template <typename E> struct FdeRecord;

template <typename E> class RChunk;
template <typename E> class ROutputEhdr;
template <typename E> class ROutputShdr;
template <typename E> class RStrtabSection;
template <typename E> class RSymtabSection;

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym);

class HashCmp {
public:
  static size_t hash(const std::string_view &k) {
    return XXH3_64bits(k.data(), k.size());
  }

  static bool equal(const std::string_view &k1, const std::string_view &k2) {
    return k1 == k2;
  }
};

//
// Mergeable section fragments
//

template <typename E>
struct SectionFragment {
  SectionFragment(MergedSection<E> *sec) : output_section(*sec) {}

  SectionFragment(const SectionFragment &other)
    : output_section(other.output_section), offset(other.offset),
      p2align(other.p2align.load()), is_alive(other.is_alive.load()) {}

  u64 get_addr(Context<E> &ctx) const;

  MergedSection<E> &output_section;
  u32 offset = -1;
  std::atomic_uint8_t p2align = 0;
  std::atomic_bool is_alive = false;
};

template <typename E>
struct SectionFragmentRef {
  SectionFragment<E> *frag = nullptr;
  i32 idx = 0;
  i32 addend = 0;
};

// Additional class members for dynamic symbols. Because most symbols
// don't need them and we allocate tens of millions of symbol objects
// for large programs, we separate them from `Symbol` class to save
// memory.
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

inline u64 hash_string(std::string_view str) {
  return XXH3_64bits(str.data(), str.size());
}

//
// input-sections.cc
//

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
struct CieRecord {
  CieRecord(Context<E> &ctx, ObjectFile<E> &file, InputSection<E> &isec,
            u32 input_offset, std::span<ElfRel<E>> rels, u32 rel_idx)
    : file(file), input_section(isec), input_offset(input_offset),
      rel_idx(rel_idx), rels(rels), contents(file.get_string(ctx, isec.shdr())) {}

  i64 size() const {
    return *(u32 *)(contents.data() + input_offset) + 4;
  }

  std::string_view get_contents() const {
    return contents.substr(input_offset, size());
  }

  std::span<ElfRel<E>> get_rels() const {
    i64 end = rel_idx;
    while (end < rels.size() && rels[end].r_offset < input_offset + size())
      end++;
    return rels.subspan(rel_idx, end - rel_idx);
  }

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

template <typename E>
struct FdeRecord {
  FdeRecord(u32 input_offset, u32 rel_idx)
    : input_offset(input_offset), rel_idx(rel_idx) {}

  FdeRecord(const FdeRecord &other)
    : input_offset(other.input_offset), output_offset(other.output_offset),
      rel_idx(other.rel_idx), cie_idx(other.cie_idx),
      is_alive(other.is_alive.load()) {}

  FdeRecord &operator=(const FdeRecord<E> &other) {
    input_offset = other.input_offset;
    output_offset = other.output_offset;
    rel_idx = other.rel_idx;
    cie_idx = other.cie_idx;
    is_alive = other.is_alive.load();
    return *this;
  }

  i64 size(ObjectFile<E> &file) const;
  std::string_view get_contents(ObjectFile<E> &file) const;
  std::span<ElfRel<E>> get_rels(ObjectFile<E> &file) const;

  u32 input_offset = -1;
  u32 output_offset = -1;
  u32 rel_idx = -1;
  u16 cie_idx = -1;
  std::atomic_bool is_alive = true;
};

template <typename E>
class RangeExtensionThunk {};

template <>
class RangeExtensionThunk<ARM64> {
public:
  RangeExtensionThunk(OutputSection<ARM64> &osec)
    : output_section(osec) {}

  i64 size() const { return symbols.size() * ENTRY_SIZE; }
  u64 get_addr(i64 idx) const;
  void copy_buf(Context<ARM64> &ctx);

  static constexpr i64 ENTRY_SIZE = 12;

  OutputSection<ARM64> &output_section;
  i32 thunk_idx = -1;
  i64 offset = -1;
  std::mutex mu;
  std::vector<Symbol<ARM64> *> symbols;
  std::vector<i32> symbol_map;
  std::unique_ptr<std::atomic_bool[]> used;
};

struct RangeExtensionRef {
  i32 thunk_idx = -1;
  i32 sym_idx = -1;
};

template <typename E>
struct InputSectionExtra {
  InputSectionExtra() = default;

  InputSectionExtra(const InputSectionExtra &other)
    : is_visited(other.is_visited.load()), leader(other.leader),
      icf_idx(other.icf_idx), icf_eligible(other.icf_eligible),
      icf_leaf(other.icf_leaf) {}

  // For garbage collection
  std::atomic_bool is_visited = false;

  // For ICF
  InputSection<E> *leader = nullptr;
  u32 icf_idx = -1;
  bool icf_eligible = false;
  bool icf_leaf = false;
};

// InputSection represents a section in an input object file.
template <typename E>
class InputSection {
public:
  InputSection(Context<E> &ctx, ObjectFile<E> &file, std::string_view name,
               i64 shndx);

  bool is_compressed();
  void uncompress(Context<E> &ctx, u8 *buf);
  void scan_relocations(Context<E> &ctx);
  void write_to(Context<E> &ctx, u8 *buf);
  void apply_reloc_alloc(Context<E> &ctx, u8 *base);
  void apply_reloc_nonalloc(Context<E> &ctx, u8 *base);
  void kill();

  std::string_view name() const;
  InputSectionExtra<E> &extra() const;
  i64 get_priority() const;
  u64 get_addr() const;
  i64 get_addend(const ElfRel<E> &rel) const;
  const ElfShdr<E> &shdr() const;
  std::span<ElfRel<E>> get_rels(Context<E> &ctx) const;
  std::span<FdeRecord<E>> get_fdes() const;

  // For ARM64 range extension thunks
  std::vector<RangeExtensionRef> &get_range_extn() const;

  // For RISC-V section resizing
  std::vector<i32> &get_r_deltas() const;
  std::span<Symbol<E> *> get_sorted_symbols() const;

  ObjectFile<E> &file;
  OutputSection<E> *output_section = nullptr;

  std::string_view contents;

  std::unique_ptr<SectionFragmentRef<E>[]> rel_fragments;
  i32 fde_begin = -1;
  i32 fde_end = -1;

  u32 offset = -1;
  u32 shndx = -1;
  u32 relsec_idx = -1;
  u32 reldyn_offset = 0;
  u32 sh_size = -1;

  // For COMDAT de-duplication and garbage collection
  std::atomic_bool is_alive = true;

  u8 p2align = 0;

private:
  typedef enum : u8 { NONE, ERROR, COPYREL, PLT, DYNREL, BASEREL } Action;

  void dispatch(Context<E> &ctx, Action table[3][4], i64 i,
                const ElfRel<E> &rel, Symbol<E> &sym);
  void copy_contents(Context<E> &ctx, u8 *buf);
  void copy_contents_riscv(Context<E> &ctx, u8 *buf);

  std::pair<SectionFragment<E> *, i64>
  get_fragment(Context<E> &ctx, const ElfRel<E> &rel);

  bool is_relr_reloc(Context<E> &ctx, const ElfRel<E> &rel);
};

template <typename E>
void report_undef(Context<E> &ctx, InputFile<E> &file, Symbol<E> &sym);

//
// output-chunks.cc
//

template <typename E>
bool is_relro(Context<E> &ctx, Chunk<E> *chunk);

template <typename E>
bool separate_page(Context<E> &ctx, Chunk<E> *a, Chunk<E> *b);

// Chunk represents a contiguous region in an output file.
template <typename E>
class Chunk {
public:
  virtual ~Chunk() = default;

  virtual bool is_header() const { return false; }
  virtual bool is_output_section() const { return false; }

  virtual void copy_buf(Context<E> &ctx) {}
  virtual void write_to(Context<E> &ctx, u8 *buf);
  virtual void update_shdr(Context<E> &ctx) {}

  std::string_view name;
  i64 shndx = 0;
  ElfShdr<E> shdr = {};

protected:
  Chunk() { shdr.sh_addralign = 1; }
};

// ELF header
template <typename E>
class OutputEhdr : public Chunk<E> {
public:
  OutputEhdr() {
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_size = sizeof(ElfEhdr<E>);
    this->shdr.sh_addralign = E::word_size;
  }

  bool is_header() const override { return true; }
  void copy_buf(Context<E> &ctx) override;
};

// Section header
template <typename E>
class OutputShdr : public Chunk<E> {
public:
  OutputShdr() {
    this->shdr.sh_addralign = E::word_size;
  }

  bool is_header() const override { return true; }
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

// Program header
template <typename E>
class OutputPhdr : public Chunk<E> {
public:
  OutputPhdr() {
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = E::word_size;
  }

  bool is_header() const override { return true; }
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class InterpSection : public Chunk<E> {
public:
  InterpSection() {
    this->name = ".interp";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

// Sections
template <typename E>
class OutputSection : public Chunk<E> {
public:
  static OutputSection *
  get_instance(Context<E> &ctx, std::string_view name, u64 type, u64 flags);

  bool is_output_section() const override { return true; }
  void copy_buf(Context<E> &ctx) override;
  void write_to(Context<E> &ctx, u8 *buf) override;

  std::vector<InputSection<E> *> members;
  u32 idx;

  void construct_relr(Context<E> &ctx);
  std::vector<typename E::WordTy> relr;

  std::vector<std::unique_ptr<RangeExtensionThunk<E>>> thunks;

private:
  OutputSection(std::string_view name, u32 type, u64 flags, u32 idx)
    : idx(idx) {
    this->name = name;
    this->shdr.sh_type = type;
    this->shdr.sh_flags = flags;
  }
};

template <typename E>
struct GotEntry {
  i64 idx = 0;
  u64 val = 0;
  i64 r_type = 0;
  Symbol<E> *sym = nullptr;
};

template <typename E>
class GotSection : public Chunk<E> {
public:
  GotSection() {
    this->name = ".got";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = E::word_size;
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

  void construct_relr(Context<E> &ctx);
  std::vector<typename E::WordTy> relr;

private:
  std::vector<GotEntry<E>> get_entries(Context<E> &ctx) const;
};

template <typename E>
class GotPltSection : public Chunk<E> {
public:
  GotPltSection() {
    this->name = ".got.plt";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = E::word_size;
  }

  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class PltSection : public Chunk<E> {
public:
  PltSection() {
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
class PltGotSection : public Chunk<E> {
public:
  PltGotSection() {
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
class RelPltSection : public Chunk<E> {
public:
  RelPltSection() {
    this->name = E::is_rel ? ".rel.plt" : ".rela.plt";
    this->shdr.sh_type = E::is_rel ? SHT_REL : SHT_RELA;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = E::word_size;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class RelDynSection : public Chunk<E> {
public:
  RelDynSection() {
    this->name = E::is_rel ? ".rel.dyn" : ".rela.dyn";
    this->shdr.sh_type = E::is_rel ? SHT_REL : SHT_RELA;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = E::word_size;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
  void sort(Context<E> &ctx);

  i64 relcount = 0;
};

template <typename E>
class RelrDynSection : public Chunk<E> {
public:
  RelrDynSection() {
    this->name = ".relr.dyn";
    this->shdr.sh_type = SHT_RELR;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = E::word_size;
    this->shdr.sh_addralign = E::word_size;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class StrtabSection : public Chunk<E> {
public:
  StrtabSection() {
    this->name = ".strtab";
    this->shdr.sh_type = SHT_STRTAB;
    this->shdr.sh_size = 1;
  }

  void update_shdr(Context<E> &ctx) override;
};

template <typename E>
class ShstrtabSection : public Chunk<E> {
public:
  ShstrtabSection() {
    this->name = ".shstrtab";
    this->shdr.sh_type = SHT_STRTAB;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class DynstrSection : public Chunk<E> {
public:
  DynstrSection() {
    this->name = ".dynstr";
    this->shdr.sh_type = SHT_STRTAB;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_size = 1;
  }

  i64 add_string(std::string_view str);
  i64 find_string(std::string_view str);
  void copy_buf(Context<E> &ctx) override;

  i64 dynsym_offset = -1;

private:
  std::unordered_map<std::string_view, i64> strings;
};

template <typename E>
class DynamicSection : public Chunk<E> {
public:
  DynamicSection() {
    this->name = ".dynamic";
    this->shdr.sh_type = SHT_DYNAMIC;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = E::word_size;
    this->shdr.sh_entsize = sizeof(ElfDyn<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class SymtabSection : public Chunk<E> {
public:
  SymtabSection() {
    this->name = ".symtab";
    this->shdr.sh_type = SHT_SYMTAB;
    this->shdr.sh_entsize = sizeof(ElfSym<E>);
    this->shdr.sh_addralign = E::word_size;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class DynsymSection : public Chunk<E> {
public:
  DynsymSection() {
    this->name = ".dynsym";
    this->shdr.sh_type = SHT_DYNSYM;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfSym<E>);
    this->shdr.sh_addralign = E::word_size;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void finalize(Context<E> &ctx);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols{1};
};

template <typename E>
class HashSection : public Chunk<E> {
public:
  HashSection() {
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
class GnuHashSection : public Chunk<E> {
public:
  GnuHashSection() {
    this->name = ".gnu.hash";
    this->shdr.sh_type = SHT_GNU_HASH;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = E::word_size;
  }

  std::span<Symbol<E> *> get_exported_symbols(Context<E> &ctx);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  static constexpr i64 LOAD_FACTOR = 8;
  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 BLOOM_SHIFT = 26;
  static constexpr i64 ELFCLASS_BITS = E::word_size * 8;

  u32 num_buckets = -1;
  u32 num_bloom = 1;
};

template <typename E>
class MergedSection : public Chunk<E> {
public:
  static MergedSection<E> *
  get_instance(Context<E> &ctx, std::string_view name, u64 type, u64 flags);

  SectionFragment<E> *insert(std::string_view data, u64 hash, i64 p2align);
  void assign_offsets(Context<E> &ctx);
  void copy_buf(Context<E> &ctx) override;
  void write_to(Context<E> &ctx, u8 *buf) override;
  void print_stats(Context<E> &ctx);

  HyperLogLog estimator;

private:
  MergedSection(std::string_view name, u64 flags, u32 type);

  ConcurrentMap<SectionFragment<E>> map;
  std::vector<i64> shard_offsets;
  std::once_flag once_flag;
};

template <typename E>
class EhFrameSection : public Chunk<E> {
public:
  EhFrameSection() {
    this->name = ".eh_frame";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = E::word_size;
  }

  void construct(Context<E> &ctx);
  void apply_reloc(Context<E> &ctx, ElfRel<E> &rel, u64 offset, u64 val);
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class EhFrameHdrSection : public Chunk<E> {
public:
  EhFrameHdrSection() {
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
class CopyrelSection : public Chunk<E> {
public:
  CopyrelSection(bool is_relro) : is_relro(is_relro) {
    this->name = is_relro ? ".copyrel.rel.ro" : ".copyrel";
    this->shdr.sh_type = SHT_NOBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = 64;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  bool is_relro;
  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class VersymSection : public Chunk<E> {
public:
  VersymSection() {
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
class VerneedSection : public Chunk<E> {
public:
  VerneedSection() {
    this->name = ".gnu.version_r";
    this->shdr.sh_type = SHT_GNU_VERNEED;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = E::word_size;
  }

  void construct(Context<E> &ctx);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

template <typename E>
class VerdefSection : public Chunk<E> {
public:
  VerdefSection() {
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
class BuildIdSection : public Chunk<E> {
public:
  BuildIdSection() {
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
class NotePropertySection : public Chunk<E> {
public:
  NotePropertySection() {
    this->name = ".note.gnu.property";
    this->shdr.sh_type = SHT_NOTE;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = E::word_size;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  u32 features = 0;
};

template <typename E>
class GabiCompressedSection : public Chunk<E> {
public:
  GabiCompressedSection(Context<E> &ctx, Chunk<E> &chunk);
  void copy_buf(Context<E> &ctx) override;

private:
  ElfChdr<E> chdr = {};
  std::unique_ptr<ZlibCompressor> contents;
};

template <typename E>
class GnuCompressedSection : public Chunk<E> {
public:
  GnuCompressedSection(Context<E> &ctx, Chunk<E> &chunk);
  void copy_buf(Context<E> &ctx) override;

private:
  static constexpr i64 HEADER_SIZE = 12;
  i64 original_size = 0;
  std::unique_ptr<ZlibCompressor> contents;
};

template <typename E>
class RelocSection : public Chunk<E> {
public:
  RelocSection(Context<E> &ctx, OutputSection<E> &osec);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

private:
  using RelaTy =
    typename std::conditional<E::word_size == 4, Elf32Rela, Elf64Rela>::type;

  OutputSection<E> &output_section;
  std::vector<i64> offsets;
};

bool is_c_identifier(std::string_view name);

template <typename E>
std::vector<ElfPhdr<E>> create_phdr(Context<E> &ctx);

//
// input-files.cc
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

template <typename E>
struct MergeableSection {
  MergedSection<E> *parent;
  u8 p2align = 0;
  std::vector<std::string_view> strings;
  std::vector<u64> hashes;
  std::vector<u32> frag_offsets;
  std::vector<SectionFragment<E> *> fragments;
};

// InputFile is the base class of ObjectFile and SharedFile.
template <typename E>
class InputFile {
public:
  InputFile(Context<E> &ctx, MappedFile<Context<E>> *mf);
  InputFile() : filename("<internal>") {}

  virtual ~InputFile() = default;

  template<typename T> std::span<T>
  get_data(Context<E> &ctx, const ElfShdr<E> &shdr);

  template<typename T> std::span<T>
  get_data(Context<E> &ctx, i64 idx);

  std::string_view get_string(Context<E> &ctx, const ElfShdr<E> &shdr);
  std::string_view get_string(Context<E> &ctx, i64 idx);

  ElfEhdr<E> &get_ehdr() { return *(ElfEhdr<E> *)mf->data; }
  ElfPhdr<E> *get_phdr() { return (ElfPhdr<E> *)(mf->data + get_ehdr().e_phoff); }

  ElfShdr<E> *find_section(i64 type);

  virtual void resolve_symbols(Context<E> &ctx) = 0;
  virtual void clear_symbols();

  virtual void
  mark_live_objects(Context<E> &ctx,
                    std::function<void(InputFile<E> *)> feeder) = 0;

  std::span<Symbol<E> *> get_global_syms();

  MappedFile<Context<E>> *mf = nullptr;
  std::span<ElfShdr<E>> elf_sections;
  std::span<ElfSym<E>> elf_syms;
  std::vector<Symbol<E> *> symbols;
  i64 first_global = 0;

  std::string filename;
  bool is_dso = false;
  u32 priority;
  std::atomic_bool is_alive = false;
  std::string_view shstrtab;

  // To create an output .symtab
  u64 local_symtab_idx = 0;
  u64 global_symtab_idx = 0;
  u64 num_local_symtab = 0;
  u64 num_global_symtab = 0;
  u64 strtab_offset = 0;
  u64 strtab_size = 0;

  // For --emit-relocs
  std::vector<i32> output_sym_indices;

protected:
  std::unique_ptr<Symbol<E>[]> local_syms;
};

// ObjectFile represents an input .o file.
template <typename E>
class ObjectFile : public InputFile<E> {
public:
  ObjectFile() = default;

  static ObjectFile<E> *create(Context<E> &ctx, MappedFile<Context<E>> *mf,
                               std::string archive_name, bool is_in_lib);

  void parse(Context<E> &ctx);
  void register_section_pieces(Context<E> &ctx);
  void resolve_symbols(Context<E> &ctx) override;
  void mark_live_objects(Context<E> &ctx,
                         std::function<void(InputFile<E> *)> feeder) override;
  void convert_undefined_weak_symbols(Context<E> &ctx);
  void resolve_comdat_groups();
  void eliminate_duplicate_comdat_groups();
  void claim_unresolved_symbols(Context<E> &ctx);
  void scan_relocations(Context<E> &ctx);
  void convert_common_symbols(Context<E> &ctx);
  void compute_symtab(Context<E> &ctx);
  void write_symtab(Context<E> &ctx);

  i64 get_shndx(const ElfSym<E> &esym);
  InputSection<E> *get_section(const ElfSym<E> &esym);

  std::string archive_name;
  std::vector<std::unique_ptr<InputSection<E>>> sections;
  std::vector<InputSectionExtra<E>> extras;
  std::vector<std::unique_ptr<MergeableSection<E>>> mergeable_sections;
  bool is_in_lib = false;
  std::vector<ElfShdr<E>> elf_sections2;
  std::vector<CieRecord<E>> cies;
  std::vector<FdeRecord<E>> fdes;
  std::vector<const char *> symvers;
  std::vector<SectionFragmentRef<E>> sym_fragments;
  std::vector<std::pair<ComdatGroup *, std::span<u32>>> comdat_groups;
  bool exclude_libs = false;
  u32 features = 0;
  bool is_lto_obj = false;

  u64 num_dynrel = 0;
  u64 reldyn_offset = 0;

  u64 fde_idx = 0;
  u64 fde_offset = 0;
  u64 fde_size = 0;

  // For ARM64 range extension thunks
  std::vector<std::vector<RangeExtensionRef>> range_extn;

  // For RISC-V section resizing
  std::vector<std::vector<i32>> r_deltas;
  std::vector<std::vector<ElfRel<E>>> sorted_rels;
  std::vector<std::vector<Symbol<E> *>> sorted_symbols;

private:
  ObjectFile(Context<E> &ctx, MappedFile<Context<E>> *mf,
             std::string archive_name, bool is_in_lib);

  void initialize_sections(Context<E> &ctx);
  void initialize_symbols(Context<E> &ctx);
  void sort_relocations(Context<E> &ctx);
  void initialize_mergeable_sections(Context<E> &ctx);
  void initialize_ehframe_sections(Context<E> &ctx);
  u32 read_note_gnu_property(Context<E> &ctx, const ElfShdr<E> &shdr);
  void read_ehframe(Context<E> &ctx, InputSection<E> &isec);
  void override_symbol(Context<E> &ctx, Symbol<E> &sym,
                       const ElfSym<E> &esym, i64 symidx);
  void merge_visibility(Context<E> &ctx, Symbol<E> &sym, u8 visibility);

  std::pair<std::string_view, const ElfShdr<E> *>
  uncompress_contents(Context<E> &ctx, const ElfShdr<E> &shdr,
                      std::string_view name);

  bool has_common_symbol = false;

  std::string_view symbol_strtab;
  const ElfShdr<E> *symtab_sec;
  std::span<u32> symtab_shndx_sec;
};

// SharedFile represents an input .so file.
template <typename E>
class SharedFile : public InputFile<E> {
public:
  static SharedFile<E> *create(Context<E> &ctx, MappedFile<Context<E>> *mf);

  void parse(Context<E> &ctx);
  void resolve_symbols(Context<E> &ctx) override;
  std::vector<Symbol<E> *> find_aliases(Symbol<E> *sym);
  bool is_readonly(Context<E> &ctx, Symbol<E> *sym);

  void mark_live_objects(Context<E> &ctx,
                         std::function<void(InputFile<E> *)> feeder) override;

  void compute_symtab(Context<E> &ctx);
  void write_symtab(Context<E> &ctx);

  std::string soname;
  std::vector<std::string_view> version_strings;
  std::vector<ElfSym<E>> elf_syms2;

private:
  SharedFile(Context<E> &ctx, MappedFile<Context<E>> *mf);

  std::string get_soname(Context<E> &ctx);
  void maybe_override_symbol(Symbol<E> &sym, const ElfSym<E> &esym);
  std::vector<std::string_view> read_verdef(Context<E> &ctx);

  std::vector<u16> versyms;
  std::string_view symbol_strtab;
  const ElfShdr<E> *symtab_sec;
};

//
// linker-script.cc
//

template <typename E>
void parse_linker_script(Context<E> &ctx, MappedFile<Context<E>> *mf);

template <typename E>
i64 get_script_output_type(Context<E> &ctx, MappedFile<Context<E>> *mf);

template <typename E>
void parse_version_script(Context<E> &ctx, std::string path);

template <typename E>
void parse_dynamic_list(Context<E> &ctx, std::string path);

//
// glob.cc
//

class GlobPattern {
  typedef enum { STRING, STAR, QUESTION, BRACKET } Kind;

  struct Element {
    Element(Kind k) : kind(k) {}
    Kind kind;
    std::string str;
    std::vector<bool> bitmap;
  };

public:
  static std::optional<GlobPattern> compile(std::string_view pat);
  bool match(std::string_view str);

private:
  GlobPattern(std::vector<Element> &&vec) : elements(vec) {}
  static bool do_match(std::string_view str, std::span<Element> elements);

  std::vector<Element> elements;
};

//
// version-matcher.cc
//

class VersionMatcher {
public:
  bool add(std::string_view pat, u16 ver);
  bool empty() const { return !root && globs.empty(); }
  std::optional<u16> find(std::string_view str);

private:
  struct TrieNode {
    u32 value = -1;
    TrieNode *suffix_link = nullptr;
    std::unique_ptr<TrieNode> children[256];
  };

  void compile();
  void fix_suffix_links(TrieNode &node);
  void fix_values();

  std::vector<std::string> strings;
  std::unique_ptr<TrieNode> root;
  std::vector<std::pair<GlobPattern, u32>> globs;
  std::vector<u16> versions;
  std::once_flag once_flag;
  bool compiled = false;
};

//
// lto.cc
//

template <typename E>
ObjectFile<E> *read_lto_object(Context<E> &ctx, MappedFile<Context<E>> *mb);

template <typename E>
std::vector<ObjectFile<E> *> do_lto(Context<E> &ctx);

template <typename E>
void lto_cleanup(Context<E> &ctx);

//
// gc-sections.cc
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

template <typename E> void apply_exclude_libs(Context<E> &);
template <typename E> void create_synthetic_sections(Context<E> &);
template <typename E> void set_file_priority(Context<E> &);
template <typename E> void resolve_symbols(Context<E> &);
template <typename E> void register_section_pieces(Context<E> &);
template <typename E> void eliminate_comdats(Context<E> &);
template <typename E> void convert_common_symbols(Context<E> &);
template <typename E> void compute_merged_section_sizes(Context<E> &);
template <typename E> void bin_sections(Context<E> &);
template <typename E> ObjectFile<E> *create_internal_file(Context<E> &);
template <typename E> void check_cet_errors(Context<E> &);
template <typename E> void print_dependencies(Context<E> &);
template <typename E> void print_dependencies(Context<E> &);
template <typename E> void print_dependencies_full(Context<E> &);
template <typename E> void write_repro_file(Context<E> &);
template <typename E> void check_duplicate_symbols(Context<E> &);
template <typename E> void sort_init_fini(Context<E> &);
template <typename E> void shuffle_sections(Context<E> &);
template <typename E> std::vector<Chunk<E> *>
collect_output_sections(Context<E> &);
template <typename E> void compute_section_sizes(Context<E> &);
template <typename E> void claim_unresolved_symbols(Context<E> &);
template <typename E> void scan_rels(Context<E> &);
template <typename E> void construct_relr(Context<E> &);
template <typename E> void create_output_symtab(Context<E> &);
template <typename E> void create_reloc_sections(Context<E> &);
template <typename E> void apply_version_script(Context<E> &);
template <typename E> void parse_symbol_version(Context<E> &);
template <typename E> void compute_import_export(Context<E> &);
template <typename E> void clear_padding(Context<E> &);
template <typename E> i64 get_section_rank(Context<E> &, Chunk<E> *chunk);
template <typename E> i64 set_osec_offsets(Context<E> &);
template <typename E> void fix_synthetic_symbols(Context<E> &);
template <typename E> i64 compress_debug_sections(Context<E> &);
template <typename E> void write_dependency_file(Context<E> &);

//
// arch-arm64.cc
//

i64 create_range_extension_thunks(Context<ARM64> &ctx);
void write_thunks(Context<ARM64> &ctx);

//
// arch-riscv64.cc
//

i64 riscv_resize_sections(Context<RISCV64> &ctx);

//
// output-file.cc
//

template <typename E>
class OutputFile {
public:
  static std::unique_ptr<OutputFile<E>>
  open(Context<E> &ctx, std::string path, i64 filesize, i64 perm);

  virtual void close(Context<E> &ctx) = 0;
  virtual ~OutputFile() = default;

  u8 *buf = nullptr;
  std::string path;
  i64 filesize;
  bool is_mmapped;
  bool is_unmapped = false;

protected:
  OutputFile(std::string path, i64 filesize, bool is_mmapped)
    : path(path), filesize(filesize), is_mmapped(is_mmapped) {}
};

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

typedef enum {
  UNRESOLVED_ERROR,
  UNRESOLVED_WARN,
  UNRESOLVED_IGNORE,
} UnresolvedKind;

typedef enum {
  SEPARATE_LOADABLE_SEGMENTS,
  SEPARATE_CODE,
  NOSEPARATE_CODE,
} SeparateCodeKind;

typedef enum {
  CET_REPORT_NONE,
  CET_REPORT_WARNING,
  CET_REPORT_ERROR,
} CetReportKind;

typedef enum {
  SHUFFLE_SECTIONS_NONE,
  SHUFFLE_SECTIONS_SHUFFLE,
  SHUFFLE_SECTIONS_REVERSE,
} ShuffleSectionsKind;

struct VersionPattern {
  std::string_view pattern;
  u16 ver_idx = -1;
  bool is_cpp = false;
};

template <typename E, typename T>
class FileCache {
public:
  void store(MappedFile<Context<E>> *mf, T *obj) {
    Key k(mf->name, mf->size, mf->mtime);
    cache[k].push_back(obj);
  }

  std::vector<T *> get(MappedFile<Context<E>> *mf) {
    Key k(mf->name, mf->size, mf->mtime);
    std::vector<T *> objs = cache[k];
    cache[k].clear();
    return objs;
  }

  T *get_one(MappedFile<Context<E>> *mf) {
    std::vector<T *> objs = get(mf);
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

  void checkpoint() {
    if (has_error) {
      cleanup();
      _exit(1);
    }
  }

  // Command-line arguments
  struct {
    BuildId build_id;
    CetReportKind z_cet_report = CET_REPORT_NONE;
    CompressKind compress_debug_sections = COMPRESS_NONE;
    SeparateCodeKind z_separate_code = SEPARATE_LOADABLE_SEGMENTS;
    ShuffleSectionsKind shuffle_sections = SHUFFLE_SECTIONS_NONE;
    UnresolvedKind unresolved_symbols = UNRESOLVED_ERROR;
    bool Bsymbolic = false;
    bool Bsymbolic_functions = false;
    bool allow_multiple_definition = false;
    bool color_diagnostics = false;
    bool default_symver = false;
    bool demangle = true;
    bool discard_all = false;
    bool discard_locals = false;
    bool eh_frame_hdr = true;
    bool emit_relocs = false;
    bool export_dynamic = false;
    bool fatal_warnings = false;
    bool fork = true;
    bool gc_sections = false;
    bool hash_style_gnu = false;
    bool hash_style_sysv = true;
    bool icf = false;
    bool is_static = false;
    bool lto_pass2 = false;
    bool noinhibit_exec = false;
    bool omagic = false;
    bool pack_dyn_relocs_relr = false;
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
    bool warn_once = false;
    bool warn_textrel = false;
    bool z_copyreloc = true;
    bool z_defs = false;
    bool z_delete = true;
    bool z_dlopen = true;
    bool z_dump = true;
    bool z_execstack = false;
    bool z_ibt = false;
    bool z_ibtplt = false;
    bool z_initfirst = false;
    bool z_interpose = false;
    bool z_keep_text_section_prefix = false;
    bool z_nodefaultlib = false;
    bool z_now = false;
    bool z_origin = false;
    bool z_relro = true;
    bool z_shstk = false;
    bool z_text = false;
    i64 emulation = -1;
    i64 filler = -1;
    i64 print_dependencies = 0;
    i64 spare_dynamic_tags = 5;
    i64 thread_count = 0;
    std::optional<GlobPattern> unique;
    std::optional<u64> shuffle_sections_seed;
    std::string Map;
    std::string chroot;
    std::string dependency_file;
    std::string directory;
    std::string dynamic_linker;
    std::string entry = "_start";
    std::string fini = "_fini";
    std::string init = "_init";
    std::string output;
    std::string plugin;
    std::string rpaths;
    std::string soname;
    std::string sysroot;
    std::unique_ptr<std::unordered_set<std::string_view>> retain_symbols_file;
    std::unordered_set<std::string_view> ignore_ir_file;
    std::unordered_set<std::string_view> wrap;
    std::vector<std::pair<Symbol<E> *, std::variant<Symbol<E> *, u64>>> defsyms;
    std::vector<std::string> library_paths;
    std::vector<std::string> plugin_opt;
    std::vector<std::string> version_definitions;
    std::vector<std::string_view> auxiliary;
    std::vector<std::string_view> exclude_libs;
    std::vector<std::string_view> filter;
    std::vector<std::string_view> require_defined;
    std::vector<std::string_view> trace_symbol;
    std::vector<std::string_view> undefined;
    u64 image_base = 0x200000;
  } arg;

  std::vector<VersionPattern> version_patterns;
  u16 default_version = VER_NDX_GLOBAL;
  i64 page_size = -1;
  i64 plt_hdr_size = -1;
  i64 plt_size = -1;

  // Reader context
  bool as_needed = false;
  bool whole_archive = false;
  bool is_static;
  bool in_lib = false;
  i64 file_priority = 10000;
  std::unordered_set<std::string_view> visited;
  tbb::task_group tg;

  bool has_error = false;
  bool has_lto_object = false;

  // Symbol table
  tbb::concurrent_hash_map<std::string_view, Symbol<E>, HashCmp> symbol_map;
  tbb::concurrent_hash_map<std::string_view, ComdatGroup, HashCmp> comdat_groups;
  tbb::concurrent_vector<std::unique_ptr<MergedSection<E>>> merged_sections;
  tbb::concurrent_vector<std::unique_ptr<Chunk<E>>> output_chunks;
  std::vector<std::unique_ptr<OutputSection<E>>> output_sections;
  FileCache<E, ObjectFile<E>> obj_cache;
  FileCache<E, SharedFile<E>> dso_cache;

  tbb::concurrent_vector<std::unique_ptr<TimerRecord>> timer_records;
  tbb::concurrent_vector<std::function<void()>> on_exit;

  tbb::concurrent_vector<std::unique_ptr<ObjectFile<E>>> obj_pool;
  tbb::concurrent_vector<std::unique_ptr<SharedFile<E>>> dso_pool;
  tbb::concurrent_vector<std::unique_ptr<u8[]>> string_pool;
  tbb::concurrent_vector<std::unique_ptr<MappedFile<Context<E>>>> mf_pool;
  tbb::concurrent_vector<std::vector<ElfRel<E>>> rel_pool;

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

  std::vector<Chunk<E> *> chunks;
  std::atomic_bool needs_tlsld = false;
  std::atomic_bool has_gottp_rel = false;
  std::atomic_bool has_textrel = false;

  // For --warn-once
  tbb::concurrent_hash_map<void *, int> warned;

  // Output chunks
  std::unique_ptr<OutputEhdr<E>> ehdr;
  std::unique_ptr<OutputShdr<E>> shdr;
  std::unique_ptr<OutputPhdr<E>> phdr;
  std::unique_ptr<InterpSection<E>> interp;
  std::unique_ptr<GotSection<E>> got;
  std::unique_ptr<GotPltSection<E>> gotplt;
  std::unique_ptr<RelPltSection<E>> relplt;
  std::unique_ptr<RelDynSection<E>> reldyn;
  std::unique_ptr<RelrDynSection<E>> relrdyn;
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
  std::unique_ptr<CopyrelSection<E>> copyrel;
  std::unique_ptr<CopyrelSection<E>> copyrel_relro;
  std::unique_ptr<VersymSection<E>> versym;
  std::unique_ptr<VerneedSection<E>> verneed;
  std::unique_ptr<VerdefSection<E>> verdef;
  std::unique_ptr<BuildIdSection<E>> buildid;
  std::unique_ptr<NotePropertySection<E>> note_property;

  // For --relocatable
  std::vector<RChunk<E> *> r_chunks;
  ROutputEhdr<E> *r_ehdr = nullptr;
  ROutputShdr<E> *r_shdr = nullptr;
  RStrtabSection<E> *r_shstrtab = nullptr;
  RStrtabSection<E> *r_strtab = nullptr;
  RSymtabSection<E> *r_symtab = nullptr;

  u64 tls_begin = -1;
  u64 tls_end = -1;
  bool relax_tlsdesc = false;

  // Linker-synthesized symbols
  Symbol<E> *_DYNAMIC = nullptr;
  Symbol<E> *_GLOBAL_OFFSET_TABLE_ = nullptr;
  Symbol<E> *__GNU_EH_FRAME_HDR = nullptr;
  Symbol<E> *__bss_start = nullptr;
  Symbol<E> *__ehdr_start = nullptr;
  Symbol<E> *__executable_start = nullptr;
  Symbol<E> *__fini_array_end = nullptr;
  Symbol<E> *__fini_array_start = nullptr;
  Symbol<E> *__global_pointer = nullptr;
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
MappedFile<Context<E>> *open_library(Context<E> &ctx, std::string path);

template <typename E>
MappedFile<Context<E>> *find_library(Context<E> &ctx, std::string path);

template <typename E>
void read_file(Context<E> &ctx, MappedFile<Context<E>> *mf);

int main(int argc, char **argv);

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
  NEEDS_COPYREL  = 1 << 4,
  NEEDS_TLSDESC  = 1 << 5,
  NEEDS_THUNK    = 1 << 6,
};

// A struct to hold taret-dependent symbol members;
template <typename E>
struct SymbolExtras {};

template <>
struct SymbolExtras<ARM64> {
  // For range extension thunks
  i32 thunk_idx = -1;
  i32 thunk_sym_idx = -1;
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

  u64 get_addr(Context<E> &ctx, bool allow_plt = true) const;
  u64 get_got_addr(Context<E> &ctx) const;
  u64 get_gotplt_addr(Context<E> &ctx) const;
  u64 get_gottp_addr(Context<E> &ctx) const;
  u64 get_tlsgd_addr(Context<E> &ctx) const;
  u64 get_tlsdesc_addr(Context<E> &ctx) const;
  u64 get_plt_addr(Context<E> &ctx) const;

  void set_got_idx(Context<E> &ctx, i32 idx);
  void set_gotplt_idx(Context<E> &ctx, i32 idx);
  void set_gottp_idx(Context<E> &ctx, i32 idx);
  void set_tlsgd_idx(Context<E> &ctx, i32 idx);
  void set_tlsdesc_idx(Context<E> &ctx, i32 idx);
  void set_plt_idx(Context<E> &ctx, i32 idx);
  void set_pltgot_idx(Context<E> &ctx, i32 idx);
  void set_dynsym_idx(Context<E> &ctx, i32 idx);

  i32 get_got_idx(Context<E> &ctx) const;
  i32 get_gotplt_idx(Context<E> &ctx) const;
  i32 get_gottp_idx(Context<E> &ctx) const;
  i32 get_tlsgd_idx(Context<E> &ctx) const;
  i32 get_tlsdesc_idx(Context<E> &ctx) const;
  i32 get_plt_idx(Context<E> &ctx) const;
  i32 get_pltgot_idx(Context<E> &ctx) const;
  i32 get_dynsym_idx(Context<E> &ctx) const;

  bool has_plt(Context<E> &ctx) const;
  bool has_got(Context<E> &ctx) const;

  bool is_absolute() const;
  bool is_relative() const;

  InputSection<E> *get_input_section() const;
  u32 get_type() const;
  std::string_view get_version() const;
  const ElfSym<E> &esym() const;
  SectionFragment<E> *get_frag() const;
  std::string_view name() const;

  // A symbol is owned by a file. If two or more files define the
  // same symbol, the one with the strongest definition owns the symbol.
  // If `file` is null, the symbol is equivalent to nonexistent.
  InputFile<E> *file = nullptr;

  u64 value = 0;

  const char *nameptr = nullptr;
  u64 namelen : 20 = 0;

  // Index into the symbol table of the owner file.
  i64 sym_idx : 20 = -1;

  // shndx > 0  : symbol is in file's shndx'th section
  // shndx == 0 : absolute symbol
  // shndx < 0  : symbol is in the -shndx'th output section
  i64 shndx : 20 = 0;

  i32 aux_idx = -1;
  u16 ver_idx = 0;

  // `flags` has NEEDS_ flags.
  std::atomic_uint8_t flags = 0;

  tbb::spin_mutex mu;
  std::atomic_uint8_t visibility = STV_DEFAULT;

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
  // dynamic loader searches a dynamic symbol from an executable
  // before examining any DSOs), so any exported symbol is export-only.
  // If we are creating a DSO, export-only symbols represent a
  // protected symbol (i.e. a symbol whose visibility is STV_PROTECTED).
  u8 is_imported : 1 = false;
  u8 is_exported : 1 = false;

  // For LTO. True if the symbol is referenced by a regular object (as
  // opposed to IR object).
  u8 referenced_by_regular_obj : 1 = false;

  // Target-dependent extra members.
  [[no_unique_address]] SymbolExtras<E> extra;
};

// If we haven't seen the same `key` before, create a new instance
// of Symbol and returns it. Otherwise, returns the previously-
// instantiated object. `key` is usually the same as `name`.
template <typename E>
Symbol<E> *get_symbol(Context<E> &ctx, std::string_view key,
                      std::string_view name) {
  typename decltype(ctx.symbol_map)::const_accessor acc;
  ctx.symbol_map.insert(acc, {key, Symbol<E>(name)});
  return const_cast<Symbol<E> *>(&acc->second);
}

template <typename E>
Symbol<E> *get_symbol(Context<E> &ctx, std::string_view name) {
  return get_symbol(ctx, name, name);
}

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym) {
  if (opt_demangle)
    out << demangle(sym.name());
  else
    out << sym.name();
  return out;
}

//
// Inline objects and functions
//

template <typename E>
inline i64 FdeRecord<E>::size(ObjectFile<E> &file) const {
  return *(u32 *)(file.cies[cie_idx].contents.data() + input_offset) + 4;
}

template <typename E>
inline std::string_view FdeRecord<E>::get_contents(ObjectFile<E> &file) const {
  return file.cies[cie_idx].contents.substr(input_offset, size(file));
}

template <typename E>
inline std::span<ElfRel<E>> FdeRecord<E>::get_rels(ObjectFile<E> &file) const {
  std::span<ElfRel<E>> rels = file.cies[cie_idx].rels;
  i64 end = rel_idx;
  while (end < rels.size() && rels[end].r_offset < input_offset + size(file))
    end++;
  return rels.subspan(rel_idx, end - rel_idx);
}

template <typename E>
inline std::ostream &
operator<<(std::ostream &out, const InputSection<E> &isec) {
  out << isec.file << ":(" << isec.name() << ")";
  return out;
}

template <typename E>
inline u64 SectionFragment<E>::get_addr(Context<E> &ctx) const {
  return output_section.shdr.sh_addr + offset;
}

template <typename E>
inline void InputSection<E>::kill() {
  if (is_alive.exchange(false))
    for (FdeRecord<E> &fde : get_fdes())
      fde.is_alive = false;
}

template <typename E>
inline u64 InputSection<E>::get_addr() const {
  return output_section->shdr.sh_addr + offset;
}

template <typename E>
inline std::string_view InputSection<E>::name() const {
  if (file.elf_sections.size() <= shndx)
    return ".common";
  return file.shstrtab.data() + file.elf_sections[shndx].sh_name;
}

template <typename E>
inline i64 InputSection<E>::get_priority() const {
  return ((i64)file.priority << 32) | shndx;
}

template <typename E>
inline InputSectionExtra<E> &InputSection<E>::extra() const {
  return file.extras[shndx];
}

template <typename E>
inline i64 InputSection<E>::get_addend(const ElfRel<E> &rel) const {
  return rel.r_addend;
}

template <>
inline i64 InputSection<I386>::get_addend(const ElfRel<I386> &rel) const {
  u8 *loc = (u8 *)contents.data() + rel.r_offset;

  switch (rel.r_type) {
  case R_386_NONE:
    return 0;
  case R_386_8:
  case R_386_PC8:
    return *loc;
  case R_386_16:
  case R_386_PC16:
    return *(u16 *)loc;
  case R_386_32:
  case R_386_PC32:
  case R_386_GOT32:
  case R_386_GOT32X:
  case R_386_PLT32:
  case R_386_GOTOFF:
  case R_386_GOTPC:
  case R_386_TLS_LDM:
  case R_386_TLS_GOTIE:
  case R_386_TLS_LE:
  case R_386_TLS_IE:
  case R_386_TLS_GD:
  case R_386_TLS_LDO_32:
  case R_386_SIZE32:
  case R_386_TLS_GOTDESC:
    return *(u32 *)loc;
  }
  unreachable();
}

template <>
inline i64 InputSection<ARM32>::get_addend(const ElfRel<ARM32> &rel) const {
  u8 *loc = (u8 *)contents.data() + rel.r_offset;

  auto bit = [](u32 val, i64 pos) -> i32 {
    return (val >> pos) & 1;
  };

  // Returns [hi:lo] bits of val.
  auto bits = [](u64 val, i64 hi, i64 lo) -> i32 {
    return (val >> lo) & (((u64)1 << (hi - lo + 1)) - 1);
  };

  auto sign_extend = [](u64 val, i64 size) -> i32 {
    return (i64)(val << (63 - size)) >> (63 - size);
  };

  auto read_thm_mov_imm = [&]() -> i32 {
    u32 imm4 = bits(*(u16 *)loc, 3, 0);
    u32 i = bit(*(u16 *)loc, 10);
    u32 imm3 = bits(*(u16 *)(loc + 2), 14, 12);
    u32 imm8 = bits(*(u16 *)(loc + 2), 7, 0);
    return (imm4 << 12) | (i << 11) | (imm3 << 8) | imm8;
  };

  switch (rel.r_type) {
  case R_ARM_NONE:
    return 0;
  case R_ARM_ABS32:
  case R_ARM_REL32:
    return *(i32 *)loc;
  case R_ARM_THM_CALL: {
    u32 S = bit(*(u16 *)loc, 10);
    u32 J1 = bit(*(u16 *)(loc + 2), 13);
    u32 J2 = bit(*(u16 *)(loc + 2), 11);
    u32 I1 = !(J1 ^ S);
    u32 I2 = !(J2 ^ S);
    u32 imm10 = bits(*(u16 *)loc, 9, 0);
    u32 imm11 = bits(*(u16 *)(loc + 2), 10, 0);
    u32 val = (S << 24) | (I1 << 23) | (I2 << 22) | (imm10 << 12) | (imm11 << 1);
    return sign_extend(val, 24);
  }
  case R_ARM_BASE_PREL:
  case R_ARM_GOT_BREL:
    return *(i32 *)loc;
  case R_ARM_CALL:
  case R_ARM_JUMP24:
    return sign_extend(*(u32 *)loc & 0x00ff'ffff, 23) << 2;
  case R_ARM_PREL31:
    return sign_extend(*(u32 *)loc, 30);
  case R_ARM_THM_MOVW_ABS_NC:
    return read_thm_mov_imm();
  case R_ARM_THM_MOVT_ABS:
    return read_thm_mov_imm() << 16;
  }
  unreachable();
}

template <typename E>
inline const ElfShdr<E> &InputSection<E>::shdr() const {
  if (shndx < file.elf_sections.size())
    return file.elf_sections[shndx];
  return file.elf_sections2[shndx - file.elf_sections.size()];
}

template <typename E>
inline std::span<ElfRel<E>> InputSection<E>::get_rels(Context<E> &ctx) const {
  if (relsec_idx == -1)
    return {};
  if (!file.sorted_rels.empty() && !file.sorted_rels[shndx].empty())
    return file.sorted_rels[shndx];
  return file.template get_data<ElfRel<E>>(ctx, file.elf_sections[relsec_idx]);
}

template <typename E>
inline std::span<FdeRecord<E>> InputSection<E>::get_fdes() const {
  if (fde_begin == -1)
    return {};
  std::span<FdeRecord<E>> span(file.fdes);
  return span.subspan(fde_begin, fde_end - fde_begin);
}

template <typename E>
inline std::vector<RangeExtensionRef> &InputSection<E>::get_range_extn() const {
  return file.range_extn[shndx];
}

template <typename E>
inline std::vector<i32> &InputSection<E>::get_r_deltas() const {
  return file.r_deltas[shndx];
}

template <typename E>
inline std::span<Symbol<E> *> InputSection<E>::get_sorted_symbols() const {
  return file.sorted_symbols[shndx];
}

template <typename E>
std::pair<SectionFragment<E> *, i64>
InputSection<E>::get_fragment(Context<E> &ctx, const ElfRel<E> &rel) {
  assert(!(shdr().sh_flags & SHF_ALLOC));

  const ElfSym<E> &esym = file.elf_syms[rel.r_sym];
  if (esym.st_type != STT_SECTION)
    return {nullptr, 0};

  std::unique_ptr<MergeableSection<E>> &m =
    file.mergeable_sections[file.get_shndx(esym)];
  if (!m)
    return {nullptr, 0};

  i64 offset = esym.st_value + get_addend(rel);
  std::span<u32> offsets = m->frag_offsets;

  auto it = std::upper_bound(offsets.begin(), offsets.end(), offset);
  if (it == offsets.begin())
    Fatal(ctx) << *this << ": bad relocation at " << rel.r_sym;
  i64 idx = it - 1 - offsets.begin();
  return {m->fragments[idx], offset - offsets[idx]};
}

template <typename E>
inline bool InputSection<E>::is_relr_reloc(Context<E> &ctx, const ElfRel<E> &rel) {
  return ctx.arg.pack_dyn_relocs_relr &&
         !(shdr().sh_flags & SHF_EXECINSTR) &&
         (shdr().sh_addralign % E::word_size) == 0 &&
         (rel.r_offset % E::word_size) == 0;
}

inline u64
RangeExtensionThunk<ARM64>::get_addr(i64 idx) const {
  return output_section.shdr.sh_addr + offset + symbol_map[idx] * ENTRY_SIZE;
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
  u8 *begin = mf->data + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mf->data + mf->size < end)
    Fatal(ctx) << *this << ": section header is out of range: " << shdr.sh_offset;
  return {(char *)begin, (size_t)(end - begin)};
}

template <typename E>
inline std::string_view InputFile<E>::get_string(Context<E> &ctx, i64 idx) {
  assert(idx < elf_sections.size());

  if (elf_sections.size() <= idx)
    Fatal(ctx) << *this << ": invalid section index: " << idx;
  return this->get_string(ctx, elf_sections[idx]);
}

template <typename E>
inline std::span<Symbol<E> *> InputFile<E>::get_global_syms() {
  return std::span<Symbol<E> *>(this->symbols).subspan(this->first_global);
}

template <typename E>
inline i64 ObjectFile<E>::get_shndx(const ElfSym<E> &esym) {
  assert(&this->elf_syms[0] <= &esym);
  assert(&esym <= &this->elf_syms[this->elf_syms.size() - 1]);

  if (esym.st_shndx == SHN_XINDEX)
    return symtab_shndx_sec[&esym - &this->elf_syms[0]];
  return esym.st_shndx;
}

template <typename E>
inline InputSection<E> *ObjectFile<E>::get_section(const ElfSym<E> &esym) {
  return sections[get_shndx(esym)].get();
}

// This operator defines a total order over symbols. This is used to
// make the output deterministic.
template <typename E>
inline bool operator<(const Symbol<E> &a, const Symbol<E> &b) {
  return std::tuple{a.file->priority, a.sym_idx} <
         std::tuple{b.file->priority, b.sym_idx};
}

template <typename E>
inline u64 Symbol<E>::get_addr(Context<E> &ctx, bool allow_plt) const {
  if (file && !file->is_dso) {
    SectionFragmentRef<E> &ref = ((ObjectFile<E> *)file)->sym_fragments[sym_idx];

    if (ref.frag) {
      if (!ref.frag->is_alive) {
        // This condition is met if a non-alloc section refers an
        // alloc section and if the referenced piece of data is
        // garbage-collected. Typically, this condition occurs if a
        // debug info section refers a string constant in .rodata.
        return 0;
      }

      return ref.frag->get_addr(ctx) + ref.addend;
    }
  }

  if (has_copyrel) {
    return copyrel_readonly
      ? ctx.copyrel_relro->shdr.sh_addr + value
      : ctx.copyrel->shdr.sh_addr + value;
  }

  if (allow_plt && has_plt(ctx))
    if (is_imported || esym().st_type == STT_GNU_IFUNC)
      return get_plt_addr(ctx);

  if (InputSection<E> *isec = get_input_section()) {
    if (!isec->is_alive) {
      if (isec->name() == ".eh_frame") {
        // .eh_frame contents are parsed and reconstructed by the linker,
        // so pointing to a specific location in a source .eh_frame
        // section doesn't make much sense. However, CRT files contain
        // symbols pointing to the very beginning and ending of the section.
        if (name() == "__EH_FRAME_BEGIN__" || name() == "__EH_FRAME_LIST__" ||
            esym().st_type == STT_SECTION)
          return ctx.eh_frame->shdr.sh_addr;

        if (name() == "__FRAME_END__" || name() == "__EH_FRAME_LIST_END__")
          return ctx.eh_frame->shdr.sh_addr + ctx.eh_frame->shdr.sh_size;

        // ARM object files contain "$d" local symbol at the beginning
        // of data sections. Their values are not significant for .eh_frame,
        // so we just treat them as offset 0.
        if (name() == "$d" || name().starts_with("$d."))
          return ctx.eh_frame->shdr.sh_addr;

        Fatal(ctx) << "symbol referring .eh_frame is not supported: "
                   << *this << " " << *file;
      }

      // The control can reach here if there's a relocation that refers
      // a local symbol belonging to a comdat group section. This is a
      // violation of the spec, as all relocations should use only global
      // symbols of comdat members. However, .eh_frame tends to have such
      // relocations.
      return 0;
    }

    return isec->get_addr() + value;
  }

  return value;
}

template <typename E>
inline u64 Symbol<E>::get_got_addr(Context<E> &ctx) const {
  return ctx.got->shdr.sh_addr + get_got_idx(ctx) * E::word_size;
}

template <typename E>
inline u64 Symbol<E>::get_gotplt_addr(Context<E> &ctx) const {
  assert(get_gotplt_idx(ctx) != -1);
  return ctx.gotplt->shdr.sh_addr + get_gotplt_idx(ctx) * E::word_size;
}

template <typename E>
inline u64 Symbol<E>::get_gottp_addr(Context<E> &ctx) const {
  assert(get_gottp_idx(ctx) != -1);
  return ctx.got->shdr.sh_addr + get_gottp_idx(ctx) * E::word_size;
}

template <typename E>
inline u64 Symbol<E>::get_tlsgd_addr(Context<E> &ctx) const {
  assert(get_tlsgd_idx(ctx) != -1);
  return ctx.got->shdr.sh_addr + get_tlsgd_idx(ctx) * E::word_size;
}

template <typename E>
inline u64 Symbol<E>::get_tlsdesc_addr(Context<E> &ctx) const {
  assert(get_tlsdesc_idx(ctx) != -1);
  return ctx.got->shdr.sh_addr + get_tlsdesc_idx(ctx) * E::word_size;
}

template <typename E>
inline u64 Symbol<E>::get_plt_addr(Context<E> &ctx) const {
  if (i32 idx = get_plt_idx(ctx); idx != -1)
    return ctx.plt->shdr.sh_addr + ctx.plt_hdr_size + idx * ctx.plt_size;
  return ctx.pltgot->shdr.sh_addr + get_pltgot_idx(ctx) * E::pltgot_size;
}

template <typename E>
inline void Symbol<E>::set_got_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].got_idx < 0);
  ctx.symbol_aux[aux_idx].got_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_gotplt_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].gotplt_idx < 0);
  ctx.symbol_aux[aux_idx].gotplt_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_gottp_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].gottp_idx < 0);
  ctx.symbol_aux[aux_idx].gottp_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_tlsgd_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].tlsgd_idx < 0);
  ctx.symbol_aux[aux_idx].tlsgd_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_tlsdesc_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].tlsdesc_idx < 0);
  ctx.symbol_aux[aux_idx].tlsdesc_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_plt_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].plt_idx < 0);
  ctx.symbol_aux[aux_idx].plt_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_pltgot_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].pltgot_idx < 0);
  ctx.symbol_aux[aux_idx].pltgot_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_dynsym_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].dynsym_idx < 0);
  ctx.symbol_aux[aux_idx].dynsym_idx = idx;
}

template <typename E>
inline i32 Symbol<E>::get_got_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].got_idx;
}

template <typename E>
inline i32 Symbol<E>::get_gotplt_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].gotplt_idx;
}

template <typename E>
inline i32 Symbol<E>::get_gottp_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].gottp_idx;
}

template <typename E>
inline i32 Symbol<E>::get_tlsgd_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].tlsgd_idx;
}

template <typename E>
inline i32 Symbol<E>::get_tlsdesc_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].tlsdesc_idx;
}

template <typename E>
inline i32 Symbol<E>::get_plt_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].plt_idx;
}

template <typename E>
inline i32 Symbol<E>::get_pltgot_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].pltgot_idx;
}

template <typename E>
inline i32 Symbol<E>::get_dynsym_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].dynsym_idx;
}

template <typename E>
inline bool Symbol<E>::has_plt(Context<E> &ctx) const {
  return get_plt_idx(ctx) != -1 || get_pltgot_idx(ctx) != -1;
}

template <typename E>
inline bool Symbol<E>::has_got(Context<E> &ctx) const {
  return get_got_idx(ctx) != -1;
}

template <typename E>
inline bool Symbol<E>::is_absolute() const {
  if (file->is_dso)
    return esym().is_abs();
  return !is_imported && !get_frag() && shndx == 0;
}

template <typename E>
inline bool Symbol<E>::is_relative() const {
  return !is_absolute();
}

template <typename E>
inline InputSection<E> *Symbol<E>::get_input_section() const {
  if (shndx > 0) {
    assert(!file->is_dso);
    return ((ObjectFile<E> *)file)->sections[shndx].get();
  }
  return nullptr;
}

template <typename E>
inline u32 Symbol<E>::get_type() const {
  if (esym().st_type == STT_GNU_IFUNC && file->is_dso)
    return STT_FUNC;
  return esym().st_type;
}

template <typename E>
inline std::string_view Symbol<E>::get_version() const {
  if (file->is_dso)
    return ((SharedFile<E> *)file)->version_strings[ver_idx];
  return "";
}

template <typename E>
inline const ElfSym<E> &Symbol<E>::esym() const {
  return file->elf_syms[sym_idx];
}

template <typename E>
inline SectionFragment<E> *Symbol<E>::get_frag() const {
  if (!file || file->is_dso)
    return nullptr;
  return ((ObjectFile<E> *)file)->sym_fragments[sym_idx].frag;
}

template <typename E>
inline std::string_view Symbol<E>::name() const {
  return {nameptr, (size_t)namelen};
}

} // namespace mold::elf
