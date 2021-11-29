#include "mold.h"

namespace mold::macho {

static std::vector<Subsection *> collect_root_set(Context &ctx) {
  std::vector<Subsection *> rootset;

  auto mark = [&](Symbol *sym) {
    if (sym && sym->subsec)
      rootset.push_back(sym->subsec);
  };

  mark(intern(ctx, ctx.arg.entry));

  if (ctx.output_type == MH_DYLIB)
    for (ObjectFile *file : ctx.objs)
      for (Symbol *sym : file->syms)
        if (sym->file == file && sym->is_extern)
          mark(sym);

  return rootset;
}

static void visit(Context &ctx, Subsection &subsec) {
  if (subsec.is_alive.exchange(true))
    return;

  for (Relocation &rel : subsec.get_rels()) {
    if (rel.sym) {
      if (rel.sym->subsec)
        visit(ctx, *rel.sym->subsec);
    } else {
      visit(ctx, *rel.subsec);
    }
  }

  for (UnwindRecord &rec : subsec.get_unwind_records()) {
    rec.is_alive = true;
    visit(ctx, *rec.subsec);
    if (rec.lsda)
      visit(ctx, *rec.lsda);
    if (Symbol *sym = rec.personality; sym && sym->subsec)
      visit(ctx, *sym->subsec);
  }
}

static bool refers_live_subsection(Subsection &subsec) {
  for (Relocation &rel : subsec.get_rels()) {
    if (rel.sym) {
      if (!rel.sym->subsec || rel.sym->subsec->is_alive)
        return true;
    } else {
      if (rel.subsec->is_alive)
        return true;
    }
  }
  return false;
}

static void mark(Context &ctx, std::span<Subsection *> rootset) {
  for (Subsection *subsec : rootset)
    visit(ctx, *subsec);

  bool repeat;
  do {
    repeat = false;
    for (ObjectFile *file : ctx.objs) {
      for (std::unique_ptr<Subsection> &subsec : file->subsections) {
        if ((subsec->isec.hdr.attr & S_ATTR_LIVE_SUPPORT) &&
            !subsec->is_alive &&
            refers_live_subsection(*subsec)) {
          visit(ctx, *subsec);
          repeat = true;
        }
      }
    }
  } while (repeat);
}

static void sweep(Context &ctx) {
  for (ObjectFile *file : ctx.objs) {
    erase(file->subsections, [](const std::unique_ptr<Subsection> &subsec) {
      return !subsec->is_alive;
    });
  }

  for (ObjectFile *file : ctx.objs)
    for (Symbol *&sym : file->syms)
      if (sym->file == file && sym->subsec && !sym->subsec->is_alive)
        sym = nullptr;
}

void dead_strip(Context &ctx) {
  std::vector<Subsection *> rootset = collect_root_set(ctx);
  mark(ctx, rootset);
  sweep(ctx);
}

} // namespace mold::macho
