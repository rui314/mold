#include "common.h"

namespace mold {

MappedFile *open_file_impl(const std::string &path, std::string &error) {
  HANDLE fd = CreateFileA(path.c_str(), GENERIC_READ,
                          FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                          nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);

  if (fd == INVALID_HANDLE_VALUE) {
    auto err = GetLastError();
    if (err != ERROR_FILE_NOT_FOUND)
      error = "opening " + path + " failed: " + err;
    return nullptr;
  }

  if (GetFileType(fd) != FILE_TYPE_DISK) {
    CloseHandle(fd);
    return nullptr;
  }

  DWORD size_hi;
  DWORD size_lo = GetFileSize(fd, &size_hi);

  if (size_lo == INVALID_FILE_SIZE) {
    error = path + ": GetFileSize failed: " + GetLastError();
    return nullptr;
  }

  u64 size = ((u64)size_hi << 32) + size_lo;

  MappedFile *mf = new MappedFile;
  mf->name = path;
  mf->size = size;
  mf->fd = fd;

  if (size > 0) {
    HANDLE h = CreateFileMapping(fd, nullptr, PAGE_READONLY, 0, size, nullptr);
    if (!h) {
      error = path + ": CreateFileMapping failed: " + GetLastError();
      return nullptr;
    }

    mf->data = (u8 *)MapViewOfFile(h, FILE_MAP_COPY, 0, 0, size);
    CloseHandle(h);

    if (!mf->data) {
      error = path + ": MapViewOfFile failed: " + GetLastError();
      return nullptr;
    }
  }

  return mf;
}

void MappedFile::unmap() {
  if (size == 0 || parent || !data)
    return;

  UnmapViewOfFile(data);
  if (fd != INVALID_HANDLE_VALUE)
    CloseHandle(fd);

  data = nullptr;
}

} // namespace mold
