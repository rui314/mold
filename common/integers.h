// This file defines integral types for file input/output. We need to use
// these types instead of the plain integers (such as uint32_t or int32_t)
// when reading from/writing to an mmap'ed file area for the following
// reasons:
//
// 1. mold is always a cross linker and should not depend on what host it
//    is running on. Users should be able to run mold on a big-endian
//    SPARC machine to create a little-endian RV64 binary, for example.
//
// 2. Even though data members in all ELF data strucutres are naturally
//    aligned, they are not guaranteed to be aligned on memory. Because
//    archive file (.a file) aligns each member only to a 2 byte boundary,
//    anything larger than 2 bytes may be unaligned in an mmap'ed memory.
//    Unaligned access is an undefined behavior in C/C++, so we shouldn't
//    cast an arbitrary pointer to a uint32_t, for example, to read a
//    32-bits value.
//
// The data types defined in this file don't depend on host byte order and
// don't do unaligned access.

#pragma once

#include <bit>
#include <cstdint>
#include <cstring>

#if !defined(__LITTLE_ENDIAN__) && !defined(__BIG_ENDIAN__)
# if __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
#  define __LITTLE_ENDIAN__ 1
# elif __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
#  define __BIG_ENDIAN__ 1
# else
#  error "unknown host byte order"
# endif
#endif

namespace mold {

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

template <typename T, std::endian endian, int size = sizeof(T)>
class Integer {
public:
  Integer() = default;
  Integer(T x) { *this = x; }

  operator T() const {
    if (size == 3) {
      if (endian == std::endian::little)
        return (val[2] << 16) | (val[1] << 8) | val[0];
      else
        return (val[0] << 16) | (val[1] << 8) | val[2];
    } else {
      T x;
      memcpy(&x, val, size);
      if (endian != std::endian::native)
        x = bswap(x);
      return x;
    }
  }

  Integer &operator=(T x) {
    if (size == 3) {
      if (endian == std::endian::little) {
        val[0] = x;
        val[1] = x >> 8;
        val[2] = x >> 16;
      } else {
        val[0] = x >> 16;
        val[1] = x >> 8;
        val[2] = x;
      }
    } else {
      if (endian != std::endian::native)
        x = bswap(x);
      memcpy(val, &x, size);
    }
    return *this;
  }

  Integer &operator++()    { return *this = *this + 1; }
  Integer operator++(int)  { return ++*this - 1; }
  Integer &operator--()    { return *this = *this - 1; }
  Integer operator--(int)  { return --*this + 1; }
  Integer &operator+=(T x) { return *this = *this + x; }
  Integer &operator-=(T x) { return *this = *this - x; }
  Integer &operator&=(T x) { return *this = *this & x; }
  Integer &operator|=(T x) { return *this = *this | x; }

private:
  static T bswap(T x) {
    switch (size) {
    case 2:  return __builtin_bswap16(x);
    case 4:  return __builtin_bswap32(x);
    case 8:  return __builtin_bswap64(x);
    default: __builtin_unreachable();
    }
  }

  u8 val[size];
};;

using il16 = Integer<i16, std::endian::little>;
using il32 = Integer<i32, std::endian::little>;
using il64 = Integer<i64, std::endian::little>;

using ul16 = Integer<u16, std::endian::little>;
using ul24 = Integer<u32, std::endian::little, 3>;
using ul32 = Integer<u32, std::endian::little>;
using ul64 = Integer<u64, std::endian::little>;

using ib16 = Integer<i16, std::endian::big>;
using ib32 = Integer<i32, std::endian::big>;
using ib64 = Integer<i64, std::endian::big>;

using ub16 = Integer<u16, std::endian::big>;
using ub24 = Integer<u32, std::endian::big, 3>;
using ub32 = Integer<u32, std::endian::big>;
using ub64 = Integer<u64, std::endian::big>;

} // namespace mold
