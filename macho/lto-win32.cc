#include "lto.h"
#include "mold.h"

namespace mold::macho {

LTOPlugin::~LTOPlugin() {}

template <typename E>
void load_lto_plugin(Context<E> &ctx) {
  Fatal(ctx) << "LTO is not supported on Windows";
}

template <typename E>
void do_lto(Context<E> &ctx) {}

using E = MOLD_TARGET;

template void load_lto_plugin(Context<E> &);
template void do_lto(Context<E> &);

} // namespace mold::macho
