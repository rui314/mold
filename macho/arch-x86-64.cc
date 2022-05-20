#include "mold.h"

namespace mold::macho {

template <>
void StubsSection<X86_64>::copy_buf(Context<X86_64> &ctx) {
  u8 *buf = ctx.buf + this->hdr.offset;

  for (i64 i = 0; i < syms.size(); i++) {
    // `ff 25 xx xx xx xx` is a RIP-relative indirect jump instruction,
    // i.e., `jmp *IMM(%rip)`. It loads an address from la_symbol_ptr
    // and jump there.
    static_assert(X86_64::stub_size == 6);
    buf[i * 6] = 0xff;
    buf[i * 6 + 1] = 0x25;
    *(ul32 *)(buf + i * 6 + 2) =
      ctx.lazy_symbol_ptr.hdr.addr + i * X86_64::word_size -
      (this->hdr.addr + i * 6 + 6);
  }
}

template <>
void StubHelperSection<X86_64>::copy_buf(Context<X86_64> &ctx) {
  u8 *start = ctx.buf + this->hdr.offset;
  u8 *buf = start;

  static const u8 insn0[] = {
    0x4c, 0x8d, 0x1d, 0, 0, 0, 0, // lea $__dyld_private(%rip), %r11
    0x41, 0x53,                   // push %r11
    0xff, 0x25, 0, 0, 0, 0,       // jmp *$dyld_stub_binder@GOT(%rip)
    0x90,                         // nop
  };

  static_assert(sizeof(insn0) == X86_64::stub_helper_hdr_size);

  memcpy(buf, insn0, sizeof(insn0));
  *(ul32 *)(buf + 3) =
    get_symbol(ctx, "__dyld_private")->get_addr(ctx) - this->hdr.addr - 7;
  *(ul32 *)(buf + 11) =
    get_symbol(ctx, "dyld_stub_binder")->get_got_addr(ctx) - this->hdr.addr - 15;

  buf += 16;

  for (i64 i = 0; i < ctx.stubs.syms.size(); i++) {
    u8 insn[] = {
      0x68, 0, 0, 0, 0, // push $bind_offset
      0xe9, 0, 0, 0, 0, // jmp $__stub_helper
    };

    static_assert(sizeof(insn) == X86_64::stub_helper_size);

    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 1) = ctx.stubs.bind_offsets[i];
    *(ul32 *)(buf + 6) = start - buf - 10;
    buf += 10;
  }
}

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
  if (r.p2size == 2)
    return *(il32 *)(buf + r.offset) + get_reloc_addend(r.type);
  if (r.p2size == 3)
    return *(il64 *)(buf + r.offset) + get_reloc_addend(r.type);
  unreachable();
}

static Relocation<X86_64>
read_reloc(Context<X86_64> &ctx, ObjectFile<X86_64> &file,
           const MachSection &hdr, MachRel &r) {
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
std::vector<Relocation<X86_64>>
read_relocations(Context<X86_64> &ctx, ObjectFile<X86_64> &file,
                 const MachSection &hdr) {
  std::vector<Relocation<X86_64>> vec;
  vec.reserve(hdr.nreloc);

  MachRel *rels = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++)
    vec.push_back(read_reloc(ctx, file, hdr, rels[i]));
  return vec;
}

template <>
void Subsection<X86_64>::scan_relocations(Context<X86_64> &ctx) {
  for (Relocation<X86_64> &r : get_rels()) {
    Symbol<X86_64> *sym = r.sym;
    if (!sym)
      continue;

    if (sym->is_imported && sym->file->is_dylib)
      ((DylibFile<X86_64> *)sym->file)->is_needed = true;

    switch (r.type) {
    case X86_64_RELOC_UNSIGNED:
      if (sym->is_imported) {
        if (r.p2size != 3) {
          Error(ctx) << this->isec << ": " << r << " relocation at offset 0x"
                     << std::hex << r.offset << " against symbol `"
                     << *sym << "' can not be used";
        }
        r.needs_dynrel = true;
      }
      break;
    case X86_64_RELOC_GOT:
    case X86_64_RELOC_GOT_LOAD:
      sym->flags |= NEEDS_GOT;
      break;
    case X86_64_RELOC_TLV:
      sym->flags |= NEEDS_THREAD_PTR;
      break;
    }

    if (sym->is_imported)
      sym->flags |= NEEDS_STUB;
  }
}

template <>
void Subsection<X86_64>::apply_reloc(Context<X86_64> &ctx, u8 *buf) {
  for (const Relocation<X86_64> &r : get_rels()) {
    if (r.sym && !r.sym->file) {
      Error(ctx) << "undefined symbol: " << isec.file << ": " << *r.sym;
      continue;
    }

    u64 val = r.addend;

    switch (r.type) {
    case X86_64_RELOC_UNSIGNED:
    case X86_64_RELOC_SIGNED:
    case X86_64_RELOC_BRANCH:
    case X86_64_RELOC_SIGNED_1:
    case X86_64_RELOC_SIGNED_2:
    case X86_64_RELOC_SIGNED_4:
      val += r.sym ? r.sym->get_addr(ctx) : r.subsec->get_addr(ctx);
      break;
    case X86_64_RELOC_GOT:
    case X86_64_RELOC_GOT_LOAD:
      val += r.sym->get_got_addr(ctx);
      break;
    case X86_64_RELOC_TLV:
      val += r.sym->get_tlv_addr(ctx);
      break;
    default:
      Fatal(ctx) << isec << ": unknown reloc: " << (int)r.type;
    }

    // An address of a thread-local variable is computed as an offset
    // to the beginning of the first thread-local section.
    if (isec.hdr.type == S_THREAD_LOCAL_VARIABLES)
      val -= ctx.tls_begin;

    if (r.is_pcrel)
      val -= get_addr(ctx) + r.offset + 4 + get_reloc_addend(r.type);

    if (r.p2size == 2)
      *(ul32 *)(buf + r.offset) = val;
    else if (r.p2size == 3)
      *(ul64 *)(buf + r.offset) = val;
    else
      unreachable();
  }
}

} // namespace mold::macho
