// This file implements multi-threaded compression routines. We simply
// create multiple compressed data blocks and concatenate them together.
// Tools supporting compressed sections generally read compressed sections
// until the section end to handle multiple concatenated streams.
//
// Using threads to compress data has a downside. Since the dictionary
// is not shared between compressed streams, the compression ratio is
// slightly sacrificed. However, if the stream size is large enough,
// that loss is negligible in practice.

#include "mold.h"

#include <zlib.h>
#include <zstd.h>

namespace mold {

Compressor::~Compressor() {
  for (std::span<u8> shard : shards)
    delete[] shard.data();
}

static std::vector<std::span<u8>> split(std::span<u8> input) {
  constexpr int BLOCK_SIZE = 1024 * 1024;
  std::vector<std::span<u8>> vec;
  while (!input.empty()) {
    i64 sz = std::min<i64>(BLOCK_SIZE, input.size());
    vec.push_back(input.subspan(0, sz));
    input = input.subspan(sz);
  }
  return vec;
}

static std::span<u8> zlib_compress(std::span<u8> input) {
  // compression level; must be between 0 to 9
  int level = Z_BEST_SPEED; // = 1

  unsigned long bufsize = compressBound(input.size());
  u8 *buf = new u8[bufsize];
  [[maybe_unused]] int r = compress2(buf, &bufsize, input.data(),
                                     input.size(), level);
  assert(r == Z_OK);
  return {buf, (size_t)bufsize};
}

static std::span<u8> zstd_compress(std::span<u8> input) {
  // compression level; must be between 1 to 22
  int level = ZSTD_CLEVEL_DEFAULT; // = 3

  i64 bufsize = ZSTD_COMPRESSBOUND(input.size());
  u8 *buf = new u8[bufsize];
  size_t sz = ZSTD_compress(buf, bufsize, input.data(), input.size(), level);
  assert(!ZSTD_isError(sz));
  return {buf, sz};
}

Compressor::Compressor(i64 format, u8 *buf, i64 size) {
  assert(format == ELFCOMPRESS_ZLIB || format == ELFCOMPRESS_ZSTD);
  std::vector<std::span<u8>> inputs = split(std::span(buf, size));
  shards.resize(inputs.size());

  // Compress each shard
  tbb::parallel_for((i64)0, (i64)inputs.size(), [&](i64 i) {
    if (format == ELFCOMPRESS_ZLIB)
      shards[i] = zlib_compress(inputs[i]);
    else
      shards[i] = zstd_compress(inputs[i]);
  });

  compressed_size = 0;
  for (std::span<u8> &shard : shards)
    compressed_size += shard.size();
}

void Compressor::write_to(u8 *buf) {
  std::vector<i64> offsets(shards.size());
  for (i64 i = 1; i < shards.size(); i++)
    offsets[i] = offsets[i - 1] + shards[i - 1].size();

  tbb::parallel_for((i64)0, (i64)shards.size(), [&](i64 i) {
    memcpy(buf + offsets[i], shards[i].data(), shards[i].size());
  });
}

} // namespace mold
