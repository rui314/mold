#pragma once

#include "mold.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold {

template <typename C>
class OutputFile {
public:
  virtual void close(C &ctx) = 0;
  virtual ~OutputFile() {}

  u8 *buf = nullptr;
  std::string path;
  i64 filesize;
  bool is_mmapped;

protected:
  OutputFile(std::string path, i64 filesize, bool is_mmapped)
    : path(path), filesize(filesize), is_mmapped(is_mmapped) {}

  friend std::unique_ptr<OutputFile<C>>
  open_output_file(C &ctx, std::string path, i64 filesize, i64 perm);
};

inline u32 get_umask() {
  u32 orig_umask = umask(0);
  umask(orig_umask);
  return orig_umask;
}

template <typename C>
class MemoryMappedOutputFile : public OutputFile<C> {
public:
  MemoryMappedOutputFile(C &ctx, std::string path, i64 filesize, i64 perm)
    : OutputFile<C>(path, filesize, true) {
    std::string dir(path_dirname(path));
    output_tmpfile = (char *)save_string(ctx, dir + "/.mold-XXXXXX").data();
    i64 fd = mkstemp(output_tmpfile);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << output_tmpfile <<  ": " << errno_string();

    if (rename(path.c_str(), output_tmpfile) == 0) {
      ::close(fd);
      fd = ::open(output_tmpfile, O_RDWR | O_CREAT, perm);
      if (fd == -1) {
        if (errno != ETXTBSY)
          Fatal(ctx) << "cannot open " << path << ": " << errno_string();
        unlink(output_tmpfile);
        fd = ::open(output_tmpfile, O_RDWR | O_CREAT, perm);
        if (fd == -1)
          Fatal(ctx) << "cannot open " << path << ": " << errno_string();
      }
    }

    if (ftruncate(fd, filesize))
      Fatal(ctx) << "ftruncate failed";

    if (fchmod(fd, (perm & ~get_umask())) == -1)
      Fatal(ctx) << "fchmod failed";

    this->buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED, fd, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << path << ": mmap failed: " << errno_string();
    ::close(fd);
  }

  void close(C &ctx) override {
    Timer t(ctx, "close_file");

    if (!ctx.buildid)
      munmap(this->buf, this->filesize);

    if (rename(output_tmpfile, this->path.c_str()) == -1)
      Fatal(ctx) << this->path << ": rename failed: " << errno_string();
    output_tmpfile = nullptr;
  }
};

template <typename C>
class MallocOutputFile : public OutputFile<C> {
public:
  MallocOutputFile(C &ctx, std::string path, i64 filesize, i64 perm)
    : OutputFile<C>(path, filesize, false), perm(perm) {
    this->buf = (u8 *)mmap(NULL, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << "mmap failed: " << errno_string();
  }

  void close(C &ctx) override {
    Timer t(ctx, "close_file");

    if (this->path == "-") {
      fwrite(this->buf, this->filesize, 1, stdout);
      fclose(stdout);
      return;
    }

    i64 fd = ::open(this->path.c_str(), O_RDWR | O_CREAT, perm);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << this->path << ": " << errno_string();

    FILE *fp = fdopen(fd, "w");
    fwrite(this->buf, this->filesize, 1, fp);
    fclose(fp);
  }

private:
  i64 perm;
};

template <typename C>
std::unique_ptr<OutputFile<C>>
open_output_file(C &ctx, std::string path, i64 filesize, i64 perm) {
  Timer t(ctx, "open_file");

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  bool is_special = false;
  if (path == "-") {
    is_special = true;
  } else {
    struct stat st;
    if (stat(path.c_str(), &st) == 0 && (st.st_mode & S_IFMT) != S_IFREG)
      is_special = true;
  }

  std::unique_ptr<OutputFile<C>> file;
  if (is_special)
    file = std::make_unique<MallocOutputFile<C>>(ctx, path, filesize, perm);
  else
    file = std::make_unique<MemoryMappedOutputFile<C>>(ctx, path, filesize, perm);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return file;
}

} // namespace mold
