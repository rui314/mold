#include "chibild.h"

Symbol *Symbol::intern(StringRef name) {
  typedef tbb::concurrent_hash_map<StringRef, Symbol> MapTy;
  static MapTy map;

  MapTy::accessor acc;
  map.insert(acc, std::make_pair(name, Symbol(name)));
  return &acc->second;
}

std::string toString(Symbol sym) {
  return (StringRef(sym.name) + "(" + toString(sym.file) + ")").str();
}
