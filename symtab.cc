#include "chibild.h"

void SymbolTable::add(StringRef name, Symbol sym) {
  MapType::accessor acc;
  map.insert(acc, name);
  acc->second = sym;
  acc.release();
}

Symbol *SymbolTable::get(StringRef name) {
  MapType::accessor acc;
  if (map.find(acc, name))
    return &acc->second;
  return nullptr;
}
