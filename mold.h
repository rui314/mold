#pragma once

#include <atomic>
#include <cassert>
#include <cstring>
#include <fcntl.h>
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

#define unreachable() assert(0 && "unreachable")

//
// Utility functions
//

inline u64 align_to(u64 val, u64 align) {
  if (align == 0)
    return val;
  assert(__builtin_popcount(align) == 1);
  return (val + align - 1) & ~(align - 1);
}

inline u64 align_down(u64 val, u64 align) {
  assert(__builtin_popcount(align) == 1);
  return val & ~(align - 1);
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
  u8 byte;
  do {
    byte = *buf++;
    val = (val << 7) | (byte & 0x7f);
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
// Bit vector
//

class BitVector {
  class BitRef {
  public:
    BitRef(u8 &byte, u8 bitpos) : byte(byte), bitpos(bitpos) {}

    BitRef &operator=(bool val) {
      if (val)
        byte |= (1 << bitpos);
      else
        byte &= ~(1 << bitpos);
      return *this;
    }

    BitRef &operator=(const BitRef &other) {
      *this = (bool)other;
      return *this;
    }

    operator bool() const {
      return byte & (1 << bitpos);
    }

  private:
    u8 &byte;
    u8 bitpos;
  };

public:
  void resize(i64 size) {
    vec.reset(new u8[(size + 7) / 8]);
    memset(vec.get(), 0, (size + 7) / 8);
  }

  BitRef operator[](i64 i) {
    return BitRef(vec[i / 8], i % 8);
  }

private:
  std::unique_ptr<u8[]> vec;
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
    std::lock_guard lock(mu);
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
// If you pass `--reproduce=repro.tar` to mold, mold collects all
// input files and put them into `repro.tar`, so that it is easy to
// run the same command with the same command line arguments.
class TarFile {
public:
  TarFile(std::string basedir) : basedir(basedir) {}
  void append(std::string path, std::string_view data);
  void write_to(u8 *buf);
  i64 size() const { return size_; }

private:
  static constexpr i64 BLOCK_SIZE = 512;

  std::string encode_path(std::string path);

  std::string basedir;
  std::vector<std::pair<std::string, std::string_view>> contents;
  i64 size_ = BLOCK_SIZE * 2;
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

  std::string name;
  u8 *data = nullptr;
  i64 size = 0;
  i64 mtime = 0;
  bool given_fullpath = true;
  MappedFile *parent = nullptr;
};

template <typename C>
MappedFile<C> *MappedFile<C>::open(C &ctx, std::string path) {
  MappedFile *mf = new MappedFile;
  mf->name = path;

  ctx.owning_mbs.push_back(std::unique_ptr<MappedFile>(mf));

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  i64 fd = ::open(path.c_str(), O_RDONLY);
  if (fd == -1)
    return nullptr;

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
  Fatal(ctx) << "cannot open " << path;
}

template <typename C>
MappedFile<C> *
MappedFile<C>::slice(C &ctx, std::string name, u64 start, u64 size) {
  MappedFile *mf = new MappedFile<C>;
  mf->name = name;
  mf->data = data + start;
  mf->size = size;
  mf->parent = this;

  ctx.owning_mbs.push_back(std::unique_ptr<MappedFile>(mf));
  return mf;
}

template <typename C>
MappedFile<C>::~MappedFile() {
  if (size && !parent)
    munmap(data, size);
}

//
// Archive file
//

struct ArHdr {
  char ar_name[16];
  char ar_date[12];
  char ar_uid[6];
  char ar_gid[6];
  char ar_mode[8];
  char ar_size[10];
  char ar_fmag[2];
};

enum class FileType { UNKNOWN, OBJ, DSO, AR, THIN_AR, TEXT, LLVM_BITCODE };

template <typename C>
bool is_text_file(MappedFile<C> *mf) {
  u8 *data = mf->data;
  return mf->size >= 4 && isprint(data[0]) && isprint(data[1]) &&
         isprint(data[2]) && isprint(data[3]);
}

template <typename C>
FileType get_file_type(MappedFile<C> *mf) {
  std::string_view data = mf->get_contents();

  if (data.starts_with("\177ELF")) {
    switch (*(u16 *)(data.data() + 16)) {
    case 1: // ET_REL
      return FileType::OBJ;
    case 3: // ET_DYN
      return FileType::DSO;
    }
    return FileType::UNKNOWN;
  }

  if (data.starts_with("!<arch>\n"))
    return FileType::AR;
  if (data.starts_with("!<thin>\n"))
    return FileType::THIN_AR;
  if (is_text_file(mf))
    return FileType::TEXT;
  if (data.starts_with("\xDE\xC0\x17\x0B"))
    return FileType::LLVM_BITCODE;
  if (data.starts_with("BC\xC0\xDE"))
    return FileType::LLVM_BITCODE;
  return FileType::UNKNOWN;
}


template <typename C>
std::vector<MappedFile<C> *>
read_thin_archive_members(C &ctx, MappedFile<C> *mf) {
  u8 *begin = mf->data;
  u8 *data = begin + 8;
  std::vector<MappedFile<C> *> vec;
  std::string_view strtab;

  while (data < begin + mf->size) {
    // Each header is aligned to a 2 byte boundary.
    if ((begin - data) % 2)
      data++;

    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);

    // Read a string table.
    if (!memcmp(hdr.ar_name, "// ", 3)) {
      strtab = {(char *)body, size};
      data = body + size;
      continue;
    }

    // Skip a symbol table.
    if (!memcmp(hdr.ar_name, "/ ", 2)) {
      data = body + size;
      continue;
    }

    if (hdr.ar_name[0] != '/')
      Fatal(ctx) << mf->name << ": filename is not stored as a long filename";

    const char *start = strtab.data() + atoi(hdr.ar_name + 1);
    std::string name(start, (const char *)strstr(start, "/\n"));
    std::string path = name.starts_with('/') ?
      name : std::string(path_dirname(mf->name)) + "/" + name;
    vec.push_back(MappedFile<C>::must_open(ctx, path));
    data = body;
  }
  return vec;
}

template <typename C>
std::vector<MappedFile<C> *>
read_fat_archive_members(C &ctx, MappedFile<C> *mf) {
  u8 *begin = mf->data;
  u8 *data = begin + 8;
  std::vector<MappedFile<C> *> vec;
  std::string_view strtab;

  while (begin + mf->size - data >= 2) {
    if ((begin - data) % 2)
      data++;

    ArHdr &hdr = *(ArHdr *)data;
    u8 *body = data + sizeof(hdr);
    u64 size = atol(hdr.ar_size);
    data = body + size;

    if (!memcmp(hdr.ar_name, "// ", 3)) {
      strtab = {(char *)body, size};
      continue;
    }

    if (!memcmp(hdr.ar_name, "/ ", 2) ||
        !memcmp(hdr.ar_name, "__.SYMDEF/", 10))
      continue;

    std::string name;

    if (hdr.ar_name[0] == '/') {
      const char *start = strtab.data() + atoi(hdr.ar_name + 1);
      name = {start, (const char *)strstr(start, "/\n")};
    } else {
      name = {hdr.ar_name, strchr(hdr.ar_name, '/')};
    }

    vec.push_back(mf->slice(ctx, name, body - begin, size));
  }
  return vec;
}

template <typename C>
std::vector<MappedFile<C> *>
read_archive_members(C &ctx, MappedFile<C> *mf) {
  switch (get_file_type(mf)) {
  case FileType::AR:
    return read_fat_archive_members(ctx, mf);
  case FileType::THIN_AR:
    return read_thin_archive_members(ctx, mf);
  default:
    unreachable();
  }
}

} // namespace mold
