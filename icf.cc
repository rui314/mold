#include "mold.h"

#include <array>
#include <openssl/sha.h>
#include <tbb/concurrent_unordered_map.h>
#include <tbb/concurrent_vector.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

static constexpr i64 HASH_SIZE = 16;

typedef std::array<u8, HASH_SIZE> Digest;

namespace tbb {
template<> struct tbb_hash<Digest> {
  size_t operator()(const Digest &k) const {
    return *(i64 *)&k[0];
  }
};
}

static bool cie_equal(const CieRecord &a, const CieRecord &b) {
  return a.contents == b.contents && a.rels == b.rels;
}

static void uniquify_cies() {
  Timer t("uniquify_cies");
  std::vector<CieRecord *> cies;

  for (ObjectFile *file : out::objs) {
    for (CieRecord &cie : file->cies) {
      for (i64 i = 0; i < cies.size(); i++) {
        if (cie_equal(cie, *cies[i])) {
          cie.icf_idx = i;
          goto found;
        }
      }
      cie.icf_idx = cies.size();
      cies.push_back(&cie);
    found:;
    }
  }
}

static bool is_eligible(InputSection &isec) {
  bool is_alloc = (isec.shdr.sh_flags & SHF_ALLOC);
  bool is_executable = (isec.shdr.sh_flags & SHF_EXECINSTR);
  bool is_relro = (isec.name == ".data.rel.ro" ||
                   isec.name.starts_with(".data.rel.ro."));
  bool is_writable = (isec.shdr.sh_flags & SHF_WRITE) && !is_relro;
  bool is_bss = (isec.shdr.sh_type == SHT_NOBITS);
  bool is_empty = (isec.shdr.sh_size == 0);
  bool is_init = (isec.shdr.sh_type == SHT_INIT_ARRAY || isec.name == ".init");
  bool is_fini = (isec.shdr.sh_type == SHT_FINI_ARRAY || isec.name == ".fini");
  bool is_enumerable = is_c_identifier(isec.name);

  return is_alloc && is_executable && !is_writable && !is_bss &&
         !is_empty && !is_init && !is_fini && !is_enumerable;
}

static Digest digest_final(SHA256_CTX &ctx) {
  u8 arr[SHA256_SIZE];
  assert(SHA256_Final(arr, &ctx) == 1);

  Digest digest;
  memcpy(digest.data(), arr, HASH_SIZE);
  return digest;
}

static bool is_leaf(InputSection &isec) {
  if (!isec.rels.empty())
    return false;

  for (FdeRecord &fde : isec.fdes)
    if (fde.rels.size() > 1)
      return false;

  return true;
}

struct LeafHasher {
  size_t operator()(const InputSection *isec) const {
    size_t h = std::hash<std::string_view>()(isec->get_contents());
    for (FdeRecord &fde : isec->fdes) {
      size_t h2 = std::hash<std::string_view>()(fde.contents.substr(8));
      h ^= h2 + 0x9e3779b9 + (h << 6) + (h >> 2);
    }
    return h;
  }
};

struct LeafEq {
  bool operator()(const InputSection *a, const InputSection *b) const {
    if (a->get_contents() != b->get_contents())
      return false;
    if (a->fdes.size() != b->fdes.size())
      return false;
    for (i64 i = 0; i < a->fdes.size(); i++) {
      if (a->fdes[i].contents.size() != b->fdes[i].contents.size())
        return false;
      if (a->fdes[i].contents.substr(8) != b->fdes[i].contents.substr(8))
        return false;
    }
    return true;
  }
};

static void merge_leaf_nodes() {
  Timer t("merge_leaf_nodes");

  static Counter eligible("icf_eligibles");
  static Counter non_eligible("icf_non_eligibles");
  static Counter leaf("icf_leaf_nodes");

  tbb::concurrent_unordered_map<InputSection *, InputSection *,
                                LeafHasher, LeafEq> map;

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    for (InputSection *isec : out::objs[i]->sections) {
      if (!isec)
        continue;

      if (!is_eligible(*isec)) {
        non_eligible++;
        continue;
      }

      if (is_leaf(*isec)) {
        leaf++;
        isec->icf_leaf = true;
        auto [it, inserted] = map.insert({isec, isec});
        if (!inserted && isec->get_priority() < it->second->get_priority())
          it->second = isec;
      } else {
        eligible++;
        isec->icf_eligible = true;
      }
    }
  });

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    for (InputSection *isec : out::objs[i]->sections) {
      if (isec && isec->icf_leaf) {
        auto it = map.find(isec);
        assert(it != map.end());
        isec->leader = it->second;
      }
    }
  });
}

