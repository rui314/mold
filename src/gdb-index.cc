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

struct NameType {
  auto operator<=>(const NameType &) const = default;
  u64 hash;
  u8 type;
  std::string_view name;
};

struct MapValue {
  u32 gdb_hash = 0;
  Atomic<u32> count;
  u32 name_offset = 0;
  u32 type_offset = 0;
};

struct Compunit {
  DwarfKind kind;
  i64 offset;
  i64 size;
  std::vector<std::pair<u64, u64>> ranges;
  std::vector<NameType> nametypes;
  std::vector<MapValue *> entries;
};

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
      i64 num_offsets = *(U32<E> *)(base - 4);
      Offset *offsets = (Offset *)base;

      for (i64 i = 0; i < num_offsets; i++)
        read_rnglist_range<E>(vec, base + offsets[i], addrx, low_pc.value);
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

  auto get_cu = [&](i64 offset) {
    for (i64 i = 0; i < cus.size(); i++)
      if (cus[i].offset == offset)
        return &cus[i];
    Fatal(ctx) << file << ": corrupted debug_info_offset";
  };

  Compunit *cu = get_cu(file.debug_info->offset + hdr.debug_info_offset);
  i64 size = hdr.size + offsetof(PubnamesHdr, size) + sizeof(hdr.size);
  u8 *p = (u8 *)&hdr + sizeof(hdr);
  u8 *end = (u8 *)&hdr + size;

  while (p < end) {
    if (*(Offset *)p == 0)
      break;
    p += sizeof(Offset);

    u8 type = *p++;
    std::string_view name = (char *)p;
    p += name.size() + 1;
    cu->nametypes.push_back(NameType{hash_string(name), type, name});
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
  for (InputSection<E> *isec : { file.debug_pubnames, file.debug_pubtypes }) {
    if (!isec)
      continue;

    isec->uncompress(ctx);
    if (isec->contents.empty())
      continue;

    u8 *p = (u8*)&isec->contents[0];
    u8 *end = p + isec->contents.size();

    while (p < end) {
      if (*(U32<E> *)p == 0xffff'ffff)
        p += read_pubnames_cu(ctx, *(PubnamesHdr64<E> *)p, cus, file);
      else
        p += read_pubnames_cu(ctx, *(PubnamesHdr32<E> *)p, cus, file);
    }
  };
}

template <typename E>
static std::vector<Compunit> read_compunits(Context<E> &ctx) {
  std::vector<Compunit> cus;

  // Read compunits from the output .debug_info section.
  u8 *begin = &ctx.debug_info[0];
  u8 *end = begin + ctx.debug_info.size();

  for (u8 *p = begin; p < end;) {
    DwarfKind kind = get_dwarf_kind(ctx, p);
    i64 size;
    if (kind == DWARF2_32 || kind == DWARF5_32)
      size = ((CuHdrDwarf2_32<E> *)p)->size + 4;
    else
      size = ((CuHdrDwarf2_64<E> *)p)->size + 12;

    cus.push_back(Compunit{kind, p - begin, size});
    p += size;
  }

  // Read address ranges for each compunit.
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

    // Remove empty ranges
    std::erase_if(cu.ranges, [](std::pair<u64, u64> p) {
      return p.first == 0 || p.first == p.second;
    });
  });

  // Read symbols from .debug_gnu_pubnames and .debug_gnu_pubtypes.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    read_pubnames(ctx, cus, *file);
  });

  // Uniquify elements because GCC 11 seems to emit one record for each
  // comdat group which results in having a lot of duplicate records.
  tbb::parallel_for_each(cus, [](Compunit &cu) {
    ranges::stable_sort(cu.nametypes);
    remove_duplicates(cu.nametypes);
  });

  return cus;
}

template <typename E>
std::span<u8> get_buffer(Context<E> &ctx, Chunk<E> *chunk) {
  if (chunk->is_compressed) {
    CompressedSection<E> &sec = *(CompressedSection<E> *)chunk;
    return {sec.uncompressed_data.get(), (size_t)sec.chdr.ch_size};
  }
  return {ctx.buf + chunk->shdr.sh_offset, (size_t)chunk->shdr.sh_size};
}

