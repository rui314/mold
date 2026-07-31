#pragma once

#include "atomics.h"
#include "integers.h"

#include <array>
#include <atomic>
#include <bit>
#include <bitset>
#include <cassert>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <filesystem>
#include <iostream>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/blocked_range.h>
#include <tbb/concurrent_vector.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for.h>
#include <vector>

#ifdef _WIN32
# include <io.h>
#else
# include <sys/mman.h>
# include <unistd.h>
#endif

#define XXH_INLINE_ALL 1
#include "../third-party/xxhash/xxhash.h"

#ifdef NDEBUG
# define unreachable() __builtin_unreachable()
#else
# define unreachable() assert(0 && "unreachable")
#endif

inline uint64_t hash_string(std::string_view str) {
  return XXH3_64bits(str.data(), str.size());
}

class HashCmp {
public:
  static size_t hash(const std::string_view &k) {
    return hash_string(k);
  }

  static bool equal(const std::string_view &k1, const std::string_view &k2) {
    return k1 == k2;
  }
};

namespace mold {

namespace ranges = std::ranges;
using namespace std::literals::string_literals;
using namespace std::literals::string_view_literals;

inline u64 combine_hash(u64 a, u64 b) {
  return a ^ (b + 0x9e3779b9 + (a << 6) + (a >> 2));
}

// TaggedPtr stores one of several pointer types and uses the low pointer bits
// to record which type it contains.
template <typename... Ts>
class TaggedPtr {
public:
  static_assert(sizeof...(Ts) > 0);

  TaggedPtr() = default;

  template <typename T> requires (std::is_same_v<T, Ts> || ...)
  TaggedPtr(T *ptr) {
    *this = ptr;
  }

  template <typename T> requires (std::is_same_v<T, Ts> || ...)
  TaggedPtr &operator=(T *ptr) {
    static_assert(alignof(T) >= 1ULL << TAG_BITS);
    value = (uintptr_t)ptr;
    if (ptr)
      value |= get_tag<T>();
    return *this;
  }

  TaggedPtr &operator=(std::nullptr_t) {
    value = 0;
    return *this;
  }

  template <typename T> requires (std::is_same_v<T, Ts> || ...)
  T *get() const {
    if (!value || (value & TAG_MASK) != get_tag<T>())
      return nullptr;
    return (T *)(value & ~TAG_MASK);
  }

private:
  template <typename T>
  static consteval uintptr_t get_tag() {
    bool matches[] = {std::is_same_v<T, Ts>...};
    for (i64 i = 0; i < sizeof...(Ts); i++)
      if (matches[i])
        return i;
    return 0;
  }

  static constexpr i64 TAG_BITS = std::bit_width((u64)sizeof...(Ts) - 1);
  static constexpr uintptr_t TAG_MASK = (1ULL << TAG_BITS) - 1;

  uintptr_t value = 0;
};

//
// perf.cc
//

// Counter is used to collect statistics numbers.
class Counter {
public:
  Counter(std::string_view name, i64 value = 0) : name(name), values(value) {
    static std::mutex mu;
    std::scoped_lock lock(mu);
    instances.push_back(this);
  }

  Counter &operator++(int) {
    if (enabled) [[unlikely]]
      values.local()++;
    return *this;
  }

  Counter &operator+=(int delta) {
    if (enabled) [[unlikely]]
      values.local() += delta;
    return *this;
  }

  static void print();

  static inline bool enabled = false;

private:
  i64 get_value();

  std::string_view name;
  tbb::enumerable_thread_specific<i64> values;

  static inline std::vector<Counter *> instances;
};

// Timer and TimeRecord records elapsed time (wall clock time)
// used by each pass of the linker.
struct TimerRecord {
  TimerRecord(std::string name, TimerRecord *parent = nullptr);
  void stop();

  std::string name;
  TimerRecord *parent;
  tbb::concurrent_vector<TimerRecord *> children;
  i64 start;
  i64 end;
  i64 user;
  i64 sys;
  bool stopped = false;
};

void
print_timer_records(tbb::concurrent_vector<std::unique_ptr<TimerRecord>> &);

template <typename Context>
class Timer {
public:
  Timer(Context &ctx, std::string name, Timer *parent = nullptr) {
    record = new TimerRecord(name, parent ? parent->record : nullptr);
    ctx.timer_records.emplace_back(record);
  }

