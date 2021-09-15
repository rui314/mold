#pragma once

#include <atomic>
#include <cassert>
#include <cstring>
#include <iostream>
#include <mutex>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <unistd.h>
#include <vector>

namespace mold {

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

inline char *output_tmpfile;
inline char *socket_tmpfile;
inline thread_local bool opt_demangle;

std::string errno_string();
std::string get_version_string();
void cleanup();
void install_signal_handler();

//
// Error output
//

template <typename C>
class SyncOut {
public:
  SyncOut(C &ctx, std::ostream &out = std::cout) : out(out) {
    opt_demangle = ctx.arg.demangle;
  }

  ~SyncOut() {
    std::lock_guard lock(mu);
    out << ss.str() << "\n";
  }

  template <class T> SyncOut &operator<<(T &&val) {
    ss << std::forward<T>(val);
    return *this;
  }

  static inline std::mutex mu;

private:
  std::ostream &out;
  std::stringstream ss;
};

template <typename C>
class Fatal {
public:
  Fatal(C &ctx) : out(ctx, std::cerr) {
    out << "mold: ";
  }

  [[noreturn]] ~Fatal() {
    out.~SyncOut();
    cleanup();
    _exit(1);
  }

  template <class T> Fatal &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

private:
  SyncOut<C> out;
};

template <typename C>
class Error {
public:
  Error(C &ctx) : out(ctx, std::cerr) {
    out << "mold: ";
    ctx.has_error = true;
  }

  template <class T> Error &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

private:
  SyncOut<C> out;
};

template <typename C>
class Warn {
public:
  Warn(C &ctx) : out(ctx, std::cerr) {
    out << "mold: ";
    if (ctx.arg.fatal_warnings)
      ctx.has_error = true;
  }

  template <class T> Warn &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

private:
  SyncOut<C> out;
};

#define unreachable(ctx)                                               \
  do {                                                                 \
    Fatal(ctx) << "internal error at " << __FILE__ << ":" << __LINE__; \
  } while (0)

//
// Utility functions
//

inline u64 align_to(u64 val, u64 align) {
  if (align == 0)
    return val;
  assert(__builtin_popcount(align) == 1);
  return (val + align - 1) & ~(align - 1);
}

inline u64 next_power_of_two(u64 val) {
  assert(val >> 63 == 0);
  if (val == 0 || val == 1)
    return 1;
  return (u64)1 << (64 - __builtin_clzl(val - 1));
}

template <typename T, typename U>
inline void append(std::vector<T> &vec1, std::vector<U> vec2) {
  vec1.insert(vec1.end(), vec2.begin(), vec2.end());
}

template <typename T>
inline std::vector<T> flatten(std::vector<std::vector<T>> &vec) {
  std::vector<T> ret;
  for (std::vector<T> &v : vec)
    append(ret, v);
  return ret;
}

template <typename T, typename U>
inline void erase(std::vector<T> &vec, U pred) {
  vec.erase(std::remove_if(vec.begin(), vec.end(), pred), vec.end());
}

template <typename T, typename U>
inline void sort(T &vec, U less) {
  std::stable_sort(vec.begin(), vec.end(), less);
}

inline u64 read64be(u8 *buf) {
  return ((u64)buf[0] << 56) | ((u64)buf[1] << 48) |
         ((u64)buf[2] << 40) | ((u64)buf[3] << 32) |
         ((u64)buf[4] << 24) | ((u64)buf[5] << 16) |
         ((u64)buf[6] << 8)  | (u64)buf[7];
}

inline void write64be(u8 *buf, u64 val) {
  buf[0] = val >> 56;
  buf[1] = val >> 48;
  buf[2] = val >> 40;
  buf[3] = val >> 32;
  buf[4] = val >> 24;
  buf[5] = val >> 16;
  buf[6] = val >> 8;
  buf[7] = val;
}

inline void write32be(u8 *buf, u32 val) {
  buf[0] = val >> 24;
  buf[1] = val >> 16;
  buf[2] = val >> 8;
  buf[3] = val;
}

//
// Concurrent Map
//

template <typename T>
class ConcurrentMap {
public:
  ConcurrentMap() {}

