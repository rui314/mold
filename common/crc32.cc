// This file contains a function to "forge" a CRC. That is, given a piece
// of data and a desired CRC32 value, crc32_solve() returns a binary blob
// to add to the end of the original data to yield the desired CRC32
// value. A trailing garbage is ignored for many bianry file formats, so
// you can create a file with a desired CRC using crc32_solve(). We need
// it for --separate-debug-info.
//
// The code in this file is based on Mark Adler's "spoof" program. You can
// obtain the original copy of it at the following URL:
//
//   https://github.com/madler/spoof/blob/master/spoof.c
//
// Below is the original license:

/* spoof.c -- modify a message to have a desired CRC

  Copyright (C) 2012, 2014, 2016, 2018, 2021 Mark Adler

  This software is provided 'as-is', without any express or implied warranty.
  In no event will the authors be held liable for any damages arising from the
  use of this software.

  Permission is granted to anyone to use this software for any purpose,
  including commercial applications, and to alter it and redistribute it
  freely, subject to the following restrictions:

  1. The origin of this software must not be misrepresented; you must not claim
     that you wrote the original software. If you use this software in a
     product, an acknowledgment in the product documentation would be
     appreciated but is not required.
  2. Altered source versions must be plainly marked as such, and must not be
     misrepresented as being the original software.
  3. This notice may not be removed or altered from any source distribution.

  Mark Adler
  madler@alumni.caltech.edu

 */

#include "common.h"

#include <tbb/parallel_for_each.h>
#include <zlib.h>

namespace mold {

static constexpr i64 deg = 32;
static constexpr u32 poly = 0xedb88320;

using Mat = std::array<u32, deg>;

static constexpr u32 gf2_matrix_times(const Mat &mat, u32 vec) {
  u32 n = 0;
  for (i64 i = 0; vec; vec >>= 1, i++)
    if (vec & 1)
      n ^= mat[i];
  return n;
}

static constexpr Mat gf2_matrix_square(const Mat &mat) {
  Mat sq;
  for (i64 i = 0; i < deg; i++)
    sq[i] = gf2_matrix_times(mat, mat[i]);
  return sq;
}

static consteval std::array<Mat, 64> get_crc_zero_powers() {
  std::array<Mat, 64> p;

  p[1][0] = poly;
  for (i64 n = 1; n < deg; n++)
    p[1][n] = 1 << (n - 1);

  p[0] = gf2_matrix_square(p[1]);
  p[1] = gf2_matrix_square(p[0]);
  p[0] = gf2_matrix_square(p[1]);
  p[1] = gf2_matrix_square(p[0]);

  for (i64 i = 2; i < 64; i++)
    p[i] = gf2_matrix_square(p[i - 1]);
  return p;
}

// Efficiently apply len zero bytes to crc, returning the resulting crc.
static u32 crc_zeros(u32 crc, i64 len) {
  static constexpr std::array<Mat, 64> power = get_crc_zero_powers();

  // apply len zeros to crc
  if (crc)
    for (i64 n = 0; len; len >>= 1, n++)
      if (len & 1)
        crc = gf2_matrix_times(power[n], crc);
  return crc;
}

// Solve M x = c for x
static std::vector<bool> gf2_matrix_solve(std::vector<u32> M, u32 c) {
  i64 cols = M.size();
  i64 rows = deg;

  // create adjoining identity matrix
  std::vector<std::vector<bool>> inv(cols);
  for (i64 i = 0; i < cols; i++) {
    inv[i].resize(cols);
    inv[i][i] = 1;
  }

  for (i64 j = 0; j < rows; j++) {
    u32 pos = 1 << j;

    if ((M[j] & pos) == 0) {
      i64 k;
      for (k = j + 1; k < cols; k++)
        if (M[k] & pos)
          break;

      if (k == cols) {
        std::cerr << "mold: internal error: crc32_solve: no solution\n";
        exit(1);
      }

      std::swap(M[j], M[k]);
      std::swap(inv[j], inv[k]);
    }

    for (i64 k = 0; k < cols; k++) {
      if (k != j && (M[k] & pos)) {
        M[k] ^= M[j];
        for (i64 i = 0; i < cols; i++)
          inv[k][i] = inv[k][i] ^ inv[j][i];
      }
    }
  }

  // multiply inverse by c to get result x
  std::vector<bool> x(cols);
  for (i64 j = 0; c; c >>= 1, j++)
    if (c & 1)
      for (i64 i = 0; i < cols; i++)
        x[i] = x[i] ^ inv[j][i];
  return x;
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
    shard.crc = crc32_z(0, shard.buf, shard.len);
  });

  for (Shard &shard : shards)
    crc = crc32_combine(crc, shard.crc, shard.len);
  return crc;
}

// Given input data and a desired CRC value, this function returns
// a binary blob such that if the blob is appended to the end of the
// input data, the entire data's CRC value becomes the desired CRC.
std::vector<u8> crc32_solve(i64 datalen, u32 current, u32 desired) {
  // Compute the CRC for the given data and the all-zero trailer
  constexpr i64 trailer_len = 16;
  current = ~crc_zeros(~current, trailer_len);

  // Compute CRCs for all bits in the trailer
  std::vector<u32> mat;
  for (i64 i = 0; i < trailer_len * 8; i++) {
    u8 buf[trailer_len] = {};
    buf[i / 8] = 1 << (i % 8);
    mat.push_back(~crc32_z(~crc_zeros(0, datalen), buf, sizeof(buf)));
  }

  // Find desired trailer data
  std::vector<bool> sol = gf2_matrix_solve(mat, desired ^ current);

  std::vector<u8> out(trailer_len);
  for (i64 i = 0; i < trailer_len * 8; i++)
    if (sol[i])
      out[i / 8] |= 1 << (i % 8);
  return out;
}

} // namespace mold
