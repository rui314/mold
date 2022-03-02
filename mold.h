#pragma once

#include "big-endian.h"

#include <atomic>
#include <bit>
#include <cassert>
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <filesystem>
#include <iostream>
#include <mutex>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/concurrent_vector.h>
#include <tbb/enumerable_thread_specific.h>
#include <unistd.h>
#include <vector>

#ifdef NDEBUG
#  define unreachable() __builtin_unreachable()
#else
#  define unreachable() assert(0 && "unreachable")
#endif

namespace mold {

using namespace std::literals::string_literals;
using namespace std::literals::string_view_literals;

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

template <typename C> class OutputFile;

inline char *output_tmpfile;
inline char *socket_tmpfile;
inline thread_local bool opt_demangle;

extern const std::string mold_version;

std::string_view errno_string();
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
    std::scoped_lock lock(mu);
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
static std::string add_color(C &ctx, std::string msg) {
  if (ctx.arg.color_diagnostics)
    return "mold: \033[0;1;31m" + msg + ":\033[0m ";
  return "mold: " + msg + ": ";
}

template <typename C>
class Fatal {
public:
  Fatal(C &ctx) : out(ctx, std::cerr) {
    out << add_color(ctx, "fatal");
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
    if (ctx.arg.noinhibit_exec) {
      out << add_color(ctx, "warning");
    } else {
      out << add_color(ctx, "error");
      ctx.has_error = true;
    }
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
    if (ctx.arg.fatal_warnings) {
      out << add_color(ctx, "error");
      ctx.has_error = true;
    } else {
      out << add_color(ctx, "warning");
    }
  }

  template <class T> Warn &operator<<(T &&val) {
    out << std::forward<T>(val);
    return *this;
  }

private:
  SyncOut<C> out;
};

//
// Utility functions
//

inline u64 align_to(u64 val, u64 align) {
  if (align == 0)
    return val;
  assert(std::popcount(align) == 1);
  return (val + align - 1) & ~(align - 1);
}

inline u64 align_down(u64 val, u64 align) {
  assert(std::popcount(align) == 1);
  return val & ~(align - 1);
}

inline u64 next_power_of_two(u64 val) {
  assert(val >> 63 == 0);
  if (val == 0 || val == 1)
    return 1;
  return (u64)1 << (64 - std::countl_zero(val - 1));
}

template <typename T, typename Compare = std::less<T>>
void update_minimum(std::atomic<T> &atomic, u64 new_val, Compare cmp = {}) {
  T old_val = atomic;
  while (cmp(new_val, old_val) &&
         !atomic.compare_exchange_weak(old_val, new_val));
}

template <typename T, typename Compare = std::less<T>>
void update_maximum(std::atomic<T> &atomic, u64 new_val, Compare cmp = {}) {
  T old_val = atomic;
  while (cmp(old_val, new_val) &&
         !atomic.compare_exchange_weak(old_val, new_val));
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

template <typename T>
inline void sort(T &vec) {
  std::stable_sort(vec.begin(), vec.end());
}

template <typename T, typename U>
inline void sort(T &vec, U less) {
  std::stable_sort(vec.begin(), vec.end(), less);
}

inline i64 write_string(u8 *buf, std::string_view str) {
  memcpy(buf, str.data(), str.size());
  buf[str.size()] = '\0';
  return str.size() + 1;
}

template <typename T>
inline i64 write_vector(u8 *buf, const std::vector<T> &vec) {
  i64 sz = vec.size() * sizeof(T);
  memcpy(buf, vec.data(), sz);
  return sz;
}

inline void encode_uleb(std::vector<u8> &vec, u64 val) {
  do {
    u8 byte = val & 0x7f;
    val >>= 7;
    vec.push_back(val ? (byte | 0x80) : byte);
  } while (val);
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

inline u64 read_uleb(u8 *&buf) {
  u64 val = 0;
  u8 shift = 0;
  u8 byte;
  do {
    byte = *buf++;
    val |= (byte & 0x7f) << shift;
    shift += 7;
  } while (byte & 0x80);
  return val;
}

inline i64 uleb_size(u64 val) {
  i64 i = 0;
  do {
    i++;
    val >>= 7;
  } while (val);
  return i;
}

template <typename C>
std::string_view save_string(C &ctx, const std::string &str) {
  u8 *buf = new u8[str.size() + 1];
  memcpy(buf, str.data(), str.size());
  buf[str.size()] = '\0';
  ctx.string_pool.push_back(std::unique_ptr<u8[]>(buf));
  return {(char *)buf, str.size()};
}

//
// Concurrent Map
//

// This is an implementation of a fast concurrent hash map. Unlike
// ordinary hash tables, this impl just aborts if it becomes full.
// So you need to give a correct estimation of the final size before
// using it. We use this hash map to uniquify pieces of data in
// mergeable sections.
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
      free((void *)key_sizes);
      free((void *)values);
    }
  }

  void resize(i64 nbuckets) {
    this->~ConcurrentMap();

    nbuckets = std::max<i64>(MIN_NBUCKETS, next_power_of_two(nbuckets));

    this->nbuckets = nbuckets;
    keys = (std::atomic<const char *> *)calloc(nbuckets, sizeof(keys[0]));
    key_sizes = (u32 *)calloc(nbuckets, sizeof(key_sizes[0]));
    values = (T *)calloc(nbuckets, sizeof(values[0]));
  }

