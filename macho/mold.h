#pragma once

#include "macho.h"
#include "../mold.h"

#include <memory>

namespace mold::macho {

static constexpr i64 PAGE_SIZE = 4096;
static constexpr i64 PAGE_ZERO_SIZE = 0x100000000;

class OutputSection;
struct Context;

//
// output-chunks.cc
//

class Chunk {
public:
  virtual ~Chunk() = default;
  virtual void copy_buf(Context &ctx) {}
  virtual void update_hdr(Context &ctx) {}

  std::string_view name;
  bool is_segment = false;

  i64 fileoff = 0;
  i64 filesize = 0;
  i64 p2align = 0;

protected:
  Chunk() = default;
};

class OutputMachHeader : public Chunk {
public:
  OutputMachHeader() {
    filesize = sizeof(MachHeader);
  }

  void copy_buf(Context &ctx) override;
};

class OutputLoadCommand : public Chunk {
public:
  void update_hdr(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  i64 ncmds = 0;

private:
  std::vector<u8> contents;
};

class OutputSegment : public Chunk {
public:
  OutputSegment(std::string_view name, u32 prot, u32 flags);

  void update_hdr(Context &ctx) override;
  void copy_buf(Context &ctx) override;

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

protected:
  OutputSection(OutputSegment &parent, std::string_view name);
};

class TextSection : public OutputSection {
public:
  TextSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class StubsSection : public OutputSection {
public:
  StubsSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class StubHelperSection : public OutputSection {
public:
  StubHelperSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  std::vector<u8> contents;
};

class CstringSection : public OutputSection {
public:
  CstringSection(OutputSegment &parent);
  void copy_buf(Context &ctx) override;

  static constexpr char contents[] = "Hello world\n";
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

  std::vector<u8> contents;
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
  std::unique_ptr<OutputSegment> text_segment;
  std::unique_ptr<OutputSegment> data_const_segment;
  std::unique_ptr<OutputSegment> data_segment;

  std::vector<Chunk *> chunks;
};

int main(int argc, char **argv);

} // namespace mold::macho
