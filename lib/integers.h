// This file defines integral types for file input/output. We need to use
// these types instead of the plain integers (such as uint32_t or int32_t)
// when reading from/writing to an mmap'ed file area for the following
// reasons:
//
// 1. mold is always a cross linker and should not depend on what host it
//    is running on. For example, users should be able to run mold on a
//    little-endian x86 machine to create a big-endian s390x binary.
//
// 2. Even though data members in all ELF data strucutres are naturally
//    aligned, they are not guaranteed to be aligned on memory because of
//    archive files. Archive files (.a files) align each file only to a
//    2 byte boundary, so anything larger than 2 bytes may be misaligned
//    in an mmap'ed memory. Misaligned access is an undefined behavior in
//    C/C++, so we shouldn't cast an arbitrary pointer to a uint32_t, for
//    example, to read a 32 bit value.
//
// The data types defined in this file are independent of the host byte
// order and are designed to avoid unaligned access.
//
// Note that in C/C++, memcpy is a portable and efficient way to access
// unaligned data, as it is typically treated as an intrinsic. Compilers
// can easily optimize memcpy calls in this file into a single load or
// store instruction.

#pragma once

#include <bit>
#include <cstdint>
#include <cstring>
#include <type_traits>

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

template <typename T, bool is_le, int size = sizeof(T)>
class Integer {
public:
  constexpr Integer() = default;

  constexpr Integer(T v) {
    if (std::is_constant_evaluated()) {
      for (int i = 0; i < size; i++) {
        int j = is_le ? i : (size - i - 1);
        buf[j] = v >> (i * 8);
      }
    } else {
      store(v);
    }
  }

  operator T() const { return load(); }
  Integer &operator=(T v)  { store(v); return *this; }
  Integer &operator++()    { return *this = *this + 1; }
  Integer operator++(int)  { auto x = *this; ++*this; return x; }
  Integer &operator--()    { return *this = *this - 1; }
  Integer operator--(int)  { auto x = *this; --*this; return x; }
  Integer &operator+=(T v) { return *this = *this + v; }
  Integer &operator-=(T v) { return *this = *this - v; }
  Integer &operator&=(T v) { return *this = *this & v; }
  Integer &operator|=(T v) { return *this = *this | v; }

private:
  static constexpr bool is_native =
    (std::endian::native == (is_le ? std::endian::little : std::endian::big));

  static T bswap(T v) {
    switch (size) {
    case 2: return __builtin_bswap16(v);
    case 3: __builtin_unreachable();
    case 4: return __builtin_bswap32(v);
    case 8: return __builtin_bswap64(v);
    }
  }

  T load() const {
    if (size == 3) {
      if (is_le)
        return buf[2] << 16 | buf[1] << 8 | buf[0];
      return buf[0] << 16 | buf[1] << 8 | buf[2];
    }

    T v;
    memcpy(&v, buf, size);
    return is_native ? v : bswap(v);
  }

  // We cannot merge this with the constructor because memcpy is not
  // allowed to use in a compile-time constant expression.
  void store(T v) {
    if (size == 3) {
      if (is_le) {
        buf[0] = v;
        buf[1] = v >> 8;
        buf[2] = v >> 16;
      } else {
        buf[0] = v >> 16;
        buf[1] = v >> 8;
        buf[2] = v;
      }
      return;
    }

    if (!is_native)
      v = bswap(v);
    memcpy(buf, &v, size);
  }

  uint8_t buf[size];
};

using i8 = int8_t;
using i16 = int16_t;
using i32 = int32_t;
using i64 = int64_t;

using u8 = uint8_t;
using u16 = uint16_t;
using u32 = uint32_t;
using u64 = uint64_t;

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
