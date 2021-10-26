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
  bool is_pcrel = false;
  bool is_gotref = false;
  i64 addend = 0;
  Symbol *sym = nullptr;
  Subsection *subsec = nullptr;
};

struct UnwindRecord {
  UnwindRecord(u32 len, u32 enc) : code_len(len), encoding(enc) {}

  inline u64 get_func_addr(Context &ctx) const;

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

protected:
  InputFile() = default;
};

class ObjectFile : public InputFile {
public:
  ObjectFile() = default;

  static ObjectFile *create(Context &ctx, MappedFile<Context> *mf);
  void parse(Context &ctx);
  void parse_compact_unwind(Context &ctx, MachSection &hdr);
  void resolve_symbols(Context &ctx);

  Relocation read_reloc(Context &ctx, const MachSection &hdr, MachRel r);

  std::vector<std::unique_ptr<InputSection>> sections;
  std::span<MachSym> mach_syms;
  std::vector<UnwindRecord> unwind_records;
};

class DylibFile : public InputFile {
public:
  static DylibFile *create(Context &ctx, MappedFile<Context> *mf);
  void parse(Context &ctx);
  void resolve_symbols(Context &ctx);

  std::string_view install_name;
  i64 dylib_idx = 0;

private:
  DylibFile() {
    is_dylib = true;
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
  OutputSection *osec = nullptr;
  std::string_view contents;
  std::vector<Subsection> subsections;
  std::vector<Relocation> rels;
};

std::ostream &operator<<(std::ostream &out, const InputSection &sec);

class Subsection {
public:
  std::string_view get_contents() {
    return isec.contents.substr(input_offset, input_size);
  }

  std::span<UnwindRecord> get_unwind_records() {
    return std::span(isec.file.unwind_records).subspan(unwind_offset, nunwind);
  }

  inline u64 get_addr(Context &ctx) const;
  void apply_reloc(Context &ctx, u8 *buf);

  InputSection &isec;
  u32 input_offset = 0;
  u32 input_size = 0;
  u32 input_addr = 0;
  u32 rel_offset = 0;
  u32 nrels = 0;
  u32 unwind_offset = 0;
  u32 nunwind = 0;
  u32 output_offset = -1;
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
  OutputSegment(std::string_view name, u32 prot, u32 flags);
  void set_offset(Context &ctx, i64 fileoff, u64 vmaddr);
  void copy_buf(Context &ctx);

  std::string_view name;
  SegmentCommand cmd = {};
  i32 seg_idx = -1;
  std::vector<Chunk *> chunks;
};

class Chunk {
public:
  virtual ~Chunk() = default;
  virtual void compute_size(Context &ctx) {};
  virtual void copy_buf(Context &ctx) {}

  MachSection hdr = {};
  OutputSegment *parent = nullptr;
  u32 sect_idx = 0;
  bool is_hidden = false;
  bool is_regular = false;
};

class OutputMachHeader : public Chunk {
public:
  OutputMachHeader() {
    is_hidden = true;
    hdr.size = sizeof(MachHeader);
  }

  void copy_buf(Context &ctx) override;
};

class OutputLoadCommand : public Chunk {
public:
  OutputLoadCommand() {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  i64 ncmds = 0;

private:
  std::vector<u8> contents;
};

class OutputPadding : public Chunk {
public:
  OutputPadding() {
    is_hidden = true;
  }
};

class OutputSection : public Chunk {
public:
  OutputSection(std::string_view name);
  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

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
  OutputRebaseSection() {
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
  OutputBindSection() {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class OutputLazyBindSection : public Chunk {
public:
  OutputLazyBindSection() {
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
  OutputExportSection() {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

private:
  ExportEncoder enc;
};

class OutputFunctionStartsSection : public Chunk {
public:
  OutputFunctionStartsSection() {
    is_hidden = true;
  }

  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class OutputSymtabSection : public Chunk {
public:
  OutputSymtabSection() {
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
  OutputStrtabSection() {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(8);
  }

  i64 add_string(std::string_view str);
  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::string contents;
};

class OutputIndirectSymtabSection : public Chunk {
public:
  OutputIndirectSymtabSection() {
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

class StubsSection : public Chunk {
public:
  StubsSection();

  void add(Context &ctx, Symbol *sym);
  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 6;

  std::vector<Symbol *> syms;
  std::vector<u32> bind_offsets;
};

class StubHelperSection : public Chunk {
public:
  StubHelperSection();
  void copy_buf(Context &ctx) override;

  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 ENTRY_SIZE = 10;
};

class UnwindEncoder {
public:
  void add(UnwindRecord &rec);
  void finish(Context &ctx);

  std::vector<u8> buf;

private:
  u32 encode_personality(Context &ctx, Symbol *sym);
  std::vector<std::span<UnwindRecord>> split_records(Context &ctx);

  std::vector<UnwindRecord> records;
  std::vector<Symbol *> personalities;
};

class UnwindInfoSection : public Chunk {
public:
  UnwindInfoSection();
  void compute_size(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class GotSection : public Chunk {
public:
  GotSection();
  void add(Context &ctx, Symbol *sym);

  std::vector<Symbol *> syms;

  static constexpr i64 ENTRY_SIZE = 8;
};

class LazySymbolPtrSection : public Chunk {
public:
  LazySymbolPtrSection();
  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 8;
};

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
  Context() = default;
  Context(const Context &) = delete;

  void checkpoint() {
    if (has_error) {
      cleanup();
      _exit(1);
    }
  }

  // Command-line arguments
  struct {
    bool demangle = false;
    bool dynamic = true;
    bool fatal_warnings = false;
    i64 platform = PLATFORM_MACOS;
    i64 platform_min_version = 0;
    i64 platform_sdk_version = 0;
    i64 headerpad = 256;
    std::string chroot;
    std::string output;
    std::string syslibroot;
    std::vector<std::string> library_paths{"/usr/lib", "/usr/local/lib"};
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

  tbb::concurrent_vector<std::unique_ptr<TimerRecord>> timer_records;

  std::vector<ObjectFile *> objs;
  std::vector<DylibFile *> dylibs;

  OutputSegment text_seg{"__TEXT", VM_PROT_READ | VM_PROT_EXECUTE, 0};
  OutputSegment data_const_seg{"__DATA_CONST", VM_PROT_READ | VM_PROT_WRITE,
                               SG_READ_ONLY};
  OutputSegment data_seg{"__DATA", VM_PROT_READ | VM_PROT_WRITE, 0};
  OutputSegment linkedit_seg{"__LINKEDIT", VM_PROT_READ, 0};

  OutputMachHeader mach_hdr;
  StubsSection stubs;
  OutputPadding headerpad;
  StubHelperSection stub_helper;
  UnwindInfoSection unwind_info;
  GotSection got;
  LazySymbolPtrSection lazy_symbol_ptr;

  OutputLoadCommand load_cmd;
  OutputRebaseSection rebase;
  OutputBindSection bind;
  OutputLazyBindSection lazy_bind;
  OutputExportSection export_;
  OutputFunctionStartsSection function_starts;
  OutputSymtabSection symtab;
  OutputIndirectSymtabSection indir_symtab;
  OutputStrtabSection strtab;
  OutputSection text{"__text"};
  OutputSection data{"__data"};

  std::vector<OutputSegment *> segments;
};

int main(int argc, char **argv);

//
// Inline functions
//

u64 UnwindRecord::get_func_addr(Context &ctx) const {
  return subsec->get_addr(ctx) + offset;
}

u64 Subsection::get_addr(Context &ctx) const {
  return isec.osec->hdr.addr + output_offset;
}

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

} // namespace mold::macho
