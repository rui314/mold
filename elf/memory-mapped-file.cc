#include "mold.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::elf {

template <typename E>
MemoryMappedFile<E> *
MemoryMappedFile<E>::open(Context<E> &ctx, std::string path) {
  MemoryMappedFile *mb = new MemoryMappedFile;
  mb->name = path;

  ctx.owning_mbs.push_back(std::unique_ptr<MemoryMappedFile>(mb));

  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  i64 fd = ::open(path.c_str(), O_RDONLY);
  if (fd == -1)
    return nullptr;

  struct stat st;
  if (fstat(fd, &st) == -1)
    Fatal(ctx) << path << ": fstat failed: " << errno_string();

  mb->size = st.st_size;

#ifdef __APPLE__
  mb->mtime = (u64)st.st_mtimespec.tv_sec * 1000000000 + st.st_mtimespec.tv_nsec;
#else
  mb->mtime = (u64)st.st_mtim.tv_sec * 1000000000 + st.st_mtim.tv_nsec;
#endif

  if (st.st_size > 0) {
    mb->data = (u8 *)mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    if (mb->data == MAP_FAILED)
      Fatal(ctx) << path << ": mmap failed: " << errno_string();
  }

  close(fd);
  return mb;
}

template <typename E>
MemoryMappedFile<E> *
MemoryMappedFile<E>::must_open(Context<E> &ctx, std::string path) {
  if (MemoryMappedFile *mb = MemoryMappedFile::open(ctx, path))
    return mb;
  Fatal(ctx) << "cannot open " << path;
}

template <typename E>
MemoryMappedFile<E> *
MemoryMappedFile<E>::slice(Context<E> &ctx, std::string name, u64 start,
                           u64 size) {
  MemoryMappedFile *mb = new MemoryMappedFile<E>;
  mb->name = name;
  mb->data = data + start;
  mb->size = size;
  mb->parent = this;

  ctx.owning_mbs.push_back(std::unique_ptr<MemoryMappedFile>(mb));
  return mb;
}

template <typename E>
MemoryMappedFile<E>::~MemoryMappedFile() {
  if (size && !parent)
    munmap(data, size);
}

template <typename E>
static bool is_text_file(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data;
  return mb->size >= 4 && isprint(data[0]) && isprint(data[1]) &&
         isprint(data[2]) && isprint(data[3]);
}

template <typename E>
FileType get_file_type(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data;

  if (mb->size >= 20 && memcmp(data, "\177ELF", 4) == 0) {
    ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)data;
    if (ehdr.e_type == ET_REL)
      return FileType::OBJ;
    if (ehdr.e_type == ET_DYN)
      return FileType::DSO;
    return FileType::UNKNOWN;
  }

  if (mb->size >= 8 && memcmp(data, "!<arch>\n", 8) == 0)
    return FileType::AR;
  if (mb->size >= 8 && memcmp(data, "!<thin>\n", 8) == 0)
    return FileType::THIN_AR;
  if (is_text_file(ctx, mb))
    return FileType::TEXT;
  if (mb->size >= 4 && memcmp(data, "\xDE\xC0\x17\x0B", 4) == 0)
    return FileType::LLVM_BITCODE;
  if (mb->size >= 4 && memcmp(data, "BC\xC0\xDE", 4) == 0)
    return FileType::LLVM_BITCODE;
  return FileType::UNKNOWN;
}

#define INSTANTIATE(E)                                                  \
  template class MemoryMappedFile<E>;                                   \
  template FileType get_file_type(Context<E> &, MemoryMappedFile<E> *)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(AARCH64);

} // namespace mold::elf
