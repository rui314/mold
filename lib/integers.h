// This file defines integral types for file input/output. We need to use
// these types instead of the plain integers (such as uint32_t or int32_t)
// when reading from/writing to an mmap'ed file area for the following
// reasons:
//
// 1. mold is always a cross linker and should not depend on what host it
//    is running on. For example, users should be able to run mold on a
//    big-endian SPARC machine to create a little-endian RV64 binary.
//
// 2. Even though data members in all ELF data strucutres are naturally
//    aligned, they are not guaranteed to be aligned on memory because of
//    archive files. Archive files (.a files) align each member only to a
//    2 byte boundary, so anything larger than 2 bytes may be misaligned
//    in an mmap'ed memory. Misaligned access is an undefined behavior in
//    C/C++, so we shouldn't cast an arbitrary pointer to a uint32_t, for
//    example, to read a 32 bit value.
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
  constexpr Integer() = default;

  constexpr Integer(T x) requires (endian == std::endian::little && size == 2)
    : buf{(u8)x, (u8)(x >> 8)} {}

  constexpr Integer(T x) requires (endian == std::endian::little && size == 3)
    : buf{(u8)x, (u8)(x >> 8), (u8)(x >> 16)} {}

  constexpr Integer(T x) requires (endian == std::endian::little && size == 4)
    : buf{(u8)x, (u8)(x >> 8), (u8)(x >> 16), (u8)(x >> 24)} {}

  constexpr Integer(T x) requires (endian == std::endian::little && size == 8)
    : buf{(u8)x,         (u8)(x >> 8),  (u8)(x >> 16), (u8)(x >> 24),
          (u8)(x >> 32), (u8)(x >> 40), (u8)(x >> 48), (u8)(x >> 56)} {}

  constexpr Integer(T x) requires (endian == std::endian::big && size == 2)
    : buf{(u8)(x >> 8), (u8)x} {}

  constexpr Integer(T x) requires (endian == std::endian::big && size == 3)
    : buf{(u8)(x >> 16), (u8)(x >> 8), (u8)x} {}

  constexpr Integer(T x) requires (endian == std::endian::big && size == 4)
    : buf{(u8)(x >> 24), (u8)(x >> 16), (u8)(x >> 8), (u8)x} {}

  constexpr Integer(T x) requires (endian == std::endian::big && size == 8)
    : buf{(u8)(x >> 56), (u8)(x >> 48), (u8)(x >> 40), (u8)(x >> 32),
          (u8)(x >> 24), (u8)(x >> 16), (u8)(x >> 8),  (u8)x} {}

  Integer &operator=(T x) {
    new (this) Integer(x);
    return *this;
  }

  operator T() const {
    if constexpr (endian == std::endian::little) {
      if constexpr (size == 2)
        return buf[1] << 8 | buf[0];
      else if constexpr (size == 3)
        return buf[2] << 16 | buf[1] << 8 | buf[0];
      else if constexpr (size == 4)
        return buf[3] << 24 | buf[2] << 16 | buf[1] << 8 | buf[0];
      else
        return (u64)buf[7] << 56 | (u64)buf[6] << 48 |
               (u64)buf[5] << 40 | (u64)buf[4] << 32 |
               (u64)buf[3] << 24 | (u64)buf[2] << 16 |
               (u64)buf[1] << 8  | (u64)buf[0];
    } else {
      if constexpr (size == 2)
        return buf[0] << 8 | buf[1];
      else if constexpr (size == 3)
        return buf[0] << 16 | buf[1] << 8 | buf[2];
      else if constexpr (size == 4)
        return buf[0] << 24 | buf[1] << 16 | buf[2] << 8 | buf[3];
      else
        return (u64)buf[0] << 56 | (u64)buf[1] << 48 |
               (u64)buf[2] << 40 | (u64)buf[3] << 32 |
               (u64)buf[4] << 24 | (u64)buf[5] << 16 |
               (u64)buf[6] << 8  | (u64)buf[7];
    }
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
  u8 buf[size];
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
