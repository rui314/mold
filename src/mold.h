#pragma once

#include "../lib/common.h"
#include "elf.h"
#include "mold-git-hash.h"

#include <cassert>
#include <cstdint>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <tbb/concurrent_hash_map.h>
#include <tbb/concurrent_vector.h>
#include <tbb/spin_mutex.h>
#include <tbb/task_group.h>
#include <type_traits>
#include <unordered_map>
#include <unordered_set>
#include <variant>
#include <vector>

#ifndef _WIN32
# include <unistd.h>
#endif

namespace mold {

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
template <typename E> class MergeableSection;
template <typename E> class RelocSection;

struct ReaderContext;

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym);

extern std::string mold_version;

//
// Mergeable section fragments
//

template <typename E>
struct __attribute__((aligned(4))) SectionFragment {
  SectionFragment(MergedSection<E> *sec, bool is_alive)
    : output_section(*sec), is_alive(is_alive) {}

  u64 get_addr(Context<E> &ctx) const {
    return output_section.shdr.sh_addr + offset;
  }

  MergedSection<E> &output_section;
  i64 offset = -1;
  Atomic<u8> p2align = 0;
  Atomic<bool> is_alive = false;

  // True if this fragment must be placed within 2^32 bytes from the
  // start of the output section.
  Atomic<bool> is_32bit = false;
};

// Additional class members for dynamic symbols. Because most symbols
// don't need them and we allocate tens of millions of symbol objects
// for large programs, we separate them from `Symbol` class to save
// memory.
template <typename E>
struct SymbolAux {
  i32 got_idx = -1;
  i32 gottp_idx = -1;
  i32 tlsgd_idx = -1;
  i32 tlsdesc_idx = -1;
  i32 plt_idx = -1;
  i32 pltgot_idx = -1;
  i32 dynsym_idx = -1;
  i32 opd_idx = -1;
  u32 djb_hash = 0;

  // For range extension thunks
  std::vector<u64> thunk_addrs;
};

//
// thunks.cc
//

template <typename E>
class Thunk {};

template <needs_thunk E>
class Thunk<E> {
public:
  Thunk(OutputSection<E> &osec, i64 offset)
    : output_section(osec), offset(offset) {}

  i64 size() const { return E::thunk_hdr_size + symbols.size() * E::thunk_size; }
  void copy_buf(Context<E> &ctx);

  u64 get_addr() const {
    return output_section.shdr.sh_addr + offset;
  }

  u64 get_addr(i64 i) const {
    return get_addr() + E::thunk_hdr_size + E::thunk_size * i;
  }

  OutputSection<E> &output_section;
  i64 offset;
  std::vector<Symbol<E> *> symbols;
  std::string name;
};

template <needs_thunk E>
static consteval i64 get_branch_distance() {
  // ARM64's branch has 26 bits immediate. The immediate is padded with
  // implicit two-bit zeros because all instructions are 4 bytes aligned
  // and therefore the least two bits are always zero. So the branch
  // operand is effectively 28 bits long. That means the branch range is
  // [-2^27, 2^27) or PC ± 128 MiB.
  if (is_arm64<E>)
    return 1 << 27;

  // ARM32's Thumb branch has 24 bits immediate, and the instructions are
  // aligned to 2, so it's effectively 25 bits. It's [-2^24, 2^24) or PC ±
  // 16 MiB.
  //
  // ARM32's non-Thumb branches have twice longer range than its Thumb
  // counterparts, but we conservatively use the Thumb's limitation.
  if (is_arm32<E>)
    return 1 << 24;

  // PPC's branch has 24 bits immediate, and the instructions are aligned
  // to 4, therefore the reach is [-2^25, 2^25) or PC ± 32 MiB.
  assert(is_ppc<E>);
  return 1 << 25;
}

// The maximum distance of branch instructions used for function calls.
//
// The exact origin for computing a destination varies slightly depending
// on the target architecture. For example, ARM32's B instruction jumps to
// the branch's address + immediate + 4 (i.e., B with offset 0 jumps to
// the next instruction), while RISC-V has no such implicit bias. Here, we
// subtract 32 as a safety margin that is large enough for all targets.
template <needs_thunk E>
static constexpr i64 branch_distance = get_branch_distance<E>() - 32;

template <needs_thunk E>
void remove_redundant_thunks(Context<E> &ctx);

template <needs_thunk E>
void gather_thunk_addresses(Context<E> &ctx);

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
    return *(U32<E> *)(contents.data() + input_offset) + 4;
  }

  std::string_view get_contents() const {
    return contents.substr(input_offset, size());
  }

  std::span<ElfRel<E>> get_rels() const {
    i64 end = input_offset + size();
    i64 i = rel_idx;
    while (i < rels.size() && rels[i].r_offset < end)
      i++;
    return rels.subspan(rel_idx, i - rel_idx);
  }

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
bool cie_equals(const CieRecord<E> &a, const CieRecord<E> &b);

template <typename E>
struct FdeRecord {
  FdeRecord(u32 input_offset, u32 rel_idx)
    : input_offset(input_offset), rel_idx(rel_idx) {}

  i64 size(ObjectFile<E> &file) const {
    return *(U32<E> *)(file.cies[cie_idx].contents.data() + input_offset) + 4;
  }

  std::string_view get_contents(ObjectFile<E> &file) const {
    return file.cies[cie_idx].contents.substr(input_offset, size(file));
  }

  std::span<ElfRel<E>> get_rels(ObjectFile<E> &file) const {
    std::span<ElfRel<E>> rels = file.cies[cie_idx].rels;
    i64 end = input_offset + size(file);
    i64 i = rel_idx;
    while (i < rels.size() && rels[i].r_offset < end)
      i++;
    return rels.subspan(rel_idx, i - rel_idx);
  }

  u32 input_offset = -1;
  u32 output_offset = -1;
  u32 rel_idx = -1;
  u16 cie_idx = -1;
  Atomic<bool> is_alive = true;
};

// A struct to hold target-dependent input section members.
template <typename E>
struct InputSectionExtras {};

template <is_arm32 E>
struct InputSectionExtras<E> {
  InputSection<E> *exidx = nullptr;
};

struct RelocDelta {
  u64 offset;
  i64 delta;
};

// RISC-V and LoongArch support code-shrinking linker relaxation.
//
// r_deltas is used to manage the locations where instructions are removed
// from a section. r_deltas is sorted by offset. Each RelocDelta indicates
// that the contents at and after `offset` and up to the next RelocDelta
// offset need to be shifted towards the beginning of the section by
// `delta` bytes when copying section contents to the output buffer.
//
// Since code-shrinking relaxation never bloats section contents, `delta`
// increases monotonically within the vector as well.
template <typename E> requires is_riscv<E> || is_loongarch<E>
struct InputSectionExtras<E> {
  std::vector<RelocDelta> r_deltas;
};

// InputSection represents a section in an input object file.
template <typename E>
class __attribute__((aligned(4))) InputSection {
public:
  InputSection(Context<E> &ctx, ObjectFile<E> &file, i64 shndx);

  void uncompress(Context<E> &ctx);
  void copy_contents_to(Context<E> &ctx, u8 *buf, i64 sz);
  void scan_relocations(Context<E> &ctx);
  void write_to(Context<E> &ctx, u8 *buf);
  void apply_reloc_alloc(Context<E> &ctx, u8 *base);
  void apply_reloc_nonalloc(Context<E> &ctx, u8 *base);
  void kill();

  std::string_view name() const;
  i64 get_priority() const;
  u64 get_addr() const;
  const ElfShdr<E> &shdr() const;
  std::span<ElfRel<E>> get_rels(Context<E> &ctx) const;
  std::span<FdeRecord<E>> get_fdes() const;
  std::string_view get_func_name(Context<E> &ctx, i64 offset) const;
  bool is_relr_reloc(Context<E> &ctx, const ElfRel<E> &rel) const;
  bool icf_removed() const;
  bool record_undef_error(Context<E> &ctx, const ElfRel<E> &rel);
  void check_range(Context<E> &ctx, i64 i, i64 val, i64 lo, i64 hi);

  std::pair<SectionFragment<E> *, i64>
  get_fragment(Context<E> &ctx, const ElfRel<E> &rel);

  ObjectFile<E> &file;
  OutputSection<E> *output_section = nullptr;
  i64 sh_size = -1;

  std::string_view contents;

  i32 fde_begin = -1;
  i32 fde_end = -1;

  i64 offset = -1;
  i32 shndx = -1;
  i32 relsec_idx = -1;
  i32 reldyn_offset = 0;

  bool uncompressed = false;

  // For COMDAT de-duplication and garbage collection
  Atomic<bool> is_alive = true;
  u8 p2align = 0;

  // For ICF
  Atomic<bool> address_taken = false;

  // For garbage collection
  Atomic<bool> is_visited = false;

  // For ICF
  //
  // `leader` is the section that this section has been merged with.
  // Three kind of values are possible:
  // - `leader == nullptr`: This section was not eligible for ICF.
  // - `leader == this`: This section was retained.
  // - `leader != this`: This section was merged with another identical section.
  InputSection<E> *leader = nullptr;
  i32 icf_idx = -1;
  bool icf_eligible = false;
  bool icf_leaf = false;

  [[no_unique_address]] InputSectionExtras<E> extra;

private:
  void scan_pcrel(Context<E> &ctx, Symbol<E> &sym, const ElfRel<E> &rel);
  void scan_absrel(Context<E> &ctx, Symbol<E> &sym, const ElfRel<E> &rel);
  void scan_tlsdesc(Context<E> &ctx, Symbol<E> &sym);
  void check_tlsle(Context<E> &ctx, Symbol<E> &sym, const ElfRel<E> &rel);

  void apply_dyn_absrel(Context<E> &ctx, Symbol<E> &sym, const ElfRel<E> &rel,
                        u8 *loc, u64 S, i64 A, u64 P, ElfRel<E> **dynrel);

  void apply_toc_rel(Context<E> &ctx, Symbol<E> &sym, const ElfRel<E> &rel,
                     u8 *loc, u64 S, i64 A, u64 P, ElfRel<E> **dynrel);

  std::optional<u64> get_tombstone(Symbol<E> &sym, SectionFragment<E> *frag);
};

//
// tls.cc
//

template <typename E> u64 get_tp_addr(const ElfPhdr<E> &);
template <typename E> u64 get_dtp_addr(const ElfPhdr<E> &);

//
// filetype.cc
//

enum class FileType {
  UNKNOWN,
  EMPTY,
  ELF_OBJ,
  ELF_DSO,
  AR,
  THIN_AR,
  TEXT,
  GCC_LTO_OBJ,
  LLVM_BITCODE,
};

template <typename E>
FileType get_file_type(Context<E> &ctx, MappedFile *mf);

template <typename E>
std::string_view
get_machine_type(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf);

//
// output-chunks.cc
//

template <typename E>
Chunk<E> *find_chunk(Context<E> &ctx, u32 sh_type);

template <typename E>
Chunk<E> *find_chunk(Context<E> &ctx, std::string_view name);

template <typename E>
u64 get_eflags(Context<E> &ctx) {
  return 0;
}

template <typename E>
i64 to_phdr_flags(Context<E> &ctx, Chunk<E> *chunk);

template <typename E>
void write_plt_header(Context<E> &ctx, u8 *buf);

template <typename E>
void write_plt_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym);

template <typename E>
void write_pltgot_entry(Context<E> &ctx, u8 *buf, Symbol<E> &sym);

