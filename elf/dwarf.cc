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
    if (*(u32 *)data.data() == 0xffffffff)
      Fatal(ctx) << *file.debug_info << ": --gdb-index: DWARF64 not supported";
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

// Try to find a compilation unit from .debug_info and its
// corresponding record from .debug_abbrev and returns them.
template <typename E>
static std::tuple<u8 *, u8 *, u32>
find_compunit(Context<E> &ctx, ObjectFile<E> &file, i64 offset) {
  // Read .debug_info to find the record at a given offset.
  u8 *cu = (u8 *)(ctx.buf + ctx.debug_info->shdr.sh_offset + offset);
  u32 dwarf_version = *(u16 *)(cu + 4);
  u32 abbrev_offset;

  // Skip a header.
  switch (dwarf_version) {
  case 2:
  case 3:
  case 4:
    abbrev_offset = *(u32 *)(cu + 6);
    cu += 11;
    break;
  case 5: {
    abbrev_offset = *(u32 *)(cu + 8);

    switch (u32 unit_type = *(u8 *)(cu + 6); unit_type) {
    case DW_UT_compile:
    case DW_UT_partial:
      cu += 12;
      break;
    case DW_UT_skeleton:
    case DW_UT_split_compile:
      cu += 20;
      break;
    default:
      Fatal(ctx) << file << ": --gdb-index: unknown DW_UT_* value: 0x"
                 << std::hex << unit_type;
    }
    break;
  }
  default:
    Fatal(ctx) << file << ": --gdb-index: unknown DWARF version: "
               << dwarf_version;
  }

  u32 abbrev_code = read_uleb(cu);

  // Find a .debug_abbrev record corresponding to the .debug_info record.
  // We assume the .debug_info record at a given offset is of
  // DW_TAG_compile_unit which describes a compunit.
  u8 *abbrev = (u8 *)(ctx.buf + ctx.debug_abbrev->shdr.sh_offset + abbrev_offset);

  for (;;) {
    u32 code = read_uleb(abbrev);
    if (code == 0)
      Fatal(ctx) << file << ": --gdb-index: .debug_abbrev does not contain"
                 << " a record for the first .debug_info record";

    if (code == abbrev_code) {
      // Found a record
      u64 abbrev_tag = read_uleb(abbrev);
      if (abbrev_tag != DW_TAG_compile_unit && abbrev_tag != DW_TAG_skeleton_unit)
        Fatal(ctx) << file << ": --gdb-index: the first entry's tag is not "
                   << " DW_TAG_compile_unit/DW_TAG_skeleton_unit but 0x"
                   << std::hex << abbrev_tag;
      break;
    }

    // Skip an uninteresting record
    read_uleb(abbrev); // tag
    abbrev++; // has_children byte
    for (;;) {
      u64 name = read_uleb(abbrev);
      u64 form = read_uleb(abbrev);
      if (name == 0 && form == 0)
        break;
      if (form == DW_FORM_implicit_const)
        read_uleb(abbrev);
    }
  }

  abbrev++; // skip has_children byte
  return {cu, abbrev, dwarf_version};
}

// Estimate the number of address ranges contained in a given file.
// It may over-estimate but never under-estimate.
template <typename E>
i64 estimate_address_areas(Context<E> &ctx, ObjectFile<E> &file) {
  // Each CU contains zero or one address area.
  i64 ret = file.compunits.size();

  // Optionally, a CU can refer an address area list in .debug_ranges.
  // .debug_ranges contains a vector of [begin, end) address pairs.
  // The last entry must be a null terminator, so we do -1.
  if (file.debug_ranges)
    ret += file.debug_ranges->sh_size / E::word_size / 2 - 1;

  // Or also .debug_rnglists, which is more complicated, as it first contains
  // a vector of offsets and then it contains a vector of differently-sized
  // entries depending on the value of DW_RLE_* code. The smallest possible
  // range entry is one byte for the code and two uleb values (each can be
  // as small as one byte), so 3 bytes.
  if (file.debug_rnglists)
    ret += file.debug_rnglists->sh_size / 3;
  return ret;
}

// .debug_info contains a variable-length fields. This class reads them.
template <typename E>
class DebugInfoReader {
public:
  DebugInfoReader(Context<E> &ctx, ObjectFile<E> &file, u8 *cu)
    : ctx(ctx), file(file), cu(cu) {}

  u64 read(u64 form);
  void skip(u64 form);

  Context<E> &ctx;
  ObjectFile<E> &file;
  u8 *cu;
  std::optional<u64> addr_base;
  std::optional<u64> rnglists_base;
};

