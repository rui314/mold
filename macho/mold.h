#pragma once

#include "macho.h"
#include "../mold.h"

#include <map>
#include <memory>
#include <tbb/concurrent_hash_map.h>
#include <tbb/spin_mutex.h>
#include <unordered_map>
#include <variant>

namespace mold::macho {

static constexpr i64 PAGE_SIZE = 0x4000;
static constexpr i64 PAGE_ZERO_SIZE = 0x100000000;
static constexpr i64 SHA256_SIZE = 32;

class Chunk;
class OutputSection;
struct Context;
struct InputSection;
struct Subsection;
struct Symbol;

//
// object-file.cc
//

struct Relocation {
  u32 offset = 0;
  u8 type = -1;
  u8 p2size = 0;
  bool is_pcrel = false;
  i64 addend = 0;
  Symbol *sym = nullptr;
  Subsection *subsec = nullptr;
};

struct UnwindRecord {
  UnwindRecord(u32 len, u32 enc) : code_len(len), encoding(enc) {}

  inline u64 get_func_raddr(Context &ctx) const;

  Subsection *subsec = nullptr;
  u32 offset = 0;
  u32 code_len;
  u32 encoding;
  Symbol *personality = nullptr;
  Subsection *lsda = nullptr;
  u32 lsda_offset = 0;
};

class InputFile {
public:
  MappedFile<Context> *mf = nullptr;
  std::vector<Symbol *> syms;
  i64 priority = 0;
  bool is_dylib = false;
  std::atomic_bool is_alive = false;
  std::string archive_name;

protected:
  InputFile() = default;
};

class ObjectFile : public InputFile {
public:
  ObjectFile() = default;

  static ObjectFile *create(Context &ctx, MappedFile<Context> *mf,
                            std::string archive_name);
  void parse(Context &ctx);
  void parse_compact_unwind(Context &ctx, MachSection &hdr);
  void resolve_regular_symbols(Context &ctx);
  void resolve_lazy_symbols(Context &ctx);
  std::vector<ObjectFile *> mark_live_objects(Context &ctx);
  void convert_common_symbols(Context &ctx);

  Relocation read_reloc(Context &ctx, const MachSection &hdr, MachRel r);

  std::vector<std::unique_ptr<InputSection>> sections;
  std::span<MachSym> mach_syms;
  std::vector<UnwindRecord> unwind_records;
  std::span<DataInCodeEntry> data_in_code_entries;

private:
  void override_symbol(Context &ctx, Symbol &sym, MachSym &msym);
  InputSection *get_common_sec(Context &ctx);

  std::unique_ptr<MachSection> common_hdr;
  InputSection *common_sec = nullptr;
};

class DylibFile : public InputFile {
public:
  static DylibFile *create(Context &ctx, MappedFile<Context> *mf);
  void parse(Context &ctx);
  void resolve_symbols(Context &ctx);

  std::string_view install_name;
  i64 dylib_idx = 0;

private:
  void parse_dylib(Context &ctx);
  void read_trie(Context &ctx, u8 *start, i64 offset,
                 const std::string &prefix);

  DylibFile() {
    is_dylib = true;
    is_alive = true;
  }
};

std::ostream &operator<<(std::ostream &out, const InputFile &file);

//
// input-sections.cc
//

class InputSection {
public:
  InputSection(Context &ctx, ObjectFile &file, const MachSection &hdr);
  void parse_relocations(Context &ctx);
  Subsection *find_subsection(Context &ctx, u32 addr);
  void scan_relocations(Context &ctx);

  ObjectFile &file;
  const MachSection &hdr;
  OutputSection &osec;
  std::string_view contents;
  std::vector<std::unique_ptr<Subsection>> subsections;
  std::vector<Relocation> rels;
};

std::ostream &operator<<(std::ostream &out, const InputSection &sec);

class Subsection {
public:
  u64 get_addr(Context &ctx) const {
    return PAGE_ZERO_SIZE + raddr;
  }

  std::string_view get_contents() {
    assert(isec.hdr.type != S_ZEROFILL);
    return isec.contents.substr(input_offset, input_size);
  }

  std::span<UnwindRecord> get_unwind_records() {
    return std::span(isec.file.unwind_records).subspan(unwind_offset, nunwind);
  }

  std::span<Relocation> get_rels() const {
    return std::span(isec.rels).subspan(rel_offset, nrels);
  }

