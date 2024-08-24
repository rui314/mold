// Since RISC instructions are generally up to 32 bits long, there's no
// way to embed very large immediates into their branch instructions. For
// example, RISC-V's JAL (jump and link) instruction can jump to only
// within PC ± 1 MiB because its immediate is 21 bits long. If the
// destination is further than that, we need to use two instructions
// instead; the first instruction being AUIPC, which sets the upper 20
// bits of a displacement to a register, and the second being JALR, which
// specifies the lower 12 bits and the register. Combined, they specify a
// 32-bit displacement, which is sufficient to support the medium code
// model.
//
// However, always using two or more instructions for function calls is a
// waste of time and space if the branch target is within a single
// instruction's reach. There are two approaches to address this problem
// as follows:
//
//  1. The compiler optimistically emits a single branch instruction for
//     all function calls. The linker then checks if the branch target is
//     reachable, and if not, redirects the branch to a linker-synthesized
//     code sequence that uses two or more instructions to branch further.
//     That linker-synthesized code is called a "thunk". All RISC psABIs
//     except RISC-V and LoongArch take this approach.
//
//  2. The compiler pessimistically emits two instructions to branch
//     anywhere in PC ± 2 GiB, and the linker rewrites them with a single
//     instruction if the branch target is close enough. RISC-V and
//     LoongArch take this approach.
//
// This file contains functions to support (2). For (1), see thunks.cc.
//
// With the presence of this code-shrinking relaxation, sections can no
// longer be considered as atomic units. If we delete an instruction from
// the middle of a section, the section contents after that point need to
// be shifted by the size of the instruction. Symbol values and relocation
// offsets have to be shifted too if they refer to bytes past the deleted
// ones.
//
// In mold, we use `r_deltas` to memorize how many bytes have been shifted
// for relocations. For symbols, we directly mutate their `value` member.
//
// RISC-V and LoongArch object files tend to have way more relocations
// than those for other targets. This is because all branches, including
// those that jump within the same section, are explicitly expressed with
// relocations. Here is why we need them: all control-flow statements,
// such as `if` or `for`, are implemented using branch instructions. For
// other targets, the compiler doesn't emit relocations for such branches
// because it knows at compile-time exactly how many bytes have to be
// skipped. That's not true in RISC-V and LoongArch because the linker may
// delete bytes between a branch and its target. Therefore, all branches,
// including in-section ones, have to be explicitly expressed with
// relocations.
//
// Note that this mechanism only shrinks sections and never enlarges them,
// as the compiler always emits the longest instruction sequence. This
// makes the linker implementation a bit simpler because we don't need to
// worry about oscillation.

#if MOLD_RV64LE || MOLD_RV64BE || MOLD_RV32LE || MOLD_RV32BE || \
    MOLD_LOONGARCH64 || MOLD_LOONGARCH32

#include "mold.h"

#include <tbb/parallel_for_each.h>

namespace mold {

using E = MOLD_TARGET;

static bool is_resizable(InputSection<E> *isec) {
  return isec && isec->is_alive && (isec->shdr().sh_flags & SHF_ALLOC) &&
         (isec->shdr().sh_flags & SHF_EXECINSTR);
}

template <>
void shrink_sections<E>(Context<E> &ctx) {
  Timer t(ctx, "shrink_sections");

  // True if we can use the 2-byte instructions. This is usually true on
  // Unix because RV64GC is generally considered the baseline hardware.
  bool use_rvc = false;
  if constexpr (is_riscv<E>)
    use_rvc = get_eflags(ctx) & EF_RISCV_RVC;

  // Find all relaxable relocations and record how many bytes we can save
  // into r_deltas.
  //
  // Technically speaking, relaxing relocations may allow more relocations
  // to be relaxed because the distance between a branch instruction and
  // its target may decrease as a result of relaxation. That said, the
  // number of such relocations is negligible (I tried to self-host mold
  // on RISC-V as an experiment and found that the mold-built .text is
  // only ~0.04% larger than that of GNU ld), so we don't bother to handle
  // them. We scan relocations only once here.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections)
      if (is_resizable(isec.get()))
        shrink_section(ctx, *isec, use_rvc);
  });

  // Fix symbol values.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->symbols) {
      if (sym->file != file)
        continue;

      InputSection<E> *isec = sym->get_input_section();
      if (!isec || isec->extra.r_deltas.empty())
        continue;

      std::span<const ElfRel<E>> rels = isec->get_rels(ctx);
      auto it = std::lower_bound(rels.begin(), rels.end(), sym->value,
                                 [&](const ElfRel<E> &r, u64 val) {
        return r.r_offset < val;
      });

      sym->value -= isec->extra.r_deltas[it - rels.begin()];
    }
  });

  // Recompute sizes of executable sections
  tbb::parallel_for_each(ctx.chunks, [&](Chunk<E> *chunk) {
    if (chunk->to_osec() && (chunk->shdr.sh_flags & SHF_EXECINSTR))
      chunk->compute_section_size(ctx);
  });
}

// Returns the distance between a relocated place and a symbol.
template <>
i64 compute_distance<E>(Context<E> &ctx, Symbol<E> &sym,
                        InputSection<E> &isec, const ElfRel<E> &rel) {
  // We handle absolute symbols as if they were infinitely far away
  // because `shrink_section` may increase a distance between a branch
  // instruction and an absolute symbol. Branching to an absolute
  // location is extremely rare in real code, though.
  if (sym.is_absolute())
    return INT64_MAX;

  // Likewise, relocations against weak undefined symbols won't be relaxed.
  if (sym.esym().is_undef_weak())
    return INT64_MAX;

  // Compute a distance between the relocated place and the symbol.
  i64 S = sym.get_addr(ctx);
  i64 A = rel.r_addend;
  i64 P = isec.get_addr() + rel.r_offset;
  return S + A - P;
}

} // namespace mold

#endif
