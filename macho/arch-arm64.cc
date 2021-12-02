#include "mold.h"

#include <algorithm>

namespace mold::macho {

template <>
Relocation<ARM64>
read_reloc(Context<ARM64> &ctx, ObjectFile<ARM64> &file,
           const MachSection &hdr, MachRel r) {
  return {};
}

template <>
void Subsection<ARM64>::scan_relocations(Context<ARM64> &ctx) {
}

template <>
void Subsection<ARM64>::apply_reloc(Context<ARM64> &ctx, u8 *buf) {
}

} // namespace mold::macho
