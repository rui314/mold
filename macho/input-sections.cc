#include "mold.h"

namespace mold::macho {

InputSection::InputSection(Context &ctx, ObjectFile &file, const MachSection &hdr)
  : file(file), hdr(hdr) {
  contents = file.mf->get_contents().substr(hdr.offset, hdr.size);
}

} // namespace mold::macho
