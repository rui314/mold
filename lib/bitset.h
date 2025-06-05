#pragma once

#include "integers.h"

#include <cassert>
#include <vector>

namespace mold {

class BitsetProxy {
public:
  BitsetProxy(u64 &word, size_t pos)
    : word(word), mask(1ULL << pos) {}

  BitsetProxy &operator=(bool val) {
    if (val)
      word |= mask;
    else
      word &= ~mask;
    return *this;
  }

  BitsetProxy &operator=(const BitsetProxy &other) {
    return *this = (bool)other;
  }

  operator bool() const {
    return word & mask;
  }

private:
  u64 &word;
  u64 mask;
};

class Bitset {
public:
  Bitset() = default;
  Bitset(i64 size) { resize(size); }

  void resize(i64 n) {
    words.clear();
    words.resize((n + 63) / 64);
    size = n;
  }

  Bitset &operator|=(const Bitset &x) {
    assert(size == x.size);
    for (i64 i = 0; i < words.size(); i++)
      words[i] |= x.words[i];
    return *this;
  }

  Bitset &operator&=(const Bitset &x) {
    assert(size == x.size);
    for (i64 i = 0; i < words.size(); i++)
      words[i] &= x.words[i];
    return *this;
  }

  Bitset &operator<<=(size_t n) {
    assert(n == 1);
    for (i64 i = words.size() - 1; i > 0; i--)
      words[i] = (words[i] << 1) | (words[i - 1] >> 63);
    words[0] <<= 1;
    return *this;
  }

  BitsetProxy operator[](size_t pos) {
    assert(pos < size);
    return {words[pos / 64], pos % 64};
  }

  i64 size = 0;
  std::vector<u64> words;
};

} // namespace mold
