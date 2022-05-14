#include "mold.h"

#include <random>

namespace mold {

std::array<u8, 16> get_uuid_v4() {
  std::array<u8, 16> bytes;

  std::random_device rand;
  u32 buf[4] = { rand(), rand(), rand(), rand() };
  memcpy(bytes.data(), buf, 16);

  // Indicate that this is UUIDv4.
  bytes[6] &= 0b00001111;
  bytes[6] |= 0b01000000;

  // Indicates that this is an RFC4122 variant.
  bytes[8] &= 0b00111111;
  bytes[8] |= 0b10000000;
  return bytes;
}

} // namespace mold
