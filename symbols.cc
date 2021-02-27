#include "mold.h"

#include <cxxabi.h>
#include <stdlib.h>

std::ostream &operator<<(std::ostream &out, const Symbol &sym) {
  if (config.demangle) {
    int status;
    char *name = abi::__cxa_demangle(std::string(sym.name).c_str(),
                                     nullptr, 0, &status);
    if (status == 0) {
      out << name;
      free(name);
      return out;
    }
  }

  out << sym.name;
  return out;
}
