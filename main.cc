#include "mold.h"

#include <cstring>
#include <signal.h>
#include <tbb/global_control.h>

#ifdef USE_SYSTEM_MIMALLOC
#include <mimalloc-new-delete.h>
#endif

namespace mold {

namespace elf {
int main(int argc, char **argv);
}

namespace macho {
int main(int argc, char **argv);
}

static std::string get_mold_version() {
  if (mold_git_hash.empty())
    return "mold " MOLD_VERSION " (compatible with GNU ld)";
  return "mold " MOLD_VERSION " (" + mold_git_hash + "; compatible with GNU ld)";
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

static void sighandler(int signo, siginfo_t *info, void *ucontext) {
  static std::mutex mu;
  std::scoped_lock lock{mu};

  if ((signo == SIGSEGV || signo == SIGBUS) &&
      output_buffer_start <= info->si_addr &&
      info->si_addr < output_buffer_end) {
    const char msg[] = "mold: failed to write to an output file. Disk full?\n";
    (void)!write(STDERR_FILENO, msg, sizeof(msg) - 1);
  }

  cleanup();
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

  std::string cmd = mold::filepath(argv[0]).filename().string();
  if (cmd == "ld64" || cmd == "ld64.mold")
    return mold::macho::main(argc, argv);
  return mold::elf::main(argc, argv);
}
