#include "config.h"
#include "mold.h"
#include "mold-git-hash.h"

std::string mold::mold_version =
#ifdef MOLD_GIT_HASH
  "mold " MOLD_VERSION " (" MOLD_GIT_HASH "; compatible with GNU ld)";
#else
  "mold " MOLD_VERSION " (compatible with GNU ld)";
#endif

int main(int argc, char **argv) {
  mold::set_mimalloc_options();
  return mold::mold_main<mold::X86_64>(argc, argv);
}
