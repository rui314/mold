#include "mold.h"

namespace mold {

template <typename E>
void fork_child() {}

template <typename E>
void notify_parent() {}

template <typename E>
[[noreturn]]
void process_run_subcommand(Context<E> &ctx, int argc, char **argv) {
  Fatal(ctx) << "-run is supported only on Unix";
}

using E = MOLD_TARGET;

template void fork_child<E>();
template void notify_parent<E>();
template void process_run_subcommand(Context<E> &, int, char **);

} // namespace mold
