// This file defines integral types for file input/output. We need to use
// these types instead of the plain integers (such as uint32_t or int32_t)
// when loading from/writing to an mmap'ed file area for the following
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
//    example, to load a 32 bit value.
//
// The data types defined in this file don't depend on host byte order and
// don't do unaligned access. Note that modern compilers are smart enough
// to recognize shift and bitwise OR patterns and compile them into a
// single load instruction.

#pragma once

#include <cstdint>

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

using u8 = uint8_t;
using u16 = uint16_t;
using u32 = uint32_t;
using u64 = uint64_t;

using i8 = int8_t;
using i16 = int16_t;
using i32 = int32_t;
using i64 = int64_t;

template <typename T, bool is_le, int size = sizeof(T)>
class Integer {
public:
  constexpr Integer() = default;
  constexpr Integer(T v) { store(v); }
  constexpr operator T() const { return load(); }

  Integer &operator=(T v)  { store(v); return *this; }
  Integer &operator++()    { return *this = *this + 1; }
  Integer operator++(int)  { Integer x = *this; ++*this; return x; }
  Integer &operator--()    { return *this = *this - 1; }
  Integer operator--(int)  { Integer x = *this; --*this; return x; }
  Integer &operator+=(T v) { return *this = *this + v; }
  Integer &operator-=(T v) { return *this = *this - v; }
  Integer &operator&=(T v) { return *this = *this & v; }
  Integer &operator|=(T v) { return *this = *this | v; }

private:
  constexpr T load() const {
    T v = 0;

    // Without this pragma, GCC 14 fails to optimize the following loop
    // into a single load instruction. Clang recognize this pragma as
    // well, though Clang can optimize this without the pragma.
#pragma GCC unroll 8
    for (int i = 0; i < size; i++) {
      int j = is_le ? i : (size - i - 1);
      v |= (T)buf[j] << (i * 8);
    }
    return v;
  }

  constexpr void store(T v) {
    for (int i = 0; i < size; i++) {
      int j = is_le ? i : (size - i - 1);
      buf[j] = v >> (i * 8);
    }
  }

  u8 buf[size];
};

using il16 = Integer<i16, true>;
using il32 = Integer<i32, true>;
using il64 = Integer<i64, true>;

using ul16 = Integer<u16, true>;
using ul24 = Integer<u32, true, 3>;
using ul32 = Integer<u32, true>;
using ul64 = Integer<u64, true>;

using ib16 = Integer<i16, false>;
using ib32 = Integer<i32, false>;
using ib64 = Integer<i64, false>;

using ub16 = Integer<u16, false>;
using ub24 = Integer<u32, false, 3>;
using ub32 = Integer<u32, false>;
using ub64 = Integer<u64, false>;

} // namespace mold
