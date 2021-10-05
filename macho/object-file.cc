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
  MachHeader &hdr = *(MachHeader *)mf->data;
  u8 *p = mf->data + sizeof(hdr);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;

    switch (lc.cmd) {
    case LC_SEGMENT_64: {
      SegmentCommand &cmd = *(SegmentCommand *)p;
      MachSection *mach_sec = (MachSection *)(p + sizeof(cmd));

      for (i64 i = 0; i < cmd.nsects; i++)
	sections.push_back(std::make_unique<InputSection>(ctx, *this, mach_sec[i]));
      break;
    }
    case LC_SYMTAB: {
      SymtabCommand &cmd = *(SymtabCommand *)p;
      MachSym *mach_sym = (MachSym *)(p + cmd.symoff);
      syms.reserve(cmd.nsyms);

      for (i64 j = 0; j < cmd.nsyms; j++) {
	std::string_view name =
	  (char *)(mf->data + cmd.stroff + mach_sym[j].stroff);
	syms.push_back(intern(ctx, name));
      }
      break;
    }
    case LC_DYSYMTAB:
    case LC_BUILD_VERSION:
      break;
    default:
      Error(ctx) << *this << ": unknown load command: 0x" << std::hex << lc.cmd;
    }

    p += lc.cmdsize;
  }

  for (std::unique_ptr<InputSection> &sec : sections)
    sec->parse_relocations(ctx);
}

} // namespace mold::macho
