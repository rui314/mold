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
// output-chunks.cc
//

class OutputSegment {
public:
  OutputSegment(std::string_view name, u32 prot, u32 flags);
  void copy_buf(Context &ctx);
  void update_hdr(Context &ctx);

  std::string_view name;
  SegmentCommand cmd = {};
  std::vector<OutputSection *> sections;
};

class OutputSection {
public:
  virtual ~OutputSection() = default;
  virtual void update_hdr(Context &ctx) {}
  virtual void copy_buf(Context &ctx) {}

  MachSection hdr = {};
  OutputSegment &parent;
  bool is_hidden = false;

protected:
  OutputSection(OutputSegment &parent);
};

class OutputMachHeader : public OutputSection {
public:
  OutputMachHeader(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = sizeof(MachHeader);;
  }

  void copy_buf(Context &ctx) override;
};

class OutputLoadCommand : public OutputSection {
public:
  OutputLoadCommand(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
  }

  void update_hdr(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  i64 ncmds = 0;

private:
  std::vector<u8> contents;
};

class OutputRebaseSection : public OutputSection {
public:
  OutputRebaseSection(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = contents.size();
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {0x11, 0x23, 0x00, 0x51, 0x00, 0x00, 0x00, 0x00};
};

class OutputBindSection : public OutputSection {
public:
  OutputBindSection(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = contents.size();
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x11, 0x40, 0x64, 0x79, 0x6c, 0x64, 0x5f, 0x73, 0x74, 0x75, 0x62, 0x5f,
    0x62, 0x69, 0x6e, 0x64, 0x65, 0x72, 0x00, 0x51, 0x72, 0x00, 0x90, 0x00,
  };
};

class OutputLazyBindSection : public OutputSection {
public:
  OutputLazyBindSection(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = contents.size();
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x73, 0x00, 0x11, 0x40, 0x5f, 0x70, 0x72, 0x69, 0x6e, 0x74, 0x66, 0x00,
    0x90, 0x00, 0x00, 0x00,
  };
};

class OutputExportSection : public OutputSection {
public:
  OutputExportSection(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = contents.size();
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x00, 0x01, 0x5f, 0x00, 0x05, 0x00, 0x03, 0x5f, 0x6d, 0x68, 0x5f, 0x65,
    0x78, 0x65, 0x63, 0x75, 0x74, 0x65, 0x5f, 0x68, 0x65, 0x61, 0x64, 0x65,
    0x72, 0x00, 0x28, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x00, 0x2c, 0x6d, 0x61,
    0x69, 0x6e, 0x00, 0x31, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0xd0, 0x7e,
    0x00, 0x03, 0x00, 0xf0, 0x7e, 0x00, 0x00, 0x00,
  };
};

class OutputFunctionStartsSection : public OutputSection {
public:
  OutputFunctionStartsSection(OutputSegment &parent) : OutputSection(parent) {
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
  OutputSymtabSection(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = contents.size();
    hdr.p2align = __builtin_ctz(8);
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x3c, 0x00, 0x00, 0x00, 0x0e, 0x08, 0x00, 0x00, 0x08, 0x80, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x0f, 0x01, 0x10, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x16, 0x00, 0x00, 0x00,
    0x0f, 0x01, 0x00, 0x00, 0x50, 0x3f, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x1d, 0x00, 0x00, 0x00, 0x0f, 0x01, 0x00, 0x00, 0x70, 0x3f, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x23, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2b, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  };
};

class OutputStrtabSection : public OutputSection {
public:
  OutputStrtabSection(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = contents.size();
    hdr.p2align = __builtin_ctz(8);
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x20, 0x00, 0x5f, 0x5f, 0x6d, 0x68, 0x5f, 0x65, 0x78, 0x65, 0x63, 0x75,
    0x74, 0x65, 0x5f, 0x68, 0x65, 0x61, 0x64, 0x65, 0x72, 0x00, 0x5f, 0x68,
    0x65, 0x6c, 0x6c, 0x6f, 0x00, 0x5f, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x5f,
    0x70, 0x72, 0x69, 0x6e, 0x74, 0x66, 0x00, 0x64, 0x79, 0x6c, 0x64, 0x5f,
    0x73, 0x74, 0x75, 0x62, 0x5f, 0x62, 0x69, 0x6e, 0x64, 0x65, 0x72, 0x00,
    0x5f, 0x5f, 0x64, 0x79, 0x6c, 0x64, 0x5f, 0x70, 0x72, 0x69, 0x76, 0x61,
    0x74, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  };
};

class OutputIndirectSymtabSection : public OutputSection {
public:
  OutputIndirectSymtabSection(OutputSegment &parent) : OutputSection(parent) {
    is_hidden = true;
    hdr.size = contents.size();
  }

  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x04, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
  };
};

class TextSection : public OutputSection {
public:
  TextSection(OutputSegment &parent);
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
  StubsSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {0xff, 0x25, 0x7c, 0x40, 0x00, 0x00};
};

class StubHelperSection : public OutputSection {
public:
  StubHelperSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {
    0x4c, 0x8d, 0x1d, 0x7d, 0x40, 0x00, 0x00, 0x41, 0x53, 0xff, 0x25, 0x6d,
    0x00, 0x00, 0x00, 0x90, 0x68, 0x00, 0x00, 0x00, 0x00, 0xe9, 0xe6, 0xff,
    0xff, 0xff,
  };
};

class CstringSection : public OutputSection {
public:
  CstringSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  static constexpr char contents[] = "Hello world\n";
};

class UnwindInfoSection : public OutputSection {
public:
  UnwindInfoSection(OutputSegment &parent);
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
  GotSection(OutputSegment &parent);

  static constexpr char contents[] = "Hello world\n";
};

class LaSymbolPtrSection : public OutputSection {
public:
  LaSymbolPtrSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents = {0x94, 0x3f, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00};
};

class DataSection : public OutputSection {
public:
  DataSection(OutputSegment &parent);
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
    std::string output;
  } arg;

  bool has_error = false;

  std::unique_ptr<OutputFile> output_file;
  u8 *buf;

  std::unique_ptr<OutputMachHeader> mach_hdr;
  std::unique_ptr<OutputLoadCommand> load_cmd;
  std::unique_ptr<OutputSegment> text_seg;
  std::unique_ptr<OutputSegment> data_const_seg;
  std::unique_ptr<OutputSegment> data_seg;
  std::unique_ptr<OutputSegment> linkedit_seg;

  std::unique_ptr<OutputRebaseSection> rebase;
  std::unique_ptr<OutputBindSection> bind;
  std::unique_ptr<OutputLazyBindSection> lazy_bind;
  std::unique_ptr<OutputExportSection> export_;
  std::unique_ptr<OutputFunctionStartsSection> function_starts;
  std::unique_ptr<OutputSymtabSection> symtab;
  std::unique_ptr<OutputIndirectSymtabSection> indir_symtab;
  std::unique_ptr<OutputStrtabSection> strtab;

  std::vector<OutputSegment *> segments;
};

int main(int argc, char **argv);

} // namespace mold::macho
