#include "chibild.h"

namespace tbb {
template<>
struct tbb_hash_compare<StringRef> {
  static size_t hash(const StringRef &k) {
    return llvm::hash_value(k);
  }

  static bool equal(const StringRef &k1, const StringRef &k2) {
    return k1 == k2;
  }
};
}

typedef tbb::concurrent_hash_map<StringRef, InternedString> MapType;

static MapType map;

InternedString::InternedString(StringRef s) {
  if (s.empty())
    return;

  MapType::accessor acc;
  map.insert(acc, s);
  if (acc->second.data() == nullptr)
    acc->second = {s.data(), s.size()};
  data_ = acc->second.data();
  size_ = acc->second.size();
}