  Timer(const Timer &) = delete;

  ~Timer() {
    record->stop();
  }

  void stop() {
    record->stop();
  }

private:
  TimerRecord *record;
};

//
// Utility functions
//

// Some C++ libraries haven't implemented std::has_single_bit yet.
inline bool has_single_bit(u64 val) {
  return std::popcount(val) == 1;
}

// Some C++ libraries haven't implemented std::bit_ceil yet.
inline u64 bit_ceil(u64 val) {
  if (has_single_bit(val))
    return val;
  return 1LL << (64 - std::countl_zero(val));
}

// ExactArray owns a fixed-size array without value-initializing trivial
// elements that the caller is about to overwrite.
template <typename T>
class ExactArray {
public:
  ExactArray() = default;

  explicit ExactArray(i64 sz)
    : ptr(new T[sz]), size_(sz) {}

  T &operator[](i64 i) {
    return ptr[i];
  }

  const T &operator[](i64 i) const {
    return ptr[i];
  }

  T *data() {
    return ptr.get();
  }

  const T *data() const {
    return ptr.get();
  }

  i64 size() const {
    return size_;
  }

private:
  std::unique_ptr<T[]> ptr;
  i64 size_ = 0;
};

inline u64 align_to(u64 val, u64 align) {
  if (align == 0)
    return val;
  assert(has_single_bit(align));
  return (val + align - 1) & ~(align - 1);
}

inline u64 align_down(u64 val, u64 align) {
  assert(has_single_bit(align));
  return val & ~(align - 1);
}

inline u64 bit(u64 val, i64 pos) {
  return (val >> pos) & 1;
};

// Returns [hi:lo] bits of val.
inline u64 bits(u64 val, u64 hi, u64 lo) {
  return (val >> lo) & ((1LL << (hi - lo + 1)) - 1);
}

// Cast val to a signed N bit integer.
// For example, sign_extend(x, 32) == (i32)x for any integer x.
inline i64 sign_extend(u64 val, i64 n) {
  return (i64)(val << (64 - n)) >> (64 - n);
}

inline bool is_int(u64 val, i64 n) {
  return sign_extend(val, n) == val;
}

template <typename T, typename Compare = std::less<T>>
void update_minimum(std::atomic<T> &atomic, u64 new_val, Compare cmp = {}) {
  T old_val = atomic.load(std::memory_order_relaxed);
  while (cmp(new_val, old_val) &&
         !atomic.compare_exchange_weak(old_val, new_val,
                                       std::memory_order_relaxed));
}

template <typename T, typename Compare = std::less<T>>
void update_maximum(std::atomic<T> &atomic, u64 new_val, Compare cmp = {}) {
  T old_val = atomic.load(std::memory_order_relaxed);
  while (cmp(old_val, new_val) &&
         !atomic.compare_exchange_weak(old_val, new_val,
                                       std::memory_order_relaxed));
}

template <typename T>
inline void append(std::vector<T> &x, const auto &y) {
  x.insert(x.end(), y.begin(), y.end());
}

template <typename T>
inline std::vector<T> flatten(std::vector<std::vector<T>> &vec) {
  i64 size = 0;
  for (std::vector<T> &v : vec)
    size += v.size();

  std::vector<T> ret;
  ret.reserve(size);
  for (std::vector<T> &v : vec)
    append(ret, v);
  return ret;
}

template <typename T>
inline void remove_duplicates(std::vector<T> &vec) {
  vec.erase(std::unique(vec.begin(), vec.end()), vec.end());
}

inline i64 write_string(void *buf, std::string_view str) {
  memcpy(buf, str.data(), str.size());
  *((u8 *)buf + str.size()) = '\0';
  return str.size() + 1;
}

template <typename T>
inline void write_vector(void *buf, const std::vector<T> &vec) {
  if (!vec.empty())
    memcpy(buf, vec.data(), vec.size() * sizeof(T));
}

inline void parallel_memcpy(void *dst, const void *src, i64 size) {
  constexpr i64 BLOCK_SIZE = 2 * 1024 * 1024;

  tbb::parallel_for(tbb::blocked_range<i64>(0, size, BLOCK_SIZE),
                    [&](const tbb::blocked_range<i64> &range) {
                      memcpy((u8 *)dst + range.begin(),
                             (const u8 *)src + range.begin(), range.size());
                    });
}

inline void encode_uleb(std::vector<u8> &vec, u64 val) {
  do {
    u8 byte = val & 0x7f;
    val >>= 7;
    vec.push_back(val ? (byte | 0x80) : byte);
  } while (val);
}

inline void encode_sleb(std::vector<u8> &vec, i64 val) {
  for (;;) {
    u8 byte = val & 0x7f;
    val >>= 7;

    bool neg = (byte & 0x40);
    if ((val == 0 && !neg) || (val == -1 && neg)) {
      vec.push_back(byte);
      break;
    }
    vec.push_back(byte | 0x80);
  }
}

inline i64 write_uleb(u8 *buf, u64 val) {
  i64 i = 0;
  do {
    u8 byte = val & 0x7f;
    val >>= 7;
    buf[i++] = val ? (byte | 0x80) : byte;
  } while (val);
  return i;
}

inline u64 read_uleb(u8 **buf) {
  u64 val = 0;
  u8 shift = 0;
  u8 byte;
  do {
    byte = *(*buf)++;
    val |= (byte & 0x7f) << shift;
    shift += 7;
  } while (byte & 0x80);
  return val;
}

inline u64 read_uleb(u8 *buf) {
  u8 *tmp = buf;
  return read_uleb(&tmp);
}

inline i64 read_sleb(u8 **buf) {
  u64 val = 0;
  u8 shift = 0;
  u8 byte;
  do {
    byte = *(*buf)++;
    val |= (byte & 0x7f) << shift;
    shift += 7;
  } while (byte & 0x80);
  return sign_extend(val, shift);
}

inline i64 read_sleb(u8 *buf) {
  u8 *tmp = buf;
  return read_sleb(&tmp);
}

inline u64 read_uleb(std::string_view *str) {
  u8 *start = (u8 *)str->data();
  u8 *ptr = start;
  u64 val = read_uleb(&ptr);
  *str = str->substr(ptr - start);
  return val;
}

inline u64 read_uleb(std::string_view str) {
  std::string_view tmp = str;
  return read_uleb(&tmp);
}

inline i64 uleb_size(u64 val) {
  for (int i = 1; i < 9; i++)
    if (val < (1LL << (7 * i)))
      return i;
  return 9;
}

inline void overwrite_uleb(u8 *loc, u64 val) {
  while (*loc & 0b1000'0000) {
    *loc++ = 0b1000'0000 | (val & 0b0111'1111);
    val >>= 7;
  }
  *loc = val & 0b0111'1111;
}

