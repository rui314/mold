#include "mold.h"

namespace mold::macho {

static std::vector<Subsection *> collect_root_set(Context &ctx) {
  std::vector<Subsection *> rootset;

  if (Symbol *sym = intern(ctx, ctx.arg.entry))
    if (sym->subsec)
      rootset.push_back(sym->subsec);

  for (ObjectFile *file : ctx.objs)
    for (std::unique_ptr<Subsection> &subsec : file->subsections)
      if (subsec->isec.hdr.match("__TEXT", "__eh_frame") ||
          subsec->isec.hdr.match("__TEXT", "__gcc_except_tab"))
        rootset.push_back(subsec.get());

  return rootset;
}

static void visit(Context &ctx, Subsection &subsec) {
  if (subsec.is_visited.exchange(true))
    return;

  for (Relocation &rel : subsec.get_rels()) {
    if (rel.sym) {
      if (rel.sym->subsec)
        visit(ctx, *rel.sym->subsec);
    } else {
      visit(ctx, *rel.subsec);
    }
  }
}

static void mark(Context &ctx, std::span<Subsection *> rootset) {
  for (Subsection *subsec : rootset)
    visit(ctx, *subsec);
}

static void sweep(Context &ctx) {
  for (ObjectFile *file : ctx.objs) {
    erase(file->subsections, [](const std::unique_ptr<Subsection> &subsec) {
      return !subsec->is_visited;
    });
  }

  for (ObjectFile *file : ctx.objs)
    for (Symbol *&sym : file->syms)
      if (sym->file == file && sym->subsec && !sym->subsec->is_visited)
        sym = nullptr;
}

void dead_strip(Context &ctx) {
  std::vector<Subsection *> rootset = collect_root_set(ctx);
  mark(ctx, rootset);
  sweep(ctx);
}

} // namespace mold::macho
