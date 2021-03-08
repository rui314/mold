// This file implements a mark-sweep garbage collector for -gc-sections.
// In this algorithm, vertices are sections and edges are relocations.
// Any section that is reachable from a root section is considered alive.

#include "mold.h"

#include <tbb/concurrent_vector.h>
#include <tbb/parallel_for_each.h>

static bool is_init_fini(const InputSection &isec) {
  return isec.shdr->sh_type == SHT_INIT_ARRAY ||
         isec.shdr->sh_type == SHT_FINI_ARRAY ||
         isec.shdr->sh_type == SHT_PREINIT_ARRAY ||
         isec.name.starts_with(".ctors") ||
         isec.name.starts_with(".dtors") ||
         isec.name.starts_with(".init") ||
         isec.name.starts_with(".fini");
}

static bool mark_section(InputSection *isec) {
  return isec && isec->is_alive && !isec->is_visited.exchange(true);
}

static void visit(InputSection *isec,
                  tbb::parallel_do_feeder<InputSection *> &feeder,
                  i64 depth) {
  assert(isec->is_visited);

  // A relocation can refer either a section fragment (i.e. a piece of
  // string in a mergeable string section) or a symbol. Mark all
  // section fragments as alive.
  for (SectionFragmentRef &ref : isec->rel_fragments)
    ref.frag->is_alive = true;

  // If this is a text section, .eh_frame may contain records
  // describing how to handle exceptions for that function.
  // We want to keep associated .eh_frame records.
  for (FdeRecord &fde : isec->fdes)
    for (EhReloc &rel : std::span(fde.rels).subspan(1))
      if (InputSection *isec = rel.sym.input_section)
        if (mark_section(isec))
          feeder.add(isec);

  for (ElfRela &rel : isec->rels) {
    Symbol &sym = *isec->file->symbols[rel.r_sym];

    // Symbol can refer either a section fragment or an input section.
    // Mark a fragment as alive.
    if (sym.frag) {
      sym.frag->is_alive = true;
      continue;
    }

    if (!mark_section(sym.input_section))
      continue;

    // Mark a section alive. For better performacne, we don't call
    // `feeder.add` too often.
    if (depth < 3)
      visit(sym.input_section, feeder, depth + 1);
    else
      feeder.add(sym.input_section);
  }
}

static tbb::concurrent_vector<InputSection *> collect_root_set() {
  Timer t("collect_root_set");
  tbb::concurrent_vector<InputSection *> roots;

  auto enqueue = [&](InputSection *isec) {
    if (mark_section(isec))
      roots.push_back(isec);
  };

  // Add sections that are not subject to garbage collection.
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (InputSection *isec : file->sections) {
      if (!isec)
        continue;

      // -gc-sections discards only SHF_ALLOC sections. If you want to
      // reduce the amount of non-memory-mapped segments, you should
      // use `strip` command, compile without debug info or use
      // -strip-all linker option.
      if (!(isec->shdr->sh_flags & SHF_ALLOC))
        isec->is_visited = true;

      if (is_init_fini(*isec) || isec->shdr->sh_type == SHT_NOTE)
        enqueue(isec);
    }
  });

  // Add sections containing exported symbols
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (Symbol *sym : file->symbols)
      if (sym->file == file)
        if (InputSection *sec = sym->input_section)
          if (sec->shdr->sh_flags & SHF_ALLOC)
            enqueue(sec);
  });

  // Add sections referenced by root symbols.
  enqueue(Symbol::intern(config.entry)->input_section);

  for (std::string_view name : config.undefined)
    if (InputSection *sec = Symbol::intern(config.entry)->input_section)
      enqueue(sec);

  // .eh_frame consists of variable-length records called CIE and FDE
  // records, and they are a unit of inclusion or exclusion.
  // We just keep all CIEs and everything that are referenced by them.
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (CieRecord &cie : file->cies)
      for (EhReloc &rel : cie.rels)
        enqueue(rel.sym.input_section);
  });

  return roots;
}

// Mark all reachable sections
static void mark(tbb::concurrent_vector<InputSection *> roots) {
  Timer t("mark");
  tbb::parallel_do(roots, [&](InputSection *isec,
                              tbb::parallel_do_feeder<InputSection *> &feeder) {
    visit(isec, feeder, 0);
  });
}

// Remove unreachable sections
static void sweep() {
  Timer t("sweep");
  static Counter counter("garbage_sections");

  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (i64 i = 0; i < file->sections.size(); i++) {
      InputSection *isec = file->sections[i];

      if (isec && isec->is_alive && !isec->is_visited) {
        if (config.print_gc_sections)
          SyncOut() << "removing unused section " << *isec;
        isec->kill();
        counter++;
      }
    }
  });
}

void gc_sections() {
  Timer t("gc");

  tbb::concurrent_vector<InputSection *> roots = collect_root_set();
  mark(roots);
  sweep();
}
