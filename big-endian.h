#pragma once

#include <cstdint>
#include <cstring>

#ifdef __BIG_ENDIAN__
#error "mold does not support big-endian hosts"
#endif

namespace mold {

template <typename T>
class BigEndian {
public:
  BigEndian() = delete;

  operator T() const {
    T x;
    memcpy(&x, val, sizeof(T));
    return bswap(x);
  }

  BigEndian &operator=(T x) {
    x = bswap(x);
    memcpy(&val, &x, sizeof(T));
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

  static T bswap(T x) {
    if constexpr (sizeof(T) == 2)
      return __builtin_bswap16(x);
    else if constexpr (sizeof(T) == 4)
      return __builtin_bswap32(x);
    else
      return __builtin_bswap64(x);
  }
};

using ibig16 = BigEndian<int16_t>;
using ibig32 = BigEndian<int32_t>;
using ibig64 = BigEndian<int64_t>;
using ubig16 = BigEndian<uint16_t>;
using ubig32 = BigEndian<uint32_t>;
using ubig64 = BigEndian<uint64_t>;

} // namespace mold
