// This file implements a mark-sweep garbage collector for -gc-sections.
// In this algorithm, vertices are sections and edges are relocations.
// Any section that is reachable from a root section is considered alive.

#include "mold.h"

#include <fstream>
#include <tbb/concurrent_unordered_map.h>
#include <tbb/concurrent_vector.h>
#include <tbb/parallel_for_each.h>

namespace mold {

template <typename E>
static bool should_keep(const InputSection<E> &isec) {
  u32 type = isec.shdr().sh_type;
  u32 flags = isec.shdr().sh_flags;
  std::string_view name = isec.name();

  if constexpr (is_ppc32<E>)
    if (name == ".got2")
      return true;

  return (flags & SHF_GNU_RETAIN) ||
         type == SHT_NOTE ||
         type == SHT_INIT_ARRAY ||
         type == SHT_FINI_ARRAY ||
         type == SHT_PREINIT_ARRAY ||
         name.starts_with(".ctors") ||
         name.starts_with(".dtors") ||
         name.starts_with(".init") ||
         name.starts_with(".fini");
}

// Sections whose names are valid C identifiers can be referenced via
// __start_<name>/__stop_<name> symbols, which the linker synthesizes.
// Such sections must be kept alive only if such a marker symbol is
// referenced from a live section. This map lets us find all sections
// of a given name when we encounter such a reference during marking.
template <typename E>
using StartStopMap =
  tbb::concurrent_unordered_map<std::string_view,
                                tbb::concurrent_vector<InputSection<E> *>>;

template <typename E>
static StartStopMap<E> build_start_stop_map(Context<E> &ctx) {
  StartStopMap<E> map;
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (InputSection<E> *isec : file->sections)
      if (isec && isec->is_alive() && (isec->shdr().sh_flags & SHF_ALLOC))
        if (std::string_view name = isec->name();
            is_c_identifier(name))
          map[name].push_back(isec);
  });
  return map;
}

template <typename E>
static bool mark_section(InputSection<E> *isec) {
  return isec && isec->is_alive() && isec->visit();
}

template <typename E>
static tbb::concurrent_vector<InputSection<E> *>
collect_root_set(Context<E> &ctx) {
  Timer t(ctx, "collect_root_set");
  tbb::concurrent_vector<InputSection<E> *> rootset;

  auto enqueue_section = [&](InputSection<E> *isec) {
    if (mark_section(isec))
      rootset.push_back(isec);
  };

  auto enqueue_symbol = [&](Symbol<E> *sym) {
    if (sym) {
      if (SectionFragment<E> *frag = sym->get_frag())
        frag->is_alive = true;
      else
        enqueue_section(sym->get_input_section());
    }
  };

  // Add sections that are not subject to garbage collection.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (InputSection<E> *isec : file->sections) {
      if (!isec || !isec->is_alive())
        continue;

      // --gc-sections discards only SHF_ALLOC sections. If you want to
      // reduce the amount of non-memory-mapped segments, you should
      // use `strip` command, compile without debug info or use
      // --strip-all linker option.
      if (!(isec->shdr().sh_flags & SHF_ALLOC)) {
        isec->set_visited();
        continue;
      }

      if (should_keep(*isec))
        enqueue_section(isec);
    }
  });

  // Add sections containing gc root or exported symbols
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->symbols)
      if (sym->file == file && (sym->gc_root || sym->is_exported))
        enqueue_symbol(sym);
  });

  // .eh_frame consists of variable-length records called CIE and FDE
  // records, and they are a unit of inclusion or exclusion.
  // We just keep all CIEs and everything that are referenced by them.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (CieRecord<E> &cie : file->cies)
      for (const ElfRel<E> &rel : cie.get_rels())
        enqueue_symbol(file->symbols[rel.r_sym]);
  });

  return rootset;
}

static std::string_view start_stop_name(std::string_view sym) {
  if (sym.starts_with("__start_"))
    return sym.substr(8);
  if (sym.starts_with("__stop_"))
    return sym.substr(7);
  return "";
}

