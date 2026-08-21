// This file contains code to read DWARF debug info to create .gdb_index.
//
// .gdb_index is an optional section to speed up GNU debugger. It contains
// two maps: 1) a map from function/variable/type names to compunits, and
// 2) a map from function address ranges to compunits. gdb uses these
// maps to quickly find a compunit given a name or an instruction pointer.
//
// (Terminology: a DWARF "unit" is a self-contained sequence of debug
// entries. A compilation unit (CU) describes a source file and owns
// address ranges. A type unit (TU) describes one shareable type and is
// identified by a signature. DWARF 4 puts TUs in .debug_types; DWARF 5
// puts DW_UT_type units in .debug_info. This file reads CUs from DWARF 2
// to 5 and TUs from DWARF 5.)
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

// Normalized view of a DWARF unit header. DWARF32 and DWARF64 describe the
// width of section offsets, not the ELF class or target address size.
struct DwarfUnitHeader {
  i64 size;
  i64 header_size;
  u64 abbrev_offset;
  u64 type_die_offset;
  u64 signature;
  u8 version;
  u8 unit_type;
  u8 address_size;
  u8 offset_size;
};

// Header of one GNU pubnames or pubtypes set in DWARF32 format.
template <typename E>
struct PubnamesHdr32 {
  U32<E> size;
  U16<E> version;
  U32<E> debug_info_offset;
  U32<E> debug_info_size;
};

// Header of one GNU pubnames or pubtypes set in DWARF64 format.
template <typename E>
struct PubnamesHdr64 {
  U32<E> magic;
  U64<E> size;
  U16<E> version;
  U64<E> debug_info_offset;
  U64<E> debug_info_size;
};

// The version 7 .gdb_index header. All table offsets are section-relative.
struct SectionHeaderV7 {
  ul32 version;
  ul32 cu_list_offset;
  ul32 cu_types_offset;
  ul32 ranges_offset;
  ul32 symtab_offset;
  ul32 const_pool_offset;
};

// Version 9 inserts a shortcut table between the symbol table and the
// constant pool. We currently emit an empty shortcut table.
struct SectionHeaderV9 {
  ul32 version;
  ul32 cu_list_offset;
  ul32 cu_types_offset;
  ul32 ranges_offset;
  ul32 symtab_offset;
  ul32 shortcut_offset;
  ul32 const_pool_offset;
};

// Build-time state for one unique name in the .gdb_index symbol table.
struct MapValue {
  u32 gdb_hash = 0;
  Atomic<u32> count;
  u32 name_offset = 0;
  u32 type_vector_offset = 0;
};

// A public name and its GNU kind before the name is interned in GdbNameMap.
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

