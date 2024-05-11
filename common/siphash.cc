// This file is based on the SipHash reference implementation which is
// in the public domain.
//
// SipHash is a keyed hash function designed to be collision-resistant
// as long as key is not known to an attacker. That is, as long as we
// use some random number as a key, we can assume that there's no hash
// collision. I don't think SipHash is throughly tested as a secure hash
// function as others such as SHA256 or Blake3 are, but for our purpose,
// I believe the hash function is robust enough. SipHash is much faster
// than SHA256 or Blake3.
//
// Our implementation always outputs a 128 bit hash value.

#include "common.h"

namespace mold {

#define ROUND                                   \
  do {                                          \
    v0 += v1;                                   \
    v1 = std::rotl(v1, 13);                     \
    v1 ^= v0;                                   \
    v0 = std::rotl(v0, 32);                     \
    v2 += v3;                                   \
    v3 = std::rotl(v3, 16);                     \
    v3 ^= v2;                                   \
    v0 += v3;                                   \
    v3 = std::rotl(v3, 21);                     \
    v3 ^= v0;                                   \
    v2 += v1;                                   \
    v1 = std::rotl(v1, 17);                     \
    v1 ^= v2;                                   \
    v2 = std::rotl(v2, 32);                     \
  } while (0)

// SipHash-1-3
#define C_ROUND ROUND
#define D_ROUND for (i64 i = 0; i < 3; i++) ROUND

SipHash::SipHash(u8 *key) {
  k0 = *(ul64 *)key;
  k1 = *(ul64 *)(key + 8);
  v0 = 0x736f6d6570736575 ^ k0;
  v1 = 0x646f72616e646f6d ^ k1 ^ 0xee;
  v2 = 0x6c7967656e657261 ^ k0;
  v3 = 0x7465646279746573 ^ k1;
}

void SipHash::update(u8 *msg, i64 msglen) {
  total_bytes += msglen;

  if (buflen > 0) {
    if (buflen + msglen < 8) {
      memcpy(buf + buflen, msg, msglen);
      buflen += msglen;
      return;
    }

    i64 j = 8 - buflen;
    memcpy(buf + buflen, msg, j);

    u64 m = *(ul64 *)buf;
    v3 ^= m;
    C_ROUND;
    v0 ^= m;

    msg += j;
    msglen -= j;
    buflen = 0;
  }

  while (msglen >= 8) {
    u64 m = *(ul64 *)msg;
    v3 ^= m;
    C_ROUND;
    v0 ^= m;

    msg += 8;
    msglen -= 8;
  }

  memcpy(buf, msg, msglen);
  buflen = msglen;
}

void SipHash::finish(u8 *out) {
  memset(buf + buflen, 0, 8 - buflen);
  u64 b = (total_bytes << 56) | *(ul64 *)buf;

  v3 ^= b;
  C_ROUND;
  v0 ^= b;

  v2 ^= 0xee;
  D_ROUND;
  *(ul64 *)out = v0 ^ v1 ^ v2 ^ v3;

  v1 ^= 0xdd;
  D_ROUND;
  *(ul64 *)(out + 8) = v0 ^ v1 ^ v2 ^ v3;
}

} // namespace mold
