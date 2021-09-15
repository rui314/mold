#pragma once

#include "macho.h"
#include "../mold.h"

#include <memory>

namespace mold::macho {

struct Context;

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
};

int main(int argc, char **argv);

} // namespace mold::macho