static inline void pause() {
#if defined(__x86_64__)
  asm volatile("pause");
#elif defined(__aarch64__)
  asm volatile("yield");
#elif defined(__ARM_ARCH_7A__) || defined(__ARM_ARCH_8A__)
  asm volatile("yield");
#endif
}

//
// Concurrent Map
//

// This is an implementation of a fast concurrent hash map. Unlike
// ordinary hash tables, this impl just aborts if it becomes full.
// So you need to give a correct estimation of the final size before
// using it. We use this hash map to uniquify pieces of data in
// mergeable sections.
//
// We've implemented this ourselves because the performance of
// conrurent hash map is critical for our linker.
template <typename T>
class ConcurrentMap {
public:
  ConcurrentMap() = default;

  ConcurrentMap(i64 nbuckets) {
    resize(nbuckets);
  }

  ~ConcurrentMap() {
    if (entries) {
#ifdef _WIN32
      _aligned_free(entries);
#else
      munmap(entries, sizeof(Entry) * nbuckets);
#endif
    }
  }

  // In order to avoid unnecessary cache-line false sharing, we want
  // to make this object to be aligned to a reasonably large
  // power-of-two address.
  struct alignas(32) Entry {
    Atomic<const char *> key;
    u32 keylen;
    T value;
  };