template <typename E>
static void visit_section(Context<E> &ctx, InputSection<E> *isec,
                          tbb::feeder<InputSection<E> *> &feeder, i64 depth,
                          const StartStopMap<E> &start_stop_map) {
  assert(isec->is_visited());

  // Mark a section alive. For better performacne, we don't call
  // `feeder.add` too often.
  auto mark = [&](InputSection<E> *sec) {
    if (mark_section(sec)) {
      if (depth < 3)
        visit_section(ctx, sec, feeder, depth + 1, start_stop_map);
      else
        feeder.add(sec);
    }
  };

  // If this is a text section, .eh_frame may contain records
  // describing how to handle exceptions for that function.
  // We want to keep associated .eh_frame records.
  for (FdeRecord<E> &fde : isec->get_fdes())
    for (const ElfRel<E> &rel : fde.get_rels(*isec->file).subspan(1))
      if (Symbol<E> *sym = isec->file->symbols[rel.r_sym])
        mark(sym->get_input_section());

  for (const ElfRel<E> &rel : isec->get_rels(ctx)) {
    // Symbol can refer to either a section fragment or an input section.
    Symbol<E> &sym = *isec->file->symbols[rel.r_sym];

    if (sym.file && sym.file->is_dso) {
      sym.file->is_reachable.test_and_set();
      continue;
    }

    if (SectionFragment<E> *frag = sym.get_frag()) {
      frag->is_alive = true;
      continue;
    }

    mark(sym.get_input_section());

    // A reference to __start_<name> or __stop_<name> keeps every
    // section named <name> alive, mirroring how those symbols are
    // defined. A single such reference can keep an enormous number of
    // sections alive, so we spread the fanout over threads instead
    // of marking the sections one by one.
    if (std::string_view name = start_stop_name(sym.name());
        !name.empty()) {
      if (auto it = start_stop_map.find(name);
          it != start_stop_map.end()) {
        // Mark and visit the sections with a nested parallel loop.
        // As in mark() below, a section added to a feeder has
        // already been marked, so a feeder's loop body must visit
        // it unconditionally.
        tbb::parallel_for_each(it->second, [&](InputSection<E> *isec) {
          if (mark_section(isec))
            visit_section(ctx, isec, feeder, 0, start_stop_map);
        });
      }
    }
  }

  if constexpr (is_arm32<E>)
    mark(isec->extra.exidx);
}

// Mark all reachable sections
template <typename E>
static void mark(Context<E> &ctx,
                 tbb::concurrent_vector<InputSection<E> *> &rootset,
                 const StartStopMap<E> &start_stop_map) {
  Timer t(ctx, "mark");

  tbb::parallel_for_each(rootset, [&](InputSection<E> *isec,
                                      tbb::feeder<InputSection<E> *> &feeder) {
    visit_section(ctx, isec, feeder, 0, start_stop_map);
  });
}

// Remove unreachable sections
template <typename E>
static void sweep(Context<E> &ctx) {
  Timer t(ctx, "sweep");

  std::vector<std::vector<InputSection<E> *>> sections(ctx.objs.size());

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    ObjectFile<E> &file = *ctx.objs[i];

    for (InputSection<E> *isec : file.sections) {
      if (isec && isec->is_alive() && !isec->is_visited()) {
        isec->kill();
        sections[i].push_back(isec);
      }
    }
  });

  std::string &path = ctx.arg.print_gc_sections;

  if (!path.empty()) {
    std::ostream *out = &std::cout;
    std::ofstream file;

    if (path != "-") {
      file.open(path);
      if (file.fail())
        Fatal(ctx) << "--print-gc-sections: cannot open " << path << ": "
                   << errno_string();
      out = &file;
    }

    i64 saved_bytes = 0;
    for (std::span<InputSection<E> *> vec : sections)
      for (InputSection<E> *isec : vec)
        *out << "removing unused section " << *isec << '\n';

    *out << "GC saved " << saved_bytes << " bytes\n";
  }
}

template <typename E>
void gc_sections(Context<E> &ctx) {
  Timer t(ctx, "gc");

  for (SharedFile<E> *file : ctx.dsos)
    if (file->as_needed)
      file->is_reachable = false;

  for (Symbol<E> *sym : ctx.arg.undefined)
    if (sym->file && sym->file->is_dso)
      sym->file->is_reachable = true;

  for (Symbol<E> *sym : ctx.arg.require_defined)
    if (sym->file && sym->file->is_dso)
      sym->file->is_reachable = true;

  tbb::concurrent_vector<InputSection<E> *> rootset = collect_root_set(ctx);
  StartStopMap<E> start_stop_map = build_start_stop_map(ctx);
  mark(ctx, rootset, start_stop_map);
  sweep(ctx);

  std::erase_if(ctx.dsos, [](SharedFile<E> *file) { return !file->is_reachable; });
}

using E = MOLD_TARGET;

template void gc_sections(Context<E> &ctx);

} // namespace mold
