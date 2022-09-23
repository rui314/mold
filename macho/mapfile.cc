#include "mold.h"

#include <iomanip>
#include <fstream>

namespace mold::macho {

struct Sym {
  u64 addr;
  u32 size;
  u32 fileidx;
  std::string_view name;
};

template <typename E>
void print_map(Context<E> &ctx) {
  std::ofstream out(ctx.arg.map.c_str());
  if (!out.is_open())
    Fatal(ctx) << "cannot open " << ctx.arg.map << ": " << errno_string();

  out << "# Path: " << ctx.arg.output << "\n"
      << "# Arch: x86-64\n"
      << "# Object files:\n";

  std::vector<Sym> syms;

  for (i64 i = 0; i < ctx.objs.size(); i++){
    ObjectFile<E> &file = *ctx.objs[i];

    if (file.is_alive) {
      out << "[" << std::setw(3) << i << "] " << file << "\n";
      for (Symbol<E> *sym : file.syms)
        if (sym && sym->file == &file)
          if (!sym->subsec || sym->subsec->is_alive)
            syms.push_back({sym->get_addr(ctx), 0, (u32)i, sym->name});
    }
  }

  sort(syms, [](const Sym &a, const Sym &b) {
    return a.addr < b.addr;
  });

  out << "# Sections:\n"
      << "# Address       Size            Segment Section\n";

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments) {
    for (Chunk<E> *chunk : seg->chunks) {
      if (!chunk->is_hidden) {
        out << "0x" << std::hex
            << std::setw(8) << std::setfill('0') << chunk->hdr.addr
            << "     0x"
            << std::setw(8) << std::setfill('0') << chunk->hdr.size
            << "      " << std::setw(7) << std::left << std::setfill(' ')
            << chunk->hdr.get_segname()
            << " " << chunk->hdr.get_sectname() << "\n";
      }
    }
  }

  out << "# Symbols:\n"
      << "# Address       Size            File  Name\n";

  for (Sym &sym : syms) {
    out << "0x" << std::hex
        << std::setw(8) << std::setfill('0') << sym.addr
        << "     0x"
        << std::setw(8) << std::setfill('0') << 0
        << "      ["
        << std::setw(3) << std::right << std::setfill(' ') << std::dec
        << sym.fileidx
        << "] " << sym.name << "\n";
  }
}

using E = MOLD_TARGET;

template void print_map(Context<E> &);

} // namespace mold::macho
