#include "mold.h"

#include <cstring>
#include <signal.h>

#ifdef USE_SYSTEM_MIMALLOC
#include <mimalloc-new-delete.h>
#endif

namespace mold {

#ifdef GIT_HASH
const std::string mold_version =
  "mold " MOLD_VERSION " (" GIT_HASH "; compatible with GNU ld)";
#else
const std::string mold_version =
  "mold " MOLD_VERSION " (compatible with GNU ld)";
#endif

namespace elf {
int main(int argc, char **argv);
}

namespace macho {
int main(int argc, char **argv);
}

void cleanup() {
  if (output_tmpfile)
    unlink(output_tmpfile);
  if (socket_tmpfile)
    unlink(socket_tmpfile);
}

// mold mmap's an output file, and the mmap succeeds even if there's
// no enough space left on the filesystem. The actual disk blocks are
// not allocated on the mmap call but when the program writes to it
// for the first time.
//
// If a disk becomes full as a result of a write to an mmap'ed memory
// region, the failure of the write is reported as a SIGBUS. This
// signal handler catches that signal and print out a user-friendly
// error message. Without this, it is very hard to realize that the
// disk might be full.
static void sighandler(int signo, siginfo_t *info, void *ucontext) {
  static std::mutex mu;
  std::scoped_lock lock{mu};

  if ((signo == SIGSEGV || signo == SIGBUS) &&
      output_buffer_start <= info->si_addr &&
      info->si_addr < output_buffer_end) {
    const char msg[] = "mold: failed to write to an output file. Disk full?\n";
    write(STDERR_FILENO, msg, sizeof(msg) - 1);
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

} // namespace mold

int main(int argc, char **argv) {
  std::string cmd = mold::filepath(argv[0]).filename();

  if (cmd == "ld64" || cmd == "ld64.mold")
    return mold::macho::main(argc, argv);

  return mold::elf::main(argc, argv);
}
