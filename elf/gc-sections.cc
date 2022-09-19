// This file implements a mark-sweep garbage collector for -gc-sections.
// In this algorithm, vertices are sections and edges are relocations.
// Any section that is reachable from a root section is considered alive.

#include "mold.h"

#include <tbb/concurrent_vector.h>
#include <tbb/parallel_for_each.h>

namespace mold::elf {

template <typename E>
static bool is_init_fini(const InputSection<E> &isec) {
  u32 type = isec.shdr().sh_type;
  std::string_view name = isec.name();

  return type == SHT_INIT_ARRAY ||
         type == SHT_FINI_ARRAY ||
         type == SHT_PREINIT_ARRAY ||
         (std::is_same_v<E, ARM32> && type == SHT_ARM_EXIDX) ||
         name.starts_with(".ctors") ||
         name.starts_with(".dtors") ||
         name.starts_with(".init") ||
         name.starts_with(".fini");
}

template <typename E>
static bool mark_section(InputSection<E> *isec) {
  return isec && isec->is_alive && !isec->is_visited.exchange(true);
}

template <typename E>
static void visit(Context<E> &ctx, InputSection<E> *isec,
                  tbb::feeder<InputSection<E> *> &feeder, i64 depth) {
  assert(isec->is_visited);

  // If this is a text section, .eh_frame may contain records
  // describing how to handle exceptions for that function.
  // We want to keep associated .eh_frame records.
  for (FdeRecord<E> &fde : isec->get_fdes())
    for (const ElfRel<E> &rel : fde.get_rels(isec->file).subspan(1))
      if (Symbol<E> *sym = isec->file.symbols[rel.r_sym])
        if (mark_section(sym->get_input_section()))
          feeder.add(sym->get_input_section());

  for (const ElfRel<E> &rel : isec->get_rels(ctx)) {
    Symbol<E> &sym = *isec->file.symbols[rel.r_sym];

    // Symbol can refer either a section fragment or an input section.
    // Mark a fragment as alive.
    if (SectionFragment<E> *frag = sym.get_frag()) {
      frag->is_alive.store(true, std::memory_order_relaxed);
      continue;
    }

    if (!mark_section(sym.get_input_section()))
      continue;

    // Mark a section alive. For better performacne, we don't call
    // `feeder.add` too often.
    if (depth < 3)
      visit(ctx, sym.get_input_section(), feeder, depth + 1);
    else
      feeder.add(sym.get_input_section());
  }
}

template <typename E>
static void collect_root_set(Context<E> &ctx,
                             tbb::concurrent_vector<InputSection<E> *> &rootset) {
  Timer t(ctx, "collect_root_set");

  auto enqueue_section = [&](InputSection<E> *isec) {
    if (mark_section(isec))
      rootset.push_back(isec);
  };

  auto enqueue_symbol = [&](Symbol<E> *sym) {
    if (sym) {
      if (SectionFragment<E> *frag = sym->get_frag())
        frag->is_alive.store(true, std::memory_order_relaxed);
      else
        enqueue_section(sym->get_input_section());
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
      u32 flags = isec->shdr().sh_flags;
      if (!(flags & SHF_ALLOC))
        isec->is_visited = true;

      if (is_init_fini(*isec) || is_c_identifier(isec->name()) ||
          (flags & SHF_GNU_RETAIN) || isec->shdr().sh_type == SHT_NOTE)
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
  enqueue_symbol(get_symbol(ctx, ctx.arg.entry));

  for (std::string_view name : ctx.arg.undefined)
    enqueue_symbol(get_symbol(ctx, name));

  for (std::string_view name : ctx.arg.require_defined)
    enqueue_symbol(get_symbol(ctx, name));

  // .eh_frame consists of variable-length records called CIE and FDE
  // records, and they are a unit of inclusion or exclusion.
  // We just keep all CIEs and everything that are referenced by them.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (CieRecord<E> &cie : file->cies)
      for (const ElfRel<E> &rel : cie.get_rels())
        enqueue_symbol(file->symbols[rel.r_sym]);
  });
}

// Mark all reachable sections
template <typename E>
static void mark(Context<E> &ctx,
                 tbb::concurrent_vector<InputSection<E> *> &rootset) {
  Timer t(ctx, "mark");

  tbb::parallel_for_each(rootset, [&](InputSection<E> *isec,
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

// Non-alloc section fragments are not subject of garbage collection.
// This function marks such fragments.
template <typename E>
static void mark_nonalloc_fragments(Context<E> &ctx) {
  Timer t(ctx, "mark_nonalloc_fragments");

  tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
    for (std::unique_ptr<MergeableSection<E>> &m : file->mergeable_sections)
      if (m)
        for (SectionFragment<E> *frag : m->fragments)
          if (!(frag->output_section.shdr.sh_flags & SHF_ALLOC))
            frag->is_alive.store(true, std::memory_order_relaxed);
  });
}

template <typename E>
void gc_sections(Context<E> &ctx) {
  Timer t(ctx, "gc");

  mark_nonalloc_fragments(ctx);

  tbb::concurrent_vector<InputSection<E> *> rootset;
  collect_root_set(ctx, rootset);
  mark(ctx, rootset);
  sweep(ctx);
}

#define INSTANTIATE(E)                                  \
  template void gc_sections(Context<E> &ctx);

INSTANTIATE_ALL;

} // namespace mold::elf
