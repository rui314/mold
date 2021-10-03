#pragma once

#include "macho.h"
#include "../mold.h"

#include <memory>

namespace mold::macho {

static constexpr i64 PAGE_SIZE = 0x4000;
static constexpr i64 PAGE_ZERO_SIZE = 0x100000000;

class OutputSection;
struct Context;

//
// object-file.cc
//

class ObjectFile {
public:
  static ObjectFile *create(Context &ctx, MappedFile<Context> *mf);

private:
  ObjectFile(Context &ctx, MappedFile<Context> *mf);

  MappedFile<Context> *mf;
};

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
  std::vector<OutputSection *> sections;
};

class OutputSection {
public:
  virtual ~OutputSection() = default;
  virtual void copy_buf(Context &ctx) {}

  MachSection hdr = {};
  OutputSegment *parent = nullptr;
  bool is_hidden = false;
};

class OutputMachHeader : public OutputSection {
public:
  OutputMachHeader() {
    is_hidden = true;
    hdr.size = sizeof(MachHeader);
  }

  void copy_buf(Context &ctx) override;
};

class OutputLoadCommand : public OutputSection {
public:
  OutputLoadCommand() {
    is_hidden = true;
  }

  void compute_size(Context &ctx);
  void copy_buf(Context &ctx) override;

  i64 ncmds = 0;

private:
  std::vector<u8> contents;
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

class OutputRebaseSection : public OutputSection {
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

class OutputBindSection : public OutputSection {
public:
  OutputBindSection();
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class OutputLazyBindSection : public OutputSection {
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

class OutputExportSection : public OutputSection {
public:
  OutputExportSection();
  void copy_buf(Context &ctx) override;

private:
  ExportEncoder enc;
};

class OutputFunctionStartsSection : public OutputSection {
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

class OutputSymtabSection : public OutputSection {
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

class OutputStrtabSection : public OutputSection {
public:
  OutputStrtabSection() {
    is_hidden = true;
    hdr.p2align = __builtin_ctz(8);
  }

  i64 add_string(std::string_view str);
  void copy_buf(Context &ctx) override;

  std::string contents;
};

class OutputIndirectSymtabSection : public OutputSection {
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

class TextSection : public OutputSection {
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

class StubsSection : public OutputSection {
public:
  StubsSection();

  void add(Context &ctx, i64 dylib_idx, std::string_view name,
           i64 flags, i64 seg_idx, i64 offset);
  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 6;

  struct Entry {
    i64 dylib_idx;
    std::string_view name;
    i64 flags;
    i64 seg_idx;
    i64 offset;
  };

  std::vector<Entry> entries;
};

class StubHelperSection : public OutputSection {
public:
  StubHelperSection();
  void copy_buf(Context &ctx) override;

  static constexpr i64 HEADER_SIZE = 16;
  static constexpr i64 ENTRY_SIZE = 10;
};

class CstringSection : public OutputSection {
public:
  CstringSection();
  void copy_buf(Context &ctx) override;

  static constexpr char contents[] = "Hello world\n";
};

class UnwindInfoSection : public OutputSection {
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

class GotSection : public OutputSection {
public:
  GotSection();
};

class LazySymbolPtrSection : public OutputSection {
public:
  LazySymbolPtrSection();
  void copy_buf(Context &ctx) override;

  static constexpr i64 ENTRY_SIZE = 8;
};

class DataSection : public OutputSection {
public:
  DataSection();
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

//
// output-file.cc
//

class OutputFile {
public:
  OutputFile(Context &ctx, std::string path, i64 filesize, i64 perm);
  void close(Context &ctx);

  u8 *buf = nullptr;
  std::string path;
  i64 filesize = 0;
  i64 perm = 0;
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
    std::string chroot;
    std::string output;
  } arg;

  std::vector<std::string_view> cmdline_args;

  bool has_error = false;

  std::unique_ptr<OutputFile> output_file;
  u8 *buf;

  tbb::concurrent_vector<std::unique_ptr<ObjectFile>> obj_pool;
  tbb::concurrent_vector<std::unique_ptr<u8[]>> string_pool;
  tbb::concurrent_vector<std::unique_ptr<MappedFile<Context>>> mf_pool;

  std::vector<ObjectFile *> objs;

  OutputSegment text_seg{"__TEXT", VM_PROT_READ | VM_PROT_EXECUTE, 0};
  OutputSegment data_const_seg{"__DATA_CONST", VM_PROT_READ | VM_PROT_WRITE,
                               SG_READ_ONLY};
  OutputSegment data_seg{"__DATA", VM_PROT_READ | VM_PROT_WRITE, 0};
  OutputSegment linkedit_seg{"__LINKEDIT", VM_PROT_READ, 0};

  OutputMachHeader mach_hdr;
  TextSection text;
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

} // namespace mold::macho
