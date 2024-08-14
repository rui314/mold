#include "common.h"

#include <cstdlib>

#ifndef _WIN32
#include <cxxabi.h>
#endif

#include "../third-party/rust-demangle/rust-demangle.h"

namespace mold {

std::optional<std::string_view> demangle_cpp(std::string_view name) {
  static thread_local char *buf;
  static thread_local size_t buflen;

  // TODO(cwasser): Actually demangle Symbols on Windows using e.g.
  // `UnDecorateSymbolName` from Dbghelp, maybe even Itanium symbols?
#ifndef _WIN32
  if (name.starts_with("_Z")) {
    int status;
    char *p = abi::__cxa_demangle(std::string(name).c_str(), buf, &buflen, &status);
    if (status == 0) {
      buf = p;
      return p;
    }
  }
#endif

  return {};
}

std::optional<std::string_view> demangle_rust(std::string_view name) {
  static thread_local char *buf;
  free(buf);
  buf = rust_demangle(std::string(name).c_str(), 0);
  if (buf)
    return buf;
  return {};
}

} // namespace mold