  ConcurrentMap(i64 nbuckets) {
    resize(nbuckets);
  }

  ~ConcurrentMap() {
    if (keys) {
      free((void *)keys);
      free((void *)sizes);
      free((void *)values);
    }
  }

  void resize(i64 nbuckets) {
    this->~ConcurrentMap();

    nbuckets = std::max<i64>(MIN_NBUCKETS, next_power_of_two(nbuckets));

    this->nbuckets = nbuckets;
    keys = (std::atomic<const char *> *)calloc(nbuckets, sizeof(keys[0]));
    sizes = (u32 *)calloc(nbuckets, sizeof(sizes[0]));
    values = (T *)calloc(nbuckets, sizeof(values[0]));
  }

  std::pair<T *, bool> insert(std::string_view key, u64 hash, const T &val) {
    if (!keys)
      return {nullptr, false};

    assert(__builtin_popcount(nbuckets) == 1);
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

    assert(false && "ConcurrentMap is full");
    return {nullptr, false};
  }

  bool has_key(i64 idx) {
    return keys[idx];
  }

  static constexpr i64 MIN_NBUCKETS = 2048;
  static constexpr i64 NUM_SHARDS = 16;
  static constexpr i64 MAX_RETRY = 128;

  i64 nbuckets = 0;
  std::atomic<const char *> *keys = nullptr;
  u32 *sizes = nullptr;
  T *values = nullptr;

private:
  static constexpr const char *locked = "marker";
};

//
// hyperloglog.cc
//

class HyperLogLog {
public:
  HyperLogLog() : buckets(NBUCKETS) {}

  void insert(u32 hash) {
    merge_one(hash & (NBUCKETS - 1), __builtin_clz(hash) + 1);
  }

  void merge_one(i64 idx, u8 newval) {
    u8 cur = buckets[idx];
    while (cur < newval)
      if (buckets[idx].compare_exchange_strong(cur, newval))
        break;
  }

  i64 get_cardinality() const;
  void merge(const HyperLogLog &other);

private:
  static constexpr i64 NBUCKETS = 2048;
  static constexpr double ALPHA = 0.79402;

  std::vector<std::atomic_uint8_t> buckets;
};

//
// filepath.cc
//

// These are various utility functions to deal with file pathnames.
std::string get_current_dir();
std::string_view path_dirname(std::string_view path);
std::string_view path_filename(std::string_view path);
std::string_view path_basename(std::string_view path);
std::string path_to_absolute(std::string_view path);
std::string path_clean(std::string_view path);

//
// compress.cc
//

class ZlibCompressor {
public:
  ZlibCompressor(std::string_view input);
  void write_to(u8 *buf);
  i64 size() const;

private:
  std::vector<std::vector<u8>> shards;
  u64 checksum = 0;
};

class GzipCompressor {
public:
  GzipCompressor(std::string_view input);
  void write_to(u8 *buf);
  i64 size() const;

private:
  std::vector<std::vector<u8>> shards;
  u32 checksum = 0;
  u32 uncompressed_size = 0;
};

//
// tar.cc
//

// TarFile is a class to create a tar file.
//
// If you pass `--reproduce=repro.tar` to mold, mold collects all
// input files and put them into `repro.tar`, so that it is easy to
// run the same command with the same command line arguments.
class TarFile {
public:
  static constexpr i64 BLOCK_SIZE = 512;


  TarFile(std::string basedir) : basedir(basedir) {}

  void append(std::string path, std::string_view data);
  void write_to(u8 *buf);
  i64 size() const { return size_; }

private:
  std::string basedir;
  std::vector<std::pair<std::string, std::string_view>> contents;
  i64 size_ = BLOCK_SIZE * 2;
};

} // namespace mold
