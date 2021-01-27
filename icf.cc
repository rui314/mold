#include "mold.h"

#include <array>
#include <openssl/rand.h>
#include <openssl/sha.h>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>

static constexpr i64 HASH_SIZE = 16;

static bool is_eligible(InputSection &isec) {
  return (isec.shdr.sh_flags & SHF_ALLOC) &&
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

static void gather_sections(std::vector<InputSection *> &sections,
                            std::vector<std::array<u8, HASH_SIZE>> &digests,
                            std::vector<u32> &indices,
                            std::vector<u32> &edges) {
  // Compute the sizes of sections and digests.
  std::vector<i64> num_sections1(out::objs.size());
  std::vector<i64> num_sections2(out::objs.size());

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    for (InputSection *isec : out::objs[i]->sections) {
      if (!isec || isec->shdr.sh_type == SHT_NOBITS)
        continue;

      if (is_eligible(*isec))
        num_sections1[i]++;
      else
        num_sections2[i]++;
    }
  });

  std::vector<i64> section_idx1(out::objs.size());
  std::vector<i64> section_idx2(out::objs.size());

  for (i64 i = 0; i < out::objs.size() + 1; i++)
    section_idx1[i + 1] = section_idx1[i] + num_sections1[i];

  section_idx2[0] = section_idx1.back();

  for (i64 i = 0; i < out::objs.size() + 1; i++)
    section_idx2[i + 1] = section_idx2[i] + num_sections2[i];

  sections.resize(section_idx2.back());
  digests.resize(section_idx2.back());

  // Fill sections and digests.
  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    i64 idx1 = section_idx1[i];
    i64 idx2 = section_idx2[i];

    for (InputSection *isec : out::objs[i]->sections) {
      if (!isec || isec->shdr.sh_type == SHT_NOBITS)
        continue;

      if (is_eligible(*isec)) {
        sections[idx1] = isec;
        digests[idx1] = compute_digest(*isec);
        idx1++;
      } else {
        sections[idx2] = isec;
        assert(RAND_bytes(digests[idx2].data(), HASH_SIZE) == 1);
        idx2++;
      }
    }
  });

  // Initialize indices.
  std::vector<i64> num_edges(section_idx1.back());

  tbb::parallel_for((i64)0, section_idx1.back(), [&](i64 i) {
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

  indices.resize(num_edges.size() + 1);
  for (i64 i = 0; i < indices.size() + 1; i++)
    indices[i + 1] += indices[i] + num_edges[i];

  // Initialize edges.
  edges.resize(indices.back());
  indices.resize(indices.size() - 1);
}

void icf_sections() {
  Timer t("icf");

  std::vector<InputSection *> sections;
  std::vector<std::array<u8, HASH_SIZE>> digests;
  std::vector<u32> indices;
  std::vector<u32> edges;

  gather_sections(sections, digests, indices, edges);
}
