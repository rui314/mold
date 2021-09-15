#pragma once

#include "macho.h"
#include "../mold.h"

#include <memory>

namespace mold::macho {

struct Context;

//
// output-chunks.cc
//

class Chunk {
public:
  // There are three types of OutputChunks:
  //  - HEADER: the ELF, section or segment headers
  //  - REGULAR: output sections containing input sections
  //  - SYNTHETIC: linker-synthesized sections such as got or plt
  enum Kind : u8 { HEADER, REGULAR, SYNTHETIC };

  virtual ~Chunk() = default;
  virtual void copy_buf(Context &ctx) {}
  virtual void update_hdr(Context &ctx) {}

  std::string_view name;
  Kind kind;
  i64 size = 0;
  i64 fileoff = 0;
  std::vector<u8> load_cmd;

protected:
  Chunk(Kind kind) : kind(kind) {}
};

class MachHeaderChunk : public Chunk {
public:
  MachHeaderChunk() : Chunk(HEADER) {
    size = sizeof(MachHeader);
  }

  void copy_buf(Context &ctx) override;
};

class LoadCommandChunk : public Chunk {
public:
  LoadCommandChunk() : Chunk(HEADER) {}

  void update_hdr(Context &ctx) override;
  void copy_buf(Context &ctx) override;

  i64 ncmds = 0;

private:
  std::vector<u8> contents;
};

class PageZeroChunk : public Chunk {
public:
  PageZeroChunk();
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

  std::unique_ptr<MachHeaderChunk> mach_hdr;
  std::unique_ptr<LoadCommandChunk> load_cmd;
  std::unique_ptr<PageZeroChunk> zero_page;
  std::vector<Chunk *> chunks;
};

int main(int argc, char **argv);

} // namespace mold::macho
