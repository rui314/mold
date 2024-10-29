#include "mold.h"

namespace mold {

std::string get_mold_version() {
  if (mold_git_hash.empty())
    return "mold "s + MOLD_VERSION + " (compatible with GNU ld)";
  return "mold "s + MOLD_VERSION + " (" + mold_git_hash +
         "; compatible with GNU ld)";
}

} // namespace mold
