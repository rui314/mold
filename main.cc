#include "elf/mold.h"
#include "macho/mold.h"

#include <cstring>

namespace mold {

std::string errno_string() {
  char buf[500];
  strerror_r(errno, buf, sizeof(buf));
  return buf;
}

std::string get_version_string() {
  if (strlen(GIT_HASH) == 0)
    return "mold " MOLD_VERSION " (compatible with GNU ld and GNU gold)";
  return "mold " MOLD_VERSION " (" GIT_HASH
         "; compatible with GNU ld and GNU gold)";
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
