#pragma once

#include "integers.h"

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

using namespace std::literals::string_literals;
using namespace std::literals::string_view_literals;

template <typename Context> class OutputFile;

inline char *output_tmpfile;
inline thread_local bool opt_demangle;

inline u8 *output_buffer_start = nullptr;
inline u8 *output_buffer_end = nullptr;

extern std::string mold_git_hash;

std::string errno_string();
std::string get_self_path();
void cleanup();
void install_signal_handler();

static u64 combine_hash(u64 a, u64 b) {
  return a ^ (b + 0x9e3779b9 + (a << 6) + (a >> 2));
}

//
// Error output
//

template <typename Context>
class SyncOut {
public:
  SyncOut(Context &ctx, std::ostream *out = &std::cout) : out(out) {
    opt_demangle = ctx.arg.demangle;
  }

  ~SyncOut() {
    if (out) {
      std::scoped_lock lock(mu);
      *out << ss.str() << "\n";
    }
  }

  template <class T> SyncOut &operator<<(T &&val) {
    if (out)
      ss << std::forward<T>(val);
    return *this;
  }

  static inline std::mutex mu;

private:
  std::ostream *out;
  std::stringstream ss;
};

template <typename Context>
static std::string add_color(Context &ctx, std::string msg) {
  if (ctx.arg.color_diagnostics)
    return "mold: \033[0;1;31m" + msg + ":\033[0m ";
  return "mold: " + msg + ": ";
}

template <typename Context>
class Fatal {
public:
  Fatal(Context &ctx) : out(ctx, &std::cerr) {
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
  SyncOut<Context> out;
};

template <typename Context>
class Error {
public:
  Error(Context &ctx) : out(ctx, &std::cerr) {
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
  SyncOut<Context> out;
};

template <typename Context>
class Warn {
public:
  Warn(Context &ctx)
    : out(ctx, ctx.arg.suppress_warnings ? nullptr : &std::cerr) {
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
  SyncOut<Context> out;
};

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
// Bit vector
//

class BitVector {
public:
  BitVector() = default;
  BitVector(u32 size) : vec((size + 7) / 8) {}

