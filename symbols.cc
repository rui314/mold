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
  if (is_mangled_name(get_name())) {
    assert(nameptr[namelen] == '\0');
    size_t len = sizeof(demangle_buf);
    int status;
    demangle_buf =
      abi::__cxa_demangle(nameptr, demangle_buf, &demangle_buf_len, &status);
    if (status == 0)
      return demangle_buf;
  }

  return get_name();
}

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym) {
  if (opt_demangle)
    out << sym.get_demangled_name();
  else
    out << sym.get_name();
  return out;
}

template class Symbol<X86_64>;

template
std::ostream &operator<<(std::ostream &out, const Symbol<X86_64> &sym);

template class Symbol<I386>;

template
std::ostream &operator<<(std::ostream &out, const Symbol<I386> &sym);
