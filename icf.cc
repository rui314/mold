#include "mold.h"

#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>
#include <tbb/partitioner.h>

static i64 slot = 0;

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
    if (sym.input_section) {
      if (sym.input_section->eq_class[slot] & 1)
        sym.input_section->eq_class[slot ^ 1] += isec.eq_class[slot] << 1;
    }
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

    if (!symx.input_section || !symy.input_section) {
      if (symx.value != symy.value)
        return false;
      continue;
    }

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

      if (is_eligible(*isec))
        isec->eq_class[0] = hash(*isec) | 1;
      else
        isec->eq_class[0] = (u64)isec & ~(u64)1;
    }
  });

  {
    Timer t2("propagate");
    for (i64 i = 0; i < 1; i++) {
      tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
        for (InputSection *isec : file->sections)
          if (isec)
            isec->eq_class[slot ^ 1] = isec->eq_class[slot].load();
      });

      tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
        for (InputSection *isec : file->sections)
          if (isec)
            propagate(*isec);
      });
      slot ^= 1;
    }
  }

  std::vector<InputSection *> vec;

  {
    Timer t2("gather");

    std::vector<i64> sizes(out::objs.size());
    tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
      for (InputSection *isec : out::objs[i]->sections)
        if (isec && is_eligible(*isec))
          sizes[i]++;
    });

    std::vector<i64> indices(out::objs.size() + 1);
    for (i64 i = 0; i < sizes.size() + 1; i++)
      indices[i + 1] = indices[i] + sizes[i];

    vec.resize(indices.back());

    tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
      i64 j = indices[i];
      for (InputSection *isec : out::objs[i]->sections)
        if (isec && is_eligible(*isec))
          vec[j++] = isec;
    });

    tbb::parallel_sort(vec.begin(), vec.end(),
                       [](InputSection *a, InputSection *b) {
                         return a->eq_class[slot] < b->eq_class[slot];
                       });
  }

  i64 count = 0;
  i64 non_eq = 0;

  for (i64 i = 0; i < vec.size();) {
    i64 j = i + 1;
    while (j < vec.size() && vec[i]->eq_class[slot] == vec[j]->eq_class[slot])
      j++;
    if (j != i + 1)
      count++;

    for (i64 k = i + 1; k < j; k++) {
      if (!equal(*vec[i], *vec[k])) {
        non_eq++;
        break;
      }
    }

    i = j;
  }
  SyncOut() << "count=" << count << " non_eq=" << non_eq;
}
