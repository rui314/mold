#pragma once

#include "chibild.h"

class SymbolTable {
public:
  void add(StringRef key, Symbol sym);
  Symbol *get(StringRef key);
  std::vector<StringRef> get_keys();

private:
  typedef tbb::concurrent_hash_map<StringRef, Symbol> MapType;

  MapType map;
};
