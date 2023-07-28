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
  if (!jobs || std::string(jobs) != "1")
    return;

  char *home = getenv("HOME");
  if (!home)
    home = getpwuid(getuid())->pw_dir;

  std::string path = std::string(home) + "/.mold-lock";
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
