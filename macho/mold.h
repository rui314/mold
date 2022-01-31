#pragma once

#include "macho.h"
#include "../mold.h"

#include <map>
#include <memory>
#include <span>
#include <tbb/concurrent_hash_map.h>
#include <tbb/spin_mutex.h>
#include <unordered_map>
#include <variant>

namespace mold::macho {

static constexpr i64 COMMON_PAGE_SIZE = 0x4000;
static constexpr i64 crypto_hash_sha256_BYTES = 32;

template <typename E> class Chunk;
template <typename E> class InputSection;
template <typename E> class OutputSection;
template <typename E> class Subsection;
template <typename E> struct Context;
template <typename E> struct Symbol;

//
// object-file.cc
//

template <typename E>
struct Relocation {
  u32 offset = 0;
  u8 type = -1;
  u8 p2size = 0;
  bool is_pcrel = false;
  i64 addend = 0;
  Symbol<E> *sym = nullptr;
  Subsection<E> *subsec = nullptr;
};

template <typename E>
struct UnwindRecord {
  UnwindRecord(u32 len, u32 enc) : code_len(len), encoding(enc) {}

  inline u64 get_func_raddr(Context<E> &ctx) const;

  Subsection<E> *subsec = nullptr;
  u32 offset = 0;
  u32 code_len;
  u32 encoding;
  Symbol<E> *personality = nullptr;
  Subsection<E> *lsda = nullptr;
  u32 lsda_offset = 0;
  bool is_alive = false;
};

template <typename E>
class InputFile {
public:
  MappedFile<Context<E>> *mf = nullptr;
  std::vector<Symbol<E> *> syms;
  i64 priority = 0;
  bool is_dylib = false;
  std::atomic_bool is_alive = false;
  std::string archive_name;

protected:
  InputFile() = default;
};

template <typename E>
class ObjectFile : public InputFile<E> {
public:
  ObjectFile() = default;

  static ObjectFile *create(Context<E> &ctx, MappedFile<Context<E>> *mf,
                            std::string archive_name);
  void parse(Context<E> &ctx);
  Subsection<E> *find_subsection(Context<E> &ctx, u32 addr);
  void parse_compact_unwind(Context<E> &ctx, MachSection &hdr);
  void resolve_regular_symbols(Context<E> &ctx);
  void resolve_lazy_symbols(Context<E> &ctx);
  bool is_objc_object(Context<E> &ctx);
  std::vector<ObjectFile *> mark_live_objects(Context<E> &ctx);
  void convert_common_symbols(Context<E> &ctx);
  void check_duplicate_symbols(Context<E> &ctx);

  Relocation<E> read_reloc(Context<E> &ctx, const MachSection &hdr, MachRel r);

  std::vector<std::unique_ptr<InputSection<E>>> sections;
  std::vector<std::unique_ptr<Subsection<E>>> subsections;
  std::vector<u32> sym_to_subsec;
  std::span<MachSym> mach_syms;
  std::vector<Symbol<E>> local_syms;
  std::vector<UnwindRecord<E>> unwind_records;
  std::span<DataInCodeEntry> data_in_code_entries;

private:
  void parse_sections(Context<E> &ctx);
  void parse_symtab(Context<E> &ctx);
  void split_subsections(Context<E> &ctx);
  void parse_data_in_code(Context<E> &ctx);
  LoadCommand *find_load_command(Context<E> &ctx, u32 type);
  i64 find_subsection_idx(Context<E> &ctx, u32 addr);
  void override_symbol(Context<E> &ctx, i64 symidx);
  InputSection<E> *get_common_sec(Context<E> &ctx);

  MachSection *unwind_sec = nullptr;
  std::unique_ptr<MachSection> common_hdr;
  InputSection<E> *common_sec = nullptr;
};

template <typename E>
class DylibFile : public InputFile<E> {
public:
  static DylibFile *create(Context<E> &ctx, MappedFile<Context<E>> *mf);
  void parse(Context<E> &ctx);
  void resolve_symbols(Context<E> &ctx);

