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
    if (*(ul32 *)data.data() == 0xffffffff)
      Fatal(ctx) << *file.debug_info << ": --gdb-index: DWARF64 not supported";
    i64 len = *(ul32 *)data.data() + 4;
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

      u32 len = *(ul32 *)contents.data() + 4;
      u32 debug_info_offset = *(ul32 *)(contents.data() + 6);
      u32 cu_idx = get_cu_idx(isec, debug_info_offset);

      std::string_view data = contents.substr(14, len - 14);
      contents = contents.substr(len);

      while (!data.empty()) {
        u32 offset = *(ul32 *)data.data();
        data = data.substr(4);
        if (offset == 0)
          break;

        u8 type = data[0];
        data = data.substr(1);

        std::string_view name = data.data();
        data = data.substr(name.size() + 1);

        vec.push_back({name, gdb_hash(name), (type << 24) | cu_idx});
      }
    }
  };

  if (file.debug_pubnames)
    read(*file.debug_pubnames);
  if (file.debug_pubtypes)
    read(*file.debug_pubtypes);

  // Uniquify elements because GCC 11 seems to emit one record for each
  // comdat group which results in having a lot of duplicate records.
  auto less = [](const GdbIndexName &a, const GdbIndexName &b) {
    return std::tuple{a.hash, a.attr, a.name} <
           std::tuple{b.hash, b.attr, b.name};
  };

  auto equal = [](const GdbIndexName &a, const GdbIndexName &b) {
    return std::tuple{a.hash, a.attr, a.name} ==
           std::tuple{b.hash, b.attr, b.name};
  };

  std::sort(vec.begin(), vec.end(), less);
  vec.erase(std::unique(vec.begin(), vec.end(), equal), vec.end());
  return vec;
}

template <typename E>
static u8 *get_buffer(Context<E> &ctx, Chunk<E> *chunk) {
  if (u8 *buf = chunk->get_uncompressed_data())
    return buf;
  return ctx.buf + chunk->shdr.sh_offset;
}

