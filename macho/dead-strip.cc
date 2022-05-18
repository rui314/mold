#include "mold.h"

namespace mold::macho {

template <typename E>
static std::vector<Subsection<E> *> collect_root_set(Context<E> &ctx) {
  std::vector<Subsection<E> *> rootset;

  auto mark = [&](Symbol<E> *sym) {
    if (sym && sym->subsec)
      rootset.push_back(sym->subsec);
  };

  mark(get_symbol(ctx, ctx.arg.entry));

  if (ctx.output_type == MH_DYLIB || ctx.output_type == MH_BUNDLE)
    for (ObjectFile<E> *file : ctx.objs)
      for (Symbol<E> *sym : file->syms)
        if (sym->file == file && sym->is_extern)
          mark(sym);

  return rootset;
}

template <typename E>
static void visit(Context<E> &ctx, Subsection<E> &subsec) {
  if (subsec.is_alive.exchange(true))
    return;

  for (Relocation<E> &rel : subsec.get_rels()) {
    if (rel.sym) {
      if (rel.sym->subsec)
        visit(ctx, *rel.sym->subsec);
    } else {
      visit(ctx, *rel.subsec);
    }
  }

  for (UnwindRecord<E> &rec : subsec.get_unwind_records()) {
    visit(ctx, *rec.subsec);
    if (rec.lsda)
      visit(ctx, *rec.lsda);
    if (Symbol<E> *sym = rec.personality; sym && sym->subsec)
      visit(ctx, *sym->subsec);
  }
}

template <typename E>
static bool refers_live_subsection(Subsection<E> &subsec) {
  for (Relocation<E> &rel : subsec.get_rels()) {
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

template <typename E>
static void mark(Context<E> &ctx, const std::vector<Subsection<E> *> &rootset) {
  for (Subsection<E> *subsec : rootset)
    visit(ctx, *subsec);

  bool repeat;
  do {
    repeat = false;
    for (ObjectFile<E> *file : ctx.objs) {
      for (std::unique_ptr<Subsection<E>> &subsec : file->subsections) {
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

template <typename E>
static void sweep(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    for (Symbol<E> *&sym : file->syms)
      if (sym->file == file && sym->subsec && !sym->subsec->is_alive)
        sym = nullptr;

  for (ObjectFile<E> *file : ctx.objs) {
    std::erase_if(file->subsections,
                  [](const std::unique_ptr<Subsection<E>> &subsec) {
      return !subsec->is_alive;
    });
  }
}

template <typename E>
void dead_strip(Context<E> &ctx) {
  std::vector<Subsection<E> *> rootset = collect_root_set(ctx);
  mark(ctx, rootset);
  sweep(ctx);
}

#define INSTANTIATE(E)                          \
  template void dead_strip(Context<E> &)

INSTANTIATE_ALL;

} // namespace mold::macho
