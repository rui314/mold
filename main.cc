#include "catld.h"
#include "llvm/Option/ArgList.h"

#include <iostream>

Config config;

// Parses command line options.
class OptTable : public llvm::opt::OptTable {
public:
  OptTable();
  llvm::opt::InputArgList parse(ArrayRef<const char *> argv);
};

// Create enum with OPT_xxx values for each option in Options.td
enum {
  OPT_INVALID = 0,
#define OPTION(_1, _2, ID, _4, _5, _6, _7, _8, _9, _10, _11, _12) OPT_##ID,
#include "options.inc"
#undef OPTION
};

int main(int argc, char **argv) {
  return 0;
}
