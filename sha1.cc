// Based on https://www.ietf.org/rfc/rfc3174.txt

#include "mold.h"

static inline u32 circular_shift(u32 bits, u32 word) {
  return (word << bits) | (word >> (32 - bits));
}

void SHA1::get_result(u8 digest[hash_size]) {
  if (!computed) {
    pad_message();
    for (int i = 0; i < 64; i++)
      block[i] = 0;
    length_low = 0;
    length_high = 0;
    computed = 1;
  }

  for (int i = 0; i < hash_size; i++)
    digest[i] = hash[i >> 2] >> 8 * (3 - (i & 3));
}

void SHA1::update(const u8 *buf, u32 len) {
  while (len--) {
    block[idx++] = (*buf & 0xFF);

    length_low += 8;
    if (length_low == 0) {
      length_high++;
      assert(length_high != 0);
    }

    if (idx == 64)
      process_message_block();
    buf++;
  }
}

void SHA1::process_message_block() {
  const u32 k[] = {0x5A827999, 0x6ED9EBA1, 0x8F1BBCDC, 0xCA62C1D6};

  u32 w[80];
  u32 a, b, c, d, e;

  for (int t = 0; t < 16; t++) {
    w[t] = block[t * 4] << 24;
    w[t] |= block[t * 4 + 1] << 16;
    w[t] |= block[t * 4 + 2] << 8;
    w[t] |= block[t * 4 + 3];
  }

  for (int t = 16; t < 80; t++)
    w[t] = circular_shift(1, w[t - 3] ^ w[t - 8] ^ w[t - 14] ^ w[t - 16]);

  a = hash[0];
  b = hash[1];
  c = hash[2];
  d = hash[3];
  e = hash[4];

  for (int t = 0; t < 20; t++) {
    u32 tmp =  circular_shift(5, a) + ((b & c) | (~b & d)) + e + w[t] + k[0];
    e = d;
    d = c;
    c = circular_shift(30,b);
    b = a;
    a = tmp;
  }

  for (int t = 20; t < 40; t++) {
    u32 tmp = circular_shift(5, a) + (b ^ c ^ d) + e + w[t] + k[1];
    e = d;
    d = c;
    c = circular_shift(30,b);
    b = a;
    a = tmp;
  }

  for (int t = 40; t < 60; t++) {
    u32 tmp = circular_shift(5,a) + ((b & c) | (b & d) | (c & d)) + e + w[t] + k[2];
    e = d;
    d = c;
    c = circular_shift(30,b);
    b = a;
    a = tmp;
  }

  for (int t = 60; t < 80; t++) {
    u32 tmp = circular_shift(5, a) + (b ^ c ^ d) + e + w[t] + k[3];
    e = d;
    d = c;
    c = circular_shift(30,b);
    b = a;
    a = tmp;
  }

  hash[0] += a;
  hash[1] += b;
  hash[2] += c;
  hash[3] += d;
  hash[4] += e;
  idx = 0;
}

void SHA1::pad_message() {
  if (idx > 55) {
    block[idx++] = 0x80;
    while (idx < 64)
      block[idx++] = 0;

    process_message_block();

    while (idx < 56)
      block[idx++] = 0;
  } else {
    block[idx++] = 0x80;
    while (idx < 56)
      block[idx++] = 0;
  }

  block[56] = length_high >> 24;
  block[57] = length_high >> 16;
  block[58] = length_high >> 8;
  block[59] = length_high;
  block[60] = length_low >> 24;
  block[61] = length_low >> 16;
  block[62] = length_low >> 8;
  block[63] = length_low;
  process_message_block();
}
