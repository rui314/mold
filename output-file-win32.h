#include "mold.h"

#include <fcntl.h>
#include <filesystem>
#include <windows.h>

namespace mold {

template <typename C>
class MallocOutputFile : public OutputFile<C> {
public:
  MallocOutputFile(C &ctx, std::string path, i64 filesize, i64 perm)
    : OutputFile<C>(path, filesize, false), perm(perm) {
    this->buf = (u8 *)malloc(filesize);
    if (!this->buf)
      Fatal(ctx) << "malloc failed";
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
    free(this->buf);
  }

private:
  i64 perm;
};

template <typename C>
std::unique_ptr<OutputFile<C>>
OutputFile<C>::open(C &ctx, std::string path, i64 filesize, i64 perm) {
  Timer t(ctx, "open_file");

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  OutputFile<C> *file = new MallocOutputFile<C>(ctx, path, filesize, perm);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return std::unique_ptr<OutputFile<C>>(file);
}

} // namespace mold
