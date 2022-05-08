// This file implements the same functionality as elf/output-file.cc.
// We have two different implemetntations for ELF and Mach-O because
// we implemented a special optimization for ELF or Linux.
//
// On Linux, it is faster to write to an existing file than creating a
// new file and writing to it. So, we overwrite an existing executable
// if exists.
//
// On macOS, it looks like the system does not assume that an executable
// is mutated once it is created. Due to some code signing mechanism or
// something, we always have to create a fresh file.

#include "mold.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

template <typename E>
class MemoryMappedOutputFile : public OutputFile<E> {
public:
  MemoryMappedOutputFile(Context<E> &ctx, std::string path, i64 filesize, i64 perm)
    : OutputFile<E>(path, filesize, true) {
    std::string tmp_path = filepath(path).parent_path() / ".mold-XXXXXX";
    output_tmpfile = (char *)save_string(ctx, tmp_path).data();

    i64 fd = mkstemp(output_tmpfile);
    if (fd == -1)
      Fatal(ctx) << "cannot open " << output_tmpfile <<  ": " << errno_string();

    if (ftruncate(fd, filesize))
      Fatal(ctx) << "ftruncate failed: " << errno_string();
    if (fchmod(fd, perm) == -1)
      Fatal(ctx) << "fchmod failed: " << errno_string();

    this->buf = (u8 *)mmap(nullptr, filesize, PROT_READ | PROT_WRITE,
                           MAP_SHARED, fd, 0);
    if (this->buf == MAP_FAILED)
      Fatal(ctx) << path << ": mmap failed: " << errno_string();
    ::close(fd);
  }

  void close(Context<E> &ctx) override {
    Timer t(ctx, "close_file");

    munmap(this->buf, this->filesize);
    if (rename(output_tmpfile, this->path.c_str()) == -1)
      Fatal(ctx) << this->path << ": rename failed: " << errno_string();
    output_tmpfile = nullptr;
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
      Fatal(ctx) << "mmap failed: " << errno_string();
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
      Fatal(ctx) << "cannot open " << this->path << ": " << errno_string();

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

  std::unique_ptr<OutputFile> file;
  if (is_special)
    file = std::make_unique<MallocOutputFile<E>>(ctx, path, filesize, perm);
  else
    file = std::make_unique<MemoryMappedOutputFile<E>>(ctx, path, filesize, perm);
  return file;
}

#define INSTANTIATE(E)                          \
  template class OutputFile<E>

INSTANTIATE_ALL;

} // namespace mold::macho
