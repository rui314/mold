#include "mold.h"

#include <cstdlib>
#include <cxxabi.h>
#include <rust-demangle/rust-demangle.h>

namespace mold {

std::string_view demangle(std::string_view name) {
  static thread_local char *p;
  if (p)
    free(p);

  // Try to demangle as a Rust symbol. Since legacy-style Rust symbols
  // are also valid as a C++ mangled name, we need to call this before
  // cpp_demangle.
  p = rust_demangle(std::string(name).c_str(), 0);
  if (p)
    return p;

  // Try to demangle as a C++ symbol.
  if (std::optional<std::string_view> s = cpp_demangle(name))
    return *s;
  return name;
}

std::optional<std::string_view> cpp_demangle(std::string_view name) {
  static thread_local char *buf;
  static thread_local size_t buflen;

  if (name.starts_with("_Z")) {
    int status;
    char *p = abi::__cxa_demangle(std::string(name).c_str(), buf, &buflen, &status);
    if (status == 0) {
      buf = p;
      return p;
    }
  }
  return {};
}

} // namespace mold
