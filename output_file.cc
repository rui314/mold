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
  MemoryMappedOutputFile(std::string path, i64 filesize)
    : OutputFile(path, filesize) {
    std::string dir = dirname(strdup(config.output.c_str()));
    tmpfile = strdup((dir + "/.mold-XXXXXX").c_str());
    i64 fd = mkstemp(tmpfile);
    if (fd == -1)
      Error() << "cannot open " << tmpfile <<  ": " << strerror(errno);

    if (rename(config.output.c_str(), tmpfile) == 0) {
      ::close(fd);
      fd = ::open(tmpfile, O_RDWR | O_CREAT, 0777);
      if (fd == -1) {
        if (errno != ETXTBSY)
          Error() << "cannot open " << config.output << ": " << strerror(errno);
        unlink(tmpfile);
        fd = ::open(tmpfile, O_RDWR | O_CREAT, 0777);
        if (fd == -1)
          Error() << "cannot open " << config.output << ": " << strerror(errno);
      }
    }

    if (ftruncate(fd, filesize))
      Error() << "ftruncate failed";

    if (fchmod(fd, (0777 & ~get_umask())) == -1)
      Error() << "fchmod failed";

    buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (buf == MAP_FAILED)
      Error() << config.output << ": mmap failed: " << strerror(errno);
    ::close(fd);
  }

  void close() override {
    Timer t("munmap");
    munmap(buf, filesize);
    if (rename(tmpfile, config.output.c_str()) == -1)
      Error() << config.output << ": rename filed: " << strerror(errno);
    tmpfile = nullptr;
  }
};

class MallocOutputFile : public OutputFile {
public:
  MallocOutputFile(std::string path, u64 filesize)
    : OutputFile(path, filesize) {
    buf = (u8 *)mmap(NULL, filesize, PROT_READ | PROT_WRITE,
                     MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (buf == MAP_FAILED)
      Error() << "mmap failed: " << strerror(errno);
  }

  void close() override {
    Timer t("munmap");
    i64 fd = ::open(path.c_str(), O_RDWR | O_CREAT, 0777);
    if (fd == -1)
      Error() << "cannot open " << config.output << ": " << strerror(errno);

    FILE *fp = fdopen(fd, "w");
    fwrite(buf, filesize, 1, fp);
    fclose(fp);
  }
};

OutputFile *OutputFile::open(std::string path, u64 filesize) {
  Timer t("open_file");

  bool is_special = false;
  struct stat st;
  if (stat(path.c_str(), &st) == 0 && (st.st_mode & S_IFMT) != S_IFREG)
    is_special = true;

  OutputFile *file;
  if (is_special)
    file = new MallocOutputFile(path, filesize);
  else
    file = new MemoryMappedOutputFile(path, filesize);

  if (config.filler != -1)
    memset(file->buf, config.filler, filesize);
  return file;
}
