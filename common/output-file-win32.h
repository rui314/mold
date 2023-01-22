#include "../elf/mold.h"

#include <fcntl.h>
#include <filesystem>
#include <windows.h>

namespace mold {

template <typename Context>
class MallocOutputFile : public OutputFile<Context> {
public:
  MallocOutputFile(Context &ctx, std::string path, i64 filesize, i64 perm)
    : OutputFile<Context>(path, filesize, false), perm(perm) {
    this->buf = (u8 *)malloc(filesize);
    if (!this->buf)
      Fatal(ctx) << "malloc failed";
  }

  void close(Context &ctx) override {
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

template <typename Context>
std::unique_ptr<OutputFile<Context>>
OutputFile<Context>::open(Context &ctx, std::string path, i64 filesize, i64 perm) {
  Timer t(ctx, "open_file");

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  OutputFile<Context> *file = new MallocOutputFile(ctx, path, filesize, perm);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return std::unique_ptr<OutputFile<Context>>(file);
}

} // namespace mold
