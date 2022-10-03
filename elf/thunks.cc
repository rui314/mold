// RISC instructions are usually up to 4 bytes long, so the immediates
// of their branch instructions are naturally smaller than 32 bits.
// This is contrary to x86-64 on which branch instructions take 4
// bytes immediates and can jump to anywhere within PC ± 2 GiB.
//
// In fact, ARM32's branch instructions can jump only within ±16 MiB
// and ARM64's ±128 MiB, for example. If a branch target is further
// than that, we need to let it branch to a linker-synthesized code
// sequence that construct a full 32 bit address in a register and
// jump there. That linker-synthesized code is called "thunk".
//
// The function in this file creates thunks.

#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

// The number of immediate bits in branch instructions.
//
// ARM64's branch has 26 bits immediate, and it's scaled by 4 because all
// instructions are 4 bytes aligned, so it's effectively 28 bits long.
//
// ARM32's Thumb branch has 24 bits immediate, and the instructions are
// aligned to 2, so it's effectively 25 bits. ARM32's non-Thumb branches
// have twice longer range than its Thumb counterparts, but we
// conservatively use the Thumb's limitation.
//
// PPC64's branch has 24 bits immediate, and the instructions are aligned
// to 4, therefore 26.
//
// Here is the summary of branch instructions reaches:
//
//   ARM64: PC ± 128 MiB
//   ARM32: PC ± 16 MiB
//   PPC64: PC ± 32 MiB
template <typename E>
static constexpr i64 jump_bits =
  std::is_same_v<E, ARM64> ? 28 : std::is_same_v<E, ARM32> ? 25 : 26;

// We redirect a branch to a thunk if its destination is further than
// this number.
//
// 5 MiB is a safety margin; we assume that there's no crazy big input
// .text section that is larger than 5 MiB.
template <typename E>
static constexpr i64 max_distance = (1LL << (jump_bits<E> - 1)) - 5 * 1024 * 1024;

// We create thunks for each 12.8/1.6/3.2 MiB code block for
// ARM64/ARM32/PPC64, respectively.
template <typename E>
static constexpr i64 group_size = (1LL << (jump_bits<E> - 1)) / 10;

template <typename E>
static bool needs_thunk_rel(const ElfRel<E> &r) {
  u32 ty = r.r_type;

  if constexpr (std::is_same_v<E, ARM64>) {
    return ty == R_AARCH64_JUMP26 || ty == R_AARCH64_CALL26;
  } else if constexpr (std::is_same_v<E, ARM32>) {
    return ty == R_ARM_JUMP24 || ty == R_ARM_THM_JUMP24 ||
           ty == R_ARM_CALL   || ty == R_ARM_THM_CALL;
  } else {
    static_assert(std::is_same_v<E, PPC64V2>);
    return ty == R_PPC64_REL24;
  }
}

template <typename E>
static bool is_reachable(Context<E> &ctx, InputSection<E> &isec,
                         Symbol<E> &sym, const ElfRel<E> &rel) {
  // We create thunks with a pessimistic assumption that all
  // out-of-section relocations would be out-of-range.
  InputSection<E> *isec2 = sym.get_input_section();
  if (!isec2 || isec.output_section != isec2->output_section)
    return false;

  // Even if the target is the same section, we branch to its PLT
  // if it has one. So a symbol with a PLT is also considered an
  // out-of-section reference.
  if (sym.has_plt(ctx))
    return false;

  // If the target section is in the same output section but
  // hasn't got any address yet, that's unreacahble.
  if (isec2->offset == -1)
    return false;

  // Thumb and ARM B instructions cannot be converted to BX, so we
  // always have to make them jump to a thunk to switch processor mode
  // even if their destinations are within their ranges.
  if constexpr (std::is_same_v<E, ARM32>) {
    bool is_thumb = sym.get_addr(ctx) & 1;
    if ((rel.r_type == R_ARM_THM_JUMP24 && !is_thumb) ||
        (rel.r_type == R_ARM_JUMP24 && is_thumb))
      return false;
  }

  // Compute a distance between the relocated place and the symbol
  // and check if they are within reach.
  i64 S = sym.get_addr(ctx);
  i64 A = isec.get_addend(rel);
  i64 P = isec.get_addr() + rel.r_offset;
  u64 val = S + A - P;
  return sign_extend(val, jump_bits<E> - 1) == val;
}

template <typename E>
static void reset_thunk(RangeExtensionThunk<E> &thunk) {
  for (Symbol<E> *sym : thunk.symbols) {
    sym->extra.thunk_idx = -1;
    sym->extra.thunk_sym_idx = -1;
    sym->flags = 0;
  }
}

