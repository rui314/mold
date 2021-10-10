#pragma once

#include "macho.h"
#include "../mold.h"

#include <memory>
#include <tbb/concurrent_hash_map.h>

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
  i64 addend = 0;
  Symbol *sym = nullptr;
  Subsection *subsec = nullptr;
};

struct UnwindEntry {
  Relocation code_start;
  u32 code_len;
  u32 compact_unwind_info;
  Relocation personality;
  Relocation lsda;
};

class ObjectFile {
public:
  static ObjectFile *create(Context &ctx, MappedFile<Context> *mf);
  void parse(Context &ctx);
  void parse_compact_unwind(Context &ctx, MachSection &hdr);
  void resolve_symbols(Context &ctx);

  Relocation read_reloc(Context &ctx, const MachSection &hdr, MachRel r);

  MappedFile<Context> *mf;
  std::vector<std::unique_ptr<InputSection>> sections;
  std::vector<Symbol *> syms;
  std::span<MachSym> mach_syms;

  std::vector<UnwindEntry> unwind_entries;

private:
  ObjectFile(Context &ctx, MappedFile<Context> *mf);
};

std::ostream &operator<<(std::ostream &out, const ObjectFile &file);

//
// input-sections.cc
//

class InputSection {
public:
  InputSection(Context &ctx, ObjectFile &file, const MachSection &hdr);
  void parse_relocations(Context &ctx);
  Subsection *find_subsection(Context &ctx, u32 addr);

  ObjectFile &file;
  const MachSection &hdr;
  OutputSection *osec = nullptr;
  std::string_view contents;
  std::vector<Subsection> subsections;
  std::vector<Relocation> rels;
  std::vector<UnwindEntry> unwind_entries;
};

std::ostream &operator<<(std::ostream &out, const InputSection &sec);

class Subsection {
public:
  std::string_view get_contents() {
    return isec.contents.substr(input_offset, input_size);
  }

  void apply_reloc(Context &ctx, u8 *buf);

  const InputSection &isec;
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

struct Symbol {
  Symbol() = default;
  Symbol(std::string_view name) : name(name) {}
  Symbol(const Symbol &other) : name(other.name) {}

  std::string_view name;

  ObjectFile *file = nullptr;
  Subsection *subsec = nullptr;
  u64 value = 0;
  i32 stub_idx = -1;

  inline u64 get_addr(Context &ctx) const;
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
  std::vector<Chunk *> chunks;
};

class Chunk {
public:
  virtual ~Chunk() = default;
  virtual void compute_size(Context &ctx) {};
  virtual void copy_buf(Context &ctx) {}

  MachSection hdr = {};
  OutputSegment *parent = nullptr;
  bool is_hidden = false;
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
  OutputRebaseSection();
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class BindEncoder {
public:
  BindEncoder(bool is_lazy);
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
  OutputBindSection();
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class OutputLazyBindSection : public Chunk {
public:
  OutputLazyBindSection();
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
  OutputExportSection();
  void copy_buf(Context &ctx) override;

private:
  ExportEncoder enc;
};

class OutputFunctionStartsSection : public Chunk {
public:
  OutputFunctionStartsSection() {
    is_hidden = true;
    hdr.size = contents.size();
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0xd0, 0x7e, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00
  };
};

class OutputSymtabSection : public Chunk {
public:
  OutputSymtabSection() {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(8);
  }

  void add(Context &ctx, std::string_view name, i64 type,
           bool is_external, i64 sect_idx, i64 lib_idx, u64 value);

  void copy_buf(Context &ctx) override;

  std::vector<MachSym> symbols;
};

class OutputStrtabSection : public Chunk {
public:
  OutputStrtabSection() {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(8);
  }

  i64 add_string(std::string_view str);
  void copy_buf(Context &ctx) override;

  std::string contents;
};

class OutputIndirectSymtabSection : public Chunk {
public:
  OutputIndirectSymtabSection() {
    is_hidden = true;
    hdr.size = contents.size();
  }

  static constexpr i64 ENTRY_SIZE = 4;

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x04, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
  };
};

class TextSection : public Chunk {
public:
  TextSection();
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x55, 0x48, 0x89, 0xe5, 0x48, 0x8d, 0x3d, 0x43, 0x00, 0x00, 0x00,
    0xb0, 0x00, 0xe8, 0x1c, 0x00, 0x00, 0x00, 0x5d, 0xc3, 0x66, 0x2e,
    0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00, 0x66, 0x90, 0x55,
    0x48, 0x89, 0xe5, 0xe8, 0xd7, 0xff, 0xff, 0xff, 0x31, 0xc0, 0x5d,
    0xc3,
  };
};

class StubsSection : public Chunk {
public:
  StubsSection();

