#include "mold.h"

namespace mold::macho {

std::ostream &operator<<(std::ostream &out, const ObjectFile &file) {
  out << path_clean(file.mf->name);
  return out;
}

ObjectFile::ObjectFile(Context &ctx, MappedFile<Context> *mf)
  : mf(mf) {}

ObjectFile *ObjectFile::create(Context &ctx, MappedFile<Context> *mf) {
  ObjectFile *obj = new ObjectFile(ctx, mf);
  ctx.obj_pool.push_back(std::unique_ptr<ObjectFile>(obj));
  return obj;
};

void ObjectFile::parse(Context &ctx) {
  std::string_view data = mf->get_contents();

  MachHeader &hdr = *(MachHeader *)data.data();
  data = data.substr(sizeof(hdr));

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &cmd = *(LoadCommand *)data.data();

    switch (cmd.cmd) {
    case LC_SEGMENT_64: {
      InputSection *sec = new InputSection(ctx, *this, *(MachSection *)&cmd);
      sections.push_back(std::unique_ptr<InputSection>(sec));
      break;
    }
    case LC_BUILD_VERSION:
    case LC_SYMTAB:
    case LC_DYSYMTAB:
      break;
    default:
      Error(ctx) << *this << ": unknown load command: 0x" << std::hex << cmd.cmd;
    }

    data = data.substr(cmd.cmdsize);
  }
}

} // namespace mold::macho
