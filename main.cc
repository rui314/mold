#include "elf/mold.h"
#include "macho/mold.h"

int main(int argc, char **argv) {
  std::string_view cmd = mold::path_filename(argv[0]);

  if (cmd == "ld" || cmd == "mold" || cmd == "ld.mold")
    return mold::elf::main(argc, argv);

  if (cmd == "ld64" || cmd == "ld64.mold")
    return mold::macho::main(argc, argv);

  std::cerr << "mold: unknown command: " << argv[0] << "\n";
  exit(1);
}
