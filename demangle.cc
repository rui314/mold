#include "mold.h"

#include <cstdlib>
#include <cxxabi.h>

namespace mold {

std::string_view demangle(std::string_view name) {
  if (name.starts_with("_Z")) {
    static thread_local char *buf1;
    static thread_local char *buf2;

    buf1 = (char *)realloc(buf1, name.size() + 1);
    memcpy(buf1, name.data(), name.size());
    buf1[name.size()] = '\0';

    int status;
    char *p = abi::__cxa_demangle(buf1, buf2, NULL, &status);
    if (status == 0) {
      buf2 = p;
      return p;
    }
  }

  return name;
}

} // namespace mold
