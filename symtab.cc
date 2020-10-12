#include "chibild.h"

Symbol *SymbolTable::add(StringRef name, Symbol sym) {
  MapType::accessor acc;
  map.insert(acc, name);
  if (acc->second.file == nullptr)
    acc->second = sym;
  acc.release();
  return nullptr;
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
