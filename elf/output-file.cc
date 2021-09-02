#include "mold.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::elf {

static u32 get_umask() {
  u32 orig_umask = umask(0);
  umask(orig_umask);
  return orig_umask;
}

template <typename E>
class MemoryMappedOutputFile : public OutputFile<E> {
public:
  MemoryMappedOutputFile(Context<E> &ctx, std::string path, i64 filesize,
                         i64 perm)
    : OutputFile<E>(path, filesize, true) {
    std::string dir(path_dirname(path));
    this->tmpfile = (char *)save_string(ctx, dir + "/.mold-XXXXXX").data();
    i64 fd = mkstemp(this->tmpfile);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << this->tmpfile <<  ": " << strerror(errno);

    if (rename(path.c_str(), this->tmpfile) == 0) {
      ::close(fd);
      fd = ::open(this->tmpfile, O_RDWR | O_CREAT, perm);
      if (fd == -1) {
        if (errno != ETXTBSY)
          Fatal(ctx) << "cannot open " << path << ": " << strerror(errno);
        unlink(this->tmpfile);
        fd = ::open(this->tmpfile, O_RDWR | O_CREAT, perm);
        if (fd == -1)
          Fatal(ctx) << "cannot open " << path << ": " << strerror(errno);
      }
    }

    if (ftruncate(fd, filesize))
      Fatal(ctx) << "ftruncate failed";

    if (fchmod(fd, (perm & ~get_umask())) == -1)
      Fatal(ctx) << "fchmod failed";

    this->buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED, fd, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << path << ": mmap failed: " << strerror(errno);
    ::close(fd);
  }

  void close(Context<E> &ctx) override {
    Timer t(ctx, "close_file");

    if (!ctx.buildid)
      munmap(this->buf, this->filesize);

    if (rename(this->tmpfile, this->path.c_str()) == -1)
      Fatal(ctx) << this->path << ": rename failed: " << strerror(errno);
    this->tmpfile = nullptr;
  }
};

template <typename E>
class MallocOutputFile : public OutputFile<E> {
public:
  MallocOutputFile(Context<E> &ctx, std::string path, i64 filesize, i64 perm)
    : OutputFile<E>(path, filesize, false), perm(perm) {
    this->buf = (u8 *)mmap(NULL, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << "mmap failed: " << strerror(errno);
  }

  void close(Context<E> &ctx) override {
    Timer t(ctx, "close_file");

    if (this->path == "-") {
      fwrite(this->buf, this->filesize, 1, stdout);
      fclose(stdout);
      return;
    }

    i64 fd = ::open(this->path.c_str(), O_RDWR | O_CREAT, perm);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << this->path << ": " << strerror(errno);

    FILE *fp = fdopen(fd, "w");
    fwrite(this->buf, this->filesize, 1, fp);
    fclose(fp);
  }

private:
  i64 perm;
};

template <typename E>
std::unique_ptr<OutputFile<E>>
OutputFile<E>::open(Context<E> &ctx, std::string path, i64 filesize, i64 perm) {
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

  std::unique_ptr<OutputFile<E>> file;
  if (is_special)
    file = std::make_unique<MallocOutputFile<E>>(ctx, path, filesize, perm);
  else
    file = std::make_unique<MemoryMappedOutputFile<E>>(ctx, path, filesize, perm);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return file;
}

#define INSTANTIATE(E)                                                  \
  template                                                              \
  std::unique_ptr<OutputFile<E>>                                        \
  OutputFile<E>::open(Context<E> &, std::string, i64, i64);

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(AARCH64);

} // namespace mold::elf
