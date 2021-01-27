#include "mold.h"

#include <array>
#include <openssl/rand.h>
#include <openssl/sha.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

static constexpr i64 HASH_SIZE = 16;

static bool is_eligible(InputSection &isec) {
  return (isec.shdr.sh_flags & SHF_ALLOC) &&
         (isec.shdr.sh_type != SHT_NOBITS) &&
         !(isec.shdr.sh_flags & SHF_WRITE) &&
         !(isec.shdr.sh_type == SHT_INIT_ARRAY || isec.name == ".init") &&
         !(isec.shdr.sh_type == SHT_FINI_ARRAY || isec.name == ".fini");
}

static std::array<u8, HASH_SIZE> compute_digest(InputSection &isec) {
  SHA256_CTX ctx;
  SHA256_Init(&ctx);

  std::string_view contents = isec.get_contents();
  SHA256_Update(&ctx, contents.data(), contents.size());
  SHA256_Update(&ctx, &isec.shdr.sh_flags, sizeof(isec.shdr.sh_flags));

  for (ElfRela &rel : isec.rels) {
    SHA256_Update(&ctx, &rel.r_offset, sizeof(rel.r_offset));
    SHA256_Update(&ctx, &rel.r_type, sizeof(rel.r_type));
    SHA256_Update(&ctx, &rel.r_addend, sizeof(rel.r_addend));

    // TODO: handle rel_fragments

    Symbol &sym = *isec.file->symbols[rel.r_sym];

    if (SectionFragment *frag = sym.fragref.frag) {
      SHA256_Update(&ctx, frag->data.data(), frag->data.size());
      SHA256_Update(&ctx, &sym.fragref.addend, sizeof(sym.fragref.addend));
    } else if (!sym.input_section) {
      SHA256_Update(&ctx, &sym.value, sizeof(sym.value));
    }
  }

  u8 digest[32];
  assert(SHA256_Final(digest, &ctx) == 1);

  std::array<u8, HASH_SIZE> arr;
  memcpy(arr.data(), digest, HASH_SIZE);
  return arr;
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

  // Initialize edges and edge_indices
  std::vector<i64> num_edges(num_eligibles.combine(std::plus()));

  tbb::parallel_for((i64)0, (i64)num_edges.size(), [&](i64 i) {
    assert(entries[i].is_eligible);
    InputSection &isec = *sections[i];

    for (i64 j = 0; j < isec.rels.size(); j++) {
      if (isec.has_fragments[i])
        continue;

      ElfRela &rel = isec.rels[j];
      Symbol &sym = *isec.file->symbols[rel.r_sym];
      if (sym.input_section)
        num_edges[i]++;
    }
  });

  edge_indices.resize(num_edges.size());
  for (i64 i = 0; i < num_edges.size() - 1; i++)
    edge_indices[i + 1] = edge_indices[i] + num_edges[i];

  edges.resize(edge_indices.back() + num_edges.back());

  tbb::parallel_for((i64)0, (i64)num_edges.size(), [&](i64 i) {
    sections[i]->icf_idx = i;
  });

  tbb::parallel_for((i64)0, (i64)num_edges.size(), [&](i64 i) {
    InputSection &isec = *sections[i];
    i64 idx = edge_indices[i];

    for (i64 j = 0; j < isec.rels.size(); j++) {
      if (isec.has_fragments[i])
        continue;

      ElfRela &rel = isec.rels[j];
      Symbol &sym = *isec.file->symbols[rel.r_sym];
      if (sym.input_section)
        edges[idx++] = sym.input_section->icf_idx;
    }
  });
}

void icf_sections() {
  Timer t("icf");

  std::vector<InputSection *> sections;
  std::vector<std::array<u8, HASH_SIZE>> digests;
  std::vector<u32> indices;
  std::vector<u32> edges;

  gather_sections(sections, digests, indices, edges);
}
