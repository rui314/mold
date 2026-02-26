#include "mold.h"

#include <fcntl.h>
#include <windows.h>

namespace mold {

template <typename E>
class MemoryMappedOutputFile : public OutputFile<E> {
public:
  MemoryMappedOutputFile(Context<E> &ctx, std::string path, i64 filesize, int perm)
      : OutputFile<E>(path, filesize, true) {
    // TODO: use intermediate temporary file for output.
    DWORD attrs = (perm & 0200) ? FILE_ATTRIBUTE_NORMAL : FILE_ATTRIBUTE_READONLY;

    handle = CreateFileA(path.c_str(), GENERIC_READ | GENERIC_WRITE,
                         FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                         nullptr, CREATE_ALWAYS, attrs, nullptr);
    if (handle == INVALID_HANDLE_VALUE)
      Fatal(ctx) << "cannot open " << path << ": " << GetLastError();

    HANDLE map = CreateFileMapping(handle, nullptr, PAGE_READWRITE, 0,
                                   filesize, nullptr);
    if (!map)
      Fatal(ctx) << path << ": CreateFileMapping failed: " << GetLastError();

    this->buf = (u8 *)MapViewOfFile(map, FILE_MAP_WRITE, 0, 0, filesize);
    if (!this->buf)
      Fatal(ctx) << path << ": MapViewOfFile failed: " << GetLastError();

    CloseHandle(map);

    mold::output_buffer_start = this->buf;
    mold::output_buffer_end = this->buf + filesize;
  }

  ~MemoryMappedOutputFile() {
    if (handle != INVALID_HANDLE_VALUE)
      CloseHandle(handle);
  }

  void close(Context<E> &ctx) override {
    Timer t(ctx, "close_file");

    UnmapViewOfFile(this->buf);

    if (this->buf2) {
      if (SetFilePointer(handle, 0, nullptr, FILE_END) == INVALID_SET_FILE_POINTER)
        Fatal(ctx) << this->path << ": SetFilePointer failed: "
                   << GetLastError();

      DWORD written;
      if (!WriteFile(handle, this->buf2, this->buf2_size, &written,
                     nullptr))
        Fatal(ctx) << this->path << ": WriteFile failed: " << GetLastError();
    }

    CloseHandle(handle);
    handle = INVALID_HANDLE_VALUE;
  }

private:
  HANDLE handle;
};

template <typename E>
std::unique_ptr<OutputFile<E>>
OutputFile<E>::open(Context<E> &ctx, std::string path, i64 filesize, int perm) {
  Timer t(ctx, "open_file");

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  bool is_special = false;
  if (path == "-") {
    is_special = true;
  } else {
    HANDLE h = CreateFileA(path.c_str(), GENERIC_READ,
                           FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                           nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h != INVALID_HANDLE_VALUE) {
      if (GetFileType(h) != FILE_TYPE_DISK)
        is_special = true;
      CloseHandle(h);
    }
  }

  OutputFile<E> *file;
  if (is_special)
    file = new MallocOutputFile(ctx, path, filesize, perm);
  else
    file = new MemoryMappedOutputFile(ctx, path, filesize, perm);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return std::unique_ptr<OutputFile<E>>(file);
}

template <typename E>
LockingOutputFile<E>::LockingOutputFile(Context<E> &ctx, std::string path,
                                        int perm)
  : OutputFile<E>(path, 0, true) {
  Fatal(ctx) << "LockingOutputFile is not supported on Windows";
}

template <typename E>
void LockingOutputFile<E>::resize(Context<E> &ctx, i64 filesize) {}

template <typename E>
void LockingOutputFile<E>::close(Context<E> &ctx) {}

using E = MOLD_TARGET;

template class OutputFile<E>;
template class LockingOutputFile<E>;

} // namespace mold
