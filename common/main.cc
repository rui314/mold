#include "common.h"
#include "config.h"

#include <cstring>
#include <filesystem>
#include <signal.h>
#include <tbb/global_control.h>
#include <tbb/version.h>

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

namespace mold {

std::string mold_version_string = MOLD_VERSION;

namespace elf {
int main(int argc, char **argv);
}

namespace macho {
int main(int argc, char **argv);
}

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

std::string errno_string() {
  // strerror is not thread-safe, so guard it with a lock.
  static std::mutex mu;
  std::scoped_lock lock(mu);
  return strerror(errno);
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

// mold mmap's an output file, and the mmap succeeds even if there's
// no enough space left on the filesystem. The actual disk blocks are
// not allocated on the mmap call but when the program writes to it
// for the first time.
//
// If a disk becomes full as a result of a write to an mmap'ed memory
// region, the failure of the write is reported as a SIGBUS or structured
// exeption with code EXCEPTION_IN_PAGE_ERROR on Windows. This
// signal handler catches that signal and prints out a user-friendly
// error message. Without this, it is very hard to realize that the
// disk might be full.
#ifdef _WIN32

static LONG WINAPI vectored_handler(_EXCEPTION_POINTERS *exception_info) {
  static std::mutex mu;
  std::scoped_lock lock{mu};

  PEXCEPTION_RECORD exception_record = exception_info->ExceptionRecord;
  ULONG_PTR *exception_information = exception_record->ExceptionInformation;
  if (exception_record->ExceptionCode == EXCEPTION_IN_PAGE_ERROR &&
      (ULONG_PTR)output_buffer_start <= exception_information[1] &&
      exception_information[1] < (ULONG_PTR)output_buffer_end) {

    const char msg[] = "mold: failed to write to an output file. Disk full?\n";
    (void)!write(_fileno(stderr), msg, sizeof(msg) - 1);
  }

  cleanup();
  _exit(1);
}

void install_signal_handler() {
  AddVectoredExceptionHandler(0, vectored_handler);
}

#else

static std::string sigabrt_msg;

static void sighandler(int signo, siginfo_t *info, void *ucontext) {
  static std::mutex mu;
  std::scoped_lock lock{mu};

  switch (signo) {
  case SIGSEGV:
  case SIGBUS:
    if (output_buffer_start <= info->si_addr &&
        info->si_addr < output_buffer_end) {
      const char msg[] = "mold: failed to write to an output file. Disk full?\n";
      (void)!write(STDERR_FILENO, msg, sizeof(msg) - 1);
    }
    break;
  case SIGABRT: {
    (void)!write(STDERR_FILENO, &sigabrt_msg[0], sigabrt_msg.size());
    break;
  }
  }

  _exit(1);
}

void install_signal_handler() {
  struct sigaction action;
  action.sa_sigaction = sighandler;
  sigemptyset(&action.sa_mask);
  action.sa_flags = SA_SIGINFO;

  sigaction(SIGINT, &action, NULL);
  sigaction(SIGTERM, &action, NULL);
  sigaction(SIGBUS, &action, NULL);

  // OneTBB 2021.9.0 has the interface version 12090.
  if (TBB_runtime_interface_version() < 12090) {
    sigabrt_msg = "mold: aborted\n"
      "mold: mold with libtbb version 2021.9.0 or older is known to be unstable "
      "under heavy load. Your libtbb version is " +
      std::string(TBB_runtime_version()) +
      ". Please upgrade your libtbb library and try again.\n";

    sigaction(SIGABRT, &action, NULL);
  }
}

#endif

i64 get_default_thread_count() {
  // mold doesn't scale well above 32 threads.
  int n = tbb::global_control::active_value(
    tbb::global_control::max_allowed_parallelism);
  return std::min(n, 32);
}

} // namespace mold

int main(int argc, char **argv) {
  mold::mold_version = mold::get_mold_version();
  return mold::elf::main(argc, argv);
}
