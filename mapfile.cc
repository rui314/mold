#include "mold.h"

#include <iomanip>
#include <ios>
#include <unordered_map>

void print_map() {
  // Construct a section-to-symbol map.
  std::unordered_map<InputSection *, std::vector<Symbol *>> map;
  for (ObjectFile *file : out::objs)
    for (Symbol *sym : file->symbols)
      if (sym->file == file && sym->input_section)
        map[sym->input_section].push_back(sym);

  for (auto &pair : map) {
    std::vector<Symbol *> &vec = pair.second;
    sort(vec, [](Symbol *a, Symbol *b) { return a->value < b->value; });
  }

  std::cout << "             VMA     Size Align Out     In      Symbol\n";
  for (OutputChunk *osec : out::chunks) {
    std::cout << std::setw(16) << (u64)osec->shdr.sh_addr
              << std::setw(9) << (u64)osec->shdr.sh_size
              << std::setw(6) << (u64)osec->shdr.sh_addralign
              << " " << osec->name << "\n";

    if (osec->kind != OutputChunk::REGULAR)
      continue;

    for (InputSection *mem : ((OutputSection *)osec)->members) {
      std::cout << std::setw(16) << (osec->shdr.sh_addr + mem->offset)
                << std::setw(9) << (u64)mem->shdr->sh_size
                << std::setw(6) << (u64)mem->shdr->sh_addralign
                << "         " << *mem << "\n";

      auto it = map.find(mem);
      if (it == map.end())
        continue;

      std::vector<Symbol *> syms = it->second;
      for (Symbol *sym : syms)
        std::cout << std::setw(16) << sym->get_addr()
                  << "        0     0                 "
                  << sym->name << "\n";
    }
  }
}
