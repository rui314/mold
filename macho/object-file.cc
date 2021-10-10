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

  MachSection *unwind_sec = nullptr;

  for (i64 i = 0; i < hdr.ncmds; i++) {
    LoadCommand &lc = *(LoadCommand *)p;

    switch (lc.cmd) {
    case LC_SEGMENT_64: {
      SegmentCommand &cmd = *(SegmentCommand *)p;
      MachSection *mach_sec = (MachSection *)(p + sizeof(cmd));

      for (i64 i = 0; i < cmd.nsects; i++) {
        if (mach_sec[i].get_segname() == "__LD" &&
            mach_sec[i].get_sectname() == "__compact_unwind") {
          unwind_sec = &mach_sec[i];
        } else {
          sections.push_back(
            std::make_unique<InputSection>(ctx, *this, mach_sec[i]));
        }
      }
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

  for (std::unique_ptr<InputSection> &sec : sections)
    sec->parse_relocations(ctx);

  if (unwind_sec)
    parse_compact_unwind(ctx, *unwind_sec);
}

void ObjectFile::parse_compact_unwind(Context &ctx, MachSection &hdr) {
  std::string_view contents = mf->get_contents().substr(hdr.offset, hdr.size);
  if (contents.size() % sizeof(CompactUnwindEntry))
    Fatal(ctx) << *this << ": invalid __compact_unwind section size";

  i64 num_entries = contents.size() % sizeof(CompactUnwindEntry);
  unwind_entries.reserve(num_entries);

  // Read comapct unwind entries
  for (i64 i = 0; i < num_entries; i++) {
    CompactUnwindEntry &ent = ((CompactUnwindEntry *)contents.data())[i];
    unwind_entries.push_back({{}, ent.code_len, ent.compact_unwind_info, {}, {}});
  }

  // Read relocations
  MachRel *mach_rels = (MachRel *)(mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++) {
    MachRel &r = mach_rels[i];
    i64 idx = r.offset / sizeof(CompactUnwindEntry);
    i64 offset = r.offset % sizeof(CompactUnwindEntry);

//    if (idx >= unwind_entries.size())
//      Fatal(ctx) << *this << ": relocation offset too large: " << i;
    UnwindEntry &ent = unwind_entries[idx];

    if (offset == offsetof(CompactUnwindEntry, code_start)) {
      
    }

    if (offset == offsetof(CompactUnwindEntry, personality)) {
    }

    if (offset == offsetof(CompactUnwindEntry, lsda)) {
    }

    // Fatal(ctx) << *this << ": unsupported relocation: " << i;
  }
}

void ObjectFile::resolve_symbols(Context &ctx) {
  for (i64 i = 0; i < syms.size(); i++) {
    Symbol &sym = *syms[i];
    MachSym &msym = mach_syms[i];

    switch (msym.type) {
    case N_ABS:
      sym.file = this;
      sym.subsec = nullptr;
      sym.value = msym.value;
      break;
    case N_SECT:
      sym.file = this;
      sym.subsec = sections[msym.sect - 1]->find_subsection(ctx, msym.value);
      sym.value = msym.value - sym.subsec->input_addr;
      break;
    }
  }
}

static i64 read_addend(u8 *buf, MachRel r) {
  switch (r.p2size) {
  case 0: return *(i8 *)(buf + r.offset);
  case 1: return *(i16 *)(buf + r.offset);
  case 2: return *(i32 *)(buf + r.offset);
  case 3: return *(i64 *)(buf + r.offset);
  }
  unreachable();
}

Relocation ObjectFile::read_reloc(Context &ctx, const MachSection &hdr, MachRel r) {
  i64 addend = read_addend((u8 *)mf->get_contents().data() + hdr.offset, r);

  if (r.is_extern)
    return {r.offset, (bool)r.is_pcrel, addend, syms[r.idx], nullptr};

  u32 addr;
  if (r.is_pcrel) {
    if (r.p2size != 2)
      Fatal(ctx) << *this << ": invalid PC-relative reloc: " << r.offset;
    addr = hdr.addr + r.offset + 4 + addend;
  } else {
    addr = addend;
  }

  Subsection *target = sections[r.idx - 1]->find_subsection(ctx, addr);
  if (!target)
    Fatal(ctx) << *this << ": bad relocation: " << r.offset;
  return {r.offset, (bool)r.is_pcrel, addr - target->input_addr, nullptr, target};
}

} // namespace mold::macho