// Chunk represents a contiguous region in an output file.
template <typename E>
class __attribute__((aligned(4))) Chunk {
public:
  virtual ~Chunk() = default;
  virtual bool is_header() { return false; }
  virtual OutputSection<E> *to_osec() { return nullptr; }
  virtual void compute_section_size(Context<E> &ctx) {}
  virtual i64 get_reldyn_size(Context<E> &ctx) const { return 0; }
  virtual void construct_relr(Context<E> &ctx) {}
  virtual void copy_buf(Context<E> &ctx) {}
  virtual void write_to(Context<E> &ctx, u8 *buf) { unreachable(); }
  virtual void update_shdr(Context<E> &ctx) {}

  std::string_view name;
  ElfShdr<E> shdr = { .sh_addralign = 1 };
  i64 shndx = 0;
  bool is_relro = false;

  // For --gdb-index
  bool is_compressed = false;
  std::vector<u8> uncompressed_data;

  // Some synethetic sections add local symbols to the output.
  // For example, range extension thunks adds function_name@thunk
  // symbol for each thunk entry. The following members are used
  // for such synthesizing symbols.
  virtual void compute_symtab_size(Context<E> &ctx) {}
  virtual void populate_symtab(Context<E> &ctx) {}

  i64 local_symtab_idx = 0;
  i64 num_local_symtab = 0;
  i64 strtab_size = 0;
  i64 strtab_offset = 0;

  // Offset in .rel.dyn
  i64 reldyn_offset = 0;

  // For --section-order
  i64 sect_order = 0;

  // For --pack-dyn-relocs=relr
  std::vector<u64> relr;
};