  void resize(i64 nbuckets) {
    assert(!entries);
    this->nbuckets = std::max<i64>(MIN_NBUCKETS, bit_ceil(nbuckets));
    i64 bufsize = sizeof(Entry) * this->nbuckets;

    // Allocate a zero-initialized buffer. mmap is faster than
    // malloc + memset.
#ifdef _WIN32
    entries = (Entry *)_aligned_malloc(bufsize, alignof(Entry));
    memset((void *)entries, 0, bufsize);
#else
    entries = (Entry *)mmap(nullptr, bufsize, PROT_READ | PROT_WRITE,
                            MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
#endif

#ifdef MADV_HUGEPAGE
    madvise((void *)entries, bufsize, MADV_HUGEPAGE);
#endif
  }

  std::pair<T *, bool> insert(std::string_view key, u64 hash, const T &val) {
    return insert_entry(key.data(), hash, val,
                        [&](const char *ptr, u32 len) {
                          return key == std::string_view(ptr, len);
                        },
                        [&](T &) { return (u32)key.size(); });
  }

  std::pair<T *, bool> insert_cstr(const char *key, u64 hash, const T &val,
                                   auto initialize) {
    // This variant avoids storing a length alongside every caller-side key.
    // Only a newly inserted key needs its length computed by `initialize`.
    return insert_entry(
      key, hash, val,
      [&](const char *ptr, u32) { return !strcmp(key, ptr); },
      [&](T &value) { return initialize(value, key); });
  }

  i64 get_idx(T *value) const {
    uintptr_t addr = (uintptr_t)value - (uintptr_t)value % sizeof(Entry);
    return (Entry *)addr - entries;
  }

  // Return a list of map entries sorted in a deterministic order.
  std::vector<Entry *> get_sorted_entries(i64 shard_idx) {
    if (nbuckets == 0)
      return {};

    i64 shard_size = nbuckets / NUM_SHARDS;
    i64 begin = shard_idx * shard_size;
    i64 end = begin + shard_size;

    i64 sz = 0;
    for (i64 i = begin; i < end; i++)
      if (entries[i].key)
        sz++;

    std::vector<Entry *> vec;
    vec.reserve(sz);

    // Since the shard is circular, we need to handle the last entries
    // as if they were next to the first entries.
    while (begin < end && entries[end - 1].key)
      vec.push_back(entries + --end);

    // Find entries contiguous in the buckets and sort them.
    i64 last = 0;
    for (i64 i = begin; i < end;) {
      while (i < end && entries[i].key)
        vec.push_back(entries + i++);

      std::sort(vec.begin() + last, vec.end(), [](Entry *a, Entry *b) {
        if (a->keylen != b->keylen)
          return a->keylen < b->keylen;
        return memcmp(a->key, b->key, a->keylen) < 0;
      });

      last = vec.size();

      while (i < end && !entries[i].key)
        i++;
    }
    return vec;
  }

  std::vector<Entry *> get_sorted_entries_all() {
    std::vector<std::vector<Entry *>> vec(NUM_SHARDS);
    tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
      vec[i] = get_sorted_entries(i);
    });
    return flatten(vec);
  }

  static constexpr i64 MIN_NBUCKETS = 4096;
  static constexpr i64 NUM_SHARDS = 16;
  static constexpr i64 MAX_RETRY = 256;

  Entry *entries = nullptr;
  u64 nbuckets = 0;