// Read value of the given DW_FORM_* form. We need this so far only
// for DW_AT_low_pc, DW_AT_high_pc and DW_AT_ranges, so only forms
// needed by those need to be handled.
template <typename E>
u64 DebugInfoReader<E>::read(u64 form) {
  auto read_addrx = [&](i64 idx) {
    if (!addr_base)
      Fatal(ctx) << file << ": --gdb-index: missing DW_AT_addr_base";
    return *(typename E::WordTy *)(ctx.buf + ctx.debug_addr->shdr.sh_offset +
                                   *addr_base + idx * E::word_size);
  };

  switch (form) {
  case DW_FORM_data1:
    return *cu++;
  case DW_FORM_data2: {
    u64 val = *(u16 *)cu;
    cu += 2;
    return val;
  }
  case DW_FORM_data4:
  case DW_FORM_sec_offset: {
    u64 val = *(u32 *)cu;
    cu += 4;
    return val;
  }
  case DW_FORM_data8: {
    u64 val = *(u64 *)cu;
    cu += 8;
    return val;
  }
  case DW_FORM_addr: {
    u64 val = *(typename E::WordTy *)cu;
    cu += E::word_size;
    return val;
  }
  case DW_FORM_addrx1:
    return read_addrx(*cu++);
  case DW_FORM_addrx2: {
    u64 val = *(u16 *)cu;
    cu += 2;
    return read_addrx(val);
  }
  case DW_FORM_addrx4: {
    u64 val = *(u32 *)cu;
    cu += 4;
    return read_addrx(val);
  }
  case DW_FORM_addrx:
    return read_addrx(read_uleb(cu));
  case DW_FORM_rnglistx: {
    if (!rnglists_base)
      Fatal(ctx) << file << ": --gdb-index: missing DW_AT_rnglists_base";

    u64 index = read_uleb(cu);
    u64 offset_to_offset = *rnglists_base + index * 4;
    u64 val = *(u32 *)(ctx.buf + ctx.debug_rnglists->shdr.sh_offset +
                       offset_to_offset);
    return *rnglists_base + val;
  }
  default:
    Fatal(ctx) << file << ": --gdb-index: unhandled debug info form: 0x"
               << std::hex << form;
    return 0;
  }
}

// Skip over the given form, without caring what the value is, as for many
// of them we do not need to know.
template <typename E>
void DebugInfoReader<E>::skip(u64 form) {
  switch (form) {
  case DW_FORM_flag_present:
    return;
  case DW_FORM_data1:
  case DW_FORM_flag:
  case DW_FORM_strx1:
  case DW_FORM_addrx1:
  case DW_FORM_ref1:
    cu++;
    return;
  case DW_FORM_data2:
  case DW_FORM_strx2:
  case DW_FORM_addrx2:
  case DW_FORM_ref2:
    cu += 2;
    return;
  case DW_FORM_data4:
  case DW_FORM_strp:
  case DW_FORM_sec_offset:
  case DW_FORM_line_strp:
  case DW_FORM_strx4:
  case DW_FORM_addrx4:
  case DW_FORM_ref4:
    cu += 4;
    return;
  case DW_FORM_data8:
  case DW_FORM_ref8:
    cu += 8;
    return;
  case DW_FORM_addr:
  case DW_FORM_ref_addr:
    cu += E::word_size;
    return;
  case DW_FORM_strx:
  case DW_FORM_addrx:
  case DW_FORM_ref_udata:
  case DW_FORM_loclistx:
  case DW_FORM_rnglistx:
    read_uleb(cu);
    return;
  case DW_FORM_string:
    while (*cu)
      cu++;
    cu++;
    return;
  default:
    Fatal(ctx) << file << ": --gdb-index: unknown debug info form: 0x"
               << std::hex << form;
    return;
  }
}

// Read a range list from .debug_ranges starting at the given offset
// (until an end of list entry).
template <typename E>
static std::vector<u64>
read_debug_range(Context<E> &ctx, ObjectFile<E> &file, u64 offset) {
  if (!ctx.debug_ranges)
    Fatal(ctx) << file << ": --gdb-index: missing debug_ranges";

  typename E::WordTy *range =
    (typename E::WordTy *)(ctx.buf + ctx.debug_ranges->shdr.sh_offset + offset);

  std::vector<u64> vec;
  u64 base = 0;

  for (i64 i = 0; range[i] || range[i + 1]; i += 2) {
    if (range[i] == (typename E::WordTy)-1) { // base address selection entry
      base = range[i + 1];
      continue;
    }

    vec.push_back(range[i] + base);
    vec.push_back(range[i + 1] + base);
  }
  return vec;
}

