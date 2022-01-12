#include "elf/mold.h"
#include "macho/mold.h"

#include <cstring>
#include <signal.h>

namespace mold {

std::string_view errno_string() {
  static thread_local char buf[200];

  // There are two incompatible strerror_r implementations as follows.
  //
  //   GNU:    char *strerror_r(int, char *, size_t)
  //   POSIX:  int   strerror_r(int, char *, size_t)
  //
  // GNU version may write an error message to a buffer other than the
  // given one and returns a pointer to the error message. POSIX version
  // always write an error message to a given buffer.

  if (std::is_same<decltype(strerror_r(errno, buf, sizeof(buf))), char *>::value)
    return reinterpret_cast<char *>(strerror_r(errno, buf, sizeof(buf)));
  strerror_r(errno, buf, sizeof(buf));
  return buf;
}

#ifdef GIT_HASH
const std::string mold_version =
  "mold " MOLD_VERSION " (" GIT_HASH "; compatible with GNU ld)";
#else
const std::string mold_version =
  "mold " MOLD_VERSION " (compatible with GNU ld)";
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
