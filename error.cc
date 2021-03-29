#include "mold.h"

[[noreturn]]
void handle_unreachable(const char *file, i64 line) {
  std::lock_guard lock(SyncOut::mu);
  std::cerr << "internal error at " << file << ":" << line
            << "\n" << std::flush;
  cleanup();
  _exit(1);
}
