// The linker plugin reports diagnostics through a printf-like callback.
// C-variadic functions can't be defined in stable Rust, so this adapter
// formats the message and hands it to Rust.

#include <stdarg.h>
#include <stdio.h>

void mold_lto_report(int level, const char *msg);

int mold_lto_message(int level, const char *fmt, ...) {
  char buf[1000];
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(buf, sizeof(buf), fmt, ap);
  va_end(ap);
  mold_lto_report(level, buf);
  return 0;
}