private:
  std::pair<T *, bool> insert_entry(const char *key, u64 hash, const T &val,
                                    auto equals, auto initialize) {
    assert(has_single_bit(nbuckets));

    u64 begin = hash & (nbuckets - 1);
    u64 mask = nbuckets / NUM_SHARDS - 1;

    for (i64 i = 0; i < MAX_RETRY; i++) {
      u64 idx = (begin & ~mask) | ((begin + i) & mask);
      Entry &ent = entries[idx];

      // Avoid an atomic update when the slot is already occupied.
      if (const char *ptr = ent.key.load(std::memory_order_acquire);
          ptr != nullptr && ptr != (char *)-1) {
        if (equals(ptr, ent.keylen))
          return {&ent.value, false};
        continue;
      }

      const char *ptr = nullptr;
      bool claimed = ent.key.compare_exchange_strong(ptr, (char *)-1,
                                                     std::memory_order_acquire);

      // -1 marks the slot as claimed until its value has been initialized.
      if (claimed) {
        new (&ent.value) T(val);
        ent.keylen = initialize(ent.value);
        ent.key.store(key, std::memory_order_release);
        return {&ent.value, true};
      }

      // Wait for the thread that claimed the slot to publish its key.
      while (ptr == (char *)-1) {
        pause();
        ptr = ent.key.load(std::memory_order_acquire);
      }

      if (equals(ptr, ent.keylen))
        return {&ent.value, false};
    }

    std::cerr << "ConcurrentMap is full\n";
    abort();
  }
};

static_assert(sizeof(void *) == 4 || sizeof(void *) == 8);

// ArenaResource owns a sparsely-backed address range for symbols, input files,
// and related linker data structures. Allocation is thread-safe and monotonic;
// individual allocations are not freed. Keeping related objects in this range
// lets ArenaPtr represent references between them as 32-bit self-relative
// offsets.
class ArenaResource {
public:
  // An 8 GiB arena ensures that the distance between any two allocations fits
  // in ArenaPtr's signed 32-bit offset. A smaller reservation is used on
  // 32-bit hosts, where address space is more limited.
  static constexpr u64 SIZE = sizeof(void *) == 8 ? 1ULL << 33 : 1ULL << 28;

  ArenaResource();
  ~ArenaResource();
  ArenaResource(const ArenaResource &) = delete;
  ArenaResource &operator=(const ArenaResource &) = delete;

  void *allocate(u64 size, u64 alignment);

  template <typename T>
  T *allocate(i64 count) {
    if (count < 0 || (u64)count > SIZE / sizeof(T)) {
      std::cerr << "mold: cannot allocate more than " << SIZE
                << " bytes for linker data structures on this host\n";
      std::exit(1);
    }
    return (T *)allocate(sizeof(T) * (u64)count, alignof(T));
  }

  template <typename T, typename... Args>
  T *make(Args &&...args) {
    return std::construct_at(allocate<T>(1), std::forward<Args>(args)...);
  }

  // ArenaPtr works only if the pointer field itself is within 8 GiB of its
  // target. Records stored outside the arena instead use an index from the
  // beginning of the arena. Indices count four-byte slots, and zero represents
  // a null pointer.
  u32 get_index(const void *ptr) const {
    uintptr_t addr = (uintptr_t)ptr;
    uintptr_t begin = (uintptr_t)data;
    assert(begin < addr && addr < begin + SIZE);
    assert((addr - begin) % 4 == 0);
    return (addr - begin) / 4;
  }

  template <typename T>
  T *get_pointer(u32 idx) const {
    assert(0 < idx && (u64)idx * 4 < SIZE);
    return (T *)(data + (u64)idx * 4);
  }

private:
  u8 *data;

  // Leave the first slots unused so that a base-relative index is never zero.
  std::atomic<u64> offset = 8;
};

// ArenaAllocator adapts ArenaResource to the standard allocator interface so
// containers can place their backing storage in the resource's address range.
// It does not own the resource, and deallocation is deferred until the
// resource itself is destroyed.
template <typename T>
class ArenaAllocator {
public:
  using value_type = T;

  ArenaAllocator(ArenaResource &arena) : arena(arena) {}

  template <typename U>
  ArenaAllocator(const ArenaAllocator<U> &other) : arena(other.arena) {}

  T *allocate(i64 count) {
    return arena.template allocate<T>(count);
  }

  void deallocate(T *, i64) {}

  template <typename U>
  bool operator==(const ArenaAllocator<U> &other) const {
    return &arena == &other.arena;
  }

private:
  template <typename U> friend class ArenaAllocator;

  ArenaResource &arena;
};

