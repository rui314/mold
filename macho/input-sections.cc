#include "mold.h"

#include <algorithm>

namespace mold::macho {

std::ostream &operator<<(std::ostream &out, const InputSection &sec) {
  out << sec.file << "(" << sec.hdr.get_segname() << ","
      << sec.hdr.get_sectname() << ")";
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

void InputSection::parse_relocations(Context &ctx) {
  rels.reserve(hdr.nreloc);

  // Parse mach-o relocations to fill `rels` vector
  MachRel *rel = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++)
    rels.push_back(file.read_reloc(ctx, hdr, rel[i]));

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
      *loc = rel.sym->get_addr(ctx) + rel.addend;
    } else {
      *loc = rel.subsec->isec.osec->hdr.addr + rel.subsec->output_offset +
             rel.addend;
    }

    if (rel.is_pcrel)
      *loc = *loc - isec.osec->hdr.addr - output_offset - rel.offset - 4;
  }
}

} // namespace mold::macho