  void apply_reloc(Context &ctx, u8 *buf);

  InputSection &isec;
  u32 input_offset = 0;
  u32 input_size = 0;
  u32 input_addr = 0;
  u32 rel_offset = 0;
  u32 nrels = 0;
  u32 unwind_offset = 0;
  u32 nunwind = 0;
  u32 raddr = -1;
  u16 p2align = 0;
};

//
// Symbol
//

enum {
  NEEDS_GOT  = 1 << 0,
  NEEDS_STUB = 1 << 1,
};

struct Symbol {
  Symbol() = default;
  Symbol(std::string_view name) : name(name) {}
  Symbol(const Symbol &other) : name(other.name) {}

  std::string_view name;
  InputFile *file = nullptr;
  Subsection *subsec = nullptr;
  u64 value = 0;

  i32 stub_idx = -1;
  i32 got_idx = -1;

  tbb::spin_mutex mu;

  std::atomic_uint8_t flags = 0;

  u8 is_extern : 1 = false;
  u8 is_lazy : 1 = false;
  u8 is_common : 1 = false;
  u8 referenced_dynamically : 1 = false;

  inline u64 get_addr(Context &ctx) const;
  inline u64 get_got_addr(Context &ctx) const;
};

std::ostream &operator<<(std::ostream &out, const Symbol &sym);

//
// output-chunks.cc
//

class OutputSegment {
public:
  static OutputSegment *get_instance(Context &ctx, std::string_view name);
  void set_offset(Context &ctx, i64 fileoff, u64 vmaddr);
  void copy_buf(Context &ctx);

  std::string_view name;
  SegmentCommand cmd = {};
  i32 seg_idx = -1;
  std::vector<Chunk *> chunks;

private:
  OutputSegment(std::string_view name);
};

class Chunk {
public:
  inline Chunk(Context &ctx, std::string_view segname, std::string_view sectname);
  virtual ~Chunk() = default;
  virtual void compute_size(Context &ctx) {};
  virtual void copy_buf(Context &ctx) {}

  MachSection hdr = {};
  u32 sect_idx = 0;
  bool is_hidden = false;
  bool is_regular = false;
};

std::ostream &operator<<(std::ostream &out, const Chunk &chunk);

class OutputMachHeader : public Chunk {
public:
  OutputMachHeader(Context &ctx) : Chunk(ctx, "__TEXT", "__mach_header") {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;
};

class OutputSection : public Chunk {
public:
  static OutputSection *
  get_instance(Context &ctx, std::string_view segname, std::string_view sectname);

  OutputSection(Context &ctx, std::string_view segname, std::string_view sectname)
    : Chunk(ctx, segname, sectname) {
    is_regular = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  void add_subsec(Subsection *subsec) {
    members.push_back(subsec);
    hdr.p2align = std::max<u32>(hdr.p2align, subsec->p2align);
    hdr.attr |= subsec->isec.hdr.attr;
    hdr.type = subsec->isec.hdr.type;
  }

  std::vector<Subsection *> members;
};

class RebaseEncoder {
public:
  RebaseEncoder();
  void add(i64 seg_idx, i64 offset);
  void flush();
  void finish();

  std::vector<u8> buf;

private:
  i64 last_seg = -1;
  i64 last_off = 0;
  i64 times = 0;
};

class OutputRebaseSection : public Chunk {
public:
  OutputRebaseSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__rebase") {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

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

class OutputBindSection : public Chunk {
public:
  OutputBindSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__binding") {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class OutputLazyBindSection : public Chunk {
public:
  OutputLazyBindSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__lazy_binding") {
    is_hidden = true;
  }

  void add(Context &ctx, Symbol &sym, i64 flags);

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

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

class OutputExportSection : public Chunk {
public:
  OutputExportSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__export") {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

private:
  ExportEncoder enc;
};

class OutputFunctionStartsSection : public Chunk {
public:
  OutputFunctionStartsSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__func_starts") {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class OutputSymtabSection : public Chunk {
public:
  OutputSymtabSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__symbol_table") {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(8);
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  struct Entry {
    Symbol *sym;
    i64 stroff;
  };

  std::vector<Entry> locals;
  std::vector<Entry> globals;
  std::vector<Entry> undefs;
};

class OutputStrtabSection : public Chunk {
public:
  OutputStrtabSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__string_table") {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(8);
  }

