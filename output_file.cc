#include "mold.h"

#include <fcntl.h>
#include <libgen.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/task_group.h>

static u32 get_umask() {
  u32 orig_umask = umask(0);
  umask(orig_umask);
  return orig_umask;
}

class MemoryMappedOutputFile : public OutputFile {
public:
  MemoryMappedOutputFile(std::string path) : OutputFile(path) {
    std::string dir = dirname(strdup(config.output.c_str()));
    tmpfile = strdup((dir + "/.mold-XXXXXX").c_str());
    fd = mkstemp(tmpfile);
    if (fd == -1)
      Fatal() << "cannot open " << tmpfile <<  ": " << strerror(errno);
    if (fchmod(fd, (0777 & ~get_umask())) == -1)
      Fatal() << tmpfile << ": fchmod failed";
  }

  void populate(u64 size) {
    populate_size = size;

    if (ftruncate(fd, size))
      Fatal() << "ftruncate failed";

    buf = (u8 *)mmap(nullptr, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (buf == MAP_FAILED)
      Fatal() << config.output << ": mmap failed: " << strerror(errno);

    tg.run([=]() { memset(buf, 0, size); });
  }

  u8 *get_buffer(u64 size) override {
    Timer t("get_buffer");
    tg.wait();

    filesize = size;
    if (ftruncate(fd, size))
      Fatal() << "ftruncate failed";

    if (buf) {
      buf = (u8 *)mremap(buf, populate_size, size, MREMAP_MAYMOVE);
      if (buf == MAP_FAILED)
        Fatal() << config.output << ": mremap failed: " << strerror(errno);
    } else {
      buf = (u8 *)mmap(nullptr, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
      if (buf == MAP_FAILED)
        Fatal() << config.output << ": mmap failed: " << strerror(errno);
    }
    ::close(fd);
    return buf;
  }

  std::function<void()> close() override {
    Timer t("close");
    munmap(buf, filesize);

    char *existing = rename_existing_file();
    if (rename(tmpfile, config.output.c_str()) == -1)
      Fatal() << config.output << ": rename filed: " << strerror(errno);
    tmpfile = nullptr;

    if (existing)
      return [=]() { unlink(existing); };
    return []() {};
  }

private:
  char *rename_existing_file() {
    Timer t("rename_existing_file");
    char *path = strdup(config.output.c_str());
    struct stat st;
    if (stat(path, &st) != 0)
      return nullptr;

    std::string dir = dirname(strdup(path));
    char *tmp = strdup((dir + "/.mold-XXXXXX").c_str());
    int fd = mkstemp(tmp);
    if (fd == -1)
      return nullptr;
    ::close(fd);

    if (::rename(path, tmp) == -1)
      return nullptr;
    return tmp;
  }

  tbb::task_group tg;
};

class MallocOutputFile : public OutputFile {
public:
  MallocOutputFile(std::string path) : OutputFile(path) {}

  u8 *get_buffer(u64 size) override {
    filesize = size;
    buf = (u8 *)mmap(NULL, size, PROT_READ | PROT_WRITE,
                     MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (buf == MAP_FAILED)
      Fatal() << "mmap failed: " << strerror(errno);
    return buf;
  }

  std::function<void()> close() override {
    Timer t("munmap");
    i64 fd = ::open(path.c_str(), O_RDWR | O_CREAT, 0777);
    if (fd == -1)
      Fatal() << "cannot open " << config.output << ": " << strerror(errno);

    FILE *fp = fdopen(fd, "w");
    fwrite(buf, filesize, 1, fp);
    fclose(fp);
    return []() {};
  }
};

OutputFile *OutputFile::open(std::string path) {
  Timer t("open_file");
  i64 size = -1;

  if (struct stat st; stat(path.c_str(), &st) == 0) {
    if ((st.st_mode & S_IFMT) != S_IFREG)
      return new MallocOutputFile(path);
    size = st.st_size;
  }

  auto *file = new MemoryMappedOutputFile(path);
  if (size != -1)
    file->populate(size);
  return file;
}
