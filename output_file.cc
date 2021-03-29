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

class MemoryMappedOutputFile : public OutputFile {
public:
  MemoryMappedOutputFile(Context &ctx, std::string path, i64 filesize)
    : OutputFile(path, filesize) {
    std::string dir = dirname(strdup(ctx.arg.output.c_str()));
    tmpfile = strdup((dir + "/.mold-XXXXXX").c_str());
    i64 fd = mkstemp(tmpfile);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << tmpfile <<  ": " << strerror(errno);

    if (rename(ctx.arg.output.c_str(), tmpfile) == 0) {
      ::close(fd);
      fd = ::open(tmpfile, O_RDWR | O_CREAT, 0777);
      if (fd == -1) {
        if (errno != ETXTBSY)
          Fatal(ctx) << "cannot open " << ctx.arg.output << ": " << strerror(errno);
        unlink(tmpfile);
        fd = ::open(tmpfile, O_RDWR | O_CREAT, 0777);
        if (fd == -1)
          Fatal(ctx) << "cannot open " << ctx.arg.output << ": " << strerror(errno);
      }
    }

    if (ftruncate(fd, filesize))
      Fatal(ctx) << "ftruncate failed";

    if (fchmod(fd, (0777 & ~get_umask())) == -1)
      Fatal(ctx) << "fchmod failed";

    buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (buf == MAP_FAILED)
      Fatal(ctx) << ctx.arg.output << ": mmap failed: " << strerror(errno);
    ::close(fd);
  }

  void close(Context &ctx) override {
    Timer t("close_file");
    munmap(buf, filesize);
    if (rename(tmpfile, ctx.arg.output.c_str()) == -1)
      Fatal(ctx) << ctx.arg.output << ": rename filed: " << strerror(errno);
    tmpfile = nullptr;
  }
};

class MallocOutputFile : public OutputFile {
public:
  MallocOutputFile(Context &ctx, std::string path, u64 filesize)
    : OutputFile(path, filesize) {
    buf = (u8 *)mmap(NULL, filesize, PROT_READ | PROT_WRITE,
                     MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (buf == MAP_FAILED)
      Fatal(ctx) << "mmap failed: " << strerror(errno);
  }

  void close(Context &ctx) override {
    Timer t("close_file");
    i64 fd = ::open(path.c_str(), O_RDWR | O_CREAT, 0777);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << ctx.arg.output << ": " << strerror(errno);

    FILE *fp = fdopen(fd, "w");
    fwrite(buf, filesize, 1, fp);
    fclose(fp);
  }
};

OutputFile *OutputFile::open(Context &ctx, std::string path, u64 filesize) {
  Timer t("open_file");

  bool is_special = false;
  struct stat st;
  if (stat(path.c_str(), &st) == 0 && (st.st_mode & S_IFMT) != S_IFREG)
    is_special = true;

  OutputFile *file;
  if (is_special)
    file = new MallocOutputFile(ctx, path, filesize);
  else
    file = new MemoryMappedOutputFile(ctx, path, filesize);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return file;
}
