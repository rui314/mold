#include "config.h"
#include "mold.h"

#include <fcntl.h>
#include <filesystem>
#include <sys/file.h>
#include <sys/mman.h>
#include <system_error>

#ifdef __linux__
# include <sys/vfs.h>
# include <linux/magic.h>
#endif

namespace mold {

static u32 get_umask() {
  u32 orig_umask = umask(0);
  umask(orig_umask);
  return orig_umask;
}

template <typename E>
static int
open_or_create_file(Context<E> &ctx, std::string path, std::string tmpfile,
                    int perm) {
  // Reuse an existing file if exists and writable because on Linux,
  // writing to an existing file is much faster than creating a fresh
  // file and writing to it.
  if (ctx.overwrite_output_file && rename(path.c_str(), tmpfile.c_str()) == 0) {
    i64 fd = ::open(tmpfile.c_str(), O_RDWR | O_CREAT, perm);
    if (fd != -1)
      return fd;
    unlink(tmpfile.c_str());
  }

  i64 fd = ::open(tmpfile.c_str(), O_RDWR | O_CREAT, perm);
  if (fd == -1)
    Fatal(ctx) << "cannot open " << tmpfile << ": " << errno_string();
  return fd;
}

template <typename E>
class MemoryMappedOutputFile : public OutputFile<E> {
public:
  MemoryMappedOutputFile(Context<E> &ctx, std::string path, i64 filesize, int perm)
    : OutputFile<E>(path, filesize, true) {
    std::string pid = std::to_string(getpid());
    std::string tmpfile =
      path_dirname(path) / ("." + path_filename(path) + "." + pid);

    this->fd = open_or_create_file(ctx, path, tmpfile, perm);

    if (fchmod(this->fd, perm & ~get_umask()) == -1)
      Fatal(ctx) << "fchmod failed: " << errno_string();

    if (ftruncate(this->fd, filesize) == -1)
      Fatal(ctx) << "ftruncate failed: " << errno_string();

    output_tmpfile = (char *)save_string(ctx, tmpfile).data();

#if HAVE_FALLOCATE
    // Calling fallocate speeds up later linking passes on ext4 by
    // taking disk block allocation out of the page-fault handler.
    // On tmpfs, it instead makes things much slower: the kernel
    // allocates and zeroes every page of the range inside the
    // syscall, on one thread, which takes ~1.8 s for a 5 GiB file.
    if (struct statfs fs;
        fstatfs(this->fd, &fs) || fs.f_type != TMPFS_MAGIC)
      fallocate(this->fd, 0, 0, filesize);
#endif

    // We map the file with twice as much address space as its size, so
    // that extend() can grow the file into the mapping in place.
    // Touching the mapping beyond the end of the file is not allowed,
    // but growing the file with ftruncate makes the tail of the
    // mapping accessible without any further mmap call.
    vasize = filesize * 2;
    this->buf = (u8 *)mmap(nullptr, vasize, PROT_READ | PROT_WRITE,
                           MAP_SHARED, this->fd, 0);

    if (this->buf == MAP_FAILED) {
      // If the address space is too tight, map just the file.
      vasize = filesize;
      this->buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE,
                             MAP_SHARED, this->fd, 0);
      if (this->buf == MAP_FAILED)
        Fatal(ctx) << path << ": mmap failed: " << errno_string();
    }

    mold::output_buffer_start = this->buf;
    mold::output_buffer_end = this->buf + filesize;
  }

  ~MemoryMappedOutputFile() {
    if (fd2 != -1)
      ::close(fd2);
  }

  // Extend the file so that the caller can fill the appended data
  // through the tail of the mapping. This is called at most once per
  // output file.
  u8 *extend(Context<E> &ctx, i64 size) override {
    i64 mapsize = this->filesize;

    if (ftruncate(this->fd, mapsize + size) == -1)
      Fatal(ctx) << "ftruncate failed: " << errno_string();

#if HAVE_FALLOCATE
    if (struct statfs fs;
        fstatfs(this->fd, &fs) || fs.f_type != TMPFS_MAGIC)
      fallocate(this->fd, 0, mapsize, size);
#endif

    if (mapsize + size > vasize) {
      // The appended data does not fit in the mapping. Map the grown
      // file again, moving the buffer.
      munmap(this->buf, vasize);
      vasize = mapsize + size;

      this->buf = (u8 *)mmap(nullptr, vasize, PROT_READ | PROT_WRITE,
                             MAP_SHARED, this->fd, 0);
      if (this->buf == MAP_FAILED)
        Fatal(ctx) << this->path << ": mmap failed: " << errno_string();

      ctx.buf = this->buf;
      mold::output_buffer_start = this->buf;
    }

    this->filesize += size;
    mold::output_buffer_end = this->buf + mapsize + size;

#ifdef MADV_HUGEPAGE
    madvise(this->buf, mapsize + size, MADV_HUGEPAGE);
#endif
    return this->buf + mapsize;
  }