// Try to find a compilation unit from .debug_info and its
// corresponding record from .debug_abbrev and returns them.
template <typename E>
static std::tuple<u8 *, u8 *, u32>
find_compunit(Context<E> &ctx, ObjectFile<E> &file, i64 offset) {
  // Read .debug_info to find the record at a given offset.
  u8 *cu = get_buffer(ctx, ctx.debug_info) + offset;
  u32 dwarf_version = *(ul16 *)(cu + 4);
  u32 abbrev_offset;

  // Skip a header.
  switch (dwarf_version) {
  case 2:
  case 3:
  case 4:
    abbrev_offset = *(ul32 *)(cu + 6);
    if (u32 address_size = cu[10]; address_size != E::word_size)
      Fatal(ctx) << file << ": --gdb-index: unsupported address size "
                 << address_size;
    cu += 11;
    break;
  case 5: {
    abbrev_offset = *(ul32 *)(cu + 8);
    if (u32 address_size = cu[7]; address_size != E::word_size)
      Fatal(ctx) << file << ": --gdb-index: unsupported address size "
                 << address_size;

    switch (u32 unit_type = cu[6]; unit_type) {
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
  u8 *abbrev = get_buffer(ctx, ctx.debug_abbrev) + abbrev_offset;

  for (;;) {
    u32 code = read_uleb(abbrev);
    if (code == 0)
      Fatal(ctx) << file << ": --gdb-index: .debug_abbrev does not contain"
                 << " a record for the first .debug_info record";

    if (code == abbrev_code) {
      // Found a record
      u64 abbrev_tag = read_uleb(abbrev);
      if (abbrev_tag != DW_TAG_compile_unit && abbrev_tag != DW_TAG_skeleton_unit)
        Fatal(ctx) << file << ": --gdb-index: the first entry's tag is not"
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

  // In DWARF 4, a CU can refer address ranges in .debug_ranges.
  // .debug_ranges contains a vector of [begin, end) address pairs.
  // The last entry must be a null terminator, so we do -1.
  if (file.debug_ranges)
    ret += file.debug_ranges->sh_size / E::word_size / 2 - 1;

  // In DWARF 5, a CU can refer address ranges in .debug_rnglists, which
  // contains variable-length entries. The smallest possible range entry
  // is one byte for the code and two ULEB128 values (each can be as
  // small as one byte), so 3 bytes.
  if (file.debug_rnglists)
    ret += file.debug_rnglists->sh_size / 3;
  return ret;
}

// .debug_info contains variable-length fields. This class reads them.
template <typename E>
class DebugInfoReader {
public:
  DebugInfoReader(Context<E> &ctx, ObjectFile<E> &file, u8 *cu)
    : ctx(ctx), file(file), cu(cu) {}

  u64 read(u64 form);

  Context<E> &ctx;
  ObjectFile<E> &file;
  u8 *cu;
};

// Read value of the given DW_FORM_* form. If a value is not scalar,
// returns a dummy value 0.
template <typename E>
inline u64 DebugInfoReader<E>::read(u64 form) {
  switch (form) {
  case DW_FORM_flag_present:
    return 0;
  case DW_FORM_data1:
  case DW_FORM_flag:
  case DW_FORM_strx1:
  case DW_FORM_addrx1:
  case DW_FORM_ref1:
    return *cu++;
  case DW_FORM_data2:
  case DW_FORM_strx2:
  case DW_FORM_addrx2:
  case DW_FORM_ref2: {
    u64 val = *(ul16 *)cu;
    cu += 2;
    return val;
  }
  case DW_FORM_strx3:
  case DW_FORM_addrx3: {
    u64 val = *(ul24 *)cu;
    cu += 3;
    return val;
  }
  case DW_FORM_data4:
  case DW_FORM_strp:
  case DW_FORM_sec_offset:
  case DW_FORM_line_strp:
  case DW_FORM_strx4:
  case DW_FORM_addrx4:
  case DW_FORM_ref4: {
    u64 val = *(ul32 *)cu;
    cu += 4;
    return val;
  }
  case DW_FORM_data8:
  case DW_FORM_ref8: {
    u64 val = *(ul64 *)cu;
    cu += 8;
    return val;
  }
  case DW_FORM_addr:
  case DW_FORM_ref_addr: {
    u64 val = *(typename E::WordTy *)cu;
    cu += E::word_size;
    return val;
  }
  case DW_FORM_strx:
  case DW_FORM_addrx:
  case DW_FORM_udata:
  case DW_FORM_ref_udata:
  case DW_FORM_loclistx:
  case DW_FORM_rnglistx:
    return read_uleb(cu);
  case DW_FORM_string:
    cu += strlen((char *)cu) + 1;
    return 0;
  default:
    Fatal(ctx) << file << ": --gdb-index: unhandled debug info form: 0x"
               << std::hex << form;
    return 0;
  }
}

// Read a range list from .debug_ranges starting at the given offset
// (until an end of list entry).
template <typename E>
static std::vector<u64>
read_debug_range(Context<E> &ctx, ObjectFile<E> &file, typename E::WordTy *range) {
  std::vector<u64> vec;
  u64 base = 0;

  for (i64 i = 0; range[i] || range[i + 1]; i += 2) {
    if (range[i] + 1 == 0) {
      // base address selection entry
      base = range[i + 1];
    } else {
      vec.push_back(range[i] + base);
      vec.push_back(range[i + 1] + base);
    }
  }
  return vec;
}

// Read a range list from .debug_rnglists starting at the given offset
// (until an end of list entry).
template <typename E>
static std::vector<u64>
read_rnglist_range(Context<E> &ctx, ObjectFile<E> &file, u8 *rnglist,
                   typename E::WordTy *addrx) {
  std::vector<u64> vec;
  u64 base = 0;

  for (;;) {
    switch (*rnglist++) {
    case DW_RLE_end_of_list:
      return vec;
    case DW_RLE_base_addressx:
      base = addrx[read_uleb(rnglist)];
      break;
    case DW_RLE_startx_endx:
      vec.push_back(addrx[read_uleb(rnglist)]);
      vec.push_back(addrx[read_uleb(rnglist)]);
      break;
    case DW_RLE_startx_length:
      vec.push_back(addrx[read_uleb(rnglist)]);
      vec.push_back(vec.back() + read_uleb(rnglist));
      break;
    case DW_RLE_offset_pair:
      vec.push_back(base + read_uleb(rnglist));
      vec.push_back(base + read_uleb(rnglist));
      break;
    case DW_RLE_base_address:
      base = *(typename E::WordTy *)rnglist;
      rnglist += E::word_size;
      break;
    case DW_RLE_start_end:
      vec.push_back(*(typename E::WordTy *)rnglist);
      rnglist += E::word_size;
      vec.push_back(*(typename E::WordTy *)rnglist);
      rnglist += E::word_size;
      break;
    case DW_RLE_start_length:
      vec.push_back(*(typename E::WordTy *)rnglist);
      rnglist += E::word_size;
      vec.push_back(vec.back() + read_uleb(rnglist));
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

  struct Record {
    u64 form = 0;
    u64 value = 0;
  };

  Record low_pc;
  Record high_pc;
  Record ranges;
  std::optional<u64> rnglists_base;
  typename E::WordTy *addrx = nullptr;

  // Read all interesting debug records.
  for (;;) {
    u64 name = read_uleb(abbrev);
    u64 form = read_uleb(abbrev);
    if (name == 0 && form == 0)
      break;

    u64 val = reader.read(form);

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
      addrx = (typename E::WordTy *)(get_buffer(ctx, ctx.debug_addr) + val);
      break;
    case DW_AT_ranges:
      ranges = {form, val};
      break;
    }
  }

  // Handle non-contiguous address ranges.
  if (ranges.form) {
    if (dwarf_version <= 4) {
       typename E::WordTy *range_begin =
        (typename E::WordTy *)(get_buffer(ctx, ctx.debug_ranges) + ranges.value);
      return read_debug_range(ctx, file, range_begin);
    }

    assert(dwarf_version == 5);

    u8 *buf = get_buffer(ctx, ctx.debug_rnglists);
    if (ranges.form == DW_FORM_sec_offset)
      return read_rnglist_range(ctx, file, buf + ranges.value, addrx);

    if (!rnglists_base)
      Fatal(ctx) << file << ": --gdb-index: missing DW_AT_rnglists_base";

    u8 *base = buf + *rnglists_base;
    return read_rnglist_range(ctx, file, base + *(ul32 *)base, addrx);
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
      Fatal(ctx) << file << ": --gdb-index: unhandled form for DW_AT_low_pc: 0x"
                 << std::hex << high_pc.form;
    }

    switch (high_pc.form) {
    case DW_FORM_addr:
      return {lo, high_pc.value};
    case DW_FORM_addrx:
    case DW_FORM_addrx1:
    case DW_FORM_addrx2:
    case DW_FORM_addrx4:
      return {lo, addrx[high_pc.value]};
    case DW_FORM_udata:
    case DW_FORM_data1:
    case DW_FORM_data2:
    case DW_FORM_data4:
    case DW_FORM_data8:
      return {lo, lo + high_pc.value};
    default:
      Fatal(ctx) << file << ": --gdb-index: unhandled form for DW_AT_high_pc: 0x"
                 << std::hex << high_pc.form;
    }
  }

  return {};
}

// Read the DWARFv2 file and directory info from .debug_line (i.e. from the include_directories
// and file_names fields in the header).
static
std::pair<std::string_view, std::string_view>
read_line_file_v2(const u8 *file_data, const u8 *end, u32 file) {
  const u8 *data = file_data;
  // First skip include_directories to read file and find out which directory is needed
  // (include_directories ends with an empty item containing only null).
  for (;;) {
    if (*data++ == '\0')
      break;
    data = (u8 *)memchr(data, '\0', end - data);
    if (data == nullptr || end - data < 2)
      return {};
    ++data;
  }

  // Skip file entries before the one we want.
  for (int i = 1; i < file; ++i) {
    data = (u8 *)memchr(data, '\0', end - data);
    if (data == nullptr)
      return {};
    ++data;
    read_uleb(data);
    read_uleb(data);
    read_uleb(data);
    if (*data == '\0')
      return {};
  }

  std::string_view name((const char*)data);
  data += name.size() + 1;
  u32 directory_index = read_uleb(data);
  std::string_view directory;
  if (directory_index > 0) {
    data = file_data;
    // Skip directory entries before the one we want.
    for (int i = 1; i < directory_index; ++i) {
      data = (u8 *)memchr(data, '\0', end - data);
      if (data == nullptr)
        return {};
      ++data;
      if (*data == '\0')
        return {};
    }
    directory = std::string_view((const char*)data);
  }
  return {name, directory};
}

// Process .debug_line for the given compilation unit and find the source location
// for the given address.
// The .debug_line section is instructions for a state machine that builds a list
// of addresses and their source information.
template <typename E>
static
std::tuple<std::string_view, std::string_view, int, int>
find_source_location_cu(Context<E> &ctx, ObjectFile<E> &object_file, i64 offset, u64 addr) {
  const u8 *data = get_buffer(ctx, ctx.debug_line) + offset;

  ul32 len = *(ul32 *)data;
  if (len == 0xffffffff)
    return {}; // DWARF64
  data += 4;
  const u8 *end = data + len;

  u32 dwarf_version = *(ul16 *)data;
  if (dwarf_version < 2 || dwarf_version > 5)
    return {}; // unknown DWARF version
  data += 2;

  if (dwarf_version == 5) {
    if (u32 address_size = *data; address_size != E::word_size)
      return {}; // unsupported address size
    data += 2;
  }
  u32 header_length = *(ul32 *)data;
  if (header_length == 0xffffffff)
    return {}; // DWARF64
  data += 4;
  const u8 *data_after_header = data + header_length;
  u8 minimum_instruction_length = *data++;
  u8 maximum_operations_per_instruction = 1;
  if (dwarf_version >= 4)
    maximum_operations_per_instruction = *data++;
  ++data; // default_is_stmt
  i8 line_base = *(i8 *)data++;
  u8 line_range = *data++;
  u8 opcode_base = *data++;
  std::span<const u8> standard_opcode_lengths = std::span(data, opcode_base - 1);
  data += opcode_base - 1;
  const u8 *file_data = data;
  data = data_after_header;

  // This is a partially interpreter for the .debug_line instructions for the state
  // machine (DWARF spec section 6.2). We only care about the address, file, line
  // and column data (and additionally op_index, since that one is needed for address).
  u64 address = 0;
  u32 op_index = 0;
  u32 file = 1;
  u32 line = 1;
  u32 column = 0;
  u64 last_address;
  u32 last_file;
  u32 last_line;
  u32 last_column;
  bool last_valid = false;

  auto advance = [&](i32 operation_advance) {
    address += minimum_instruction_length * ((op_index + operation_advance)
      / maximum_operations_per_instruction);
    op_index = (op_index + operation_advance) % maximum_operations_per_instruction;
  };
  auto advance_opcode = [&](u8 opcode) {
    i32 adjusted_opcode = opcode - (i16)opcode_base;
    line += line_base + (adjusted_opcode % line_range);
    i32 operation_advance = adjusted_opcode / line_range;
    return advance(operation_advance);
  };

  while (data < end) {
    bool check_address = false;
    bool end_sequence = false;
    u8 opcode = *data;
    ++data;
    if (opcode < opcode_base) {
      // standard opcodes (including extended opcodes)
      switch (opcode) {
      case DW_LNS_copy:
        check_address = true;
        break;
      case DW_LNS_advance_pc:
        advance(read_uleb(data));
        check_address = true;
        break;
      case DW_LNS_advance_line:
        line += read_sleb(data);
        break;
      case DW_LNS_set_file:
        file = read_uleb(data);
        break;
      case DW_LNS_set_column:
        column = read_uleb(data);
        break;
      case DW_LNS_const_add_pc:
        advance_opcode(255);
        break;
      case DW_LNS_fixed_advance_pc:
        address += *(ul16*)data;
        data += 2;
        op_index = 0;
        break;
      case 0: {
        // extended opcodes
        u32 bytes = read_uleb(data);
        u8 extended_opcode = *data;
        ++data;
        switch (extended_opcode) {
          case DW_LNE_end_sequence:
            check_address = true;
            end_sequence = true;
            break;
          case DW_LNE_set_address:
            address = *(typename E::WordTy *)data;
            data += E::word_size;
            op_index = 0;
            break;
          case DW_LNE_set_discriminator:
            read_uleb(data);
            break;
          case DW_LNE_define_file:
            return {}; // deprecated
          default:
            data += bytes;
            break;
        }
        break;
      }
      default:
        // All the unhandled standard opcodes, including unknown (vendor
        // extensions), skip their arguments.
        for (u8 i = 0; i < standard_opcode_lengths[opcode - 1]; ++i)
            read_uleb(data);
        break;
      }
    } else {
      // special opcodes
      advance_opcode(opcode);
      check_address = true;
    }

    if (check_address) {
      // Check since the last (valid) address until before the current one.
      // If found, the last location is the location of the asked for address.
      if (last_valid && addr >= last_address && addr < address) {
        std::string_view filename;
        std::string_view directory;
        if (dwarf_version <= 4)
          std::tie(filename, directory) = read_line_file_v2(file_data, data_after_header, file);
        else
          return {}; // TODO
        if (filename.empty())
          return {};
        return {filename, directory, last_line, last_column};
      }
      last_address = address;
      last_file = file;
      last_line = line;
      last_column = column;
      last_valid = true;
    }
    if (end_sequence) {
      address = 0;
      op_index = 0;
      file = 1;
      line = 1;
      column = 0;
      end_sequence = false;
    }
  }

  return {};
}

// Return filename, line and column as source location for the address
// in the given object file, by finding it in .debug_line .
//
// It is necessary to find find the compilation unit for the given address,
// and then process the relevant part of .debug_line for that unit.
template <typename E>
std::tuple<std::string_view, std::string_view, int, int>
find_source_location(Context<E> &ctx, ObjectFile<E> &file, u64 address) {
  if (!file.debug_info)
    return {};

  // Find the compilation unit that contains the given address.
  u64 offset = file.debug_info->offset;

  for (i64 i = 0; i < file.compunits.size(); i++) {
    std::vector<u64> addrs = read_address_areas(ctx, file, offset);
    for (i64 j = 0; j < addrs.size(); j += 2) {
      if (address >= addrs[j] && address < addrs[j + 1]) {
        return find_source_location_cu(ctx, file, offset, address);
      }
      offset += file.compunits[i].size();
    }
  }

  return {};
}

template <typename E>
void setup_context_debuginfo(Context<E> &ctx) {
  for (Chunk<E> *chunk : ctx.chunks) {
    std::string_view name = chunk->name;
    if (name == ".debug_info" || name == ".zdebug_info")
      ctx.debug_info = chunk;
    if (name == ".debug_abbrev" || name == ".zdebug_abbrev")
      ctx.debug_abbrev = chunk;
    if (name == ".debug_ranges" || name == ".zdebug_ranges")
      ctx.debug_ranges = chunk;
    if (name == ".debug_addr" || name == ".zdebug_addr")
      ctx.debug_addr = chunk;
    if (name == ".debug_rnglists" || name == ".zdebug_rnglists")
      ctx.debug_rnglists = chunk;
    if (name == ".debug_line" || name == ".zdebug_line")
      ctx.debug_line = chunk;
  }
}

#define INSTANTIATE(E)                                                  \
  template std::vector<std::string_view>                                \
  read_compunits(Context<E> &, ObjectFile<E> &);                        \
  template std::vector<GdbIndexName>                                    \
  read_pubnames(Context<E> &, ObjectFile<E> &);                         \
  template i64                                                          \
  estimate_address_areas(Context<E> &, ObjectFile<E> &);                \
  template std::vector<u64>                                             \
  read_address_areas(Context<E> &, ObjectFile<E> &, i64);               \
  template std::tuple<std::string_view, std::string_view, int, int>     \
  find_source_location(Context<E> &ctx, ObjectFile<E> &file, u64 address); \
  template void                                                         \
  setup_context_debuginfo(Context<E> &ctx)

INSTANTIATE_ALL;

} // namespace mold::elf
