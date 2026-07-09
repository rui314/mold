#include "config.h"
#include "mold.h"
#include "mold-git-hash.h"

#if MOLD_USE_MIMALLOC
# include <mimalloc.h>
#endif

#if MOLD_USE_SYSTEM_MIMALLOC
// Including mimalloc-new-delete.h overrides the new/delete operators.
// We need it only when using mimalloc as a dynamic library.
// This header should be included in only one source file, so we do
// it in this file.
# include <mimalloc-new-delete.h>
#endif

#ifdef __linux__
# include <sys/prctl.h>
#endif

std::string mold::mold_version =
#ifdef MOLD_GIT_HASH
  "mold " MOLD_VERSION " (" MOLD_GIT_HASH "; compatible with GNU ld)";
#else
  "mold " MOLD_VERSION " (compatible with GNU ld)";
#endif

int main(int argc, char **argv) {
#if MOLD_USE_MIMALLOC
  // Silence mimalloc warnings that users can ignore
  mi_option_disable(mi_option_verbose);
  mi_option_disable(mi_option_show_errors);
#endif

#if defined(__linux__) && defined(PR_SET_THP_DISABLE)
  // A parent process may have disabled transparent huge pages, and the
  // flag is inherited. Huge pages make mold considerably faster on
  // large links, so re-enable them for this process.
  prctl(PR_SET_THP_DISABLE, 0, 0, 0, 0);
#endif

  return mold::mold_main<mold::MOLD_FIRST_TARGET>(argc, argv);
}