// ELF header
template <typename E>
class OutputEhdr : public Chunk<E> {
public:
  OutputEhdr(u32 sh_flags) {
    this->name = "EHDR";
    this->shdr.sh_flags = sh_flags;
    this->shdr.sh_size = sizeof(ElfEhdr<E>);
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  bool is_header() override { return true; }
  void copy_buf(Context<E> &ctx) override;
};

// Section header
template <typename E>
class OutputShdr : public Chunk<E> {
public:
  OutputShdr() {
    this->name = "SHDR";
    this->shdr.sh_size = 1;
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  bool is_header() override { return true; }
  void copy_buf(Context<E> &ctx) override;
};

// Program header
template <typename E>
class OutputPhdr : public Chunk<E> {
public:
  OutputPhdr(u32 sh_flags) {
    this->name = "PHDR";
    this->shdr.sh_flags = sh_flags;
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  bool is_header() override { return true; }
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<ElfPhdr<E>> phdrs;
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

enum AbsRelKind {
  ABS_REL_NONE,
  ABS_REL_BASEREL,
  ABS_REL_RELR,
  ABS_REL_IFUNC,
  ABS_REL_DYNREL,
};

// Represents a word-size absolute relocation (e.g. R_X86_64_64)
template <typename E>
struct AbsRel {
  InputSection<E> *isec = nullptr;
  u64 offset = 0;
  Symbol<E> *sym = nullptr;
  i64 addend = 0;
  AbsRelKind kind = ABS_REL_NONE;
};

// Sections
template <typename E>
class OutputSection : public Chunk<E> {
public:
  OutputSection(std::string_view name, u32 type) {
    this->name = name;
    this->shdr.sh_type = type;
  }

  OutputSection<E> *to_osec() override { return this; }
  void compute_section_size(Context<E> &ctx) override;
  i64 get_reldyn_size(Context<E> &ctx) const override;
  void construct_relr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
  void write_to(Context<E> &ctx, u8 *buf) override;

  void compute_symtab_size(Context<E> &ctx) override;
  void populate_symtab(Context<E> &ctx) override;

  void scan_abs_relocations(Context<E> &ctx);
  void create_range_extension_thunks(Context<E> &ctx);

  std::vector<InputSection<E> *> members;
  std::vector<std::unique_ptr<Thunk<E>>> thunks;
  std::unique_ptr<RelocSection<E>> reloc_sec;
  std::vector<AbsRel<E>> abs_rels;
  Atomic<u32> sh_flags;

  // Used only by create_output_sections()
  std::vector<std::vector<InputSection<E> *>> members_vec;
};

template <typename E>
class GotSection : public Chunk<E> {
public:
  GotSection() {
    this->name = ".got";
    this->is_relro = true;
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = sizeof(Word<E>);

    // We always create a .got so that _GLOBAL_OFFSET_TABLE_ has
    // something to point to. s390x psABI define GOT[1] and GOT[2]
    // as reserved slots, so we allocate two more for them.
    this->shdr.sh_size = (is_s390x<E> ? 3 : 1) * sizeof(Word<E>);
  }

  void add_got_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_gottp_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsgd_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsdesc_symbol(Context<E> &ctx, Symbol<E> *sym);
  void add_tlsld(Context<E> &ctx);

  u64 get_tlsld_addr(Context<E> &ctx) const;
  bool has_tlsld(Context<E> &ctx) const { return tlsld_idx != -1; }
  i64 get_reldyn_size(Context<E> &ctx) const override;
  void copy_buf(Context<E> &ctx) override;

  void construct_relr(Context<E> &ctx) override;
  void compute_symtab_size(Context<E> &ctx) override;
  void populate_symtab(Context<E> &ctx) override;

  std::vector<Symbol<E> *> got_syms;
  std::vector<Symbol<E> *> tlsgd_syms;
  std::vector<Symbol<E> *> tlsdesc_syms;
  std::vector<Symbol<E> *> gottp_syms;
  i64 tlsld_idx = -1;
};

template <typename E>
class GotPltSection : public Chunk<E> {
public:
  GotPltSection(Context<E> &ctx) {
    this->name = ".got.plt";
    this->is_relro = ctx.arg.z_now;
    this->shdr.sh_type = is_ppc64<E> ? SHT_NOBITS : SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = sizeof(Word<E>);
    this->shdr.sh_size = HDR_SIZE;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  static constexpr i64 HDR_SIZE = (is_ppc64v2<E> ? 2 : 3) * sizeof(Word<E>);
  static constexpr i64 ENTRY_SIZE = (is_ppc64v1<E> ? 3 : 1) * sizeof(Word<E>);
};

template <typename E>
class PltSection : public Chunk<E> {
public:
  PltSection() {
    this->name = ".plt";
    this->shdr.sh_type = SHT_PROGBITS;

    if constexpr (is_sparc<E>) {
      this->shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR | SHF_WRITE;
      this->shdr.sh_addralign = 256;
    } else {
      this->shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
      this->shdr.sh_addralign = 16;
    }
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  void compute_symtab_size(Context<E> &ctx) override;
  void populate_symtab(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class PltGotSection : public Chunk<E> {
public:
  PltGotSection() {
    this->name = ".plt.got";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
    this->shdr.sh_addralign = 16;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void copy_buf(Context<E> &ctx) override;

  void compute_symtab_size(Context<E> &ctx) override;
  void populate_symtab(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols;
};

template <typename E>
class RelPltSection : public Chunk<E> {
public:
  RelPltSection() {
    this->name = E::is_rela ? ".rela.plt" : ".rel.plt";
    this->shdr.sh_type = E::is_rela ? SHT_RELA : SHT_REL;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class RelDynSection : public Chunk<E> {
public:
  RelDynSection() {
    this->name = E::is_rela ? ".rela.dyn" : ".rel.dyn";
    this->shdr.sh_type = E::is_rela ? SHT_RELA : SHT_REL;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  void update_shdr(Context<E> &ctx) override;
};

template <typename E>
class RelrDynSection : public Chunk<E> {
public:
  RelrDynSection() {
    this->name = ".relr.dyn";
    this->shdr.sh_type = SHT_RELR;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(Word<E>);
    this->shdr.sh_addralign = sizeof(Word<E>);
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
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  // Offsets in .strtab for ARM32 mapping symbols
  static constexpr i64 ARM = 1;
  static constexpr i64 THUMB = 4;
  static constexpr i64 DATA = 7;
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
  }

  i64 add_string(std::string_view str);
  i64 find_string(std::string_view str);
  void copy_buf(Context<E> &ctx) override;

private:
  std::unordered_map<std::string_view, i64> strings;
};

template <typename E>
class DynamicSection : public Chunk<E> {
public:
  DynamicSection(Context<E> &ctx) {
    this->name = ".dynamic";
    this->shdr.sh_type = SHT_DYNAMIC;
    this->shdr.sh_addralign = sizeof(Word<E>);
    this->shdr.sh_entsize = sizeof(ElfDyn<E>);

    if (ctx.arg.z_rodynamic) {
      this->shdr.sh_flags = SHF_ALLOC;
      this->is_relro = false;
    } else {
      this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
      this->is_relro = true;
    }
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
std::optional<ElfSym<E>>
to_output_esym(Context<E> &ctx, Symbol<E> &sym, u32 st_name, U32<E> *shndx);

template <typename E>
class SymtabSection : public Chunk<E> {
public:
  SymtabSection() {
    this->name = ".symtab";
    this->shdr.sh_type = SHT_SYMTAB;
    this->shdr.sh_entsize = sizeof(ElfSym<E>);
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class SymtabShndxSection : public Chunk<E> {
public:
  SymtabShndxSection() {
    this->name = ".symtab_shndx";
    this->shdr.sh_type = SHT_SYMTAB_SHNDX;
    this->shdr.sh_entsize = 4;
    this->shdr.sh_addralign = 4;
  }
};

template <typename E>
class DynsymSection : public Chunk<E> {
public:
  DynsymSection() {
    this->name = ".dynsym";
    this->shdr.sh_type = SHT_DYNSYM;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(ElfSym<E>);
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> symbols;
  i64 dynstr_offset = -1;
};

template <typename E>
class HashSection : public Chunk<E> {
public:
  HashSection() {
    this->name = ".hash";
    this->shdr.sh_type = SHT_HASH;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_entsize = sizeof(Entry);
    this->shdr.sh_addralign = sizeof(Entry);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

private:
  // Even though u32 should suffice for all targets, s390x uses u64.
  // It looks like a spec bug, but we need to follow suit for the
  // sake of binary compatibility.
  using Entry = std::conditional_t<is_s390x<E>, U64<E>, U32<E>>;
};

template <typename E>
class GnuHashSection : public Chunk<E> {
public:
  GnuHashSection() {
    this->name = ".gnu.hash";
    this->shdr.sh_type = SHT_GNU_HASH;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  static constexpr i64 LOAD_FACTOR = 8;
  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 BLOOM_SHIFT = 26;

  i64 num_buckets = -1;
  i64 num_bloom = 1;
  i64 num_exported = -1;
};

template <typename E>
class MergedSection : public Chunk<E> {
public:
  static MergedSection<E> *
  get_instance(Context<E> &ctx, std::string_view name, const ElfShdr<E> &shdr);

  SectionFragment<E> *insert(Context<E> &ctx, std::string_view data,
                             u64 hash, i64 p2align);

  void resolve(Context<E> &ctx);
  void compute_section_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
  void write_to(Context<E> &ctx, u8 *buf) override;
  void print_stats(Context<E> &ctx);

  std::vector<MergeableSection<E> *> members;
  std::mutex mu;

  ConcurrentMap<SectionFragment<E>> map;
  HyperLogLog estimator;
  bool resolved = false;

private:
  MergedSection(std::string_view name, i64 flags, i64 type, i64 entsize);

  std::vector<i64> shard_offsets;
};

template <typename E>
class EhFrameSection : public Chunk<E> {
public:
  EhFrameSection() {
    this->name = ".eh_frame";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  void construct(Context<E> &ctx);
  void apply_eh_reloc(Context<E> &ctx, const ElfRel<E> &rel, u64 offset, u64 val);
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

  i64 num_fdes = 0;
};

template <typename E>
class EhFrameRelocSection : public Chunk<E> {
public:
  EhFrameRelocSection() {
    this->name = E::is_rela ? ".rela.eh_frame" : ".rel.eh_frame";
    this->shdr.sh_type = E::is_rela ? SHT_RELA : SHT_REL;
    this->shdr.sh_flags = SHF_INFO_LINK;
    this->shdr.sh_addralign = sizeof(Word<E>);
    this->shdr.sh_entsize = sizeof(ElfRel<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class CopyrelSection : public Chunk<E> {
public:
  CopyrelSection(bool is_relro) {
    this->name = is_relro ? ".copyrel.rel.ro" : ".copyrel";
    this->is_relro = is_relro;
    this->shdr.sh_type = SHT_NOBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
  }

  void add_symbol(Context<E> &ctx, Symbol<E> *sym);
  i64 get_reldyn_size(Context<E> &ctx) const override { return symbols.size(); }
  void copy_buf(Context<E> &ctx) override;

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

  std::vector<U16<E>> contents;
};

template <typename E>
class VerneedSection : public Chunk<E> {
public:
  VerneedSection() {
    this->name = ".gnu.version_r";
    this->shdr.sh_type = SHT_GNU_VERNEED;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 4;
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
    this->shdr.sh_addralign = 4;
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

  std::vector<u8> contents;
};

template <typename E>
class NotePackageSection : public Chunk<E> {
public:
  NotePackageSection() {
    this->name = ".note.package";
    this->shdr.sh_type = SHT_NOTE;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 4;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class NotePropertySection : public Chunk<E> {
public:
  NotePropertySection() {
    this->name = ".note.gnu.property";
    this->shdr.sh_type = SHT_NOTE;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = sizeof(Word<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

private:
  struct Entry64 {
    U32<E> type;
    U32<E> size;
    U32<E> flags;
    u8 padding[4];
  };

  struct Entry32 {
    U32<E> type;
    U32<E> size;
    U32<E> flags;
  };

  using Entry = std::conditional_t<E::is_64, Entry64, Entry32>;

  std::vector<Entry> contents;
};

template <typename E>
class GnuDebuglinkSection : public Chunk<E> {
public:
  GnuDebuglinkSection() {
    this->name = ".gnu_debuglink";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_addralign = 4;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::string filename;
  u32 crc32 = 0;
};

template <typename E>
class GdbIndexSection : public Chunk<E> {
public:
  GdbIndexSection() {
    this->name = ".gdb_index";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_addralign = 4;
  }
};

template <typename E>
class CompressedSection : public Chunk<E> {
public:
  CompressedSection(Context<E> &ctx, Chunk<E> &chunk);
  void copy_buf(Context<E> &ctx) override;

private:
  ElfChdr<E> chdr = {};
  std::unique_ptr<Compressor> compressor;
};

template <typename E>
class RelocSection : public Chunk<E> {
public:
  RelocSection(Context<E> &ctx, OutputSection<E> &osec);
  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

private:
  OutputSection<E> &output_section;
  std::vector<i64> offsets;
};

// PT_GNU_RELRO works on page granularity. We want to align its end to
// a page boundary. We append this section at end of a segment so that
// the segment always ends at a page boundary.
template <typename E>
class RelroPaddingSection : public Chunk<E> {
public:
  RelroPaddingSection() {
    this->name = ".relro_padding";
    this->is_relro = true;
    this->shdr.sh_type = SHT_NOBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = 1;
    this->shdr.sh_size = 1;
  }
};

template <typename E>
class ComdatGroupSection : public Chunk<E> {
public:
  ComdatGroupSection(Symbol<E> &sym, std::vector<Chunk<E> *> members)
    : sym(sym), members(std::move(members)) {
    this->name = ".group";
    this->shdr.sh_type = SHT_GROUP;
    this->shdr.sh_entsize = 4;
    this->shdr.sh_addralign = 4;
    this->shdr.sh_size = this->members.size() * 4 + 4;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

private:
  Symbol<E> &sym;
  std::vector<Chunk<E> *> members;
};

//
// output-file.cc
//

template <typename E>
class OutputFile {
public:
  static std::unique_ptr<OutputFile<E>>
  open(Context<E> &ctx, std::string path, i64 filesize, int perm);

  virtual void close(Context<E> &ctx) = 0;
  virtual ~OutputFile() = default;

  u8 *buf = nullptr;
  std::vector<u8> buf2;
  std::string path;
  int fd = -1;
  i64 filesize = 0;
  bool is_mmapped = false;
  bool is_unmapped = false;

protected:
  OutputFile(std::string path, i64 filesize, bool is_mmapped)
    : path(path), filesize(filesize), is_mmapped(is_mmapped) {}
};

template <typename E>
class MallocOutputFile : public OutputFile<E> {
public:
  MallocOutputFile(Context<E> &ctx, std::string path, i64 filesize, int perm)
    : OutputFile<E>(path, filesize, false), ptr(new u8[filesize]),
      perm(perm) {
    this->buf = ptr.get();
  }

  void close(Context<E> &ctx) override {
    Timer t(ctx, "close_file");
    FILE *fp;

    if (this->path == "-") {
      fp = stdout;
    } else {
#ifdef _WIN32
      int pmode = (perm & 0200) ? (_S_IREAD | _S_IWRITE) : _S_IREAD;
      i64 fd = _open(this->path.c_str(), _O_RDWR | _O_CREAT | _O_BINARY, pmode);
#else
      i64 fd = ::open(this->path.c_str(), O_RDWR | O_CREAT, perm);
#endif
      if (fd == -1)
        Fatal(ctx) << "cannot open " << this->path << ": " << errno_string();
#ifdef _WIN32
      fp = _fdopen(fd, "wb");
#else
      fp = fdopen(fd, "w");
#endif
    }

    fwrite(this->buf, this->filesize, 1, fp);
    if (!this->buf2.empty())
      fwrite(this->buf2.data(), this->buf2.size(), 1, fp);
    fclose(fp);
  }

private:
  std::unique_ptr<u8[]> ptr;
  int perm;
};

template <typename E>
class LockingOutputFile : public OutputFile<E> {
public:
  LockingOutputFile(Context<E> &ctx, std::string path, int perm);
  void resize(Context<E> &ctx, i64 filesize);
  void close(Context<E> &ctx) override;
};

//
// gdb-index.cc
//

template <typename E> void write_gdb_index(Context<E> &ctx);

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
  // The file priority of the owner file of this comdat section.
  Atomic<u32> owner = -1;
};

template <typename E>
struct ComdatGroupRef {
  ComdatGroup *group;
  i32 sect_idx;
  std::span<U32<E>> members;
};

template <typename E>
class MergeableSection {
public:
  MergeableSection(Context<E> &ctx, MergedSection<E> &parent,
                   std::unique_ptr<InputSection<E>> &isec);

  void split_contents(Context<E> &ctx);
  void resolve_contents(Context<E> &ctx);
  std::pair<SectionFragment<E> *, i64> get_fragment(i64 offset);
  std::string_view get_contents(i64 idx);

  MergedSection<E> &parent;
  u8 p2align = 0;
  std::unique_ptr<InputSection<E>> input_section;
  std::vector<SectionFragment<E> *> fragments;

private:
  std::vector<u32> frag_offsets;
  std::vector<u32> hashes;
};

// InputFile is the base class of ObjectFile and SharedFile.
template <typename E>
class InputFile {
public:
  InputFile(Context<E> &ctx, MappedFile *mf);
  InputFile() : filename("<internal>") {}

  virtual ~InputFile() = default;

  template <typename T>
  std::span<T> get_data(Context<E> &ctx, const ElfShdr<E> &shdr);

  template <typename T>
  std::span<T> get_data(Context<E> &ctx, i64 idx);

  std::string_view get_string(Context<E> &ctx, const ElfShdr<E> &shdr);
  std::string_view get_string(Context<E> &ctx, i64 idx);
  u32 get_eflags() { return ((ElfEhdr<E> *)mf->data)->e_flags; }

  ElfShdr<E> *find_section(i64 type);

  virtual void resolve_symbols(Context<E> &ctx) = 0;

  virtual void
  mark_live_objects(Context<E> &ctx,
                    std::function<void(InputFile<E> *)> feeder) = 0;

  std::span<Symbol<E> *> get_local_syms();
  std::span<Symbol<E> *> get_global_syms();
  std::string_view get_source_name() const;

  MappedFile *mf = nullptr;
  std::span<ElfShdr<E>> elf_sections;
  std::span<ElfSym<E>> elf_syms;
  std::vector<Symbol<E> *> symbols;
  i64 first_global = 0;

  std::string filename;
  bool is_dso = false;
  i64 priority;
  Atomic<bool> is_reachable = false;
  std::string_view shstrtab;
  std::string_view symbol_strtab;

  bool as_needed = false;
  bool has_init_array = false;
  bool has_ctors = false;

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
  std::vector<Symbol<E>> local_syms;
  std::vector<Symbol<E>> frag_syms;
};

template <typename E>
struct ObjectFileExtras {};

template <is_riscv E>
struct ObjectFileExtras<E> {
  std::optional<i64> stack_align;
  std::optional<std::string_view> arch;
  bool unaligned_access = false;
};

template <>
struct ObjectFileExtras<PPC32> {
  InputSection<PPC32> *got2 = nullptr;
};

// ObjectFile represents an input .o file.
template <typename E>
class ObjectFile : public InputFile<E> {
public:
  ObjectFile() = default;

  ObjectFile(Context<E> &ctx, MappedFile *mf, std::string archive_name)
    : InputFile<E>(ctx, mf), archive_name(archive_name) {}

  void parse(Context<E> &ctx);
  void initialize_symbols(Context<E> &ctx);
  void parse_ehframe(Context<E> &ctx);
  void convert_mergeable_sections(Context<E> &ctx);
  void reattach_section_pieces(Context<E> &ctx);
  void resolve_symbols(Context<E> &ctx) override;
  void mark_live_objects(Context<E> &ctx,
                         std::function<void(InputFile<E> *)> feeder) override;
  void convert_undefined_weak_symbols(Context<E> &ctx);
  void scan_relocations(Context<E> &ctx);
  void convert_common_symbols(Context<E> &ctx);
  void compute_symtab_size(Context<E> &ctx);
  void populate_symtab(Context<E> &ctx);

  i64 get_shndx(const ElfSym<E> &esym);
  InputSection<E> *get_section(const ElfSym<E> &esym);

  std::string archive_name;
  std::vector<std::unique_ptr<InputSection<E>>> sections;
  std::vector<std::unique_ptr<MergeableSection<E>>> mergeable_sections;
  std::vector<ElfShdr<E>> elf_sections2;
  std::vector<CieRecord<E>> cies;
  std::vector<FdeRecord<E>> fdes;
  std::vector<bool> has_symver;
  std::vector<ComdatGroupRef<E>> comdat_groups;
  std::vector<InputSection<E> *> eh_frame_sections;
  std::vector<std::vector<ElfRel<E>>> decoded_crel;
  bool exclude_libs = false;
  std::map<u32, u32> gnu_properties;
  bool needs_executable_stack = false;
  bool is_lto_obj = false;
  bool is_gcc_offload_obj = false;
  bool is_rust_obj = false;
  bool is_dwarf32 = false;

  i64 fde_idx = 0;
  i64 fde_offset = 0;
  i64 fde_size = 0;

  // For ICF
  std::unique_ptr<InputSection<E>> llvm_addrsig;

  // For .gdb_index
  InputSection<E> *debug_info = nullptr;
  InputSection<E> *debug_pubnames = nullptr;
  InputSection<E> *debug_pubtypes = nullptr;

  // For LTO
  std::vector<ElfSym<E>> lto_elf_syms;
  std::vector<ComdatGroup *> lto_comdat_groups;

private:
  void initialize_sections(Context<E> &ctx);
  void sort_relocations(Context<E> &ctx);
  void initialize_ehframe_sections(Context<E> &ctx);
  void parse_note_gnu_property(Context <E> &ctx, const ElfShdr <E> &shdr);
  void override_symbol(Context<E> &ctx, Symbol<E> &sym,
                       const ElfSym<E> &esym, i64 symidx);
  void merge_visibility(Context<E> &ctx, Symbol<E> &sym, u8 visibility);

  bool has_common_symbol = false;

  const ElfShdr<E> *symtab_sec;
  std::span<U32<E>> symtab_shndx_sec;

public:
  // Target-specific member
  [[no_unique_address]] ObjectFileExtras<E> extra;
};

// SharedFile represents an input .so file.
template <typename E>
class SharedFile : public InputFile<E> {
public:
  SharedFile(Context<E> &ctx, MappedFile *mf) : InputFile<E>(ctx, mf) {}

  void parse(Context<E> &ctx);
  void resolve_symbols(Context<E> &ctx) override;
  std::span<Symbol<E> *> get_symbols_at(Symbol<E> *sym);
  i64 get_alignment(Symbol<E> *sym);
  std::vector<std::string_view> get_dt_needed(Context<E> &ctx);
  std::string_view get_dt_audit(Context<E> &ctx);
  bool is_readonly(Symbol<E> *sym);

  void mark_live_objects(Context<E> &ctx,
                         std::function<void(InputFile<E> *)> feeder) override;

  void compute_symtab_size(Context<E> &ctx);
  void populate_symtab(Context<E> &ctx);

  std::string soname;
  std::vector<std::string_view> version_strings;
  std::vector<Symbol<E> *> symbols2;
  std::vector<ElfSym<E>> elf_syms2;

private:
  std::string get_soname(Context<E> &ctx);
  void maybe_override_symbol(Symbol<E> &sym, const ElfSym<E> &esym);
  std::vector<std::string_view> read_dt_needed(Context<E> &ctx);
  std::vector<std::string_view> read_verdef(Context<E> &ctx);

  std::vector<u16> versyms;
  const ElfShdr<E> *symtab_sec;

  // Used by get_symbols_at()
  std::once_flag init_sorted_syms;
  std::vector<Symbol<E> *> sorted_syms;
};

//
// linker-script.cc
//

struct ReaderContext {
  bool as_needed = false;
  bool in_lib = false;
  bool static_ = false;
  bool whole_archive = false;
  tbb::task_group *tg = nullptr;
};

struct DynamicPattern {
  std::string_view pattern;
  std::string_view source;
  bool is_cpp = false;
};

template <typename E>
class Script {
public:
  Script(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf)
    : ctx(ctx), rctx(rctx), mf(mf) {}

  std::string_view get_script_output_type();
  void parse_linker_script();
  void parse_version_script();
  std::vector<DynamicPattern> parse_dynamic_list();

private:
  [[noreturn]] void error(std::string_view pos, std::string msg);

  void tokenize();

  std::span<std::string_view>
  skip(std::span<std::string_view> tok, std::string_view str);

  std::span<std::string_view> read_output_format(std::span<std::string_view> tok);
  std::span<std::string_view> read_group(std::span<std::string_view> tok);

  std::span<std::string_view>
  read_version_script_commands(std::span<std::string_view> tok,
                               std::string_view ver_str, u16 ver_idx,
                               bool is_global, bool is_cpp);

  std::span<std::string_view> read_version_script(std::span<std::string_view> tok);

  MappedFile *resolve_path(std::string_view tok, bool check_target);

  std::span<std::string_view>
  read_dynamic_list_commands(std::span<std::string_view> tok,
                             std::vector<DynamicPattern> &result, bool is_cpp);

  Context<E> &ctx;
  ReaderContext &rctx;
  MappedFile *mf = mf;
  std::once_flag once;
  std::vector<std::string_view> tokens;
};

template <typename E>
std::vector<DynamicPattern>
parse_dynamic_list(Context<E> &ctx, std::string_view path);

//
// lto.cc
//

template <typename E>
ObjectFile<E> *read_lto_object(Context<E> &ctx, MappedFile *mb);

template <typename E>
std::vector<ObjectFile<E> *> run_lto_plugin(Context<E> &ctx);

template <typename E>
void lto_cleanup(Context<E> &ctx);

//
// shrink-sections.cc
//

inline i64 get_removed_bytes(std::span<RelocDelta> deltas, i64 i) {
  if (i == 0)
    return deltas[i].delta;
  return deltas[i].delta - deltas[i - 1].delta;
}

template <typename E>
void shrink_sections(Context<E> &ctx);

template <typename E>
void shrink_section(Context<E> &ctx, InputSection<E> &isec);

template <typename E>
i64 get_r_delta(InputSection<E> &isec, u64 offset);

template <typename E>
i64 compute_distance(Context<E> &ctx, Symbol<E> &sym,
                     InputSection<E> &isec, const ElfRel<E> &rel);

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
void combine_objects(Context<E> &ctx);

//
// mapfile.cc
//

template <typename E>
void print_map(Context<E> &ctx);

//
// subprocess.cc
//

void fork_child();
void notify_parent();

template <typename E>
[[noreturn]]
void process_run_subcommand(Context<E> &ctx, int argc, char **argv);

//
// cmdline.cc
//

template <typename E>
std::vector<std::string_view> expand_response_files(Context<E> &ctx, char **argv);

template <typename E>
std::vector<std::string> parse_nonpositional_args(Context<E> &ctx);

//
// passes.cc
//

template <typename E> int redo_main(Context<E> &, int argc, char **argv);
template <typename E> void create_internal_file(Context<E> &);
template <typename E> void apply_exclude_libs(Context<E> &);
template <typename E> void create_synthetic_sections(Context<E> &);
template <typename E> void set_file_priority(Context<E> &);
template <typename E> void resolve_symbols(Context<E> &);
template <typename E> void do_lto(Context<E> &);
template <typename E> void parse_eh_frame_sections(Context<E> &);
template <typename E> void create_merged_sections(Context<E> &);
template <typename E> void convert_common_symbols(Context<E> &);
template <typename E> void create_output_sections(Context<E> &);
template <typename E> void add_synthetic_symbols(Context<E> &);
template <typename E> void apply_section_align(Context<E> &);
template <typename E> void check_cet_errors(Context<E> &);
template <typename E> void print_dependencies(Context<E> &);
template <typename E> void write_repro_file(Context<E> &);
template <typename E> void check_duplicate_symbols(Context<E> &);
template <typename E> void check_shlib_undefined(Context<E> &);
template <typename E> void check_symbol_types(Context<E> &);
template <typename E> void sort_init_fini(Context<E> &);
template <typename E> void sort_ctor_dtor(Context<E> &);
template <typename E> void fixup_ctors_in_init_array(Context<E> &);
template <typename E> void shuffle_sections(Context<E> &);
template <typename E> void add_dynamic_strings(Context<E> &);
template <typename E> void compute_section_sizes(Context<E> &);
template <typename E> void sort_output_sections(Context<E> &);
template <typename E> void claim_unresolved_symbols(Context<E> &);
template <typename E> void scan_relocations(Context<E> &);
template <typename E> void compute_imported_symbol_weakness(Context<E> &);
template <typename E> void construct_relr(Context<E> &);
template <typename E> void sort_dynsyms(Context<E> &);
template <typename E> void sort_debug_info_sections(Context<E> &);
template <typename E> void create_output_symtab(Context<E> &);
template <typename E> void report_undef_errors(Context<E> &);
template <typename E> void create_reloc_sections(Context<E> &);
template <typename E> void copy_chunks(Context<E> &);
template <typename E> void apply_version_script(Context<E> &);
template <typename E> void parse_symbol_version(Context<E> &);
template <typename E> void compute_import_export(Context<E> &);
template <typename E> void compute_address_significance(Context<E> &);
template <typename E> void separate_debug_sections(Context<E> &);
template <typename E> void compute_section_headers(Context<E> &);
template <typename E> i64 set_osec_offsets(Context<E> &);
template <typename E> void fix_synthetic_symbols(Context<E> &);
template <typename E> void compress_debug_sections(Context<E> &);
template <typename E> void sort_reldyn(Context<E> &);
template <typename E> void write_build_id(Context<E> &);
template <typename E> void write_gnu_debuglink(Context<E> &);
template <typename E> void write_separate_debug_file(Context<E> &ctx);
template <typename E> void write_dependency_file(Context<E> &);
template <typename E> void show_stats(Context<E> &);

//
// arch-x86-64.cc
//

void rewrite_endbr(Context<X86_64> &ctx);

//
// arch-arm32.cc
//

template <is_arm32 E>
class Arm32ExidxSection : public Chunk<E> {
public:
  Arm32ExidxSection(OutputSection<E> &osec) : output_section(osec) {
    this->name = ".ARM.exidx";
    this->shdr.sh_type = SHT_ARM_EXIDX;
    this->shdr.sh_flags = SHF_ALLOC;
    this->shdr.sh_addralign = 4;
  }

  void compute_section_size(Context<E> &ctx) override;
  void update_shdr(Context<E> &ctx) override;
  void remove_duplicate_entries(Context<E> &ctx);
  void copy_buf(Context<E> &ctx) override;

private:
  std::vector<u8> get_contents(Context<E> &ctx);

  OutputSection<E> &output_section;
};

template <is_arm32 E>
u64 get_eflags(Context<E> &ctx);

template <is_arm32 E>
void create_arm_exidx_section(Context<E> &ctx);

void arm32be_swap_bytes(Context<ARM32BE> &ctx);

//
// arch-riscv.cc
//

template <is_riscv E>
class RiscvAttributesSection : public Chunk<E> {
public:
  RiscvAttributesSection() {
    this->name = ".riscv.attributes";
    this->shdr.sh_type = SHT_RISCV_ATTRIBUTES;
  }

  void update_shdr(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

template <is_riscv E>
u64 get_eflags(Context<E> &ctx);

//
// arch-ppc64v1.cc
//

void ppc64v1_rewrite_opd(Context<PPC64V1> &ctx);
void ppc64v1_scan_symbols(Context<PPC64V1> &ctx);

class PPC64OpdSection : public Chunk<PPC64V1> {
public:
  PPC64OpdSection() {
    this->name = ".opd";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_WRITE;
    this->shdr.sh_addralign = 8;
  }

  void add_symbol(Context<PPC64V1> &ctx, Symbol<PPC64V1> *sym);
  i64 get_reldyn_size(Context<PPC64V1> &ctx) const override;
  void copy_buf(Context<PPC64V1> &ctx) override;

  static constexpr i64 ENTRY_SIZE = sizeof(Word<PPC64V1>) * 3;

  std::vector<Symbol<PPC64V1> *> symbols;
};

//
// arch-ppc64v2.cc
//

extern const std::vector<std::pair<std::string_view, u32>>
ppc64_save_restore_insns;

class PPC64SaveRestoreSection : public Chunk<PPC64V2> {
public:
  PPC64SaveRestoreSection() {
    this->name = ".save_restore_gprs";
    this->shdr.sh_type = SHT_PROGBITS;
    this->shdr.sh_flags = SHF_ALLOC | SHF_EXECINSTR;
    this->shdr.sh_addralign = 16;
    this->shdr.sh_size = ppc64_save_restore_insns.size() * 4;
  }

  void copy_buf(Context<PPC64V2> &ctx) override;
};

template <> u64 get_eflags(Context<PPC64V2> &ctx);

//
// main.cc
//

struct BuildId {
  i64 size() const {
    switch (kind) {
    case HEX:
      return value.size();
    case HASH:
      return hash_size;
    case UUID:
      return 16;
    default:
      unreachable();
    }
  }

  enum { NONE, HEX, HASH, UUID } kind = NONE;
  std::vector<u8> value;
  i64 hash_size = 0;
};

typedef enum { COMPRESS_NONE, COMPRESS_ZLIB, COMPRESS_ZSTD } CompressKind;

typedef enum {
  UNRESOLVED_ERROR,
  UNRESOLVED_WARN,
  UNRESOLVED_IGNORE,
} UnresolvedKind;

typedef enum {
  BSYMBOLIC_NONE,
  BSYMBOLIC_ALL,
  BSYMBOLIC_FUNCTIONS,
  BSYMBOLIC_NON_WEAK,
  BSYMBOLIC_NON_WEAK_FUNCTIONS,
} BsymbolicKind;

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
  std::string_view source;
  std::string_view ver_str;
  i64 ver_idx = -1;
  bool is_cpp = false;
};

struct SectionOrder {
  enum { NONE, SECTION, GROUP, ADDR, ALIGN, SYMBOL } type = NONE;
  std::string name;
  u64 value = 0;
};

// Target-specific context members
template <typename E>
struct ContextExtras {};

template <is_x86 E>
struct ContextExtras<E> {
  NotePropertySection<E> *note_property = nullptr;
};

template <is_arm32 E>
struct ContextExtras<E> {
  Arm32ExidxSection<E> *exidx = nullptr;
};

template <is_riscv E>
struct ContextExtras<E> {
  RiscvAttributesSection<E> *riscv_attributes = nullptr;
};

template <>
struct ContextExtras<PPC32> {
  Symbol<PPC32> *_SDA_BASE_ = nullptr;
};

template <>
struct ContextExtras<PPC64V1> {
  PPC64OpdSection *opd = nullptr;
  Symbol<PPC64V1> *TOC = nullptr;
};

template <>
struct ContextExtras<PPC64V2> {
  PPC64SaveRestoreSection *save_restore = nullptr;
  Symbol<PPC64V2> *TOC = nullptr;
  Atomic<bool> is_power10 = false;
};

template <>
struct ContextExtras<SPARC64> {
  Symbol<SPARC64> *tls_get_addr = nullptr;
};

// Context represents a context object for each invocation of the linker.
// It contains command line flags, pointers to singleton objects
// (such as linker-synthesized output sections), unique_ptrs for
// resource management, and other miscellaneous objects.
template <typename E>
struct Context {
  Context() {
    arg.entry = get_symbol(*this, "_start");
    arg.fini = get_symbol(*this, "_fini");
    arg.init = get_symbol(*this, "_init");

    if constexpr (is_sparc<E>)
      extra.tls_get_addr = get_symbol(*this, "__tls_get_addr");
  }

  Context(const Context<E> &) = delete;

  void checkpoint() {
    if (has_error) {
      cleanup();
      _exit(1);
    }
  }

  // Command-line arguments
  struct {
    BsymbolicKind Bsymbolic = BSYMBOLIC_NONE;
    BuildId build_id;
    CetReportKind z_cet_report = CET_REPORT_NONE;
    CompressKind compress_debug_sections = COMPRESS_NONE;
    MultiGlob undefined_glob;
    SeparateCodeKind z_separate_code = NOSEPARATE_CODE;
    ShuffleSectionsKind shuffle_sections = SHUFFLE_SECTIONS_NONE;
    Symbol<E> *entry = nullptr;
    Symbol<E> *fini = nullptr;
    Symbol<E> *init = nullptr;
    UnresolvedKind unresolved_symbols = UNRESOLVED_IGNORE;
    bool allow_multiple_definition = false;
    bool allow_shlib_undefined = true;
    bool apply_dynamic_relocs = true;
    bool be8 = false;
    bool color_diagnostics = false;
    bool default_symver = false;
    bool demangle = true;
    bool detach = true;
    bool discard_all = false;
    bool discard_locals = false;
    bool dynamic_list_data = false;
    bool eh_frame_hdr = true;
    bool emit_relocs = false;
    bool enable_new_dtags = true;
    bool execute_only = false;
    bool export_dynamic = false;
    bool fatal_warnings = false;
    bool fork = true;
    bool gc_sections = false;
    bool gdb_index = false;
    bool hash_style_gnu = true;
    bool hash_style_sysv = true;
    bool icf = false;
    bool icf_all = false;
    bool ignore_data_address_equality = false;
    bool lto_pass2 = false;
    bool nmagic = false;
    bool noinhibit_exec = false;
    bool oformat_binary = false;
    bool omagic = false;
    bool pack_dyn_relocs_relr = false;
    bool perf = false;
    bool pic = false;
    bool pie = false;
    bool print_dependencies = false;
    bool print_gc_sections = false;
    bool print_icf_sections = false;
    bool print_map = false;
    bool quick_exit = true;
    bool relax = true;
    bool relocatable = false;
    bool relocatable_merge_sections = false;
    bool repro = false;
    bool rosegment = true;
    bool shared = false;
    bool start_stop = false;
    bool static_ = false;
    bool stats = false;
    bool strip_all = false;
    bool strip_debug = false;
    bool suppress_warnings = false;
    bool trace = false;
    bool undefined_version = false;
    bool warn_common = false;
    bool warn_once = false;
    bool warn_textrel = false;
    bool z_copyreloc = true;
    bool z_delete = true;
    bool z_dlopen = true;
    bool z_dump = true;
    bool z_dynamic_undefined_weak = true;
    bool z_execstack = false;
    bool z_execstack_if_needed = false;
    bool z_ibt = false;
    bool z_initfirst = false;
    bool z_interpose = false;
    bool z_keep_text_section_prefix = false;
    bool z_nodefaultlib = false;
    bool z_now = false;
    bool z_origin = false;
    bool z_relro = true;
    bool z_rewrite_endbr = false;
    bool z_rodynamic = false;
    bool z_sectionheader = true;
    bool z_shstk = false;
    bool z_start_stop_visibility_protected = false;
    bool z_text = false;
    i64 filler = -1;
    i64 spare_dynamic_tags = 5;
    i64 spare_program_headers = 0;
    i64 thread_count = 0;
    i64 z_stack_size = 0;
    std::optional<Glob> unique;
    std::optional<u64> physical_image_base;
    std::optional<std::vector<Symbol<E> *>> retain_symbols_file;
    std::string Map;
    std::string audit;
    std::string chroot;
    std::string depaudit;
    std::string dependency_file;
    std::string directory;
    std::string dynamic_linker;
    std::string output = "a.out";
    std::string package_metadata;
    std::string plugin;
    std::string rpaths;
    std::string separate_debug_file;
    std::string soname;
    std::string sysroot;
    std::string_view emulation;
    std::unordered_map<std::string_view, u64> section_align;
    std::unordered_map<std::string_view, u64> section_start;
    std::unordered_set<std::string_view> discard_section;
    std::unordered_set<std::string_view> ignore_ir_file;
    std::unordered_set<std::string_view> wrap;
    std::vector<SectionOrder> section_order;
    std::vector<Symbol<E> *> require_defined;
    std::vector<Symbol<E> *> undefined;
    std::vector<std::pair<Symbol<E> *, std::variant<Symbol<E> *, u64>>> defsyms;
    std::vector<std::string> library_paths;
    std::vector<std::string> plugin_opt;
    std::vector<std::string> version_definitions;
    std::vector<std::string_view> auxiliary;
    std::vector<std::string_view> exclude_libs;
    std::vector<std::string_view> filter;
    std::vector<std::string_view> trace_symbol;
    u32 z_x86_64_isa_level = 0;
    u64 image_base = 0x200000;
    u64 shuffle_sections_seed = 0;
  } arg;

  std::vector<VersionPattern> version_patterns;
  std::vector<DynamicPattern> dynamic_list_patterns;
  i64 default_version = VER_NDX_UNSPECIFIED;
  i64 page_size = E::page_size;
  bool has_error = false;

  // Reader context
  i64 file_priority = 10000;

  // Symbol table
  tbb::concurrent_hash_map<std::string_view, Symbol<E>, HashCmp> symbol_map;
  tbb::concurrent_hash_map<std::string_view, ComdatGroup, HashCmp> comdat_groups;
  tbb::concurrent_vector<std::unique_ptr<MergedSection<E>>> merged_sections;

  tbb::concurrent_vector<std::unique_ptr<TimerRecord>> timer_records;
  tbb::concurrent_vector<std::function<void()>> on_exit;

  tbb::concurrent_vector<std::unique_ptr<ObjectFile<E>>> obj_pool;
  tbb::concurrent_vector<std::unique_ptr<SharedFile<E>>> dso_pool;
  tbb::concurrent_vector<std::unique_ptr<u8[]>> string_pool;
  tbb::concurrent_vector<std::unique_ptr<MappedFile>> mf_pool;
  tbb::concurrent_vector<std::unique_ptr<Chunk<E>>> chunk_pool;
  tbb::concurrent_vector<std::unique_ptr<OutputSection<E>>> osec_pool;

  // Symbol auxiliary data
  std::vector<SymbolAux<E>> symbol_aux;

  // Fully-expanded command line args
  std::vector<std::string_view> cmdline_args;

  // Input files
  std::vector<ObjectFile<E> *> objs;
  std::vector<SharedFile<E> *> dsos;

  ObjectFile<E> *internal_obj = nullptr;
  std::vector<ElfSym<E>> internal_esyms;

  // Output buffer
  std::unique_ptr<OutputFile<E>> output_file;
  u8 *buf = nullptr;
  bool overwrite_output_file = false;

  std::vector<Chunk<E> *> chunks;
  Atomic<bool> needs_tlsld = false;
  Atomic<bool> has_textrel = false;
  Atomic<i32> num_ifunc_dynrels = 0;

  tbb::concurrent_hash_map<Symbol<E> *, std::vector<std::string>> undef_errors;

  // For --separate-debug-file
  std::vector<Chunk<E> *> debug_chunks;

  // Output chunks
  OutputEhdr<E> *ehdr = nullptr;
  OutputShdr<E> *shdr = nullptr;
  OutputPhdr<E> *phdr = nullptr;
  InterpSection<E> *interp = nullptr;
  GotSection<E> *got = nullptr;
  GotPltSection<E> *gotplt = nullptr;
  RelPltSection<E> *relplt = nullptr;
  RelDynSection<E> *reldyn = nullptr;
  RelrDynSection<E> *relrdyn = nullptr;
  DynamicSection<E> *dynamic = nullptr;
  StrtabSection<E> *strtab = nullptr;
  DynstrSection<E> *dynstr = nullptr;
  HashSection<E> *hash = nullptr;
  GnuHashSection<E> *gnu_hash = nullptr;
  GnuDebuglinkSection<E> *gnu_debuglink = nullptr;
  ShstrtabSection<E> *shstrtab = nullptr;
  PltSection<E> *plt = nullptr;
  PltGotSection<E> *pltgot = nullptr;
  SymtabSection<E> *symtab = nullptr;
  SymtabShndxSection<E> *symtab_shndx = nullptr;
  DynsymSection<E> *dynsym = nullptr;
  EhFrameSection<E> *eh_frame = nullptr;
  EhFrameHdrSection<E> *eh_frame_hdr = nullptr;
  EhFrameRelocSection<E> *eh_frame_reloc = nullptr;
  CopyrelSection<E> *copyrel = nullptr;
  CopyrelSection<E> *copyrel_relro = nullptr;
  VersymSection<E> *versym = nullptr;
  VerneedSection<E> *verneed = nullptr;
  VerdefSection<E> *verdef = nullptr;
  BuildIdSection<E> *buildid = nullptr;
  NotePackageSection<E> *note_package = nullptr;
  GdbIndexSection<E> *gdb_index = nullptr;
  RelroPaddingSection<E> *relro_padding = nullptr;
  MergedSection<E> *comment = nullptr;

  // For --gdb-index
  std::span<u8> debug_info;
  std::span<u8> debug_abbrev;
  std::span<u8> debug_ranges;
  std::span<u8> debug_addr;
  std::span<u8> debug_rnglists;

  // For thread-local variables
  u64 tls_begin = 0;
  u64 tp_addr = 0;
  u64 dtp_addr = 0;

  // Linker-synthesized symbols
  Symbol<E> *_DYNAMIC = nullptr;
  Symbol<E> *_GLOBAL_OFFSET_TABLE_ = nullptr;
  Symbol<E> *_PROCEDURE_LINKAGE_TABLE_ = nullptr;
  Symbol<E> *_TLS_MODULE_BASE_ = nullptr;
  Symbol<E> *__GNU_EH_FRAME_HDR = nullptr;
  Symbol<E> *__bss_start = nullptr;
  Symbol<E> *__dso_handle = nullptr;
  Symbol<E> *__ehdr_start = nullptr;
  Symbol<E> *__executable_start = nullptr;
  Symbol<E> *__exidx_end = nullptr;
  Symbol<E> *__exidx_start = nullptr;
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

  [[no_unique_address]] ContextExtras<E> extra;
};

template <typename E>
std::string_view
get_machine_type(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf);

template <typename E>
MappedFile *open_library(Context<E> &ctx, ReaderContext &rctx, std::string path);

template <typename E>
MappedFile *find_library(Context<E> &ctx, ReaderContext &rctx, std::string path);

template <typename E>
void read_file(Context<E> &ctx, ReaderContext &rctx, MappedFile *mf);

template <typename E>
int mold_main(int argc, char **argv);

template <typename E>
std::ostream &operator<<(std::ostream &out, const InputFile<E> &file);

//
// Symbol
//

enum {
  NEEDS_GOT       = 1 << 0,
  NEEDS_PLT       = 1 << 1,
  NEEDS_CPLT      = 1 << 2,
  NEEDS_GOTTP     = 1 << 3,
  NEEDS_TLSGD     = 1 << 4,
  NEEDS_COPYREL   = 1 << 5,
  NEEDS_TLSDESC   = 1 << 6,
  NEEDS_PPC_OPD   = 1 << 7, // for PPCv1
};

// Flags for Symbol<E>::get_addr()
enum {
  NO_PLT = 1 << 0, // Request an address other than .plt
  NO_OPD = 1 << 1, // Request an address other than .opd (PPC64V1 only)
};

// Symbol class represents a symbol. For each unique symbol name, we
// create one instance of Symbol.
//
// A symbol has not only one but several different addresses if it
// has PLT or GOT entries. This class provides various functions to
// compute different addresses.
template <typename E>
class Symbol {
public:
  Symbol() = default;

  Symbol(std::string_view name, bool demangle)
    : nameptr(name.data()), namelen(name.size()), demangle(demangle) {}

  Symbol(const Symbol<E> &other) : Symbol(other.name(), other.demangle) {}

  u64 get_addr(Context<E> &ctx, i64 flags = 0) const;
  u64 get_got_addr(Context<E> &ctx) const;
  u64 get_gotplt_addr(Context<E> &ctx) const;
  u64 get_gottp_addr(Context<E> &ctx) const;
  u64 get_tlsgd_addr(Context<E> &ctx) const;
  u64 get_tlsdesc_addr(Context<E> &ctx) const;
  u64 get_plt_addr(Context<E> &ctx) const;
  u64 get_opd_addr(Context<E> &ctx) const;
  u64 get_got_pltgot_addr(Context<E> &ctx) const;

  void set_got_idx(Context<E> &ctx, i32 idx);
  void set_gottp_idx(Context<E> &ctx, i32 idx);
  void set_tlsgd_idx(Context<E> &ctx, i32 idx);
  void set_tlsdesc_idx(Context<E> &ctx, i32 idx);
  void set_plt_idx(Context<E> &ctx, i32 idx);
  void set_pltgot_idx(Context<E> &ctx, i32 idx);
  void set_opd_idx(Context<E> &ctx, i32 idx);
  void set_dynsym_idx(Context<E> &ctx, i32 idx);

  i32 get_got_idx(Context<E> &ctx) const;
  i32 get_gottp_idx(Context<E> &ctx) const;
  i32 get_tlsgd_idx(Context<E> &ctx) const;
  i32 get_tlsdesc_idx(Context<E> &ctx) const;
  i32 get_plt_idx(Context<E> &ctx) const;
  i32 get_pltgot_idx(Context<E> &ctx) const;
  i32 get_opd_idx(Context<E> &ctx) const;
  i32 get_dynsym_idx(Context<E> &ctx) const;

  bool has_plt(Context<E> &ctx) const;
  bool has_got(Context<E> &ctx) const { return get_got_idx(ctx) != -1; }
  bool has_gottp(Context<E> &ctx) const { return get_gottp_idx(ctx) != -1; }
  bool has_tlsgd(Context<E> &ctx) const { return get_tlsgd_idx(ctx) != -1; }
  bool has_tlsdesc(Context<E> &ctx) const { return get_tlsdesc_idx(ctx) != -1; }
  bool has_opd(Context<E> &ctx) const { return get_opd_idx(ctx) != -1; }

  u32 get_djb_hash(Context<E> &ctx) const;
  void set_djb_hash(Context<E> &ctx, u32 hash);

  u64 get_thunk_addr(Context<E> &ctx, u64 P) const requires needs_thunk<E>;

  bool is_absolute() const;
  bool is_relative() const { return !is_absolute(); }
  bool is_local(Context<E> &ctx) const;
  bool is_ifunc() const { return get_type() == STT_GNU_IFUNC; }
  bool is_pde_ifunc(Context<E> &ctx) const;
  bool is_remaining_undef_weak() const;

  bool is_pcrel_linktime_const(Context<E> &ctx) const;
  bool is_tprel_linktime_const(Context<E> &ctx) const;
  bool is_tprel_runtime_const(Context<E> &ctx) const;

  InputSection<E> *get_input_section() const;
  Chunk<E> *get_output_section() const;
  SectionFragment<E> *get_frag() const;

  void set_input_section(InputSection<E> *);
  void set_output_section(Chunk<E> *);
  void set_frag(SectionFragment<E> *);

  void set_name(std::string_view);
  std::string_view name() const;

  u32 get_type() const;
  std::string_view get_version() const;
  i64 get_output_sym_idx(Context<E> &ctx) const;
  const ElfSym<E> &esym() const;
  void add_aux(Context<E> &ctx);

  // A symbol is owned by a file. If two or more files define the
  // same symbol, the one with the strongest definition owns the symbol.
  // If `file` is null, the symbol is not defined by any input file.
  InputFile<E> *file = nullptr;

  // A symbol usually belongs to an input section, but it can belong
  // to a section fragment, an output section or nothing
  // (i.e. absolute symbol). `origin` holds one of them. We use the
  // least significant two bits to distinguish type.
  enum : uintptr_t {
    TAG_ABS  = 0b00,
    TAG_ISEC = 0b01,
    TAG_OSEC = 0b10,
    TAG_FRAG = 0b11,
    TAG_MASK = 0b11,
  };

  // We want to make sure there are enough number of unused bits in
  // pointers referring to these structures. In particular, we need
  // __attribute__((aligned(4))) for m68k on which int, long, float
  // and double are aligned only to two byte boundaries.
  static_assert(alignof(InputSection<E>) >= 4);
  static_assert(alignof(Chunk<E>) >= 4);
  static_assert(alignof(SectionFragment<E>) >= 4);

  uintptr_t origin = 0;

  // `value` contains symbol value. If it's an absolute symbol, it is
  // equivalent to its address. If it belongs to an input section or a
  // section fragment, value is added to the base of the input section
  // to yield an address.
  u64 value = 0;

  const char *nameptr = nullptr;
  i32 namelen = 0;

  // Index into the symbol table of the owner file.
  i32 sym_idx = -1;

  i32 aux_idx = -1;
  u16 ver_idx = VER_NDX_UNSPECIFIED;

  // `flags` has NEEDS_ flags.
  Atomic<u8> flags = 0;

  tbb::spin_mutex mu;
  Atomic<u8> visibility = STV_DEFAULT;

  bool is_weak : 1 = false;
  bool write_to_symtab : 1 = false; // for --strip-all and the like
  bool is_traced : 1 = false;       // for --trace-symbol
  bool is_wrapped : 1 = false;      // for --wrap

  // For symbols with default symbol version, e.g. foo@@VERSION.
  bool is_versioned_default : 1 = false;

  // If a symbol can be resolved to a symbol in a different ELF file at
  // runtime, `is_imported` is true. If a symbol is a dynamic symbol and
  // can be used by other ELF file at runtime, `is_exported` is true.
  //
  // Note that both can be true at the same time. Such symbol represents
  // a function or data exported from this ELF file which can be
  // imported by other definition at runtime. That is actually a usual
  // exported symbol when creating a DSO. In other words, a dynamic
  // symbol exported by a DSO is usually imported by itself.
  //
  // If is_imported is true and is_exported is false, it is a dynamic
  // symbol just imported from other DSO.
  //
  // If is_imported is false and is_exported is true, there are two
  // possible cases. If we are creating an executable, we know that
  // exported symbols cannot be intercepted by any DSO (because the
  // dynamic loader searches a dynamic symbol from an executable before
  // examining any DSOs), so any exported symbol is export-only in an
  // executable. If we are creating a DSO, export-only symbols
  // represent a protected symbol (i.e. a symbol whose visibility is
  // STV_PROTECTED).
  bool is_imported : 1 = false;
  bool is_exported : 1 = false;

  // `is_canonical` is true if this symbol represents a "canonical" PLT.
  // Here is the explanation as to what the canonical PLT is.
  //
  // In C/C++, the process-wide function pointer equality is guaranteed.
  // That is, if you take an address of a function `foo`, it's always
  // evaluated to the same address wherever you do that.
  //
  // For the sake of explanation, assume that `libx.so` exports a
  // function symbol `foo`, and there's a program that uses `libx.so`.
  // Both `libx.so` and the main executable take the address of `foo`,
  // which must be evaluated to the same address because of the above
  // guarantee.
  //
  // If the main executable is position-independent code (PIC), `foo` is
  // evaluated to the beginning of the function code, as you would have
  // expected. The address of `foo` is stored to GOTs, and the machine
  // code that takes the address of `foo` reads the GOT entries at
  // runtime.
  //
  // However, if it's not PIC, the main executable's code was compiled
  // to not use GOT (note that shared objects are always PIC, only
  // executables can be non-PIC). It instead assumes that `foo` (and any
  // other global variables/functions) has an address that is fixed at
  // link-time. This assumption is correct if `foo` is in the same
  // position-dependent executable, but it's not if `foo` is imported
  // from some other DSO at runtime.
  //
  // In this case, we use the address of the `foo`'s PLT entry in the
  // main executable (whose address is fixed at link-time) as its
  // address. In order to guarantee pointer equality, we also need to
  // fill foo's GOT entries in DSOs with the addres of the foo's PLT
  // entry instead of `foo`'s real address. We can do that by setting a
  // symbol value to `foo`'s dynamic symbol. If a symbol value is set,
  // the dynamic loader initialize `foo`'s GOT entries with that value
  // instead of the symbol's real address.
  //
  // We call such PLT entry in the main executable as "canonical".
  // If `foo` has a canonical PLT, its address is evaluated to its
  // canonical PLT's address. Otherwise, it's evaluated to `foo`'s
  // address.
  //
  // Only non-PIC main executables may have canonical PLTs. PIC
  // executables and shared objects never have a canonical PLT.
  //
  // This bit manages if we need to make this symbol's PLT canonical.
  // This bit is meaningful only when the symbol has a PLT entry.
  bool is_canonical : 1 = false;

  // If an input object file is not compiled with -fPIC (or with
  // -fno-PIC), the file not position independent. That means the
  // machine code included in the object file does not use GOT to access
  // global variables. Instead, it assumes that addresses of global
  // variables are known at link-time.
  //
  // Let's say `libx.so` exports a global variable `foo`, and a main
  // executable uses the variable. If the executable is not compiled
  // with -fPIC, we can't simply apply a relocation that refers `foo`
  // because `foo`'s address is not known at link-time.
  //
  // In this case, we could print out the "recompile with -fPIC" error
  // message, but there's a way to workaround.
  //
  // The loader supports a feature so-called "copy relocations".
  // A copy relocation instructs the loader to copy data from a DSO to a
  // specified location in the main executable. By using this feature,
  // we can copy `foo`'s data to a BSS region at runtime. With that,
  // we can apply relocations agianst `foo` as if `foo` existed in the
  // main executable's BSS area, whose address is known at link-time.
  //
  // Copy relocations are used only by position-dependent executables.
  // Position-independent executables and DSOs don't need them because
  // they use GOT to access global variables.
  //
  // `has_copyrel` is true if we need to emit a copy relocation for this
  // symbol. If the original symbol in a DSO is in a read-only memory
  // region, `is_copyrel_readonly` is set to true so that the copied data
  // will become read-only at run-time.
  bool has_copyrel : 1 = false;
  bool is_copyrel_readonly : 1 = false;

  // For symbol resolution. This flag is used rarely. See a comment in
  // resolve_symbols().
  bool skip_dso : 1 = false;

  // For --gc-sections
  bool gc_root : 1 = false;

  // For LTO. True if the symbol is referenced by a regular object (as
  // opposed to IR object).
  bool referenced_by_regular_obj : 1 = false;

  // If true, we try to dmenagle the sybmol when printing.
  bool demangle : 1 = false;
};

template <typename E>
Symbol<E> *get_symbol(Context<E> &ctx, std::string_view key,
                      std::string_view name);

template <typename E>
Symbol<E> *get_symbol(Context<E> &ctx, std::string_view name);

template <typename E>
ComdatGroup *insert_comdat_group(Context<E> &ctx, std::string_view name);

template <typename E>
std::string_view demangle(const Symbol<E> &sym);

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym);

//
// Inline objects and functions
//

template <typename E>
inline std::ostream &
operator<<(std::ostream &out, const InputSection<E> &isec) {
  out << isec.file << ":(" << isec.name() << ")";
  return out;
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
    return (shdr().sh_flags & SHF_TLS) ? ".tls_common" : ".common";
  return file.shstrtab.data() + file.elf_sections[shndx].sh_name;
}

template <typename E>
inline i64 InputSection<E>::get_priority() const {
  return ((i64)file.priority << 32) | shndx;
}

template <typename E>
i64 get_addend(u8 *loc, const ElfRel<E> &rel);

template <typename E> requires (E::is_rela && !is_sh4<E>)
inline i64 get_addend(u8 *loc, const ElfRel<E> &rel) {
  return rel.r_addend;
}

template <typename E>
i64 get_addend(InputSection<E> &isec, const ElfRel<E> &rel) {
  return get_addend((u8 *)isec.contents.data() + rel.r_offset, rel);
}

template <typename E>
void write_addend(u8 *loc, i64 val, const ElfRel<E> &rel);

template <typename E> requires (E::is_rela && !is_sh4<E>)
void write_addend(u8 *loc, i64 val, const ElfRel<E> &rel) {}

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

  ElfShdr<E> &shdr = file.elf_sections[relsec_idx];
  if (shdr.sh_type == SHT_CREL)
    return file.decoded_crel[relsec_idx];
  return file.template get_data<ElfRel<E>>(ctx, shdr);
}

template <typename E>
inline std::span<FdeRecord<E>> InputSection<E>::get_fdes() const {
  if (fde_begin == -1)
    return {};
  std::span<FdeRecord<E>> span(file.fdes);
  return span.subspan(fde_begin, fde_end - fde_begin);
}

template <typename E>
std::pair<SectionFragment<E> *, i64>
InputSection<E>::get_fragment(Context<E> &ctx, const ElfRel<E> &rel) {
  assert(!(shdr().sh_flags & SHF_ALLOC));

  const ElfSym<E> &esym = file.elf_syms[rel.r_sym];
  if (esym.is_abs() || esym.is_common() || esym.is_undef())
    return {nullptr, 0};

  i64 shndx = file.get_shndx(esym);
  std::unique_ptr<MergeableSection<E>> &m = file.mergeable_sections[shndx];
  if (!m)
    return {nullptr, 0};

  if (esym.st_type == STT_SECTION)
    return m->get_fragment(esym.st_value + get_addend(*this, rel));

  std::pair<SectionFragment<E> *, i64> p = m->get_fragment(esym.st_value);
  return {p.first, p.second + get_addend(*this, rel)};
}

// Input object files may contain duplicate code for inline functions
// and such. Linkers de-duplicate them at link-time. However, linkers
// generaly don't remove debug info for de-duplicated functions because
// doing that requires parsing the entire debug section.
//
// Instead, linkers write "tombstone" values to dead debug info records
// instead of bogus values so that debuggers can skip them.
//
// This function returns a tombstone value for the symbol if the symbol
// refers a dead debug info section.
template <typename E>
inline std::optional<u64>
InputSection<E>::get_tombstone(Symbol<E> &sym, SectionFragment<E> *frag) {
  if (frag)
    return {};

  InputSection<E> *isec = sym.get_input_section();

  // Setting a tombstone is a special feature for a dead debug section.
  if (!isec || isec->is_alive)
    return {};

  std::string_view str = name();
  if (!str.starts_with(".debug"))
    return {};

  // If the section was dead due to ICF, we don't want to emit debug
  // info for that section but want to set real values to .debug_line so
  // that users can set a breakpoint inside a merged section.
  if (isec->icf_removed() && str == ".debug_line")
    return {};

  // 0 is an invalid value in most debug info sections, so we use it
  // as a tombstone value. .debug_loc and .debug_ranges reserve 0 as
  // the terminator marker, so we use 1 if that'str the case.
  return (str == ".debug_loc" || str == ".debug_ranges") ? 1 : 0;
}

template <typename E>
inline bool InputSection<E>::icf_removed() const {
  return this->leader && this->leader != this;
}

template <typename E>
inline void
InputSection<E>::check_range(Context<E> &ctx, i64 i, i64 val, i64 lo, i64 hi) {
  if (val < lo || hi <= val) {
    const ElfRel<E> &rel = get_rels(ctx)[i];
    Symbol<E> &sym = *file.symbols[rel.r_sym];
    Error(ctx) << *this << ": relocation " << rel << " against "
               << sym << " out of range: " << val << " is not in ["
               << lo << ", " << hi << ")";
  }
}

template <typename E>
std::pair<SectionFragment<E> *, i64>
MergeableSection<E>::get_fragment(i64 offset) {
  auto it = ranges::upper_bound(frag_offsets, offset);
  i64 idx = it - 1 - frag_offsets.begin();
  return {fragments[idx], offset - frag_offsets[idx]};
}

template <typename E>
std::string_view MergeableSection<E>::get_contents(i64 i) {
  i64 cur = frag_offsets[i];
  if (i == frag_offsets.size() - 1)
    return input_section->contents.substr(cur);
  return input_section->contents.substr(cur, frag_offsets[i + 1] - cur);
}

template <typename E>
template <typename T>
inline std::span<T>
InputFile<E>::get_data(Context<E> &ctx, const ElfShdr<E> &shdr) {
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
  if (elf_sections.size() <= idx)
    Fatal(ctx) << *this << ": invalid section index: " << idx;
  return this->get_string(ctx, elf_sections[idx]);
}

template <typename E>
inline std::span<Symbol<E> *> InputFile<E>::get_local_syms() {
  return std::span<Symbol<E> *>(this->symbols).subspan(0, this->first_global);
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
  if (esym.st_shndx >= SHN_LORESERVE)
    return 0;
  return esym.st_shndx;
}

template <typename E>
inline InputSection<E> *ObjectFile<E>::get_section(const ElfSym<E> &esym) {
  return sections[get_shndx(esym)].get();
}

template <typename E>
u64 Symbol<E>::get_addr(Context<E> &ctx, i64 flags) const {
  if (SectionFragment<E> *frag = get_frag()) {
    if (!frag->is_alive) {
      // This condition is met if a non-alloc section refers an
      // alloc section and if the referenced piece of data is
      // garbage-collected. Typically, this condition occurs if a
      // debug info section refers a string constant in .rodata.
      return 0;
    }

    return frag->get_addr(ctx) + value;
  }

  if (has_copyrel) {
    return is_copyrel_readonly
      ? ctx.copyrel_relro->shdr.sh_addr + value
      : ctx.copyrel->shdr.sh_addr + value;
  }

  if constexpr (is_ppc64v1<E>)
    if (!(flags & NO_OPD) && has_opd(ctx))
      return get_opd_addr(ctx);

  if (!(flags & NO_PLT) && has_plt(ctx)) {
    assert(is_imported || is_ifunc());
    return get_plt_addr(ctx);
  }

  InputSection<E> *isec = get_input_section();
  if (!isec)
    return value; // absolute symbol

  if (!isec->is_alive) {
    if (isec->icf_removed())
      return isec->leader->get_addr() + value;

    if (isec->name() == ".eh_frame") {
      // .eh_frame contents are parsed and reconstructed by the linker,
      // so pointing to a specific location in a source .eh_frame
      // section doesn't make much sense. However, CRT files contain
      // symbols pointing to the very beginning and ending of the section.
      //
      // If LTO is enabled, GCC may add `.lto_priv.<whatever>` as a symbol
      // suffix. That's why we use starts_with() instead of `==` here.
      if (name().starts_with("__EH_FRAME_BEGIN__") ||
          name().starts_with("__EH_FRAME_LIST__") ||
          name().starts_with(".eh_frame_seg") ||
          esym().st_type == STT_SECTION)
        return ctx.eh_frame->shdr.sh_addr;

      if (name().starts_with("__FRAME_END__") ||
          name().starts_with("__EH_FRAME_LIST_END__"))
        return ctx.eh_frame->shdr.sh_addr + ctx.eh_frame->shdr.sh_size;

      // ARM object files contain "$d" local symbol at the beginning
      // of data sections. Their values are not significant for .eh_frame,
      // so we just treat them as offset 0.
      if (name() == "$d" || name().starts_with("$d."))
        return ctx.eh_frame->shdr.sh_addr;

      Fatal(ctx) << "symbol referring to .eh_frame is not supported: "
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

template <typename E>
inline u64 Symbol<E>::get_got_addr(Context<E> &ctx) const {
  return ctx.got->shdr.sh_addr + get_got_idx(ctx) * sizeof(Word<E>);
}

template <typename E>
inline u64 Symbol<E>::get_gotplt_addr(Context<E> &ctx) const {
  assert(get_plt_idx(ctx) != -1);
  return ctx.gotplt->shdr.sh_addr + GotPltSection<E>::HDR_SIZE +
         get_plt_idx(ctx) * GotPltSection<E>::ENTRY_SIZE;
}

template <typename E>
inline u64 Symbol<E>::get_gottp_addr(Context<E> &ctx) const {
  assert(get_gottp_idx(ctx) != -1);
  return ctx.got->shdr.sh_addr + get_gottp_idx(ctx) * sizeof(Word<E>);
}

template <typename E>
inline u64 Symbol<E>::get_tlsgd_addr(Context<E> &ctx) const {
  assert(get_tlsgd_idx(ctx) != -1);
  return ctx.got->shdr.sh_addr + get_tlsgd_idx(ctx) * sizeof(Word<E>);
}

template <typename E>
inline u64 Symbol<E>::get_tlsdesc_addr(Context<E> &ctx) const {
  assert(get_tlsdesc_idx(ctx) != -1);
  return ctx.got->shdr.sh_addr + get_tlsdesc_idx(ctx) * sizeof(Word<E>);
}

template <typename E>
inline u64 to_plt_offset(i32 pltidx) {
  if constexpr (is_ppc64v1<E>) {
    // The PPC64 ELFv1 ABI requires PLT entries to vary in size
    // depending on their indices. For entries whose PLT index is
    // less than 32768, the entry size is 8 bytes. Other entries are
    // 12 bytes long.
    if (pltidx < 0x8000)
      return E::plt_hdr_size + pltidx * 8;
    return E::plt_hdr_size + 0x8000 * 8 + (pltidx - 0x8000) * 12;
  } else {
    return E::plt_hdr_size + pltidx * E::plt_size;
  }
}

template <typename E>
inline u64 Symbol<E>::get_plt_addr(Context<E> &ctx) const {
  if (i32 idx = get_plt_idx(ctx); idx != -1)
    return ctx.plt->shdr.sh_addr + to_plt_offset<E>(idx);
  return ctx.pltgot->shdr.sh_addr + get_pltgot_idx(ctx) * E::pltgot_size;
}

template <typename E>
inline u64 Symbol<E>::get_opd_addr(Context<E> &ctx) const {
  assert(get_opd_idx(ctx) != -1);
  return ctx.extra.opd->shdr.sh_addr +
         get_opd_idx(ctx) * PPC64OpdSection::ENTRY_SIZE;
}

template <typename E>
inline u64 Symbol<E>::get_got_pltgot_addr(Context<E> &ctx) const {
  // An ifunc symbol occupies two consecutive GOT slots in a
  // position-dependent executable (PDE). The first slot contains the
  // symbol's PLT address, and the second slot holds the resolved
  // address. A PDE uses the ifunc symbol's PLT entry as the address
  // for the symbol, akin to a canonical PLT.
  //
  // This function returns the address that the PLT entry should use
  // to jump to the resolved address.
  //
  // Note that we don't use this function for PPC64. In PPC64, symbols
  // are always accessed through the TOC table regardless of the
  // -fno-PIE setting. We don't need canonical PLTs on the psABIs too.
  if (is_pde_ifunc(ctx))
    return get_got_addr(ctx) + sizeof(Word<E>);
  return get_got_addr(ctx);
}

template <typename E>
inline void Symbol<E>::set_got_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].got_idx < 0);
  ctx.symbol_aux[aux_idx].got_idx = idx;
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
inline void Symbol<E>::set_opd_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  assert(ctx.symbol_aux[aux_idx].opd_idx < 0);
  ctx.symbol_aux[aux_idx].opd_idx = idx;
}

template <typename E>
inline void Symbol<E>::set_dynsym_idx(Context<E> &ctx, i32 idx) {
  assert(aux_idx != -1);
  ctx.symbol_aux[aux_idx].dynsym_idx = idx;
}

template <typename E>
inline i32 Symbol<E>::get_got_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].got_idx;
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
inline i32 Symbol<E>::get_opd_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].opd_idx;
}

template <typename E>
inline i32 Symbol<E>::get_dynsym_idx(Context<E> &ctx) const {
  return (aux_idx == -1) ? -1 : ctx.symbol_aux[aux_idx].dynsym_idx;
}

template <typename E>
inline u32 Symbol<E>::get_djb_hash(Context<E> &ctx) const {
  assert(aux_idx != -1);
  return ctx.symbol_aux[aux_idx].djb_hash;
}

template <typename E>
inline void Symbol<E>::set_djb_hash(Context<E> &ctx, u32 hash) {
  assert(aux_idx != -1);
  ctx.symbol_aux[aux_idx].djb_hash = hash;
}

template <typename E>
u64
Symbol<E>::get_thunk_addr(Context<E> &ctx, u64 P) const requires needs_thunk<E> {
  std::span<u64> vec = ctx.symbol_aux[aux_idx].thunk_addrs;

  u64 lo = (P < branch_distance<E>) ? 0 : P - branch_distance<E>;
  auto it = ranges::lower_bound(vec, lo);
  if (it == vec.end())
    Fatal(ctx) << "range extension thunk out of range: " << *this;

  i64 disp = *it - P;
  if (disp < -branch_distance<E> || branch_distance<E> <= disp)
    Fatal(ctx) << "range extension thunk out of range: " << *this;
  return *it;
}

template <typename E>
inline bool Symbol<E>::has_plt(Context<E> &ctx) const {
  return get_plt_idx(ctx) != -1 || get_pltgot_idx(ctx) != -1;
}

template <typename E>
inline bool Symbol<E>::is_absolute() const {
  // An unresolved weak symbol acts as if it were an absolute address
  // at address 0
  if (is_remaining_undef_weak())
    return true;

  return !is_imported && !get_frag() && !get_input_section() &&
         !get_output_section();
}

template <typename E>
inline bool Symbol<E>::is_local(Context<E> &ctx) const {
  if (ctx.arg.relocatable)
    return esym().st_bind == STB_LOCAL;
  return !is_imported && !is_exported;
}

template <typename E>
inline bool Symbol<E>::is_pde_ifunc(Context<E> &ctx) const {
  // Returns true if this is an ifunc tha uses two GOT slots
  return is_ifunc() && !ctx.arg.pic && !is_ppc64<E>;
}

// A remaining weak undefined symbol is promoted to a dynamic symbol
// in DSO and resolved to 0 in an executable. This function returns
// true if it's latter.
template <typename E>
inline bool Symbol<E>::is_remaining_undef_weak() const {
  return !is_imported && esym().is_undef_weak();
}

// Returns true if the symbol's PC-relative address is known at link-time.
template <typename E>
inline bool Symbol<E>::is_pcrel_linktime_const(Context<E> &ctx) const {
  return !is_imported && !is_ifunc() && (is_relative() || !ctx.arg.pic);
}

// Returns true if the symbol's Thread Pointer-relative address is
// known at link-time.
template <typename E>
inline bool Symbol<E>::is_tprel_linktime_const(Context<E> &ctx) const {
  assert(get_type() == STT_TLS);
  return !ctx.arg.shared && !is_imported;
}

// Returns true if the symbol's Thread Pointer-relative address is
// known at load-time.
template <typename E>
inline bool Symbol<E>::is_tprel_runtime_const(Context<E> &ctx) const {
  // Returns true unless we are creating a dlopen'able DSO.
  assert(get_type() == STT_TLS);
  return !(ctx.arg.shared && ctx.arg.z_dlopen);
}

template <typename E>
inline InputSection<E> *Symbol<E>::get_input_section() const {
  if ((origin & TAG_MASK) == TAG_ISEC)
    return (InputSection<E> *)(origin & ~TAG_MASK);
  return nullptr;
}

template <typename E>
inline Chunk<E> *Symbol<E>::get_output_section() const {
  if ((origin & TAG_MASK) == TAG_OSEC)
    return (Chunk<E> *)(origin & ~TAG_MASK);
  return nullptr;
}

template <typename E>
inline SectionFragment<E> *Symbol<E>::get_frag() const {
  if ((origin & TAG_MASK) == TAG_FRAG)
    return (SectionFragment<E> *)(origin & ~TAG_MASK);
  return nullptr;
}

template <typename E>
inline void Symbol<E>::set_input_section(InputSection<E> *isec) {
  uintptr_t addr = (uintptr_t)isec;
  assert((addr & TAG_MASK) == 0);
  origin = addr | TAG_ISEC;
}

template <typename E>
inline void Symbol<E>::set_output_section(Chunk<E> *osec) {
  uintptr_t addr = (uintptr_t)osec;
  assert((addr & TAG_MASK) == 0);
  origin = addr | TAG_OSEC;
}

template <typename E>
inline void Symbol<E>::set_frag(SectionFragment<E> *frag) {
  uintptr_t addr = (uintptr_t)frag;
  assert((addr & TAG_MASK) == 0);
  origin = addr | TAG_FRAG;
}

template <typename E>
inline u32 Symbol<E>::get_type() const {
  if (esym().st_type == STT_GNU_IFUNC && file->is_dso)
    return STT_FUNC;
  return esym().st_type;
}

template <typename E>
inline std::string_view Symbol<E>::get_version() const {
  if (file->is_dso) {
    std::span<std::string_view> vers = ((SharedFile<E> *)file)->version_strings;
    if (!vers.empty())
      return vers[ver_idx];
  }
  return "";
}

template <typename E>
inline i64 Symbol<E>::get_output_sym_idx(Context<E> &ctx) const {
  i64 i = file->output_sym_indices[sym_idx];
  assert(i != -1);
  if (is_local(ctx))
    return file->local_symtab_idx + i;
  return file->global_symtab_idx + i;
}

template <typename E>
inline const ElfSym<E> &Symbol<E>::esym() const {
  return file->elf_syms[sym_idx];
}

template <typename E>
inline void Symbol<E>::set_name(std::string_view name) {
  nameptr = name.data();
  namelen = name.size();
}

template <typename E>
inline std::string_view Symbol<E>::name() const {
  return {nameptr, (size_t)namelen};
}

template <typename E>
inline void Symbol<E>::add_aux(Context<E> &ctx) {
  if (aux_idx == -1) {
    aux_idx = ctx.symbol_aux.size();
    ctx.symbol_aux.resize(aux_idx + 1);
  }
}

inline bool is_c_identifier(std::string_view s) {
  if (s.empty())
    return false;

  auto is_alpha = [](char c) {
    return c == '_' || ('a' <= c && c <= 'z') || ('A' <= c && c <= 'Z');
  };

  auto is_alnum = [&](char c) {
    return is_alpha(c) || ('0' <= c && c <= '9');
  };

  if (!is_alpha(s[0]))
    return false;
  for (i64 i = 1; i < s.size(); i++)
    if (!is_alnum(s[i]))
      return false;
  return true;
}

template <typename E>
std::string_view save_string(Context<E> &ctx, const std::string &str) {
  u8 *buf = new u8[str.size() + 1];
  memcpy(buf, str.data(), str.size());
  buf[str.size()] = '\0';
  ctx.string_pool.emplace_back(buf);
  return {(char *)buf, str.size()};
}

} // namespace mold
