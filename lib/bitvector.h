#pragma once

#include "integers.h"

#include <cassert>
#include <vector>

namespace mold {

class BitvectorProxy {
public:
  BitvectorProxy(u64 &word, size_t pos)
    : word(word), mask(1ULL << pos) {}

  BitvectorProxy &operator=(bool val) {
    if (val)
      word |= mask;
    else
      word &= ~mask;
    return *this;
  }

  BitvectorProxy &operator=(const BitvectorProxy &other) {
    return *this = (bool)other;
  }

  operator bool() const {
    return word & mask;
  }

private:
  u64 &word;
  u64 mask;
};

class Bitvector {
public:
  Bitvector() = default;
  Bitvector(i64 n) : size(n), words((n + 63) / 64) {}

  void resize(i64 n) {
    words.clear();
    words.resize((n + 63) / 64);
    size = n;
  }

  Bitvector &operator|=(const Bitvector &x) {
    assert(size == x.size);
    for (i64 i = 0; i < words.size(); i++)
      words[i] |= x.words[i];
    return *this;
  }

  Bitvector &operator&=(const Bitvector &x) {
    assert(size == x.size);
    for (i64 i = 0; i < words.size(); i++)
      words[i] &= x.words[i];
    return *this;
  }

  Bitvector &operator<<=(size_t n) {
    assert(n == 1);
    for (i64 i = words.size() - 1; i > 0; i--)
      words[i] = (words[i] << 1) | (words[i - 1] >> 63);
    words[0] <<= 1;
    return *this;
  }

  BitvectorProxy operator[](size_t pos) {
    assert(pos < size);
    return {words[pos / 64], pos % 64};
  }

  i64 size = 0;
  std::vector<u64> words;
};

} // namespace mold
