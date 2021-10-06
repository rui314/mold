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
    return nullptr;
  return &*(it - 1);
}

static i64 read_addend(u8 *buf, u32 offset, u32 p2size) {
  switch (p2size) {
  case 0: return *(i8 *)(buf + offset);
  case 1: return *(i16 *)(buf + offset);
  case 2: return *(i32 *)(buf + offset);
  case 3: return *(i64 *)(buf + offset);
  }
  unreachable();
}

void InputSection::parse_relocations(Context &ctx) {
  rels.reserve(hdr.nreloc);

  // Parse mach-o relocations to fill `rels` vector
  MachRel *rel = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++) {
    MachRel &r = rel[i];
    i64 addend = read_addend((u8 *)contents.data(), r.offset, r.p2size);

    if (r.is_extern) {
      rels.push_back({r.offset, (bool)r.is_pcrel, addend, file.syms[r.idx],
                      nullptr});
    } else {
      u32 addr;
      if (r.is_pcrel) {
        if (r.p2size != 2)
          Fatal(ctx) << *this << ": invalid PC-relative reloc: " << i;
        addr = hdr.addr + r.offset + 4 + addend;
      } else {
	addr = addend;
      }

      Subsection *target = file.sections[r.idx - 1]->find_subsection(ctx, addr);
      if (!target)
	Fatal(ctx) << *this << ": bad relocation: " << i;

      rels.push_back({r.offset, (bool)r.is_pcrel, addend - target->input_addr,
                      nullptr, target});
    }
  }

  // Sort `rels` vector
  sort(rels, [](const Relocation &a, const Relocation &b) {
    return a.offset < b.offset;
  });

  // Assign each subsection a group of relocations
  i64 i = 0;
  for (Subsection &subsec : subsections) {
    subsec.rel_offset = i;
    while (i < rels.size() &&
           rels[i].offset < subsec.input_offset + subsec.input_size)
      i++;
    subsec.nrels = i - subsec.rel_offset;
  }
}

void Subsection::apply_reloc(Context &ctx, u8 *buf) {
  for (const Relocation &rel : std::span(isec.rels).subspan(rel_offset, nrels)) {
    u32 *loc = (u32 *)(buf + rel.offset);

    if (rel.sym) {
      *loc = rel.sym->get_addr() + rel.addend;
    } else {
      *loc = rel.subsec->isec.osec->hdr.addr + rel.subsec->output_offset +
             rel.addend;
    }

    if (rel.is_pcrel)
      *loc = *loc - isec.osec->hdr.addr - output_offset - rel.offset - 4;
  }
}

} // namespace mold::macho
