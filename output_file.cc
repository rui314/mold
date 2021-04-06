#include "mold.h"

#include <fcntl.h>
#include <libgen.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

static u32 get_umask() {
  u32 orig_umask = umask(0);
  umask(orig_umask);
  return orig_umask;
}

template <typename E>
class MemoryMappedOutputFile : public OutputFile<E> {
public:
  MemoryMappedOutputFile(Context<E> &ctx, std::string path, i64 filesize)
    : OutputFile<E>(path, filesize) {
    std::string dir = dirname(strdup(ctx.arg.output.c_str()));
    this->tmpfile = strdup((dir + "/.mold-XXXXXX").c_str());
    i64 fd = mkstemp(this->tmpfile);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << this->tmpfile <<  ": " << strerror(errno);

    if (rename(ctx.arg.output.c_str(), this->tmpfile) == 0) {
      ::close(fd);
      fd = ::open(this->tmpfile, O_RDWR | O_CREAT, 0777);
      if (fd == -1) {
        if (errno != ETXTBSY)
          Fatal(ctx) << "cannot open " << ctx.arg.output << ": "
                     << strerror(errno);
        unlink(this->tmpfile);
        fd = ::open(this->tmpfile, O_RDWR | O_CREAT, 0777);
        if (fd == -1)
          Fatal(ctx) << "cannot open " << ctx.arg.output << ": "
                     << strerror(errno);
      }
    }

    if (ftruncate(fd, filesize))
      Fatal(ctx) << "ftruncate failed";

    if (fchmod(fd, (0777 & ~get_umask())) == -1)
      Fatal(ctx) << "fchmod failed";

    this->buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED, fd, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << ctx.arg.output << ": mmap failed: " << strerror(errno);
    ::close(fd);
  }

  void close(Context<E> &ctx) override {
    Timer t("close_file");
    munmap(this->buf, this->filesize);
    if (rename(this->tmpfile, ctx.arg.output.c_str()) == -1)
      Fatal(ctx) << ctx.arg.output << ": rename filed: " << strerror(errno);
    this->tmpfile = nullptr;
  }
};

template <typename E>
class MallocOutputFile : public OutputFile<E> {
public:
  MallocOutputFile(Context<E> &ctx, std::string path, u64 filesize)
    : OutputFile<E>(path, filesize) {
    this->buf = (u8 *)mmap(NULL, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << "mmap failed: " << strerror(errno);
  }

  void close(Context<E> &ctx) override {
    Timer t("close_file");
    i64 fd = ::open(this->path.c_str(), O_RDWR | O_CREAT, 0777);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << ctx.arg.output << ": " << strerror(errno);

    FILE *fp = fdopen(fd, "w");
    fwrite(this->buf, this->filesize, 1, fp);
    fclose(fp);
  }
};

template <typename E>
std::unique_ptr<OutputFile<E>>
OutputFile<E>::open(Context<E> &ctx, std::string path, u64 filesize) {
  Timer t("open_file");

  bool is_special = false;
  struct stat st;
  if (stat(path.c_str(), &st) == 0 && (st.st_mode & S_IFMT) != S_IFREG)
    is_special = true;

  std::unique_ptr<OutputFile<E>> file;
  if (is_special)
    file = std::make_unique<MallocOutputFile<E>>(ctx, path, filesize);
  else
    file = std::make_unique<MemoryMappedOutputFile<E>>(ctx, path, filesize);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return file;
}

template
std::unique_ptr<OutputFile<X86_64>>
OutputFile<X86_64>::open(Context<X86_64> &ctx, std::string path, u64 filesize);

template
std::unique_ptr<OutputFile<I386>>
OutputFile<I386>::open(Context<I386> &ctx, std::string path, u64 filesize);