  void add(Context &ctx, Symbol &sym, i64 dylib_idx, i64 flags,
           i64 seg_idx, i64 offset);
  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 6;

  struct Entry {
    Symbol &sym;
    i64 dylib_idx;
    i64 flags;
    i64 seg_idx;
    i64 offset;
  };

  std::vector<Entry> entries;
};

class StubHelperSection : public Chunk {
public:
  StubHelperSection();
  void copy_buf(Context &ctx) override;

  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 ENTRY_SIZE = 10;
};

class CstringSection : public Chunk {
public:
  CstringSection();
  void copy_buf(Context &ctx) override;

  static constexpr char contents[] = "Hello world\n";
};

class UnwindInfoSection : public Chunk {
public:
  UnwindInfoSection();
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x01, 0x00, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x50, 0x3f, 0x00, 0x00, 0x34, 0x00, 0x00, 0x00,
    0x34, 0x00, 0x00, 0x00, 0x7e, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x34, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x01, 0x00,
    0x10, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  };
};

class GotSection : public Chunk {
public:
  GotSection();
};

class LazySymbolPtrSection : public Chunk {
public:
  LazySymbolPtrSection();
  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 8;
};

class DataSection : public Chunk {
public:
  DataSection();
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

//
// dumper.cc
//

void dump_file(std::string path);

//
// cmdline.cc
//

void parse_nonpositional_args(Context &ctx,
                              std::vector<std::string_view> &remaining);

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
    bool fatal_warnings = false;
    i64 filler = -1;
    std::string chroot;
    std::string output;
  } arg;

  std::vector<std::string_view> cmdline_args;

  bool has_error = false;

  tbb::concurrent_hash_map<std::string_view, Symbol> symbol_map;

  std::unique_ptr<OutputFile<Context>> output_file;
  u8 *buf;

  tbb::concurrent_vector<std::unique_ptr<ObjectFile>> obj_pool;
  tbb::concurrent_vector<std::unique_ptr<u8[]>> string_pool;
  tbb::concurrent_vector<std::unique_ptr<MappedFile<Context>>> mf_pool;

  tbb::concurrent_vector<std::unique_ptr<TimerRecord>> timer_records;

  std::vector<ObjectFile *> objs;

  OutputSegment text_seg{"__TEXT", VM_PROT_READ | VM_PROT_EXECUTE, 0};
  OutputSegment data_const_seg{"__DATA_CONST", VM_PROT_READ | VM_PROT_WRITE,
                               SG_READ_ONLY};
  OutputSegment data_seg{"__DATA", VM_PROT_READ | VM_PROT_WRITE, 0};
  OutputSegment linkedit_seg{"__LINKEDIT", VM_PROT_READ, 0};

  OutputMachHeader mach_hdr;
  StubsSection stubs;
  StubHelperSection stub_helper;
  CstringSection cstring;
  UnwindInfoSection unwind_info;
  GotSection got;
  LazySymbolPtrSection lazy_symbol_ptr;
  DataSection data;

  OutputLoadCommand load_cmd;
  OutputRebaseSection rebase;
  OutputBindSection bind;
  OutputLazyBindSection lazy_bind;
  OutputExportSection export_;
  OutputFunctionStartsSection function_starts;
  OutputSymtabSection symtab;
  OutputIndirectSymtabSection indir_symtab;
  OutputStrtabSection strtab;

  std::vector<OutputSegment *> segments;
};

int main(int argc, char **argv);

//
// Inline functions
//

u64 Symbol::get_addr(Context &ctx) const {
  if (subsec)
    return subsec->isec.osec->hdr.addr + subsec->output_offset + value;
  if (stub_idx != -1)
    return ctx.stubs.hdr.addr + stub_idx * StubsSection::ENTRY_SIZE;
  return value;
}

inline Symbol *intern(Context &ctx, std::string_view name) {
  typename decltype(ctx.symbol_map)::const_accessor acc;
  ctx.symbol_map.insert(acc, {name, Symbol(name)});
  return const_cast<Symbol*>(&acc->second);
}

inline std::ostream &operator<<(std::ostream &out, const Symbol &sym) {
  out << sym.name;
  return out;
}

} // namespace mold::macho