static Digest compute_digest(InputSection &isec) {
  SHA256_CTX ctx;
  SHA256_Init(&ctx);

  auto hash = [&](auto val) {
    SHA256_Update(&ctx, &val, sizeof(val));
  };

  auto hash_string = [&](std::string_view str) {
    hash(str.size());
    SHA256_Update(&ctx, str.data(), str.size());
  };

  auto hash_symbol = [&](Symbol &sym) {
    InputSection *isec = sym.input_section;

    if (SectionFragment *frag = sym.frag) {
      hash('2');
      hash_string(frag->data);
    } else if (!isec) {
      hash('3');
    } else if (isec->leader) {
      hash('4');
      hash(isec->leader->get_priority());
    } else if (isec->icf_eligible) {
      hash('5');
    } else {
      hash('6');
      hash(isec->get_priority());
    }
    hash(sym.value);
  };

  hash_string(isec.get_contents());
  hash(isec.shdr.sh_flags);
  hash(isec.fdes.size());
  hash(isec.rels.size());

  for (FdeRecord &fde : isec.fdes) {
    hash(isec.file->cies[fde.cie_idx].icf_idx);

    // Bytes 0 to 4 contain the length of this record, and
    // bytes 4 to 8 contain an offset to CIE.
    hash_string(fde.contents.substr(8));

    hash(fde.rels.size());

    for (EhReloc &rel : std::span(fde.rels).subspan(1)) {
      hash_symbol(rel.sym);
      hash(rel.type);
      hash(rel.offset);
      hash(rel.addend);
    }
  }

  i64 ref_idx = 0;

  for (i64 i = 0; i < isec.rels.size(); i++) {
    ElfRela &rel = isec.rels[i];
    hash(rel.r_offset);
    hash(rel.r_type);
    hash(rel.r_addend);

    if (isec.has_fragments[i]) {
      SectionFragmentRef &ref = isec.rel_fragments[ref_idx++];
      hash('1');
      hash(ref.addend);
      hash_string(ref.frag->data);
    } else {
      hash_symbol(*isec.file->symbols[rel.r_sym]);
    }
  }

  return digest_final(ctx);
}

static std::vector<InputSection *> gather_sections() {
  Timer t("gather_sections");

  // Count the number of input sections for each input file.
  std::vector<i64> num_sections(out::objs.size());

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    for (InputSection *isec : out::objs[i]->sections)
      if (isec && isec->icf_eligible)
        num_sections[i]++;
  });

  std::vector<i64> section_indices(out::objs.size());
  for (i64 i = 0; i < out::objs.size() - 1; i++)
    section_indices[i + 1] = section_indices[i] + num_sections[i];

  std::vector<InputSection *> sections(section_indices.back() + num_sections.back());

  // Fill `sections` contents.
  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    i64 idx = section_indices[i];
    for (InputSection *isec : out::objs[i]->sections)
      if (isec && isec->icf_eligible)
        sections[idx++] = isec;
  });

  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    sections[i]->icf_idx = i;
  });

  return sections;
}

static std::vector<Digest> compute_digests(std::span<InputSection *> sections) {
  Timer t("compute_digests");

  std::vector<Digest> digests(sections.size());
  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    digests[i] = compute_digest(*sections[i]);
  });
  return digests;
}

static void gather_edges(std::span<InputSection *> sections,
                         std::vector<u32> &edges, std::vector<u32> &edge_indices) {
  Timer t("gather_edges");

  std::vector<i64> num_edges(sections.size());
  edge_indices.resize(sections.size());

  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    InputSection &isec = *sections[i];
    assert(isec.icf_eligible);

    for (i64 j = 0; j < isec.rels.size(); j++) {
      if (!isec.has_fragments[j]) {
        ElfRela &rel = isec.rels[j];
        Symbol &sym = *isec.file->symbols[rel.r_sym];
        if (!sym.frag && sym.input_section && sym.input_section->icf_eligible)
          num_edges[i]++;
      }
    }
  });

  for (i64 i = 0; i < num_edges.size() - 1; i++)
    edge_indices[i + 1] = edge_indices[i] + num_edges[i];

  edges.resize(edge_indices.back() + num_edges.back());

  tbb::parallel_for((i64)0, (i64)num_edges.size(), [&](i64 i) {
    InputSection &isec = *sections[i];
    i64 idx = edge_indices[i];

    for (i64 j = 0; j < isec.rels.size(); j++) {
      if (!isec.has_fragments[j]) {
        ElfRela &rel = isec.rels[j];
        Symbol &sym = *isec.file->symbols[rel.r_sym];
        if (!sym.frag && sym.input_section && sym.input_section->icf_eligible)
          edges[idx++] = sym.input_section->icf_idx;
      }
    }
  });
}

