#include "common.h"

#include <signal.h>
#include <tbb/version.h>

#ifdef __FreeBSD__
# include <sys/sysctl.h>
# include <unistd.h>
#endif

namespace mold {

std::string errno_string() {
  // strerror is not thread-safe, so guard it with a lock.
  static std::mutex mu;
  std::scoped_lock lock(mu);
  return strerror(errno);
}

void cleanup() {
  if (output_tmpfile)
    unlink(output_tmpfile);
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
static std::string sigabrt_msg;

static void sighandler(int signo, siginfo_t *info, void *ucontext) {
  static std::mutex mu;
  std::scoped_lock lock{mu};

  // Handle disk full error
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

  // Re-throw the signal
  signal(SIGSEGV, SIG_DFL);
  signal(SIGBUS, SIG_DFL);
  signal(SIGABRT, SIG_DFL);

  cleanup();
  raise(signo);
}

void install_signal_handler() {
  struct sigaction action;
  action.sa_sigaction = sighandler;
  sigemptyset(&action.sa_mask);
  action.sa_flags = SA_SIGINFO;

  sigaction(SIGSEGV, &action, NULL);
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

} // namespace mold
