#include "mold.h"
#include "lto.h"

namespace mold {

template <typename E>
ObjectFile<E> *read_lto_object(Context<E> &ctx, MappedFile *mf) {
  Fatal(ctx) << "LTO is not supported on Windows";
}

template <typename E>
std::vector<ObjectFile<E> *> run_lto_plugin(Context<E> &ctx) {
  return {};
}

template <typename E>
void lto_cleanup(Context<E> &ctx) {}

using E = MOLD_TARGET;

template ObjectFile<E> *read_lto_object(Context<E> &, MappedFile *);
template std::vector<ObjectFile<E> *> run_lto_plugin(Context<E> &);
template void lto_cleanup(Context<E> &);

} // namespace mold
