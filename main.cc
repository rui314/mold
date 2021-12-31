#include "elf/mold.h"
#include "macho/mold.h"

#include <cstring>
#include <signal.h>

namespace mold {

std::string_view errno_string() {
  static thread_local char buf[200];
#if _GNU_SOURCE
  // The GNU version of strerror_r() returns a char * and may completely ignore
  // buf
  return strerror_r(errno, buf, sizeof(buf));
#else
  // The POSIX.1-2001-compliant version of strerror_r() returns an int and
  // writes the string into buf
  return !strerror_r(errno, buf, sizeof(buf)) ? buf : "Unknown error";
#endif
}

#ifdef GIT_HASH
const std::string mold_version =
  "mold " MOLD_VERSION " (" GIT_HASH "; compatible with GNU ld and GNU gold)";
#else
const std::string mold_version =
  "mold " MOLD_VERSION " (compatible with GNU ld and GNU gold)";
#endif

void cleanup() {
  if (output_tmpfile)
    unlink(output_tmpfile);
  if (socket_tmpfile)
    unlink(socket_tmpfile);
}

static void signal_handler(int) {
  cleanup();
  _exit(1);
}

void install_signal_handler() {
  signal(SIGINT, signal_handler);
  signal(SIGTERM, signal_handler);
}

} // namespace mold

int main(int argc, char **argv) {
  std::string cmd = mold::filepath(argv[0]).filename();

  if (cmd == "ld64" || cmd == "ld64.mold")
    return mold::macho::main(argc, argv);

  return mold::elf::main(argc, argv);
}