// ArenaPtr stores a pointer as a signed 32-bit offset from itself, in units of
// four bytes. It is used for references between objects in an ArenaResource;
// the ArenaPtr and its target must be four-byte aligned and less than 8 GiB
// apart. An offset of zero represents a null pointer.
template <typename T>
class alignas(4) ArenaPtr {
public:
  ArenaPtr() = default;
  ArenaPtr(std::nullptr_t) {}
  ArenaPtr(T *ptr) { *this = ptr; }

  // The offset is relative to this ArenaPtr, so copying it verbatim would
  // make it point somewhere else.
  ArenaPtr(const ArenaPtr &other) { *this = (T *)other; }
  ArenaPtr(ArenaPtr &&other) { *this = (T *)other; }

  ArenaPtr &operator=(T *ptr) {
    if (!ptr) {
      offset = 0;
      return *this;
    }

    i64 byte_offset = (intptr_t)ptr - (intptr_t)this;
    assert(byte_offset % 4 == 0);
    assert(is_int(byte_offset / 4, 32));
    offset = byte_offset / 4;
    assert(offset != 0);
    return *this;
  }

  ArenaPtr &operator=(const ArenaPtr &other) {
    return *this = (T *)other;
  }

  ArenaPtr &operator=(ArenaPtr &&other) {
    return *this = (T *)other;
  }

  operator T *() const {
    return offset ? (T *)((intptr_t)this + (i64)offset * 4) : nullptr;
  }

  T *operator->() const {
    return (T *)*this;
  }

  T &operator*() const {
    return *(T *)*this;
  }

private:
  i32 offset = 0;
};

static_assert(sizeof(ArenaPtr<u64>) == 4);

// ArenaObjectDeleter runs an arena object's destructor without freeing its
// storage. ArenaObjectPtr uses it to retain normal unique_ptr ownership
// semantics for objects whose storage belongs to ArenaResource.
template <typename T>
struct ArenaObjectDeleter {
  void operator()(T *ptr) const {
    std::destroy_at(ptr);
  }
};

template <typename T>
using ArenaObjectPtr = std::unique_ptr<T, ArenaObjectDeleter<T>>;

template <typename T>
struct ShardedMapEntry : T {
  ShardedMapEntry(std::string_view key) : key(key) {}

  // Keeping the key outside T means that values not stored in a map do not
  // pay for it. A mapped T is the base subobject of this entry.
  std::string_view key;
};

template <typename T>
std::string_view get_sharded_map_key(const T &object) {
  return static_cast<const ShardedMapEntry<T> &>(object).key;
}

// ShardedMap is a map from strings to values of type T, built in two phases.
// In the first phase, which may run in parallel, add() records a key and an
// ArenaPtr<T> slot that needs the key's value. gather() then deduplicates the
// keys, finds or creates one value for each key, and writes its address to
// every recorded slot. The slots must remain at stable addresses until
// gather().
//
// Keys whose hashes fall into different shards never interact, and
// each shard is processed by exactly one thread during gather(), so
// unlike with a concurrent hash table, no synchronization is needed,
// and each key is hashed only once, in add().
//
// If a value needs to be initialized from its key, pass an
// `on_create(key, value)` callback to gather() or insert(). It is called
// exactly once when a value is created.
//
// insert() handles keys that arrive outside the two-phase pattern under a
// shard mutex. All values live in stable arena blocks.
template <typename T>
class ShardedMap {
public:
  explicit ShardedMap(ArenaResource &arena) {
    for (Shard &shard : shards)
      shard.arena = &arena;
  }

  void add(std::string_view key, ArenaPtr<T> &slot) {
    u64 hash = hash_string(key);
    bins.local()[hash % NUM_SHARDS].push_back({key, hash, &slot});
  }

  T *insert(std::string_view key, auto on_create) {
    u64 hash = hash_string(key);
    Shard &shard = shards[hash % NUM_SHARDS];
    std::scoped_lock lock(shard.mu);
    return shard.insert(key, hash, on_create);
  }

