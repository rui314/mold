#include "chibild.h"

std::string toString(Symbol sym) {
  return (StringRef(sym.name) + "(" + toString(sym.file) + ")").str();
}

Symbol *SymbolTable::add(Symbol sym) {
  MapType::accessor acc;
  map.insert(acc, (uintptr_t)sym.name.data());
  if (acc->second.file == nullptr)
    acc->second = sym;
  return &acc->second;
}

Symbol *SymbolTable::get(InternedString name) {
  MapType::accessor acc;
  if (map.find(acc, (uintptr_t)name.data()))
    return &acc->second;
  return nullptr;
}
