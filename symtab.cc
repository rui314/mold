#include "chibild.h"

void SymbolTable::add(StringRef key, StringRef val) {
  MapType::accessor acc;
  map.insert(acc, key);
  acc->second = val;
  acc.release();
}

StringRef SymbolTable::get(StringRef key) {
  MapType::accessor acc;
  if (map.find(acc, key))
    return acc->second;
  return "";
}
