#pragma once

#include <cstdint>

namespace mold {

template <typename T>
class BigEndian {
public:
  BigEndian() : BigEndian(0) {}

  BigEndian(T x) {
    *this = x;
  }

  operator T() const {
    // We don't need to optimize this code because compilers are
    // usually smart enough to compile this loop into a single
    // byte-swap instruction such as x86's bswap.
    T ret = 0;
    for (int i = 0; i < sizeof(T); i++)
      ret = (ret << 8) | val[i];
    return ret;
  }

  BigEndian &operator=(T x) {
    for (int i = 0; i < sizeof(T); i++)
      val[sizeof(T) - 1 - i] = x >> (i * 8);
    return *this;
  }

  BigEndian &operator++() {
    return *this = *this + 1;
  }

  BigEndian operator++(int) {
    T ret = *this;
    *this = *this + 1;
    return ret;
  }

  BigEndian &operator--() {
    return *this = *this - 1;
  }

  BigEndian operator--(int) {
    T ret = *this;
    *this = *this - 1;
    return ret;
  }

  BigEndian &operator+=(T x) {
    return *this = *this + x;
  }

  BigEndian &operator&=(T x) {
    return *this = *this & x;
  }

  BigEndian &operator|=(T x) {
    return *this = *this | x;
  }

private:
  uint8_t val[sizeof(T)];
};

using ibig16 = BigEndian<int16_t>;
using ibig32 = BigEndian<int32_t>;
using ibig64 = BigEndian<int64_t>;
using ubig16 = BigEndian<uint16_t>;
using ubig32 = BigEndian<uint32_t>;
using ubig64 = BigEndian<uint64_t>;

} // namespace mold
