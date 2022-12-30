#pragma once

#include "inttypes.h"

#include <array>
#include <atomic>
#include <bit>
#include <bitset>
#include <cassert>
#include <cstdio>
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
#include <vector>

#ifdef _WIN32
# include <io.h>
#else
# include <sys/mman.h>
# include <unistd.h>
#endif

#define XXH_INLINE_ALL 1
#include "third-party/xxhash/xxhash.h"

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

using namespace std::literals::string_literals;
using namespace std::literals::string_view_literals;

template <typename C> class OutputFile;

inline char *output_tmpfile;
inline thread_local bool opt_demangle;

inline u8 *output_buffer_start = nullptr;
inline u8 *output_buffer_end = nullptr;

inline std::string mold_version;
extern std::string mold_version_string;
extern std::string mold_git_hash;

std::string errno_string();
std::string get_self_path();
void cleanup();
void install_signal_handler();
i64 get_default_thread_count();

static u64 combine_hash(u64 a, u64 b) {
  return a ^ (b + 0x9e3779b9 + (a << 6) + (a >> 2));
}

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

inline i64 sign_extend(u64 val, i64 size) {
  return (i64)(val << (63 - size)) >> (63 - size);
};

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

// An optimized "mark" operation for parallel mark-and-sweep algorithms.
// Returns true if `visited` was false and updated to true.
inline bool fast_mark(std::atomic<bool> &visited) {
  // A relaxed load + branch (assuming miss) takes only around 20 cycles,
  // while an atomic RMW can easily take hundreds on x86. We note that it's
  // common that another thread beat us in marking, so doing an optimistic
  // early test tends to improve performance in the ~20% ballpark.
  return !visited.load(std::memory_order_relaxed) &&
         !visited.exchange(true, std::memory_order_relaxed);
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
inline i64 write_vector(void *buf, const std::vector<T> &vec) {
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

inline u64 read_uleb(u8 const*&buf) {
  return read_uleb(const_cast<u8 *&>(buf));
}

inline u64 read_uleb(std::string_view &str) {
  u8 *start = (u8 *)&str[0];
  u8 *ptr = start;
  u64 val = read_uleb(ptr);
  str = str.substr(ptr - start);
  return val;
}

inline i64 uleb_size(u64 val) {
#if __GNUC__
#pragma GCC unroll 8
#endif
  for (int i = 1; i < 9; i++)
    if (val < (1LL << (7 * i)))
      return i;
  return 9;
}

template <typename C>
std::string_view save_string(C &ctx, const std::string &str) {
  u8 *buf = new u8[str.size() + 1];
  memcpy(buf, str.data(), str.size());
  buf[str.size()] = '\0';
  ctx.string_pool.push_back(std::unique_ptr<u8[]>(buf));
  return {(char *)buf, str.size()};
}

inline bool remove_prefix(std::string_view &s, std::string_view prefix) {
  if (s.starts_with(prefix)) {
    s = s.substr(prefix.size());
    return true;
  }
  return false;
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

    nbuckets = std::max<i64>(MIN_NBUCKETS, bit_ceil(nbuckets));

    this->nbuckets = nbuckets;
    keys = (std::atomic<const char *> *)calloc(nbuckets, sizeof(keys[0]));
    key_sizes = (u32 *)calloc(nbuckets, sizeof(key_sizes[0]));
    values = (T *)calloc(nbuckets, sizeof(values[0]));
  }

  std::pair<T *, bool> insert(std::string_view key, u64 hash, const T &val) {
    if (!keys)
      return {nullptr, false};

    assert(has_single_bit(nbuckets));
    i64 idx = hash & (nbuckets - 1);
    i64 retry = 0;

    while (retry < MAX_RETRY) {
      const char *ptr = keys[idx].load(std::memory_order_acquire);
      if (ptr == marker) {
        pause();
        continue;
      }

      if (ptr == nullptr) {
        if (!keys[idx].compare_exchange_weak(ptr, marker,
                                             std::memory_order_acq_rel))
          continue;
        new (values + idx) T(val);
        key_sizes[idx] = key.size();
        keys[idx].store(key.data(), std::memory_order_release);
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
    return keys[idx].load(std::memory_order_relaxed);
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
// output-file.h
//

template <typename C>
class OutputFile {
public:
  static std::unique_ptr<OutputFile<C>>
  open(C &ctx, std::string path, i64 filesize, i64 perm);

  virtual void close(C &ctx) = 0;
  virtual ~OutputFile() = default;

  u8 *buf = nullptr;
  std::string path;
  i64 filesize;
  bool is_mmapped;
  bool is_unmapped = false;

protected:
  OutputFile(std::string path, i64 filesize, bool is_mmapped)
    : path(path), filesize(filesize), is_mmapped(is_mmapped) {}
};

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
  bool add(std::string_view pat, u32 val);
  bool empty() const { return strings.empty(); }
  std::optional<u32> find(std::string_view str);

private:
  struct TrieNode {
    u32 value = -1;
    TrieNode *suffix_link = nullptr;
    std::unique_ptr<TrieNode> children[256];
  };

  void compile();
  void fix_suffix_links(TrieNode &node);
  void fix_values();

  std::vector<std::string> strings;
  std::unique_ptr<TrieNode> root;
  std::vector<std::pair<Glob, u32>> globs;
  std::once_flag once;
  bool is_compiled = false;
};

//
// uuid.cc
//

std::array<u8, 16> get_uuid_v4();

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
std::optional<std::string_view> cpp_demangle(std::string_view name);

//
// compress.cc
//

class Compressor {
public:
  virtual void write_to(u8 *buf) = 0;
  virtual ~Compressor() {}
  i64 compressed_size = 0;
};

class ZlibCompressor : public Compressor {
public:
  ZlibCompressor(u8 *buf, i64 size);
  void write_to(u8 *buf) override;

private:
  std::vector<std::vector<u8>> shards;
  u64 checksum = 0;
};

class ZstdCompressor : public Compressor {
public:
  ZstdCompressor(u8 *buf, i64 size);
  void write_to(u8 *buf) override;

private:
  std::vector<std::vector<u8>> shards;
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

  ~MappedFile() { unmap(); }
  void unmap();

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

    if (thin_parent) {
      // If this is a thin archive member, the filename part is
      // guaranteed to be unique.
      return thin_parent->name + ":" + name;
    }
    return name;
  }

  std::string name;
  u8 *data = nullptr;
  i64 size = 0;
  i64 mtime = 0;
  bool given_fullpath = true;
  MappedFile *parent = nullptr;
  MappedFile *thin_parent = nullptr;
  int fd = -1;
#ifdef _WIN32
  HANDLE file_handle = INVALID_HANDLE_VALUE;
#endif
};

template <typename C>
MappedFile<C> *MappedFile<C>::open(C &ctx, std::string path) {
  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  i64 fd;
#ifdef _WIN32
    fd = ::_open(path.c_str(), O_RDONLY);
#else
    fd = ::open(path.c_str(), O_RDONLY);
#endif

  if (fd == -1)
    return nullptr;

  struct stat st;
  if (fstat(fd, &st) == -1)
    Fatal(ctx) << path << ": fstat failed: " << errno_string();

  MappedFile *mf = new MappedFile;
  ctx.mf_pool.push_back(std::unique_ptr<MappedFile>(mf));

  mf->name = path;
  mf->size = st.st_size;

#ifdef _WIN32
    mf->mtime = st.st_mtime;
#elif defined(__APPLE__)
    mf->mtime = (u64)st.st_mtimespec.tv_sec * 1000000000 + st.st_mtimespec.tv_nsec;
#else
    mf->mtime = (u64)st.st_mtim.tv_sec * 1000000000 + st.st_mtim.tv_nsec;
#endif

  if (st.st_size > 0) {
#ifdef _WIN32
    HANDLE handle = CreateFileMapping((HANDLE)_get_osfhandle(fd),
                                      nullptr, PAGE_READWRITE, 0,
                                      st.st_size, nullptr);
    if (!handle)
      Fatal(ctx) << path << ": CreateFileMapping failed: " << GetLastError();
    mf->file_handle = handle;
    mf->data = (u8 *)MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, st.st_size);
    if (!mf->data)
      Fatal(ctx) << path << ": MapViewOfFile failed: " << GetLastError();
#else
    mf->data = (u8 *)mmap(nullptr, st.st_size, PROT_READ | PROT_WRITE,
                          MAP_PRIVATE, fd, 0);
    if (mf->data == MAP_FAILED)
      Fatal(ctx) << path << ": mmap failed: " << errno_string();
#endif
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
void MappedFile<C>::unmap() {
  if (size == 0 || parent || !data)
    return;

#ifdef _WIN32
  UnmapViewOfFile(data);
  if (file_handle != INVALID_HANDLE_VALUE)
    CloseHandle(file_handle);
#else
  munmap(data, size);
#endif

  data = nullptr;
}

} // namespace mold
