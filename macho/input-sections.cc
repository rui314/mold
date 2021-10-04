#include "mold.h"

namespace mold::macho {

InputSection::InputSection(Context &ctx, ObjectFile &file, const MachSection &hdr)
  : file(file), hdr(hdr) {
  contents = file.mf->get_contents().substr(hdr.offset, hdr.size);
  subsections.push_back({*this, 0, (u32)contents.size()});

  rels.reserve(hdr.nreloc);

  MachRel *rel = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++) {
    MachRel r = rel[i];
    rels.push_back({r.offset, r.type});
  }
}

} // namespace mold::macho