  i64 add_string(std::string_view str);
  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::string contents{"\0"};
};

class OutputIndirectSymtabSection : public Chunk {
public:
  OutputIndirectSymtabSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__ind_sym_tab") {
    is_hidden = true;
  }

  static constexpr i64 ENTRY_SIZE = 4;

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  struct Entry {
    Symbol *sym;
    i64 symtab_idx;
  };

  std::vector<Entry> stubs;
  std::vector<Entry> gots;
};

class CodeSignatureSection : public Chunk {
public:
  CodeSignatureSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__code_signature") {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(16);
  }

  void compute_size(Context &ctx) override;
  void write_signature(Context &ctx);

  static constexpr i64 BLOCK_SIZE = 4096;
};

class DataInCodeSection : public Chunk {
public:
  DataInCodeSection(Context &ctx)
    : Chunk(ctx, "__LINKEDIT", "__data_in_code") {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(alignof(DataInCodeEntry));
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::vector<DataInCodeEntry> contents;
};

class StubsSection : public Chunk {
public:
  StubsSection(Context &ctx) : Chunk(ctx, "__TEXT", "__stubs") {
    hdr.p2align = __builtin_ctz(2);
    hdr.type = S_SYMBOL_STUBS;
    hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
    hdr.reserved2 = ENTRY_SIZE;
  }

  void add(Context &ctx, Symbol *sym);
  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 6;

  std::vector<Symbol *> syms;
  std::vector<u32> bind_offsets;
};

class StubHelperSection : public Chunk {
public:
  StubHelperSection(Context &ctx) : Chunk(ctx, "__TEXT", "__stub_helper") {
    hdr.p2align = __builtin_ctz(4);
    hdr.attr = S_ATTR_SOME_INSTRUCTIONS | S_ATTR_PURE_INSTRUCTIONS;
  }

  void copy_buf(Context &ctx) override;

  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 ENTRY_SIZE = 10;
};

class UnwindEncoder {
public:
  std::vector<u8> encode(Context &ctx, std::span<UnwindRecord> records);

private:
  u32 encode_personality(Context &ctx, Symbol *sym);

  std::vector<std::span<UnwindRecord>>
  split_records(Context &ctx, std::span<UnwindRecord>);

