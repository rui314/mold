#include "chibild.h"

Symbol *SymbolTable::add(InternedString name, Symbol sym) {
  MapType::accessor acc;
  map.insert(acc, (uintptr_t)name.data());
  if (acc->second.file == nullptr)
    acc->second = sym;
  acc.release();
  return nullptr;
}

Symbol *SymbolTable::get(InternedString name) {
  MapType::accessor acc;
  if (map.find(acc, (uintptr_t)name.data()))
    return &acc->second;
  return nullptr;
}