// Scan relocations to collect symbols that need thunks.
template <typename E>
static void scan_rels(Context<E> &ctx, InputSection<E> &isec,
                      RangeExtensionThunk<E> &thunk) {
  std::span<const ElfRel<E>> rels = isec.get_rels(ctx);
  std::vector<RangeExtensionRef> &range_extn = isec.extra.range_extn;
  range_extn.resize(rels.size());

  for (i64 i = 0; i < rels.size(); i++) {
    const ElfRel<E> &rel = rels[i];
    if (!needs_thunk_rel(rel))
      continue;

    // Skip if the symbol is undefined. apply_reloc() will report an error.
    Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
    if (!sym.file)
      continue;

    // Skip if the destination is within reach.
    if (is_reachable(ctx, isec, sym, rel))
      continue;

    // This relocation needs a thunk. If the symbol is already in a
    // previous thunk, reuse it.
    if (sym.extra.thunk_idx != -1) {
      range_extn[i].thunk_idx = sym.extra.thunk_idx;
      range_extn[i].sym_idx = sym.extra.thunk_sym_idx;
      continue;
    }

    // Otherwise, add the symbol to the current thunk if it's not
    // added already.
    range_extn[i].thunk_idx = thunk.thunk_idx;
    range_extn[i].sym_idx = -1;

    if (sym.flags.exchange(-1) == 0) {
      std::scoped_lock lock(thunk.mu);
      thunk.symbols.push_back(&sym);
    }
  }
}

template <typename E>
void create_range_extension_thunks(Context<E> &ctx, OutputSection<E> &osec) {
  std::span<InputSection<E> *> members = osec.members;
  if (members.empty())
    return;

  members[0]->offset = 0;

  // Initialize input sections with a dummy offset so that we can
  // distinguish sections that have got an address with the one who
  // haven't.
  tbb::parallel_for((i64)1, (i64)members.size(), [&](i64 i) {
    members[i]->offset = -1;
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
    while (d < members.size() && offset - members[b]->offset < max_distance<E>) {
      offset = align_to(offset, 1 << members[d]->p2align);
      members[d]->offset = offset;
      offset += members[d]->sh_size;
      d++;
    }

    // Move C forward so that C is apart from B by GROUP_SIZE.
    while (c < members.size() &&
           members[c]->offset - members[b]->offset < group_size<E>)
      c++;

    // Move A forward so that A is reachable from C.
    if (c > 0) {
      i64 c_end = members[c - 1]->offset + members[c - 1]->sh_size;
      while (a < osec.thunks.size() &&
             osec.thunks[a]->offset < c_end - max_distance<E>)
        reset_thunk(*osec.thunks[a++]);
    }

    // Create a thunk for input sections between B and C and place it at D.
    osec.thunks.emplace_back(new RangeExtensionThunk<E>{osec});

    RangeExtensionThunk<E> &thunk = *osec.thunks.back();
    thunk.thunk_idx = osec.thunks.size() - 1;
    offset = align_to(offset, thunk.alignment);
    thunk.offset = offset;

    // Scan relocations between B and C to collect symbols that need thunks.
    tbb::parallel_for_each(members.begin() + b, members.begin() + c,
                           [&](InputSection<E> *isec) {
      scan_rels(ctx, *isec, thunk);
    });

    // Now that we know the number of symbols in the thunk, we can compute
    // its size.
    offset += thunk.size();

    // Sort symbols added to the thunk to make the output deterministic.
    sort(thunk.symbols, [](Symbol<E> *a, Symbol<E> *b) {
      return std::tuple{a->file->priority, a->sym_idx} <
             std::tuple{b->file->priority, b->sym_idx};
    });

    // Assign offsets within the thunk to the symbols.
    for (i64 i = 0; Symbol<E> *sym : thunk.symbols) {
      sym->extra.thunk_idx = thunk.thunk_idx;
      sym->extra.thunk_sym_idx = i++;
    }

    // Scan relocations again to fix symbol offsets in the last thunk.
    tbb::parallel_for_each(members.begin() + b, members.begin() + c,
                           [&](InputSection<E> *isec) {
      std::span<Symbol<E> *> syms = isec->file.symbols;
      std::span<const ElfRel<E>> rels = isec->get_rels(ctx);
      std::span<RangeExtensionRef> range_extn = isec->extra.range_extn;

      for (i64 i = 0; i < rels.size(); i++)
        if (range_extn[i].thunk_idx == thunk.thunk_idx)
          range_extn[i].sym_idx = syms[rels[i].r_sym]->extra.thunk_sym_idx;
    });

    // Move B forward to point to the begining of the next group.
    b = c;
  }

  while (a < osec.thunks.size())
    reset_thunk(*osec.thunks[a++]);

  osec.shdr.sh_size = offset;
}

#if defined(MOLD_ARM64) || defined(MOLD_ARM32) || defined(MOLD_PPC64V2)
using E = MOLD_TARGET;
template void create_range_extension_thunks(Context<E> &, OutputSection<E> &);
#endif

} // namespace mold::elf
