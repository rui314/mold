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

std::string get_realpath(std::string_view path) {
  std::error_code ec;
  std::filesystem::path link = std::filesystem::read_symlink(path, ec);
  if (ec)
    return std::string(path);
  return (filepath(path) / ".." / link).lexically_normal().string();
}

// Removes redundant '/..' or '/.' from a given path.
// The transformation is done purely by lexical processing.
// This function does not access file system.
std::string path_clean(std::string_view path) {
  return filepath(path).lexically_normal().string();
}

std::filesystem::path to_abs_path(std::filesystem::path path) {
  if (path.is_absolute())
    return path.lexically_normal();
  return (std::filesystem::current_path() / path).lexically_normal();
}

// Returns the path of the mold executable itself
std::string get_self_path() {
#if __APPLE__
    char path[8192];
    u32 size = sizeof(path);
    if (_NSGetExecutablePath(path, &size)) {
      std::cerr << "_NSGetExecutablePath failed\n";
      exit(1);
    }
    return path;
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
#elif _WIN32
  char path[32768];
  if (!GetModuleFileName(nullptr, path, sizeof(path))) {
    std::cerr << "GetModuleFileName failed\n";
    exit(1);
  }
  return path;
#else
  return std::filesystem::read_symlink("/proc/self/exe").string();
#endif
}

} // namespace mold
