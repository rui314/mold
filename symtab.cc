#include "chibild.h"

void SymbolTable::add(StringRef name, Symbol sym) {
  MapType::accessor acc;
  map.insert(acc, name);
  acc->second = sym;
  acc.release();
}

Symbol *SymbolTable::get(StringRef name) {
  MapType::accessor acc;
  if (map.find(acc, StringRef(name)))
    return &acc->second;
  return nullptr;
}

std::vector<StringRef> SymbolTable::get_keys() {
  auto it = map.begin();
  auto end = map.end();

  std::vector<StringRef> vec;
  vec.reserve(map.size());

  for (; it != end; it++)
    vec.push_back(it->first);
  return vec;
}