  void gather(auto on_create) {
    tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
      Shard &shard = shards[i];

      i64 count = 0;
      for (Bin &bin : bins)
        count += bin[i].size();
      if (count == 0)
        return;

      shard.reserve(count);

      for (Bin &bin : bins)
        for (Pending &p : bin[i])
          *p.slot = shard.insert(p.key, p.hash, on_create);
    });

    bins.clear();
  }

  void parallel_for_each(auto fn) {
    tbb::parallel_for((i64)0, NUM_SHARDS, [&](i64 i) {
      Shard &shard = shards[i];
      for (Block &block : shard.blocks)
        for (i64 j = 0; j < block.size; j++)
          fn(block.data[j]);
    });
  }

private:
  static constexpr i64 NUM_SHARDS = 64;
  using Entry = ShardedMapEntry<T>;
  static_assert(std::is_trivially_destructible_v<Entry>);

  struct Block {
    Entry *data;
    i64 size;
    i64 capacity;
  };

  struct Pending {
    std::string_view key;
    u64 hash;
    ArenaPtr<T> *slot;
  };

  // add() already computed the string hash. Store it in the map key so that
  // unordered_map does not scan the string again.
  struct PassThroughHash {
    size_t operator()(const std::pair<u64, std::string_view> &key) const {
      return key.first;
    }
  };

  using Map = std::unordered_map<std::pair<u64, std::string_view>, Entry *,
                                 PassThroughHash>;

  struct Shard {
    Shard() = default;
    Shard(const Shard &) = delete;
    Shard &operator=(const Shard &) = delete;

    T *insert(std::string_view key, u64 hash, auto on_create) {
      auto [it, inserted] = map.try_emplace({hash, key}, nullptr);
      if (!inserted)
        return it->second;

      if (blocks.empty() || blocks.back().size == blocks.back().capacity)
        add_block(INSERT_BLOCK_SIZE);

      Entry *entry = emplace(blocks.back(), key);
      it->second = entry;
      on_create(key, *entry);
      return entry;
    }

    void reserve(i64 count) {
      map.reserve(map.size() + count);
      if (blocks.empty() ||
          blocks.back().capacity - blocks.back().size < count)
        add_block(std::max<i64>(INSERT_BLOCK_SIZE, count));
    }

    static constexpr i64 INSERT_BLOCK_SIZE = 256;

    void add_block(i64 capacity) {
      Entry *data = arena->allocate<Entry>(capacity);
      blocks.push_back({data, 0, capacity});
    }

    Entry *emplace(Block &block, std::string_view key) {
      assert(block.size < block.capacity);
      Entry *entry = block.data + block.size++;
      return std::construct_at(entry, key);
    }

    std::mutex mu;
    ArenaResource *arena = nullptr;
    std::vector<Block> blocks;
    Map map;
  };

  using Bin = std::array<std::vector<Pending>, NUM_SHARDS>;

  Shard shards[NUM_SHARDS];
  tbb::enumerable_thread_specific<Bin> bins;
};

//
// random.cc
//

void get_random_bytes(u8 *buf, i64 size);

//
// hyperloglog.cc
//

class HyperLogLog {
private:
  static constexpr i64 NBUCKETS = 2048;
  static constexpr double ALPHA = 0.79402;

public:
  class Sketch {
  public:
    void insert(u64 hash) {
      u8 &val = buckets[hash & (NBUCKETS - 1)];
      val = std::max<u8>(val, std::countl_zero(hash) + 1);
    }

  private:
    friend class HyperLogLog;
    u8 buckets[NBUCKETS] = {};
  };

  Sketch &local() {
    return sketches.local();
  }

  i64 get_cardinality() const;

private:
  tbb::enumerable_thread_specific<Sketch> sketches;
};

//
// aho-corasick.cc
//

class AhoCorasick {
public:
  bool add(std::string_view pat, i64 val);
  bool empty() const { return nodes.empty(); }
  void compile();
  i64 find(std::string_view str);

  static bool can_handle(std::string_view str);

private:
  struct TrieNode {
    i64 value = -1;
    i32 suffix_link = -1;
    i32 first_child = -1;
    i32 next_sibling = -1;
    u8 ch = 0;
  };

  i32 find_child(i32 node, u8 ch) const;
  i32 add_child(i32 node, u8 ch);

