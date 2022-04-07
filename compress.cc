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

#define CHECK(fn) do { int r = (fn); assert(r == Z_OK); } while (0)

namespace mold {

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
  // pretty well with lower compression levels, we chose compression
  // level 1.
  z_stream strm;
  strm.zalloc = Z_NULL;
  strm.zfree = Z_NULL;
  strm.opaque = Z_NULL;

  CHECK(deflateInit2(&strm, 1, Z_DEFLATED, -15, 8, Z_DEFAULT_STRATEGY));

  // Set an input buffer
  strm.avail_in = input.size();
  strm.next_in = (u8 *)input.data();

  // Set an output buffer. deflateBound() returns an upper bound
  // on the compression size. +16 for Z_SYNC_FLUSH.
  std::vector<u8> buf(deflateBound(&strm, strm.avail_in) + 16);

  // Compress data. It writes all compressed bytes except the last
  // partial byte, so up to 7 bits can be held to be written to the
  // buffer.
  strm.avail_out = buf.size();
  strm.next_out = buf.data();
  CHECK(deflate(&strm, Z_BLOCK));

  // This is a workaround for libbacktrace before 2022-04-06.
  //
  // Zlib is a bit stream, and what Z_SYNC_FLUSH does is to write a
  // three bit value indicating the start of an uncompressed data
  // block followed by four byte data 00 00 ff ff which indicates that
  // the length of the block is zero. libbacktrace uses its own zlib
  // inflate routine, and it had a bug that if that particular three
  // bit value happens to end at a byte boundary, it accidentally
  // skipped the next byte.
  //
  // In order to avoid triggering that bug, we should avoid calling
  // deflate() with Z_SYNC_FLUSH if the current bit position is 5.
  // If it's 5, we insert an empty block consisting of 10 bits so
  // that the bit position is 7 in the next byte.
  //
  // https://github.com/ianlancetaylor/libbacktrace/pull/87
  int nbits;
  deflatePending(&strm, Z_NULL, &nbits);
  if (nbits == 5)
    CHECK(deflatePrime(&strm, 10, 2));
  CHECK(deflate(&strm, Z_SYNC_FLUSH));

  assert(strm.avail_out > 0);
  buf.resize(buf.size() - strm.avail_out);
  deflateEnd(&strm);
  return buf;
}

ZlibCompressor::ZlibCompressor(std::string_view input) {
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

i64 ZlibCompressor::size() const {
  i64 size = 2;    // +2 for header
  for (const std::vector<u8> &shard : shards)
    size += shard.size();
  return size + 6; // +6 for trailer and checksum
}

void ZlibCompressor::write_to(u8 *buf) {
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
  *(ubig32 *)(end - 4) = checksum;
}

GzipCompressor::GzipCompressor(std::string_view input) {
  std::vector<std::string_view> inputs = split(input);
  std::vector<u32> crc(inputs.size());
  shards.resize(inputs.size());

  // Compress each shard
  tbb::parallel_for((i64)0, (i64)inputs.size(), [&](i64 i) {
    crc[i] = crc32(0, (u8 *)inputs[i].data(), inputs[i].size());
    shards[i] = do_compress(inputs[i]);
  });

  // Combine checksums
  checksum = crc[0];
  for (i64 i = 1; i < inputs.size(); i++)
    checksum = crc32_combine(checksum, crc[i], inputs[i].size());

  uncompressed_size = input.size();
}

i64 GzipCompressor::size() const {
  i64 size = 10;    // +10 for header
  for (const std::vector<u8> &shard : shards)
    size += shard.size();
  return size + 10; // +10 for trailer and checksum
}

void GzipCompressor::write_to(u8 *buf) {
  // Write a zlib-format header
  memset(buf, 0, 10);
  buf[0] = 0x1f; // magic
  buf[1] = 0x8b; // magic
  buf[2] = 0x08; // compression method is zlib
  buf[9] = 0xff; // made on unknown OS

  // Copy compressed data
  std::vector<i64> offsets(shards.size());
  offsets[0] = 10; // +10 for header
  for (i64 i = 1; i < shards.size(); i++)
    offsets[i] = offsets[i - 1] + shards[i - 1].size();

  tbb::parallel_for((i64)0, (i64)shards.size(), [&](i64 i) {
    memcpy(&buf[offsets[i]], shards[i].data(), shards[i].size());
  });

  // Write a trailer
  u8 *end = buf + size();
  end[-10] = 0x3; // two-byte zlib stream terminator
  end[-9] = 0;
  *(u32 *)(end - 8) = checksum;
  *(u32 *)(end - 4) = uncompressed_size;
}

} // namespace mold
