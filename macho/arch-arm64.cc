#include "mold.h"

#include <algorithm>

namespace mold::macho {

template <>
void StubsSection<ARM64>::copy_buf(Context<ARM64> &ctx) {
  u8 *buf = ctx.buf + this->hdr.offset;

  for (i64 i = 0; i < syms.size(); i++) {
    // `ff 25 xx xx xx xx` is a RIP-relative indirect jump instruction,
    // i.e., `jmp *IMM(%rip)`. It loads an address from la_symbol_ptr
    // and jump there.
    assert(ARM64::stub_size == 6);
    buf[i * 6] = 0xff;
    buf[i * 6 + 1] = 0x25;
    *(u32 *)(buf + i * 6 + 2) =
      ctx.lazy_symbol_ptr.hdr.addr + i * ARM64::wordsize -
      (this->hdr.addr + i * 6 + 6);
  }
}

template <>
void StubHelperSection<ARM64>::copy_buf(Context<ARM64> &ctx) {
  u8 *start = ctx.buf + this->hdr.offset;
  u8 *buf = start;

  u8 insn0[16] = {
    0x4c, 0x8d, 0x1d, 0, 0, 0, 0, // lea $__dyld_private(%rip), %r11
    0x41, 0x53,                   // push %r11
    0xff, 0x25, 0, 0, 0, 0,       // jmp *$dyld_stub_binder@GOT(%rip)
    0x90,                         // nop
  };

  memcpy(buf, insn0, sizeof(insn0));
  *(u32 *)(buf + 3) =
    intern(ctx, "__dyld_private")->get_addr(ctx) - this->hdr.addr - 7;
  *(u32 *)(buf + 11) =
    intern(ctx, "dyld_stub_binder")->get_got_addr(ctx) - this->hdr.addr - 15;

  buf += 16;

  for (i64 i = 0; i < ctx.stubs.syms.size(); i++) {
    u8 insn[10] = {
      0x68, 0, 0, 0, 0, // push $bind_offset
      0xe9, 0, 0, 0, 0, // jmp $__stub_helper
    };

    memcpy(buf, insn, sizeof(insn));

    *(u32 *)(buf + 1) = ctx.stubs.bind_offsets[i];
    *(u32 *)(buf + 6) = start - buf - 10;
    buf += 10;
  }
}

// Returns [hi:lo] bits of val.
static u64 bits(u64 val, u64 hi, u64 lo) {
  return (val >> lo) & (((u64)1 << (hi - lo + 1)) - 1);
}

static i64 read_addend(u8 *buf, const MachRel &r) {
  switch (r.p2size) {
  case 2:
    return *(i32 *)(buf + r.offset);
  case 3:
    return *(i64 *)(buf + r.offset);
  default:
    unreachable();
  }
}

static Relocation<ARM64>
read_reloc(Context<ARM64> &ctx, ObjectFile<ARM64> &file,
           const MachSection &hdr, MachRel *rels, i64 &idx) {
  i64 addend = 0;

  switch (rels[idx].type) {
  case ARM64_RELOC_UNSIGNED:
  case ARM64_RELOC_SUBTRACTOR:
    addend = read_addend((u8 *)file.mf->data + hdr.offset, rels[idx]);
    break;
  case ARM64_RELOC_ADDEND:
    addend = rels[idx++].offset;
    break;
  }

  MachRel &r = rels[idx];
  Relocation<ARM64> rel{r.offset, (u8)r.type, (u8)r.p2size, (bool)r.is_pcrel};

  if (r.is_extern) {
    rel.sym = file.syms[r.idx];
    rel.addend = addend;
    return rel;
  }

  u32 addr;
  if (r.is_pcrel)
    addr = hdr.addr + r.offset + addend;
  else
    addr = addend;

  Subsection<ARM64> *target = file.find_subsection(ctx, addr);
  if (!target)
    Fatal(ctx) << file << ": bad relocation: " << r.offset;

  rel.subsec = target;
  rel.addend = addr - target->input_addr;
  return rel;
}

template <>
std::vector<Relocation<ARM64>>
read_relocations(Context<ARM64> &ctx, ObjectFile<ARM64> &file,
                 const MachSection &hdr) {
  std::vector<Relocation<ARM64>> vec;

  MachRel *rels = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++)
    vec.push_back(read_reloc(ctx, file, hdr, rels, i));
  return vec;
}

