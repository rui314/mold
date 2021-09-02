#include "mold.h"

namespace mold::elf {

static const char *locked = (char *)-1;

template <typename T>
ConcurrentMap<T>::ConcurrentMap() {}

template <typename T>
ConcurrentMap<T>::ConcurrentMap(i64 nbuckets) {
  resize(nbuckets);
}

template <typename T>
void ConcurrentMap<T>::resize(i64 nbuckets) {
  this->~ConcurrentMap();

  nbuckets = std::max<i64>(MIN_NBUCKETS, next_power_of_two(nbuckets));

  this->nbuckets = nbuckets;
  keys = (std::atomic<const char *> *)calloc(nbuckets, sizeof(keys[0]));
  sizes = (u32 *)calloc(nbuckets, sizeof(sizes[0]));
  values = (T *)calloc(nbuckets, sizeof(values[0]));
}

template <typename T>
ConcurrentMap<T>::~ConcurrentMap() {
  if (keys) {
    free((void *)keys);
    free((void *)sizes);
    free((void *)values);
  }
}

template <typename T>
std::pair<T *, bool>
ConcurrentMap<T>::insert(std::string_view key, u64 hash, const T &val) {
  if (!keys)
    return {nullptr, false};

  ASSERT(__builtin_popcount(nbuckets) == 1);
  i64 idx = hash & (nbuckets - 1);
  i64 retry = 0;

  while (retry < MAX_RETRY) {
    const char *ptr = keys[idx];
    if (ptr == locked) {
#ifdef __x86_64__
      asm volatile("pause" ::: "memory");
#endif
      continue;
    }

    if (ptr == nullptr) {
      if (!keys[idx].compare_exchange_weak(ptr, locked))
        continue;
      new (values + idx) T(val);
      sizes[idx] = key.size();
      keys[idx] = key.data();
      return {values + idx, true};
    }

    if (key.size() == sizes[idx] && memcmp(ptr, key.data(), sizes[idx]) == 0)
      return {values + idx, false};

    u64 mask = nbuckets / NUM_SHARDS - 1;
    idx = (idx & ~mask) | ((idx + 1) & mask);
    retry++;
  }

  ASSERT(false && "ConcurrentMap is full");
  return {nullptr, false};
}

#define INSTANTIATE(E)                          \
  template class ConcurrentMap<SectionFragment<E>>;

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(AARCH64);

} // namespace mold::elf
