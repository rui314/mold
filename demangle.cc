#include "mold.h"

#include <cstdlib>
#include <cxxabi.h>

namespace mold {

std::string_view demangle(std::string_view name) {
  if (name.starts_with("_Z")) {
    static thread_local char *buf;
    static thread_local size_t buflen;

    int status;
    char *p = abi::__cxa_demangle(std::string(name).c_str(), buf, &buflen, &status);
    if (status == 0) {
      buf = p;
      return p;
    }
  }

  return name;
}

} // namespace mold
