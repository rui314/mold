// This file defines integral types for file input/output. You should
// use these types instead of the plain integers (such as u32 or i32)
// when reading from/writing to an mmap'ed file area.
//
// Here is why you need these types. In C/C++, all data accesses must
// be aligned. That is, if you read an N byte value from memory, the
// address of that value must be a multiple of N. For example, reading
// an u32 value from address 16 is legal, while reading from 18 is not
// (since 18 is not a multiple of 4.) An unaligned access yields an
// undefined behavior.
//
// All data members of the ELF data structures are naturally aligned,
// so it may look like we don't need the integral types defined in
// this file to access them. But there's a catch; if an object file is
// in an archive file (a .a file), the beginning of the file is
// guaranteed to be aligned only to a 2 bytes boundary, because ar
// aligns each member only to a 2 byte boundary. Therefore, any data
// members larger than 2 bytes may be unaligned in an mmap'ed memory,
// and thus you always need to use the integral types in this file to
// access it.

#pragma once

#include <cstdint>
#include <cstring>

#ifdef __BIG_ENDIAN__
#error "mold does not support big-endian hosts"
#endif

namespace mold {

template <typename T, size_t SIZE = sizeof(T)>
class LittleEndian {
public:
  LittleEndian() { *this = 0; }
  LittleEndian(T x) { *this = x; }

  operator T() const {
    T x = 0;
    memcpy(&x, val, SIZE);
    return x;
  }

  LittleEndian &operator=(T x) {
    memcpy(&val, &x, SIZE);
    return *this;
  }

  LittleEndian &operator++() {
    return *this = *this + 1;
  }

  LittleEndian operator++(int) {
    T ret = *this;
    *this = *this + 1;
    return ret;
  }

  LittleEndian &operator--() {
    return *this = *this - 1;
  }

  LittleEndian operator--(int) {
    T ret = *this;
    *this = *this - 1;
    return ret;
  }

  LittleEndian &operator+=(T x) {
    return *this = *this + x;
  }

  LittleEndian &operator-=(T x) {
    return *this = *this - x;
  }

  LittleEndian &operator&=(T x) {
    return *this = *this & x;
  }

  LittleEndian &operator|=(T x) {
    return *this = *this | x;
  }

private:
  uint8_t val[SIZE];
};

using il16 = LittleEndian<int16_t>;
using il32 = LittleEndian<int32_t>;
using il64 = LittleEndian<int64_t>;
using ul16 = LittleEndian<uint16_t>;
using ul24 = LittleEndian<uint32_t, 3>;
using ul32 = LittleEndian<uint32_t>;
using ul64 = LittleEndian<uint64_t>;

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
    // Compiler is usually smart enough to compile the following code into
    // a single bswap instruction. See https://godbolt.org/z/7nvaM7qab
    if constexpr (sizeof(T) == 2) {
      return ((x & 0xff00) >> 8) |
             ((x & 0x00ff) << 8);
    } else if constexpr (sizeof(T) == 4) {
      return ((x & 0xff000000) >> 24) |
             ((x & 0x00ff0000) >> 8)  |
             ((x & 0x0000ff00) << 8)  |
             ((x & 0x000000ff) << 24);
    } else {
      return ((x & 0xff000000'00000000) >> 56) |
             ((x & 0x00ff0000'00000000) >> 40) |
             ((x & 0x0000ff00'00000000) >> 24) |
             ((x & 0x000000ff'00000000) >> 8)  |
             ((x & 0x00000000'ff000000) << 8)  |
             ((x & 0x00000000'00ff0000) << 24) |
             ((x & 0x00000000'0000ff00) << 40) |
             ((x & 0x00000000'000000ff) << 56);
    }
  }
};

using ib16 = BigEndian<int16_t>;
using ib32 = BigEndian<int32_t>;
using ib64 = BigEndian<int64_t>;
using ub16 = BigEndian<uint16_t>;
using ub32 = BigEndian<uint32_t>;
using ub64 = BigEndian<uint64_t>;

} // namespace mold
