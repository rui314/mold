#include "mold.h"

#include <algorithm>

namespace mold::macho {

static i64 get_reloc_addend(u32 type) {
  switch (type) {
  case X86_64_RELOC_SIGNED_1:
    return 1;
  case X86_64_RELOC_SIGNED_2:
    return 2;
  case X86_64_RELOC_SIGNED_4:
    return 4;
  default:
    return 0;
  }
}

static i64 read_addend(u8 *buf, const MachRel &r) {
  switch (r.p2size) {
  case 2:
    return *(i32 *)(buf + r.offset) + get_reloc_addend(r.type);
  case 3:
    return *(i64 *)(buf + r.offset) + get_reloc_addend(r.type);
  default:
    unreachable();
  }
}

template <>
Relocation<X86_64>
read_reloc(Context<X86_64> &ctx, ObjectFile<X86_64> &file,
           const MachSection &hdr, MachRel r) {
  if (r.p2size != 2 && r.p2size != 3)
    Fatal(ctx) << file << ": invalid r.p2size: " << (u32)r.p2size;

  if (r.is_pcrel) {
    if (r.p2size != 2)
      Fatal(ctx) << file << ": invalid PC-relative reloc: " << r.offset;
  } else {
    if (r.p2size != 3)
      Fatal(ctx) << file << ": invalid non-PC-relative reloc: " << r.offset;
  }

  u8 *buf = (u8 *)file.mf->data + hdr.offset;
  Relocation<X86_64> rel{r.offset, (u8)r.type, (u8)r.p2size, (bool)r.is_pcrel};
  i64 addend = read_addend(buf, r);

  if (r.is_extern) {
    rel.sym = file.syms[r.idx];
    rel.addend = addend;
    return rel;
  }

  u32 addr;
  if (r.is_pcrel)
    addr = hdr.addr + r.offset + 4 + addend;
  else
    addr = addend;

  Subsection<X86_64> *target = file.find_subsection(ctx, addr);
  if (!target)
    Fatal(ctx) << file << ": bad relocation: " << r.offset;

  rel.subsec = target;
  rel.addend = addr - target->input_addr;
  return rel;
}

template <>
void Subsection<X86_64>::scan_relocations(Context<X86_64> &ctx) {
  for (Relocation<X86_64> &rel : get_rels()) {
    Symbol<X86_64> *sym = rel.sym;
    if (!sym)
      continue;

    switch (rel.type) {
    case X86_64_RELOC_GOT_LOAD:
      if (sym->file && sym->file->is_dylib)
        sym->flags |= NEEDS_GOT;
      break;
    case X86_64_RELOC_GOT:
      sym->flags |= NEEDS_GOT;
      break;
    case X86_64_RELOC_TLV:
      if (sym->file && sym->file->is_dylib)
        sym->flags |= NEEDS_THREAD_PTR;
      break;
    }

    if (sym->file && sym->file->is_dylib) {
      sym->flags |= NEEDS_STUB;
      ((DylibFile<X86_64> *)sym->file)->is_needed = true;
    }
  }
}

template <>
void Subsection<X86_64>::apply_reloc(Context<X86_64> &ctx, u8 *buf) {
  for (const Relocation<X86_64> &rel : get_rels()) {
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
      val -= get_addr(ctx) + rel.offset + 4 + get_reloc_addend(rel.type);

    switch (rel.p2size) {
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
