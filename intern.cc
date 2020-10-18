#include "chibild.h"

namespace tbb {
template<>
struct tbb_hash<StringRef> {
  size_t operator()(const StringRef &k) const {
    return llvm::hash_value(k);
  }
};
}

InternedString::InternedString(StringRef s) {
  if (s.empty())
    return;

  static tbb::concurrent_unordered_set<StringRef> set;

  auto it = set.insert(s);
  data_ = it.first->data();
  size_ = it.first->size();
}