  std::string_view install_name;
  i64 dylib_idx = 0;
  std::atomic_bool is_needed = false;

private:
  void parse_dylib(Context<E> &ctx);
  void read_trie(Context<E> &ctx, u8 *start, i64 offset = 0,
                 const std::string &prefix = "");

  DylibFile() {
    this->is_dylib = true;
    this->is_alive = true;
  }
};

template <typename E>
std::ostream &operator<<(std::ostream &out, const InputFile<E> &file);

//
// input-sections.cc
//

template <typename E>
class InputSection {
public:
  InputSection(Context<E> &ctx, ObjectFile<E> &file, const MachSection &hdr);
  void parse_relocations(Context<E> &ctx);

  ObjectFile<E> &file;
  const MachSection &hdr;
  OutputSection<E> &osec;
  std::string_view contents;
  std::vector<Symbol<E> *> syms;
  std::vector<Relocation<E>> rels;
};

template <typename E>
std::ostream &operator<<(std::ostream &out, const InputSection<E> &sec);

template <typename E>
class Subsection {
public:
  inline u64 get_addr(Context<E> &ctx) const;

  std::string_view get_contents() {
    assert(isec.hdr.type != S_ZEROFILL);
    return isec.contents.substr(input_offset, input_size);
  }

  std::span<UnwindRecord<E>> get_unwind_records() {
    return std::span<UnwindRecord<E>>(isec.file.unwind_records)
      .subspan(unwind_offset, nunwind);
  }

  std::span<Relocation<E>> get_rels() const {
    return std::span<Relocation<E>>(isec.rels).subspan(rel_offset, nrels);
  }

  void scan_relocations(Context<E> &ctx);
  void apply_reloc(Context<E> &ctx, u8 *buf);

  InputSection<E> &isec;
  u32 input_offset = 0;
  u32 input_size = 0;
  u32 input_addr = 0;
  u32 rel_offset = 0;
  u32 nrels = 0;
  u32 unwind_offset = 0;
  u32 nunwind = 0;
  u32 raddr = -1;
  u16 p2align = 0;
  std::atomic_bool is_alive = false;
};

template <typename E>
std::vector<Relocation<E>>
read_relocations(Context<E> &ctx, ObjectFile<E> &file, const MachSection &hdr);

//
// Symbol
//

enum {
  NEEDS_GOT        = 1 << 0,
  NEEDS_STUB       = 1 << 1,
  NEEDS_THREAD_PTR = 1 << 2,
};

template <typename E>
struct Symbol {
  Symbol() = default;
  Symbol(std::string_view name) : name(name) {}
  Symbol(const Symbol<E> &other) : name(other.name) {}

  std::string_view name;
  InputFile<E> *file = nullptr;
  Subsection<E> *subsec = nullptr;
  u64 value = 0;

  i32 stub_idx = -1;
  i32 got_idx = -1;
  i32 tlv_idx = -1;

  tbb::spin_mutex mu;

  std::atomic_uint8_t flags = 0;

  u8 is_extern : 1 = false;
  u8 is_lazy : 1 = false;
  u8 is_common : 1 = false;
  u8 referenced_dynamically : 1 = false;

  inline u64 get_addr(Context<E> &ctx) const;
  inline u64 get_got_addr(Context<E> &ctx) const;
  inline u64 get_tlv_addr(Context<E> &ctx) const;
};

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym);

//
// output-chunks.cc
//

template <typename E>
class OutputSegment {
public:
  static OutputSegment<E> *
  get_instance(Context<E> &ctx, std::string_view name);

  void set_offset(Context<E> &ctx, i64 fileoff, u64 vmaddr);
  void copy_buf(Context<E> &ctx);

  std::string_view name;
  SegmentCommand cmd = {};
  i32 seg_idx = -1;
  std::vector<Chunk<E> *> chunks;

private:
  OutputSegment(std::string_view name);
};

template <typename E>
class Chunk {
public:
  inline Chunk(Context<E> &ctx, std::string_view segname,
               std::string_view sectname);

