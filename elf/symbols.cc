#include "mold.h"

#include <cxxabi.h>
#include <stdlib.h>

namespace mold::elf {

static bool is_mangled_name(std::string_view name) {
  return name.starts_with("_Z");
}

template <typename E>
std::string_view Symbol<E>::get_demangled_name() const {
  if (is_mangled_name(name())) {
    static thread_local char *buf1;
    static thread_local char *buf2;

    buf1 = (char *)realloc(buf1, name().size() + 1);
    memcpy(buf1, name().data(), name().size());
    buf1[name().size()] = '\0';

    int status;
    char *p = abi::__cxa_demangle(buf1, buf2, NULL, &status);
    if (status == 0) {
      buf2 = p;
      return p;
    }
  }

  return name();
}

template <typename E>
std::ostream &operator<<(std::ostream &out, const Symbol<E> &sym) {
  if (opt_demangle)
    out << sym.get_demangled_name();
  else
    out << sym.name();
  return out;
}

#define INSTANTIATE(E)                                                  \
  template class Symbol<E>;                                             \
  template std::ostream &operator<<(std::ostream &, const Symbol<E> &)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(AARCH64);

} // namespace mold::elf
