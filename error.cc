#include "mold.h"

[[noreturn]]
void handle_unreachable(const char *file, i64 line) {
  std::cerr << "internal error at " << file << ":" << line
            << "\n" << std::flush;
  _exit(1);
}
