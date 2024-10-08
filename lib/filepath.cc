#include "common.h"

#include <filesystem>
#include <sys/stat.h>

#ifdef __APPLE__
# include <mach-o/dyld.h>
#endif

#ifdef __FreeBSD__
# include <sys/sysctl.h>
#endif

namespace mold {

// Returns the path of the mold executable itself
std::string get_self_path() {
#if __APPLE__ || _WIN32
  fprintf(stderr, "mold: get_self_path is not supported");
  exit(1);
#elif __FreeBSD__
  // /proc may not be mounted on FreeBSD. The proper way to get the
  // current executable's path is to use sysctl(2).
  int mib[4];
  mib[0] = CTL_KERN;
  mib[1] = KERN_PROC;
  mib[2] = KERN_PROC_PATHNAME;
  mib[3] = -1;

  size_t size;
  sysctl(mib, 4, NULL, &size, NULL, 0);

  std::string path;
  path.resize(size);
  sysctl(mib, 4, path.data(), &size, NULL, 0);
  return path;
#else
  return std::filesystem::read_symlink("/proc/self/exe").string();
#endif
}

} // namespace mold