  void close(Context<E> &ctx) override {
    Timer t(ctx, "close_file");

    if (!this->is_unmapped)
      munmap(this->buf, vasize);
    ::close(this->fd);

    // If an output file already exists, open a file and then remove it.
    // This is the fastest way to unlink a file, as it does not make the
    // system to immediately release disk blocks occupied by the file.
    fd2 = ::open(this->path.c_str(), O_RDONLY);
    if (fd2 != -1)
      unlink(this->path.c_str());

    if (rename(output_tmpfile, this->path.c_str()) == -1)
      Fatal(ctx) << this->path << ": rename failed: " << errno_string();
    output_tmpfile = nullptr;
  }

private:
  int fd2 = -1;

  // Size of the file mapping, which may extend past the end of the file.
  i64 vasize = 0;
};

template <typename E>
std::unique_ptr<OutputFile<E>>
OutputFile<E>::open(Context<E> &ctx, std::string path, i64 filesize, int perm) {
  Timer t(ctx, "open_file");

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  std::error_code error;
  bool is_special = path == "-" ||
                    (!std::filesystem::is_regular_file(path, error) && !error);

  OutputFile<E> *file;
  if (is_special)
    file = new MallocOutputFile(ctx, path, filesize, perm);
  else
    file = new MemoryMappedOutputFile(ctx, path, filesize, perm);

#ifdef MADV_HUGEPAGE
  // Enable transparent huge page for an output memory-mapped file.
  // Linking a Chromium debug build is ~20% faster with this madvise call.
  madvise(file->buf, filesize, MADV_HUGEPAGE);
#endif

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return std::unique_ptr<OutputFile>(file);
}

// LockingOutputFile is similar to MemoryMappedOutputFile, but it doesn't
// rename output files and instead acquires file lock using flock().
template <typename E>
LockingOutputFile<E>::LockingOutputFile(Context<E> &ctx, std::string path,
                                        int perm)
  : OutputFile<E>(path, 0, true) {
  this->fd = ::open(path.c_str(), O_RDWR | O_CREAT, perm);
  if (this->fd == -1)
    Fatal(ctx) << "cannot open " << path << ": " << errno_string();
  flock(this->fd, LOCK_EX);

  // We may be overwriting to an existing debug info file. We want to
  // make the file unusable so that gdb won't use it by accident until
  // it's ready.
  u8 buf[256] = {};
  (void)!!write(this->fd, buf, sizeof(buf));
}

template <typename E>
void LockingOutputFile<E>::resize(Context<E> &ctx, i64 filesize) {
  if (ftruncate(this->fd, filesize) == -1)
    Fatal(ctx) << "ftruncate failed: " << errno_string();

  // As in MemoryMappedOutputFile, we map the file with twice as much
  // address space as its size so that extend() can grow the file into
  // the mapping in place.
  vasize = filesize * 2;
  this->buf = (u8 *)mmap(nullptr, vasize, PROT_READ | PROT_WRITE,
                         MAP_SHARED, this->fd, 0);

  if (this->buf == MAP_FAILED) {
    vasize = filesize;
    this->buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED, this->fd, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << this->path << ": mmap failed: " << errno_string();
  }

  this->filesize = filesize;
  mold::output_buffer_start = this->buf;
  mold::output_buffer_end = this->buf + filesize;
}

template <typename E>
u8 *LockingOutputFile<E>::extend(Context<E> &ctx, i64 size) {
  i64 mapsize = this->filesize;

  if (ftruncate(this->fd, mapsize + size) == -1)
    Fatal(ctx) << "ftruncate failed: " << errno_string();

  if (mapsize + size > vasize) {
    // The appended data does not fit in the mapping. Map the grown
    // file again, moving the buffer.
    munmap(this->buf, vasize);
    vasize = mapsize + size;

    this->buf = (u8 *)mmap(nullptr, vasize, PROT_READ | PROT_WRITE,
                           MAP_SHARED, this->fd, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << this->path << ": mmap failed: " << errno_string();

    ctx.buf = this->buf;
    mold::output_buffer_start = this->buf;
  }

  this->filesize += size;
  mold::output_buffer_end = this->buf + mapsize + size;
  return this->buf + mapsize;
}

template <typename E>
void LockingOutputFile<E>::close(Context<E> &ctx) {
  if (!this->is_unmapped)
    munmap(this->buf, vasize);
  ::close(this->fd);
}

using E = MOLD_TARGET;

template class OutputFile<E>;
template class LockingOutputFile<E>;

} // namespace mold
