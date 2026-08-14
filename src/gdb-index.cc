// This file contains code to read DWARF debug info to create .gdb_index.
//
// .gdb_index is an optional section to speed up GNU debugger. It contains
// two maps: 1) a map from function/variable/type names to compunits, and
// 2) a map from function address ranges to compunits. gdb uses these
// maps to quickly find a compunit given a name or an instruction pointer.
//
// (Terminology: a compilation unit, often abbreviated as compunit or
// CU, is a unit of debug info. An input .debug_info section usually
// contains one compunit, and thus an output .debug_info contains as
// many compunits as the number of input files.)
//
// .gdb_index is not mandatory. All the information in .gdb_index is
// also in other debug info sections. You can actually create an
// executable without .gdb_index and later add it using the
// `gdb-add-index` post-processing tool that comes with gdb.
//
// Post-relocated debug section contents are needed to create a
// .gdb_index. Therefore, we create it after relocating all the other
// sections. The size of the section is also hard to estimate before
// applying relocations to debug info sections, so a .gdb_index is
// placed at the very end of the output file, even after the section
// header.
//
// The mapping from names to compunits is 1:n while the mapping from
// address ranges to compunits is 1:1. That is, two object files may
// define the same type name, while there should be no two functions
// that overlap with each other in memory.
//
// .gdb_index contains an on-disk hash table for names, so gdb can
// lookup names without loading all strings into memory and construct an
// in-memory hash table.
//
// Names are in .debug_gnu_pubnames and .debug_gnu_pubtypes input
// sections. These sections are created if `-ggnu-pubnames` is given.
// Besides names, these sections contain attributes for each name so
// that gdb can distinguish type names from function names, for example.
//
// A compunit contains one or more function address ranges. If an
// object file is compiled without -ffunction-sections, it contains
// only one .text section and therefore contains a single address range.
// Such range is typically stored directly to the compunit.
//
// If an object file is compiled with -ffunction-sections, it contains
// more than one .text section, and it has as many address ranges as
// the number of .text sections. Such discontiguous address ranges are
// stored to .debug_ranges in DWARF 2/3/4 and .debug_rnglists/.debug_addr
// in DWARF 5.
//
// .debug_info section contains DWARF debug info. Although we don't need
// to parse the whole .debug_info section to read address ranges, we
// have to do a little bit. DWARF is complicated and often handled using
// a library such as libdwarf. But we don't use any library because we
// don't want to add an extra run-time dependency just for --gdb-index.
//
// This page explains the format of .gdb_index:
// https://sourceware.org/gdb/onlinedocs/gdb/Index-Section-Format.html

#include "mold.h"
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_scan.h>

namespace mold {

enum DwarfKind { DWARF2_32, DWARF5_32, DWARF2_64, DWARF5_64 };

template <typename E>
struct CuHdrDwarf2_32 {
  U32<E> size;
  U16<E> version;
  U32<E> abbrev_offset;
  u8 address_size;
};

template <typename E>
struct CuHdrDwarf5_32 {
  U32<E> size;
  U16<E> version;
  u8 unit_type;
  u8 address_size;
  U32<E> abbrev_offset;
};

template <typename E>
struct CuHdrDwarf2_64 {
  U32<E> magic;
  U64<E> size;
  U16<E> version;
  U64<E> abbrev_offset;
  u8 address_size;
};

template <typename E>
struct CuHdrDwarf5_64 {
  U32<E> magic;
  U64<E> size;
  U16<E> version;
  u8 unit_type;
  u8 address_size;
  U64<E> abbrev_offset;
};

template <typename E>
struct PubnamesHdr32 {
  U32<E> size;
  U16<E> version;
  U32<E> debug_info_offset;
  U32<E> debug_info_size;
};

template <typename E>
struct PubnamesHdr64 {
  U32<E> magic;
  U64<E> size;
  U16<E> version;
  U64<E> debug_info_offset;
  U64<E> debug_info_size;
};

struct SectionHeader {
  ul32 version = 7;
  ul32 cu_list_offset = 0;
  ul32 cu_types_offset = 0;
  ul32 ranges_offset = 0;
  ul32 symtab_offset = 0;
  ul32 const_pool_offset = 0;
};

struct MapValue {
  u32 gdb_hash = 0;
  Atomic<u32> count;
  u32 name_offset = 0;
  u32 type_offset = 0;
};

struct NameType {
  NameType(u64 hash, u8 type, const char *name)
    : hash_and_type((hash << 8) | type), name(name) {}