  // Most trie nodes have only one child. The root uses a dense table because
  // it is visited for almost every input byte; other edges are stored sparsely.
  std::array<i32, 256> root_children;
  std::vector<TrieNode> nodes;
};

class Glob {
public:
  bool add(std::string_view pat, i64 val);
  bool empty() const { return is_empty; }
  i64 find(std::string_view str);

private:
  std::once_flag once;
  bool is_empty = true;
  bool is_compiled = false;

  // Patterns that need only a literal string comparison are kept out
  // of the automaton-based matchers below, which scan the entire input
  // string per query. Real version scripts consist almost entirely of
  // such patterns (e.g. `local: *;` or `v8dbg_*;`), and we match them
  // against every defined symbol name.
  struct LiteralPattern {
    std::string pat;
    i64 value;
  };

  struct Pattern {
    enum Kind { STRING, STAR, QUESTION, BRACKET };

    struct Token {
      Token(Kind kind) : kind(kind) {}

      Kind kind;
      std::string str;
      std::bitset<256> chars;
    };

    Pattern(std::vector<Token> &&tokens, i64 value)
      : tokens(std::move(tokens)), value(value) {}

    static std::optional<Pattern> compile(std::string_view pat, i64 value);
    bool match(std::string_view str) const;

    std::vector<Token> tokens;
    i64 value;
  };

  // Nfa matches many glob patterns in parallel by representing each state
  // with one bit. It is used only for large pattern sets; matching individual
  // patterns is faster when there are only a few of them.
  struct Nfa {
    void compile(std::span<const Pattern> patterns);
    i64 match(std::string_view str) const;
    bool empty() const { return initial_states.empty(); }

    std::vector<u64> initial_states;
    std::vector<u64> star_states;
    std::vector<u64> accept_states;
    std::vector<u64> char_masks;
    std::vector<i64> values;
  };

  i64 match_all = -1;                   // "*"
  std::vector<LiteralPattern> exacts;   // "foo"
  std::vector<LiteralPattern> prefixes; // "foo*"
  std::vector<LiteralPattern> suffixes; // "*foo"
  std::vector<Pattern> patterns;        // "foo*bar"

  Nfa nfa;
  AhoCorasick aho_corasick;
};

//
// filepath.cc
//

inline std::filesystem::path path_dirname(std::string_view path) {
  return std::filesystem::path(path).parent_path();
}

inline std::string path_filename(std::string_view path) {
  return std::filesystem::path(path).filename().string();
}

inline std::string path_clean(std::string_view path) {
  return std::filesystem::path(path).lexically_normal().string();
}

std::string get_self_path();

//
// demangle.cc
//

std::optional<std::string_view> demangle_cpp(std::string_view name);
std::optional<std::string_view> demangle_rust(std::string_view name);

//
// crc32.cc
//

u32 compute_crc32(u32 crc, u8 *buf, i64 len);
std::vector<u8> crc32_solve(u32 current, u32 desired);

//
// compress.cc
//

class Compressor {
public:
  virtual void write_to(u8 *buf) = 0;
  virtual ~Compressor();
  i64 compressed_size = 0;

protected:
  std::vector<std::span<u8>> shards;
};

class ZlibCompressor : public Compressor {
public:
  ZlibCompressor(u8 *buf, i64 size, i64 level);
  void write_to(u8 *buf) override;

private:
  u32 checksum = 0;
};

class ZstdCompressor : public Compressor {
public:
  ZstdCompressor(u8 *buf, i64 size, i64 level);
  void write_to(u8 *buf) override;
};

//
// tar.cc
//

// TarFile is a class to create a tar file.
//
// If you pass `--repro` to mold, mold collects all input files and
// put them into `<output-file-path>.repro.tar`, so that it is easy to
// run the same command with the same command line arguments.
class TarWriter {
public:
  static std::unique_ptr<TarWriter>
  open(std::string output_path, std::string basedir);

  ~TarWriter();
  void append(std::string path, std::string_view data);

private:
  TarWriter(FILE *out, std::string basedir) : out(out), basedir(basedir) {}

  FILE *out = nullptr;
  std::string basedir;
};

} // namespace mold
