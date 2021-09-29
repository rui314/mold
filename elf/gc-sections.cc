// This file implements a mark-sweep garbage collector for -gc-sections.
// In this algorithm, vertices are sections and edges are relocations.
// Any section that is reachable from a root section is considered alive.

#include "mold.h"

#include <tbb/concurrent_vector.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

template <typename E>
static bool is_init_fini(const InputSection<E> &isec) {
  return isec.shdr.sh_type == SHT_INIT_ARRAY ||
         isec.shdr.sh_type == SHT_FINI_ARRAY ||
         isec.shdr.sh_type == SHT_PREINIT_ARRAY ||
         isec.name().starts_with(".ctors") ||
         isec.name().starts_with(".dtors") ||
         isec.name().starts_with(".init") ||
         isec.name().starts_with(".fini");
}

template <typename E>
static bool mark_section(InputSection<E> *isec) {
  return isec && isec->is_alive && !isec->is_visited.exchange(true);
}

template <typename E>
static void visit(Context<E> &ctx, InputSection<E> *isec,
                  tbb::feeder<InputSection<E> *> &feeder, i64 depth) {
  assert(isec->is_visited);

  // A relocation can refer either a subsection (i.e. a piece of
  // string in a mergeable string section) or a symbol. Mark all
  // subsections as alive.
  if (SubsectionRef<E> *refs = isec->rel_subsections.get())
    for (i64 i = 0; refs[i].idx >= 0; i++)
      refs[i].subsec->is_alive.store(true, std::memory_order_relaxed);

  // If this is a text section, .eh_frame may contain records
  // describing how to handle exceptions for that function.
  // We want to keep associated .eh_frame records.
  for (FdeRecord<E> &fde : isec->get_fdes())
    for (ElfRel<E> &rel : fde.get_rels().subspan(1))
      if (Symbol<E> *sym = isec->file.symbols[rel.r_sym])
        if (mark_section(sym->input_section))
          feeder.add(sym->input_section);

  for (ElfRel<E> &rel : isec->get_rels(ctx)) {
    Symbol<E> &sym = *isec->file.symbols[rel.r_sym];

    // Symbol can refer either a subsection or an input section.
    // Mark a subsection as alive.
    if (Subsection<E> *subsec = sym.get_subsec()) {
      subsec->is_alive.store(true, std::memory_order_relaxed);
      continue;
    }

    if (!mark_section(sym.input_section))
      continue;

    // Mark a section alive. For better performacne, we don't call
    // `feeder.add` too often.
    if (depth < 3)
      visit(ctx, sym.input_section, feeder, depth + 1);
    else
      feeder.add(sym.input_section);
  }
}

template <typename E>
static tbb::concurrent_vector<InputSection<E> *>
collect_root_set(Context<E> &ctx) {
  Timer t(ctx, "collect_root_set");
  tbb::concurrent_vector<InputSection<E> *> roots;

  auto enqueue_section = [&](InputSection<E> *isec) {
    if (mark_section(isec))
      roots.push_back(isec);
  };

  auto enqueue_symbol = [&](Symbol<E> *sym) {
    if (sym) {
      if (Subsection<E> *subsec = sym->get_subsec())
        subsec->is_alive.store(true, std::memory_order_relaxed);
      else
        enqueue_section(sym->input_section);
    }
  };

  // Add sections that are not subject to garbage collection.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (!isec || !isec->is_alive)
        continue;

      // -gc-sections discards only SHF_ALLOC sections. If you want to
      // reduce the amount of non-memory-mapped segments, you should
      // use `strip` command, compile without debug info or use
      // -strip-all linker option.
      if (!(isec->shdr.sh_flags & SHF_ALLOC))
        isec->is_visited = true;

      if (is_init_fini(*isec) || is_c_identifier(isec->name()) ||
          isec->shdr.sh_type == SHT_NOTE)
        enqueue_section(isec.get());
    }
  });

  // Add sections containing exported symbols
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->symbols)
      if (sym->file == file && sym->is_exported)
        enqueue_symbol(sym);
  });

  // Add sections referenced by root symbols.
  enqueue_symbol(Symbol<E>::intern(ctx, ctx.arg.entry));

  for (std::string_view name : ctx.arg.undefined)
    enqueue_symbol(Symbol<E>::intern(ctx, name));

  for (std::string_view name : ctx.arg.require_defined)
    enqueue_symbol(Symbol<E>::intern(ctx, name));

  // .eh_frame consists of variable-length records called CIE and FDE
  // records, and they are a unit of inclusion or exclusion.
  // We just keep all CIEs and everything that are referenced by them.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (CieRecord<E> &cie : file->cies)
      for (ElfRel<E> &rel : cie.get_rels())
        enqueue_symbol(file->symbols[rel.r_sym]);
  });

  return roots;
}

// Mark all reachable sections
template <typename E>
static void mark(Context<E> &ctx,
                 tbb::concurrent_vector<InputSection<E> *> &roots) {
  Timer t(ctx, "mark");

  tbb::parallel_for_each(roots, [&](InputSection<E> *isec,
                                    tbb::feeder<InputSection<E> *> &feeder) {
    visit(ctx, isec, feeder, 0);
  });
}

// Remove unreachable sections
template <typename E>
static void sweep(Context<E> &ctx) {
  Timer t(ctx, "sweep");
  static Counter counter("garbage_sections");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (isec && isec->is_alive && !isec->is_visited) {
        if (ctx.arg.print_gc_sections)
          SyncOut(ctx) << "removing unused section " << *isec;
        isec->kill();
        counter++;
      }
    }
  });
}

// Non-alloc subsections are not subject of garbage collection.
// This function marks such subsections.
template <typename E>
static void mark_nonalloc_subsections(Context<E> &ctx) {
  Timer t(ctx, "mark_nonalloc_subsections");

  tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
    for (Subsection<E> *subsec : file->subsections)
      if (!(subsec->output_section.shdr.sh_flags & SHF_ALLOC))
        subsec->is_alive.store(true, std::memory_order_relaxed);
  });
}

template <typename E>
void gc_sections(Context<E> &ctx) {
  Timer t(ctx, "gc");

  mark_nonalloc_subsections(ctx);

  tbb::concurrent_vector<InputSection<E> *> roots = collect_root_set(ctx);
  mark(ctx, roots);
  sweep(ctx);
}

#define INSTANTIATE(E)                                  \
  template void gc_sections(Context<E> &ctx);

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(AARCH64);

} // namespace mold::elf
