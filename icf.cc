#include "mold.h"

#include <array>
#include <mutex>
#include <openssl/rand.h>
#include <openssl/sha.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>
#include <tbb/partitioner.h>

static constexpr i64 HASH_SIZE = 16;

static bool is_eligible(InputSection &isec) {
  return (isec.shdr.sh_flags & SHF_ALLOC) &&
         (isec.shdr.sh_type != SHT_NOBITS) &&
         !(isec.shdr.sh_flags & SHF_WRITE) &&
         !(isec.shdr.sh_type == SHT_INIT_ARRAY || isec.name == ".init") &&
         !(isec.shdr.sh_type == SHT_FINI_ARRAY || isec.name == ".fini");
}

static void update_string(SHA256_CTX &ctx, std::string_view str) {
  u64 size = str.size();
  SHA256_Update(&ctx, &size, 8);
  SHA256_Update(&ctx, str.data(), str.size());
}

static void update_i64(SHA256_CTX &ctx, i64 val) {
  SHA256_Update(&ctx, &val, 8);
}

static std::array<u8, HASH_SIZE> digest_final(SHA256_CTX &ctx) {
  u8 digest[SHA256_SIZE];
  assert(SHA256_Final(digest, &ctx) == 1);

  std::array<u8, HASH_SIZE> arr;
  memcpy(arr.data(), digest, HASH_SIZE);
  return arr;
}

static std::array<u8, HASH_SIZE> compute_digest(InputSection &isec) {
  SHA256_CTX ctx;
  SHA256_Init(&ctx);

  update_string(ctx, isec.get_contents());
  update_i64(ctx, isec.shdr.sh_flags);
  update_i64(ctx, isec.rels.size());

  i64 ref_idx = 0;

  for (i64 i = 0; i < isec.rels.size(); i++) {
    ElfRela &rel = isec.rels[i];
    update_i64(ctx, rel.r_offset);
    update_i64(ctx, rel.r_type);
    update_i64(ctx, rel.r_addend);

    if (isec.has_fragments[i]) {
      SectionFragmentRef &ref = isec.rel_fragments[ref_idx++];
      update_i64(ctx, 1);
      update_i64(ctx, ref.addend);
      update_string(ctx, ref.frag->data);
      continue;
    }

    Symbol &sym = *isec.file->symbols[rel.r_sym];

    if (SectionFragment *frag = sym.fragref.frag) {
      update_i64(ctx, 2);
      update_i64(ctx, sym.fragref.addend);
      update_string(ctx, frag->data);
    } else if (!sym.input_section) {
      update_i64(ctx, 3);
      update_i64(ctx, sym.value);
    } else {
      update_i64(ctx, 4);
    }
  }

  return digest_final(ctx);
}

static std::array<u8, HASH_SIZE> get_random_bytes() {
  std::array<u8, HASH_SIZE> arr;
  assert(RAND_bytes(arr.data(), HASH_SIZE) == 1);
  return arr;
}

struct Entry {
  InputSection *isec;
  bool is_eligible;
  std::array<u8, HASH_SIZE> digest;
};

