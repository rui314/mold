#include "mold.h"

#include <cxxabi.h>
#include <stdlib.h>

static thread_local char *demangle_buf;
static thread_local size_t demangle_buf_len;

static bool is_mangled_name(std::string_view name) {
  return name.starts_with("_Z");
}

template <typename E>
std::string_view Symbol<E>::get_demangled_name() const {
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

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym) {
  if (opt_demangle)
    out << sym.get_demangled_name();
  else
    out << sym.name;
  return out;
}

template class Symbol<ELF64LE>;

template
std::ostream &operator<<(std::ostream &out, const Symbol<ELF64LE> &sym);
