#pragma once

#include "config.h"
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
#include <mutex>
#include <optional>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <sys/stat.h>
#include <sys/types.h>
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

//
// Atomics
//

// This is the same as std::atomic except that the default memory
// order is relaxed instead of sequential consistency.
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

//
// mimalloc.cc
//

void set_mimalloc_options();

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

    // Allocate a zero-initialized buffer. We use mmap() if available
    // because it's faster than malloc() and memset().
#ifdef _WIN32
    entries = (Entry *)_aligned_malloc(bufsize, alignof(Entry));
    memset((void *)entries, 0, bufsize);
#else
    entries = (Entry *)mmap(nullptr, bufsize, PROT_READ | PROT_WRITE,
                            MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
#endif
  }

  std::pair<T *, bool> insert(std::string_view key, u64 hash, const T &val) {
    assert(has_single_bit(nbuckets));

    u64 begin = hash & (nbuckets - 1);
    u64 mask = nbuckets / NUM_SHARDS - 1;

    for (i64 i = 0; i < MAX_RETRY; i++) {
      u64 idx = (begin & ~mask) | ((begin + i) & mask);
      Entry &ent = entries[idx];

      // It seems avoiding compare-and-swap is faster overall at least
      // on my Zen4 machine, so do it.
      if (const char *ptr = ent.key.load(std::memory_order_acquire);
          ptr != nullptr && ptr != (char *)-1) {
        if (key == std::string_view(ptr, ent.keylen))
          return {&ent.value, false};
        continue;
      }

      // Otherwise, use CAS to atomically claim the ownership of the slot.
      const char *ptr = nullptr;
      bool claimed = ent.key.compare_exchange_strong(ptr, (char *)-1,
                                                     std::memory_order_acquire);

      // If we successfully claimed the ownership of the slot,
      // copy values to it.
      if (claimed) {
        new (&ent.value) T(val);
        ent.keylen = key.size();
        ent.key.store(key.data(), std::memory_order_release);
        return {&ent.value, true};
      }

      // If someone is copying values to the slot, do busy wait.
      while (ptr == (char *)-1) {
        pause();
        ptr = ent.key.load(std::memory_order_acquire);
      }

      // If the same key is already present, this is the slot we are
      // looking for.
      if (key == std::string_view(ptr, ent.keylen))
        return {&ent.value, false};
    }

    std::cerr << "ConcurrentMap is full\n";
    abort();
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
};

//
// random.cc
//

void get_random_bytes(u8 *buf, i64 size);

//
// hyperloglog.cc
//

class HyperLogLog {
public:
  void insert(u64 hash) {
    update_maximum(buckets[hash & (NBUCKETS - 1)], std::countl_zero(hash) + 1);
  }

  i64 get_cardinality() const;

  void merge(const HyperLogLog &other) {
    for (i64 i = 0; i < NBUCKETS; i++)
      update_maximum(buckets[i], other.buckets[i]);
  }

private:
  static constexpr i64 NBUCKETS = 2048;
  static constexpr double ALPHA = 0.79402;

  Atomic<u8> buckets[NBUCKETS];
};

//
// glob.cc
//

class Glob {
  typedef enum { STRING, STAR, QUESTION, BRACKET } Kind;

  struct Element {
    Element(Kind k) : kind(k) {}
    Kind kind;
    std::string str;
    std::bitset<256> bitset;
  };

public:
  static std::optional<Glob> compile(std::string_view pat);
  bool match(std::string_view str);

private:
  Glob(std::vector<Element> &&vec) : elements(vec) {}
  static bool do_match(std::string_view str, std::span<Element> elements);

  std::vector<Element> elements;
};

//
// multi-glob.cc
//

class MultiGlob {
public:
  bool add(std::string_view pat, i64 val);
  bool empty() const { return strings.empty(); }
  std::optional<i64> find(std::string_view str);

private:
  struct TrieNode {
    i64 value = -1;
    TrieNode *suffix_link = nullptr;
    std::unique_ptr<TrieNode> children[256];
  };

  void compile();
  void fix_suffix_links(TrieNode &node);
  void fix_values();
  i64 find_aho_corasick(std::string_view str);

  std::vector<std::string> strings;
  std::unique_ptr<TrieNode> root;
  std::vector<std::pair<Glob, i64>> globs;
  std::once_flag once;
  bool is_compiled = false;
  bool prefix_match = false;
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
