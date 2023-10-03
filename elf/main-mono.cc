#include "mold.h"

namespace mold::elf {

// Since elf_main is a template, we can't run it without a type parameter.
// We speculatively run elf_main with X86_64, and if the speculation was
// wrong, re-run it with an actual machine type.
int redo_main(int argc, char **argv, std::string_view target) {
  if (target == I386::target_name)
    return elf_main<I386>(argc, argv);
  if (target == ARM64::target_name)
    return elf_main<ARM64>(argc, argv);
  if (target == ARM32::target_name)
    return elf_main<ARM32>(argc, argv);
  if (target == RV64LE::target_name)
    return elf_main<RV64LE>(argc, argv);
  if (target == RV64BE::target_name)
    return elf_main<RV64BE>(argc, argv);
  if (target == RV32LE::target_name)
    return elf_main<RV32LE>(argc, argv);
  if (target == RV32BE::target_name)
    return elf_main<RV32BE>(argc, argv);
  if (target == PPC32::target_name)
    return elf_main<PPC32>(argc, argv);
  if (target == PPC64V1::target_name)
    return elf_main<PPC64V1>(argc, argv);
  if (target == PPC64V2::target_name)
    return elf_main<PPC64V2>(argc, argv);
  if (target == S390X::target_name)
    return elf_main<S390X>(argc, argv);
  if (target == SPARC64::target_name)
    return elf_main<SPARC64>(argc, argv);
  if (target == M68K::target_name)
    return elf_main<M68K>(argc, argv);
  if (target == SH4::target_name)
    return elf_main<SH4>(argc, argv);
  if (target == ALPHA::target_name)
    return elf_main<ALPHA>(argc, argv);
  if (target == LOONGARCH32::target_name)
    return elf_main<LOONGARCH32>(argc, argv);
  if (target == LOONGARCH64::target_name)
    return elf_main<LOONGARCH64>(argc, argv);
  unreachable();
}

int main(int argc, char **argv) {
  return elf_main<X86_64>(argc, argv);
}

} // namespace mold::elf