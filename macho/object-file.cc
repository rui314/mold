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

static u8 *find_section(u8 *buf, u32 type) {
  MachHeader &hdr = *(MachHeader *)buf;
  buf += sizeof(hdr);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)buf;
    if (lc.cmd == type)
      return buf;
    buf += lc.cmdsize;
  }
  return nullptr;
}

void ObjectFile::parse(Context &ctx) {
  u8 *buf = mf->data;

  // Read symbol table
  SymtabCommand *symtab = (SymtabCommand *)find_section(buf, LC_SYMTAB);
  if (!symtab)
    Fatal(ctx) << *this << ": LC_SYMTAB is missing";

  MachSym *mach_sym = (MachSym *)(buf + symtab->symoff);
  syms.reserve(symtab->nsyms);

  for (i64 j = 0; j < symtab->nsyms; j++) {
    std::string_view name = (char *)(buf + symtab->stroff + mach_sym[j].stroff);
    syms.push_back(Symbol::intern(ctx, name));
  }

  // Read other segments
  MachHeader &hdr = *(MachHeader *)buf;
  u8 *p = mf->data + sizeof(hdr);

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;

    switch (lc.cmd) {
    case LC_SEGMENT_64: {
      SegmentCommand &cmd = *(SegmentCommand *)p;
      MachSection *mach_sec = (MachSection *)(p + sizeof(cmd));

      for (i64 i = 0; i < cmd.nsects; i++) {
	sections.push_back(
          std::unique_ptr<InputSection>(new InputSection(ctx, *this, mach_sec[i])));
      }
      break;
    }
    case LC_BUILD_VERSION:
    case LC_SYMTAB:
    case LC_DYSYMTAB:
      break;
    default:
      Error(ctx) << *this << ": unknown load command: 0x" << std::hex << lc.cmd;
    }

    p += lc.cmdsize;
  }
}

} // namespace mold::macho
