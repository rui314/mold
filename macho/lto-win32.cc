#include "lto.h"
#include "mold.h"

namespace mold::macho {

template <typename E>
void load_lto_plugin(Context<E> &ctx) {
  Fatal(ctx) << "LTO is not supported on Windows";
}

template <typename E>
void do_lto(Context<E> &ctx) {}

#ifdef MOLD_ARM64
LTOPlugin::~LTOPlugin() {}
#endif

using E = MOLD_TARGET;

template void load_lto_plugin(Context<E> &);
template void do_lto(Context<E> &);

} // namespace mold::macho
