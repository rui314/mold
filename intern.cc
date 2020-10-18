#include "chibild.h"

namespace tbb {
template<>
struct tbb_hash<StringRef> {
  size_t operator()(const StringRef &k) const {
    return llvm::hash_value(k);
  }
};
}

InternedString intern(StringRef s) {
  if (s.empty())
    return {nullptr, 0};

  static tbb::concurrent_unordered_set<StringRef> set;

  auto it = set.insert(s);
  return {it.first->data(), it.first->size()};
}
