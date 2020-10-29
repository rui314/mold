#include "mold.h"

using namespace llvm;

void print_map(ArrayRef<ObjectFile *> files,
               ArrayRef<OutputSection *> output_sections) {
  // Construct a section-to-symbol map.
  std::unordered_multimap<InputSection *, Symbol *> map;
  for (ObjectFile *file : files)
    for (Symbol *sym : file->symbols)
      if (sym->file == file && sym->input_section)
        map.insert({sym->input_section, sym});

  llvm::outs() << "             VMA     Size Align Out     In      Symbol\n";
  for (OutputSection *osec : output_sections) {
    llvm::outs() << format("%16llx %8llx %5lld ",
                           (uint64_t)osec->shdr.sh_addr,
                           (uint64_t)osec->shdr.sh_size,
                           (uint64_t)osec->shdr.sh_addralign)
                 << osec->name << "\n";

    for (InputSection *isec : osec->sections) {
      llvm::outs() << format("%16llx %8llx %5lld         ",
                             osec->shdr.sh_addr + isec->offset,
                             (uint64_t)isec->shdr.sh_size,
                             (uint64_t)isec->shdr.sh_addralign)
                   << toString(isec) << "\n";

      auto range = map.equal_range(isec);
      for (auto it = range.first; it != range.second; ++it) {
        Symbol *sym = it->second;
        llvm::outs() << format("%16llx %8llx %5lld                 ", sym->addr, 0, 0)
                     << sym->name << "\n";
      }
    }
  }
}