template <>
void Subsection<ARM64>::scan_relocations(Context<ARM64> &ctx) {
  for (Relocation<ARM64> &r : get_rels()) {
    Symbol<ARM64> *sym = r.sym;
    if (!sym)
      continue;

    switch (r.type) {
    case ARM64_RELOC_GOT_LOAD_PAGE21:
    case ARM64_RELOC_GOT_LOAD_PAGEOFF12:
    case ARM64_RELOC_POINTER_TO_GOT:
      sym->flags |= NEEDS_GOT;
      break;
    case ARM64_RELOC_TLVP_LOAD_PAGE21:
    case ARM64_RELOC_TLVP_LOAD_PAGEOFF12:
      sym->flags |= NEEDS_THREAD_PTR;
      break;
    }

    if (sym->file && sym->file->is_dylib) {
      sym->flags |= NEEDS_STUB;
      ((DylibFile<ARM64> *)sym->file)->is_needed = true;
    }
  }
}

template <>
void Subsection<ARM64>::apply_reloc(Context<ARM64> &ctx, u8 *buf) {
  std::span<Relocation<ARM64>> rels = get_rels();

  for (i64 i = 0; i < rels.size(); i++) {
    Relocation<ARM64> &r = rels[i];

    if (r.sym && !r.sym->file) {
      Error(ctx) << "undefined symbol: " << isec.file << ": " << *r.sym;
      continue;
    }

    u64 val = 0;

    switch (r.type) {
    case ARM64_RELOC_UNSIGNED:
    case ARM64_RELOC_BRANCH26:
    case ARM64_RELOC_PAGE21:
    case ARM64_RELOC_PAGEOFF12:
      val = r.sym ? r.sym->get_addr(ctx) : r.subsec->get_addr(ctx);
      break;
    case ARM64_RELOC_SUBTRACTOR: {
      Relocation<ARM64> s = rels[++i];
      assert(s.type == ARM64_RELOC_UNSIGNED);
      u64 val1 = r.sym ? r.sym->get_addr(ctx) : r.subsec->get_addr(ctx);
      u64 val2 = s.sym ? s.sym->get_addr(ctx) : s.subsec->get_addr(ctx);
      val = val2 - val1;
      break;
    }
    case ARM64_RELOC_GOT_LOAD_PAGE21:
    case ARM64_RELOC_GOT_LOAD_PAGEOFF12:
    case ARM64_RELOC_POINTER_TO_GOT:
      val = r.sym->get_got_addr(ctx);
      break;
    case ARM64_RELOC_TLVP_LOAD_PAGE21:
    case ARM64_RELOC_TLVP_LOAD_PAGEOFF12:
      val = r.sym->get_tlv_addr(ctx);
      break;
    default:
      Fatal(ctx) << isec << ": unknown reloc: " << (int)r.type;
    }

    val += r.addend;

    if (r.is_pcrel)
      val -= get_addr(ctx) + r.offset;

    switch (r.type) {
    case ARM64_RELOC_UNSIGNED:
    case ARM64_RELOC_SUBTRACTOR:
    case ARM64_RELOC_POINTER_TO_GOT:
      switch (r.p2size) {
      case 2:
        *(i32 *)(buf + r.offset) = val;
        break;
      case 3:
        *(i64 *)(buf + r.offset) = val;
        break;
      default:
        unreachable();
      }
      break;
    case ARM64_RELOC_BRANCH26:
      *(u32 *)(buf + r.offset) |= bits(val, 27, 2);
      break;
    case ARM64_RELOC_PAGE21:
    case ARM64_RELOC_GOT_LOAD_PAGE21:
    case ARM64_RELOC_TLVP_LOAD_PAGE21:
      *(u32 *)(buf + r.offset) |=
        (bits(val, 13, 12) << 29) | (bits(val, 32, 14) << 5);
      break;
    case ARM64_RELOC_PAGEOFF12:
    case ARM64_RELOC_GOT_LOAD_PAGEOFF12:
    case ARM64_RELOC_TLVP_LOAD_PAGEOFF12: {
      u32 insn = *(u32 *)(buf + r.offset);
      i64 scale = 0;
      if ((insn & 0x3b000000) == 0x39000000)
        scale = insn >> 30;
      *(u32 *)(buf + r.offset) |= bits(val, 12, scale) << 10;
      break;
    }
    default:
      Fatal(ctx) << isec << ": unknown reloc: " << (int)r.type;
    }
  }
}

} // namespace mold::macho
