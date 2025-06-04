// This is the same as std::atomic except that the default memory
// order is relaxed instead of sequential consistency.

#pragma once

#include <atomic>

namespace mold {

template <typename T>
struct Atomic : std::atomic<T> {
  static constexpr std::memory_order relaxed = std::memory_order_relaxed;

  using std::atomic<T>::atomic;

  Atomic(const Atomic<T> &other) : std::atomic<T>(other.load()) {}

  Atomic<T> &operator=(const Atomic<T> &other) {
    store(other.load());
    return *this;
  }

  void operator=(T val) { store(val); }
  operator T() const { return load(); }

  void store(T val, std::memory_order order = relaxed) {
    std::atomic<T>::store(val, order);
  }

  T load(std::memory_order order = relaxed) const {
    return std::atomic<T>::load(order);
  }

  T exchange(T val) { return std::atomic<T>::exchange(val, relaxed); }
  T operator|=(T val) { return std::atomic<T>::fetch_or(val, relaxed); }
  T operator++() { return std::atomic<T>::fetch_add(1, relaxed) + 1; }
  T operator--() { return std::atomic<T>::fetch_sub(1, relaxed) - 1; }
  T operator++(int) { return std::atomic<T>::fetch_add(1, relaxed); }
  T operator--(int) { return std::atomic<T>::fetch_sub(1, relaxed); }

  bool test_and_set() {
    // A relaxed load + branch (assuming miss) takes only around 20 cycles,
    // while an atomic RMW can easily take hundreds on x86. We note that it's
    // common that another thread beat us in marking, so doing an optimistic
    // early test tends to improve performance in the ~20% ballpark.
    return load() || exchange(true);
  }
};

} // namespace mold
