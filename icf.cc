#include "mold.h"

static bool is_eligible(InputSection *isec) {
  return (isec->shdr.sh_flags & SHF_ALLOC) &&
         !(isec->shdr.sh_flags & SHF_WRITE) &&
         !(isec->shdr.sh_type == SHT_INIT_ARRAY || isec->name == ".init") &&
         !(isec->shdr.sh_type == SHT_FINI_ARRAY || isec->name == ".fini");
}
