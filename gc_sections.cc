#include "mold.h"

#include <tbb/concurrent_vector.h>
#include <tbb/parallel_for_each.h>

static bool is_init_fini(const InputSection &isec) {
  return isec.shdr.sh_type == SHT_INIT_ARRAY ||
         isec.shdr.sh_type == SHT_FINI_ARRAY ||
         isec.shdr.sh_type == SHT_PREINIT_ARRAY ||
         isec.name.starts_with(".ctors") ||
         isec.name.starts_with(".dtors") ||
         isec.name.starts_with(".init") ||
         isec.name.starts_with(".fini");
}

static bool mark_section(InputSection *isec) {
  return isec && isec->is_alive && !isec->is_visited.exchange(true);
}

static void
visit(InputSection *isec, std::function<void(InputSection *)> enqueue) {
  assert(isec->is_visited);

  for (SectionFragmentRef &ref : isec->rel_fragments)
    ref.frag->is_alive = true;

  for (ElfRela &rel : isec->rels)
    enqueue(isec->file->symbols[rel.r_sym]->input_section);
}

void gc_sections() {
  Timer t("gc_sections");

  tbb::concurrent_vector<InputSection *> roots;

  // Add sections that are not subject to garbage collection.
  auto enqueue = [&](InputSection *isec) {
    if (mark_section(isec))
      roots.push_back(isec);
  };

  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (InputSection *isec : file->sections) {
      if (!isec)
        continue;

      // -gc-sections discards only SHF_ALLOC sections. If you want to
      // reduce the amount of non-memory-mapped segments, you should
      // use `strip` command, compile without debug info or use
      // -strip-all linker option.
      if (!(isec->shdr.sh_flags & SHF_ALLOC))
        isec->is_visited = true;

      if (is_init_fini(*isec))
        enqueue(isec);
      if (isec->shdr.sh_type == SHT_NOTE)
        enqueue(isec);
    }
  });

  // Add sections referenced by root symbols.
  enqueue(Symbol::intern(config.entry)->input_section);

  // .eh_frame consists of variable-length records called CIE and FDE
  // records, and they are a unit of inclusion or exclusion.
  // We just keep all CIEs and everything that are referenced by them.
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (CieRecord &cie : file->cies)
      for (EhReloc &rel : cie.rels)
        enqueue(rel.sym->input_section);
  });

  // Mark all reachable sections
  tbb::parallel_do(
    roots,
    [&](InputSection *isec, tbb::parallel_do_feeder<InputSection *> &feeder) {
      visit(isec, [&](InputSection *x) {
        if (mark_section(x))
          feeder.add(x);
      });
    });

  // Mark everything that are referenced by live FDEs.
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (CieRecord &cie : file->cies) {
      for (FdeRecord &fde : cie.fdes) {
        if (!fde.is_alive())
          continue;

        // We skip the first relocation because it's always alive if
        // is_alive() returned true.
        for (i64 i = 1; i < fde.rels.size(); i++) {
          InputSection *isec = fde.rels[i].sym->input_section;
          if (!isec)
            continue;
          if (!isec->rels.empty())
            Fatal() << *isec << ": a section referenced by .eh_frame"
                    << " with relocations is not supported";
          enqueue(isec);
        }
      }
    }
  });

  // Remove unreachable sections
  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (i64 i = 0; i < file->sections.size(); i++) {
      InputSection *isec = file->sections[i];

      if (isec && isec->is_alive && !isec->is_ehframe && !isec->is_visited) {
        if (config.print_gc_sections)
          SyncOut() << "removing unused section " << *isec;
        isec->is_alive = false;
        file->sections[i] = nullptr;
      }
    }
  });
}
