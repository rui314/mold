#include "mold.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

OutputFile::OutputFile(Context &ctx, std::string path, i64 filesize, i64 perm)
    : path(path), filesize(filesize), perm(perm) {
  buf = (u8 *)mmap(NULL, filesize, PROT_READ | PROT_WRITE,
                   MAP_SHARED | MAP_ANONYMOUS, -1, 0);
  if (buf == MAP_FAILED)
    Fatal(ctx) << "mmap failed: " << errno_string();
}

void OutputFile::close(Context &ctx) {
  if (path == "-") {
    fwrite(buf, filesize, 1, stdout);
    fclose(stdout);
    return;
  }

  i64 fd = ::open(path.c_str(), O_RDWR | O_CREAT, perm);
  if (fd == -1)
    Fatal(ctx) << "cannot open " << this->path << ": " << errno_string();

  FILE *fp = fdopen(fd, "w");
  fwrite(this->buf, this->filesize, 1, fp);
  fclose(fp);
}

} // namespace mold::elf