  void resize(u32 size) { vec.resize((size + 7) / 8); }
  bool get(u32 idx) const { return vec[idx / 8] & (1 << (idx % 8)); }
  void set(u32 idx) { vec[idx / 8] |= 1 << (idx % 8); }

private:
  std::vector<u8> vec;
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

template <typename T, typename U>
inline void append(std::vector<T> &vec1, std::vector<U> vec2) {
  vec1.insert(vec1.end(), vec2.begin(), vec2.end());
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

inline void sort(auto &vec) {
  std::stable_sort(vec.begin(), vec.end());
}

inline void sort(auto &vec, auto less) {
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

template <typename Context>
std::string_view save_string(Context &ctx, const std::string &str) {
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
//
// We've implemented this ourselves because the performance of
// conrurent hash map is critical for our linker.
template <typename T>
class ConcurrentMap {
public:
  ConcurrentMap() {}

  ConcurrentMap(i64 nbuckets) {
    resize(nbuckets);
  }

  // In order to avoid unnecessary cache-line false sharing, we want
  // to make this object to be aligned to a reasonably large
  // power-of-two address.
  struct alignas(32) Entry {
    Atomic<const char *> key;
    T value;
    u32 keylen;
  };

  void resize(i64 nbuckets) {
    this->nbuckets = std::max<i64>(MIN_NBUCKETS, bit_ceil(nbuckets));

    // Even though std::aligned_alloc is defined in C++17, MSVC doesn't
    // seem to provide that function. C11's aligned_alloc may not always be
    // available. Therefore, we'll align the buffer ourselves.
    entries_buf.clear();
    entries_buf.resize(sizeof(Entry) * this->nbuckets + alignof(Entry) - 1);
    entries = (Entry *)align_to((uintptr_t)&entries_buf[0], alignof(Entry));
  }

  std::pair<T *, bool> insert(std::string_view key, u64 hash, const T &val) {
    assert(has_single_bit(nbuckets));

    i64 idx = hash & (nbuckets - 1);
    i64 retry = 0;

    while (retry < MAX_RETRY) {
      Entry &ent = entries[idx];
      const char *ptr = nullptr;
      bool claimed = ent.key.compare_exchange_weak(ptr, (char *)-1,
                                                   std::memory_order_acquire);

      // If we successfully claimed the ownership of an unused slot,
      // copy values to it.
      if (claimed) {
        new (&ent.value) T(val);
        ent.keylen = key.size();
        ent.key.store(key.data(), std::memory_order_release);
        return {&ent.value, true};
      }

      // Loop on a spurious failure.
      if (ptr == nullptr)
        continue;

      // If someone is copying values to the slot, do busy wait.
      while (ptr == (char *)-1) {
        pause();
        ptr = ent.key.load(std::memory_order_acquire);
      }

      // If the same key is already present, this is the slot we are
      // looking for.
      if (key == std::string_view(ptr, ent.keylen))
        return {&ent.value, false};

      // Otherwise, move on to the next slot.
      u64 mask = nbuckets / NUM_SHARDS - 1;
      idx = (idx & ~mask) | ((idx + 1) & mask);
      retry++;
    }

    assert(false && "ConcurrentMap is full");
    return {nullptr, false};
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
    while (entries[end - 1].key)
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

  static constexpr i64 MIN_NBUCKETS = 2048;
  static constexpr i64 NUM_SHARDS = 16;
  static constexpr i64 MAX_RETRY = 128;

  std::vector<u8> entries_buf;
  Entry *entries = nullptr;
  i64 nbuckets = 0;

private:
  static void pause() {
#if defined(__x86_64__)
    asm volatile("pause");
#elif defined(__aarch64__)
    asm volatile("yield");
#endif
  }
};

//
// output-file.h
//

template <typename Context>
class OutputFile {
public:
  static std::unique_ptr<OutputFile<Context>>
  open(Context &ctx, std::string path, i64 filesize, i64 perm);

  virtual void close(Context &ctx) = 0;
  virtual ~OutputFile() = default;

  u8 *buf = nullptr;
  std::vector<u8> buf2;
  std::string path;
  i64 fd = -1;
  i64 filesize = 0;
  bool is_mmapped = false;
  bool is_unmapped = false;

protected:
  OutputFile(std::string path, i64 filesize, bool is_mmapped)
    : path(path), filesize(filesize), is_mmapped(is_mmapped) {}
};

template <typename Context>
class MallocOutputFile : public OutputFile<Context> {
public:
  MallocOutputFile(Context &ctx, std::string path, i64 filesize, i64 perm)
    : OutputFile<Context>(path, filesize, false), ptr(new u8[filesize]),
      perm(perm) {
    this->buf = ptr.get();
  }

  void close(Context &ctx) override {
    Timer t(ctx, "close_file");
    FILE *fp;

    if (this->path == "-") {
      fp = stdout;
    } else {
#ifdef _WIN32
      int pmode = (perm & 0200) ? (_S_IREAD | _S_IWRITE) : _S_IREAD;
      i64 fd = _open(this->path.c_str(), _O_RDWR | _O_CREAT | _O_BINARY, pmode);
#else
      i64 fd = ::open(this->path.c_str(), O_RDWR | O_CREAT, perm);
#endif
      if (fd == -1)
        Fatal(ctx) << "cannot open " << this->path << ": " << errno_string();
#ifdef _WIN32
      fp = _fdopen(fd, "wb");
#else
      fp = fdopen(fd, "w");
#endif
    }

    fwrite(this->buf, this->filesize, 1, fp);
    if (!this->buf2.empty())
      fwrite(this->buf2.data(), this->buf2.size(), 1, fp);
    fclose(fp);
  }

private:
  std::unique_ptr<u8[]> ptr;
  i64 perm;
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

  std::vector<std::string> strings;
  std::unique_ptr<TrieNode> root;
  std::vector<std::pair<Glob, i64>> globs;
  std::once_flag once;
  bool is_compiled = false;
};

//
// filepath.cc
//

std::filesystem::path filepath(const auto &path) {
  return {path, std::filesystem::path::format::generic_format};
}

std::string get_realpath(std::string_view path);
std::string path_clean(std::string_view path);
std::filesystem::path to_abs_path(std::filesystem::path path);

//
// demangle.cc
//

std::optional<std::string_view> demangle_cpp(std::string_view name);
std::optional<std::string_view> demangle_rust(std::string_view name);

//
// jbos.cc
//

void acquire_global_lock();
void release_global_lock();

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

//
// Memory-mapped file
//

// MappedFile represents an mmap'ed input file.
// mold uses mmap-IO only.
class MappedFile {
public:
  ~MappedFile() { unmap(); }
  void unmap();

  template <typename Context>
  MappedFile *slice(Context &ctx, std::string name, u64 start, u64 size) {
    MappedFile *mf = new MappedFile;
    mf->name = name;
    mf->data = data + start;
    mf->size = size;
    mf->parent = this;

    ctx.mf_pool.push_back(std::unique_ptr<MappedFile>(mf));
    return mf;
  }

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
  bool given_fullpath = true;
  MappedFile *parent = nullptr;
  MappedFile *thin_parent = nullptr;

#ifdef _WIN32
  HANDLE fd = INVALID_HANDLE_VALUE;
#else
  int fd = -1;
#endif
};

MappedFile *open_file_impl(const std::string &path, std::string &error);

template <typename Context>
MappedFile *open_file(Context &ctx, std::string path) {
  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  std::string error;
  MappedFile *mf = open_file_impl(path, error);
  if (!error.empty())
    Fatal(ctx) << error;

  if (mf)
    ctx.mf_pool.push_back(std::unique_ptr<MappedFile>(mf));
  return mf;
}

template <typename Context>
MappedFile *must_open_file(Context &ctx, std::string path) {
  MappedFile *mf = open_file(ctx, path);
  if (!mf)
    Fatal(ctx) << "cannot open " << path << ": " << errno_string();
  return mf;
}

} // namespace mold
