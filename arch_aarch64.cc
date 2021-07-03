#include "mold.h"

template <>
void PltSection<AARCH64>::copy_buf(Context<AARCH64> &ctx) {
}

template <>
void PltGotSection<AARCH64>::copy_buf(Context<AARCH64> &ctx) {
}

template <>
void EhFrameSection<AARCH64>::apply_reloc(Context<AARCH64> &ctx,
                                       ElfRel<AARCH64> &rel,
                                       u64 loc, u64 val) {
}

template <>
void InputSection<AARCH64>::apply_reloc_alloc(Context<AARCH64> &ctx, u8 *base) {
}

template <>
void InputSection<AARCH64>::apply_reloc_nonalloc(Context<AARCH64> &ctx, u8 *base) {
}

template <>
void InputSection<AARCH64>::scan_relocations(Context<AARCH64> &ctx) {
}