// The interned name's map entry and reserved slot in its type vector.
struct IndexedName {
  MapValue *entry;
  u32 type_vector_idx;
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

// CU metadata carried from input parsing through final index serialization.
// CUs own address ranges; both CUs and TUs below may own public names.
struct Compunit {
  // Initially relative to the input contribution selected by file_idx/shndx;
  // rebased to the output .debug_info section in build_gdb_index_tables.
  i64 offset;
  i64 size;
  i32 file_idx;
  i32 shndx;
  std::vector<std::pair<u64, u64>> ranges;
  std::vector<NameRecord> names;
};

// TU metadata used for the .gdb_index type-unit list. TUs have no address
// ranges because executable code is attributed to compilation units.
struct Typeunit {
  // `offset` is rebased like Compunit::offset. `type_die_offset` remains
  // relative to the unit, as required by the .gdb_index type-unit table.
  i64 offset;
  u64 type_die_offset;
  u64 signature;
  i32 file_idx;
  i32 shndx;
  std::vector<NameRecord> names;
};

using GdbNameMap = ConcurrentMap<MapValue>;

// Byte counts accumulated by the parallel constant-pool layout scan.
struct PoolSize {
  i64 type_bytes = 0;
  i64 name_bytes = 0;
};

// State shared by the input reader, table builder and final serialization pass.
struct GdbIndexData {
  std::vector<Compunit> cus;
  std::vector<Typeunit> tus;
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
static void dedup_names(auto &cu) {
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

template <typename T>
static u64 read_uint(u8 **p) {
  u64 val = *(T *)*p;
  *p += sizeof(T);
  return val;
}

template <typename E>
static u64 read_offset(u8 **p, u8 offset_size) {
  if (offset_size == 4)
    return read_uint<U32<E>>(p);
  assert(offset_size == 8);
  return read_uint<U64<E>>(p);
}

template <typename E>
static DwarfUnitHeader parse_unit_header(Context<E> &ctx, u8 *start) {
  // The first word is either a DWARF32 unit length or DWARF64's reserved
  // marker. unit_length excludes its own encoding: four bytes in DWARF32, or
  // the four-byte marker plus eight-byte length in DWARF64.
  u8 *p = start;
  i64 unit_length = read_uint<U32<E>>(&p);
  i64 initial_length_size = 4;
  u8 offset_size = 4;

  if (unit_length == UINT32_MAX) {
    unit_length = read_uint<U64<E>>(&p);
    initial_length_size = 12;
    offset_size = 8;
  }

  u16 version = read_uint<U16<E>>(&p);

  if (version > 5)
    Fatal(ctx) << "--gdb-index: DWARF version " << version << " is not supported";

  DwarfUnitHeader unit = {};
  unit.size = unit_length + initial_length_size;
  unit.version = version;
  unit.unit_type = DW_UT_compile;
  unit.offset_size = offset_size;

  if (version < 5) {
    unit.abbrev_offset = read_offset<E>(&p, offset_size);
    unit.address_size = *p++;
  } else {
    unit.unit_type = *p++;
    unit.address_size = *p++;
    unit.abbrev_offset = read_offset<E>(&p, offset_size);

    switch (unit.unit_type) {
    case DW_UT_skeleton:
    case DW_UT_split_compile:
      p += 8; // dwo_id
      break;
    case DW_UT_type:
    case DW_UT_split_type:
      unit.signature = read_uint<U64<E>>(&p);
      unit.type_die_offset = read_offset<E>(&p, offset_size);
      break;
    }
  }

  unit.header_size = p - start;
  return unit;
}

template <typename E>
u8 *find_cu_abbrev(Context<E> &ctx, u8 **p, const DwarfUnitHeader &hdr) {
  if (hdr.address_size != sizeof(Word<E>))
    Fatal(ctx) << "--gdb-index: unsupported address size " << hdr.address_size;

  i64 abbrev_code = read_uleb(p);

  // The first DIE refers to an abbreviation by its ULEB128 code. Walk the
  // unit's abbreviation table to find the attribute forms for that DIE.
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

// .debug_info contains variable-length fields. `offset_size` is four or eight
// bytes according to the DWARF32/DWARF64 format; Word<E> is instead the
// target's address width. This function advances over one scalar value.
template <typename E>
u64 read_scalar(Context<E> &ctx, u8 **p, u64 form, u8 offset_size) {
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
  case DW_FORM_ref2:
    return read_uint<U16<E>>(p);
  case DW_FORM_strx3:
  case DW_FORM_addrx3:
    return read_uint<U24<E>>(p);
  case DW_FORM_data4:
  case DW_FORM_strx4:
  case DW_FORM_addrx4:
  case DW_FORM_ref4:
    return read_uint<U32<E>>(p);
  case DW_FORM_data8:
  case DW_FORM_ref8:
    return read_uint<U64<E>>(p);
  case DW_FORM_strp:
  case DW_FORM_sec_offset:
  case DW_FORM_line_strp:
    return read_offset<E>(p, offset_size);
  case DW_FORM_addr:
  case DW_FORM_ref_addr:
    return read_uint<Word<E>>(p);
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
template <typename E>
static std::vector<std::pair<u64, u64>>
read_address_ranges(Context<E> &ctx, const Compunit &cu) {
  // Read .debug_info to find the record at a given offset.
  u8 *start = &ctx.debug_info[0] + cu.offset;
  DwarfUnitHeader hdr = parse_unit_header(ctx, start);
  u8 *p = start + hdr.header_size;

  u8 *abbrev = find_cu_abbrev(ctx, &p, hdr);

  // Now, read debug info records.
  struct Record {
    u64 form = 0;
    u64 value = 0;
  };

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

    u64 val = read_scalar<E>(ctx, &p, form, hdr.offset_size);

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

  // Before DWARF 5, DW_AT_ranges is a byte offset into .debug_ranges. In
  // DWARF 5 it is either a direct .debug_rnglists offset (sec_offset) or an
  // index into the offset table rooted at DW_AT_rnglists_base (rnglistx).
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
      u8 *entry = base + ranges.value * hdr.offset_size;
      u64 offset = read_offset<E>(&entry, hdr.offset_size);
      read_rnglist_range<E>(vec, base + offset, addrx, low_pc.value);
    }
    return vec;
  }

  // For one contiguous range, high_pc is either an address or an unsigned
  // length, as indicated by its form. DWARF 5 may store either endpoint as an
  // index into the address table rooted at DW_AT_addr_base.
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

// Returns the .debug_info contribution a pubnames set refers to. The set
// header's debug_info_offset field is relocated against the particular
// contribution containing the unit. This matters for DWARF 5 because type
// units live in separate COMDAT contributions with the same section name.
template <typename E, typename PubnamesHdr>
static std::pair<InputSection<E> *, i64>
get_pubnames_target(Context<E> &ctx, InputSection<E> &isec,
                    const PubnamesHdr &hdr, ObjectFile<E> &file) {
  i64 off = (u8 *)&hdr + offsetof(PubnamesHdr, debug_info_offset) - isec.contents;
  std::span<ElfRel<E>> rels = isec.get_rels(ctx);

  auto it = ranges::lower_bound(rels, off, std::less(), &ElfRel<E>::r_offset);
  if (it == rels.end() || it->r_offset != off)
    return {nullptr, 0};

  const ElfSym<E> &esym = file.elf_syms[it->r_sym];
  return {file.get_section(esym), esym.st_value + get_addend(isec, *it)};
}

// Units are appended in input section and contribution offset order, so both
// the CU and TU vectors are sorted by this key.
static std::vector<NameRecord> *find_unit_names(auto &units, i32 shndx,
                                                i64 offset) {
  auto key = std::pair(shndx, offset);
  auto it = ranges::lower_bound(units, key, {}, [](const auto &unit) {
    return std::pair(unit.shndx, unit.offset);
  });

  if (it == units.end() || std::pair(it->shndx, it->offset) != key)
    return nullptr;
  return &it->names;
}

template <typename E, typename PubnamesHdr>
static i64 read_pubnames_cu(Context<E> &ctx, const PubnamesHdr &hdr,
                            std::vector<Compunit> &cus, std::vector<Typeunit> &tus,
                            ObjectFile<E> &file, InputSection<E> &isec) {
  auto [target, offset] = get_pubnames_target(ctx, isec, hdr, file);
  std::vector<NameRecord> *names = nullptr;

  if (target) {
    names = find_unit_names(cus, target->shndx, offset);
    if (!names)
      names = find_unit_names(tus, target->shndx, offset);
  }

  if (!names)
    Fatal(ctx) << file << ": corrupted debug_info_offset";

  i64 size = hdr.size + offsetof(PubnamesHdr, size) + sizeof(hdr.size);
  u8 *p = (u8 *)&hdr + sizeof(hdr);
  u8 *end = (u8 *)&hdr + size;

  while (p < end) {
    using T = decltype(hdr.size);
    if (*(T *)p == 0)
      break;
    p += sizeof(T);

    u8 type = *p++;
    const char *name = (char *)p;
    i64 len = strlen(name);
    p += len + 1;
    names->emplace_back(hash_string(std::string_view(name, len)), type, name);
  }

  return size;
}

// Parses .debug_gnu_pubnames and .debug_gnu_pubtypes. Each set starts with a
// DWARF32 or DWARF64 header identifying one debug unit, followed by
// (DIE offset, 1-byte kind, NUL-terminated name) tuples. The GNU kind byte lets
// GDB distinguish functions, variables and types without reading their DIEs.
template <typename E>
static void read_pubnames(Context<E> &ctx, std::vector<Compunit> &cus,
                          std::vector<Typeunit> &tus, ObjectFile<E> &file) {
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
        p += read_pubnames_cu(ctx, *(PubnamesHdr64<E> *)p, cus, tus, file,
                              *isec);
      else
        p += read_pubnames_cu(ctx, *(PubnamesHdr32<E> *)p, cus, tus, file,
                              *isec);
    }
  }
}

// CUs and TUs collected from one object file or from the entire link.
struct DebugUnits {
  std::vector<Compunit> cus;
  std::vector<Typeunit> tus;
};

// Read every unit in one input .debug_info contribution. Keeping this separate
// leaves read_debug_units responsible only for object-level orchestration.
template <typename E>
static void read_debug_info_section(Context<E> &ctx, DebugUnits &units,
                                    InputSection<E> &isec, i32 file_idx) {
  isec.uncompress(ctx);
  std::string_view contents = isec.get_contents();
  u8 *begin = (u8 *)contents.data();
  u8 *end = begin + contents.size();

  for (u8 *p = begin; p < end;) {
    DwarfUnitHeader unit = parse_unit_header(ctx, p);

    switch (unit.unit_type) {
    case DW_UT_compile:
    case DW_UT_partial:
    case DW_UT_skeleton:
    case DW_UT_split_compile:
      units.cus.push_back(
          Compunit{p - begin, unit.size, file_idx, isec.shndx});
      break;
    case DW_UT_type:
    case DW_UT_split_type:
      units.tus.push_back(Typeunit{p - begin, unit.type_die_offset,
                                   unit.signature, file_idx, isec.shndx});
      break;
    default:
      Fatal(ctx) << "--gdb-index: unknown unit type: 0x" << std::hex
                 << (u32)unit.unit_type;
    }

    p += unit.size;
  }
}

template <typename E> static DebugUnits read_debug_units(Context<E> &ctx) {
  std::vector<DebugUnits> file_units(ctx.objs.size());

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 file_idx) {
    ObjectFile<E> &file = *ctx.objs[file_idx];
    DebugUnits &units = file_units[file_idx];

    // Skip type units that lost COMDAT group selection.
    for (InputSection<E> *isec : file.debug_info_sections)
      if (isec->is_alive())
        read_debug_info_section(ctx, units, *isec, (i32)file_idx);

    read_pubnames(ctx, units.cus, units.tus, file);
    for (Compunit &cu : units.cus)
      dedup_names(cu);
    for (Typeunit &tu : units.tus)
      dedup_names(tu);
  });

