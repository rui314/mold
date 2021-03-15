#include "mold.h"

#include <fstream>
#include <iomanip>
#include <ios>
#include <unordered_map>

static std::ofstream *open_output_file(std::string path) {
  std::ofstream *file = new std::ofstream;
  file->open(path.c_str());
  if (!file->is_open())
    Fatal() << "cannot open " << config.Map << ": " << strerror(errno);
  return file;
}

void print_map() {
  std::ostream *out = &std::cout;
  std::ofstream *file = nullptr;

  if (!config.Map.empty())
    out = file = open_output_file(config.Map);

  // Construct a section-to-symbol map.
  std::unordered_map<InputSection *, std::vector<Symbol *>> map;
  for (ObjectFile *file : out::objs)
    for (Symbol *sym : file->symbols)
      if (sym->file == file && sym->input_section &&
          sym->get_type() != STT_SECTION)
        map[sym->input_section].push_back(sym);

  for (auto &pair : map) {
    std::vector<Symbol *> &vec = pair.second;
    sort(vec, [](Symbol *a, Symbol *b) { return a->value < b->value; });
  }

  *out << "             VMA     Size Align Out     In      Symbol\n";
  for (OutputChunk *osec : out::chunks) {
    *out << std::setw(16) << (u64)osec->shdr.sh_addr
         << std::setw(9) << (u64)osec->shdr.sh_size
         << std::setw(6) << (u64)osec->shdr.sh_addralign
         << " " << osec->name << "\n";

    if (osec->kind != OutputChunk::REGULAR)
      continue;

    for (InputSection *mem : ((OutputSection *)osec)->members) {
      *out << std::setw(16) << (osec->shdr.sh_addr + mem->offset)
           << std::setw(9) << (u64)mem->shdr.sh_size
           << std::setw(6) << (u64)mem->shdr.sh_addralign
           << "         " << *mem << "\n";

      auto it = map.find(mem);
      if (it == map.end())
        continue;

      std::vector<Symbol *> syms = it->second;
      for (Symbol *sym : syms)
        *out << std::setw(16) << sym->get_addr()
             << "        0     0                 "
             << *sym << "\n";
    }
  }

  if (file)
    file->close();
}
