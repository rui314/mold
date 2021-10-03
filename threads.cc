#include "mold.h"

#include <tbb/global_control.h>

namespace mold {

static i64 get_default_thread_count() {
  // mold doesn't scale above 32 threads.
  int n = tbb::global_control::active_value(
    tbb::global_control::max_allowed_parallelism);
  return std::min(n, 32);
}

void set_thread_count(i64 n) {
  if (n == 0)
    n = get_default_thread_count();
  tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism, n);
}

} // namespace mold
