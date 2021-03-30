#include "mold.h"

template <>
std::string rel_to_string(Context<I386> &ctx, u32 r_type) {
  return "";
}

template <>
void InputSection<I386>::apply_reloc_alloc(Context<I386> &ctx, u8 *base) {
}

template <>
void InputSection<I386>::apply_reloc_nonalloc(Context<I386> &ctx, u8 *base) {
}

template <>
void InputSection<I386>::scan_relocations(Context<I386> &ctx) {
}
