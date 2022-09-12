#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold::macho {

using E = ARM64;

static u64 page(u64 val) {
  return val & 0xffff'ffff'ffff'f000;
}

static u64 page_offset(u64 hi, u64 lo) {
  u64 val = page(hi) - page(lo);
  return (bits(val, 13, 12) << 29) | (bits(val, 32, 14) << 5);
}

template <>
void StubsSection<E>::copy_buf(Context<E> &ctx) {
  ul32 *buf = (ul32 *)(ctx.buf + this->hdr.offset);

  for (i64 i = 0; i < syms.size(); i++) {
    static const ul32 insn[] = {
      0x90000010, // adrp x16, $ptr@PAGE
      0xf9400210, // ldr  x16, [x16, $ptr@PAGEOFF]
      0xd61f0200, // br   x16
    };

    static_assert(sizeof(insn) == E::stub_size);

    u64 la_addr = ctx.lazy_symbol_ptr.hdr.addr + word_size * i;
    u64 this_addr = this->hdr.addr + E::stub_size * i;

    memcpy(buf, insn, sizeof(insn));
    buf[0] |= page_offset(la_addr, this_addr);
    buf[1] |= bits(la_addr, 11, 3) << 10;
    buf += 3;
  }
}

template <>
void StubHelperSection<E>::copy_buf(Context<E> &ctx) {
  ul32 *start = (ul32 *)(ctx.buf + this->hdr.offset);
  ul32 *buf = start;

  static const ul32 insn0[] = {
    0x90000011, // adrp x17, $__dyld_private@PAGE
    0x91000231, // add  x17, x17, $__dyld_private@PAGEOFF
    0xa9bf47f0, // stp	x16, x17, [sp, #-16]!
    0x90000010, // adrp x16, $dyld_stub_binder@PAGE
    0xf9400210, // ldr  x16, [x16, $dyld_stub_binder@PAGEOFF]
    0xd61f0200, // br   x16
  };

  static_assert(sizeof(insn0) == E::stub_helper_hdr_size);
  memcpy(buf, insn0, sizeof(insn0));

  u64 dyld_private = get_symbol(ctx, "__dyld_private")->get_addr(ctx);
  buf[0] |= page_offset(dyld_private, this->hdr.addr);
  buf[1] |= bits(dyld_private, 11, 0) << 10;

  u64 stub_binder = get_symbol(ctx, "dyld_stub_binder")->get_got_addr(ctx);
  buf[3] |= page_offset(stub_binder, this->hdr.addr - 12);
  buf[4] |= bits(stub_binder, 11, 0) << 10;

  buf += 6;

  for (i64 i = 0; i < ctx.stubs.syms.size(); i++) {
    static const ul32 insn[] = {
      0x18000050, // ldr  w16, addr
      0x14000000, // b    stubHelperHeader
      0x00000000, // addr: .long <idx>
    };

    static_assert(sizeof(insn) == E::stub_helper_size);

    memcpy(buf, insn, sizeof(insn));
    buf[1] |= bits((start - buf - 1) * 4, 27, 2);
    buf[2] = ctx.stubs.bind_offsets[i];
    buf += 3;
  }
}

template <>
std::vector<Relocation<E>>
read_relocations(Context<E> &ctx, ObjectFile<E> &file, const MachSection &hdr) {
  std::vector<Relocation<E>> vec;
  vec.reserve(hdr.nreloc);

  MachRel *rels = (MachRel *)(file.mf->data + hdr.reloff);

  for (i64 i = 0; i < hdr.nreloc; i++) {
    i64 addend = 0;

    switch (rels[i].type) {
    case ARM64_RELOC_UNSIGNED:
    case ARM64_RELOC_SUBTRACTOR:
      switch (rels[i].p2size) {
      case 2:
        addend = *(il32 *)((u8 *)file.mf->data + hdr.offset + rels[i].offset);
        break;
      case 3:
        addend = *(il64 *)((u8 *)file.mf->data + hdr.offset + rels[i].offset);
        break;
      default:
        unreachable();
      }
      break;
    case ARM64_RELOC_ADDEND:
      addend = rels[i++].idx;
      break;
    }

    MachRel &r = rels[i];
    vec.push_back({r.offset, (u8)r.type, (u8)r.p2size});

    Relocation<E> &rel = vec.back();

    if (i > 0 && rels[i - 1].type == ARM64_RELOC_SUBTRACTOR)
      rel.is_subtracted = true;

    if (!rel.is_subtracted && rels[i].type != ARM64_RELOC_SUBTRACTOR)
      rel.is_pcrel = r.is_pcrel;

    if (r.is_extern) {
      rel.sym = file.syms[r.idx];
      rel.addend = addend;
      continue;
    }

    u64 addr = r.is_pcrel ? (hdr.addr + r.offset + addend) : addend;
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
    case ARM64_RELOC_UNSIGNED:
      if (sym->is_imported) {
        if (r.p2size != 3) {
          Error(ctx) << this->isec << ": " << r << " relocation at offset 0x"
                     << std::hex << r.offset << " against symbol `"
                     << *sym << "' can not be used";
        }
        r.needs_dynrel = true;
      }
      break;
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

    if (sym->is_imported || !sym->file)
      sym->flags |= NEEDS_STUB;
  }
}

