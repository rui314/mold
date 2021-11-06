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
  if (hdr.type != S_ZEROFILL)
    contents = file.mf->get_contents().substr(hdr.offset, hdr.size);

  Subsection *subsec = new Subsection{
    .isec = *this,
    .input_offset = 0,
    .input_size = (u32)contents.size(),
    .input_addr = (u32)hdr.addr,
    .p2align = (u8)hdr.p2align,
  };

  subsections.push_back(std::unique_ptr<Subsection>(subsec));
}

Subsection *InputSection::find_subsection(Context &ctx, u32 addr) {
  auto it = std::upper_bound(subsections.begin(), subsections.end(), addr,
                             [&](u32 addr,
                                 const std::unique_ptr<Subsection> &subsec) {
    return addr < subsec->input_addr;
  });

  if (it == subsections.begin())
    return nullptr;
  return it[-1].get();
}

static i64 read_addend(u8 *buf, MachRel r) {
  i64 addend = 0;

  switch (r.type) {
  case X86_64_RELOC_SIGNED_1:
    addend = 1;
    break;
  case X86_64_RELOC_SIGNED_2:
    addend = 2;
    break;
  case X86_64_RELOC_SIGNED_4:
    addend = 4;
    break;
  }

  switch (r.p2size) {
  case 0:
    return buf[r.offset] + addend;
  case 1:
    return *(i16 *)(buf + r.offset) + addend;
  case 2:
    return *(i32 *)(buf + r.offset) + addend;
  case 3:
    return *(i64 *)(buf + r.offset) + addend;
  }

  unreachable();
}

static Relocation read_reloc(Context &ctx, ObjectFile &file,
                             const MachSection &hdr, MachRel r) {
  u8 *buf = (u8 *)file.mf->data + hdr.offset;
  Relocation rel{r.offset, (u8)r.type, (u8)r.p2size, (bool)r.is_pcrel};
  i64 addend = read_addend(buf, r);

  if (r.is_extern) {
    rel.addend = addend;
    rel.sym = file.syms[r.idx];
    return rel;
  }

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

  rel.addend = addr - target->input_addr;
  rel.subsec = target;
  return rel;
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
  for (std::unique_ptr<Subsection> &subsec : subsections) {
    subsec->rel_offset = i;
    while (i < rels.size() &&
           rels[i].offset < subsec->input_offset + subsec->input_size)
      i++;
    subsec->nrels = i - subsec->rel_offset;
  }
}

void InputSection::scan_relocations(Context &ctx) {
  for (Relocation &rel : rels) {
    if (Symbol *sym = rel.sym) {
      if (rel.type == X86_64_RELOC_GOT_LOAD || rel.type == X86_64_RELOC_GOT)
        sym->flags |= NEEDS_GOT;
      if (sym->file && sym->file->is_dylib)
        sym->flags |= NEEDS_STUB;
    }
  }
}

void Subsection::apply_reloc(Context &ctx, u8 *buf) {
  for (const Relocation &rel : std::span(isec.rels).subspan(rel_offset, nrels)) {
    if (rel.sym && !rel.sym->file) {
      Error(ctx) << "undefined symbol: " << isec.file << ": " << *rel.sym;
      continue;
    }

    auto write = [&](u64 val) {
      if (rel.p2size == 0)
        *(buf + rel.offset) = val;
      else if (rel.p2size == 1)
        *(u16 *)(buf + rel.offset) = val;
      else if (rel.p2size == 2)
        *(u32 *)(buf + rel.offset) = val;
      else if (rel.p2size == 3)
        *(u64 *)(buf + rel.offset) = val;
      else
        unreachable();
    };

#define S (rel.sym ? rel.sym->get_addr(ctx) : rel.subsec->get_addr(ctx))
#define A rel.addend
#define P (rel.is_pcrel ? get_addr(ctx) + rel.offset + 4 : 0)
#define G rel.sym->get_got_addr(ctx)

    switch (rel.type) {
    case X86_64_RELOC_UNSIGNED:
    case X86_64_RELOC_SIGNED:
    case X86_64_RELOC_BRANCH:
    case X86_64_RELOC_SIGNED_1:
    case X86_64_RELOC_SIGNED_2:
    case X86_64_RELOC_SIGNED_4:
      write(S + A - P);
      break;
    case X86_64_RELOC_GOT_LOAD:
    case X86_64_RELOC_GOT:
      write(G + A - P);
      break;
    default:
      Fatal(ctx) << isec << ": unknown reloc: " << (int)rel.type;
    }

#undef S
#undef A
#undef P
#undef G
  }
}

} // namespace mold::macho