  virtual ~Chunk() = default;
  virtual void compute_size(Context<E> &ctx) {};
  virtual void copy_buf(Context<E> &ctx) {}

  MachSection hdr = {};
  u32 sect_idx = 0;
  bool is_hidden = false;
  bool is_regular = false;
};

template <typename E>
std::ostream &operator<<(std::ostream &out, const Chunk<E> &chunk);

template <typename E>
class OutputMachHeader : public Chunk<E> {
public:
  OutputMachHeader(Context<E> &ctx)
    : Chunk<E>(ctx, "__TEXT", "__mach_header") {
    this->is_hidden = true;
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class OutputSection : public Chunk<E> {
public:
  static OutputSection<E> *
  get_instance(Context<E> &ctx, std::string_view segname,
               std::string_view sectname);

  OutputSection(Context<E> &ctx, std::string_view segname,
                std::string_view sectname)
    : Chunk<E>(ctx, segname, sectname) {
    this->is_regular = true;
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  void add_subsec(Subsection<E> *subsec) {
    members.push_back(subsec);
    this->hdr.p2align = std::max<u32>(this->hdr.p2align, subsec->p2align);
    this->hdr.attr |= subsec->isec.hdr.attr;
    this->hdr.type = subsec->isec.hdr.type;
  }

  std::vector<Subsection<E> *> members;
};

class RebaseEncoder {
public:
  RebaseEncoder();
  void add(i64 seg_idx, i64 offset);
  void flush();
  void finish();

  std::vector<u8> buf;

private:
  i64 cur_seg = -1;
  i64 cur_off = 0;
  i64 times = 0;
};

template <typename E>
class OutputRebaseSection : public Chunk<E> {
public:
  OutputRebaseSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__rebase") {
    this->is_hidden = true;
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

class BindEncoder {
public:
  BindEncoder();
  void add(i64 dylib_idx, std::string_view sym, i64 flags, i64 seg_idx,
           i64 offset);
  void finish();

  std::vector<u8> buf;

private:
  std::string_view last_sym;
  i64 last_flags = -1;
  i64 last_dylib = -1;
  i64 last_seg = -1;
  i64 last_off = -1;
};

template <typename E>
class OutputBindSection : public Chunk<E> {
public:
  OutputBindSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__binding") {
    this->is_hidden = true;
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

template <typename E>
class OutputLazyBindSection : public Chunk<E> {
public:
  OutputLazyBindSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__lazy_binding") {
    this->is_hidden = true;
    this->hdr.p2align = std::countr_zero(8U);
  }

  void add(Context<E> &ctx, Symbol<E> &sym, i64 flags);

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

class ExportEncoder {
public:
  void add(std::string_view name, u32 flags, u64 addr);
  i64 finish();

  void write_trie(u8 *buf) {
    write_trie(buf, root);
  }

private:
  struct Entry {
    std::string_view name;
    u32 flags;
    u64 addr;
  };

  struct TrieNode {
    std::string_view prefix;
    bool is_leaf = false;
    u32 flags = 0;
    u64 addr = 0;
    u32 offset = -1;
    std::vector<TrieNode> children;
  };

  static i64 common_prefix_len(std::span<Entry> entries, i64 len);
  static TrieNode construct_trie(std::span<Entry> entries, i64 len);
  static i64 set_offset(TrieNode &node, i64 offset);
  void write_trie(u8 *buf, TrieNode &node);

  TrieNode root;
  std::vector<Entry> entries;
};

template <typename E>
class OutputExportSection : public Chunk<E> {
public:
  OutputExportSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__export") {
    this->is_hidden = true;
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

private:
  ExportEncoder enc;
};

template <typename E>
class OutputFunctionStartsSection : public Chunk<E> {
public:
  OutputFunctionStartsSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__func_starts") {
    this->is_hidden = true;
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

template <typename E>
class OutputSymtabSection : public Chunk<E> {
public:
  OutputSymtabSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__symbol_table") {
    this->is_hidden = true;
    this->hdr.p2align = std::countr_zero(8U);
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  struct Entry {
    Symbol<E> *sym;
    i64 stroff;
  };

  std::vector<Entry> locals;
  std::vector<Entry> globals;
  std::vector<Entry> undefs;
};

template <typename E>
class OutputStrtabSection : public Chunk<E> {
public:
  OutputStrtabSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__string_table") {
    this->is_hidden = true;
    this->hdr.p2align = std::countr_zero(8U);
  }

  i64 add_string(std::string_view str);
  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::string contents{1, '\0'};
};

template <typename E>
class OutputIndirectSymtabSection : public Chunk<E> {
public:
  OutputIndirectSymtabSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__ind_sym_tab") {
    this->is_hidden = true;
  }

  static constexpr i64 ENTRY_SIZE = 4;

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  struct Entry {
    Symbol<E> *sym;
    i64 symtab_idx;
  };

  std::vector<Entry> stubs;
  std::vector<Entry> gots;
};

template <typename E>
class CodeSignatureSection : public Chunk<E> {
public:
  CodeSignatureSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__code_signature") {
    this->is_hidden = true;
    this->hdr.p2align = std::countr_zero(16U);
  }

  void compute_size(Context<E> &ctx) override;
  void write_signature(Context<E> &ctx);

  static constexpr i64 BLOCK_SIZE = 4096;
};

template <typename E>
class DataInCodeSection : public Chunk<E> {
public:
  DataInCodeSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__LINKEDIT", "__data_in_code") {
    this->is_hidden = true;
    this->hdr.p2align = std::countr_zero(alignof(DataInCodeEntry));
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<DataInCodeEntry> contents;
};

template <typename E>
class StubsSection : public Chunk<E> {
public:
  StubsSection(Context<E> &ctx) : Chunk<E>(ctx, "__TEXT", "__stubs") {
    this->hdr.p2align = std::countr_zero(2U);
    this->hdr.type = S_SYMBOL_STUBS;
    this->hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
    this->hdr.reserved2 = E::stub_size;
  }

  void add(Context<E> &ctx, Symbol<E> *sym);
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> syms;
  std::vector<u32> bind_offsets;
};

template <typename E>
class StubHelperSection : public Chunk<E> {
public:
  StubHelperSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__TEXT", "__stub_helper") {
    this->hdr.p2align = std::countr_zero(4U);
    this->hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  }

  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class UnwindEncoder {
public:
  std::vector<u8> encode(Context<E> &ctx, std::span<UnwindRecord<E>> records);

private:
  u32 encode_personality(Context<E> &ctx, Symbol<E> *sym);

  std::vector<std::span<UnwindRecord<E>>>
  split_records(Context<E> &ctx, std::span<UnwindRecord<E>>);

  std::vector<Symbol<E> *> personalities;
};

template <typename E>
class UnwindInfoSection : public Chunk<E> {
public:
  UnwindInfoSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__TEXT", "__unwind_info") {
    this->hdr.p2align = std::countr_zero(4U);
  }

  void compute_size(Context<E> &ctx) override;
  void copy_buf(Context<E> &ctx) override;

  std::vector<u8> contents;
};

template <typename E>
class GotSection : public Chunk<E> {
public:
  GotSection(Context<E> &ctx) : Chunk<E>(ctx, "__DATA_CONST", "__got") {
    this->hdr.p2align = std::countr_zero(8U);
    this->hdr.type = S_NON_LAZY_SYMBOL_POINTERS;
  }

  void add(Context<E> &ctx, Symbol<E> *sym);
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> syms;
};

template <typename E>
class LazySymbolPtrSection : public Chunk<E> {
public:
  LazySymbolPtrSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__DATA", "__la_symbol_ptr") {
    this->hdr.p2align = std::countr_zero(8U);
    this->hdr.type = S_LAZY_SYMBOL_POINTERS;
  }

  void copy_buf(Context<E> &ctx) override;
};

template <typename E>
class ThreadPtrsSection : public Chunk<E> {
public:
  ThreadPtrsSection(Context<E> &ctx)
    : Chunk<E>(ctx, "__DATA", "__thread_ptrs") {
    this->hdr.p2align = std::countr_zero(8U);
    this->hdr.type = S_THREAD_LOCAL_VARIABLE_POINTERS;
  }