template <>
void Subsection<E>::apply_reloc(Context<E> &ctx, u8 *buf) {
  std::span<Relocation<E>> rels = get_rels();

  for (i64 i = 0; i < rels.size(); i++) {
    Relocation<E> &r = rels[i];
    u8 *loc = buf + r.offset;
    i64 val = r.addend;
    u64 pc = get_addr(ctx) + r.offset;

    if (r.sym && !r.sym->file && ctx.arg.undefined == ERROR) {
      Error(ctx) << "undefined symbol: " << isec.file << ": " << *r.sym;
      continue;
    }

    // Compute a relocated value.
    switch (r.type) {
    case ARM64_RELOC_UNSIGNED:
    case ARM64_RELOC_BRANCH26:
    case ARM64_RELOC_PAGE21:
    case ARM64_RELOC_PAGEOFF12:
      val += r.sym ? r.sym->get_addr(ctx) : r.subsec->get_addr(ctx);
      break;
    case ARM64_RELOC_SUBTRACTOR: {
      Relocation<E> s = rels[++i];
      assert(s.type == ARM64_RELOC_UNSIGNED);
      u64 val1 = r.sym ? r.sym->get_addr(ctx) : r.subsec->get_addr(ctx);
      u64 val2 = s.sym ? s.sym->get_addr(ctx) : s.subsec->get_addr(ctx);
      val += val2 - val1;
      break;
    }
    case ARM64_RELOC_GOT_LOAD_PAGE21:
    case ARM64_RELOC_GOT_LOAD_PAGEOFF12:
    case ARM64_RELOC_POINTER_TO_GOT:
      val += r.sym->get_got_addr(ctx);
      break;
    case ARM64_RELOC_TLVP_LOAD_PAGE21:
    case ARM64_RELOC_TLVP_LOAD_PAGEOFF12:
      val += r.sym->get_tlv_addr(ctx);
      break;
    default:
      Fatal(ctx) << isec << ": unknown reloc: " << (int)r.type;
    }

    // An address of a thread-local variable is computed as an offset
    // to the beginning of the first thread-local section.
    if (isec.hdr.type == S_THREAD_LOCAL_VARIABLES)
      val -= ctx.tls_begin;

    // Write a computed value to the output buffer.
    switch (r.type) {
    case ARM64_RELOC_UNSIGNED:
    case ARM64_RELOC_SUBTRACTOR:
    case ARM64_RELOC_POINTER_TO_GOT:
      if (r.is_pcrel)
        val -= pc;

      if (r.p2size == 2)
        *(ul32 *)loc = val;
      else if (r.p2size == 3)
        *(ul64 *)loc = val;
      else
        unreachable();
      break;
    case ARM64_RELOC_BRANCH26: {
      assert(r.is_pcrel);
      val -= pc;

      i64 lo = -(1 << 27);
      i64 hi = 1 << 27;

      if (val < lo || hi <= val) {
        val = isec.osec.thunks[r.thunk_idx]->get_addr(r.thunk_sym_idx) - pc;
        assert(lo <= val && val < hi);
      }

      *(ul32 *)loc |= bits(val, 27, 2);
      break;
    }
    case ARM64_RELOC_PAGE21:
    case ARM64_RELOC_GOT_LOAD_PAGE21:
    case ARM64_RELOC_TLVP_LOAD_PAGE21:
      assert(r.is_pcrel);
      *(ul32 *)loc |= page_offset(val, pc);
      break;
    case ARM64_RELOC_PAGEOFF12:
    case ARM64_RELOC_GOT_LOAD_PAGEOFF12:
    case ARM64_RELOC_TLVP_LOAD_PAGEOFF12: {
      assert(!r.is_pcrel);
      u32 insn = *(ul32 *)loc;
      i64 scale = 0;
      if ((insn & 0x3b000000) == 0x39000000) {
        scale = bits(insn, 31, 30);
        if (scale == 0 && (insn & 0x04800000) == 0x04800000)
          scale = 4;
      }
      *(ul32 *)loc |= bits(val, 11, scale) << 10;
      break;
    }
    default:
      Fatal(ctx) << isec << ": unknown reloc: " << (int)r.type;
    }
  }
}