// Read a range list from .debug_rnglists starting at the given offset
// (until an end of list entry).
template <typename E>
static std::vector<u64>
read_rnglist_range(Context<E> &ctx, ObjectFile<E> &file, u64 offset,
                   std::optional<u64> addr_base) {
  if (!ctx.debug_rnglists)
    Fatal(ctx) << file << ": --gdb-index: missing debug_rnglists";

  u8 *rnglist = (u8 *)(ctx.buf + ctx.debug_rnglists->shdr.sh_offset + offset);

  std::vector<u64> vec;
  u64 base = 0;

  auto read_addrx = [&](i64 idx) {
    if (!addr_base)
      Fatal(ctx) << file << ": --gdb-index: missing DW_AT_addr_base";
    return *(typename E::WordTy *)(ctx.buf + ctx.debug_addr->shdr.sh_offset +
                                   *addr_base + idx * E::word_size);
  };

  for (;;) {
    switch (*rnglist++) {
    case DW_RLE_end_of_list:
      return vec;
    case DW_RLE_base_addressx:
      base = read_addrx(read_uleb(rnglist));
      continue;
    case DW_RLE_startx_endx:
      vec.push_back(read_addrx(read_uleb(rnglist)));
      vec.push_back(read_addrx(read_uleb(rnglist)));
      break;
    case DW_RLE_startx_length:
      vec.push_back(read_addrx(read_uleb(rnglist)));
      vec.push_back(vec.back() + read_uleb(rnglist));
      break;
    case DW_RLE_offset_pair:
      vec.push_back(base + read_uleb(rnglist));
      vec.push_back(base + read_uleb(rnglist));
      break;
    case DW_RLE_base_address:
      base = *(u32 *)rnglist;
      rnglist += 4;
      continue;
    case DW_RLE_start_end:
      vec.push_back(*(u32 *)rnglist);
      rnglist += 4;
      vec.push_back(*(u32 *)rnglist);
      rnglist += 4;
      break;
    case DW_RLE_start_length:
      vec.push_back(*(u32 *)rnglist);
      rnglist += 4;
      vec.push_back(read_uleb(rnglist));
      break;
    }
  }
}

// Returns a list of address ranges explained by a compunit at the
// `offset` in an output .debug_info section.
//
// .debug_info contains DWARF debug info records, so this function
// parses DWARF. If a designated compunit contains multiple ranges, the
// ranges are read from .debug_ranges (or .debug_rnglists for DWARF5).
// Otherwise, a range is read directly from .debug_info (or possibly
// from .debug_addr for DWARF5).
template <typename E>
std::vector<u64>
read_address_areas(Context<E> &ctx, ObjectFile<E> &file, i64 offset) {
  u8 *cu;
  u8 *abbrev;
  u32 dwarf_version;
  std::tie(cu, abbrev, dwarf_version) = find_compunit(ctx, file, offset);

  DebugInfoReader<E> reader{ctx, file, cu};

  // At least Clang14 can emit DW_AT_addr_base and DW_AT_rnglists_base only
  // after previous entries already used any of the DW_FORM_addrx* forms
  // or DW_FORM_rnglistx, so the *_base values need to be read first.
  u8 *saved_abbrev = abbrev;
  while (!reader.addr_base || !reader.rnglists_base) {
    u64 name = read_uleb(abbrev);
    u64 form = read_uleb(abbrev);
    if (name == 0 && form == 0)
      break;

    switch (name) {
    case DW_AT_addr_base:
      reader.addr_base = reader.read(form);
      break;
    case DW_AT_rnglists_base:
      reader.rnglists_base = reader.read(form);
      break;
    default:
      reader.skip(form);
      break;
    }
  }

  reader.cu = cu;
  abbrev = saved_abbrev;

  std::optional<u64> high_pc_abs;
  std::optional<u64> high_pc_rel;
  std::optional<u64> low_pc;

  for (;;) {
    u64 name = read_uleb(abbrev);
    u64 form = read_uleb(abbrev);
    if (name == 0 && form == 0)
      break;

    switch (name) {
    case DW_AT_low_pc:
      low_pc = reader.read(form);
      break;
    case DW_AT_high_pc:
      switch (form) {
      case DW_FORM_addr:
      case DW_FORM_addrx:
      case DW_FORM_addrx1:
      case DW_FORM_addrx2:
      case DW_FORM_addrx4:
        high_pc_abs = reader.read(form);
        break;
      case DW_FORM_data1:
      case DW_FORM_data2:
      case DW_FORM_data4:
      case DW_FORM_data8:
        high_pc_rel = reader.read(form);
        break;
      default:
        Fatal(ctx) << file << ": --gdb-index: unhandled form for DW_AT_high_pc 0x"
                   << std::hex << form;
      }
      break;
    case DW_AT_ranges: {
      u64 offset = reader.read(form);
      if (dwarf_version <= 4)
        return read_debug_range(ctx, file, offset);
      return read_rnglist_range(ctx, file, offset, reader.addr_base);
    }
    default:
      reader.skip(form);
      break;
    }
  }

  if (low_pc && high_pc_abs)
    return {*low_pc, *high_pc_abs};
  if (low_pc && high_pc_rel)
    return {*low_pc, *low_pc + *high_pc_rel};

  return {};
}

#define INSTANTIATE(E)                                                  \
  template std::vector<std::string_view>                                \
  read_compunits(Context<E> &, ObjectFile<E> &);                        \
  template std::vector<GdbIndexName>                                    \
  read_pubnames(Context<E> &, ObjectFile<E> &);                         \
  template i64                                                          \
  estimate_address_areas(Context<E> &, ObjectFile<E> &);                \
  template std::vector<u64>                                             \
  read_address_areas(Context<E> &, ObjectFile<E> &, i64)

INSTANTIATE_ALL;

} // namespace mold::elf
