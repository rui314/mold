#include "mold.h"

#include <windows.h>

namespace mold {

void cleanup() {
  if (output_tmpfile)
    _unlink(output_tmpfile);
}

std::string errno_string() {
  LPVOID buf;
  DWORD dw = GetLastError();

  FormatMessage(FORMAT_MESSAGE_ALLOCATE_BUFFER |
                FORMAT_MESSAGE_FROM_SYSTEM |
                FORMAT_MESSAGE_IGNORE_INSERTS,
                nullptr, dw,
                MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT),
                (LPTSTR)&buf, 0, nullptr);

  std::string ret = (char *)buf;
  LocalFree(buf);
  return ret;
}

static LONG WINAPI vectored_handler(_EXCEPTION_POINTERS *exception_info) {
  static std::mutex mu;
  std::scoped_lock lock{mu};

  PEXCEPTION_RECORD rec = exception_info->ExceptionRecord;
  ULONG_PTR *p = rec->ExceptionInformation;

  if (rec->ExceptionCode == EXCEPTION_IN_PAGE_ERROR &&
      (ULONG_PTR)output_buffer_start <= p[1] &&
      p[1] < (ULONG_PTR)output_buffer_end) {
    static const char msg[] =
      "mold: failed to write to an output file. Disk full?\n";
    (void)!_write(_fileno(stderr), msg, sizeof(msg) - 1);
  } else if (rec->ExceptionCode == EXCEPTION_STACK_OVERFLOW) {
    static const char msg[] =
      "mold: stack overflow\n";
    (void)!_write(_fileno(stderr), msg, sizeof(msg) - 1);
  }

  cleanup();
  _exit(1);
}

void install_signal_handler() {
  AddVectoredExceptionHandler(0, vectored_handler);
}

} // namespace mold