static void reset_thunk(RangeExtensionThunk<E> &thunk) {
  for (Symbol<E> *sym : thunk.symbols) {
    sym->thunk_idx = -1;
    sym->thunk_sym_idx = -1;
    sym->flags &= (u8)~NEEDS_RANGE_EXTN_THUNK;
  }
}

static bool is_reachable(Context<E> &ctx, Symbol<E> &sym,
                         Subsection<E> &subsec, Relocation<E> &rel) {
  // We pessimistically assume that PLT entries are unreacahble.
  if (sym.stub_idx != -1)
    return false;

  // We create thunks with a pessimistic assumption that all
  // out-of-section relocations would be out-of-range.
  if (!sym.subsec || &sym.subsec->isec.osec != &subsec.isec.osec)
    return false;

  if (sym.subsec->output_offset == -1)
    return false;

  // Compute a distance between the relocated place and the symbol
  // and check if they are within reach.
  i64 addr = sym.get_addr(ctx);
  i64 pc = subsec.get_addr(ctx) + rel.offset;
  i64 val = addr + rel.addend - pc;
  return -(1 << 27) <= val && val < (1 << 27);
}

// We create a thunk no further than 100 MiB from any section.
static constexpr i64 MAX_DISTANCE = 100 * 1024 * 1024;

// We create a thunk for each 10 MiB input sections.
static constexpr i64 GROUP_SIZE = 10 * 1024 * 1024;

// ARM64's call/jump instructions take 27 bits displacement, so they
// can refer only up to ±128 MiB. If a branch target is further than
// that, we need to let it branch to a linker-synthesized code
// sequence that construct a full 32 bit address in a register and
// jump there. That linker-synthesized code is called "thunk".
void create_range_extension_thunks(Context<E> &ctx, OutputSection<E> &osec) {
  std::span<Subsection<E> *> members = osec.members;
  if (members.empty())
    return;

  members[0]->output_offset = 0;

  // Initialize input sections with a dummy offset so that we can
  // distinguish sections that have got an address with the one who
  // haven't.
  tbb::parallel_for((i64)1, (i64)members.size(), [&](i64 i) {
    members[i]->output_offset = -1;
  });

  // We create thunks from the beginning of the section to the end.
  // We manage progress using four offsets which increase monotonically.
  // The locations they point to are always A <= B <= C <= D.
  i64 a = 0;
  i64 b = 0;
  i64 c = 0;
  i64 d = 0;
  i64 offset = 0;

  while (b < members.size()) {
    // Move D foward as far as we can jump from B to D.
    while (d < members.size() &&
           offset - members[b]->output_offset < MAX_DISTANCE) {
      offset = align_to(offset, 1 << members[d]->p2align);
      members[d]->output_offset = offset;
      offset += members[d]->input_size;
      d++;
    }

    // Move C forward so that C is apart from B by GROUP_SIZE.
    while (c < members.size() &&
           members[c]->output_offset - members[b]->output_offset < GROUP_SIZE)
      c++;

    // Move A forward so that A is reachable from C.
    if (c > 0) {
      i64 c_end = members[c - 1]->output_offset + members[c - 1]->input_size;
      while (a < osec.thunks.size() &&
             osec.thunks[a]->offset < c_end - MAX_DISTANCE)
        reset_thunk(*osec.thunks[a++]);
    }

    // Create a thunk for input sections between B and C and place it at D.
    osec.thunks.emplace_back(new RangeExtensionThunk<E>{osec});

    RangeExtensionThunk<E> &thunk = *osec.thunks.back();
    thunk.thunk_idx = osec.thunks.size() - 1;
    thunk.offset = offset;

    // Scan relocations between B and C to collect symbols that need thunks.
    tbb::parallel_for_each(members.begin() + b, members.begin() + c,
                           [&](Subsection<E> *subsec) {
      std::span<Relocation<E>> rels = subsec->get_rels();

      for (i64 i = 0; i < rels.size(); i++) {
        Relocation<E> &r = rels[i];
        if (!r.sym->file || r.type != ARM64_RELOC_BRANCH26)
          continue;

        // Skip if the destination is within reach.
        if (is_reachable(ctx, *r.sym, *subsec, r))
          continue;

        // If the symbol is already in another thunk, reuse it.
        if (r.sym->thunk_idx != -1) {
          r.thunk_idx = r.sym->thunk_idx;
          r.thunk_sym_idx = r.sym->thunk_sym_idx;
          continue;
        }

        // Otherwise, add the symbol to this thunk if it's not added already.
        r.thunk_idx = thunk.thunk_idx;
        r.thunk_sym_idx = -1;

        if (!(r.sym->flags.fetch_or(NEEDS_RANGE_EXTN_THUNK) &
              NEEDS_RANGE_EXTN_THUNK)) {
          std::scoped_lock lock(thunk.mu);
          thunk.symbols.push_back(r.sym);
        }
      }
    });

    // Now that we know the number of symbols in the thunk, we can compute
    // its size.
    offset += thunk.size();

    // Sort symbols added to the thunk to make the output deterministic.
    sort(thunk.symbols, [](Symbol<E> *a, Symbol<E> *b) {
      return std::tuple{a->file->priority, a->value} <
             std::tuple{b->file->priority, b->value};
    });

    // Assign offsets within the thunk to the symbols.
    for (i64 i = 0; Symbol<E> *sym : thunk.symbols) {
      sym->thunk_idx = thunk.thunk_idx;
      sym->thunk_sym_idx = i++;
    }

    // Scan relocations again to fix symbol offsets in the last thunk.
    tbb::parallel_for_each(members.begin() + b, members.begin() + c,
                           [&](Subsection<E> *subsec) {
      for (Relocation<E> &r : subsec->get_rels())
        if (r.thunk_idx == thunk.thunk_idx)
          r.thunk_sym_idx = r.sym->thunk_sym_idx;
    });

    // Move B forward to point to the begining of the next group.
    b = c;
  }

  while (a < osec.thunks.size())
    reset_thunk(*osec.thunks[a++]);

  osec.hdr.size = offset;
}

