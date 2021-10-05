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
      mach_syms = {(MachSym *)(mf->data + cmd.symoff), cmd.nsyms};
      syms.reserve(mach_syms.size());

      for (MachSym &msym : mach_syms) {
        std::string_view name = (char *)(mf->data + cmd.stroff + msym.stroff);
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

  for (std::unique_ptr<InputSection> &sec : sections) {
    sec->parse_relocations(ctx);
    for (Relocation &rel : sec->rels)
      if (rel.sym)
	SyncOut(ctx) << *sec << ": " << rel.offset << " " << rel.addend
		     << " " << *rel.sym << "!";
      else
	SyncOut(ctx) << *sec << ": " << rel.offset << " " << rel.addend
		     << " " << rel.subsec;
  }
}

void ObjectFile::resolve_symbols(Context &ctx) {
  for (i64 i = 0; i < syms.size(); i++) {
    Symbol &sym = *syms[i];
    MachSym &msym = mach_syms[i];

    switch (msym.type) {
    case N_ABS:
      sym.file = this;
      sym.value = msym.value;
      break;
    case N_SECT:
      sym.file = this;
      sym.subsec = sections[msym.sect - 1]->find_subsection(ctx, msym.value);
      break;
    }
  }
}

} // namespace mold::macho
