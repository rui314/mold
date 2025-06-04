#include "lib.h"

#include <random>

namespace mold {

void get_random_bytes(u8 *buf, i64 size) {
  std::random_device rand;
  i64 i = 0;

  for (; i < size - 4; i += 4) {
    u32 val = rand();
    memcpy(buf + i, &val, 4);
  }

  u32 val = rand();
  memcpy(buf + i, &val, size - i);
}

} // namespace mold
