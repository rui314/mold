#include "mold.h"

#include <iomanip>
#include <ios>

void print_map() {
  // Construct a section-to-symbol map.
  std::unordered_multimap<InputChunk *, Symbol *> map;
  for (ObjectFile *file : out::objs)
    for (Symbol *sym : file->symbols)
      if (sym->file == file && sym->input_section)
        map.insert({sym->input_section, sym});

  std::cout << "             VMA     Size Align Out     In      Symbol\n";
  for (OutputChunk *osec : out::chunks) {
    std::cout << std::setw(16) << (u64)osec->shdr.sh_addr
              << std::setw(8) << (u64)osec->shdr.sh_size
              << std::setw(5) << (u64)osec->shdr.sh_addralign
              << " " << osec->name << "\n";

    if (osec->kind != OutputChunk::REGULAR)
      continue;

    for (InputChunk *mem : ((OutputSection *)osec)->members) {
      std::cout << std::setw(16) << (osec->shdr.sh_addr + mem->offset)
                << std::setw(8) << (u64)mem->shdr.sh_size
                << std::setw(5) << (u64)mem->shdr.sh_addralign
                << " " << toString(mem) << "\n";

      auto range = map.equal_range(mem);
      for (auto it = range.first; it != range.second; ++it) {
        Symbol *sym = it->second;
        std::cout << std::setw(16) << sym->get_addr()
                  << "       0    0 "
                  << sym->name << "\n";
      }
    }
  }
}
