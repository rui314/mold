#include "mold.h"

#include <algorithm>

namespace mold::macho {

std::ostream &operator<<(std::ostream &out, const InputSection &sec) {
  out << sec.file << "(" << sec.hdr.segname << "," << sec.hdr.sectname << ")";
  return out;
}

InputSection::InputSection(Context &ctx, ObjectFile &file, const MachSection &hdr)
  : file(file), hdr(hdr) {
  contents = file.mf->get_contents().substr(hdr.offset, hdr.size);
  subsections.push_back({*this, 0, (u32)contents.size(), (u32)hdr.addr});
}

Subsection *InputSection::find_subsection(Context &ctx, u32 addr) {
  auto it = std::upper_bound(subsections.begin(), subsections.end(), addr,
                             [&](u32 addr, const Subsection &subsec) {
    return addr < subsec.input_addr;
  });

  if (it == subsections.begin())
    Fatal(ctx) << *this << ": bad relocation at 0x" << std::hex << addr;
  return &*(it - 1);
}

static u64 read_addend(u8 *buf, u32 offset, u32 p2size) {
  switch (p2size) {
  case 0: return buf[offset];
  case 1: return *(u16 *)(buf + offset);
  case 2: return *(u32 *)(buf + offset);
  case 3: return *(i64 *)(buf + offset);
  }
  unreachable();
}

void InputSection::parse_relocations(Context &ctx) {
  rels.reserve(hdr.nreloc);

  MachRel *rel = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++) {
    MachRel &r = rel[i];
    u64 addend = read_addend((u8 *)contents.data(), r.offset, r.p2size);

    if (r.is_extern) {
      rels.push_back({r.offset, addend, file.syms[r.idx], nullptr});
    } else {
      u32 addr;
      if (r.is_pcrel) {
        if (r.p2size != 2)
          Fatal(ctx) << *this << ": invalid PC-relative reloc: " << i;
        addr = hdr.addr + r.offset + 4 + addend;
      } else {
	addr = addend;
      }

      Subsection *target = file.sections[r.idx]->find_subsection(ctx, addr);
      rels.push_back({r.offset, addend - target->input_addr, nullptr, target});
    }
  }
}

} // namespace mold::macho
