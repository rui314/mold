#include "elf/mold.h"
#include "macho/mold.h"

#include <cstring>
#include <signal.h>

#ifdef USE_SYSTEM_MIMALLOC
#include <mimalloc-new-delete.h>
#endif

namespace mold {

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

static void sigint_handler(int) {
  cleanup();
  _exit(1);
}

static void sigbus_handler(int) {
  puts("mold: BUS error: This might have been caused as a result"
       " of a disk full error. Check your filesystem usage.");
  cleanup();
  _exit(1);
}

void install_signal_handler() {
  signal(SIGINT, sigint_handler);
  signal(SIGTERM, sigint_handler);
  signal(SIGBUS, sigbus_handler);
}

} // namespace mold

int main(int argc, char **argv) {
  std::string cmd = mold::filepath(argv[0]).filename();

  if (cmd == "ld64" || cmd == "ld64.mold")
    return mold::macho::main(argc, argv);

  return mold::elf::main(argc, argv);
}
