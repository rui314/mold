#include "mold.h"

#include <fstream>
#include <iomanip>
#include <ios>
#include <tbb/parallel_for_each.h>
#include <unordered_map>

template <typename E>
static std::ofstream *open_output_file(Context<E> &ctx) {
  std::ofstream *file = new std::ofstream;
  file->open(ctx.arg.Map.c_str());
  if (!file->is_open())
    Fatal(ctx) << "cannot open " << ctx.arg.Map << ": " << strerror(errno);
  return file;
}

template <typename E>
void print_map(Context<E> &ctx) {
  typedef tbb::concurrent_hash_map<InputSection<E> *,
                                   std::vector<Symbol<E> *>>
    MapTy;

  std::ostream *out = &std::cout;
  std::ofstream *file = nullptr;

  if (!ctx.arg.Map.empty())
    out = file = open_output_file(ctx);

  // Construct a section-to-symbol map.
  MapTy map;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->symbols) {
      if (sym->file == file && sym->input_section &&
          sym->get_type() != STT_SECTION) {
        assert(file == &sym->input_section->file);

        typename MapTy::accessor acc;
        map.insert(acc, {sym->input_section, {}});
        acc->second.push_back(sym);
      }
    }
  });

  tbb::parallel_for(map.range(), [](const typename MapTy::range_type &range) {
    for (auto it = range.begin(); it != range.end(); it++) {
      std::vector<Symbol<E> *> &vec = it->second;
      sort(vec, [](Symbol<E> *a, Symbol<E> *b) { return a->value < b->value; });
    }
  });

  *out << "             VMA       Size Align Out     In      Symbol\n";

  for (OutputChunk<E> *osec : ctx.chunks) {
    *out << std::setw(16) << (u64)osec->shdr.sh_addr
         << std::setw(11) << (u64)osec->shdr.sh_size
         << std::setw(6) << (u64)osec->shdr.sh_addralign
         << " " << osec->name << "\n";

    if (osec->kind != OutputChunk<E>::REGULAR)
      continue;

    for (InputSection<E> *mem : ((OutputSection<E> *)osec)->members) {
      *out << std::setw(16) << (osec->shdr.sh_addr + mem->offset)
           << std::setw(11) << (u64)mem->shdr.sh_size
           << std::setw(6) << (u64)mem->shdr.sh_addralign
           << "         " << *mem << "\n";

      typename MapTy::const_accessor acc;
      if (!map.find(acc, mem))
        continue;

      std::vector<Symbol<E> *> syms = acc->second;
      for (Symbol<E> *sym : syms)
        *out << std::setw(16) << sym->get_addr(ctx)
             << "          0     0                 "
             << *sym << "\n";
    }
  }

  if (file)
    file->close();
}

template void print_map(Context<ELF64LE> &ctx);
