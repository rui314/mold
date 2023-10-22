// Many build systems attempt to invoke as many linker processes as there
// are cores, based on the assumption that the linker is single-threaded.
// However, since mold is multi-threaded, such build systems' behavior is
// not beneficial and just increases the overall peak memory usage.
// On machines with limited memory, this could lead to an out-of-memory
// error.
//
// This file implements a feature that limits the number of concurrent
// mold processes to just 1 for each user. It is intended to be used as
// `MOLD_JOBS=1 ninja` or `MOLD_JOBS=1 make -j$(nproc)`.

#include "mold.h"

#ifndef _WIN32
# include <fcntl.h>
# include <pwd.h>
# include <sys/stat.h>
# include <sys/types.h>
# include <unistd.h>
#endif

namespace mold::elf {

template <typename E>
void acquire_global_lock(Context<E> &ctx) {
#ifndef _WIN32
  char *jobs = getenv("MOLD_JOBS");
  if (!jobs || jobs != "1"s)
    return;

  std::string path;
  if (char *dir = getenv("XDG_RUNTIME_DIR"))
    path = dir + "/mold-lock"s;
  else
    path = "/tmp/mold-lock-"s + getpwuid(getuid())->pw_name;

  int fd = open(path.c_str(), O_WRONLY | O_CREAT | O_CLOEXEC, 0600);
  if (fd == -1)
    return;

  if (lockf(fd, F_LOCK, 0) == -1)
    return;

  ctx.global_lock_fd = fd;
#endif
}

template <typename E>
void release_global_lock(Context<E> &ctx) {
#ifndef _WIN32
  if (ctx.global_lock_fd)
    close(*ctx.global_lock_fd);
#endif
}

using E = MOLD_TARGET;

template void acquire_global_lock(Context<E> &);
template void release_global_lock(Context<E> &);

} // namespace mold::elf