static i64 propagate(std::span<std::vector<Digest>> digests,
                     std::span<u32> edges, std::span<u32> edge_indices,
                     bool &slot, tbb::affinity_partitioner &ap) {
  static Counter round("icf_round");
  round++;

  i64 num_digests = digests[0].size();
  tbb::enumerable_thread_specific<i64> changed;

  tbb::parallel_for((i64)0, num_digests, [&](i64 i) {
    if (digests[slot][i] == digests[!slot][i])
      return;

    SHA256_CTX ctx;
    SHA256_Init(&ctx);
    SHA256_Update(&ctx, digests[2][i].data(), HASH_SIZE);

    i64 begin = edge_indices[i];
    i64 end = (i + 1 == num_digests) ? edges.size() : edge_indices[i + 1];

    for (i64 j = begin; j < end; j++)
      SHA256_Update(&ctx, digests[slot][edges[j]].data(), HASH_SIZE);

    digests[!slot][i] = digest_final(ctx);

    if (digests[slot][i] != digests[!slot][i])
      changed.local()++;
  }, ap);

  slot = !slot;
  return changed.combine(std::plus());
}

static i64 count_num_classes(std::span<Digest> digests) {
  std::vector<Digest> vec(digests.begin(), digests.end());
  tbb::parallel_sort(vec);

  tbb::enumerable_thread_specific<i64> num_classes;
  tbb::parallel_for((i64)0, (i64)vec.size() - 1, [&](i64 i) {
    if (vec[i] != vec[i + 1])
      num_classes.local()++;
  });
  return num_classes.combine(std::plus());
}

static void print_icf_sections() {
  tbb::concurrent_vector<InputSection *> leaders;
  tbb::concurrent_unordered_multimap<InputSection *, InputSection *> map;

  tbb::parallel_for_each(out::objs, [&](ObjectFile *file) {
    for (InputSection *isec : file->sections) {
      if (isec && isec->leader) {
        if (isec == isec->leader)
          leaders.push_back(isec);
        else
          map.insert({isec->leader, isec});
      }
    }
  });

  tbb::parallel_sort(leaders.begin(), leaders.end(),
                     [&](InputSection *a, InputSection *b) {
                       return a->get_priority() < b->get_priority();
                     });

  i64 saved_bytes = 0;

  for (InputSection *leader : leaders) {
    auto [begin, end] = map.equal_range(leader);
    if (begin == end)
      continue;

    SyncOut() << "selected section " << *leader;

    i64 n = 0;
    for (auto it = begin; it != end; it++) {
      SyncOut() << "  removing identical section " << *it->second;
      n++;
    }
    saved_bytes += leader->get_contents().size() * n;
  }

  SyncOut() << "ICF saved " << saved_bytes << " bytes";
}

void icf_sections() {
  Timer t("icf");

  uniquify_cies();
  merge_leaf_nodes();

  // Prepare for the propagation rounds.
  std::vector<InputSection *> sections = gather_sections();

  std::vector<std::vector<Digest>> digests(3);
  digests[0] = compute_digests(sections);
  digests[1].resize(digests[0].size());
  digests[2] = digests[0];

  std::vector<u32> edges;
  std::vector<u32> edge_indices;
  gather_edges(sections, edges, edge_indices);

  bool slot = 0;

  // Execute the propagation rounds until convergence is obtained.
  {
    Timer t("propagate");
    tbb::affinity_partitioner ap;

    i64 num_changed = -1;
    for (;;) {
      i64 n = propagate(digests, edges, edge_indices, slot, ap);
      if (n == num_changed)
        break;
      num_changed = n;
    }

    i64 num_classes = -1;
    for (;;) {
      for (i64 i = 0; i < 10; i++)
        propagate(digests, edges, edge_indices, slot, ap);

      i64 n = count_num_classes(digests[slot]);
      if (n == num_classes)
        break;
      num_classes = n;
    }
  }

  // Group sections by SHA digest.
  {
    Timer t("group");

    auto *map = new tbb::concurrent_unordered_map<Digest, InputSection *>;
    std::span<Digest> digest = digests[slot];

    tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
      InputSection *isec = sections[i];
      auto [it, inserted] = map->insert({digest[i], isec});
      if (!inserted && isec->get_priority() < it->second->get_priority())
        it->second = isec;
    });

    tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
      auto it = map->find(digest[i]);
      assert(it != map->end());
      sections[i]->leader = it->second;
    });
  }

  if (config.print_icf_sections)
    print_icf_sections();

  // Re-assign input sections to symbols.
  {
    Timer t("reassign");
    tbb::parallel_for_each(out::objs, [](ObjectFile *file) {
      for (Symbol *sym : file->symbols) {
        if (sym->file != file)
          continue;
        InputSection *isec = sym->input_section;
        if (isec && isec->leader && isec->leader != isec) {
          sym->input_section = isec->leader;
          isec->kill();
        }
      }
    });
  }
}
