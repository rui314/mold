// This file implements a multi-threaded zlib compression routine.
//
// Multiple pieces of raw compressed data in zlib-format can be merged
// just by concatenation as long as each zlib stream is flushed with
// Z_SYNC_FLUSH. In this file, we split input data into multiple
// shards, compress them individually and concatenate them. We then
// append a header, a trailer and a checksum so that the concatenated
// data is valid zlib-format data.
//
// Using threads to compress data has a downside. Since the dictionary
// is reset on boundaries of shards, compression ratio is sacrificed
// a little bit. However, if a shard size is large enough, that loss
// is negligible in practice.

#include "mold.h"

#include <tbb/parallel_for_each.h>
#include <zlib.h>

static constexpr i64 SHARD_SIZE = 1024 * 1024;

static std::vector<std::string_view> split(std::string_view input) {
  std::vector<std::string_view> shards;

  while (input.size() >= SHARD_SIZE) {
    shards.push_back(input.substr(0, SHARD_SIZE));
    input = input.substr(SHARD_SIZE);
  }
  if (!input.empty())
    shards.push_back(input);
  return shards;
}

static std::vector<u8> do_compress(std::string_view input) {
  // Initialize zlib stream. Since debug info is generally compressed
  // pretty well, we chose compression level 3.
  z_stream strm;
  strm.zalloc = Z_NULL;
  strm.zfree = Z_NULL;
  strm.opaque = Z_NULL;
  int r = deflateInit2(&strm, 3, Z_DEFLATED, -15, 8, Z_DEFAULT_STRATEGY);
  assert(r == Z_OK);

  // Set an input buffer
  strm.avail_in = input.size();
  strm.next_in = (u8 *)input.data();

  // Set an output buffer. deflateBound() returns an upper bound
  // on the compression size. +16 for Z_SYNC_FLUSH.
  std::vector<u8> buf(deflateBound(&strm, strm.avail_in) + 16);

  strm.avail_out = buf.size();
  strm.next_out = buf.data();

  r = deflate(&strm, Z_SYNC_FLUSH);
  assert(r == Z_OK);
  assert(strm.avail_out > 0);

  buf.resize(buf.size() - strm.avail_out);
  deflateEnd(&strm);
  return buf;
}

Compress::Compress(std::string_view input) {
  std::vector<std::string_view> inputs = split(input);
  std::vector<u64> adlers(inputs.size());
  shards.resize(inputs.size());

  // Compress each shard
  tbb::parallel_for((i64)0, (i64)inputs.size(), [&](i64 i) {
    adlers[i] = adler32(1, (u8 *)inputs[i].data(), inputs[i].size());
    shards[i] = do_compress(inputs[i]);
  });

  // Combine checksums
  checksum = adlers[0];
  for (i64 i = 1; i < inputs.size(); i++)
    checksum = adler32_combine(checksum, adlers[i], inputs[i].size());
}

i64 Compress::size() const {
  i64 size = 2;    // +2 for header
  for (const std::vector<u8> &shard : shards)
    size += shard.size();
  return size + 6; // +6 for trailer and checksum
}

void Compress::write_to(u8 *buf) {
  // Write a zlib-format header
  buf[0] = 0x78;
  buf[1] = 0x9c;

  // Copy compressed data
  std::vector<i64> offsets(shards.size());
  offsets[0] = 2; // +2 for header
  for (i64 i = 1; i < shards.size(); i++)
    offsets[i] = offsets[i - 1] + shards[i - 1].size();

  tbb::parallel_for((i64)0, (i64)shards.size(), [&](i64 i) {
    memcpy(&buf[offsets[i]], shards[i].data(), shards[i].size());
  });

  // Write a trailer
  u8 *end = buf + size();
  end[-6] = 3;
  end[-5] = 0;

  // Write a checksum
  write32be(end - 4, checksum);
}
