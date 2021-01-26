#include "mold.h"

#include <tbb/parallel_for_each.h>
#include <tbb/partitioner.h>
#include <tbb/task_arena.h>

static i64 slot = 0;
static thread_local i64 noneligible_id = 0;

static bool is_eligible(InputSection &isec) {
  return (isec.shdr.sh_flags & SHF_ALLOC) &&
         !(isec.shdr.sh_flags & SHF_WRITE) &&
         !(isec.shdr.sh_type == SHT_INIT_ARRAY || isec.name == ".init") &&
         !(isec.shdr.sh_type == SHT_FINI_ARRAY || isec.name == ".fini");
}

static u64 hash(u64 x) {
  x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9;
  x = (x ^ (x >> 27)) * 0x94d049bb133111eb;
  return x ^ (x >> 31);
}

static u64 hash(std::string_view x) {
  return std::hash<std::string_view>()(x);
}

template <typename T>
inline void hash_combine(u64 &seed, const T &val) {
  seed ^= hash(val) + 0x9e3779b9 + (seed << 6) + (seed >> 2);
}

static u64 hash(InputSection &isec) {
  u64 hv = 0;
  hash_combine(hv, isec.shdr.sh_flags); 
  hash_combine(hv, isec.rels.size());
  hash_combine(hv, isec.get_contents());

  for (ElfRela &rel : isec.rels) {
    hash_combine(hv, rel.r_offset);
    hash_combine(hv, rel.r_type);
    hash_combine(hv, rel.r_addend);
  }
  return hv;
}

static void propagate(InputSection &isec) {
  for (ElfRela &rel : isec.rels) {
    Symbol &sym = *isec.file->symbols[rel.r_sym];
    if (sym.input_section)
      sym.input_section->eq_class[slot] += isec.eq_class[slot ^ 1];
  }
}

static bool equal(InputSection &x, InputSection &y) {
  if (x.shdr.sh_flags != y.shdr.sh_flags)
    return false;
  if (x.rels.size() != y.rels.size())
    return false;
  if (x.get_contents() != y.get_contents())
    return false;

  for (i64 i = 0; i < x.rels.size(); i++) {
    ElfRela &relx = x.rels[i];
    ElfRela &rely = y.rels[i];

    if (relx.r_offset != rely.r_offset)
      return false;
    if (relx.r_type != rely.r_type)
      return false;
    if (relx.r_addend != rely.r_addend)
      return false;

    Symbol &symx = *x.file->symbols[relx.r_sym];
    Symbol &symy = *y.file->symbols[rely.r_sym];
    if (&symx == &symy)
      continue;

    if (!symx.fragref.frag ^ !symx.fragref.frag)
      return false;

    if (symx.fragref.frag)
      if (symx.fragref.frag != symy.fragref.frag ||
          symx.fragref.addend != symy.fragref.addend)
        return false;

    if (!symx.input_section ^ !symx.input_section)
      return false;

    InputSection &isecx = *symx.input_section;
    InputSection &isecy = *symy.input_section;
    if (isecx.eq_class[slot] != isecy.eq_class[slot])
      return false;
  }
  return true;
}

void icf_sections() {
  Timer t("icf");

  tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
    for (InputSection *isec : file->sections) {
      if (!isec)
        continue;

      if (is_eligible(*isec)) {
        isec->eq_class[0] = hash(*isec) | 1;
      } else {
        i64 tid = tbb::task_arena::current_thread_index();
        isec->eq_class[0] = (tid << 32) | (noneligible_id++ << 1);
      }
    }
  });

  slot ^= 1;

  {
    Timer t2("propagate");
    for (i64 i = 0; i < 2; i++) {
      tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
        for (InputSection *isec : file->sections)
          if (isec)
            propagate(*isec);
      });
      slot ^= 1;
    }
  }
}
