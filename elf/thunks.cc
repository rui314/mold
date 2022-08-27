// RISC instructions are usually up to 4 bytes long, so the immediates
// of their branch instructions are naturally smaller than 32 bits.
// This is contrary to x86-64 on which branch instructions take 4
// bytes immediates and can jump to anywhere within PC ± 2 GiB.
//
// In fact, ARM32's branch instructions can jump only within ±16 MiB
// and ARM64's ±128 MiB. If a branch target is further than that, we
// need to let it branch to a linker-synthesized code sequence that
// construct a full 32 bit address in a register and jump there. That
// linker-synthesized code is called "thunk".
//
// The function in this file creates thunks.

#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

template <typename E>
static void reset_thunk(RangeExtensionThunk<E> &thunk) {
  for (Symbol<E> *sym : thunk.symbols) {
    sym->extra.thunk_idx = -1;
    sym->extra.thunk_sym_idx = -1;
    sym->flags = 0;
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
    while (d < members.size() &&
           offset - members[b]->offset < E::thunk_max_distance) {
      offset = align_to(offset, 1 << members[d]->p2align);
      members[d]->offset = offset;
      offset += members[d]->sh_size;
      d++;
    }

    // Move C forward so that C is apart from B by GROUP_SIZE.
    while (c < members.size() &&
           members[c]->offset - members[b]->offset < E::thunk_group_size)
      c++;

    // Move A forward so that A is reachable from C.
    if (c > 0) {
      i64 c_end = members[c - 1]->offset + members[c - 1]->sh_size;
      while (a < osec.thunks.size() &&
             osec.thunks[a]->offset < c_end - E::thunk_max_distance)
        reset_thunk(*osec.thunks[a++]);
    }

    // Create a thunk for input sections between B and C and place it at D.
    osec.thunks.emplace_back(new RangeExtensionThunk<E>{osec});

    RangeExtensionThunk<E> &thunk = *osec.thunks.back();
    thunk.thunk_idx = osec.thunks.size() - 1;
    offset = align_to(offset, thunk.alignment);
    thunk.offset = offset;

    std::mutex mu;

    // Scan relocations between B and C to collect symbols that need thunks.
    tbb::parallel_for_each(members.begin() + b, members.begin() + c,
                           [&](InputSection<E> *isec) {
      std::span<const ElfRel<E>> rels = isec->get_rels(ctx);
      std::vector<RangeExtensionRef> &range_extn = isec->extra.range_extn;
      range_extn.resize(rels.size());

      for (i64 i = 0; i < rels.size(); i++) {
        const ElfRel<E> &rel = rels[i];
        if (!needs_thunk_rel(rel))
          continue;

        // Skip if the symbol is undefined. apply_reloc() will report an error.
        Symbol<E> &sym = *isec->file.symbols[rel.r_sym];
        if (!sym.file)
          continue;

        // Skip if the destination is within reach.
        if (is_reachable(ctx, sym, *isec, rel))
          continue;

        // If the symbol is already in another thunk, reuse it.
        if (sym.extra.thunk_idx != -1) {
          range_extn[i].thunk_idx = sym.extra.thunk_idx;
          range_extn[i].sym_idx = sym.extra.thunk_sym_idx;
          continue;
        }

        // Otherwise, add the symbol to this thunk if it's not added already.
        range_extn[i].thunk_idx = thunk.thunk_idx;
        range_extn[i].sym_idx = -1;

        if (sym.flags.exchange(-1) == 0) {
          std::scoped_lock lock(mu);
          thunk.symbols.push_back(&sym);
        }
      }
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
      std::span<const ElfRel<E>> rels = isec->get_rels(ctx);
      std::vector<RangeExtensionRef> &range_extn = isec->extra.range_extn;

      for (i64 i = 0; i < rels.size(); i++) {
        if (range_extn[i].thunk_idx == thunk.thunk_idx) {
          Symbol<E> &sym = *isec->file.symbols[rels[i].r_sym];
          range_extn[i].sym_idx = sym.extra.thunk_sym_idx;
        }
      }
    });

    // Move B forward to point to the begining of the next group.
    b = c;
  }

  while (a < osec.thunks.size())
    reset_thunk(*osec.thunks[a++]);

  osec.shdr.sh_size = offset;

  for (InputSection<E> *isec : members)
    osec.shdr.sh_addralign =
      std::max<u32>(osec.shdr.sh_addralign, 1 << isec->p2align);
}

template
void create_range_extension_thunks(Context<ARM64> &, OutputSection<ARM64> &);

template
void create_range_extension_thunks(Context<ARM32> &, OutputSection<ARM32> &);

template
void create_range_extension_thunks(Context<PPC64> &, OutputSection<PPC64> &);

} // namespace mold::elf
