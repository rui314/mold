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

namespace mold {

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

template <typename T, size_t SIZE = sizeof(T)>
class LittleEndian {
public:
  LittleEndian() = default;
  LittleEndian(T x) { *this = x; }

  operator T() const {
    // Compilers are usually smart enough to convert this loop to a
    // simple data copy. https://godbolt.org/z/1qz177vzs
    T x = 0;
#pragma GCC unroll 8
    for (int i = 0; i < SIZE; i++)
      x |= (u64)val[i] << (i * 8);
    return x;
  }

  LittleEndian &operator=(T x) {
#pragma GCC unroll 8
    for (int i = 0; i < SIZE; i++)
      val[i] = x >> (i * 8);
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
  u8 val[SIZE];
};

using il16 = LittleEndian<i16>;
using il32 = LittleEndian<i32>;
using il64 = LittleEndian<i64>;
using ul16 = LittleEndian<u16>;
using ul24 = LittleEndian<u32, 3>;
using ul32 = LittleEndian<u32>;
using ul64 = LittleEndian<u64>;

template <typename T, size_t SIZE = sizeof(T)>
class BigEndian {
public:
  BigEndian() = default;
  BigEndian(T x) { *this = x; }

  operator T() const {
    // This is compiled to a MOV followed by a BSWAP.
    // https://godbolt.org/z/1qz177vzs
    T x = 0;
#pragma GCC unroll 8
    for (int i = 0, j = SIZE - 1; i < SIZE; i++, j--)
      x |= (u64)val[i] << (j * 8);
    return x;
  }

  BigEndian &operator=(T x) {
#pragma GCC unroll 8
    for (int i = 0, j = SIZE - 1; i < SIZE; i++, j--)
      val[i] = x >> (j * 8);
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
  u8 val[SIZE];
};

using ib16 = BigEndian<i16>;
using ib32 = BigEndian<i32>;
using ib64 = BigEndian<i64>;
using ub16 = BigEndian<u16>;
using ub24 = BigEndian<u32, 3>;
using ub32 = BigEndian<u32>;
using ub64 = BigEndian<u64>;

} // namespace mold
