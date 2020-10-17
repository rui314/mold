#include "chibild.h"

typedef tbb::concurrent_hash_map<StringRef, const char *> MapType;

static MapType map;

const char *intern(StringRef s) {
  MapType::accessor acc;
  map.insert(acc, s);
  if (acc->second == nullptr)
    acc->second = s.data();
  return acc->second;
}
