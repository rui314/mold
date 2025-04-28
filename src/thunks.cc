// RISC instructions are usually up to 4 bytes long, so the immediates of
// their branch instructions are naturally smaller than 32 bits.  This is
// contrary to x86-64 on which branch instructions take 4 bytes immediates
// and can jump to anywhere within PC ± 2 GiB.
//
// In fact, ARM32's branch instructions can jump only within ±16 MiB and
// ARM64's ±128 MiB, for example. If a branch target is further than that,
// we need to let it branch to a linker-synthesized code sequence that
// construct a full 32 bit address in a register and jump there. That
// linker-synthesized code is called "thunk".
//
// The function in this file creates thunks.
//
// Note that although thunks play an important role in an executable, they
// don't take up too much space in it. For example, among the clang-16's
// text segment whose size is ~300 MiB on ARM64, thunks in total occupy
// only ~30 KiB or 0.01%. Of course the number depends on an ISA; we would
// need more thunks on ARM32 whose branch range is shorter than ARM64.
// That said, the total size of thunks still isn't that much. Therefore,
// we don't need to try too hard to reduce thunk size to the absolute
// minimum.

#if MOLD_ARM32LE || MOLD_ARM32BE || MOLD_ARM64LE || MOLD_ARM64BE || \
    MOLD_PPC32 || MOLD_PPC64V1 || MOLD_PPC64V2

#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