  void add(Context<E> &ctx, Symbol<E> *sym);
  void copy_buf(Context<E> &ctx) override;

  std::vector<Symbol<E> *> syms;
};

//
// mapfile.cc
//

template <typename E>
void print_map(Context<E> &ctx);

//
// dumper.cc
//

void dump_file(std::string path);

//
// output-file.cc
//

template <typename E>
class OutputFile {
public:
  static std::unique_ptr<OutputFile>
  open(Context<E> &ctx, std::string path, i64 filesize, i64 perm);

  virtual void close(Context<E> &ctx) = 0;
  virtual ~OutputFile() {}

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
// yaml.cc
//

struct YamlNode {
  std::variant<std::string_view,
               std::vector<YamlNode>,
               std::map<std::string_view, YamlNode>> data;
};

struct YamlError {
  std::string msg;
  i64 pos;
};

std::variant<std::vector<YamlNode>, YamlError>
parse_yaml(std::string_view str);

//
// tapi.cc
//

struct TextDylib {
  std::string_view uuid;
  std::string_view install_name;
  std::string_view current_version = "1.0";
  std::string_view parent_umbrella;
  std::vector<std::string_view> reexported_libs;
  std::vector<std::string_view> exports;
};

template <typename E>
TextDylib parse_tbd(Context<E> &ctx, MappedFile<Context<E>> *mf);

//
// cmdline.cc
//

template <typename E>
void parse_nonpositional_args(Context<E> &ctx,
                              std::vector<std::string> &remaining);

//
// dead-strip.cc
//

template <typename E>
void dead_strip(Context<E> &ctx);

//
// main.cc
//

template <typename E>
struct Context {
  Context() {
    text_seg = OutputSegment<E>::get_instance(*this, "__TEXT");
    data_const_seg = OutputSegment<E>::get_instance(*this, "__DATA_CONST");
    data_seg = OutputSegment<E>::get_instance(*this, "__DATA");
    linkedit_seg = OutputSegment<E>::get_instance(*this, "__LINKEDIT");

    text = OutputSection<E>::get_instance(*this, "__TEXT", "__text");
    data = OutputSection<E>::get_instance(*this, "__DATA", "__data");
    bss = OutputSection<E>::get_instance(*this, "__DATA", "__bss");
    cstring = OutputSection<E>::get_instance(*this, "__TEXT", "__cstring");
    common = OutputSection<E>::get_instance(*this, "__DATA", "__common");

    bss->hdr.type = S_ZEROFILL;
    cstring->hdr.type = S_CSTRING_LITERALS;
    common->hdr.type = S_ZEROFILL;
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
    bool ObjC = false;
    bool adhoc_codesign = true;
    bool color_diagnostics = false;
    bool dead_strip = true;
    bool dead_strip_dylibs = false;
    bool deduplicate = true;
    bool demangle = false;
    bool dylib = false;
    bool dynamic = true;
    bool fatal_warnings = false;
    bool trace = false;
    i64 arch = CPU_TYPE_ARM64;
    i64 headerpad = 256;
    i64 pagezero_size = 0;
    i64 platform = PLATFORM_MACOS;
    i64 platform_min_version = 0;
    i64 platform_sdk_version = 0;
    std::string chroot;
    std::string entry = "_main";
    std::string map;
    std::string output = "a.out";
    std::vector<std::string> framework_paths;
    std::vector<std::string> library_paths;
    std::vector<std::string> rpath;
    std::vector<std::string> syslibroot;
  } arg;

