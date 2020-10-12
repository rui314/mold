#pragma once

#include "chibild.h"
#include "symbols.h"

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

class SymbolTable {
public:
  void add(StringRef key, Symbol sym);
  Symbol *get(StringRef key);
  std::vector<StringRef> get_keys();

private:
  typedef tbb::concurrent_hash_map<StringRef, Symbol> MapType;

  MapType map;
};
