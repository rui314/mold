#include "common.h"

#include <random>

namespace mold {

void write_uuid_v4(u8 *buf) {
  std::random_device rand;
  u32 tmp[4] = { rand(), rand(), rand(), rand() };
  memcpy(buf, tmp, 16);

  // Indicate that this is UUIDv4 as defined by RFC4122.
  buf[6] = (buf[6] & 0b0000'1111) | 0b0100'0000;
  buf[8] = (buf[8] & 0b0011'1111) | 0b1000'0000;
}

} // namespace mold
