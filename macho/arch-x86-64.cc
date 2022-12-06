#include "mold.h"

namespace mold::macho {

using E = X86_64;

template <>
void StubsSection<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->hdr.offset;

  for (i64 i = 0; i < syms.size(); i++) {
    // `ff 25 xx xx xx xx` is a RIP-relative indirect jump instruction,
    // i.e., `jmp *IMM(%rip)`. It loads an address from la_symbol_ptr
    // and jump there.
    static_assert(E::stub_size == 6);
    buf[i * 6] = 0xff;
    buf[i * 6 + 1] = 0x25;
    *(ul32 *)(buf + i * 6 + 2) =
      ctx.lazy_symbol_ptr.hdr.addr + i * word_size - (this->hdr.addr + i * 6 + 6);
  }
}

template <>
void StubHelperSection<E>::copy_buf(Context<E> &ctx) {
  u8 *start = ctx.buf + this->hdr.offset;
  u8 *buf = start;

  static const u8 insn0[] = {
    0x4c, 0x8d, 0x1d, 0, 0, 0, 0, // lea $__dyld_private(%rip), %r11
    0x41, 0x53,                   // push %r11
    0xff, 0x25, 0, 0, 0, 0,       // jmp *$dyld_stub_binder@GOT(%rip)
    0x90,                         // nop
  };

  static_assert(sizeof(insn0) == E::stub_helper_hdr_size);

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

    static_assert(sizeof(insn) == E::stub_helper_size);

    memcpy(buf, insn, sizeof(insn));
    *(ul32 *)(buf + 1) = ctx.stubs.bind_offsets[i];
    *(ul32 *)(buf + 6) = start - buf - 10;
    buf += 10;
  }
}

template <>
void ObjcStubsSection<E>::copy_buf(Context<E> &ctx) {}

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

template <>
std::vector<Relocation<E>>
read_relocations(Context<E> &ctx, ObjectFile<E> &file,
                 const MachSection &hdr) {
  std::vector<Relocation<E>> vec;
  vec.reserve(hdr.nreloc);

  MachRel *rels = (MachRel *)(file.mf->data + hdr.reloff);

  for (i64 i = 0; i < hdr.nreloc; i++) {
    MachRel &r = rels[i];
    i64 addend = read_addend((u8 *)file.mf->data + hdr.offset, r);

    vec.push_back({r.offset, (u8)r.type, (u8)r.p2size});
    Relocation<E> &rel = vec.back();

    if (i > 0 && rels[i - 1].type == X86_64_RELOC_SUBTRACTOR)
      rel.is_subtracted = true;

    if (!rel.is_subtracted && rels[i].type != X86_64_RELOC_SUBTRACTOR)
      rel.is_pcrel = r.is_pcrel;

    if (r.is_extern) {
      rel.sym = file.syms[r.idx];
      rel.addend = addend;
      continue;
    }

    u64 addr = r.is_pcrel ? (hdr.addr + r.offset + addend + 4) : addend;
    Subsection<E> *target = file.find_subsection(ctx, r.idx - 1, addr);
    if (!target)
      Fatal(ctx) << file << ": bad relocation: " << r.offset;

    rel.subsec = target;
    rel.addend = addr - target->input_addr;
  }

  return vec;
}

template <>
void Subsection<E>::scan_relocations(Context<E> &ctx) {
  for (Relocation<E> &r : get_rels()) {
    Symbol<E> *sym = r.sym;
    if (!sym)
      continue;

    if (sym->is_imported && sym->file->is_dylib)
      ((DylibFile<E> *)sym->file)->is_alive = true;

    switch (r.type) {
    case X86_64_RELOC_UNSIGNED:
    case X86_64_RELOC_SUBTRACTOR:
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
void Subsection<E>::apply_reloc(Context<E> &ctx, u8 *buf) {
  std::span<Relocation<E>> rels = get_rels();

  for (i64 i = 0; i < rels.size(); i++) {
    Relocation<E> &r = rels[i];
    u64 val = r.addend;

    if (r.sym && !r.sym->file) {
      Error(ctx) << "undefined symbol: " << isec.file << ": " << *r.sym;
      continue;
    }

    switch (r.type) {
    case X86_64_RELOC_UNSIGNED:
    case X86_64_RELOC_SIGNED:
    case X86_64_RELOC_BRANCH:
    case X86_64_RELOC_SIGNED_1:
    case X86_64_RELOC_SIGNED_2:
    case X86_64_RELOC_SIGNED_4:
      val += r.sym ? r.sym->get_addr(ctx) : r.subsec->get_addr(ctx);
      break;
    case X86_64_RELOC_SUBTRACTOR: {
      Relocation<E> s = rels[++i];
      assert(s.type == X86_64_RELOC_UNSIGNED);
      assert(r.p2size == s.p2size);
      u64 val1 = r.sym ? r.sym->get_addr(ctx) : r.subsec->get_addr(ctx);
      u64 val2 = s.sym ? s.sym->get_addr(ctx) : s.subsec->get_addr(ctx);
      val += val2 - val1;
      break;
    }
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

    if (isec.hdr.type == S_THREAD_LOCAL_VARIABLES) {
      // An address of a thread-local variable is computed as an offset
      // to the beginning of the first thread-local section.
      val -= ctx.tls_begin;
    } else if (r.is_pcrel) {
      val -= get_addr(ctx) + r.offset + 4 + get_reloc_addend(r.type);
    }

    if (r.p2size == 2)
      *(ul32 *)(buf + r.offset) = val;
    else if (r.p2size == 3)
      *(ul64 *)(buf + r.offset) = val;
    else
      unreachable();
  }
}

} // namespace mold::macho
