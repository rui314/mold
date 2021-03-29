#include "mold.h"

#include <cxxabi.h>
#include <stdlib.h>

static thread_local char *demangle_buf;
static thread_local size_t demangle_buf_len;

static bool is_mangled_name(std::string_view name) {
  return name.starts_with("_Z");
}

std::string_view Symbol::get_demangled_name() const {
  if (is_mangled_name(name)) {
    assert(name[name.size()] == '\0');
    size_t len = sizeof(demangle_buf);
    int status;
    demangle_buf =
      abi::__cxa_demangle(name.data(), demangle_buf, &demangle_buf_len, &status);
    if (status == 0)
      return demangle_buf;
  }

  return name;
}

std::ostream &operator<<(std::ostream &out, const Symbol &sym) {
  if (ctx.arg.demangle)
    out << sym.get_demangled_name();
  else
    out << sym.name;
  return out;
}