void RangeExtensionThunk<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + output_section.hdr.offset + offset;

  static const ul32 data[] = {
    0x90000010, // adrp x16, 0   # R_AARCH64_ADR_PREL_PG_HI21
    0x91000210, // add  x16, x16 # R_AARCH64_ADD_ABS_LO12_NC
    0xd61f0200, // br   x16
  };

  static_assert(ENTRY_SIZE == sizeof(data));

  for (i64 i = 0; i < symbols.size(); i++) {
    u64 addr = symbols[i]->get_addr(ctx);
    u64 pc = output_section.hdr.addr + offset + i * ENTRY_SIZE;

    u8 *loc = buf + i * ENTRY_SIZE;
    memcpy(loc , data, sizeof(data));
    *(ul32 *)loc |= page_offset(addr, pc);
    *(ul32 *)(loc + 4) |= bits(addr, 11, 0) << 10;
  }
}

#define ASSERT_RANGE(val, start, size)                          \
  assert((start) <= (val) && (val) < ((start) + (size)))

// Rewrite
//   ldr Xb/Wb, [Xa, #imm]
// with
//   ldr Xb/Wb, [#base_addr + #imm]
// if we can.
static bool relax_ldr(ul32 *loc, u64 loc_addr, u64 base_addr) {
  switch (*loc & 0xffc0'0000) {
  case 0xf940'0000: { // ldr Xb, [Xa, #imm]
    u64 imm = bits(*loc, 21, 10) * 8;
    i64 disp = base_addr + imm - loc_addr;
    if (disp == sign_extend(disp, 20) && (disp & 0b11) == 0) {
      // ldr Xc, [#base_addr + #imm]
      *loc = 0x5800'0000 | (bits(disp, 20, 2) << 5) | bits(*loc, 4, 0);
      return true;
    }
    return false;
  }
  case 0xb940'0000: { // ldr Wb, [Xa, #imm]
    u64 imm = bits(*loc, 21, 10) * 4;
    i64 disp = base_addr + imm - loc_addr;
    if (disp == sign_extend(disp, 20) && (disp & 0b11) == 0) {
      // ldr Wc, [#base_addr + #imm]
      *loc = 0x1800'0000 | (bits(disp, 20, 2) << 5) | bits(*loc, 4, 0);
      return true;
    }
    return false;
  }
  }
  return false;
}

