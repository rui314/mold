#include "mold.h"

namespace mold {

// Files up to this size are read into malloc'ed memory rather than
// mmap'ed. mmap(2) takes the process's address space lock, so with tens
// of thousands of input files, the calls serialize at a few microseconds
// each no matter how many threads make them. read(2) takes no such lock,
// but copying costs memory bandwidth in proportion to the file size, so
// large files are still mmap'ed. On a Chromium link, files below this
// threshold are 64% of the inputs by count but 16% by size.
//
// The buffers add up to hundreds of megabytes on such a link, so they
// need to be backed by huge pages; with 4 KiB pages, faulting them in
// costs more than the mmap calls did. mimalloc does that by default.
static constexpr i64 READ_THRESHOLD = 32 * 1024;

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

  if (st.st_size > 0 && st.st_size <= READ_THRESHOLD && S_ISREG(st.st_mode)) {
    u8 *buf = (u8 *)malloc(st.st_size);
    for (i64 pos = 0; pos < st.st_size;) {
      i64 n = pread(fd, buf + pos, st.st_size - pos, pos);
      if (n > 0) {
        pos += n;
      } else if (n == 0) {
        error = path + ": file is shorter than its reported size";
        break;
      } else if (errno != EINTR) {
        error = path + ": read failed: " + errno_string();
        break;
      }
    }
    mf->data = buf;
  } else if (st.st_size > 0) {
    mf->data = (u8 *)mmap(nullptr, st.st_size, PROT_READ | PROT_WRITE,
                          MAP_PRIVATE, fd, 0);
    if (mf->data == MAP_FAILED)
      error = path + ": mmap failed: " + errno_string();
    mf->is_mmapped = true;
  }

  close(fd);
  return mf;
}

void MappedFile::unmap() {
  if (size == 0 || parent || !data)
    return;
  if (is_mmapped)
    munmap(data, size);
  else
    free(data);
  data = nullptr;
}

void MappedFile::close_fd() {
  if (fd == -1)
    return;
  close(fd);
  fd = -1;
}

void MappedFile::reopen_fd(const std::string &path) {
  if (fd == -1)
    fd = open(path.c_str(), O_RDONLY);
}

} // namespace mold