namespace mold {

using E = MOLD_TARGET;

// We create thunks for each 25.6/3.2/6.4 MiB code block for
// ARM64/ARM32/PPC, respectively.
static constexpr i64 batch_size = branch_distance<E> / 5;

// We assume that a single thunk group is smaller than 1 MiB.
static constexpr i64 max_thunk_size = 1024 * 1024;

// We align thunks to 16 byte boundaries because many processor vendors
// recommend we align branch targets to 16 byte boundaries for performance
// reasons.
static constexpr i64 thunk_align = 16;

template <typename E>
static bool is_reachable(Context<E> &ctx, bool first_pass, InputSection<E> &isec,
                         Symbol<E> &sym, const ElfRel<E> &rel) {
  // If the target section is in the same output section but
  // hasn't got any address yet, that's unreacahble.
  InputSection<E> *isec2 = sym.get_input_section();
  if (isec2 && isec.output_section == isec2->output_section &&
      isec2->offset == -1)
    return false;

  // We don't know about the final file layout on the first pass, so
  // we assume pessimistically that all out-of-section relocations are
  // out-of-range. Excessive thunks will be removed on the second pass.
  if (first_pass) {
    if (!isec2 || isec.output_section != isec2->output_section)
      return false;

    // Even if the target is the same section, we branch to its PLT
    // if it has one. So a symbol with a PLT is also considered an
    // out-of-section reference.
    if (sym.has_plt(ctx))
      return false;
  }

  // Compute a distance between the relocated place and the symbol
  // and check if they are within reach.
  i64 S = sym.get_addr(ctx, NO_OPD);
  i64 A = get_addend(isec, rel);
  i64 P = isec.get_addr() + rel.r_offset;
  i64 val = S + A - P;
  return -branch_distance<E> <= val && val < branch_distance<E>;
}

template <typename E>
static bool needs_shim(Context<E> &ctx, Symbol<E> &sym, const ElfRel<E> &rel) {
  // Thumb and ARM B instructions cannot be converted to BX, so we
  // always have to make them jump to a thunk to switch processor mode
  // even if their destinations are within their ranges.
  if constexpr (is_arm32<E>) {
    bool is_thumb = sym.get_addr(ctx) & 1;
    return (rel.r_type == R_ARM_THM_JUMP24 && !is_thumb) ||
           (rel.r_type == R_ARM_JUMP24 && is_thumb) ||
           (rel.r_type == R_ARM_PLT32 && is_thumb);
  }

  // On PowerPC, all PLT calls go through range extension thunks.
  if constexpr (is_ppc32<E> || is_ppc64v1<E>)
    return sym.has_plt(ctx);

  // PowerPC before Power9 lacks PC-relative load/store instructions.
  // Functions compiled for Power9 or earlier assume that r2 points to
  // GOT+0x8000, while those for Power10 uses r2 as a scratch register.
  // We need to a thunk to recompute r2 for interworking.
  if constexpr (is_ppc64v2<E>)
    return sym.has_plt(ctx) ||
           (rel.r_type == R_PPC64_REL24 && !sym.esym().ppc64_preserves_r2()) ||
           (rel.r_type == R_PPC64_REL24_NOTOC && sym.esym().ppc64_uses_toc());
  return false;
}

template <>
void OutputSection<E>::create_range_extension_thunks(Context<E> &ctx,
                                                     bool first_pass) {
  std::span<InputSection<E> *> m = members;
  if (m.empty())
    return;

  // Initialize input sections with a dummy offset so that we can
  // distinguish sections that have got an address with the one who
  // haven't.
  for (InputSection<E> *isec : m)
    isec->offset = -1;
  thunks.clear();

  // We create thunks from the beginning of the section to the end.
  // We manage progress using four offsets which increase monotonically.
  // The locations they point to are always A <= B <= C <= D.
  //
  // Input sections between B and C are in the current batch.
  //
  // A is the input section with the smallest address than can reach
  // from the current batch.
  //
  // D is the input section with the largest address such that the thunk
  // is reachable from the current batch if it's inserted at D.
  //
  //  ................................ <input sections> ............
  //     A    B    C    D
  //                    ^ We insert a thunk for the current batch just before D
  //          <--->       The current batch, which is smaller than BATCH_SIZE
  //     <-------->       Smaller than BRANCH_DISTANCE
  //          <-------->  Smaller than BRANCH_DISTANCE
  //     <------------->  Reachable from the current batch
  i64 a = 0;
  i64 b = 0;
  i64 c = 0;
  i64 d = 0;
  i64 offset = 0;

  // The smallest thunk index that is reachable from the current batch.
  i64 t = 0;

  while (b < m.size()) {
    // Move D foward as far as we can jump from B to a thunk at D.
    auto d_thunk_end = [&] {
      u64 d_end = align_to(offset, 1 << m[d]->p2align) + m[d]->sh_size;
      return align_to(d_end, thunk_align) + max_thunk_size;
    };

    while (d < m.size() &&
           (b == d || d_thunk_end() <= m[b]->offset + branch_distance<E>)) {
      offset = align_to(offset, 1 << m[d]->p2align);
      m[d]->offset = offset;
      offset += m[d]->sh_size;
      d++;
    }

    // Move C forward so that C is apart from B by BATCH_SIZE. We want
    // to make sure that there's at least one section between B and C
    // to ensure progress.
    c = b + 1;
    while (c < d && m[c]->offset + m[c]->sh_size < m[b]->offset + batch_size)
      c++;

    // Move A forward so that A is reachable from C.
    i64 c_offset = (c == d) ? offset : m[c]->offset;
    while (a < b && m[a]->offset + branch_distance<E> < c_offset)
      a++;

    // Erase references to out-of-range thunks.
    for (; t < thunks.size() && thunks[t]->offset < m[a]->offset; t++)
      for (Symbol<E> *sym : thunks[t]->symbols)
        sym->flags = 0;

    // Create a new thunk and place it at D.
    offset = align_to(offset, thunk_align);
    thunks.emplace_back(std::make_unique<Thunk<E>>(*this, offset));
    Thunk<E> &thunk = *thunks.back();

    // Scan relocations between B and C to collect symbols that need
    // entries in the new thunk.
    std::mutex mu;

    tbb::parallel_for(b, c, [&](i64 i) {
      InputSection<E> &isec = *m[i];

      for (const ElfRel<E> &rel : isec.get_rels(ctx)) {
        if (!is_func_call_rel(rel))
          continue;

        // Skip if the symbol is undefined. apply_reloc() will report an error.
        Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
        if (!sym.file)
          continue;

        // Skip if we can directly branch to the destination.
        if (is_reachable(ctx, first_pass, isec, sym, rel) &&
            !needs_shim(ctx, sym, rel))
          continue;

        // Add the symbol to the current thunk if it's not added already.
        if (!sym.flags.test_and_set()) {
          std::scoped_lock lock(mu);
          thunk.symbols.push_back(&sym);
        }
      }
    });

    // Sort symbols added to the thunk to make the output deterministic.
    ranges::sort(thunk.symbols, {}, [](Symbol<E> *x) {
      return std::tuple{x->file->priority, x->sym_idx};
    });

    // Now that we know the number of symbols in the thunk, we can compute
    // the thunk's size.
    assert(thunk.size() < max_thunk_size);
    offset += thunk.size();

    // Move B forward to point to the begining of the next batch.
    b = c;
  }

  for (; t < thunks.size(); t++)
    for (Symbol<E> *sym : thunks[t]->symbols)
      sym->flags = 0;

  this->shdr.sh_size = offset;
}

template <>
void remove_redundant_thunks(Context<E> &ctx) {
  Timer t(ctx, "remove_redundant_thunks");
  set_osec_offsets(ctx);

  for (Chunk<E> *chunk : ctx.chunks)
    if (OutputSection<E> *osec = chunk->to_osec())
      if (osec->shdr.sh_flags & SHF_EXECINSTR)
        osec->create_range_extension_thunks(ctx, false);
}

// When applying relocations, we want to know the address in a reachable
// range extension thunk for a given symbol. Doing it by scanning all
// reachable range extension thunks is too expensive.
//
// In this function, we create a list of all addresses in range extension
// thunks for each symbol, so that it is easy to find one.
//
// Note that thunk_addrs must be sorted for binary search.
template <>
void gather_thunk_addresses(Context<E> &ctx) {
  Timer t(ctx, "gather_thunk_addresses");

  std::vector<OutputSection<E> *> sections;
  for (Chunk<E> *chunk : ctx.chunks)
    if (OutputSection<E> *osec = chunk->to_osec())
      sections.push_back(osec);

  ranges::stable_sort(sections, {}, [](OutputSection<E> *x) {
    return x->shdr.sh_addr;
  });

  for (OutputSection<E> *osec : sections) {
    for (std::unique_ptr<Thunk<E>> &thunk : osec->thunks) {
      for (i64 i = 0; i < thunk->symbols.size(); i++) {
        Symbol<E> &sym = *thunk->symbols[i];
        sym.add_aux(ctx);
        ctx.symbol_aux[sym.aux_idx].thunk_addrs.push_back(thunk->get_addr(i));
      }
    }
  }
}

} // namespace mold

#endif
