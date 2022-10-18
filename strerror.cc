// GNU's strerror_r is different from POSIX's strerror_r.
// In this file, we explicitly undefine _GNU_SOURCE to always
// use the POSIX version.

#define _POSIX_C_SOURCE 200809L
#undef _GNU_SOURCE

#include <cstring>
#include <errno.h>
#include <string_view>

namespace mold {

std::string_view errno_string() {
  static thread_local char buf[200];
#ifdef _WIN32
  strerror_s(buf, errno);
#else
  if (strerror_r(errno, buf, sizeof(buf)) != 0)
    return "strerror failed";
#endif
  return buf;
}

} // namespace mold
