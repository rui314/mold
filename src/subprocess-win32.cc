#include "mold.h"

namespace mold {

#ifdef MOLD_X86_64
void fork_child() {}
void notify_parent() {}
#endif

template <typename E>
[[noreturn]]
void process_run_subcommand(Context<E> &ctx, int argc, char **argv) {
  Fatal(ctx) << "-run is supported only on Unix";
}

using E = MOLD_TARGET;

template void process_run_subcommand(Context<E> &, int, char **);

} // namespace mold