  std::vector<std::string_view> cmdline_args;
  u32 output_type = MH_EXECUTE;

  bool has_error = false;

  tbb::concurrent_hash_map<std::string_view, Symbol<E>> symbol_map;

  std::unique_ptr<OutputFile<E>> output_file;
  u8 *buf;

  tbb::concurrent_vector<std::unique_ptr<ObjectFile<E>>> obj_pool;
  tbb::concurrent_vector<std::unique_ptr<DylibFile<E>>> dylib_pool;
  tbb::concurrent_vector<std::unique_ptr<u8[]>> string_pool;
  tbb::concurrent_vector<std::unique_ptr<MappedFile<Context<E>>>> mf_pool;
  std::vector<std::unique_ptr<OutputSection<E>>> osec_pool;

  tbb::concurrent_vector<std::unique_ptr<TimerRecord>> timer_records;

  std::vector<ObjectFile<E> *> objs;
  std::vector<DylibFile<E> *> dylibs;

  OutputSegment<E> *text_seg = nullptr;
  OutputSegment<E> *data_const_seg = nullptr;
  OutputSegment<E> *data_seg = nullptr;
  OutputSegment<E> *linkedit_seg = nullptr;

  std::vector<std::unique_ptr<OutputSegment<E>>> segments;
  std::vector<Chunk<E> *> chunks;