  i64 num_cus = 0;
  i64 num_tus = 0;
  for (DebugUnits &units : file_units) {
    num_cus += units.cus.size();
    num_tus += units.tus.size();
  }

  DebugUnits result;
  result.cus.reserve(num_cus);
  result.tus.reserve(num_tus);

  for (DebugUnits &units : file_units) {
    for (Compunit &cu : units.cus)
      result.cus.push_back(std::move(cu));
    for (Typeunit &tu : units.tus)
      result.tus.push_back(std::move(tu));
  }
  return result;
}

template <typename E>
static void read_address_ranges(Context<E> &ctx, std::vector<Compunit> &cus) {
  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    cu.ranges = read_address_ranges(ctx, cu);

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
  DebugUnits units = read_debug_units(ctx);
  data.cus = std::move(units.cus);
  data.tus = std::move(units.tus);

  HyperLogLog estimator;
  auto estimate_names = [&](auto &units) {
    tbb::parallel_for_each(units, [&](auto &unit) {
      HyperLogLog::Sketch &sketch = estimator.local();

      // NameType keeps 56 hash bits. Spread them across a 64-bit word because
      // HyperLogLog uses the number of leading zero bits.
      for (NameRecord &record : unit.names)
        sketch.insert(record.nametype.get_hash() * 0x9e37'79b9'7f4a'7c15);
    });
  };
  estimate_names(data.cus);
  estimate_names(data.tus);

  data.map = std::make_unique<GdbNameMap>(estimator.get_cardinality() * 3 / 2);

  auto intern_names = [&](auto &units) {
    tbb::parallel_for_each(units, [&](auto &unit) {
      for (NameRecord &record : unit.names) {
        NameType &nt = record.nametype;
        const char *name = nt.name;
        u64 hash = nt.get_hash();
        u8 type = nt.get_type();
        MapValue *ent = data.map->insert_cstr(name, hash, {}, initialize_gdb_name).first;
        record.indexed = {ent, ent->count++ + 1, type};
      }
    });
  };

  intern_names(data.cus);
  intern_names(data.tus);

  data.entries = data.map->get_sorted_entries_all();
  data.ht_size = bit_ceil(data.entries.size() * 5 / 4 + 1);

  // The map may contain millions of names. Assign their type and string
  // ranges with a parallel prefix sum.
  auto scan = [&](const tbb::blocked_range<i64> &range, PoolSize size,
                  bool is_final) {
    for (i64 i = range.begin(); i < range.end(); i++) {
      GdbNameMap::Entry *ent = data.entries[i];
      if (is_final) {
        ent->value.type_vector_offset = size.type_bytes;
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
  if ((data.cus.empty() && data.tus.empty()) || data.tables)
    return;

  std::vector<Compunit> &cus = data.cus;
  std::vector<Typeunit> &tus = data.tus;
  std::vector<GdbNameMap::Entry *> &entries = data.entries;
  using Entry = GdbNameMap::Entry;

  // Unit offsets are relative to their input .debug_info contributions.
  // Convert them to output-section offsets and sort each .gdb_index list in
  // output order. The format's unit-number namespace consists of every CU-list
  // entry followed by every TU-list entry, even when CUs and TUs are
  // interleaved in .debug_info. Address-area records refer only to the CU list.
  auto get_output_offset = [&](const auto &unit) {
    InputSection<E> *isec = ctx.objs[unit.file_idx]->sections[unit.shndx];
    return isec->offset;
  };

  for (Compunit &cu : cus)
    cu.offset += get_output_offset(cu);
  for (Typeunit &tu : tus)
    tu.offset += get_output_offset(tu);
  ranges::sort(cus, {}, &Compunit::offset);
  ranges::sort(tus, {}, &Typeunit::offset);

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
    ht[i * 2 + 1] = ent->value.type_vector_offset;
  }

  u8 *base = data.tables.get() + symtab_size;

  // Each occurrence of a name contributes one value to its type vector. Each
  // occurrence was assigned a distinct slot while the names were interned, so
  // the vectors can be filled in parallel. The high byte is the name's type
  // and the low 24 bits are the unit number. TU numbers follow CU numbers.
  auto write_names = [&](auto &units, i64 unit_base) {
    tbb::parallel_for_each(units, [&](auto &unit) {
      i64 i = unit_base + (&unit - units.data());
      for (NameRecord &record : unit.names) {
        IndexedName &name = record.indexed;
        MapValue *ent = name.entry;
        ul32 *p = (ul32 *)(base + ent->type_vector_offset);
        p[name.type_vector_idx] = (name.type << 24) | i;
      }
    });
  };
  write_names(cus, 0);
  write_names(tus, cus.size());

  // Prefix each type vector with its length and sort it for deterministic
  // output. Store the NUL-terminated name at its assigned string-pool offset.
  tbb::enumerable_thread_specific<std::vector<ul32>> scratch;
  tbb::parallel_for_each(entries, [&](Entry *ent) {
    ul32 *p = (ul32 *)(base + ent->value.type_vector_offset);
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
  auto release_names = [](auto &units) {
    tbb::parallel_for_each(
        units, [](auto &unit) { std::vector<NameRecord>().swap(unit.names); });
  };
  release_names(cus);
  release_names(tus);
  std::vector<GdbNameMap::Entry *>().swap(data.entries);
  data.map.reset();
}

// Read relocated address ranges and serialize the index prepared above.
template <typename E>
void write_gdb_index(Context<E> &ctx) {
  Timer t(ctx, "write_gdb_index");

  std::shared_ptr<GdbIndexData> owner = std::move(ctx.gdb_index_data);
  GdbIndexData &data = *owner;

  if (data.cus.empty() && data.tus.empty())
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
  std::vector<Typeunit> &tus = data.tus;

  read_address_ranges(ctx, cus);

  // Version 8 made symbol-table entries refer directly to type units, as ours
  // do. GDB 12 accepts version 8 but assumes that its type-unit list refers to
  // .debug_types and crashes on DWARF 5 type units in .debug_info. Version 9
  // makes it safely ignore such an index, while newer GDB versions can use it.
  // Keep version 7 for TU-free indices so older GDB versions can still use them.
  i64 header_size = tus.empty() ? sizeof(SectionHeaderV7) : sizeof(SectionHeaderV9);

  // Compute sizes of each component.
  i64 cu_list_offset = header_size;
  i64 cu_types_offset = cu_list_offset + cus.size() * 16;
  i64 ranges_offset = cu_types_offset + tus.size() * 24;

  i64 symtab_offset = ranges_offset;
  for (Compunit &cu : cus)
    symtab_offset += cu.ranges.size() * 20;

  i64 ht_size = data.ht_size;
  i64 shortcut_offset = symtab_offset + ht_size * 8;
  i64 const_pool_offset = shortcut_offset + (tus.empty() ? 0 : 8);

  i64 bufsize = const_pool_offset + data.pool_size.type_bytes +
                data.pool_size.name_bytes;

  u8 *buf = ctx.output_file->extend(ctx, bufsize);

  // Write a section header. A zero language marks the version 9 shortcut
  // table as containing no main-function information.
  if (tus.empty()) {
    SectionHeaderV7 &hdr = *(SectionHeaderV7 *)buf;
    hdr.version = 7;
    hdr.cu_list_offset = cu_list_offset;
    hdr.cu_types_offset = cu_types_offset;
    hdr.ranges_offset = ranges_offset;
    hdr.symtab_offset = symtab_offset;
    hdr.const_pool_offset = const_pool_offset;
  } else {
    SectionHeaderV9 &hdr = *(SectionHeaderV9 *)buf;
    hdr.version = 9;
    hdr.cu_list_offset = cu_list_offset;
    hdr.cu_types_offset = cu_types_offset;
    hdr.ranges_offset = ranges_offset;
    hdr.symtab_offset = symtab_offset;
    hdr.shortcut_offset = shortcut_offset;
    hdr.const_pool_offset = const_pool_offset;
    memset(buf + shortcut_offset, 0, 8);
  }

  // A CU-list entry is {.debug_info offset, unit size}.
  u8 *p = buf + cu_list_offset;

  for (Compunit &cu : cus) {
    *(ul64 *)p = cu.offset;
    *(ul64 *)(p + 8) = cu.size;
    p += 16;
  }

  // A TU-list entry is {.debug_info offset, unit-relative type DIE offset,
  // signature}. Unlike a CU-list entry, it does not contain the unit size.
  for (Typeunit &tu : tus) {
    *(ul64 *)p = tu.offset;
    *(ul64 *)(p + 8) = tu.type_die_offset;
    *(ul64 *)(p + 16) = tu.signature;
    p += 24;
  }

  // An address-area entry is {start address, end address, CU-list index}.
  std::vector<i64> range_offsets(cus.size());
  for (i64 i = 1; i < cus.size(); i++)
    range_offsets[i] = range_offsets[i - 1] + cus[i - 1].ranges.size() * 20;

  tbb::parallel_for_each(cus, [&](Compunit &cu) {
    i64 i = &cu - cus.data();
    u8 *p = buf + ranges_offset + range_offsets[i];
    for (std::pair<u64, u64> range : cu.ranges) {
      *(ul64 *)p = range.first;
      *(ul64 *)(p + 8) = range.second;
      *(ul32 *)(p + 16) = i;
      p += 20;
    }
  });

  i64 pool_size = data.pool_size.type_bytes + data.pool_size.name_bytes;
  parallel_memcpy(buf + symtab_offset, data.tables.get(), ht_size * 8);
  parallel_memcpy(buf + const_pool_offset, data.tables.get() + ht_size * 8, pool_size);

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
