#include "elf/mold.h"
#include "macho/mold.h"

#include <cstring>
#include <signal.h>

namespace mold {

std::string_view errno_string() {
  static thread_local char buf[200];
  strerror_r(errno, buf, sizeof(buf));
  return buf;
}

std::string get_version_string() {
  if (GIT_HASH[0] == '\0')
    return "mold " MOLD_VERSION " (compatible with GNU ld and GNU gold)";
  return "mold " MOLD_VERSION " (" GIT_HASH
         "; compatible with GNU ld and GNU gold)";
}

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
  std::string_view cmd = mold::path_filename(argv[0]);

  if (cmd == "ld" || cmd == "mold" || cmd == "ld.mold")
    return mold::elf::main(argc, argv);

  if (cmd == "ld64" || cmd == "ld64.mold")
    return mold::macho::main(argc, argv);

  std::cerr << "mold: unknown command: " << argv[0] << "\n";
  exit(1);
}
