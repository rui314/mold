#include "mold.h"

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

static void compute_digest(InputSection &isec, u8 *digest) {
  SHA256_CTX ctx;
  SHA256_Init(&ctx);

  std::string_view contents = isec.get_contents();
  SHA256_Update(&ctx, contents.data(), contents.size());
  SHA256_Update(&ctx, &isec.shdr.sh_flags, sizeof(isec.shdr.sh_flags));

  for (ElfRela &rel : isec.rels) {
    SHA256_Update(&ctx, &rel.r_offset, sizeof(rel.r_offset));
    SHA256_Update(&ctx, &rel.r_type, sizeof(rel.r_type));
    SHA256_Update(&ctx, &rel.r_addend, sizeof(rel.r_addend));

    Symbol &sym = *isec.file->symbols[rel.r_sym];

    if (SectionFragment *frag = sym.fragref.frag) {
      SHA256_Update(&ctx, frag->data.data(), frag->data.size());
      SHA256_Update(&ctx, &sym.fragref.addend, sizeof(sym.fragref.addend));
    } else if (!sym.input_section) {
      SHA256_Update(&ctx, &sym.value, sizeof(sym.value));
    }
  }

  u8 tmp[32];
  assert(SHA256_Final(tmp, &ctx) == 1);
  memcpy(digest, tmp, HASH_SIZE);
}

struct Pair {
  InputSection *isec;
  u8 digest[HASH_SIZE];
};

static void gather_sections(std::vector<Pair> &eligibles,
                            std::vector<Pair> &non_eligibles) {
  std::vector<i64> num_eligibles(out::objs.size());
  std::vector<i64> num_non_eligibles(out::objs.size());

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    for (InputSection *isec : out::objs[i]->sections) {
      if (!isec || isec->shdr.sh_type == SHT_NOBITS)
        continue;

      if (is_eligible(*isec))
        num_eligibles[i]++;
      else
        num_non_eligibles[i]++;
    }
  });

  std::vector<i64> eligible_indices(out::objs.size());
  std::vector<i64> non_eligible_indices(out::objs.size());

  for (i64 i = 0; i < out::objs.size() + 1; i++) {
    eligible_indices[i + 1] = eligible_indices[i] + num_eligibles[i];
    non_eligible_indices[i + 1] = non_eligible_indices[i] + num_non_eligibles[i];
  }

  eligibles.resize(eligible_indices.back());
  non_eligibles.resize(non_eligible_indices.back());

  tbb::parallel_for((i64)0, (i64)out::objs.size(), [&](i64 i) {
    i64 idx1 = eligible_indices[i];
    i64 idx2 = non_eligible_indices[i];

    for (InputSection *isec : out::objs[i]->sections) {
      if (!isec || isec->shdr.sh_type == SHT_NOBITS)
        continue;

      if (is_eligible(*isec)) {
        eligibles[idx1].isec = isec;
        compute_digest(*isec, eligibles[idx1].digest);
        idx1++;
      } else {
        non_eligibles[idx2].isec = isec;
        compute_digest(*isec, non_eligibles[idx2].digest);
        idx2++;
      }
    }
  });
}

void icf_sections() {
  Timer t("icf");

  std::vector<Pair> eligibles;
  std::vector<Pair> non_eligibles;
  gather_sections(eligibles, non_eligibles);
}