static bool is_adrp(ul32 *loc) {
  return (*loc & 0x9f00'0000) == 0x9000'0000;
}

static u64 get_adrp_imm(ul32 *loc) {
  return (bits(*loc, 23, 5) << 14) + (bits(*loc, 30, 29) << 12);
}

template <typename E>
static void relax_adrp_ldr_got_ldr(Context<E> &ctx,
                                   ul32 *loc1, ul32 *loc2, ul32 *loc3,
                                   u64 addr1, u64 addr2, u64 addr3,
                                   ObjectFile<E> *file) {
  // We expect the following instructions for a GOT-indirect load:
  //
  //   adrp Xa, _foo@GOTPAGE
  //   ldr  Xb, [Xa, _foo@GOTPAGEOFF]
  //   ldr/ldrb/ldrsb/ldrsh/... Xc/Wc, [Xb, #imm]
  if (!is_adrp(loc1) || (*loc2 & 0xffc0'0000) != 0xf940'0000)
    return;

  u64 got_page = page(addr1) + get_adrp_imm(loc1);
  u64 pageoff = bits(*loc2, 21, 10) << 3;

  if (got_page + pageoff < ctx.got.hdr.addr ||
      ctx.got.hdr.addr + ctx.got.hdr.size <= got_page + pageoff)
    Fatal(ctx) << *file << ": LDR_GOT_LDR out of range";

  ASSERT_RANGE(got_page + pageoff, ctx.got.hdr.addr, ctx.got.hdr.size);

  u64 got_value = *(ul64 *)(ctx.buf + ctx.got.hdr.offset + got_page + pageoff -
                            ctx.got.hdr.addr);

  // If the GOT slot is already filled with a value and we can
  // inline the value, do it.
  if (got_value && relax_ldr(loc3, addr3, got_value)) {
    *loc1 = 0xd503'201f; // nop
    *loc2 = 0xd503'201f; // nop
    return;
  }

  // If the GOT slot is close enough to PC, we can eliminate ADRP.
  if (relax_ldr(loc2, addr2, got_page))
    *loc1 = 0xd503'201f; // nop
}

static void relax_adrp_adrp(Context<E> &ctx, ul32 *loc1, ul32 *loc2,
                            u64 addr1, u64 addr2) {
  //   adrp Xa, _foo@PAGE
  //   adrp Xa, _bar@PAGE
  // ->
  //   ldr  Xa, _foo@PAGE
  //   nop

  if (is_adrp(loc1) && is_adrp(loc2) &&
      page(addr1) + get_adrp_imm(loc1) == page(addr2) + get_adrp_imm(loc2) &&
      bits(*loc1, 4, 0) == bits(*loc2, 4, 0))
    *loc2 = 0xd503'201f; // nop
}

static void relax_adrp_ldr(Context<E> &ctx, ul32 *loc1, ul32 *loc2,
                           u64 addr1, u64 addr2) {
  //   adrp Xa, _foo@PAGE
  //   ldr  Xb/Wb, [Xa, _foo@PAGEOFF]
  // ->
  //   nop
  //   ldr  Xb/Wb, [_foo]
  if (is_adrp(loc1) && relax_ldr(loc2, addr2, page(addr1) + get_adrp_imm(loc1)))
    *loc1 = 0xd503'201f; // nop
}

static void relax_adrp_add(Context<E> &ctx, ul32 *loc1, ul32 *loc2,
                           u64 addr1, u64 addr2) {
  //   adrp Xa, _foo@PAGE
  //   add  Xb, Xa, _foo@PAGEOFF
  // ->
  //   nop
  //   adr  Xb, _foo
  if (!is_adrp(loc1) || (*loc2 & 0xffc0'0000) != 0x9100'0000)
    return;

  i64 disp = page(addr1) + get_adrp_imm(loc1) + bits(*loc2, 21, 10) - addr2;

  if (disp == sign_extend(disp, 20)) {
    *loc1 = 0xd503'201f;                                  // nop
    *loc2 = 0x1000'0000 | (bits(disp, 1, 0) << 29) |
            (bits(disp, 20, 2) << 5) | bits(*loc2, 4, 0); // adr Xb, _foo
  }
}

static Subsection<E> *find_subsection_by_addr(ObjectFile<E> *file, u32 addr) {
  for (Subsection<E> *subsec : file->subsections)
    if (subsec->input_addr <= addr &&
        addr < subsec->input_addr + subsec->input_size)
      return subsec;
  return nullptr;
}

// On ARM, we generally need two or more instructions to materialize
// an address of an object in a register or jump to a function.
// However, if an object or a function is close enough to PC, a single
// instruction suffices.
//
// This function replaces such redundant instruction sequences with
// shorter instruction sequences. We pad holes with NOPs, so the total
// number of instructions won't change by this relaxation, but replacing
// instructions with NOPs generally makes program faster since the CPU
// has a special logic to skip NOPs as quickly as possible.
//
// Locations of relaxable instructions are in the
// LC_LINKER_OPTIMIZATION_HINT segment. That segment contains a
// sequence of ULEB-encoded integers.
//
// This pass is optional; an output file works correctly without
// applying these optimizations.
void apply_linker_optimization_hints(Context<E> &ctx) {
  Timer t(ctx, "apply_linker_optimization_hints");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::string_view hints = file->get_linker_optimization_hints(ctx);

    while (!hints.empty()) {
      i64 type = read_uleb(hints);
      if (type == 0)
        return;

      i64 nargs = read_uleb(hints);

      switch (type) {
      case LOH_ARM64_ADRP_LDR_GOT_LDR: {
        assert(nargs == 3);
        i64 input_addr1 = read_uleb(hints);
        i64 input_addr2 = read_uleb(hints);
        i64 input_addr3 = read_uleb(hints);

        Subsection<E> *subsec = find_subsection_by_addr(file, input_addr1);
        if (!subsec || !subsec->is_alive)
          return;

        ASSERT_RANGE(input_addr1, subsec->input_addr, subsec->input_size);
        ASSERT_RANGE(input_addr2, subsec->input_addr, subsec->input_size);
        ASSERT_RANGE(input_addr3, subsec->input_addr, subsec->input_size);

        i64 offset1 = subsec->input_addr - input_addr1;
        i64 offset2 = subsec->input_addr - input_addr2;
        i64 offset3 = subsec->input_addr - input_addr3;

        u8 *loc = ctx.buf + subsec->isec.osec.hdr.offset + subsec->output_offset;
        ul32 *loc1 = (ul32 *)(loc + offset1);
        ul32 *loc2 = (ul32 *)(loc + offset2);
        ul32 *loc3 = (ul32 *)(loc + offset3);

        u64 addr1 = subsec->get_addr(ctx) + offset1;
        u64 addr2 = subsec->get_addr(ctx) + offset2;
        u64 addr3 = subsec->get_addr(ctx) + offset3;

        relax_adrp_ldr_got_ldr(ctx, loc1, loc2, loc3, addr1, addr2, addr3,
                               file);
        break;
      }
      case LOH_ARM64_ADRP_ADRP:
      case LOH_ARM64_ADRP_LDR:
      case LOH_ARM64_ADRP_ADD: {
        assert(nargs == 2);
        i64 input_addr1 = read_uleb(hints);
        i64 input_addr2 = read_uleb(hints);

        Subsection<E> *subsec = find_subsection_by_addr(file, input_addr1);
        if (!subsec || !subsec->is_alive)
          return;

        ASSERT_RANGE(input_addr1, subsec->input_addr, subsec->input_size);
        ASSERT_RANGE(input_addr2, subsec->input_addr, subsec->input_size);

        i64 offset1 = subsec->input_addr - input_addr1;
        i64 offset2 = subsec->input_addr - input_addr2;

        u8 *loc = ctx.buf + subsec->isec.osec.hdr.offset + subsec->output_offset;
        ul32 *loc1 = (ul32 *)(loc + offset1);
        ul32 *loc2 = (ul32 *)(loc + offset2);

        u64 addr1 = subsec->get_addr(ctx) + offset1;
        u64 addr2 = subsec->get_addr(ctx) + offset2;

        switch (type) {
        case LOH_ARM64_ADRP_ADRP:
          relax_adrp_adrp(ctx, loc1, loc2, addr1, addr2);
          break;
        case LOH_ARM64_ADRP_LDR:
          relax_adrp_ldr(ctx, loc1, loc2, addr1, addr2);
          break;
        case LOH_ARM64_ADRP_ADD:
          relax_adrp_add(ctx, loc1, loc2, addr1, addr2);
          break;
        }
        break;
      }
      default:
        // Skip unsupported optimizations hints.
        for (i64 i = 0; i < nargs; i++)
          read_uleb(hints);
      }
    }
  });
}

} // namespace mold::macho
