#include "mold.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

template <typename E>
MemoryMappedFile<E> *
MemoryMappedFile<E>::open(Context<E> &ctx, std::string path) {
  if (path.starts_with('/') && !ctx.arg.chroot.empty())
    path = ctx.arg.chroot + "/" + path_clean(path);

  struct stat st;
  if (stat(path.c_str(), &st) == -1)
    return nullptr;
  u64 mtime = (u64)st.st_mtim.tv_sec * 1000000000 + st.st_mtim.tv_nsec;

  MemoryMappedFile *mb = new MemoryMappedFile(path, nullptr, st.st_size, mtime);
  ctx.owning_mbs.push_back(std::unique_ptr<MemoryMappedFile>(mb));
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
u8 *MemoryMappedFile<E>::data(Context<E> &ctx) {
  if (data_)
    return data_;

  std::lock_guard lock(mu);
  if (data_)
    return data_;

  i64 fd = ::open(name.c_str(), O_RDONLY);
  if (fd == -1)
    Fatal(ctx) << name << ": cannot open: " << strerror(errno);

  data_ = (u8 *)mmap(nullptr, size_, PROT_READ, MAP_PRIVATE, fd, 0);
  if (data_ == MAP_FAILED)
    Fatal(ctx) << name << ": mmap failed: " << strerror(errno);
  close(fd);
  return data_;
}

template <typename E>
MemoryMappedFile<E> *
MemoryMappedFile<E>::slice(Context<E> &ctx, std::string name, u64 start,
                           u64 size) {
  MemoryMappedFile *mb = new MemoryMappedFile<E>(name, data_ + start, size);
  ctx.owning_mbs.push_back(std::unique_ptr<MemoryMappedFile>(mb));
  mb->parent = this;
  return mb;
}

template <typename E>
MemoryMappedFile<E>::~MemoryMappedFile() {
  if (data_ && !parent)
    munmap(data_, size_);
}


template <typename E>
static bool is_text_file(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data(ctx);
  return mb->size() >= 4 &&
         isprint(data[0]) &&
         isprint(data[1]) &&
         isprint(data[2]) &&
         isprint(data[3]);
}

template <typename E>
FileType get_file_type(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  u8 *data = mb->data(ctx);

  if (mb->size() >= 20 && memcmp(data, "\177ELF", 4) == 0) {
    ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)data;
    if (ehdr.e_type == ET_REL)
      return FileType::OBJ;
    if (ehdr.e_type == ET_DYN)
      return FileType::DSO;
    return FileType::UNKNOWN;
  }

  if (mb->size() >= 8 && memcmp(data, "!<arch>\n", 8) == 0)
    return FileType::AR;
  if (mb->size() >= 8 && memcmp(data, "!<thin>\n", 8) == 0)
    return FileType::THIN_AR;
  if (is_text_file(ctx, mb))
    return FileType::TEXT;
  return FileType::UNKNOWN;
}

#define INSTANTIATE(E)                                                  \
  template class MemoryMappedFile<E>;                                   \
  template FileType get_file_type(Context<E> &, MemoryMappedFile<E> *)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
