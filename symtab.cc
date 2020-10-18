#include "chibild.h"

namespace tbb {
template<>
struct tbb_hash_compare<StringRef> {
  static size_t hash(const StringRef& k) {
    return llvm::hash_value(k);
  }

  static bool equal(const StringRef& k1, const StringRef& k2) {
    return k1 == k2;
  }
};
}

Symbol *Symbol::intern(StringRef name) {
  typedef tbb::concurrent_hash_map<StringRef, Symbol> MapTy;
  static MapTy map;

  MapTy::accessor acc;
  map.insert(acc, std::pair<StringRef, Symbol>(name, Symbol(name)));
  return &acc->second;
}

std::string toString(Symbol sym) {
  return (StringRef(sym.name) + "(" + toString(sym.file) + ")").str();
}
