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
}

Subsection *InputSection::find_subsection(Context &ctx, u32 addr) {
  auto it = std::upper_bound(subsections.begin(), subsections.end(), addr,
                             [&](u32 addr, Subsection *subsec) {
    return addr < subsec->input_addr;
  });

  if (it == subsections.begin())
    return nullptr;
  return it[-1];
}

static i64 read_addend(u8 *buf, MachRel r) {
  i64 val = 0;

  switch (r.type) {
  case X86_64_RELOC_SIGNED_1:
    val = 1;
    break;
  case X86_64_RELOC_SIGNED_2:
    val = 2;
    break;
  case X86_64_RELOC_SIGNED_4:
    val = 4;
    break;
  }

  switch (r.p2size) {
  case 0:
    return buf[r.offset] + val;
  case 1:
    return *(i16 *)(buf + r.offset) + val;
  case 2:
    return *(i32 *)(buf + r.offset) + val;
  case 3:
    return *(i64 *)(buf + r.offset) + val;
  }

  unreachable();
}

static Relocation read_reloc(Context &ctx, ObjectFile &file,
                             const MachSection &hdr, MachRel r) {
  u8 *buf = (u8 *)file.mf->data + hdr.offset;
  Relocation rel{r.offset, (u8)r.type, (u8)r.p2size, (bool)r.is_pcrel};
  i64 addend = read_addend(buf, r);

  if (r.is_extern) {
    rel.sym = file.syms[r.idx];
    rel.addend = addend;
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

  Subsection *target = file.find_subsection(ctx, addr);;
  if (!target)
    Fatal(ctx) << file << ": bad relocation: " << r.offset;

  rel.subsec = target;
  rel.addend = addr - target->input_addr;
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
  for (Subsection *subsec : subsections) {
    subsec->rel_offset = i;
    while (i < rels.size() &&
           rels[i].offset < subsec->input_offset + subsec->input_size)
      i++;
    subsec->nrels = i - subsec->rel_offset;
  }
}

void Subsection::scan_relocations(Context &ctx) {
  for (Relocation &rel : get_rels()) {
    Symbol *sym = rel.sym;
    if (!sym)
      continue;

    switch (rel.type) {
    case X86_64_RELOC_GOT_LOAD:
    case X86_64_RELOC_GOT:
      if (sym->file->is_dylib)
        sym->flags |= NEEDS_GOT;
      break;
    case X86_64_RELOC_TLV:
      if (sym->file->is_dylib)
        sym->flags |= NEEDS_THREAD_PTR;
      break;
    }

    if (sym->file && sym->file->is_dylib)
      sym->flags |= NEEDS_STUB;
  }
}

void Subsection::apply_reloc(Context &ctx, u8 *buf) {
  for (const Relocation &rel : get_rels()) {
    if (rel.sym && !rel.sym->file) {
      Error(ctx) << "undefined symbol: " << isec.file << ": " << *rel.sym;
      continue;
    }

    u64 val = 0;

    switch (rel.type) {
    case X86_64_RELOC_UNSIGNED:
    case X86_64_RELOC_SIGNED:
    case X86_64_RELOC_BRANCH:
    case X86_64_RELOC_SIGNED_1:
    case X86_64_RELOC_SIGNED_2:
    case X86_64_RELOC_SIGNED_4:
      val = rel.sym ? rel.sym->get_addr(ctx) : rel.subsec->get_addr(ctx);
      break;
    case X86_64_RELOC_GOT_LOAD:
      if (rel.sym->got_idx != -1) {
        val = rel.sym->get_got_addr(ctx);
      } else {
        // Relax MOVQ into LEAQ
        if (buf[rel.offset - 2] != 0x8b)
          Error(ctx) << isec << ": invalid GOT_LOAD relocation";
        buf[rel.offset - 2] = 0x8d;
        val = rel.sym->get_addr(ctx);
      }
      break;
    case X86_64_RELOC_GOT:
      val = rel.sym->get_got_addr(ctx);
      break;
    case X86_64_RELOC_TLV:
      if (rel.sym->tlv_idx != -1) {
        val = rel.sym->get_tlv_addr(ctx);
      } else {
        // Relax MOVQ into LEAQ
        if (buf[rel.offset - 2] != 0x8b)
          Error(ctx) << isec << ": invalid TLV relocation";
        buf[rel.offset - 2] = 0x8d;
        val = rel.sym->get_addr(ctx);
      }
      break;
    default:
      Fatal(ctx) << isec << ": unknown reloc: " << (int)rel.type;
    }

    val += rel.addend;

    if (rel.is_pcrel)
      val -= get_addr(ctx) + rel.offset + 4;

    switch (rel.p2size) {
    case 0:
      buf[rel.offset] = val;
      break;
    case 1:
      *(u16 *)(buf + rel.offset) = val;
      break;
    case 2:
      *(u32 *)(buf + rel.offset) = val;
      break;
    case 3:
      *(u64 *)(buf + rel.offset) = val;
      break;
    default:
      unreachable();
    };
  }
}

} // namespace mold::macho
