#include "mold.h"

#include <algorithm>

namespace mold::macho {

std::ostream &operator<<(std::ostream &out, const InputSection &sec) {
  out << sec.file << "(" << sec.hdr.get_segname() << ","
      << sec.hdr.get_sectname() << ")";
  return out;
}

InputSection::InputSection(Context &ctx, ObjectFile &file, const MachSection &hdr)
  : file(file), hdr(hdr),
    osec(*OutputSection::get_instance(ctx, hdr.get_segname(), hdr.get_sectname())) {
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

static Relocation read_reloc(Context &ctx, ObjectFile &file,
                             const MachSection &hdr, MachRel r) {
  u8 *buf = (u8 *)file.mf->data + hdr.offset;

  i64 addend;
  if (r.p2size == 0)
    addend = *(i8 *)(buf + r.offset);
  else if (r.p2size == 1)
    addend = *(i16 *)(buf + r.offset);
  else if (r.p2size == 2)
    addend = *(i32 *)(buf + r.offset);
  else if (r.p2size == 3)
    addend = *(i64 *)(buf + r.offset);
  else
    unreachable();

  bool is_gotref = (r.type == X86_64_RELOC_GOT_LOAD || r.type == X86_64_RELOC_GOT);

  if (r.is_extern)
    return {r.offset, (bool)r.is_pcrel, is_gotref, addend, file.syms[r.idx],
            nullptr};

  u32 addr;
  if (r.is_pcrel) {
    if (r.p2size != 2)
      Fatal(ctx) << file << ": invalid PC-relative reloc: " << r.offset;
    addr = hdr.addr + r.offset + 4 + addend;
  } else {
    addr = addend;
  }

  Subsection *target = file.sections[r.idx - 1]->find_subsection(ctx, addr);
  if (!target)
    Fatal(ctx) << file << ": bad relocation: " << r.offset;

  return {r.offset, (bool)r.is_pcrel, is_gotref, addr - target->input_addr,
          nullptr, target};
}

void InputSection::parse_relocations(Context &ctx) {
  rels.reserve(hdr.nreloc);

  // Parse mach-o relocations to fill `rels` vector
  MachRel *rel = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++)
    rels.push_back(read_reloc(ctx, file, hdr, rel[i]));

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

void InputSection::scan_relocations(Context &ctx) {
  for (Relocation &rel : rels) {
    if (Symbol *sym = rel.sym) {
      if (rel.is_gotref)
        sym->flags |= NEEDS_GOT;
      if (sym->file && sym->file->is_dylib)
        sym->flags |= NEEDS_STUB;
    }
  }
}

void Subsection::apply_reloc(Context &ctx, u8 *buf) {
  for (const Relocation &rel : std::span(isec.rels).subspan(rel_offset, nrels)) {
    u32 *loc = (u32 *)(buf + rel.offset);

#define S (rel.sym ? rel.sym->get_addr(ctx) : \
           rel.subsec->isec.osec.hdr.addr + rel.subsec->output_offset)
#define A rel.addend
#define P (isec.osec.hdr.addr + output_offset + rel.offset)
#define G rel.sym->get_got_addr(ctx)

    if (rel.is_gotref)
      *loc = G - P - 4;
    else if (rel.is_pcrel)
      *loc = S + A - P - 4;
    else
      *loc = S + A;

#undef S
#undef A
#undef P
#undef G
  }
}

} // namespace mold::macho