static void gather_sections(std::vector<InputSection *> &sections,
                            std::vector<std::array<u8, HASH_SIZE>> &digests,
                            std::vector<u32> &edges,
                            std::vector<u32> &edge_indices) {
  std::vector<i64> num_sections(out::objs.size());

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    for (InputSection *isec : out::objs[i]->sections)
      if (isec)
        num_sections[i]++;
  });

  std::vector<i64> section_indices(out::objs.size() + 1);
  for (i64 i = 0; i < out::objs.size(); i++)
    section_indices[i + 1] = section_indices[i] + num_sections[i];

  std::vector<Entry> entries(section_indices.back());
  tbb::enumerable_thread_specific<i64> num_eligibles;

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    i64 idx = section_indices[i];
    for (InputSection *isec : out::objs[i]->sections) {
      if (isec) {
        Entry &ent = entries[idx++];
        ent.isec = isec;
        ent.is_eligible = is_eligible(*isec);
        ent.digest = ent.is_eligible ? compute_digest(*isec) : get_random_bytes();

        if (ent.is_eligible)
          num_eligibles.local() += 1;
      }
    }
  });

  tbb::parallel_sort(entries.begin(), entries.end(),
                     [](const Entry &a, const Entry &b) {
                       if (!a.is_eligible || !b.is_eligible)
                         return a.is_eligible && !b.is_eligible;
                       return a.digest < b.digest;
                     });

  // Initialize sections and digests
  sections.reserve(entries.size());
  digests.reserve(entries.size());

  for (Entry &ent : entries) {
    sections.push_back(ent.isec);
    digests.push_back(std::move(ent.digest));
  }

  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    sections[i]->icf_idx = i;
  });

  // Initialize edges and edge_indices
  std::vector<i64> num_edges(num_eligibles.combine(std::plus()));

  tbb::parallel_for((i64)0, (i64)num_edges.size(), [&](i64 i) {
    assert(entries[i].is_eligible);
    InputSection &isec = *sections[i];

    for (i64 j = 0; j < isec.rels.size(); j++) {
      if (isec.has_fragments[j])
        continue;

      ElfRela &rel = isec.rels[j];
      Symbol &sym = *isec.file->symbols[rel.r_sym];
      if (!sym.fragref.frag && sym.input_section)
        num_edges[i]++;
    }
  });

  edge_indices.resize(num_edges.size());
  for (i64 i = 0; i < num_edges.size() - 1; i++)
    edge_indices[i + 1] = edge_indices[i] + num_edges[i];

  edges.resize(edge_indices.back() + num_edges.back());

  tbb::parallel_for((i64)0, (i64)num_edges.size(), [&](i64 i) {
    InputSection &isec = *sections[i];
    i64 idx = edge_indices[i];

    for (i64 j = 0; j < isec.rels.size(); j++) {
      if (isec.has_fragments[j])
        continue;

      ElfRela &rel = isec.rels[j];
      Symbol &sym = *isec.file->symbols[rel.r_sym];

      if (!sym.fragref.frag && sym.input_section) {
        assert(sym.input_section->icf_idx != -1);
        edges[idx++] = sym.input_section->icf_idx;
      }
    }
  });
}

void icf_sections() {
  Timer t("icf");

  std::vector<InputSection *> sections;
  std::vector<std::array<u8, HASH_SIZE>> digests0;
  std::vector<u32> edges;
  std::vector<u32> edge_indices;

  gather_sections(sections, digests0, edges, edge_indices);

  std::vector<std::vector<std::array<u8, HASH_SIZE>>> digests(2);
  digests[0] = std::move(digests0);
  digests[1] = digests[0];

  i64 slot = 0;
  i64 num_eligibles = edge_indices.size();

  auto count_num_classes = [&]() {
    tbb::enumerable_thread_specific<i64> num_classes;
    tbb::parallel_for((i64)0, num_eligibles - 1, [&](i64 i) {
      if (digests[slot][i] != digests[slot][i + 1])
        num_classes.local() += 1;
    });
    return num_classes.combine(std::plus());
  };

  i64 num_classes = count_num_classes();

  Timer t2("rounds");

  for (;;) {
    tbb::parallel_for((i64)0, num_eligibles, [&](i64 i) {
      SHA256_CTX ctx;
      SHA256_Init(&ctx);
      SHA256_Update(&ctx, digests[slot][i].data(), HASH_SIZE);

      i64 begin = edge_indices[i];
      i64 end = (i + 1 == num_eligibles) ? edges.size() : edge_indices[i + 1];
      for (i64 j = begin; j < end; j++)
        SHA256_Update(&ctx, digests[slot][edges[j]].data(), HASH_SIZE);

      digests[slot ^ 1][i] = digest_final(ctx);
    });

    slot ^= 1;

    i64 n = count_num_classes();
    if (n == num_classes)
      break;
    num_classes = n;
  }
}
