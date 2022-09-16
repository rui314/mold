#include "mold.h"

namespace mold::elf {

using E = SPARC64;

template <>
void PltSection<E>::copy_buf(Context<E> &ctx) {}

template <>
void PltGotSection<E>::copy_buf(Context<E> &ctx) {}

template <>
void EhFrameSection<E>::apply_reloc(Context<E> &ctx, const ElfRel<E> &rel,
                                    u64 offset, u64 val) {}

template <>
void InputSection<E>::apply_reloc_alloc(Context<E> &ctx, u8 *base) {}

template <>
void InputSection<E>::apply_reloc_nonalloc(Context<E> &ctx, u8 *base) {}

template <>
void InputSection<E>::scan_relocations(Context<E> &ctx) {}

} // namespace mold::elf
