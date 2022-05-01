#pragma once

#include <cstring>
#include <cstdint>
#include <type_traits>

namespace mold {

template<class ValueType, std::size_t Alignment, std::size_t Sizeof = sizeof(ValueType)>
class Packed {
  static_assert(std::is_integral_v<ValueType>, "Integral ValueType is required");
public:
  Packed() : value({}) {}

  operator ValueType() const {
    return read(value.buffer);
  }

  explicit Packed(const ValueType &other) {
    write(value.buffer, other);
  }

  void operator=(ValueType new_value) {
    write(value.buffer, new_value);
  }

  Packed &operator+=(ValueType new_value) {
    *this = *this + new_value;
    return *this;
  }

  Packed &operator-=(ValueType new_value) {
    *this = *this - new_value;
    return *this;
  }

  Packed& operator++() {
    *this = *this + 1;
    return *this;
  }

  Packed operator++(int) {
    Packed old_value = *this;
    operator++();
    return old_value;
  }

  Packed &operator|=(ValueType new_value) {
    *this = *this | new_value;
    return *this;
  }
private:
  struct {
    alignas(Alignment) uint8_t buffer[Sizeof];
  } value;

  static ValueType read(const void *ptr) {
    ValueType result = 0;
    std::memcpy(&result, ptr, Sizeof);
    return result;
  }

  static void write(void *ptr, ValueType new_value) {
    std::memcpy(ptr, &new_value, Sizeof);
  }
};

}
