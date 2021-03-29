#include "mold.h"

#include <fstream>
#include <iomanip>
#include <ios>
#include <tbb/parallel_for_each.h>
#include <unordered_map>

static std::ofstream *open_output_file(std::string path) {
  std::ofstream *file = new std::ofstream;
  file->open(path.c_str());
  if (!file->is_open())
    Fatal() << "cannot open " << ctx.arg.Map << ": " << strerror(errno);
  return file;
}

void print_map() {
  typedef tbb::concurrent_hash_map<InputSection *, std::vector<Symbol *>> MapTy;

  std::ostream *out = &std::cout;
  std::ofstream *file = nullptr;

  if (!ctx.arg.Map.empty())
    out = file = open_output_file(ctx.arg.Map);

  // Construct a section-to-symbol map.
  MapTy map;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile *file) {
    for (Symbol *sym : file->symbols) {
      if (sym->file == file && sym->input_section &&
          sym->get_type() != STT_SECTION) {
        assert(file == &sym->input_section->file);

        MapTy::accessor acc;
        map.insert(acc, {sym->input_section, {}});
        acc->second.push_back(sym);
      }
    }
  });

  tbb::parallel_for(map.range(), [](const MapTy::range_type &range) {
    for (auto it = range.begin(); it != range.end(); it++) {
      std::vector<Symbol *> &vec = it->second;
      sort(vec, [](Symbol *a, Symbol *b) { return a->value < b->value; });
    }
  });

  *out << "             VMA       Size Align Out     In      Symbol\n";

  for (OutputChunk *osec : ctx.chunks) {
    *out << std::setw(16) << (u64)osec->shdr.sh_addr
         << std::setw(11) << (u64)osec->shdr.sh_size
         << std::setw(6) << (u64)osec->shdr.sh_addralign
         << " " << osec->name << "\n";

    if (osec->kind != OutputChunk::REGULAR)
      continue;

    for (InputSection *mem : ((OutputSection *)osec)->members) {
      *out << std::setw(16) << (osec->shdr.sh_addr + mem->offset)
           << std::setw(11) << (u64)mem->shdr.sh_size
           << std::setw(6) << (u64)mem->shdr.sh_addralign
           << "         " << *mem << "\n";

      MapTy::const_accessor acc;
      if (!map.find(acc, mem))
        continue;

      std::vector<Symbol *> syms = acc->second;
      for (Symbol *sym : syms)
        *out << std::setw(16) << sym->get_addr(ctx)
             << "          0     0                 "
             << *sym << "\n";
    }
  }

  if (file)
    file->close();
}
