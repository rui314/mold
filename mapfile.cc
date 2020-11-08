#include "mold.h"

#include "llvm/Support/Format.h"

using namespace llvm;

void print_map(ArrayRef<ObjectFile *> files, ArrayRef<OutputChunk *> output_sections) {
  // Construct a section-to-symbol map.
  std::unordered_multimap<InputChunk *, Symbol *> map;
  for (ObjectFile *file : files)
    for (Symbol *sym : file->symbols)
      if (sym->file == file && sym->input_section)
        map.insert({sym->input_section, sym});

  llvm::outs() << "             VMA     Size Align Out     In      Symbol\n";
  for (OutputChunk *osec : output_sections) {
    llvm::outs() << format("%16llx %8llx %5lld ",
                           (u64)osec->shdr.sh_addr,
                           (u64)osec->shdr.sh_size,
                           (u64)osec->shdr.sh_addralign)
                 << osec->name << "\n";

    for (InputChunk *mem : osec->members) {
      llvm::outs() << format("%16llx %8llx %5lld         ",
                             osec->shdr.sh_addr + mem->offset,
                             (u64)mem->shdr.sh_size,
                             (u64)mem->shdr.sh_addralign)
                   << toString(mem) << "\n";

      auto range = map.equal_range(mem);
      for (auto it = range.first; it != range.second; ++it) {
        Symbol *sym = it->second;
        llvm::outs()
          << format("%16llx %8llx %5lld                 ", sym->get_addr(), 0, 0)
          << sym->name << "\n";
      }
    }
  }
}