  u64 get_hash() const {
    return hash_and_type >> 8;
  }

  u8 get_type() const {
    return hash_and_type & 0xff;
  }

  bool operator==(const NameType &other) const {
    return hash_and_type == other.hash_and_type && !strcmp(name, other.name);
  }

  // The low byte stores the type. Hash collisions, including collisions in
  // the discarded high byte, are resolved by comparing the complete name.
  u64 hash_and_type;
  const char *name;
};

struct IndexedName {
  MapValue *entry;
  u32 type_idx;
  u8 type;
};

// A name record is read as a NameType and replaced with an IndexedName after
// the name has been interned. The union avoids retaining both forms for the
// millions of public names in a large debug link.
union NameRecord {
  NameRecord(u64 hash, u8 type, const char *name)
    : nametype(hash, type, name) {}

  NameType nametype;
  IndexedName indexed;
};

struct Compunit {
  DwarfKind kind;
  i64 offset;
  i64 size;
  i32 file_idx;
  std::vector<std::pair<u64, u64>> ranges;
  std::vector<NameRecord> names;
};

using GdbNameMap = ConcurrentMap<MapValue>;

struct PoolSize {
  i64 type_bytes = 0;
  i64 name_bytes = 0;
};

struct GdbIndexData {
  std::vector<Compunit> cus;
  std::unique_ptr<GdbNameMap> map;
  std::vector<GdbNameMap::Entry *> entries;
  std::unique_ptr<u8[]> tables;
  PoolSize pool_size;
  i64 ht_size = 0;
};

// GDB index type lists contain fixed-width 32-bit records. Four byte-wise
// passes are faster than comparison sorting once a list is sufficiently large.
static void radix_sort(std::span<ul32> values, std::vector<ul32> &scratch) {
  scratch.resize(values.size());
  ul32 *src = values.data();
  ul32 *dst = scratch.data();

  for (i64 shift = 0; shift < 32; shift += 8) {
    u32 counts[256] = {};
    for (i64 i = 0; i < values.size(); i++)
      counts[((u32)src[i] >> shift) & 255]++;

    u32 offsets[256];
    offsets[0] = 0;
    for (i64 i = 1; i < 256; i++)
      offsets[i] = offsets[i - 1] + counts[i - 1];

    for (i64 i = 0; i < values.size(); i++) {
      u32 val = src[i];
      dst[offsets[(val >> shift) & 255]++] = val;
    }
    std::swap(src, dst);
  }

  assert(src == values.data());
}

// GCC can emit the same public name once for each COMDAT group. Remove these
// duplicates with a local hash table instead of sorting the strings.
static void dedup_names(Compunit &cu) {
  if (cu.names.size() < 2)
    return;

  u64 capacity = bit_ceil(cu.names.size() * 2);
  std::vector<u32> buckets(capacity, UINT32_MAX);
  u64 mask = capacity - 1;
  i64 out = 0;

  for (NameRecord &record : cu.names) {
    NameType &nt = record.nametype;
    u64 idx =
      (nt.get_hash() ^ ((u64)nt.get_type() * 0x9e37'79b9)) & mask;
    bool duplicate = false;

    while (buckets[idx] != UINT32_MAX) {
      if (cu.names[buckets[idx]].nametype == nt) {
        duplicate = true;
        break;
      }
      idx = (idx + 1) & mask;
    }

    if (duplicate)
      continue;

    if (&record != &cu.names[out])
      cu.names[out] = record;
    buckets[idx] = out++;
  }

  cu.names.erase(cu.names.begin() + out, cu.names.end());
}

// Compute the .gdb_index hash and length together when a name is first
// inserted.
static u32 initialize_gdb_name(MapValue &value, const char *name) {
  u32 h = 0;
  u32 size = 0;

  for (u8 c = *name; c; c = name[++size]) {
    if ('A' <= c && c <= 'Z')
      c = 'a' + c - 'A';
    h = h * 67 + c - 113;
  }
  value.gdb_hash = h;
  return size;
}

template <typename E>
static DwarfKind get_dwarf_kind(Context<E> &ctx, u8 *p) {
  if (*(U32<E> *)p == 0xffff'ffff) {
    CuHdrDwarf2_64<E> &hdr = *(CuHdrDwarf2_64<E> *)p;
    if (hdr.version > 5)
      Fatal(ctx) << "--gdb-index: DWARF version " << hdr.version
                 << " is not supported";
    return (hdr.version == 5) ? DWARF5_64 : DWARF2_64;
  }

  CuHdrDwarf2_32<E> &hdr = *(CuHdrDwarf2_32<E> *)p;
  if (hdr.version > 5)
    Fatal(ctx) << "--gdb-index: DWARF version " << hdr.version
               << " is not supported";
  return (hdr.version == 5) ? DWARF5_32 : DWARF2_32;
}

template <typename E, typename CuHdr>
u8 *find_cu_abbrev(Context<E> &ctx, u8 **p, const CuHdr &hdr) {
  if (hdr.address_size != sizeof(Word<E>))
    Fatal(ctx) << "--gdb-index: unsupported address size " << hdr.address_size;

  if constexpr (requires { hdr.unit_type; }) {
    switch (hdr.unit_type) {
    case DW_UT_compile:
    case DW_UT_partial:
      break;
    case DW_UT_skeleton:
    case DW_UT_split_compile:
      *p += 8;
      break;
    default:
      Fatal(ctx) << "--gdb-index: unknown unit type: 0x"
                 << std::hex << hdr.unit_type;
    }
  }

  i64 abbrev_code = read_uleb(p);

  // Find a .debug_abbrev record corresponding to the .debug_info record.
  // We assume the .debug_info record at a given offset is of
  // DW_TAG_compile_unit which describes a compunit.
  u8 *abbrev = &ctx.debug_abbrev[0] + hdr.abbrev_offset;

  for (;;) {
    u32 code = read_uleb(&abbrev);
    if (code == 0)
      Fatal(ctx) << "--gdb-index: .debug_abbrev does not contain"
                 << " a record for the first .debug_info record";

    if (code == abbrev_code) {
      // Found a record
      u64 abbrev_tag = read_uleb(&abbrev);
      if (abbrev_tag != DW_TAG_compile_unit && abbrev_tag != DW_TAG_skeleton_unit)
        Fatal(ctx) << "--gdb-index: the first entry's tag is not"
                   << " DW_TAG_compile_unit/DW_TAG_skeleton_unit but 0x"
                   << std::hex << abbrev_tag;
      break;
    }

    // Skip an uninteresting record
    read_uleb(&abbrev); // tag
    abbrev++; // has_children byte
    for (;;) {
      u64 name = read_uleb(&abbrev);
      u64 form = read_uleb(&abbrev);
      if (name == 0 && form == 0)
        break;
      if (form == DW_FORM_implicit_const)
        read_uleb(&abbrev);
    }
  }

  abbrev++; // skip has_children byte
  return abbrev;
}

// .debug_info contains variable-length fields.
// This function reads one scalar value from a given location.
template <typename E, typename Offset>
u64 read_scalar(Context<E> &ctx, u8 **p, u64 form) {
  switch (form) {
  case DW_FORM_flag_present:
    return 0;
  case DW_FORM_data1:
  case DW_FORM_flag:
  case DW_FORM_strx1:
  case DW_FORM_addrx1:
  case DW_FORM_ref1:
    return *(*p)++;
  case DW_FORM_data2:
  case DW_FORM_strx2:
  case DW_FORM_addrx2:
  case DW_FORM_ref2: {
    u64 val = *(U16<E> *)*p;
    *p += 2;
    return val;
  }
  case DW_FORM_strx3:
  case DW_FORM_addrx3: {
    u64 val = *(U24<E> *)*p;
    *p += 3;
    return val;
  }
  case DW_FORM_data4:
  case DW_FORM_strx4:
  case DW_FORM_addrx4:
  case DW_FORM_ref4: {
    u64 val = *(U32<E> *)*p;
    *p += 4;
    return val;
  }
  case DW_FORM_data8:
  case DW_FORM_ref8: {
    u64 val = *(U64<E> *)*p;
    *p += 8;
    return val;
  }
  case DW_FORM_strp:
  case DW_FORM_sec_offset:
  case DW_FORM_line_strp: {
    u64 val = *(Offset *)*p;
    *p += sizeof(Offset);
    return val;
  }
  case DW_FORM_addr:
  case DW_FORM_ref_addr: {
    u64 val = *(Word<E> *)*p;
    *p += sizeof(Word<E>);
    return val;
  }
  case DW_FORM_strx:
  case DW_FORM_addrx:
  case DW_FORM_udata:
  case DW_FORM_ref_udata:
  case DW_FORM_loclistx:
  case DW_FORM_rnglistx:
    return read_uleb(p);
  case DW_FORM_string:
    *p += strlen((char *)*p) + 1;
    return 0;
  default:
    Fatal(ctx) << "--gdb-index: unhandled debug info form: 0x"
               << std::hex << form;
  }
}

// Read a range list from .debug_ranges starting at the given offset.
template <typename E>
static std::vector<std::pair<u64, u64>>
read_debug_range(Word<E> *range, u64 base) {
  std::vector<std::pair<u64, u64>> vec;

  for (i64 i = 0; range[i] || range[i + 1]; i += 2) {
    if (range[i] + 1 == 0)
      base = range[i + 1];
    else
      vec.emplace_back(range[i] + base, range[i + 1] + base);
  }
  return vec;
}

// Read a range list from .debug_rnglists starting at the given offset.
template <typename E>
static void
read_rnglist_range(std::vector<std::pair<u64, u64>> &vec, u8 *p,
                   Word<E> *addrx, u64 base) {
  for (;;) {
    switch (*p++) {
    case DW_RLE_end_of_list:
      return;
    case DW_RLE_base_addressx:
      base = addrx[read_uleb(&p)];
      break;
    case DW_RLE_startx_endx: {
      u64 val1 = read_uleb(&p);
      u64 val2 = read_uleb(&p);
      vec.emplace_back(addrx[val1], addrx[val2]);
      break;
    }
    case DW_RLE_startx_length: {
      u64 val1 = read_uleb(&p);
      u64 val2 = read_uleb(&p);
      vec.emplace_back(addrx[val1], addrx[val1] + val2);
      break;
    }
    case DW_RLE_offset_pair: {
      u64 val1 = read_uleb(&p);
      u64 val2 = read_uleb(&p);

      // If the base is 0, this address range is for an eliminated
      // section. We only emit it if it's alive.
      if (base)
        vec.emplace_back(base + val1, base + val2);
      break;
    }
    case DW_RLE_base_address:
      base = *(Word<E> *)p;
      p += sizeof(Word<E>);
      break;
    case DW_RLE_start_end: {
      u64 val1 = ((Word<E> *)p)[0];
      u64 val2 = ((Word<E> *)p)[1];
      p += sizeof(Word<E>) * 2;
      vec.emplace_back(val1, val2);
      break;
    }
    case DW_RLE_start_length: {
      u64 val1 = *(Word<E> *)p;
      p += sizeof(Word<E>);
      u64 val2 = read_uleb(&p);
      vec.emplace_back(val1, val1 + val2);
      break;
    }
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
template <typename E, typename CuHdr>
static std::vector<std::pair<u64, u64>>
read_address_ranges(Context<E> &ctx, const Compunit &cu) {
  // Read .debug_info to find the record at a given offset.
  u8 *p = &ctx.debug_info[0] + cu.offset;
  CuHdr &hdr = *(CuHdr *)p;
  p += sizeof(hdr);

  u8 *abbrev = find_cu_abbrev(ctx, &p, hdr);

  // Now, read debug info records.
  struct Record {
    u64 form = 0;
    u64 value = 0;
  };

  using Offset = decltype(hdr.size);

  Record low_pc;
  Record high_pc;
  Record ranges;
  u64 rnglists_base = -1;
  Word<E> *addrx = nullptr;

  // Read all interesting debug records.
  for (;;) {
    u64 name = read_uleb(&abbrev);
    u64 form = read_uleb(&abbrev);
    if (name == 0 && form == 0)
      break;

    u64 val = read_scalar<E, Offset>(ctx, &p, form);

    switch (name) {
    case DW_AT_low_pc:
      low_pc = {form, val};
      break;
    case DW_AT_high_pc:
      high_pc = {form, val};
      break;
    case DW_AT_rnglists_base:
      rnglists_base = val;
      break;
    case DW_AT_addr_base:
      addrx = (Word<E> *)(&ctx.debug_addr[0] + val);
      break;
    case DW_AT_ranges:
      ranges = {form, val};
      break;
    }
  }

  // Handle non-contiguous address ranges.
  if (ranges.form) {
    if (hdr.version <= 4) {
      Word<E> *p = (Word<E> *)(&ctx.debug_ranges[0] + ranges.value);
      return read_debug_range<E>(p, low_pc.value);
    }

    assert(hdr.version == 5);

    std::vector<std::pair<u64, u64>> vec;
    u8 *buf = &ctx.debug_rnglists[0];

    if (ranges.form == DW_FORM_sec_offset) {
      read_rnglist_range<E>(vec, buf + ranges.value, addrx, low_pc.value);
    } else {
      if (rnglists_base == -1)
        Fatal(ctx) << "--gdb-index: missing DW_AT_rnglists_base";

      u8 *base = buf + rnglists_base;
      Offset *offsets = (Offset *)base;
      read_rnglist_range<E>(vec, base + offsets[ranges.value], addrx, low_pc.value);
    }
    return vec;
  }

  // Handle a contiguous address range.
  if (low_pc.form && high_pc.form) {
    u64 lo;

    switch (low_pc.form) {
    case DW_FORM_addr:
      lo = low_pc.value;
      break;
    case DW_FORM_addrx:
    case DW_FORM_addrx1:
    case DW_FORM_addrx2:
    case DW_FORM_addrx4:
      lo = addrx[low_pc.value];
      break;
    default:
      Fatal(ctx) << "--gdb-index: unhandled form for DW_AT_low_pc: 0x"
                 << std::hex << high_pc.form;
    }

    switch (high_pc.form) {
    case DW_FORM_addr:
      return {{lo, high_pc.value}};
    case DW_FORM_addrx:
    case DW_FORM_addrx1:
    case DW_FORM_addrx2:
    case DW_FORM_addrx4:
      return {{lo, addrx[high_pc.value]}};
    case DW_FORM_udata:
    case DW_FORM_data1:
    case DW_FORM_data2:
    case DW_FORM_data4:
    case DW_FORM_data8:
      return {{lo, lo + high_pc.value}};
    default:
      Fatal(ctx) << "--gdb-index: unhandled form for DW_AT_high_pc: 0x"
                 << std::hex << high_pc.form;
    }
  }

  return {};
}

template <typename E, typename PubnamesHdr>
static i64 read_pubnames_cu(Context<E> &ctx, const PubnamesHdr &hdr,
                            std::vector<Compunit> &cus, ObjectFile<E> &file) {
  using Offset = decltype(hdr.size);

  // Compunits are sorted by offset, so we can use binary search.
  auto get_cu = [&](i64 offset) {
    auto it = ranges::lower_bound(cus, offset, {}, &Compunit::offset);
    if (it == cus.end() || it->offset != offset)
      Fatal(ctx) << file << ": corrupted debug_info_offset";
    return &*it;
  };

  Compunit *cu = get_cu(hdr.debug_info_offset);
  i64 size = hdr.size + offsetof(PubnamesHdr, size) + sizeof(hdr.size);
  u8 *p = (u8 *)&hdr + sizeof(hdr);
  u8 *end = (u8 *)&hdr + size;

  while (p < end) {
    if (*(Offset *)p == 0)
      break;
    p += sizeof(Offset);

    u8 type = *p++;
    const char *name = (char *)p;
    i64 len = strlen(name);
    p += len + 1;
    cu->names.emplace_back(
      hash_string(std::string_view(name, len)), type, name);
  }

  return size;
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
static void read_pubnames(Context<E> &ctx, std::vector<Compunit> &cus,
                          ObjectFile<E> &file) {
  InputSection<E> *sections[] = {file.debug_pubnames, file.debug_pubtypes};
  for (InputSection<E> *isec : sections) {
    if (!isec)
      continue;

    isec->uncompress(ctx);
    std::string_view contents = isec->get_contents();
    if (contents.empty())
      continue;

    u8 *p = (u8 *)contents.data();
    u8 *end = p + contents.size();

    while (p < end) {
      if (*(U32<E> *)p == 0xffff'ffff)
        p += read_pubnames_cu(ctx, *(PubnamesHdr64<E> *)p, cus, file);
      else
        p += read_pubnames_cu(ctx, *(PubnamesHdr32<E> *)p, cus, file);
    }
  }
}

template <typename E>
static std::vector<Compunit> read_compunits(Context<E> &ctx) {
  std::vector<std::vector<Compunit>> file_cus(ctx.objs.size());

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 file_idx) {
    ObjectFile<E> &file = *ctx.objs[file_idx];
    if (!file.debug_info)
      return;

    file.debug_info->uncompress(ctx);
    std::string_view contents = file.debug_info->get_contents();
    std::vector<Compunit> &cus = file_cus[file_idx];
    u8 *begin = (u8 *)contents.data();
    u8 *end = begin + contents.size();

    for (u8 *p = begin; p < end;) {
      DwarfKind kind = get_dwarf_kind(ctx, p);
      i64 size;
      if (kind == DWARF2_32 || kind == DWARF5_32)
        size = ((CuHdrDwarf2_32<E> *)p)->size + 4;
      else
        size = ((CuHdrDwarf2_64<E> *)p)->size + 12;

      cus.push_back(Compunit{kind, p - begin, size, (i32)file_idx});
      p += size;
    }

    read_pubnames(ctx, cus, file);
    for (Compunit &cu : cus)
      dedup_names(cu);
  });

  i64 size = 0;
  for (std::vector<Compunit> &cus : file_cus)
    size += cus.size();

  std::vector<Compunit> cus;
  cus.reserve(size);
  for (std::vector<Compunit> &vec : file_cus)
    for (Compunit &cu : vec)
      cus.push_back(std::move(cu));
  return cus;
}

template <typename E>
static void read_address_ranges(Context<E> &ctx, std::vector<Compunit> &cus) {
  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    switch (cu.kind) {
    case DWARF2_32:
      cu.ranges = read_address_ranges<E, CuHdrDwarf2_32<E>>(ctx, cu);
      break;
    case DWARF5_32:
      cu.ranges = read_address_ranges<E, CuHdrDwarf5_32<E>>(ctx, cu);
      break;
    case DWARF2_64:
      cu.ranges = read_address_ranges<E, CuHdrDwarf2_64<E>>(ctx, cu);
      break;
    case DWARF5_64:
      cu.ranges = read_address_ranges<E, CuHdrDwarf5_64<E>>(ctx, cu);
      break;
    }

    std::erase_if(cu.ranges, [](std::pair<u64, u64> range) {
      return range.first == 0 || range.first == range.second;
    });
  });
}

template <typename E>
static std::span<u8> get_buffer(Context<E> &ctx, Chunk<E> *chunk) {
  if (chunk->is_compressed) {
    CompressedSection<E> &sec = *(CompressedSection<E> *)chunk;
    return {sec.uncompressed_data.get(), (size_t)sec.chdr.ch_size};
  }
  return {ctx.buf + chunk->shdr.sh_offset, (size_t)chunk->shdr.sh_size};
}

// Read compilation units and their public names, deduplicate and intern the
// names, and determine the constant-pool layout. This stage needs only input
// sections, so it can run before output-section offsets are assigned.
template <typename E>
void read_gdb_index_inputs(Context<E> &ctx) {
  Timer t(ctx, "read_gdb_index_inputs");

  ctx.gdb_index_data = std::make_shared<GdbIndexData>();

  GdbIndexData &data = *ctx.gdb_index_data;
  data.cus = read_compunits(ctx);

  HyperLogLog estimator;
  tbb::parallel_for_each(data.cus, [&](Compunit &cu) {
    HyperLogLog::Sketch &sketch = estimator.local();

    // NameType keeps 56 hash bits. Spread them across a 64-bit word because
    // HyperLogLog uses the number of leading zero bits.
    for (NameRecord &record : cu.names)
      sketch.insert(record.nametype.get_hash() * 0x9e37'79b9'7f4a'7c15);
  });

  data.map = std::make_unique<GdbNameMap>(estimator.get_cardinality() * 3 / 2);

  tbb::parallel_for_each(data.cus, [&](Compunit &cu) {
    for (NameRecord &record : cu.names) {
      NameType &nt = record.nametype;
      const char *name = nt.name;
      u64 hash = nt.get_hash();
      u8 type = nt.get_type();
      MapValue *ent =
        data.map->insert_cstr(name, hash, {}, initialize_gdb_name).first;
      record.indexed = {ent, ent->count++ + 1, type};
    }
  });

  data.entries = data.map->get_sorted_entries_all();
  data.ht_size = bit_ceil(data.entries.size() * 5 / 4 + 1);

  // The map may contain millions of names. Assign their type and string
  // ranges with a parallel prefix sum.
  auto scan = [&](const tbb::blocked_range<i64> &range, PoolSize size,
                  bool is_final) {
    for (i64 i = range.begin(); i < range.end(); i++) {
      GdbNameMap::Entry *ent = data.entries[i];
      if (is_final) {
        ent->value.type_offset = size.type_bytes;
        ent->value.name_offset = size.name_bytes;
      }
      size.type_bytes += ent->value.count * 4 + 4;
      size.name_bytes += ent->keylen + 1;
    }
    return size;
  };

  data.pool_size = tbb::parallel_scan(
    tbb::blocked_range<i64>(0, data.entries.size()), PoolSize{}, scan,
    [](PoolSize a, PoolSize b) -> PoolSize {
      return {a.type_bytes + b.type_bytes, a.name_bytes + b.name_bytes};
    });

  tbb::parallel_for_each(data.entries, [&](GdbNameMap::Entry *ent) {
    ent->value.name_offset += data.pool_size.type_bytes;
  });
}

// Build the name lookup table and the constant pool for .gdb_index. They
// depend on compilation-unit order but not on relocated address ranges, so
// they can be built in the background once .debug_info offsets are fixed.
template <typename E>
void build_gdb_index_tables(Context<E> &ctx) {
  Timer t(ctx, "build_gdb_index_tables");

  GdbIndexData &data = *ctx.gdb_index_data;
  if (data.cus.empty() || data.tables)
    return;

  std::vector<Compunit> &cus = data.cus;
  std::vector<GdbNameMap::Entry *> &entries = data.entries;
  using Entry = GdbNameMap::Entry;

  // CU offsets are relative to their input .debug_info sections. Convert them
  // to output-section offsets and sort the CUs in output order. A CU's position
  // in this vector is the CU number stored in type vectors and address ranges.
  for (Compunit &cu : cus)
    cu.offset += ctx.objs[cu.file_idx]->debug_info->offset;
  ranges::sort(cus, {}, &Compunit::offset);

  // `tables` contains the name hash table followed by the constant pool. The
  // constant pool contains all type vectors followed by all name strings.
  i64 symtab_size = data.ht_size * 8;
  i64 pool_size = data.pool_size.type_bytes + data.pool_size.name_bytes;
  data.tables = std::unique_ptr<u8[]>(new u8[symtab_size + pool_size]);

  // Each occupied hash-table slot contains the constant-pool offsets of a name
  // and its type vector.
  ul32 *ht = (ul32 *)data.tables.get();
  memset(ht, 0, symtab_size);

  u32 mask = data.ht_size - 1;
  for (Entry *ent : entries) {
    u32 hash = ent->value.gdb_hash;

    // This probing sequence is part of the .gdb_index format. The table size
    // is a power of two, so an odd step visits every slot.
    u32 step = ((hash * 17) & mask) | 1;
    u32 i = hash & mask;

    while (ht[i * 2] || ht[i * 2 + 1])
      i = (i + step) & mask;

    ht[i * 2] = ent->value.name_offset;
    ht[i * 2 + 1] = ent->value.type_offset;
  }

  u8 *base = data.tables.get() + symtab_size;

  // Each occurrence of a name contributes one value to its type vector. Each
  // occurrence was assigned a distinct slot while the names were interned, so
  // the vectors can be filled in parallel. The high byte is the name's type
  // and the low 24 bits are the CU number.
  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    i64 i = &cu - cus.data();
    for (NameRecord &record : cu.names) {
      IndexedName &name = record.indexed;
      MapValue *ent = name.entry;
      ul32 *p = (ul32 *)(base + ent->type_offset);
      p[name.type_idx] = (name.type << 24) | i;
    }
  });

  // Prefix each type vector with its length and sort it for deterministic
  // output. Store the NUL-terminated name at its assigned string-pool offset.
  tbb::enumerable_thread_specific<std::vector<ul32>> scratch;
  tbb::parallel_for_each(entries, [&](Entry *ent) {
    ul32 *p = (ul32 *)(base + ent->value.type_offset);
    p[0] = ent->value.count;
    std::span<ul32> values(p + 1, ent->value.count);

    if (values.size() < 256)
      ranges::sort(values);
    else
      radix_sort(values, scratch.local());

    u8 *dst = base + ent->value.name_offset;
    memcpy(dst, ent->key, ent->keylen);
    dst[ent->keylen] = '\0';
  });

  // The serialized tables contain everything needed from names and the map.
  // Release their storage here so reclamation remains part of this background
  // phase rather than delaying the final output path.
  tbb::parallel_for_each(cus, [](Compunit &cu) {
    std::vector<NameRecord>().swap(cu.names);
  });
  std::vector<GdbNameMap::Entry *>().swap(data.entries);
  data.map.reset();
}

// Read relocated address ranges and serialize the index prepared above.
template <typename E>
void write_gdb_index(Context<E> &ctx) {
  Timer t(ctx, "write_gdb_index");

  std::shared_ptr<GdbIndexData> owner = std::move(ctx.gdb_index_data);
  GdbIndexData &data = *owner;

  if (data.cus.empty())
    return;

  // Find debug info sections
  for (Chunk<E> *chunk : ctx.chunks) {
    std::string_view name = chunk->name;
    if (name == ".debug_info")
      ctx.debug_info = get_buffer(ctx, chunk);
    if (name == ".debug_abbrev")
      ctx.debug_abbrev = get_buffer(ctx, chunk);
    if (name == ".debug_ranges")
      ctx.debug_ranges = get_buffer(ctx, chunk);
    if (name == ".debug_addr")
      ctx.debug_addr = get_buffer(ctx, chunk);
    if (name == ".debug_rnglists")
      ctx.debug_rnglists = get_buffer(ctx, chunk);
  }

  std::vector<Compunit> &cus = data.cus;

  read_address_ranges(ctx, cus);

  // Compute sizes of each component
  SectionHeader hdr;
  hdr.cu_list_offset = sizeof(hdr);
  hdr.cu_types_offset = hdr.cu_list_offset + cus.size() * 16;
  hdr.ranges_offset = hdr.cu_types_offset;

  hdr.symtab_offset = hdr.ranges_offset;
  for (Compunit &cu : cus)
    hdr.symtab_offset += cu.ranges.size() * 20;

  i64 ht_size = data.ht_size;
  hdr.const_pool_offset = hdr.symtab_offset + ht_size * 8;

  i64 bufsize = hdr.const_pool_offset + data.pool_size.type_bytes +
                data.pool_size.name_bytes;

  u8 *buf = ctx.output_file->extend(ctx, bufsize);

  // Write a section header
  memcpy(buf, &hdr, sizeof(hdr));

  // Write a CU list
  u8 *p = buf + sizeof(hdr);

  for (Compunit &cu : cus) {
    *(ul64 *)p = cu.offset;
    *(ul64 *)(p + 8) = cu.size;
    p += 16;
  }

  // Write address areas
  std::vector<i64> range_offsets(cus.size());
  for (i64 i = 1; i < cus.size(); i++)
    range_offsets[i] = range_offsets[i - 1] + cus[i - 1].ranges.size() * 20;

  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    i64 i = &cu - cus.data();
    u8 *p = buf + hdr.ranges_offset + range_offsets[i];
    for (std::pair<u64, u64> range : cu.ranges) {
      *(ul64 *)p = range.first;
      *(ul64 *)(p + 8) = range.second;
      *(ul32 *)(p + 16) = i;
      p += 20;
    }
  });

  i64 tables_size = ht_size * 8 + data.pool_size.type_bytes +
                    data.pool_size.name_bytes;
  parallel_memcpy(buf + hdr.symtab_offset, data.tables.get(), tables_size);

  // Update the section size and rewrite the section header
  if (ctx.shdr) {
    ctx.gdb_index->shdr.sh_size = bufsize;
    ctx.shdr->copy_buf(ctx);
  }
}

using E = MOLD_TARGET;

template void read_gdb_index_inputs(Context<E> &);
template void build_gdb_index_tables(Context<E> &);
template void write_gdb_index(Context<E> &);

} // namespace mold
