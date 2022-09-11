#include "mold.h"

namespace mold::macho {

template <typename E>
std::unique_ptr<DwarfObject<E>>
DwarfObject<E>::create(ObjectFile<E> *obj) {
  std::unique_ptr<DwarfObject<E>> dwarf_obj = std::make_unique<DwarfObject>();

  bool has_dwarf_info = false;
  for (std::unique_ptr<InputSection<E>> &isec : obj->debug_sections) {
    if (isec->hdr.match("__DWARF", "__debug_str")) {
      dwarf_obj->str_section = isec->contents;
      has_dwarf_info = true;
    }
  }

  if (has_dwarf_info)
    return dwarf_obj;
  return nullptr;
}


#define INSTANTIATE(E)           \
  template class DwarfObject<E>;

INSTANTIATE_ALL;

} // namespace mold::macho