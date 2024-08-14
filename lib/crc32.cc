#include "common.h"

#include <tbb/parallel_for_each.h>
#include <zlib.h>

namespace mold {

// This function "forges" a CRC. That is, given the current and a desired
// CRC32 value, crc32_solve() returns a binary blob to add to the end of
// the original data to yield the desired CRC. Trailing garbage is ignored
// by many bianry file formats, so you can create a file with a desired
// CRC using crc32_solve(). We need it for --separate-debug-file.
std::vector<u8> crc32_solve(u32 current, u32 desired) {
  constexpr u32 poly = 0xedb88320;
  u32 x = ~desired;

  // Each iteration computes x = (x * x^-1) mod poly.
  for (i64 i = 0; i < 32; i++) {
    x = std::rotl(x, 1);
    x ^= (x & 1) * (poly << 1);
  }

  x ^= ~current;

  std::vector<u8> out(4);
  out[0] = x;
  out[1] = x >> 8;
  out[2] = x >> 16;
  out[3] = x >> 24;
  return out;
}

// Compute a CRC for given data in parallel
u32 compute_crc32(u32 crc, u8 *buf, i64 len) {
  struct Shard {
    u8 *buf;
    i64 len;
    u32 crc;
  };

  constexpr i64 shard_size = 1024 * 1024; // 1 MiB
  std::vector<Shard> shards;

  while (len > 0) {
    i64 sz = std::min(len, shard_size);
    shards.push_back({buf, sz, 0});
    buf += sz;
    len -= sz;
  }

  tbb::parallel_for_each(shards.begin(), shards.end(), [](Shard &shard) {
    shard.crc = crc32(0, shard.buf, shard.len);
  });

  for (Shard &shard : shards)
    crc = crc32_combine(crc, shard.crc, shard.len);
  return crc;
}

} // namespace mold