  std::vector<Symbol *> personalities;
};

class UnwindInfoSection : public Chunk {
public:
  UnwindInfoSection(Context &ctx) : Chunk(ctx, "__TEXT", "__unwind_info") {
    hdr.p2align = __builtin_ctz(4);
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class GotSection : public Chunk {
public:
  GotSection(Context &ctx) : Chunk(ctx, "__DATA_CONST", "__got") {
    hdr.p2align = __builtin_ctz(8);
    hdr.type = S_NON_LAZY_SYMBOL_POINTERS;
  }

  void add(Context &ctx, Symbol *sym);
  void copy_buf(Context &ctx) override;

  std::vector<Symbol *> syms;

  static constexpr i64 ENTRY_SIZE = 8;
};

class LazySymbolPtrSection : public Chunk {
public:
  LazySymbolPtrSection(Context &ctx) : Chunk(ctx, "__DATA", "__la_symbol_ptr") {
    hdr.p2align = __builtin_ctz(8);
    hdr.type = S_LAZY_SYMBOL_POINTERS;
  }

  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 8;
};

//
// mapfile.cc
//

void print_map(Context &ctx);

//
// dumper.cc
//

void dump_file(std::string path);

//
// output-file.cc
//

class OutputFile {
public:
  static std::unique_ptr<OutputFile>
  open(Context &ctx, std::string path, i64 filesize, i64 perm);

  virtual void close(Context &ctx) = 0;
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

TextDylib parse_tbd(Context &ctx, MappedFile<Context> *mf);

//
// cmdline.cc
//

void parse_nonpositional_args(Context &ctx,
                              std::vector<std::string> &remaining);

//
// main.cc
//

struct Context {
  Context() {
    text_seg = OutputSegment::get_instance(*this, "__TEXT");
    data_const_seg = OutputSegment::get_instance(*this, "__DATA_CONST");
    data_seg = OutputSegment::get_instance(*this, "__DATA");
    linkedit_seg = OutputSegment::get_instance(*this, "__LINKEDIT");

    text = OutputSection::get_instance(*this, "__TEXT", "__text");
    data = OutputSection::get_instance(*this, "__DATA", "__data");
    bss = OutputSection::get_instance(*this, "__DATA", "__bss");
    cstring = OutputSection::get_instance(*this, "__TEXT", "__cstring");
    common = OutputSection::get_instance(*this, "__DATA", "__common");

    bss->hdr.type = S_ZEROFILL;
    cstring->hdr.type = S_CSTRING_LITERALS;
    common->hdr.type = S_ZEROFILL;
  }

  Context(const Context &) = delete;

  void checkpoint() {
    if (has_error) {
      cleanup();
      _exit(1);
    }
  }

  // Command-line arguments
  struct {
    bool adhoc_codesign = false;
    bool deduplicate = true;
    bool demangle = false;
    bool dynamic = true;
    bool fatal_warnings = false;
    bool trace = false;
    i64 platform = PLATFORM_MACOS;
    i64 platform_min_version = 0;
    i64 platform_sdk_version = 0;
    i64 headerpad = 256;
    std::string chroot;
    std::string map;
    std::string output;
    std::vector<std::string> syslibroot;
    std::vector<std::string> library_paths;
  } arg;

  std::vector<std::string_view> cmdline_args;

  bool has_error = false;

  tbb::concurrent_hash_map<std::string_view, Symbol> symbol_map;

  std::unique_ptr<OutputFile> output_file;
  u8 *buf;

  tbb::concurrent_vector<std::unique_ptr<ObjectFile>> obj_pool;
  tbb::concurrent_vector<std::unique_ptr<DylibFile>> dylib_pool;
  tbb::concurrent_vector<std::unique_ptr<u8[]>> string_pool;
  tbb::concurrent_vector<std::unique_ptr<MappedFile<Context>>> mf_pool;
  std::vector<std::unique_ptr<OutputSection>> osec_pool;

  tbb::concurrent_vector<std::unique_ptr<TimerRecord>> timer_records;

  std::vector<ObjectFile *> objs;
  std::vector<DylibFile *> dylibs;

  OutputSegment *text_seg = nullptr;
  OutputSegment *data_const_seg = nullptr;
  OutputSegment *data_seg = nullptr;
  OutputSegment *linkedit_seg = nullptr;

  std::vector<std::unique_ptr<OutputSegment>> segments;
  std::vector<Chunk *> chunks;

  OutputMachHeader mach_hdr{*this};
  StubsSection stubs{*this};
  StubHelperSection stub_helper{*this};
  UnwindInfoSection unwind_info{*this};
  GotSection got{*this};
  LazySymbolPtrSection lazy_symbol_ptr{*this};
  CodeSignatureSection code_sig{*this};
  DataInCodeSection data_in_code{*this};

  OutputRebaseSection rebase{*this};
  OutputBindSection bind{*this};
  OutputLazyBindSection lazy_bind{*this};
  OutputExportSection export_{*this};
  OutputFunctionStartsSection function_starts{*this};
  OutputSymtabSection symtab{*this};
  OutputIndirectSymtabSection indir_symtab{*this};
  OutputStrtabSection strtab{*this};

  OutputSection *text = nullptr;
  OutputSection *data = nullptr;
  OutputSection *bss = nullptr;
  OutputSection *cstring = nullptr;
  OutputSection *common = nullptr;
};

int main(int argc, char **argv);

//
// Inline functions
//

u64 Symbol::get_addr(Context &ctx) const {
  if (subsec)
    return subsec->get_addr(ctx) + value;
  if (stub_idx != -1)
    return ctx.stubs.hdr.addr + stub_idx * StubsSection::ENTRY_SIZE;
  return value;
}

u64 Symbol::get_got_addr(Context &ctx) const {
  assert(got_idx != -1);
  return ctx.got.hdr.addr + got_idx * GotSection::ENTRY_SIZE;
}

inline Symbol *intern(Context &ctx, std::string_view name) {
  typename decltype(ctx.symbol_map)::const_accessor acc;
  ctx.symbol_map.insert(acc, {name, Symbol(name)});
  return (Symbol *)(&acc->second);
}

inline std::ostream &operator<<(std::ostream &out, const Symbol &sym) {
  out << sym.name;
  return out;
}

Chunk::Chunk(Context &ctx, std::string_view segname, std::string_view sectname) {
  ctx.chunks.push_back(this);
  hdr.set_segname(segname);
  hdr.set_sectname(sectname);
}

u64 UnwindRecord::get_func_raddr(Context &ctx) const {
  return subsec->raddr + offset;
}

} // namespace mold::macho