template <typename E>
void write_gdb_index(Context<E> &ctx) {
  Timer t(ctx, "write_gdb_index");

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

  if (ctx.debug_info.empty())
    return;

  // Read debug info
  std::vector<Compunit> cus = read_compunits(ctx);

  // Uniquify symbols
  HyperLogLog estimator;

  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    HyperLogLog e;
    for (NameType &nt : cu.nametypes)
      e.insert(nt.hash);
    estimator.merge(e);
  });

  ConcurrentMap<MapValue> map(estimator.get_cardinality() * 3 / 2);

  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    cu.entries.reserve(cu.nametypes.size());
    for (NameType &nt : cu.nametypes) {
      MapValue *ent;
      bool inserted;
      std::tie(ent, inserted) = map.insert(nt.name, nt.hash,
                                           MapValue{gdb_hash(nt.name)});
      ent->count++;
      cu.entries.push_back(ent);
    }
  });

  // Sort symbols for build reproducibility
  using Entry = typename decltype(map)::Entry;
  std::vector<Entry *> entries = map.get_sorted_entries_all();

  // Compute sizes of each components
  SectionHeader hdr;
  hdr.cu_list_offset = sizeof(hdr);
  hdr.cu_types_offset = hdr.cu_list_offset + cus.size() * 16;
  hdr.ranges_offset = hdr.cu_types_offset;

  hdr.symtab_offset = hdr.ranges_offset;
  for (Compunit &cu : cus)
    hdr.symtab_offset += cu.ranges.size() * 20;

  i64 ht_size = bit_ceil(entries.size() * 5 / 4 + 1);
  hdr.const_pool_offset = hdr.symtab_offset + ht_size * 8;

  i64 offset = 0;
  for (Entry *ent : entries) {
    ent->value.type_offset = offset;
    offset += ent->value.count * 4 + 4;
  }

  for (Entry *ent : entries) {
    ent->value.name_offset = offset;
    offset += ent->keylen + 1;
  }

  i64 bufsize = hdr.const_pool_offset + offset;

  // Allocate an output buffer. We use malloc instead of vector to
  // avoid zero-initializing the entire buffer.
  ctx.output_file->buf2 = (u8 *)malloc(bufsize);
  ctx.output_file->buf2_size = bufsize;
  u8 *buf = ctx.output_file->buf2;

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

  // Write a symbol table
  u32 mask = ht_size - 1;
  ul32 *ht = (ul32 *)(buf + hdr.symtab_offset);
  memset(ht, 0, ht_size * 8);

  for (Entry *ent : entries) {
    u32 hash = ent->value.gdb_hash;
    u32 step = ((hash * 17) & mask) | 1;
    u32 j = hash & mask;

    while (ht[j * 2] || ht[j * 2 + 1])
      j = (j + step) & mask;

    ht[j * 2] = ent->value.name_offset;
    ht[j * 2 + 1] = ent->value.type_offset;
  }

  // Write types. Use MapValue::count as an atomic slot counter.
  u8 *base = buf + hdr.const_pool_offset;

  for (Entry *ent : entries)
    ent->value.count = 0;

  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    i64 i = &cu - cus.data();
    for (i64 j = 0; j < cu.nametypes.size(); j++) {
      MapValue *ent = cu.entries[j];
      ul32 *p = (ul32 *)(base + ent->type_offset);
      i64 idx = ++ent->count;
      p[idx] = (cu.nametypes[j].type << 24) | i;
    }
  });

  // Write the final counts into the buffer.
  for (Entry *ent : entries)
    *(ul32 *)(base + ent->value.type_offset) = ent->value.count;

  // Write names
  tbb::parallel_for_each(entries, [&](Entry *ent) {
    u8 *dst = buf + hdr.const_pool_offset + ent->value.name_offset;
    memcpy(dst, ent->key, ent->keylen);
    dst[ent->keylen] = '\0';
  });

  // Update the section size and rewrite the section header
  if (ctx.shdr) {
    ctx.gdb_index->shdr.sh_size = bufsize;
    ctx.shdr->copy_buf(ctx);
  }
}

using E = MOLD_TARGET;

template void write_gdb_index(Context<E> &);

} // namespace mold
