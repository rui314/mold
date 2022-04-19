#include "mold.h"

namespace mold::elf {

// The hash function for .gdb_index.
static u32 gdb_hash(std::string_view name) {
  u32 h = 0;
  for (u8 c : name) {
    if ('A' <= c && c <= 'Z')
      c = 'a' + c - 'A';
    h = h * 67 + c - 113;
  }
  return h;
}

// Split .debug_info into so-called "compilation units". A .debug_info
// section usually contains one compunit unless it was created by `ld -r`.
// This is for --gdb-index.
template <typename E>
std::vector<std::string_view>
read_compunits(Context<E> &ctx, ObjectFile<E> &file) {
  file.debug_info->uncompress(ctx);
  std::string_view data = file.debug_info->contents;
  std::vector<std::string_view> vec;

  while (!data.empty()) {
    if (data.size() < 4)
      Fatal(ctx) << *file.debug_info << ": corrupted .debug_info";
    i64 len = *(u32 *)data.data() + 4;
    vec.push_back(data.substr(0, len));
    data = data.substr(len);
  }
  return vec;
}

// Parses .debug_gnu_pubnames and .debug_gnu_pubtypes. These sections
// start with a 14 bytes header followed by (4-byte offset, 1-byte type,
// null-terminated string) tuples.
//
// The 4-byte offset is an offset into .debug_info that contains details
// about the name. The 1-byte type is a type of the corresponding name
// (e.g. function, variable or datatype). The string is a name of a
// function, a variable or a type.
template <typename E>
std::vector<GdbIndexName> read_pubnames(Context<E> &ctx, ObjectFile<E> &file) {
  std::vector<GdbIndexName> vec;

  auto get_cu_idx = [&](InputSection<E> &isec, i64 offset) {
    i64 off = 0;
    for (i64 i = 0; i < file.compunits.size(); i++) {
      if (offset == off)
        return file.compunits_idx + i;
      off += file.compunits[i].size();
    }
    Fatal(ctx) << isec << ": corrupted debug_info_offset";
  };

  auto read = [&](InputSection<E> &isec) {
    isec.uncompress(ctx);
    std::string_view contents = isec.contents;

    while (!contents.empty()) {
      if (contents.size() < 14)
        Fatal(ctx) << isec << ": corrupted header";

      u32 len = *(u32 *)contents.data() + 4;
      u32 debug_info_offset = *(u32 *)(contents.data() + 6);
      u32 cu_idx = get_cu_idx(isec, debug_info_offset);

      std::string_view data = contents.substr(14, len - 14);
      contents = contents.substr(len);

      while (!data.empty()) {
        u32 offset = *(u32 *)data.data();
        data = data.substr(4);
        if (offset == 0)
          break;

        u8 type = data[0];
        data = data.substr(1);

        std::string_view name = data.data();
        data = data.substr(name.size() + 1);

        vec.push_back({name, gdb_hash(name), offset + debug_info_offset,
                       (type << 24) | cu_idx});
      }
    }
  };

  if (file.debug_pubnames)
    read(*file.debug_pubnames);
  if (file.debug_pubtypes)
    read(*file.debug_pubtypes);
  return vec;
}

#define INSTANTIATE(E)                                                  \
  template std::vector<std::string_view>                                \
  read_compunits(Context<E> &, ObjectFile<E> &);                        \
  template std::vector<GdbIndexName>                                    \
  read_pubnames(Context<E> &, ObjectFile<E> &);

INSTANTIATE_ALL;

} // namespace mold::elf
