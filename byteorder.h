#pragma once

#include <cstdint>

namespace mold {

template <typename T>
class Int {
public:
  Int() : Int(0) {}

  Int(T x) {
    *this = x;
  }

  operator T() const {
    T ret = 0;
    for (int i = 0; i < sizeof(T); i++)
      ret = (ret << 8) | val[i];
    return ret;
  }

  Int &operator=(T x) {
    for (int i = 0; i < sizeof(T); i++)
      val[sizeof(T) - 1 - i] = x >> (i * 8);
    return *this;
  }

  Int &operator++() {
    return *this = *this + 1;
  }

  Int operator++(int) {
    T ret = *this;
    *this = *this + 1;
    return ret;
  }

  Int &operator--() {
    return *this = *this - 1;
  }

  Int operator--(int) {
    T ret = *this;
    *this = *this - 1;
    return ret;
  }

  Int &operator+=(T x) {
    return *this = *this + x;
  }

  Int &operator&=(T x) {
    return *this = *this & x;
  }

  Int &operator|=(T x) {
    return *this = *this | x;
  }

private:
  uint8_t val[sizeof(T)];
};

class ibig16 : public Int<int16_t> { using Int::Int; };
class ibig32 : public Int<int32_t> { using Int::Int; };
class ibig64 : public Int<int64_t> { using Int::Int; };
class ubig16 : public Int<uint16_t> { using Int::Int; };
class ubig32 : public Int<uint32_t> { using Int::Int; };
class ubig64 : public Int<uint64_t> { using Int::Int; };

} // namespace mold
