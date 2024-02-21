#include "common.h"

#include <fcntl.h>
#include <filesystem>
#include <windows.h>

namespace mold {

template <typename Context>
class MemoryMappedOutputFile : public OutputFile<Context> {
public:
  MemoryMappedOutputFile(Context &ctx, std::string path, i64 filesize, i64 perm)
      : OutputFile<Context>(path, filesize, true) {
    // TODO: use intermediate temporary file for output.
    DWORD file_attrs =
        (perm & 0200) ? FILE_ATTRIBUTE_NORMAL : FILE_ATTRIBUTE_READONLY;
    file_handle =
        CreateFileA(path.c_str(), GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    nullptr, CREATE_ALWAYS, file_attrs, nullptr);
    if (file_handle == INVALID_HANDLE_VALUE)
      Fatal(ctx) << "cannot open " << path << ": " << GetLastError();

    HANDLE mapping_handle = CreateFileMapping(
        file_handle, nullptr, PAGE_READWRITE, 0, filesize, nullptr);
    if (!mapping_handle)
      Fatal(ctx) << path << ": CreateFileMapping failed: " << GetLastError();

    this->buf =
        (u8 *)MapViewOfFile(mapping_handle, FILE_MAP_WRITE, 0, 0, filesize);
    CloseHandle(mapping_handle);
    if (!this->buf)
      Fatal(ctx) << path << ": MapViewOfFile failed: " << GetLastError();

    mold::output_buffer_start = this->buf;
    mold::output_buffer_end = this->buf + filesize;
  }

  ~MemoryMappedOutputFile() {
    if (file_handle != INVALID_HANDLE_VALUE)
      CloseHandle(file_handle);
  }

  void close(Context &ctx) override {
    Timer t(ctx, "close_file");

    UnmapViewOfFile(this->buf);

    if (!this->buf2.empty()) {
      if (SetFilePointer(file_handle, 0, nullptr, FILE_END) ==
          INVALID_SET_FILE_POINTER)
        Fatal(ctx) << this->path
                   << ": SetFilePointer failed: " << GetLastError();

      DWORD written;
      if (!WriteFile(file_handle, this->buf2.data(), this->buf2.size(),
                     &written, nullptr))
        Fatal(ctx) << this->path << ": WriteFile failed: " << GetLastError();
    }

    CloseHandle(file_handle);
    file_handle = INVALID_HANDLE_VALUE;
  }

private:
  HANDLE file_handle;
};

template <typename Context>
std::unique_ptr<OutputFile<Context>>
OutputFile<Context>::open(Context &ctx, std::string path, i64 filesize, i64 perm) {
  Timer t(ctx, "open_file");

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  bool is_special = false;
  if (path == "-") {
    is_special = true;
  } else {
    HANDLE file_handle =
        CreateFileA(path.c_str(), GENERIC_READ,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file_handle != INVALID_HANDLE_VALUE) {
      if (GetFileType(file_handle) != FILE_TYPE_DISK)
        is_special = true;
      CloseHandle(file_handle);
    }
  }

  OutputFile<Context> *file;
  if (is_special)
    file = new MallocOutputFile(ctx, path, filesize, perm);
  else
    file = new MemoryMappedOutputFile(ctx, path, filesize, perm);

  if (ctx.arg.filler != -1)
    memset(file->buf, ctx.arg.filler, filesize);
  return std::unique_ptr<OutputFile<Context>>(file);
}

} // namespace mold
