#include "common.h"
#include "config.h"

#include <tbb/global_control.h>

#ifdef USE_SYSTEM_MIMALLOC
#include <mimalloc-new-delete.h>
#endif

#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif

#ifdef __FreeBSD__
# include <sys/sysctl.h>
# include <unistd.h>
#endif

#ifdef _WIN32
# define unlink _unlink
#endif

namespace mold {

std::string mold_version_string = MOLD_VERSION;

static std::string get_mold_version() {
  if (mold_git_hash.empty())
    return "mold "s + MOLD_VERSION + " (compatible with GNU ld)";
  return "mold "s + MOLD_VERSION + " (" + mold_git_hash +
         "; compatible with GNU ld)";
}

void cleanup() {
  if (output_tmpfile)
    unlink(output_tmpfile);
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
#else
  return std::filesystem::read_symlink("/proc/self/exe").string();
#endif
}

i64 get_default_thread_count() {
  // mold doesn't scale well above 32 threads.
  int n = tbb::global_control::active_value(
    tbb::global_control::max_allowed_parallelism);
  return std::min(n, 32);
}

} // namespace mold

namespace mold::elf {
int main(int argc, char **argv);
}

namespace mold::macho {
int main(int argc, char **argv);
}

int main(int argc, char **argv) {
  mold::mold_version = mold::get_mold_version();
  return mold::elf::main(argc, argv);
}