  OutputMachHeader<E> mach_hdr{*this};
  StubsSection<E> stubs{*this};
  StubHelperSection<E> stub_helper{*this};
  UnwindInfoSection<E> unwind_info{*this};
  GotSection<E> got{*this};
  LazySymbolPtrSection<E> lazy_symbol_ptr{*this};
  CodeSignatureSection<E> code_sig{*this};
  DataInCodeSection<E> data_in_code{*this};
  ThreadPtrsSection<E> thread_ptrs{*this};

  OutputRebaseSection<E> rebase{*this};
  OutputBindSection<E> bind{*this};
  OutputLazyBindSection<E> lazy_bind{*this};
  OutputExportSection<E> export_{*this};
  OutputFunctionStartsSection<E> function_starts{*this};
  OutputSymtabSection<E> symtab{*this};
  OutputIndirectSymtabSection<E> indir_symtab{*this};
  OutputStrtabSection<E> strtab{*this};

  OutputSection<E> *text = nullptr;
  OutputSection<E> *data = nullptr;
  OutputSection<E> *bss = nullptr;
  OutputSection<E> *cstring = nullptr;
  OutputSection<E> *common = nullptr;
};

int main(int argc, char **argv);

//
// Inline functions
//

template <typename E>
std::ostream &operator<<(std::ostream &out, const InputSection<E> &sec) {
  out << sec.file << "(" << sec.hdr.get_segname() << ","
      << sec.hdr.get_sectname() << ")";
  return out;
}

template <typename E>
u64 Subsection<E>::get_addr(Context<E> &ctx) const {
  return ctx.arg.pagezero_size + raddr;
}

template <typename E>
u64 Symbol<E>::get_addr(Context<E> &ctx) const {
  if (subsec)
    return subsec->get_addr(ctx) + value;
  if (stub_idx != -1)
    return ctx.stubs.hdr.addr + stub_idx * E::stub_size;
  return value;
}

template <typename E>
u64 Symbol<E>::get_got_addr(Context<E> &ctx) const {
  assert(got_idx != -1);
  return ctx.got.hdr.addr + got_idx * E::word_size;
}

template <typename E>
u64 Symbol<E>::get_tlv_addr(Context<E> &ctx) const {
  assert(tlv_idx != -1);
  return ctx.thread_ptrs.hdr.addr + tlv_idx * E::word_size;
}

template <typename E>
inline Symbol<E> *get_symbol(Context<E> &ctx, std::string_view name) {
  typename decltype(ctx.symbol_map)::const_accessor acc;
  ctx.symbol_map.insert(acc, {name, Symbol<E>(name)});
  return (Symbol<E> *)(&acc->second);
}

template <typename E>
inline std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym) {
  out << sym.name;
  return out;
}

template <typename E>
Chunk<E>::Chunk(Context<E> &ctx, std::string_view segname,
                std::string_view sectname) {
  ctx.chunks.push_back(this);
  hdr.set_segname(segname);
  hdr.set_sectname(sectname);
}

template <typename E>
u64 UnwindRecord<E>::get_func_raddr(Context<E> &ctx) const {
  return subsec->raddr + offset;
}

} // namespace mold::macho
