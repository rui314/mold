#include "chibild.h"

IString::MapType IString::map;

IString::IString(StringRef s) {
  MapType::accessor acc;
  map.insert(acc, s);
  if (acc->second.data() == nullptr)
    acc->second = s;
  data = acc->second.data();
  size = acc->second.size();
}
