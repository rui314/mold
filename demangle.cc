#include "mold.h"

#include <cstdlib>
#include <cxxabi.h>
#include <rust-demangle/rust-demangle.h>

namespace mold {

std::string_view demangle(std::string_view name) {
  if (name.starts_with("_Z")) {
    static thread_local char *buf;
    if (buf)
      free(buf);

    int status;
    buf = abi::__cxa_demangle(std::string(name).c_str(), nullptr, nullptr,
                              &status);
    if (status == 0)
      return buf;

    buf = rust_demangle(std::string(name).c_str(), 0);
    if (buf)
      return buf;
  }

  return name;
}

} // namespace mold
