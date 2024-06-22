#include "common.h"

namespace mold {

MappedFile *open_file_impl(const std::string &path, std::string &error) {
  i64 fd = ::open(path.c_str(), O_RDONLY);
  if (fd == -1) {
    if (errno != ENOENT)
      error = "opening " + path + " failed: " + errno_string();
    return nullptr;
  }

  struct stat st;
  if (fstat(fd, &st) == -1)
    error = path + ": fstat failed: " + errno_string();

  MappedFile *mf = new MappedFile;
  mf->name = path;
  mf->size = st.st_size;

  if (st.st_size > 0) {
    mf->data = (u8 *)mmap(nullptr, st.st_size, PROT_READ | PROT_WRITE,
                          MAP_PRIVATE, fd, 0);
    if (mf->data == MAP_FAILED)
      error = path + ": mmap failed: " + errno_string();
  }

  close(fd);
  return mf;
}

void MappedFile::unmap() {
  if (size == 0 || parent || !data)
    return;
  munmap(data, size);
  data = nullptr;
}

void MappedFile::close_fd() {
  if (fd == -1)
    return;
  close(fd);
  fd = -1;
}

void MappedFile::reopen_fd(const std::string &path) {
  if (fd != -1)
    close(fd);
  fd = open(path.c_str(), O_RDONLY);
}

} // namespace mold