  std::pair<T *, bool> insert(std::string_view key, u64 hash, const T &val) {
    if (!keys)
      return {nullptr, false};

    assert(std::popcount<u64>(nbuckets) == 1);
    i64 idx = hash & (nbuckets - 1);
    i64 retry = 0;

    while (retry < MAX_RETRY) {
      const char *ptr = keys[idx];
      if (ptr == marker) {
        pause();
        continue;
      }

      if (ptr == nullptr) {
        if (!keys[idx].compare_exchange_weak(ptr, marker))
          continue;
        new (values + idx) T(val);
        key_sizes[idx] = key.size();
        keys[idx] = key.data();
        return {values + idx, true};
      }

      if (key.size() == key_sizes[idx] &&
          memcmp(ptr, key.data(), key_sizes[idx]) == 0)
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
  u32 *key_sizes = nullptr;
  T *values = nullptr;

private:
  static void pause() {
#if defined(__x86_64__)
    asm volatile("pause");
#elif defined(__aarch64__)
    asm volatile("yield");
#endif
  }

  static constexpr const char *marker = "marker";
};

//
// threads.cc
//

void set_thread_count(i64 n);

//
// hyperloglog.cc
//

class HyperLogLog {
public:
  HyperLogLog() : buckets(NBUCKETS) {}

  void insert(u32 hash) {
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

  std::vector<std::atomic_uint8_t> buckets;
};

//
// filepath.cc
//

template <typename T>
std::filesystem::path filepath(const T &path) {
  return {path, std::filesystem::path::format::generic_format};
}

std::string get_realpath(std::string_view path);
std::string path_clean(std::string_view path);
std::filesystem::path to_abs_path(std::filesystem::path path);

//
// demangle.cc
//

std::string_view demangle(std::string_view name);

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
    if (enabled)
      values.local()++;
    return *this;
  }

  Counter &operator+=(int delta) {
    if (enabled)
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

template <typename C>
class Timer {
public:
  Timer(C &ctx, std::string name, Timer *parent = nullptr) {
    record = new TimerRecord(name, parent ? parent->record : nullptr);
    ctx.timer_records.push_back(std::unique_ptr<TimerRecord>(record));
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
  static constexpr i64 BLOCK_SIZE = 512;

  TarWriter(FILE *out, std::string basedir) : out(out), basedir(basedir) {}

  FILE *out = nullptr;
  std::string basedir;
};

//
// Memory-mapped file
//

// MappedFile represents an mmap'ed input file.
// mold uses mmap-IO only.
template <typename C>
class MappedFile {
public:
  static MappedFile *open(C &ctx, std::string path);
  static MappedFile *must_open(C &ctx, std::string path);

  ~MappedFile();

  MappedFile *slice(C &ctx, std::string name, u64 start, u64 size);

  std::string_view get_contents() {
    return std::string_view((char *)data, size);
  }

  i64 get_offset() const {
    return parent ? (data - parent->data + parent->get_offset()) : 0;
  }

  // Returns a string that uniquely identify a file that is possibly
  // in an archive.
  std::string get_identifier() const {
    if (parent) {
      // We use the file offset within an archive as an identifier
      // because archive members may have the same name.
      return parent->name + ":" + std::to_string(get_offset());
    }
    return name;
  }

  std::string name;
  u8 *data = nullptr;
  i64 size = 0;
  i64 mtime = 0;
  bool given_fullpath = true;
  MappedFile *parent = nullptr;
  int fd = -1;
};

template <typename C>
MappedFile<C> *MappedFile<C>::open(C &ctx, std::string path) {
  MappedFile *mf = new MappedFile;
  mf->name = path;

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  i64 fd = ::open(path.c_str(), O_RDONLY);
  if (fd == -1)
    return nullptr;

  ctx.mf_pool.push_back(std::unique_ptr<MappedFile>(mf));

  struct stat st;
  if (fstat(fd, &st) == -1)
    Fatal(ctx) << path << ": fstat failed: " << errno_string();

  mf->size = st.st_size;

#ifdef __APPLE__
  mf->mtime = (u64)st.st_mtimespec.tv_sec * 1000000000 + st.st_mtimespec.tv_nsec;
#else
  mf->mtime = (u64)st.st_mtim.tv_sec * 1000000000 + st.st_mtim.tv_nsec;
#endif

  if (st.st_size > 0) {
    mf->data = (u8 *)mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    if (mf->data == MAP_FAILED)
      Fatal(ctx) << path << ": mmap failed: " << errno_string();
  }

  close(fd);
  return mf;
}

template <typename C>
MappedFile<C> *MappedFile<C>::must_open(C &ctx, std::string path) {
  if (MappedFile *mf = MappedFile::open(ctx, path))
    return mf;
  Fatal(ctx) << "cannot open " << path << ": " << errno_string();
}

template <typename C>
MappedFile<C> *
MappedFile<C>::slice(C &ctx, std::string name, u64 start, u64 size) {
  MappedFile *mf = new MappedFile<C>;
  mf->name = name;
  mf->data = data + start;
  mf->size = size;
  mf->parent = this;

  ctx.mf_pool.push_back(std::unique_ptr<MappedFile>(mf));
  return mf;
}

template <typename C>
MappedFile<C>::~MappedFile() {
  if (size && !parent)
    munmap(data, size);
}

} // namespace mold
